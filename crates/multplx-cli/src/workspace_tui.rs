//! Thin terminal workspace over the canonical project and task projections.

use std::io::{self, IsTerminal, Stdout};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use multplx_domain::project_discovery::{self, ScanStatus};
use multplx_domain::project_registry::{ProjectRecord, read_catalog};
use multplx_domain::snapshot::{
    DomainRecord, PortfolioTask, SystemSnapshot, parse_system_snapshot,
};

use crate::system_snapshot;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs};
use ratatui::{Frame, Terminal};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Exit,
    Chat(Option<PathBuf>),
    Viz,
    Task {
        project: String,
        domain: Option<String>,
        text: String,
    },
    Interrupted(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum View {
    Projects,
    Tasks,
    Decisions,
    Domains,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LoadState {
    Loading,
    Ready,
    Error(String),
}

#[derive(Clone, Debug)]
struct ProjectRow {
    id: String,
    name: String,
    path: String,
    status: String,
}

#[derive(Clone, Debug)]
struct TaskRow {
    id: String,
    title: String,
    state: String,
    project: String,
}

#[derive(Clone, Debug)]
struct DetailRow {
    title: String,
    detail: String,
    target: Option<String>,
}

#[derive(Clone, Debug)]
struct Model {
    home: PathBuf,
    caller: PathBuf,
    connection: String,
    projects: Vec<ProjectRow>,
    tasks: Vec<TaskRow>,
    decisions: Vec<DetailRow>,
    domains: Vec<DetailRow>,
    scan: String,
    selected: usize,
    selection_active: bool,
    filter: String,
    filtering: bool,
    task_entry: bool,
    task_text: String,
    view: View,
    item_selected: usize,
    snapshot_at: Option<Instant>,
    state: LoadState,
}

impl Model {
    fn new(home: &Path, config: &Path, data: &Path, caller: &Path, connection: String) -> Self {
        let mut model = Self {
            home: home.to_path_buf(),
            caller: caller.to_path_buf(),
            connection,
            projects: project_rows(home, data),
            tasks: Vec::new(),
            decisions: Vec::new(),
            domains: Vec::new(),
            scan: scan_status(config, data),
            selected: 0,
            selection_active: false,
            filter: String::new(),
            filtering: false,
            task_entry: false,
            task_text: String::new(),
            view: View::Projects,
            item_selected: 0,
            snapshot_at: None,
            state: LoadState::Loading,
        };
        if let Ok(output) = Command::new("git")
            .arg("-C")
            .arg(caller)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            && output.status.success()
            && let Ok(path) = String::from_utf8(output.stdout).map(|value| value.trim().to_owned())
            && !model.projects.iter().any(|row| row.path == path)
        {
            model.projects.push(ProjectRow {
                id: path.clone(),
                name: Path::new(&path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("caller project")
                    .to_owned(),
                path,
                status: "caller · select to register".into(),
            });
        }
        let matched = model
            .projects
            .iter()
            .enumerate()
            .filter(|(_, row)| caller.starts_with(Path::new(&row.path)))
            .max_by_key(|(_, row)| Path::new(&row.path).components().count())
            .map(|(index, _)| index);
        model.selection_active = matched.is_some();
        model.selected = matched.unwrap_or(0);
        model
    }

    fn visible_projects(&self) -> Vec<&ProjectRow> {
        let needle = self.filter.to_ascii_lowercase();
        self.projects
            .iter()
            .filter(|row| {
                needle.is_empty()
                    || row.name.to_ascii_lowercase().contains(&needle)
                    || row.path.to_ascii_lowercase().contains(&needle)
            })
            .collect()
    }

    fn selected_id(&self) -> Option<String> {
        self.visible_projects()
            .get(self.selected)
            .map(|row| row.id.clone())
    }

    fn selected_path(&self) -> Option<PathBuf> {
        if !self.selection_active {
            return None;
        }
        self.visible_projects()
            .get(self.selected)
            .map(|row| PathBuf::from(&row.path))
    }

    fn replace_projects(&mut self, projects: Vec<ProjectRow>) {
        let selected = self.selected_id();
        self.projects = projects;
        let visible = self.visible_projects();
        self.selected = selected
            .and_then(|id| visible.iter().position(|row| row.id == id))
            .unwrap_or_else(|| self.selected.min(visible.len().saturating_sub(1)));
    }

    fn move_selection(&mut self, delta: isize) {
        if self.view != View::Projects {
            let len = match self.view {
                View::Tasks => self.tasks.len(),
                View::Decisions => self.decisions.len(),
                View::Domains => self.domains.len(),
                View::Projects => 0,
            };
            self.item_selected = self
                .item_selected
                .saturating_add_signed(delta)
                .min(len.saturating_sub(1));
            return;
        }
        self.selection_active = true;
        let len = self.visible_projects().len();
        if len == 0 {
            self.selected = 0;
        } else {
            self.selected = self
                .selected
                .saturating_add_signed(delta)
                .min(len.saturating_sub(1));
        }
    }

    fn cycle_view(&mut self) {
        self.view = match self.view {
            View::Projects => View::Tasks,
            View::Tasks => View::Decisions,
            View::Decisions => View::Domains,
            View::Domains => View::Projects,
        };
        self.item_selected = 0;
    }
}

fn project_rows(home: &Path, data: &Path) -> Vec<ProjectRow> {
    let mut rows = read_catalog(home)
        .map(|catalog| {
            catalog
                .projects
                .into_iter()
                .filter_map(project_row)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Ok(Some(cache)) = project_discovery::read_cache(data) {
        for candidate in cache.candidates {
            if rows
                .iter()
                .any(|row| row.path == candidate.canonical_path.display().to_string())
            {
                continue;
            }
            rows.push(ProjectRow {
                id: candidate
                    .checkout_id
                    .unwrap_or_else(|| candidate.canonical_path.display().to_string()),
                name: candidate.display_name,
                path: candidate.canonical_path.display().to_string(),
                status: if candidate.registered {
                    "registered".to_owned()
                } else if candidate.task_ready {
                    "discovered".to_owned()
                } else {
                    candidate.limitation.unwrap_or_else(|| "limited".to_owned())
                },
            });
        }
    }
    rows.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    rows
}

fn scan_status(config: &Path, data: &Path) -> String {
    match project_discovery::read_cache(data) {
        Ok(Some(cache)) => {
            let age = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |now| now.as_secs().saturating_sub(cache.updated_at));
            format!(
                "{} · updated {}s ago · {} directories · {}/{} roots{}",
                if cache.status == ScanStatus::Scanning {
                    "cached partial scan"
                } else {
                    "cached"
                },
                age,
                cache.scanned_directories,
                cache.completed_roots,
                cache.total_roots,
                if cache.errors.is_empty() {
                    String::new()
                } else {
                    format!(" · {} errors", cache.errors.len())
                }
            )
        }
        Ok(None)
            if project_discovery::read_config(config).is_ok_and(|value| value.roots.is_empty()) =>
        {
            "no discovery roots configured".to_owned()
        }
        Ok(None) => "not scanned · press r to refresh".to_owned(),
        Err(error) => format!("cache error: {error}"),
    }
}

fn project_row(project: ProjectRecord) -> Option<ProjectRow> {
    let path = project
        .checkouts
        .iter()
        .find(|checkout| checkout.canonical_path.is_dir())
        .or_else(|| project.checkouts.first())?
        .canonical_path
        .display()
        .to_string();
    Some(ProjectRow {
        id: project.project_id,
        name: project.display_name,
        path,
        status: "registered".to_owned(),
    })
}

fn task_rows(snapshot: &SystemSnapshot) -> Vec<TaskRow> {
    snapshot
        .portfolio
        .as_ref()
        .map(|portfolio| portfolio.tasks.iter().map(task_row).collect())
        .unwrap_or_default()
}

fn task_row(task: &PortfolioTask) -> TaskRow {
    TaskRow {
        id: task.id.clone(),
        title: task.title.clone(),
        state: task.state.clone(),
        project: task
            .project
            .get("display_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("-")
            .to_owned(),
    }
}

fn decision_rows(tasks: &[PortfolioTask]) -> Vec<DetailRow> {
    tasks
        .iter()
        .flat_map(|task| {
            task.decisions.iter().map(|decision| DetailRow {
                title: format!(
                    "{} · {}",
                    task.id,
                    decision
                        .get("question")
                        .or_else(|| decision.get("summary"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("pending decision")
                ),
                detail: decision.to_string(),
                target: None,
            })
        })
        .collect()
}

fn domain_rows(domains: &[DomainRecord]) -> Vec<DetailRow> {
    domains
        .iter()
        .map(|domain| DetailRow {
            title: domain
                .domain_id
                .clone()
                .unwrap_or_else(|| "unnamed domain".into()),
            detail: format!(
                "scope={} · projects={} · coordinator={}",
                domain.scope.as_deref().unwrap_or("-"),
                domain.projects.join(","),
                domain.coordinator
            ),
            target: domain.projects.first().cloned(),
        })
        .collect()
}

fn safe(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn safe_multiline(value: &str) -> String {
    value.lines().map(safe).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
fn push_utf8(pending: &mut Vec<u8>, target: &mut String, byte: u8) {
    pending.push(byte);
    match std::str::from_utf8(pending) {
        Ok(value) => {
            target.extend(value.chars().filter(|character| !character.is_control()));
            pending.clear();
        }
        Err(error) if error.error_len().is_some() => {
            target.push('\u{fffd}');
            pending.clear();
        }
        Err(_) => {}
    }
}

fn cells(character: char) -> usize {
    if character == '\0' || character.is_control() {
        0
    } else if matches!(character as u32, 0x1100..=0x115f | 0x2e80..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff | 0xfe10..=0xfe6f | 0xff00..=0xff60 | 0x1f300..=0x1faff)
    {
        2
    } else {
        1
    }
}

fn clip(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let value = safe(value);
    if value.chars().map(cells).sum::<usize>() <= width {
        return value;
    }
    if width == 1 {
        return "…".to_owned();
    }
    let mut used = 0;
    let mut output = String::new();
    for character in value.chars() {
        let next = cells(character);
        if used + next >= width {
            break;
        }
        output.push(character);
        used += next;
    }
    output + "…"
}

fn render(model: &Model, width: usize, height: usize) -> String {
    let width = width.max(1);
    let mut lines = vec![
        "Multplx workspace".to_owned(),
        clip(&format!("Home: {}", model.home.display()), width),
        clip(&format!("Context: {}", model.caller.display()), width),
        clip(
            &format!(
                "Suggested project: {}",
                model
                    .visible_projects()
                    .get(model.selected)
                    .filter(|_| model.selection_active)
                    .map_or("none", |row| row.name.as_str())
            ),
            width,
        ),
        clip(
            &format!(
                "Chat: {} · snapshot {}",
                model.connection,
                model.snapshot_at.map_or_else(
                    || "loading".to_owned(),
                    |at| format!("{}s ago", at.elapsed().as_secs())
                )
            ),
            width,
        ),
    ];
    if model.filtering || !model.filter.is_empty() {
        lines.push(clip(&format!("Filter: {}", model.filter), width));
    }
    lines.push(format!(
        "[Projects {}] [Tasks {}] [Decisions {}] [Domains {}]",
        model.projects.len(),
        model.tasks.len(),
        model.decisions.len(),
        model.domains.len()
    ));
    let available = height.saturating_sub(lines.len() + 2).max(1);
    match model.view {
        View::Projects => {
            lines.push(clip(&format!("Discovery: {}", model.scan), width));
            let projects = model.visible_projects();
            if projects.is_empty() {
                lines.push("No known projects. Add a path or discovery root.".to_owned());
            }
            let room = available.saturating_sub(1).max(1);
            let start = model.selected.saturating_sub(room.saturating_sub(1));
            for (index, row) in projects.iter().enumerate().skip(start).take(room) {
                lines.push(clip(
                    &format!(
                        "{} {} [{}] {}",
                        if index == model.selected { ">" } else { " " },
                        row.name,
                        row.status,
                        row.path
                    ),
                    width,
                ));
            }
        }
        View::Tasks => {
            match &model.state {
                LoadState::Loading => lines.push("Loading canonical task state…".into()),
                LoadState::Error(message) => lines.push(format!("State unavailable: {message}")),
                LoadState::Ready if model.tasks.is_empty() => {
                    lines.push("No accepted tasks.".into())
                }
                _ => {}
            }
            let start = model
                .item_selected
                .saturating_sub(available.saturating_sub(2));
            for (index, row) in model
                .tasks
                .iter()
                .enumerate()
                .skip(start)
                .take(available.saturating_sub(1).max(1))
            {
                lines.push(clip(
                    &format!(
                        "{} {} {} {} {}",
                        if index == model.item_selected {
                            ">"
                        } else {
                            " "
                        },
                        row.id,
                        row.state,
                        row.project,
                        row.title
                    ),
                    width,
                ));
            }
            if let Some(row) = model.tasks.get(model.item_selected) {
                lines.push(clip(
                    &format!("Detail: task {} · {} · {}", row.id, row.state, row.title),
                    width,
                ));
            }
        }
        View::Decisions => render_details(
            &mut lines,
            &model.decisions,
            model.item_selected,
            available,
            width,
            "No pending decisions.",
        ),
        View::Domains => render_details(
            &mut lines,
            &model.domains,
            model.item_selected,
            available,
            width,
            "No active domains.",
        ),
    }
    while lines.len() < height.saturating_sub(1) {
        lines.push(String::new());
    }
    lines.push(if model.task_entry {
        clip(&format!("New task: {}", model.task_text), width)
    } else if model.filtering {
        "Type to filter · Enter finish · Esc clear".to_owned()
    } else {
        "Tab view · ↑/↓ move · / filter · t task · r refresh · c chat · v Viz · q quit".to_owned()
    });
    lines.truncate(height.max(1));
    lines
        .into_iter()
        .map(|line| clip(&line, width))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn render_details(
    lines: &mut Vec<String>,
    rows: &[DetailRow],
    selected: usize,
    available: usize,
    width: usize,
    empty: &str,
) {
    if rows.is_empty() {
        lines.push(empty.to_owned());
        return;
    }
    let room = available.saturating_sub(1).max(1);
    let start = selected.saturating_sub(room.saturating_sub(1));
    for (index, row) in rows.iter().enumerate().skip(start).take(room) {
        lines.push(clip(
            &format!(
                "{} {}",
                if index == selected { ">" } else { " " },
                row.title
            ),
            width,
        ));
    }
    if let Some(row) = rows.get(selected) {
        lines.push(clip(&format!("Detail: {}", row.detail), width));
    }
}

fn selected_view_index(view: View) -> usize {
    match view {
        View::Projects => 0,
        View::Tasks => 1,
        View::Decisions => 2,
        View::Domains => 3,
    }
}

fn current_rows(model: &Model) -> (Vec<String>, Option<String>, Option<usize>) {
    match model.view {
        View::Projects => {
            let rows = model.visible_projects();
            (
                rows.iter()
                    .map(|row| format!("{}  ·  {}", safe(&row.name), safe(&row.status)))
                    .collect(),
                rows.get(model.selected)
                    .map(|row| format!("{}\n{}\n\n{}", row.name, row.path, row.status)),
                (!rows.is_empty()).then_some(model.selected),
            )
        }
        View::Tasks => (
            model
                .tasks
                .iter()
                .map(|row| format!("{}  ·  {}", safe(&row.id), safe(&row.title)))
                .collect(),
            model
                .tasks
                .get(model.item_selected)
                .map(|row| format!("{}\n{}\nProject: {}", row.title, row.state, row.project)),
            (!model.tasks.is_empty()).then_some(model.item_selected),
        ),
        View::Decisions => (
            model.decisions.iter().map(|row| safe(&row.title)).collect(),
            model
                .decisions
                .get(model.item_selected)
                .map(|row| format!("{}\n\n{}", row.title, row.detail)),
            (!model.decisions.is_empty()).then_some(model.item_selected),
        ),
        View::Domains => (
            model.domains.iter().map(|row| safe(&row.title)).collect(),
            model
                .domains
                .get(model.item_selected)
                .map(|row| format!("{}\n\n{}", row.title, row.detail)),
            (!model.domains.is_empty()).then_some(model.item_selected),
        ),
    }
}

fn render_frame(frame: &mut Frame<'_>, model: &Model) {
    let area = frame.area();
    let background = Block::default()
        .title(" Multplx · Workspace ")
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = background.inner(area);
    frame.render_widget(background, area);
    let compact_header = inner.height < 18;
    let [header_area, tabs_area, body_area, footer_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if compact_header { 3 } else { 4 }),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .areas(inner);

    let selected_project = model
        .visible_projects()
        .get(model.selected)
        .filter(|_| model.selection_active)
        .map_or("none", |row| row.name.as_str());
    let mut header_lines = vec![
        Line::from(vec![
            Span::styled("Snapshot  ", Style::default().fg(Color::Cyan)),
            Span::raw(model.snapshot_at.map_or_else(
                || "loading".to_owned(),
                |at| format!("{}s ago", at.elapsed().as_secs()),
            )),
            Span::raw(" · "),
            Span::styled("Chat  ", Style::default().fg(Color::Cyan)),
            Span::raw(safe(&model.connection)),
        ]),
        Line::from(vec![
            Span::styled("Context  ", Style::default().fg(Color::Cyan)),
            Span::raw(safe(&model.caller.display().to_string())),
            Span::raw("   "),
            Span::styled("Project  ", Style::default().fg(Color::Cyan)),
            Span::raw(safe(selected_project)),
        ]),
        Line::from(vec![
            Span::styled("Discovery  ", Style::default().fg(Color::Cyan)),
            Span::raw(safe(&model.scan)),
        ]),
    ];
    if !compact_header {
        header_lines.push(Line::from(vec![
            Span::styled("Home  ", Style::default().fg(Color::Cyan)),
            Span::raw(safe(&model.home.display().to_string())),
        ]));
    }
    frame.render_widget(
        Paragraph::new(header_lines).style(Style::default().fg(Color::White)),
        header_area,
    );

    let tabs = [
        format!("Projects {}", model.projects.len()),
        format!("Tasks {}", model.tasks.len()),
        format!("Decisions {}", model.decisions.len()),
        format!("Domains {}", model.domains.len()),
    ]
    .into_iter()
    .map(Line::from)
    .collect::<Vec<_>>();
    frame.render_widget(
        Tabs::new(tabs)
            .select(selected_view_index(model.view))
            .divider(" │ ")
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        tabs_area,
    );

    let (rows, detail, selected) = current_rows(model);
    let body_columns = if body_area.width >= 64 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(body_area)
            .to_vec()
    } else if body_area.height >= 8 && detail.is_some() {
        let [list_area, detail_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .areas(body_area);
        vec![list_area, detail_area]
    } else {
        vec![body_area]
    };
    let empty = match model.view {
        View::Projects => {
            "No known projects. Enter a path or configure discovery roots.".to_owned()
        }
        View::Tasks => match &model.state {
            LoadState::Loading => "Loading canonical task state…".to_owned(),
            LoadState::Error(message) => format!("State unavailable: {}", safe(message)),
            LoadState::Ready => "No accepted tasks.".to_owned(),
        },
        View::Decisions => "No pending decisions.".to_owned(),
        View::Domains => "No active domains.".to_owned(),
    };
    let list_items = if rows.is_empty() {
        vec![ListItem::new(empty).style(Style::default().fg(Color::DarkGray))]
    } else {
        rows.iter()
            .map(|row| ListItem::new(safe(row)))
            .collect::<Vec<_>>()
    };
    let title = match model.view {
        View::Projects => "Projects",
        View::Tasks => "Tasks",
        View::Decisions => "Pending decisions",
        View::Domains => "Active domains",
    };
    let mut list_state = ListState::default();
    list_state.select(selected);
    frame.render_stateful_widget(
        List::new(list_items)
            .block(
                Block::default()
                    .title(format!(" {title} "))
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .highlight_symbol("› ")
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        body_columns[0],
        &mut list_state,
    );
    if let Some(detail_area) = body_columns.get(1) {
        let detail = detail.unwrap_or_else(|| match &model.state {
            LoadState::Loading if model.view == View::Tasks => {
                "Loading canonical task state…".into()
            }
            LoadState::Error(message) if model.view == View::Tasks => message.clone(),
            _ => "Select an item to see its details.".into(),
        });
        frame.render_widget(
            Paragraph::new(safe_multiline(&detail))
                .block(
                    Block::default()
                        .title(" Details ")
                        .borders(Borders::ALL)
                        .border_type(ratatui::widgets::BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(ratatui::widgets::Wrap { trim: true }),
            *detail_area,
        );
    }

    let (footer, offset) = if model.task_entry {
        let line = format!("New task: {}█", safe(&model.task_text));
        let width = Line::from(line.clone()).width();
        let visible = usize::from(footer_area.width.saturating_sub(1));
        let offset = width.saturating_sub(visible).min(usize::from(u16::MAX)) as u16;
        (line, offset)
    } else if model.filtering {
        (format!("Filter: {}█", safe(&model.filter)), 0)
    } else if footer_area.width < 64 {
        ("Tab · ↑↓ · / · t · c · v · q".into(), 0)
    } else {
        (
            "Tab · ↑↓/jk · /filter · t task · r refresh · c chat · v viz · q quit".into(),
            0,
        )
    };
    frame.render_widget(
        Paragraph::new(safe(&footer))
            .style(Style::default().fg(Color::Gray))
            .scroll((0, offset)),
        footer_area,
    );
}

fn dimensions() -> (usize, usize) {
    if let Ok((columns, rows)) = terminal::size()
        && columns > 0
        && rows > 0
    {
        return (usize::from(columns), usize::from(rows));
    }
    let value = |name: &str, fallback| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(fallback)
    };
    (value("COLUMNS", 80), value("LINES", 24))
}

struct SnapshotRows {
    tasks: Vec<TaskRow>,
    decisions: Vec<DetailRow>,
    domains: Vec<DetailRow>,
}

fn snapshot_receiver(
    root: &Path,
    home: &Path,
    config: &Path,
    data: &Path,
) -> Receiver<Result<SnapshotRows, String>> {
    let (sender, receiver) = mpsc::channel();
    let root = root.to_path_buf();
    let home = home.to_path_buf();
    let config = config.to_path_buf();
    let data = data.to_path_buf();
    thread::spawn(move || {
        let paths = system_snapshot::Paths {
            projects: home.join("projects"),
            config,
            state: home.join("state"),
            data,
            source_root: root.clone(),
            root,
            home,
        };
        let (status, stdout, stderr) = system_snapshot::run(&["--json".to_owned()], &paths);
        let result = if status == 0 {
            parse_system_snapshot(stdout.as_bytes())
                .map(|snapshot| SnapshotRows {
                    decisions: snapshot
                        .portfolio
                        .as_ref()
                        .map_or_else(Vec::new, |portfolio| decision_rows(&portfolio.tasks)),
                    domains: domain_rows(&snapshot.domains.records),
                    tasks: task_rows(&snapshot),
                })
                .map_err(|error| error.stderr.trim().to_owned())
        } else {
            Err(stderr.trim().to_owned())
        };
        let _ = sender.send(result);
    });
    receiver
}

fn refresh_receiver(home: &Path, config: &Path, data: &Path) -> Receiver<Result<(), String>> {
    let (sender, receiver) = mpsc::channel();
    let home = home.to_path_buf();
    let config = config.to_path_buf();
    let data = data.to_path_buf();
    thread::spawn(move || {
        let _ = sender.send(project_discovery::refresh(&home, &config, &data).map(|_| ()));
    });
    receiver
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    raw: bool,
    alternate: bool,
    signal_ids: Vec<signal_hook::SigId>,
    signal: Arc<AtomicUsize>,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        let mut session = Self {
            terminal: Terminal::new(CrosstermBackend::new(io::stdout()))?,
            raw: false,
            alternate: false,
            signal_ids: Vec::new(),
            signal: Arc::new(AtomicUsize::new(0)),
        };
        terminal::enable_raw_mode()?;
        session.raw = true;
        execute!(session.terminal.backend_mut(), EnterAlternateScreen)?;
        session.alternate = true;
        execute!(
            session.terminal.backend_mut(),
            EnableMouseCapture,
            crossterm::cursor::Hide
        )?;
        for (number, code) in [
            (signal_hook::consts::SIGINT, 130),
            (signal_hook::consts::SIGTERM, 143),
            (signal_hook::consts::SIGHUP, 129),
        ] {
            let id = signal_hook::flag::register_usize(number, Arc::clone(&session.signal), code)
                .map_err(io::Error::other)?;
            session.signal_ids.push(id);
        }
        Ok(session)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        for id in self.signal_ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
        let _ = self.terminal.show_cursor();
        if self.alternate {
            let _ = execute!(
                self.terminal.backend_mut(),
                crossterm::cursor::Show,
                DisableMouseCapture,
                LeaveAlternateScreen
            );
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum KeyOutcome {
    Continue,
    Refresh,
    Action(Action),
}

fn handle_key(model: &mut Model, key: KeyEvent) -> KeyOutcome {
    if key.kind == KeyEventKind::Release {
        return KeyOutcome::Continue;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return KeyOutcome::Action(Action::Interrupted(130));
    }
    if model.task_entry {
        match key.code {
            KeyCode::Enter if !model.task_text.trim().is_empty() => {
                let routed = (model.view == View::Domains)
                    .then(|| model.domains.get(model.item_selected))
                    .flatten();
                let project = routed
                    .and_then(|row| row.target.clone())
                    .or_else(|| model.selected_path().map(|path| path.display().to_string()));
                if let Some(project) = project {
                    return KeyOutcome::Action(Action::Task {
                        project,
                        domain: routed.map(|row| row.title.clone()),
                        text: model.task_text.trim().to_owned(),
                    });
                }
            }
            KeyCode::Esc => {
                model.task_entry = false;
                model.task_text.clear();
            }
            KeyCode::Backspace => {
                model.task_text.pop();
            }
            KeyCode::Char(character) if !character.is_control() => model.task_text.push(character),
            _ => {}
        }
        return KeyOutcome::Continue;
    }
    if model.filtering {
        match key.code {
            KeyCode::Enter => model.filtering = false,
            KeyCode::Esc => {
                model.filter.clear();
                model.filtering = false;
                model.selected = 0;
            }
            KeyCode::Backspace => {
                model.filter.pop();
                model.selected = 0;
            }
            KeyCode::Char(character) if !character.is_control() => {
                model.filter.push(character);
                model.selected = 0;
            }
            _ => {}
        }
        return KeyOutcome::Continue;
    }
    match key.code {
        KeyCode::Char('q') => KeyOutcome::Action(Action::Exit),
        KeyCode::Char('c') | KeyCode::Enter => {
            KeyOutcome::Action(Action::Chat(model.selected_path()))
        }
        KeyCode::Char('v') => KeyOutcome::Action(Action::Viz),
        KeyCode::Tab => {
            model.cycle_view();
            KeyOutcome::Continue
        }
        KeyCode::Char('t')
            if model.selected_path().is_some()
                || (model.view == View::Domains
                    && model
                        .domains
                        .get(model.item_selected)
                        .and_then(|row| row.target.as_ref())
                        .is_some()) =>
        {
            model.task_entry = true;
            KeyOutcome::Continue
        }
        KeyCode::Char('/') => {
            model.filtering = true;
            KeyOutcome::Continue
        }
        KeyCode::Char('j') | KeyCode::Down => {
            model.move_selection(1);
            KeyOutcome::Continue
        }
        KeyCode::Char('k') | KeyCode::Up => {
            model.move_selection(-1);
            KeyOutcome::Continue
        }
        KeyCode::Char('r') => KeyOutcome::Refresh,
        _ => KeyOutcome::Continue,
    }
}

fn handle_event(model: &mut Model, event: Event) -> KeyOutcome {
    match event {
        Event::Key(key) => handle_key(model, key),
        Event::Mouse(mouse)
            if !model.filtering
                && !model.task_entry
                && mouse.kind == MouseEventKind::ScrollDown =>
        {
            model.move_selection(1);
            KeyOutcome::Continue
        }
        Event::Mouse(mouse)
            if !model.filtering && !model.task_entry && mouse.kind == MouseEventKind::ScrollUp =>
        {
            model.move_selection(-1);
            KeyOutcome::Continue
        }
        _ => KeyOutcome::Continue,
    }
}

/// Render the workspace, returning the selected launcher action.
pub(crate) struct RunContext<'a> {
    pub(crate) root: &'a Path,
    pub(crate) home: &'a Path,
    pub(crate) caller: &'a Path,
    pub(crate) config: &'a Path,
    pub(crate) data: &'a Path,
    pub(crate) selected: Option<&'a Path>,
    pub(crate) connection: String,
    pub(crate) plain: bool,
}

pub(crate) fn run(context: RunContext<'_>) -> Action {
    let RunContext {
        root,
        home,
        caller,
        config,
        data,
        selected,
        connection,
        plain,
    } = context;
    let mut model = Model::new(home, config, data, caller, connection);
    if let Some(selected) = selected {
        let selected = selected.display().to_string();
        if let Some(index) = model.projects.iter().position(|row| row.path == selected) {
            model.selected = index;
            model.selection_active = true;
        }
    }
    let mut snapshot = Some(snapshot_receiver(root, home, config, data));
    let mut refresh: Option<Receiver<Result<(), String>>> = if !plain
        && project_discovery::read_cache(data).ok().flatten().is_none()
        && project_discovery::read_config(config).is_ok_and(|value| !value.roots.is_empty())
    {
        model.scan = "scanning in background".to_owned();
        Some(refresh_receiver(home, config, data))
    } else {
        None
    };
    if plain || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        match snapshot.take().expect("snapshot receiver").recv() {
            Ok(Ok(rows)) => {
                model.tasks = rows.tasks;
                model.decisions = rows.decisions;
                model.domains = rows.domains;
                model.state = LoadState::Ready;
                model.snapshot_at = Some(Instant::now());
            }
            Ok(Err(message)) => model.state = LoadState::Error(message),
            Err(_) => model.state = LoadState::Error("snapshot worker stopped".to_owned()),
        }
        let (width, height) = dimensions();
        print!("{}", render(&model, width, height));
        return Action::Exit;
    }

    let mut session = match TerminalSession::enter() {
        Ok(session) => session,
        Err(error) => {
            eprintln!("workspace terminal setup failed: {error}");
            return Action::Interrupted(1);
        }
    };
    let mut dirty = true;
    let mut last_status_tick = Instant::now();
    loop {
        if let Some(code) = match session.signal.load(Ordering::Relaxed) {
            129 | 130 | 143 => Some(session.signal.load(Ordering::Relaxed) as i32),
            _ => None,
        } {
            return Action::Interrupted(code);
        }
        if let Some(receiver) = &snapshot
            && let Ok(result) = receiver.try_recv()
        {
            match result {
                Ok(rows) => {
                    model.tasks = rows.tasks;
                    model.decisions = rows.decisions;
                    model.domains = rows.domains;
                    model.state = LoadState::Ready;
                    model.snapshot_at = Some(Instant::now());
                }
                Err(message) => model.state = LoadState::Error(message),
            }
            snapshot = None;
            dirty = true;
        }
        if let Some(receiver) = &refresh
            && let Ok(result) = receiver.try_recv()
        {
            match result {
                Ok(()) => {
                    model.replace_projects(project_rows(home, data));
                    model.scan = scan_status(config, data);
                }
                Err(error) => model.scan = format!("refresh failed: {error}"),
            }
            refresh = None;
            dirty = true;
        }
        if last_status_tick.elapsed() >= std::time::Duration::from_secs(1) {
            last_status_tick = Instant::now();
            if model.snapshot_at.is_some() {
                dirty = true;
            }
            if refresh.is_some() {
                let projects = project_rows(home, data);
                let scan = scan_status(config, data);
                if model.scan != scan || model.projects.len() != projects.len() {
                    model.replace_projects(projects);
                    model.scan = scan;
                    dirty = true;
                }
            }
        }
        if dirty {
            if let Err(error) = session.terminal.draw(|frame| render_frame(frame, &model)) {
                drop(session);
                eprintln!("workspace render failed: {error}");
                return Action::Interrupted(1);
            }
            dirty = false;
        }
        match event::poll(std::time::Duration::from_millis(100)) {
            Ok(true) => match event::read() {
                Ok(Event::Resize(_, _)) => dirty = true,
                Ok(input) => match handle_event(&mut model, input) {
                    KeyOutcome::Continue => dirty = true,
                    KeyOutcome::Action(action) => return action,
                    KeyOutcome::Refresh if refresh.is_none() => {
                        model.scan = "scanning in background".to_owned();
                        refresh = Some(refresh_receiver(home, config, data));
                        if snapshot.is_none() {
                            model.state = LoadState::Loading;
                            snapshot = Some(snapshot_receiver(root, home, config, data));
                        }
                        model.connection =
                            multplx_backend::harness_launch::conversation_state(home).description();
                        dirty = true;
                    }
                    KeyOutcome::Refresh => {}
                },
                Err(error) => {
                    drop(session);
                    eprintln!("workspace input failed: {error}");
                    return Action::Interrupted(1);
                }
            },
            Ok(false) => {}
            Err(error) => {
                drop(session);
                eprintln!("workspace input failed: {error}");
                return Action::Interrupted(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::fs;

    fn model() -> Model {
        let mut model = Model::new(
            Path::new("/a/home"),
            Path::new("/a/config"),
            Path::new("/a/data"),
            Path::new("/a/caller"),
            "offline".into(),
        );
        model.projects = vec![row("alpha", "Alpha"), row("beta", "Beta")];
        model.selection_active = true;
        model
    }

    fn text_of(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn ratatui_frame_fits_narrow_terminals_and_keeps_selected_details() {
        let mut model = model();
        model.projects[1].name = "Second 界 project".into();
        model.selected = 1;
        let backend = TestBackend::new(38, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render_frame(frame, &model)).unwrap();
        let text = text_of(&terminal);
        assert!(text.contains("Projects"));
        assert!(text.contains("Second"));
        assert!(text.contains("/work/Beta"));
        assert!(text.contains("· q"));

        model.view = View::Tasks;
        model.state = LoadState::Ready;
        model.tasks.push(TaskRow {
            id: "task-1".into(),
            title: "Review plan".into(),
            state: "queued".into(),
            project: "Multplx".into(),
        });
        terminal
            .resize(ratatui::layout::Rect::new(0, 0, 38, 16))
            .unwrap();
        terminal.draw(|frame| render_frame(frame, &model)).unwrap();
        let text = text_of(&terminal);
        assert!(text.contains("Review plan"));
        assert!(text.contains("queued"));
    }

    #[test]
    fn crossterm_key_and_mouse_events_preserve_navigation_and_action_contract() {
        let mut model = model();
        assert_eq!(
            handle_event(
                &mut model,
                Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            ),
            KeyOutcome::Continue
        );
        assert_eq!(model.selected, 1);
        handle_event(
            &mut model,
            Event::Mouse(crossterm::event::MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }),
        );
        assert_eq!(model.selected, 0);
        assert_eq!(
            handle_event(
                &mut model,
                Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE))
            ),
            KeyOutcome::Action(Action::Chat(Some(PathBuf::from("/work/Alpha"))))
        );
        assert_eq!(
            handle_event(
                &mut model,
                Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            ),
            KeyOutcome::Action(Action::Interrupted(130))
        );

        model.task_entry = true;
        for character in "task 界".chars() {
            handle_event(
                &mut model,
                Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
            );
        }
        assert_eq!(model.task_text, "task 界");
        assert_eq!(
            handle_event(
                &mut model,
                Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            ),
            KeyOutcome::Continue
        );
        assert_eq!(model.task_text, "task ");
    }

    fn row(id: &str, name: &str) -> ProjectRow {
        ProjectRow {
            id: id.to_owned(),
            name: name.to_owned(),
            path: format!("/work/{name}"),
            status: "registered".to_owned(),
        }
    }

    #[test]
    fn render_covers_narrow_empty_loading_and_error_views() {
        let mut model = Model::new(
            Path::new("/home"),
            Path::new("/config"),
            Path::new("/data"),
            Path::new("/caller"),
            "offline".into(),
        );
        let narrow = render(&model, 24, 12);
        assert!(narrow.lines().all(|line| line.chars().count() <= 24));
        assert!(narrow.contains("No known projects"));
        model.view = View::Tasks;
        let narrow = render(&model, 24, 12);
        assert!(narrow.contains("Loading canonical"));
        model.state = LoadState::Error("provider failed".into());
        assert!(render(&model, 80, 24).contains("State unavailable: provider failed"));
    }

    #[test]
    fn filtering_navigation_and_refresh_keep_a_stable_selection() {
        let mut model = Model::new(
            Path::new("/home"),
            Path::new("/config"),
            Path::new("/data"),
            Path::new("/caller"),
            "live".into(),
        );
        model.projects = vec![row("a", "alpha"), row("b", "beta"), row("g", "gamma")];
        model.move_selection(1);
        assert_eq!(model.selected_id().as_deref(), Some("b"));
        model.replace_projects(vec![row("g", "gamma"), row("b", "beta"), row("a", "alpha")]);
        assert_eq!(model.selected_id().as_deref(), Some("b"));
        model.filter = "gam".into();
        model.selected = 0;
        assert_eq!(model.selected_id().as_deref(), Some("g"));
        model.move_selection(-1);
        assert_eq!(model.selected, 0);
    }

    #[test]
    fn resize_bounds_visible_rows_and_screen_height() {
        let mut model = Model::new(
            Path::new("/home"),
            Path::new("/config"),
            Path::new("/data"),
            Path::new("/caller"),
            "live".into(),
        );
        model.projects = (0..20)
            .map(|index| row(&format!("p{index}"), &format!("project-{index}")))
            .collect();
        model.state = LoadState::Ready;
        let small = render(&model, 40, 9);
        let large = render(&model, 100, 30);
        assert!(small.lines().count() <= 9);
        assert!(large.lines().count() > small.lines().count());
        model.selected = 19;
        assert!(render(&model, 100, 30).contains("project-19"));
    }

    #[test]
    fn renderer_neutralizes_terminal_controls_and_counts_wide_cells() {
        assert_eq!(clip("safe\u{1b}[31m", 80), "safe [31m");
        assert_eq!(clip("界界", 3), "界…");
        assert_eq!(clip("anything", 0), "");
        assert_eq!(clip("anything", 1), "…");
        assert_eq!(cells('\0'), 0);
        let mut pending = Vec::new();
        let mut text = String::new();
        for byte in "café 界".bytes() {
            push_utf8(&mut pending, &mut text, byte);
        }
        assert_eq!(text, "café 界");
        assert!(pending.is_empty());
        push_utf8(&mut pending, &mut text, 0xff);
        assert!(text.ends_with('\u{fffd}'));
    }

    #[test]
    fn empty_views_entry_prompts_and_selection_bounds_remain_visible() {
        let mut model = Model::new(
            Path::new("/home"),
            Path::new("/config"),
            Path::new("/data"),
            Path::new("/caller"),
            "offline".into(),
        );
        model.state = LoadState::Ready;
        model.move_selection(1);
        assert_eq!(model.selected, 0);

        model.view = View::Decisions;
        model.move_selection(1);
        assert!(render(&model, 80, 15).contains("No pending decisions."));
        model.view = View::Domains;
        model.move_selection(1);
        assert!(render(&model, 80, 15).contains("No active domains."));

        model.task_entry = true;
        model.task_text = "review café".into();
        assert!(render(&model, 80, 15).contains("New task: review café"));
        model.task_entry = false;
        model.filtering = true;
        assert!(render(&model, 80, 15).contains("Type to filter"));
        model.filtering = false;
        model.filter = "alpha".into();
        assert!(render(&model, 80, 15).contains("Filter: alpha"));
    }

    #[test]
    fn scan_status_distinguishes_unconfigured_unscanned_and_corrupt_cache() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        fs::create_dir(&config).unwrap();
        fs::create_dir(&data).unwrap();
        assert_eq!(scan_status(&config, &data), "no discovery roots configured");

        fs::write(
            config.join("project-discovery.json"),
            serde_json::json!({
                "schema_version": project_discovery::DISCOVERY_SCHEMA_VERSION,
                "roots": [{"path": temp.path(), "max_depth": 1}],
                "exclusions": []
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            scan_status(&config, &data),
            "not scanned · press r to refresh"
        );
        fs::write(data.join("project-discovery-cache.json"), "not json").unwrap();
        assert!(scan_status(&config, &data).starts_with("cache error:"));

        fs::write(config.join("project-discovery.json"), "not json").unwrap();
        let refresh = refresh_receiver(&temp.path().join("home"), &config, &data)
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(refresh.is_err());
    }

    #[test]
    fn plain_run_resolves_selected_context_and_starts_background_workers() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("runtime");
        let home = temp.path().join("home");
        let config = home.join("config");
        let data = home.join("data");
        let caller = temp.path().join("caller");
        for path in [
            &root,
            &home,
            &config,
            &data,
            &home.join("projects"),
            &home.join("state"),
            &caller,
        ] {
            fs::create_dir_all(path).unwrap();
        }
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&caller)
                .args(["init", "-q"])
                .status()
                .unwrap()
                .success()
        );
        let caller = caller.canonicalize().unwrap();
        fs::write(
            config.join("project-discovery.json"),
            serde_json::json!({
                "schema_version": project_discovery::DISCOVERY_SCHEMA_VERSION,
                "roots": [{"path": caller, "max_depth": 0}],
                "exclusions": []
            })
            .to_string(),
        )
        .unwrap();

        assert_eq!(
            run(RunContext {
                root: &root,
                home: &home,
                caller: &caller,
                config: &config,
                data: &data,
                selected: Some(&caller),
                connection: "offline".to_owned(),
                plain: false,
            }),
            Action::Exit
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        let snapshot = snapshot_receiver(&root, &home, &config, &data)
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(snapshot.is_ok());
    }

    #[test]
    fn tab_views_keep_details_and_controls_visible() {
        let mut model = Model::new(
            Path::new("/home"),
            Path::new("/config"),
            Path::new("/data"),
            Path::new("/caller"),
            "live".into(),
        );
        model.decisions = vec![DetailRow {
            title: "task-7 · choose API".into(),
            detail: "question=Which API?".into(),
            target: None,
        }];
        model.domains = vec![DetailRow {
            title: "payments".into(),
            detail: "scope=services/payments".into(),
            target: Some("project-payments".into()),
        }];
        model.view = View::Decisions;
        let decision = render(&model, 80, 24);
        assert!(decision.contains("Which API?"));
        assert!(decision.lines().last().unwrap().contains("Tab view"));
        model.cycle_view();
        assert!(render(&model, 80, 24).contains("services/payments"));
    }

    #[test]
    fn outside_a_project_requires_an_explicit_navigation_before_context() {
        let mut model = Model::new(
            Path::new("/home"),
            Path::new("/config"),
            Path::new("/data"),
            Path::new("/outside"),
            "offline".into(),
        );
        model.projects = vec![row("a", "alpha"), row("b", "beta")];
        model.selected = 0;
        model.selection_active = false;
        assert!(model.selected_path().is_none());
        assert!(render(&model, 80, 15).contains("Suggested project: none"));
        model.move_selection(1);
        assert_eq!(model.selected_path(), Some(PathBuf::from("/work/beta")));
    }

    #[test]
    fn caller_git_root_is_suggested_without_implicit_registration() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("caller-repo");
        fs::create_dir(&repo).unwrap();
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["init", "-q"])
                .status()
                .unwrap()
                .success()
        );
        let nested = repo.join("nested");
        fs::create_dir(&nested).unwrap();
        let nested = nested.canonicalize().unwrap();
        let model = Model::new(
            &temp.path().join("home"),
            &temp.path().join("config"),
            &temp.path().join("data"),
            &nested,
            "offline".into(),
        );
        assert_eq!(model.projects.len(), 1);
        assert_eq!(model.projects[0].status, "caller · select to register");
        assert_eq!(model.selected_path(), Some(repo.canonicalize().unwrap()));
        assert!(
            !temp
                .path()
                .join("home/config/project-registry.json")
                .exists()
        );
    }

    #[test]
    fn task_view_scrolls_to_selected_canonical_detail() {
        let mut model = Model::new(
            Path::new("/home"),
            Path::new("/config"),
            Path::new("/data"),
            Path::new("/outside"),
            "live".into(),
        );
        model.state = LoadState::Ready;
        model.view = View::Tasks;
        model.tasks = (0..12)
            .map(|index| TaskRow {
                id: format!("task-{index}"),
                title: format!("title {index}"),
                state: "queued".into(),
                project: "alpha".into(),
            })
            .collect();
        for _ in 0..11 {
            model.move_selection(1);
        }
        let rendered = render(&model, 60, 12);
        assert!(rendered.contains("> task-11 queued alpha title 11"));
        assert!(rendered.contains("Detail: task task-11"));
        assert!(rendered.lines().last().unwrap().contains("Tab view"));
    }

    #[test]
    fn persisted_incomplete_scan_is_presented_as_cached_progress() {
        let temp = tempfile::tempdir().unwrap();
        let cache = project_discovery::DiscoveryCache {
            schema_version: project_discovery::DISCOVERY_SCHEMA_VERSION,
            status: ScanStatus::Scanning,
            scanned_directories: 17,
            completed_roots: 1,
            total_roots: 3,
            updated_at: 1,
            candidates: Vec::new(),
            errors: vec!["root unavailable".into()],
        };
        fs::write(
            temp.path().join("project-discovery-cache.json"),
            serde_json::to_vec(&cache).unwrap(),
        )
        .unwrap();
        let status = scan_status(&temp.path().join("config"), temp.path());
        assert!(status.contains("cached partial scan"));
        assert!(status.contains("updated "));
        assert!(status.contains("17 directories · 1/3 roots · 1 errors"));
    }

    #[test]
    fn discovery_rows_preserve_ready_registered_and_limited_states() {
        let temp = tempfile::tempdir().unwrap();
        let candidates = [
            ("registered", true, true, None),
            ("ready", false, true, None),
            ("unborn", false, false, Some("unborn repository".to_owned())),
        ]
        .into_iter()
        .map(
            |(name, registered, task_ready, limitation)| project_discovery::DiscoveryCandidate {
                canonical_path: temp.path().join(name),
                display_name: name.to_owned(),
                kind: project_discovery::CandidateKind::Git,
                project_id: registered.then(|| format!("project-{name}")),
                checkout_id: registered.then(|| format!("checkout-{name}")),
                parent_repository: None,
                registered,
                task_ready,
                limitation,
            },
        )
        .collect();
        let cache = project_discovery::DiscoveryCache {
            schema_version: project_discovery::DISCOVERY_SCHEMA_VERSION,
            status: ScanStatus::Complete,
            scanned_directories: 3,
            completed_roots: 1,
            total_roots: 1,
            updated_at: 1,
            candidates,
            errors: Vec::new(),
        };
        fs::write(
            temp.path().join("project-discovery-cache.json"),
            serde_json::to_vec(&cache).unwrap(),
        )
        .unwrap();
        let rows = project_rows(&temp.path().join("home"), temp.path());
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .find(|row| row.name == "registered")
                .unwrap()
                .status,
            "registered"
        );
        assert_eq!(
            rows.iter().find(|row| row.name == "ready").unwrap().status,
            "discovered"
        );
        assert_eq!(
            rows.iter().find(|row| row.name == "unborn").unwrap().status,
            "unborn repository"
        );
        assert!(
            scan_status(&temp.path().join("config"), temp.path()).starts_with("cached · updated")
        );
    }

    #[test]
    fn canonical_task_decisions_and_domains_keep_routing_details() {
        let task: PortfolioTask = serde_json::from_value(serde_json::json!({
            "key":"home:task-1", "id":"task-1", "title":"Deliver", "children":[],
            "project":{"display_name":"alpha"}, "state":"blocked", "priority":1,
            "owner":{}, "attempt":{}, "prior_attempts":[], "brief":{}, "workflow":{},
            "dependencies":[], "decisions":[{"question":"Choose API"}], "evidence":{},
            "allocation":{}, "sessions":[], "native_observations":[], "latest_change":{},
            "freshness":{"status":"fresh", "partial":false, "reasons":[]}
        }))
        .unwrap();
        let task_row = task_row(&task);
        assert_eq!(task_row.project, "alpha");
        let decisions = decision_rows(&[task]);
        assert_eq!(decisions[0].title, "task-1 · Choose API");

        let domain: DomainRecord = serde_json::from_value(serde_json::json!({
            "domain_id":"payments", "scope":"services/payments", "projects":["project-a"],
            "coordinator":{"id":"coord"}, "channel":{}, "observation":{}, "counts":{},
            "children":[], "tasks":[], "workflow_runs":[]
        }))
        .unwrap();
        let domains = domain_rows(&[domain]);
        assert_eq!(domains[0].title, "payments");
        assert_eq!(domains[0].target.as_deref(), Some("project-a"));
        assert!(domains[0].detail.contains("services/payments"));
    }
}
