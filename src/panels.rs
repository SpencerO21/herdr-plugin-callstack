use crate::{
    host::AgentTarget,
    ui::{Action, Effect, Hit, Line, Screen, Target, View, clip},
};
use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Tools,
    Search,
    Types,
    Compose,
    Agents,
    Send,
}
#[derive(Clone, Debug)]
pub enum Choice {
    Action(Action),
    Call { flow: String, path: Vec<usize> },
    Type(String),
    Agent(AgentTarget),
}
#[derive(Clone, Debug)]
pub struct Panel {
    pub kind: PanelKind,
    pub input: String,
    pub selected: usize,
    pub scroll: usize,
    pub items: Vec<(String, Choice)>,
    pub text: Vec<String>,
}
impl Panel {
    pub fn new(kind: PanelKind) -> Self {
        Self {
            kind,
            input: String::new(),
            selected: 0,
            scroll: 0,
            items: vec![],
            text: vec![],
        }
    }
}

impl View {
    pub fn tools_panel(&mut self) {
        let mut panel = Panel::new(PanelKind::Tools);
        panel.items = [
            ("Search functions", Action::Search),
            (
                if self.focus_key.is_some() {
                    "Clear path focus"
                } else {
                    "Focus selected path"
                },
                Action::Focus,
            ),
            (
                if self.changes_only {
                    "Show all calls"
                } else {
                    "Changes only"
                },
                Action::Changes,
            ),
            ("Source preview / Reload", Action::Preview),
            ("Trace a type", Action::Trace),
            ("Clear type trace", Action::ClearTrace),
            ("Module lanes", Action::Lanes),
            (
                if self.history {
                    "Return to active flows"
                } else {
                    "History: archived flows"
                },
                Action::History,
            ),
            (
                if self.history {
                    "Restore selected flow"
                } else {
                    "Archive selected flow"
                },
                Action::Archive,
            ),
            ("Generate a new flow", Action::Generate),
            ("Update selected flow", Action::UpdateFlow),
            ("Clear all filters", Action::ClearFilters),
        ]
        .into_iter()
        .map(|(s, a)| (s.into(), Choice::Action(a)))
        .collect();
        self.panel = Some(panel);
    }
    pub fn search_items(&mut self) {
        let Some(panel) = &mut self.panel else {
            return;
        };
        let query = panel.input.to_lowercase();
        panel.items.clear();
        fn walk(
            frames: &[crate::model::Frame],
            path: &mut Vec<usize>,
            flow: &str,
            query: &str,
            out: &mut Vec<(String, Choice)>,
        ) {
            for (i, f) in frames.iter().enumerate() {
                path.push(i);
                if out.len() < 500 && f.function.to_lowercase().contains(query) {
                    out.push((
                        format!(
                            "{} | {} | {}",
                            f.function,
                            flow,
                            f.loc.as_deref().unwrap_or("no source")
                        ),
                        Choice::Call {
                            flow: flow.into(),
                            path: path.clone(),
                        },
                    ));
                }
                walk(&f.calls, path, flow, query, out);
                path.pop();
            }
        }
        for record in &self.records {
            walk(
                &record.flow.frames,
                &mut vec![],
                &record.flow.name,
                &query,
                &mut panel.items,
            );
        }
        panel.selected = 0;
    }
    pub fn panel_event(&mut self, event: &Event, screen: &Screen) -> Effect {
        let Some(panel) = &mut self.panel else {
            return Effect::None;
        };
        let mut choose = None;
        let mut edited = false;
        let input = matches!(panel.kind, PanelKind::Search | PanelKind::Compose);
        if panel.kind == PanelKind::Send {
            let scroll = match event {
                Event::Key(k) => match k.code {
                    KeyCode::Up | KeyCode::PageUp => Some(false),
                    KeyCode::Down | KeyCode::PageDown => Some(true),
                    _ => None,
                },
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => Some(false),
                    MouseEventKind::ScrollDown => Some(true),
                    MouseEventKind::Down(MouseButton::Left) => screen
                        .hits
                        .iter()
                        .find(|h| {
                            h.y == m.row as usize
                                && h.x <= m.column as usize
                                && h.end > m.column as usize
                        })
                        .and_then(|h| match h.target {
                            Target::Action(Action::PanelUp) => Some(false),
                            Target::Action(Action::PanelDown) => Some(true),
                            _ => None,
                        }),
                    _ => None,
                },
                _ => None,
            };
            if let Some(down) = scroll {
                panel.scroll = if down {
                    panel.scroll.saturating_add(3)
                } else {
                    panel.scroll.saturating_sub(3)
                };
                return Effect::None;
            }
        }
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Esc => {
                    self.panel = None;
                    return Effect::None;
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Effect::Quit;
                }
                KeyCode::Up => panel.selected = panel.selected.saturating_sub(1),
                KeyCode::Down => {
                    panel.selected = (panel.selected + 1).min(panel.items.len().saturating_sub(1))
                }
                KeyCode::PageUp => panel.selected = panel.selected.saturating_sub(10),
                KeyCode::PageDown => {
                    panel.selected = (panel.selected + 10).min(panel.items.len().saturating_sub(1))
                }
                KeyCode::Enter if panel.kind == PanelKind::Compose => {
                    return self.action(Action::FindAgents);
                }
                KeyCode::Enter => choose = Some(panel.selected),
                KeyCode::Backspace if input => {
                    panel.input.pop();
                    edited = true;
                }
                KeyCode::Char(c)
                    if input
                        && !c.is_control()
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && panel.input.len() < 500 =>
                {
                    panel.input.push(c);
                    edited = true;
                }
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => panel.selected = panel.selected.saturating_sub(1),
                MouseEventKind::ScrollDown => {
                    panel.selected = (panel.selected + 1).min(panel.items.len().saturating_sub(1))
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(hit) = screen.hits.iter().find(|h| {
                        h.y == mouse.row as usize
                            && h.x <= mouse.column as usize
                            && h.end > mouse.column as usize
                    }) {
                        match hit.target {
                            Target::PanelItem(i) => choose = Some(i),
                            Target::Action(Action::Cancel) => {
                                self.panel = None;
                                return Effect::None;
                            }
                            Target::Action(Action::PanelClear) => {
                                panel.input.clear();
                                edited = true;
                            }
                            Target::Action(Action::PanelUp) => {
                                panel.selected = panel.selected.saturating_sub(1)
                            }
                            Target::Action(Action::PanelDown) => {
                                panel.selected =
                                    (panel.selected + 1).min(panel.items.len().saturating_sub(1))
                            }
                            Target::Action(a) => return self.action(a),
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        if edited
            && self
                .panel
                .as_ref()
                .is_some_and(|p| p.kind == PanelKind::Search)
        {
            self.search_items();
        }
        if let Some(index) = choose {
            return self.choose_panel(index);
        }
        Effect::None
    }
    fn choose_panel(&mut self, index: usize) -> Effect {
        let choice = self
            .panel
            .as_ref()
            .and_then(|p| p.items.get(index))
            .map(|(_, c)| c.clone());
        match choice {
            Some(Choice::Action(action)) => {
                self.panel = None;
                return self.action(action);
            }
            Some(Choice::Call { flow, path }) => {
                self.panel = None;
                self.jump_to(&flow, &path);
            }
            Some(Choice::Type(name)) => {
                self.panel = None;
                self.trace = Some(name);
                self.refresh_explore();
            }
            Some(Choice::Agent(agent)) => {
                let mut panel = Panel::new(PanelKind::Send);
                panel.text = vec![
                    format!("Agent: {}", agent.label),
                    format!(
                        "Project: {}",
                        self.generation_project
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    ),
                    format!("Request: {}", self.generation_subject),
                    "This sends a read-and-publish request. It may use agent credits.".into(),
                    "It does not authorize source edits, commits, or pushes.".into(),
                ];
                panel.items.push((
                    "Send request".into(),
                    Choice::Action(Action::SendGeneration),
                ));
                self.generation_target = Some(agent);
                self.panel = Some(panel);
            }
            None => {}
        }
        Effect::None
    }
    pub fn render_panel(&mut self, width: usize, height: usize) -> Screen {
        let panel = self.panel.as_mut().unwrap();
        let mut screen = Screen {
            lines: vec![],
            hits: vec![],
            diagram: None,
        };
        let title = match panel.kind {
            PanelKind::Tools => "TOOLS",
            PanelKind::Search => "SEARCH FUNCTIONS",
            PanelKind::Types => "TRACE A TYPE",
            PanelKind::Compose => "GENERATE / UPDATE FLOW",
            PanelKind::Agents => "CHOOSE AN IDLE AGENT",
            PanelKind::Send => "CONFIRM AGENT REQUEST",
        };
        screen.lines.push(Line {
            text: title.into(),
            color: 1,
        });
        crate::ui::buttons(
            &mut screen,
            width,
            &[
                ("Close", Action::Cancel),
                ("Up", Action::PanelUp),
                ("Down", Action::PanelDown),
            ],
        );
        if panel.kind == PanelKind::Send {
            crate::ui::buttons(
                &mut screen,
                width,
                &[("Send request", Action::SendGeneration)],
            );
            let text: Vec<_> = panel
                .text
                .iter()
                .flat_map(|s| crate::ui::wrap(s, width))
                .collect();
            let available = height.saturating_sub(screen.lines.len());
            panel.scroll = panel.scroll.min(text.len().saturating_sub(available));
            screen.lines.extend(
                text.into_iter()
                    .skip(panel.scroll)
                    .take(available)
                    .map(|text| Line { text, color: 0 }),
            );
            for line in &mut screen.lines {
                line.text = clip(&line.text, 0, width);
            }
            return screen;
        }
        if matches!(panel.kind, PanelKind::Search | PanelKind::Compose) {
            screen.lines.push(Line {
                text: format!(
                    "> {}_",
                    clip(
                        &panel.input,
                        panel.input.width().saturating_sub(width.saturating_sub(3)),
                        width.saturating_sub(3)
                    )
                ),
                color: 36,
            });
            crate::ui::buttons(&mut screen, width, &[("Clear", Action::PanelClear)]);
        }
        if panel.kind == PanelKind::Compose {
            crate::ui::buttons(&mut screen, width, &[("Choose agent", Action::FindAgents)]);
        }
        for text in &panel.text {
            screen.lines.extend(
                crate::ui::wrap(text, width)
                    .into_iter()
                    .map(|text| Line { text, color: 0 }),
            );
        }
        let available = height.saturating_sub(screen.lines.len() + 2).max(1);
        let start = panel.selected.saturating_sub(available - 1);
        if panel.items.is_empty() {
            screen.lines.push(Line {
                text: if panel.kind == PanelKind::Search {
                    "No matches. Up to 500 results."
                } else {
                    ""
                }
                .into(),
                color: 90,
            });
        }
        for (i, (label, _)) in panel.items.iter().enumerate().skip(start).take(available) {
            let y = screen.lines.len();
            screen.lines.push(Line {
                text: format!("{} {}", if i == panel.selected { ">" } else { " " }, label),
                color: if i == panel.selected { 7 } else { 0 },
            });
            screen.hits.push(Hit {
                x: 0,
                end: width,
                y,
                target: Target::PanelItem(i),
            });
        }
        screen.lines.truncate(height);
        screen.hits.retain(|h| h.y < height);
        for line in &mut screen.lines {
            line.text = clip(&line.text, 0, width);
        }
        screen
    }
}
