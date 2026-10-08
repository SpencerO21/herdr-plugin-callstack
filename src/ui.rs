use crate::model::{Frame, Record, Scope, clean};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Prev,
    Next,
    Details,
    Open,
    Diff,
    Fold,
    Expand,
    Collapse,
    Up,
    Down,
    PageUp,
    PageDown,
    Left,
    Right,
    Delete,
    Confirm,
    Cancel,
    Quit,
}
#[derive(Debug)]
pub enum Effect {
    None,
    Quit,
    Open(PathBuf, String),
    Diff(PathBuf, String),
    Delete(String),
}
#[derive(Clone, Debug)]
pub struct Row {
    pub path: Vec<usize>,
    pub id: String,
    pub depth: usize,
}
#[derive(Clone, Debug)]
pub enum Target {
    Action(Action),
    Row(usize),
    Fold(usize),
}
#[derive(Clone, Debug)]
pub struct Hit {
    pub x: usize,
    pub end: usize,
    pub y: usize,
    pub target: Target,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub color: u8,
}
pub struct Screen {
    pub lines: Vec<Line>,
    pub hits: Vec<Hit>,
}

pub struct View {
    pub scope: Scope,
    pub records: Vec<Arc<Record>>,
    pub flow_name: Option<String>,
    pub selected: usize,
    pub rows: Vec<Row>,
    pub detail: bool,
    pub detail_offset: usize,
    pub offset: usize,
    pub horizontal: usize,
    pub page: usize,
    pub project: Option<PathBuf>,
    pub message: String,
    pub busy: bool,
    pub confirm: Option<String>,
    folded: HashMap<String, HashSet<String>>,
    last_click: Option<(String, Instant)>,
}

impl View {
    pub fn new(scope: Scope, project: Option<PathBuf>) -> Self {
        Self {
            scope,
            records: vec![],
            flow_name: None,
            selected: 0,
            rows: vec![],
            detail: false,
            detail_offset: 0,
            offset: 0,
            horizontal: 0,
            page: 1,
            project,
            message: String::new(),
            busy: false,
            confirm: None,
            folded: HashMap::new(),
            last_click: None,
        }
    }
    pub fn record(&self) -> Option<&Record> {
        self.records
            .iter()
            .find(|r| Some(&r.flow.name) == self.flow_name.as_ref())
            .map(AsRef::as_ref)
    }
    pub fn frame(&self, row: usize) -> Option<&Frame> {
        let path = &self.rows.get(row)?.path;
        let mut frames = &self.record()?.flow.frames;
        let mut found = None;
        for i in path {
            let frame = frames.get(*i)?;
            found = Some(frame);
            frames = &frame.calls;
        }
        found
    }
    pub fn update(&mut self, records: Vec<Arc<Record>>) {
        let previous = self.flow_name.clone();
        self.records = records;
        if !self
            .records
            .iter()
            .any(|r| Some(&r.flow.name) == self.flow_name.as_ref())
        {
            self.flow_name = self.records.first().map(|r| r.flow.name.clone());
        }
        if previous != self.flow_name {
            self.selected = 0;
            self.offset = 0;
            self.confirm = None;
        }
        self.last_click = None;
        self.rebuild();
    }
    pub fn rebuild(&mut self) {
        fn walk(
            items: &[Frame],
            path: &mut Vec<usize>,
            folded: &HashSet<String>,
            out: &mut Vec<Row>,
        ) {
            for (i, frame) in items.iter().enumerate() {
                path.push(i);
                let id = path.iter().map(|i| format!("/{i}")).collect::<String>();
                out.push(Row {
                    path: path.clone(),
                    id: id.clone(),
                    depth: path.len() - 1,
                });
                if !folded.contains(&id) {
                    walk(&frame.calls, path, folded, out);
                }
                path.pop();
            }
        }
        let mut rows = vec![];
        if let Some(record) = self.record() {
            let empty = HashSet::new();
            let set = self.folded.get(&record.flow.name).unwrap_or(&empty);
            walk(&record.flow.frames, &mut vec![], set, &mut rows);
        }
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }
    pub fn action(&mut self, action: Action) -> Effect {
        if !matches!(action, Action::Confirm | Action::Delete) {
            self.confirm = None;
        }
        match action {
            Action::Quit => return Effect::Quit,
            Action::Prev | Action::Next if !self.records.is_empty() => {
                let index = self
                    .records
                    .iter()
                    .position(|r| Some(&r.flow.name) == self.flow_name.as_ref())
                    .unwrap_or(0);
                let step = if action == Action::Next {
                    1
                } else {
                    self.records.len() - 1
                };
                self.flow_name = Some(
                    self.records[(index + step) % self.records.len()]
                        .flow
                        .name
                        .clone(),
                );
                self.selected = 0;
                self.offset = 0;
                self.detail_offset = 0;
                self.horizontal = 0;
                self.last_click = None;
                self.rebuild();
            }
            Action::Details => {
                self.detail = !self.detail;
                self.detail_offset = 0;
            }
            Action::Fold => {
                if self
                    .frame(self.selected)
                    .is_some_and(|f| !f.calls.is_empty())
                {
                    let id = self.rows[self.selected].id.clone();
                    let set = self
                        .folded
                        .entry(self.flow_name.clone().unwrap())
                        .or_default();
                    if !set.remove(&id) {
                        set.insert(id);
                    }
                    self.rebuild();
                }
            }
            Action::Expand => {
                if let Some(name) = &self.flow_name {
                    self.folded.remove(name);
                    self.rebuild();
                }
            }
            Action::Collapse => {
                if let Some(name) = self.flow_name.clone() {
                    self.folded.remove(&name);
                    self.rebuild();
                    let ids = self
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| self.frame(*i).is_some_and(|f| !f.calls.is_empty()))
                        .map(|(_, row)| row.id.clone())
                        .collect();
                    self.folded.insert(name, ids);
                    self.rebuild();
                }
            }
            Action::Up | Action::Down | Action::PageUp | Action::PageDown => {
                let step = if matches!(action, Action::PageUp | Action::PageDown) {
                    self.page
                } else {
                    1
                };
                let up = matches!(action, Action::Up | Action::PageUp);
                let value = if self.detail {
                    &mut self.detail_offset
                } else {
                    &mut self.selected
                };
                *value = if up {
                    value.saturating_sub(step)
                } else {
                    value.saturating_add(step)
                };
                self.selected = self.selected.min(self.rows.len().saturating_sub(1));
            }
            Action::Left => self.horizontal = self.horizontal.saturating_sub(12),
            Action::Right => self.horizontal = self.horizontal.saturating_add(12).min(2000),
            Action::Delete if !self.busy => self.confirm = self.flow_name.clone(),
            Action::Confirm if !self.busy => {
                if let Some(name) = self.confirm.take() {
                    self.busy = true;
                    self.message = "Removing flow…".into();
                    return Effect::Delete(name);
                }
            }
            Action::Open | Action::Diff if !self.busy => {
                let loc = self.frame(self.selected).and_then(|f| f.loc.clone());
                let project = self.project.clone().or_else(|| {
                    self.record()
                        .and_then(|r| r.project_root.as_ref().map(PathBuf::from))
                });
                match (project, loc) {
                    (Some(project), Some(loc)) => {
                        self.busy = true;
                        self.message = if action == Action::Diff {
                            "Opening DiffNav…"
                        } else {
                            "Opening Neovim…"
                        }
                        .into();
                        return if action == Action::Diff {
                            Effect::Diff(project, loc)
                        } else {
                            Effect::Open(project, loc)
                        };
                    }
                    (_, None) => self.message = "This call has no source location.".into(),
                    _ => self.message = "Set the project folder with --project PATH.".into(),
                }
            }
            _ => {}
        }
        Effect::None
    }
    pub fn event(&mut self, event: Event, screen: &Screen, now: Instant) -> Effect {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let action = match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        Some(Action::Quit)
                    }
                    KeyCode::Char('q') => Some(Action::Quit),
                    KeyCode::Char('o') => Some(Action::Open),
                    KeyCode::Char('g') => Some(Action::Diff),
                    KeyCode::Char('d') => Some(Action::Details),
                    KeyCode::Enter | KeyCode::Char(' ') => Some(Action::Fold),
                    KeyCode::Up | KeyCode::Char('k') => Some(Action::Up),
                    KeyCode::Down | KeyCode::Char('j') => Some(Action::Down),
                    KeyCode::PageUp => Some(Action::PageUp),
                    KeyCode::PageDown => Some(Action::PageDown),
                    KeyCode::Left => Some(Action::Left),
                    KeyCode::Right => Some(Action::Right),
                    KeyCode::Tab => Some(Action::Next),
                    KeyCode::BackTab => Some(Action::Prev),
                    KeyCode::Esc => Some(Action::Cancel),
                    _ => None,
                };
                if let Some(action) = action {
                    return self.action(action);
                }
            }
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => return self.action(Action::Up),
                    MouseEventKind::ScrollDown => return self.action(Action::Down),
                    MouseEventKind::ScrollLeft => return self.action(Action::Left),
                    MouseEventKind::ScrollRight => return self.action(Action::Right),
                    MouseEventKind::Down(MouseButton::Left) => {}
                    _ => return Effect::None,
                }
                let hit = screen.hits.iter().find(|h| {
                    h.y == mouse.row as usize
                        && mouse.column as usize >= h.x
                        && (mouse.column as usize) < h.end
                });
                match hit.map(|h| &h.target) {
                    Some(Target::Action(a)) => {
                        self.last_click = None;
                        return self.action(*a);
                    }
                    Some(Target::Fold(i)) => {
                        self.selected = *i;
                        self.last_click = None;
                        return self.action(Action::Fold);
                    }
                    Some(Target::Row(i)) => {
                        self.selected = *i;
                        self.confirm = None;
                        let id = format!(
                            "{}:{}",
                            self.flow_name.as_deref().unwrap_or(""),
                            self.rows[*i].id
                        );
                        if self.last_click.as_ref().is_some_and(|(old, time)| {
                            *old == id && now.duration_since(*time) < Duration::from_millis(400)
                        }) {
                            self.last_click = None;
                            return self.action(Action::Open);
                        }
                        self.last_click = Some((id, now));
                    }
                    None => self.last_click = None,
                }
            }
            _ => {}
        }
        Effect::None
    }

    pub fn render(&mut self, width: usize, height: usize) -> Screen {
        let width = width.saturating_sub(1);
        let mut screen = Screen {
            lines: vec![],
            hits: vec![],
        };
        if width < 28 || height < 16 {
            screen.lines.push(Line {
                text: clip("Enlarge pane to 29 × 16.", 0, width),
                color: 0,
            });
            screen.lines.push(Line {
                text: clip("[Quit]", 0, width),
                color: 0,
            });
            if height > 1 {
                screen.hits.push(Hit {
                    x: 0,
                    end: 6.min(width),
                    y: 1,
                    target: Target::Action(Action::Quit),
                });
            }
            screen.lines.truncate(height);
            return screen;
        }
        let title = self
            .record()
            .map(|r| {
                format!(
                    "{}/{} {} [{}]{}{}",
                    self.records
                        .iter()
                        .position(|a| a.flow.name == r.flow.name)
                        .unwrap_or(0)
                        + 1,
                    self.records.len(),
                    r.flow.name,
                    r.flow.status(),
                    if r.drifted_paths.is_empty() {
                        String::new()
                    } else {
                        format!(" [SOURCE CHANGED: {}]", r.drifted_paths.len())
                    },
                    if r.warnings.is_empty() {
                        if r.source_hashes.is_none() {
                            " [UNTRACKED]".into()
                        } else {
                            String::new()
                        }
                    } else {
                        format!(" [WARNINGS: {}]", r.warnings.len())
                    }
                )
            })
            .unwrap_or_else(|| "No flows. Ask your agent to publish one.".into());
        screen.lines.push(Line {
            text: format!(
                "Call stacks | {} | {}",
                self.scope.workspace, self.scope.session
            ),
            color: 0,
        });
        screen.lines.push(Line {
            text: title,
            color: 0,
        });
        let buttons = if self.confirm.is_some() {
            vec![("Yes, delete", Action::Confirm), ("Cancel", Action::Cancel)]
        } else {
            vec![
                ("Prev", Action::Prev),
                ("Next", Action::Next),
                (
                    if self.detail { "Back" } else { "Details" },
                    Action::Details,
                ),
                ("Open nvim", Action::Open),
                ("Diff dn", Action::Diff),
                ("Fold", Action::Fold),
                ("Expand all", Action::Expand),
                ("Fold all", Action::Collapse),
                ("Up", Action::Up),
                ("Down", Action::Down),
                ("Page up", Action::PageUp),
                ("Page down", Action::PageDown),
                ("Left", Action::Left),
                ("Right", Action::Right),
                ("Delete", Action::Delete),
                ("Quit", Action::Quit),
            ]
        };
        let mut line = String::new();
        for (label, action) in buttons {
            let text = format!("[{label}]");
            if !line.is_empty() && line.len() + text.len() > width {
                screen.lines.push(Line {
                    text: line,
                    color: 0,
                });
                line = String::new();
            }
            screen.hits.push(Hit {
                x: line.len(),
                end: line.len() + text.len(),
                y: screen.lines.len(),
                target: Target::Action(action),
            });
            line.push_str(&text);
            line.push(' ');
        }
        if !line.is_empty() {
            screen.lines.push(Line {
                text: line,
                color: 0,
            });
        }
        if let Some(name) = &self.confirm {
            screen.lines.push(Line {
                text: format!("Delete {name}? No undo is available."),
                color: 33,
            });
        }
        self.page = height.saturating_sub(screen.lines.len() + 3).max(1);
        if self.detail {
            let mut raw = vec![];
            if let Some(record) = self.record() {
                if record.source_hashes.is_none() {
                    raw.push(
                        "Source tracking unavailable. Republish this flow with a project folder."
                            .into(),
                    );
                }
                for path in &record.drifted_paths {
                    raw.push(format!(
                        "SOURCE CHANGED: {path}. Verify the code and republish."
                    ));
                }
                raw.extend(record.warnings.iter().map(|s| format!("Warning: {s}")));
                if let Some(d) = &record.flow.description {
                    raw.push(d.clone());
                }
                if let Some(frame) = self.frame(self.selected) {
                    raw.push(frame.function.clone());
                    raw.extend(frame.details());
                }
                raw.extend(
                    record
                        .flow
                        .types
                        .iter()
                        .map(|(k, v)| format!("type {k}: {v}")),
                );
            }
            let wrapped: Vec<_> = raw.iter().flat_map(|s| wrap(s, width)).collect();
            self.detail_offset = self
                .detail_offset
                .min(wrapped.len().saturating_sub(self.page));
            screen.lines.extend(
                wrapped
                    .into_iter()
                    .skip(self.detail_offset)
                    .take(self.page)
                    .map(|text| Line { text, color: 0 }),
            );
        } else {
            if self.selected < self.offset {
                self.offset = self.selected;
            }
            if self.selected >= self.offset + self.page {
                self.offset = self.selected + 1 - self.page;
            }
            self.offset = self.offset.min(self.rows.len().saturating_sub(self.page));
            for (i, row) in self
                .rows
                .iter()
                .enumerate()
                .skip(self.offset)
                .take(self.page)
            {
                let f = self.frame(i).unwrap();
                let prefix = format!(
                    "{} {}",
                    if self.selected == i { ">" } else { " " },
                    "  ".repeat(row.depth)
                );
                let folded = self
                    .flow_name
                    .as_ref()
                    .and_then(|n| self.folded.get(n))
                    .is_some_and(|s| s.contains(&row.id));
                let fold = if f.calls.is_empty() {
                    "   "
                } else if folded {
                    "[+]"
                } else {
                    "[-]"
                };
                let text = format!(
                    "{prefix}{fold} {} {}{}{}{}",
                    f.marker(),
                    f.function,
                    if f.concurrent == Some(true) {
                        " [parallel]"
                    } else {
                        ""
                    },
                    f.loc.as_ref().map(|s| format!("  {s}")).unwrap_or_default(),
                    if f.loc
                        .as_ref()
                        .and_then(|loc| crate::host::location_parts(loc).ok())
                        .is_some_and(|(path, _)| self
                            .record()
                            .unwrap()
                            .drifted_paths
                            .iter()
                            .any(|p| p == path))
                    {
                        " [SOURCE CHANGED]"
                    } else {
                        ""
                    }
                );
                let y = screen.lines.len();
                if !f.calls.is_empty() {
                    let start = prefix.len();
                    let end = start + 3;
                    if end > self.horizontal && start < self.horizontal + width {
                        screen.hits.push(Hit {
                            x: start.saturating_sub(self.horizontal),
                            end: end.saturating_sub(self.horizontal).min(width),
                            y,
                            target: Target::Fold(i),
                        });
                    }
                }
                screen.hits.push(Hit {
                    x: 0,
                    end: width,
                    y,
                    target: Target::Row(i),
                });
                screen.lines.push(Line {
                    text: clip(&text, self.horizontal, width),
                    color: match f.marker() {
                        '+' => 32,
                        '~' => 33,
                        '-' => 31,
                        _ => 0,
                    },
                });
            }
        }
        while screen.lines.len() < height - 3 {
            screen.lines.push(Line {
                text: String::new(),
                color: 0,
            });
        }
        screen.lines.push(Line {
            text: "= same | + added | ~ modified | - removed".into(),
            color: 0,
        });
        screen.lines.push(Line {
            text: self.message.clone(),
            color: 0,
        });
        screen.lines.push(Line {
            text: "Click: select | Double-click: source | Wheel: scroll".into(),
            color: 0,
        });
        screen.lines.truncate(height);
        for line in &mut screen.lines {
            line.text = clip(&line.text, 0, width);
        }
        screen
    }
}

pub fn clip(text: &str, skip: usize, width: usize) -> String {
    let mut position = 0;
    let mut output = String::new();
    for c in clean(text).chars() {
        let size = c.width().unwrap_or(0);
        if position >= skip && position + size <= skip + width {
            output.push(c);
        } else if position < skip && position + size > skip {
            output.push(' ');
        }
        position += size;
        if position >= skip + width {
            break;
        }
    }
    output
}
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let text = clean(text);
    if text.is_empty() {
        return vec![String::new()];
    }
    let length = text.width();
    (0..length)
        .step_by(width.max(1))
        .map(|offset| clip(&text, offset, width))
        .collect()
}
