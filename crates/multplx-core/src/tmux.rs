//! Shared tmux composer and verified-submit primitives from `bin/mx-tmux-lib.sh`.

use std::process::Command;
use std::time::Duration;

use regex::RegexBuilder;

use crate::composer::{ComposerState, classify_content, strip_ansi, strip_ghost};
use crate::error::{CoreError, Result};

/// Default shared busy-footer pattern.
pub const BUSY_REGEX_DEFAULT: &str = r"esc (to )?interrupt|Working(\.\.\.)?|ctrl\+c to stop";

/// Shared composer-reading policy.
#[derive(Clone, Copy, Debug)]
pub struct ComposerPolicy<'a> {
    /// Optional harness-specific idle placeholder.
    pub idle_pattern: Option<&'a str>,
    /// Shared or overridden busy-footer regex.
    pub busy_pattern: &'a str,
    /// Maximum truecolor luminance treated as de-emphasized ghost text.
    pub luma_max: u16,
}

impl Default for ComposerPolicy<'static> {
    fn default() -> Self {
        Self {
            idle_pattern: None,
            busy_pattern: BUSY_REGEX_DEFAULT,
            luma_max: 128,
        }
    }
}

/// Submit retry and settle timing.
#[derive(Clone, Copy, Debug)]
pub struct SubmitPolicy {
    /// Maximum Enter attempts, with zero still meaning one initial attempt.
    pub retries: usize,
    /// Delay after each Enter before reading the composer.
    pub retry_delay: Duration,
    /// Delay after literal typing before the first Enter.
    pub settle: Duration,
}

/// Injectable tmux transport.
pub trait TmuxTransport {
    /// Read numeric cursor row.
    fn cursor_y(&mut self, target: &str) -> Result<u32>;
    /// Read exactly one styled row.
    fn styled_row(&mut self, target: &str, row: u32) -> Result<Vec<u8>>;
    /// Read the last 40 plain rows.
    fn tail(&mut self, target: &str) -> Result<String>;
    /// Send literal content once.
    fn send_literal(&mut self, target: &str, text: &str) -> Result<()>;
    /// Send one Enter key.
    fn send_enter(&mut self, target: &str) -> Result<()>;
}

/// Injectable settle and retry delay.
pub trait Sleeper {
    /// Wait the requested duration.
    fn sleep(&mut self, duration: Duration);
}

/// Real thread sleeper.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Argument-array tmux command transport with bounded capture output.
#[derive(Clone, Debug)]
pub struct SystemTmux {
    executable: String,
    output_limit: usize,
}

impl Default for SystemTmux {
    fn default() -> Self {
        Self {
            executable: "tmux".to_owned(),
            output_limit: 256 * 1024,
        }
    }
}

impl SystemTmux {
    fn output(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = Command::new(&self.executable)
            .args(args)
            .output()
            .map_err(|error| CoreError::Command {
                command: self.executable.clone(),
                reason: error.to_string(),
            })?;
        if !output.status.success() || output.stdout.len() > self.output_limit {
            return Err(CoreError::Command {
                command: self.executable.clone(),
                reason: format!("exit {:?} or oversized output", output.status.code()),
            });
        }
        Ok(output.stdout)
    }

    fn status(&self, args: &[&str]) -> Result<()> {
        let status = Command::new(&self.executable)
            .args(args)
            .status()
            .map_err(|error| CoreError::Command {
                command: self.executable.clone(),
                reason: error.to_string(),
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(CoreError::Command {
                command: self.executable.clone(),
                reason: format!("exit {:?}", status.code()),
            })
        }
    }
}

impl TmuxTransport for SystemTmux {
    fn cursor_y(&mut self, target: &str) -> Result<u32> {
        let bytes = self.output(&["display-message", "-p", "-t", target, "#{cursor_y}"])?;
        String::from_utf8(bytes)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok())
            .ok_or(CoreError::MalformedRecord {
                kind: "tmux cursor row",
                reason: "cursor row is not numeric",
            })
    }

    fn styled_row(&mut self, target: &str, row: u32) -> Result<Vec<u8>> {
        let row = row.to_string();
        self.output(&[
            "capture-pane",
            "-e",
            "-p",
            "-t",
            target,
            "-S",
            &row,
            "-E",
            &row,
        ])
    }

    fn tail(&mut self, target: &str) -> Result<String> {
        String::from_utf8(self.output(&["capture-pane", "-p", "-t", target, "-S", "-40"])?).map_err(
            |_| CoreError::MalformedRecord {
                kind: "tmux tail",
                reason: "tail is not UTF-8",
            },
        )
    }

    fn send_literal(&mut self, target: &str, text: &str) -> Result<()> {
        self.status(&["send-keys", "-t", target, "-l", text])
    }

    fn send_enter(&mut self, target: &str) -> Result<()> {
        self.status(&["send-keys", "-t", target, "Enter"])
    }
}

fn trim_row(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).trim().to_owned()
}

fn strip_matching_border(content: &str) -> (bool, &str) {
    for border in ['│', '┃', '|'] {
        if content.starts_with(border)
            && content.ends_with(border)
            && content.len() >= 2 * border.len_utf8()
        {
            let inner = &content[border.len_utf8()..content.len() - border.len_utf8()];
            return (true, inner.trim());
        }
    }
    (false, content)
}

/// Classify one styled cursor row using the shared composer owner.
pub fn classify_row(
    styled: &[u8],
    idle_pattern: Option<&str>,
    busy_pattern: &str,
    luma_max: u16,
) -> Result<ComposerState> {
    let plain = trim_row(strip_ansi(styled));
    let (bordered, _) = strip_matching_border(&plain);
    let real = trim_row(strip_ghost(styled, luma_max));
    let (_, stripped) = strip_matching_border(&real);
    let busy = RegexBuilder::new(busy_pattern)
        .case_insensitive(true)
        .build()
        .map_err(|_| CoreError::MalformedRecord {
            kind: "tmux busy regex",
            reason: "invalid regular expression",
        })?;
    if !stripped.is_empty() && busy.is_match(stripped) {
        return Ok(ComposerState::Empty);
    }
    classify_content(bordered, stripped, idle_pattern, true, Some(&plain))
}

/// Read and classify the current composer, returning unknown on transport failure.
pub fn composer_state(
    tmux: &mut impl TmuxTransport,
    target: &str,
    idle_pattern: Option<&str>,
    busy_pattern: &str,
    luma_max: u16,
) -> ComposerState {
    tmux.cursor_y(target)
        .and_then(|row| tmux.styled_row(target, row))
        .and_then(|styled| classify_row(&styled, idle_pattern, busy_pattern, luma_max))
        .unwrap_or(ComposerState::Unknown)
}

/// Return whether the last six nonblank rows of a 40-row tail match busy text.
pub fn pane_is_busy(tmux: &mut impl TmuxTransport, target: &str, pattern: &str) -> bool {
    let Ok(tail) = tmux.tail(target) else {
        return false;
    };
    let Ok(regex) = RegexBuilder::new(pattern).case_insensitive(true).build() else {
        return false;
    };
    let rows: Vec<&str> = tail
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    rows.iter().rev().take(6).any(|line| regex.is_match(line))
}

/// Send Enter up to `retries` times, then accept a busy queued submit.
pub fn submit_enter_core(
    tmux: &mut impl TmuxTransport,
    sleeper: &mut impl Sleeper,
    target: &str,
    submit: SubmitPolicy,
    composer: ComposerPolicy<'_>,
) -> ComposerState {
    let attempts = submit.retries.max(1);
    for _ in 0..attempts {
        let _ = tmux.send_enter(target);
        sleeper.sleep(submit.retry_delay);
        let state = composer_state(
            tmux,
            target,
            composer.idle_pattern,
            composer.busy_pattern,
            composer.luma_max,
        );
        if state != ComposerState::Pending {
            return state;
        }
    }
    if pane_is_busy(tmux, target, composer.busy_pattern) {
        ComposerState::Empty
    } else {
        ComposerState::Pending
    }
}

/// Type once, settle, and verify submit without ever retyping content.
pub fn submit_core(
    tmux: &mut impl TmuxTransport,
    sleeper: &mut impl Sleeper,
    target: &str,
    text: &str,
    submit: SubmitPolicy,
    composer: ComposerPolicy<'_>,
) -> Result<ComposerState> {
    tmux.send_literal(target, text)?;
    sleeper.sleep(submit.settle);
    Ok(submit_enter_core(tmux, sleeper, target, submit, composer))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use super::{
        ComposerPolicy, Sleeper, SubmitPolicy, SystemTmux, ThreadSleeper, TmuxTransport,
        classify_row, composer_state, pane_is_busy, submit_core, submit_enter_core,
    };
    use crate::composer::ComposerState;
    use crate::error::{CoreError, Result};

    #[derive(Default)]
    struct FakeTmux {
        states: VecDeque<Vec<u8>>,
        tail: String,
        enters: usize,
        literals: Vec<String>,
        cursor_error: bool,
        styled_error: bool,
        tail_error: bool,
        literal_error: bool,
    }

    fn fixture_error() -> CoreError {
        CoreError::Command {
            command: "fixture".to_owned(),
            reason: "requested failure".to_owned(),
        }
    }

    impl TmuxTransport for FakeTmux {
        fn cursor_y(&mut self, _target: &str) -> Result<u32> {
            if self.cursor_error {
                Err(fixture_error())
            } else {
                Ok(0)
            }
        }

        fn styled_row(&mut self, _target: &str, _row: u32) -> Result<Vec<u8>> {
            if self.styled_error {
                Err(fixture_error())
            } else {
                Ok(self
                    .states
                    .pop_front()
                    .unwrap_or_else(|| b"pending".to_vec()))
            }
        }

        fn tail(&mut self, _target: &str) -> Result<String> {
            if self.tail_error {
                Err(fixture_error())
            } else {
                Ok(self.tail.clone())
            }
        }

        fn send_literal(&mut self, _target: &str, text: &str) -> Result<()> {
            if self.literal_error {
                Err(fixture_error())
            } else {
                self.literals.push(text.to_owned());
                Ok(())
            }
        }

        fn send_enter(&mut self, _target: &str) -> Result<()> {
            self.enters += 1;
            Ok(())
        }
    }

    #[derive(Default)]
    struct NoSleep(Vec<Duration>);

    impl Sleeper for NoSleep {
        fn sleep(&mut self, duration: Duration) {
            self.0.push(duration);
        }
    }

    #[test]
    fn busy_pending_after_retries_is_a_queued_submit() {
        let mut tmux = FakeTmux {
            states: VecDeque::from(vec![b"typed".to_vec(), b"typed".to_vec()]),
            tail: "Working...\n".to_owned(),
            ..FakeTmux::default()
        };
        assert_eq!(
            submit_enter_core(
                &mut tmux,
                &mut NoSleep::default(),
                "pane",
                SubmitPolicy {
                    retries: 2,
                    retry_delay: Duration::ZERO,
                    settle: Duration::ZERO,
                },
                ComposerPolicy::default(),
            ),
            ComposerState::Empty
        );
        assert_eq!(tmux.enters, 2);
    }

    #[test]
    fn row_and_pane_classification_cover_success_and_refusal_paths() {
        assert_eq!(
            classify_row("│ Working... │".as_bytes(), None, "working", 128).expect("busy"),
            ComposerState::Empty
        );
        for row in ["│ typed │", "┃ typed ┃", "| typed |"] {
            assert_eq!(
                classify_row(row.as_bytes(), None, "working", 128).expect("pending"),
                ComposerState::Pending
            );
        }
        assert!(classify_row(b"typed", None, "[", 128).is_err());

        let mut tmux = FakeTmux {
            states: VecDeque::from([b"| |".to_vec()]),
            tail: "old busy\n1\n2\n3\n4\n5\n6\n".to_owned(),
            ..FakeTmux::default()
        };
        assert_eq!(
            composer_state(&mut tmux, "pane", None, "working", 128),
            ComposerState::Empty
        );
        assert!(!pane_is_busy(&mut tmux, "pane", "old busy"));
        tmux.tail.push_str("Working...\n");
        assert!(pane_is_busy(&mut tmux, "pane", "working"));
        assert!(!pane_is_busy(&mut tmux, "pane", "["));
        tmux.tail_error = true;
        assert!(!pane_is_busy(&mut tmux, "pane", "working"));
        tmux.cursor_error = true;
        assert_eq!(
            composer_state(&mut tmux, "pane", None, "working", 128),
            ComposerState::Unknown
        );
    }

    #[test]
    fn submit_types_once_and_covers_retry_outcomes() {
        let submit = SubmitPolicy {
            retries: 0,
            retry_delay: Duration::from_millis(2),
            settle: Duration::from_millis(3),
        };
        let mut sleeper = NoSleep::default();
        let mut tmux = FakeTmux {
            states: VecDeque::from([b"| |".to_vec()]),
            ..FakeTmux::default()
        };
        assert_eq!(
            submit_core(
                &mut tmux,
                &mut sleeper,
                "pane",
                "hello",
                submit,
                ComposerPolicy::default(),
            )
            .expect("submit"),
            ComposerState::Empty
        );
        assert_eq!(tmux.literals, ["hello"]);
        assert_eq!(tmux.enters, 1);
        assert_eq!(sleeper.0, [submit.settle, submit.retry_delay]);

        let mut pending = FakeTmux::default();
        assert_eq!(
            submit_enter_core(
                &mut pending,
                &mut NoSleep::default(),
                "pane",
                submit,
                ComposerPolicy::default(),
            ),
            ComposerState::Pending
        );
        let mut refused = FakeTmux {
            literal_error: true,
            ..FakeTmux::default()
        };
        assert!(
            submit_core(
                &mut refused,
                &mut NoSleep::default(),
                "pane",
                "never accepted",
                submit,
                ComposerPolicy::default(),
            )
            .is_err()
        );
        assert_eq!(refused.enters, 0);
    }

    #[test]
    fn system_transport_bounds_and_command_failures_are_covered() {
        let mut echo = SystemTmux {
            executable: "/bin/echo".to_owned(),
            output_limit: 1024,
        };
        assert!(echo.cursor_y("pane").is_err());
        assert!(!echo.styled_row("pane", 3).expect("styled").is_empty());
        assert!(!echo.tail("pane").expect("tail").is_empty());
        echo.send_literal("pane", "text").expect("literal");
        echo.send_enter("pane").expect("enter");

        let mut oversized = SystemTmux {
            executable: "/bin/echo".to_owned(),
            output_limit: 1,
        };
        assert!(oversized.tail("pane").is_err());
        let mut failure = SystemTmux {
            executable: "/usr/bin/false".to_owned(),
            output_limit: 1024,
        };
        assert!(failure.tail("pane").is_err());
        assert!(failure.send_enter("pane").is_err());
        let mut missing = SystemTmux {
            executable: "/definitely/missing/mx-tmux".to_owned(),
            output_limit: 1024,
        };
        assert!(missing.tail("pane").is_err());
        assert!(missing.send_enter("pane").is_err());

        let _ = SystemTmux::default();
        let mut thread = ThreadSleeper;
        thread.sleep(Duration::ZERO);
    }
}
