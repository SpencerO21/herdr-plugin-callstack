use crate::graph::Graph;
use crate::model::{Frame, Record, Scope, clean};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Paragraph,
};
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
    More,
    Tree,
    Diagram,
    Combined,
    Size,
    Center,
    NodePrev,
    NodeNext,
    Version,
    DetailUp,
    DetailDown,
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
    Node(usize),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Tree,
    Diagram,
    Combined,
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
    pub diagram: Option<(usize, Buffer)>,
}

pub fn style(color: u8) -> Style {
    match color {
        1 => Style::default().add_modifier(Modifier::BOLD),
        7 => Style::default().add_modifier(Modifier::REVERSED),
        31 => Style::default().fg(Color::Red),
        32 => Style::default().fg(Color::Green),
        33 => Style::default().fg(Color::Yellow),
        36 => Style::default().fg(Color::Cyan),
        90 => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}
impl Screen {
    pub fn draw(&self, frame: &mut ratatui::Frame<'_>, colors: bool) {
        let area = frame.area();
        for (y, line) in self.lines.iter().take(usize::from(area.height)).enumerate() {
            frame.render_widget(
                Paragraph::new(line.text.as_str()).style(if colors {
                    style(line.color)
                } else {
                    Style::default()
                }),
                Rect::new(area.x, area.y + y as u16, area.width, 1),
            );
        }
        if let Some((top, buffer)) = &self.diagram {
            for y in 0..buffer.area.height {
                for x in 0..buffer.area.width {
                    if x < area.width && *top + usize::from(y) < usize::from(area.height) {
                        let mut cell = buffer[(x, y)].clone();
                        if !colors {
                            cell.set_style(Style::reset());
                        }
                        frame.buffer_mut()[(area.x + x, area.y + *top as u16 + y)] = cell;
                    }
                }
            }
        }
    }
}

pub struct View {
    pub scope: Scope,
    pub records: Vec<Arc<Record>>,
    pub flow_name: Option<String>,
    pub selected: usize,
    pub rows: Vec<Row>,
    pub detail: bool,
    pub more: bool,
    pub mode: Mode,
    pub graph: Graph,
    pub graph_selected: usize,
    pub version: usize,
    pub expanded: bool,
    pub camera: (usize, usize),
    graph_area: (usize, usize, usize),
    center_graph: bool,
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
            more: false,
            mode: Mode::Tree,
            graph: Graph::default(),
            graph_selected: 0,
            version: 0,
            expanded: false,
            camera: (0, 0),
            graph_area: (0, 0, 0),
            center_graph: true,
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
    pub fn selected_frame(&self) -> Option<&Frame> {
        if self.mode == Mode::Tree {
            self.frame(self.selected)
        } else {
            self.graph
                .nodes
                .get(self.graph_selected)?
                .occurrences
                .get(self.version)
                .map(|o| o.frame())
        }
    }
    pub fn selected_record(&self) -> Option<&Record> {
        if self.mode == Mode::Tree {
            self.record()
        } else {
            self.graph
                .nodes
                .get(self.graph_selected)?
                .occurrences
                .get(self.version)
                .map(|o| o.record.as_ref())
        }
    }
    fn rebuild_graph(&mut self) {
        if self.mode == Mode::Tree {
            return;
        }
        let old = self
            .graph
            .nodes
            .get(self.graph_selected)
            .map(|n| n.key.clone());
        let old_version = self
            .graph
            .nodes
            .get(self.graph_selected)
            .and_then(|n| n.occurrences.get(self.version))
            .map(|o| (o.record.flow.name.clone(), o.path.clone()));
        let records: Vec<_> = self
            .records
            .iter()
            .filter(|r| {
                self.mode == Mode::Combined || Some(&r.flow.name) == self.flow_name.as_ref()
            })
            .cloned()
            .collect();
        self.graph = Graph::build(&records, self.mode == Mode::Combined, self.expanded);
        self.graph_selected = self
            .graph
            .nodes
            .iter()
            .position(|n| Some(&n.key) == old.as_ref())
            .unwrap_or(0);
        self.version = self
            .graph
            .nodes
            .get(self.graph_selected)
            .and_then(|n| {
                n.occurrences.iter().position(|o| {
                    old_version
                        .as_ref()
                        .is_some_and(|(name, path)| *name == o.record.flow.name && *path == o.path)
                })
            })
            .unwrap_or(0);
        self.last_click = None;
        self.center_graph = true;
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
        self.rebuild_graph();
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
            Action::DetailUp => self.detail_offset = self.detail_offset.saturating_sub(3),
            Action::DetailDown => self.detail_offset = self.detail_offset.saturating_add(3),
            Action::Tree | Action::Diagram | Action::Combined => {
                if self.mode == Mode::Combined && action != Action::Combined {
                    self.flow_name = self.selected_record().map(|r| r.flow.name.clone());
                    self.selected = 0;
                    self.offset = 0;
                    self.rebuild();
                }
                self.mode = match action {
                    Action::Diagram => Mode::Diagram,
                    Action::Combined => Mode::Combined,
                    _ => Mode::Tree,
                };
                self.more = false;
                self.detail_offset = 0;
                self.rebuild_graph();
                self.last_click = None;
            }
            Action::Size => {
                self.expanded = !self.expanded;
                self.graph.layout(self.expanded);
                self.center_graph = true;
            }
            Action::Center => self.center_graph = true,
            Action::NodePrev | Action::NodeNext if !self.graph.nodes.is_empty() => {
                let count = self.graph.nodes.len();
                self.graph_selected = (self.graph_selected
                    + if action == Action::NodeNext {
                        1
                    } else {
                        count - 1
                    })
                    % count;
                self.version = 0;
                self.detail_offset = 0;
                self.center_graph = true;
                self.last_click = None;
            }
            Action::Version => {
                if let Some(node) = self.graph.nodes.get(self.graph_selected) {
                    self.version = (self.version + 1) % node.occurrences.len();
                    self.detail = true;
                    self.detail_offset = 0;
                }
            }
            Action::Up
            | Action::Down
            | Action::Left
            | Action::Right
            | Action::PageUp
            | Action::PageDown
                if self.mode != Mode::Tree =>
            {
                match action {
                    Action::Up => self.camera.1 = self.camera.1.saturating_sub(3),
                    Action::Down => self.camera.1 = self.camera.1.saturating_add(3),
                    Action::PageUp => {
                        self.camera.1 = self.camera.1.saturating_sub(self.graph_area.2)
                    }
                    Action::PageDown => {
                        self.camera.1 = self.camera.1.saturating_add(self.graph_area.2)
                    }
                    Action::Left => self.camera.0 = self.camera.0.saturating_sub(8),
                    Action::Right => self.camera.0 = self.camera.0.saturating_add(8),
                    _ => {}
                }
                self.center_graph = false;
            }
            Action::More => self.more = !self.more,
            Action::Cancel => self.more = false,
            Action::Quit => return Effect::Quit,
            Action::Prev | Action::Next if !self.records.is_empty() => {
                if self.mode == Mode::Combined {
                    self.mode = Mode::Diagram;
                }
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
                self.rebuild_graph();
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
            Action::Delete if !self.busy => {
                self.confirm = self.selected_record().map(|r| r.flow.name.clone())
            }
            Action::Confirm if !self.busy => {
                if let Some(name) = self.confirm.take() {
                    self.busy = true;
                    self.message = "Removing flow…".into();
                    return Effect::Delete(name);
                }
            }
            Action::Open | Action::Diff if !self.busy => {
                let loc = self.selected_frame().and_then(|f| f.loc.clone());
                let project = self.project.clone().or_else(|| {
                    self.selected_record()
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
                    KeyCode::Char('m') => Some(Action::More),
                    KeyCode::Char('1') => Some(Action::Tree),
                    KeyCode::Char('2') => Some(Action::Diagram),
                    KeyCode::Char('3') => Some(Action::Combined),
                    KeyCode::Char('z') => Some(Action::Size),
                    KeyCode::Char('c') => Some(Action::Center),
                    KeyCode::Char('n') => Some(Action::NodeNext),
                    KeyCode::Char('p') => Some(Action::NodePrev),
                    KeyCode::Char('v') => Some(Action::Version),
                    KeyCode::Enter | KeyCode::Char(' ') => Some(if self.mode == Mode::Tree {
                        Action::Fold
                    } else {
                        Action::Details
                    }),
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
                if self.mode != Mode::Tree
                    && self.detail
                    && mouse.row as usize >= self.graph_area.0 + self.graph_area.2
                    && (mouse.row as usize) < screen.lines.len().saturating_sub(3)
                {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            self.detail_offset = self.detail_offset.saturating_sub(3);
                            return Effect::None;
                        }
                        MouseEventKind::ScrollDown => {
                            self.detail_offset = self.detail_offset.saturating_add(3);
                            return Effect::None;
                        }
                        _ => {}
                    }
                }
                match mouse.kind {
                    MouseEventKind::ScrollUp
                        if self.mode != Mode::Tree
                            && mouse.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        return self.action(Action::Left);
                    }
                    MouseEventKind::ScrollDown
                        if self.mode != Mode::Tree
                            && mouse.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        return self.action(Action::Right);
                    }
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
                    Some(Target::Node(index)) => {
                        self.confirm = None;
                        if self.graph_selected != *index {
                            self.version = 0;
                        }
                        self.graph_selected = *index;
                        self.detail = true;
                        self.detail_offset = 0;
                        let id = self.graph.nodes[*index].key.clone();
                        if self.last_click.as_ref().is_some_and(|(old, time)| {
                            *old == id && now.duration_since(*time) < Duration::from_millis(400)
                        }) {
                            self.last_click = None;
                            return self.action(Action::Open);
                        }
                        self.last_click = Some((id, now));
                    }
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
                        self.detail_offset = 0;
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
            diagram: None,
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
        if self.mode != Mode::Tree {
            return self.render_graph(width, height);
        }
        let title = self
            .record()
            .map(|r| {
                format!(
                    "{}/{} {}  |  {}",
                    self.records
                        .iter()
                        .position(|a| a.flow.name == r.flow.name)
                        .unwrap_or(0)
                        + 1,
                    self.records.len(),
                    r.flow.name,
                    r.flow.status()
                )
            })
            .unwrap_or_else(|| "No flows. Ask your agent to publish one.".into());
        screen.lines.push(Line {
            text: format!(
                "Call stacks | {} | {}",
                self.scope.workspace, self.scope.session
            ),
            color: 90,
        });
        screen.lines.push(Line {
            text: title,
            color: 1,
        });
        if let Some(record) = self.record() {
            screen.lines.push(Line {
                text: format!(
                    "{}  |  {} warnings",
                    if !record.drifted_paths.is_empty() {
                        format!("SOURCE CHANGED: {}", record.drifted_paths.len())
                    } else if record.source_hashes.is_none() {
                        "Source not tracked".into()
                    } else {
                        "Source unchanged".into()
                    },
                    record.warnings.len()
                ),
                color: if record.drifted_paths.is_empty() && record.warnings.is_empty() {
                    90
                } else {
                    33
                },
            });
        }
        let buttons = if self.confirm.is_some() {
            vec![("Yes, delete", Action::Confirm), ("Cancel", Action::Cancel)]
        } else if self.more {
            vec![
                ("Back", Action::More),
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
        } else {
            vec![
                ("Prev", Action::Prev),
                ("Next", Action::Next),
                (
                    if self.detail {
                        "Hide details"
                    } else {
                        "Details"
                    },
                    Action::Details,
                ),
                ("Open nvim", Action::Open),
                ("Diff dn", Action::Diff),
                ("More", Action::More),
                ("Diagram", Action::Diagram),
                ("Combined", Action::Combined),
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
        let body_start = screen.lines.len();
        let available = height.saturating_sub(body_start + 5);
        self.page = if self.detail {
            (available / 2).max(1).min(self.rows.len().max(1))
        } else {
            available.max(1)
        };
        let detail_page = if self.detail {
            available.saturating_sub(self.page)
        } else {
            0
        };
        screen.lines.push(Line {
            text: format!("CALL STACK  |  {} calls", self.rows.len()),
            color: 90,
        });
        self.render_tree(&mut screen, width);
        while screen.lines.len() < body_start + 1 + self.page {
            screen.lines.push(Line {
                text: String::new(),
                color: 0,
            });
        }
        screen.lines.push(Line {
            text: self
                .frame(self.selected)
                .map(|f| {
                    format!(
                        "Source: {}",
                        f.loc.as_deref().unwrap_or("No source location")
                    )
                })
                .unwrap_or_default(),
            color: 90,
        });
        if self.detail {
            let mut raw = vec![];
            if let Some(record) = self.record() {
                if let Some(frame) = self.frame(self.selected) {
                    raw.push(format!("DETAILS  |  {}", frame.function));
                    raw.extend(frame.details());
                    raw.push(String::new());
                }
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
                .min(wrapped.len().saturating_sub(detail_page));
            screen.lines.extend(
                wrapped
                    .into_iter()
                    .skip(self.detail_offset)
                    .take(detail_page)
                    .map(|text| Line { text, color: 0 }),
            );
        }
        while screen.lines.len() < height - 3 {
            screen.lines.push(Line {
                text: String::new(),
                color: 0,
            });
        }
        screen.lines.push(Line {
            text: "= same | + added | ~ modified | - removed".into(),
            color: 90,
        });
        screen.lines.push(Line {
            text: self.message.clone(),
            color: 0,
        });
        screen.lines.push(Line {
            text: if self.detail {
                "Click call: details | Wheel: details"
            } else {
                "Click: select | Double-click: source"
            }
            .into(),
            color: 90,
        });
        screen.lines.truncate(height);
        screen.hits.retain(|h| h.y < height.saturating_sub(3));
        for line in &mut screen.lines {
            line.text = clip(&line.text, 0, width);
        }
        screen
    }

    fn render_graph(&mut self, width: usize, height: usize) -> Screen {
        let mut screen = Screen {
            lines: vec![],
            hits: vec![],
            diagram: None,
        };
        if height < 24 {
            screen.lines.push(Line {
                text: "Diagram needs 29 x 24 cells.".into(),
                color: 0,
            });
            buttons(
                &mut screen,
                width,
                &[("Tree", Action::Tree), ("Quit", Action::Quit)],
            );
            return screen;
        }
        screen.lines.push(Line {
            text: if self.mode == Mode::Combined {
                format!(
                    "Combined | {} flows | {} functions",
                    self.records.len(),
                    self.graph.nodes.len()
                )
            } else {
                format!(
                    "Diagram | {}",
                    self.flow_name.as_deref().unwrap_or("No flow")
                )
            },
            color: 1,
        });
        screen.lines.push(Line {
            text: format!(
                "{} | {} | {}",
                self.scope.workspace,
                self.scope.session,
                if self.expanded { "Expanded" } else { "Compact" }
            ),
            color: 90,
        });
        if self.confirm.is_some() {
            buttons(
                &mut screen,
                width,
                &[("Yes, delete", Action::Confirm), ("Cancel", Action::Cancel)],
            );
            screen.lines.push(Line {
                text: format!("Delete {}? No undo.", self.confirm.as_deref().unwrap()),
                color: 33,
            });
        } else if self.more {
            buttons(
                &mut screen,
                width,
                &[
                    ("Back", Action::More),
                    ("Left", Action::Left),
                    ("Right", Action::Right),
                    ("Up", Action::Up),
                    ("Down", Action::Down),
                    ("Page up", Action::PageUp),
                    ("Page down", Action::PageDown),
                    ("Center", Action::Center),
                    ("Prev call", Action::NodePrev),
                    ("Next call", Action::NodeNext),
                    ("Prev flow", Action::Prev),
                    ("Next flow", Action::Next),
                    ("Details up", Action::DetailUp),
                    ("Details down", Action::DetailDown),
                    ("Delete", Action::Delete),
                    ("Quit", Action::Quit),
                ],
            );
        } else {
            buttons(
                &mut screen,
                width,
                &[
                    ("Tree", Action::Tree),
                    ("Diagram", Action::Diagram),
                    ("Combined", Action::Combined),
                    (
                        if self.expanded { "Compact" } else { "Expanded" },
                        Action::Size,
                    ),
                    (
                        if self.detail {
                            "Hide details"
                        } else {
                            "Details"
                        },
                        Action::Details,
                    ),
                    ("Open nvim", Action::Open),
                    ("Diff dn", Action::Diff),
                    ("Version", Action::Version),
                    ("More", Action::More),
                ],
            );
        }
        let available = height.saturating_sub(screen.lines.len() + 4);
        let detail_height = if self.detail && available >= 10 {
            (available / 3).clamp(4, 12)
        } else {
            0
        };
        let graph_height = available.saturating_sub(detail_height);
        let top = screen.lines.len();
        self.graph_area = (top, width, graph_height);
        if self.center_graph {
            if let Some(node) = self.graph.nodes.get(self.graph_selected) {
                self.camera = (
                    (node.x + self.graph.node_width / 2).saturating_sub(width / 2),
                    node.y.saturating_sub(1),
                );
            }
            self.center_graph = false;
        }
        self.camera.0 = self.camera.0.min(self.graph.width.saturating_sub(width));
        self.camera.1 = self
            .camera
            .1
            .min(self.graph.height.saturating_sub(graph_height));
        let buffer = self.graph.draw(
            width as u16,
            graph_height as u16,
            self.camera,
            (self.graph_selected, self.version),
            self.expanded,
            std::env::var_os("NO_COLOR").is_none(),
        );
        for y in 0..buffer.area.height {
            screen.lines.push(Line {
                text: (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect(),
                color: 0,
            });
        }
        for (index, node) in self.graph.nodes.iter().enumerate() {
            let left = node.x.max(self.camera.0);
            let right = (node.x + self.graph.node_width).min(self.camera.0 + width);
            let start = node.y.max(self.camera.1);
            let end = (node.y + self.graph.node_height).min(self.camera.1 + graph_height);
            if left < right {
                for y in start..end {
                    screen.hits.push(Hit {
                        x: left - self.camera.0,
                        end: right - self.camera.0,
                        y: top + y - self.camera.1,
                        target: Target::Node(index),
                    });
                }
            }
        }
        screen.diagram = Some((top, buffer));
        let mut details = vec![];
        if let Some(node) = self.graph.nodes.get(self.graph_selected) {
            let occurrence = &node.occurrences[self.version];
            let frame = occurrence.frame();
            details.push(format!(
                "{} | {} | version {}/{}",
                frame.function,
                occurrence.record.flow.name,
                self.version + 1,
                node.occurrences.len()
            ));
            details.push(format!(
                "{} | {}",
                occurrence.record.flow.status(),
                if occurrence.changed() {
                    "SOURCE CHANGED"
                } else {
                    "Saved flow"
                }
            ));
            details.extend(frame.details());
            details.extend(
                occurrence
                    .record
                    .warnings
                    .iter()
                    .map(|w| format!("Warning: {w}")),
            );
            details.extend(
                occurrence
                    .record
                    .flow
                    .types
                    .iter()
                    .map(|(k, v)| format!("type {k}: {v}")),
            );
        } else {
            details.push("No flows. Ask your agent to publish one.".into());
        }
        screen.lines.push(Line {
            text: details.first().cloned().unwrap_or_default(),
            color: 1,
        });
        if detail_height > 0 {
            let wrapped: Vec<_> = details
                .iter()
                .skip(1)
                .flat_map(|s| wrap(s, width))
                .collect();
            self.detail_offset = self
                .detail_offset
                .min(wrapped.len().saturating_sub(detail_height));
            screen.lines.extend(
                wrapped
                    .into_iter()
                    .skip(self.detail_offset)
                    .take(detail_height)
                    .map(|text| Line { text, color: 0 }),
            );
        }
        while screen.lines.len() < height - 3 {
            screen.lines.push(Line {
                text: String::new(),
                color: 0,
            });
        }
        screen.lines.push(Line {
            text: "= same | + added | ~ modified | - removed".into(),
            color: 90,
        });
        screen.lines.push(Line {
            text: if self.message.is_empty() {
                format!(
                    "Pan {},{} | dashed: if | double: shared | return: cycle",
                    self.camera.0, self.camera.1
                )
            } else {
                self.message.clone()
            },
            color: 90,
        });
        screen.lines.push(Line {
            text: "Click: details | Double-click: source | Wheel: pan".into(),
            color: 90,
        });
        for line in &mut screen.lines {
            line.text = clip(&line.text, 0, width);
        }
        screen
    }

    fn render_tree(&mut self, screen: &mut Screen, width: usize) {
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
                "{prefix}{fold} {} {}{}{}",
                f.marker(),
                f.function,
                if f.concurrent == Some(true) {
                    " [parallel]"
                } else {
                    ""
                },
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
                    " [source changed]"
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
                color: if self.selected == i {
                    7
                } else {
                    match f.marker() {
                        '+' => 32,
                        '~' => 33,
                        '-' => 31,
                        _ => 0,
                    }
                },
            });
        }
    }
}

fn buttons(screen: &mut Screen, width: usize, items: &[(&str, Action)]) {
    let mut line = String::new();
    for (label, action) in items {
        let text = format!("[{label}]");
        if !line.is_empty() && line.len() + text.len() > width {
            screen.lines.push(Line {
                text: std::mem::take(&mut line),
                color: 0,
            });
        }
        screen.hits.push(Hit {
            x: line.len(),
            end: line.len() + text.len(),
            y: screen.lines.len(),
            target: Target::Action(*action),
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
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() {
            if line.width() + 1 + word.width() > width {
                lines.push(std::mem::take(&mut line));
            } else {
                line.push(' ');
            }
        }
        for c in word.chars() {
            if !line.is_empty() && line.width() + c.width().unwrap_or(0) > width {
                lines.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}
