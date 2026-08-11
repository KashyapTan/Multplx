//! ANSI-aware ghost extraction and safe composer-content classification from
//! `bin/mx-composer-lib.sh`.

use regex::RegexBuilder;

use crate::error::{CoreError, Result};

/// The shared empty, pending, or unsafe composer verdict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerState {
    /// Safe empty agent composer.
    Empty,
    /// Real unsubmitted content exists.
    Pending,
    /// The row is unreadable or may be a dead shell.
    Unknown,
}

impl ComposerState {
    /// Return the exact legacy token.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Pending => "pending",
            Self::Unknown => "unknown",
        }
    }
}

fn csi_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&0x1b) || bytes.get(start + 1) != Some(&b'[') {
        return None;
    }
    let mut index = start + 2;
    while let Some(byte) = bytes.get(index) {
        if (0x40..=0x7e).contains(byte) {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// Strip CSI sequences for structural row inspection.
#[must_use]
pub fn strip_ansi(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if let Some(end) = csi_end(input, index) {
            index = end + 1;
        } else if input[index] == 0x1b {
            index += 1;
        } else {
            output.push(input[index]);
            index += 1;
        }
    }
    output
}

fn colon_truecolor_dark(parameter: &str, luma_max: u16) -> Option<bool> {
    let fields: Vec<&str> = parameter.split(':').collect();
    if fields.first() != Some(&"38") || fields.get(1) != Some(&"2") || fields.len() < 5 {
        return None;
    }
    let red = fields.get(fields.len() - 3)?.parse::<u16>().ok()?;
    let green = fields.get(fields.len() - 2)?.parse::<u16>().ok()?;
    let blue = fields.last()?.parse::<u16>().ok()?;
    Some((299 * red + 587 * green + 114 * blue) / 1000 < luma_max)
}

fn update_sgr(parameters: &str, dim: &mut bool, dark: &mut bool, luma_max: u16) {
    let parameters = if parameters.is_empty() {
        "0"
    } else {
        parameters
    };
    let fields: Vec<&str> = parameters.split(';').collect();
    let mut index = 0;
    while index < fields.len() {
        let field = fields[index];
        let code = field.split(':').next().unwrap_or("0");
        match code {
            "0" => {
                *dim = false;
                *dark = false;
            }
            "2" => *dim = true,
            "22" => *dim = false,
            "39" => *dark = false,
            _ if code
                .parse::<u8>()
                .is_ok_and(|code| (30..=37).contains(&code) || (90..=97).contains(&code)) =>
            {
                *dark = false;
            }
            "38" => {
                if let Some(colon_dark) = colon_truecolor_dark(field, luma_max) {
                    *dark = colon_dark;
                } else if fields.get(index + 1) == Some(&"2") && index + 4 < fields.len() {
                    let components = (
                        fields[index + 2].parse::<u16>(),
                        fields[index + 3].parse::<u16>(),
                        fields[index + 4].parse::<u16>(),
                    );
                    if let (Ok(red), Ok(green), Ok(blue)) = components {
                        *dark = (299 * red + 587 * green + 114 * blue) / 1000 < luma_max;
                    }
                    index += 4;
                } else if fields.get(index + 1) == Some(&"5") {
                    index += 2;
                }
            }
            "48" | "58" => {
                if field.contains(':') {
                    // The payload is carried in this parameter.
                } else if fields.get(index + 1) == Some(&"2") {
                    index = (index + 4).min(fields.len() - 1);
                } else if fields.get(index + 1) == Some(&"5") {
                    index = (index + 2).min(fields.len() - 1);
                } else {
                    index = (index + 1).min(fields.len() - 1);
                }
            }
            _ => {}
        }
        index += 1;
    }
}

/// Keep only normal-intensity, non-dark foreground bytes from a styled row.
#[must_use]
pub fn strip_ghost(input: &[u8], luma_max: u16) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    let mut dim = false;
    let mut dark = false;
    while index < input.len() {
        if let Some(end) = csi_end(input, index) {
            if input[end] == b'm' {
                let parameters = std::str::from_utf8(&input[index + 2..end]).unwrap_or("");
                update_sgr(parameters, &mut dim, &mut dark, luma_max);
            }
            index = end + 1;
        } else if input[index] == 0x1b {
            index += 1;
        } else {
            if !dim && !dark {
                output.push(input[index]);
            }
            index += 1;
        }
    }
    output
}

fn idle_matches(content: &str, pattern: Option<&str>, insensitive: bool) -> Result<bool> {
    let Some(pattern) = pattern.filter(|pattern| !pattern.is_empty()) else {
        return Ok(false);
    };
    RegexBuilder::new(pattern)
        .case_insensitive(insensitive)
        .build()
        .map(|regex| regex.is_match(content))
        .map_err(|_| CoreError::MalformedRecord {
            kind: "composer idle regex",
            reason: "invalid regular expression",
        })
}

fn strip_prompt(content: &str) -> &str {
    const GLYPHS: [char; 7] = ['❯', '›', '→', '>', '$', '%', '#'];
    let Some(first) = content.chars().next() else {
        return content;
    };
    if !GLYPHS.contains(&first) {
        return content;
    }
    content[first.len_utf8()..]
        .strip_prefix(' ')
        .unwrap_or(&content[first.len_utf8()..])
}

/// Classify already-extracted composer content with the dead-shell safeguard.
pub fn classify_content(
    bordered: bool,
    content: &str,
    idle_pattern: Option<&str>,
    insensitive: bool,
    plain_content: Option<&str>,
) -> Result<ComposerState> {
    let plain = plain_content.unwrap_or(content);
    if !bordered && content.is_empty() && !plain.is_empty() {
        return Ok(if matches!(plain, "❯" | "›" | "→") {
            ComposerState::Empty
        } else {
            ComposerState::Unknown
        });
    }
    match content {
        "❯" | "›" | "→" => return Ok(ComposerState::Empty),
        ">" | "$" | "%" | "#" => {
            return Ok(if bordered {
                ComposerState::Empty
            } else {
                ComposerState::Unknown
            });
        }
        "" => return Ok(ComposerState::Empty),
        _ => {}
    }
    if idle_matches(content, idle_pattern, insensitive)? {
        return Ok(ComposerState::Empty);
    }
    let remainder = strip_prompt(content).trim();
    if remainder.is_empty() || idle_matches(remainder, idle_pattern, insensitive)? {
        return Ok(ComposerState::Empty);
    }
    Ok(ComposerState::Pending)
}

#[cfg(test)]
mod tests {
    use super::{ComposerState, classify_content, strip_ansi, strip_ghost};

    #[test]
    fn bare_shell_prompts_fail_closed() {
        for glyph in [">", "$", "%", "#"] {
            assert_eq!(
                classify_content(false, glyph, None, false, None).expect("classification"),
                ComposerState::Unknown
            );
            assert_eq!(
                classify_content(true, glyph, None, false, None).expect("classification"),
                ComposerState::Empty
            );
        }
    }

    #[test]
    fn composer_state_tokens_are_exact() {
        assert_eq!(ComposerState::Empty.as_str(), "empty");
        assert_eq!(ComposerState::Pending.as_str(), "pending");
        assert_eq!(ComposerState::Unknown.as_str(), "unknown");
    }

    #[test]
    fn ghost_extraction_preserves_real_text() {
        let styled = b"\x1b[31mreal\x1b[0m \x1b[2mghost\x1b[0m";
        assert_eq!(strip_ansi(styled), b"real ghost");
        assert_eq!(strip_ghost(styled, 128), b"real ");
        assert_eq!(
            strip_ghost(b"real \x1b[38;2;50;47;70mplaceholder\x1b[0m", 128),
            b"real "
        );
    }

    #[test]
    fn idle_regex_case_mode_is_explicit() {
        let pattern = Some(r"^Type a message\.\.\.$");
        assert_eq!(
            classify_content(true, "type a message...", pattern, false, None)
                .expect("classification"),
            ComposerState::Pending
        );
        assert_eq!(
            classify_content(true, "type a message...", pattern, true, None)
                .expect("classification"),
            ComposerState::Empty
        );
    }
}
