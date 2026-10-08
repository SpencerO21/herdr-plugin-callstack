//! Pure in-memory operations used by the viewer controls.
use crate::{
    graph::Graph,
    model::Frame,
    panels::{Choice, Panel, PanelKind},
    ui::{Action, Effect, Mode, View},
};
use std::collections::{HashSet, VecDeque};

pub fn mentions(frame: &Frame, name: &str) -> bool {
    [&frame.input, &frame.output]
        .into_iter()
        .flatten()
        .any(|text| {
            text.split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|token| token == name)
        })
}

impl View {
    pub fn selection_path(&self) -> Option<(String, Vec<usize>)> {
        if self.mode == Mode::Tree {
            Some((
                self.flow_name.clone()?,
                self.rows.get(self.selected)?.path.clone(),
            ))
        } else {
            let o = self
                .graph
                .nodes
                .get(self.graph_selected)?
                .occurrences
                .get(self.version)?;
            Some((o.record.flow.name.clone(), o.path.clone()))
        }
    }
    pub fn filter_rows(&mut self) {
        let changes: Vec<_> = if self.changes_only {
            self.rows
                .iter()
                .enumerate()
                .filter(|(i, _)| self.frame(*i).is_some_and(|f| f.marker() != '='))
                .map(|(_, r)| r.path.clone())
                .collect()
        } else {
            vec![]
        };
        self.rows.retain(|row| {
            let focused = self.focus_key.as_ref().is_none_or(|(name, path)| {
                Some(name) == self.flow_name.as_ref()
                    && (row.path.starts_with(path) || path.starts_with(&row.path))
            });
            focused && (!self.changes_only || changes.iter().any(|p| p.starts_with(&row.path)))
        });
    }
    pub fn filter_graph(&mut self) {
        let mut keep: HashSet<_> = (0..self.graph.nodes.len()).collect();
        if let Some((name, path)) = &self.focus_key {
            let seeds = self
                .graph
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    n.occurrences
                        .iter()
                        .any(|o| o.record.flow.name == *name && o.path == *path)
                })
                .map(|(i, _)| i)
                .collect();
            keep = related(&self.graph, &seeds, true, true);
        }
        if self.changes_only {
            let seeds = self
                .graph
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.occurrences.iter().any(|o| o.frame().marker() != '='))
                .map(|(i, _)| i)
                .collect();
            let changes = related(&self.graph, &seeds, true, false);
            keep.retain(|i| changes.contains(i));
        }
        self.graph.filter(&keep);
        if self.mode == Mode::Lanes {
            self.graph.layout_lanes(self.expanded);
        } else {
            self.graph.layout(self.expanded);
        }
        self.graph.trace_type(self.trace.as_deref());
    }
    pub fn refresh_explore(&mut self) {
        let selected = self.selection_path();
        self.offset = 0;
        self.detail_offset = 0;
        self.preview = false;
        self.rebuild();
        if self.mode == Mode::Tree
            && let Some((name, path)) = selected
            && self.flow_name.as_ref() == Some(&name)
        {
            self.selected = self.rows.iter().position(|r| r.path == path).unwrap_or(0);
        }
        self.rebuild_graph();
    }
    pub fn jump_to(&mut self, name: &str, path: &[usize]) {
        self.focus_key = None;
        self.changes_only = false;
        self.flow_name = Some(name.into());
        self.folded.remove(name);
        self.refresh_explore();
        if self.mode == Mode::Tree {
            self.selected = self.rows.iter().position(|r| r.path == path).unwrap_or(0);
        } else {
            for (i, node) in self.graph.nodes.iter().enumerate() {
                if let Some(version) = node
                    .occurrences
                    .iter()
                    .position(|o| o.record.flow.name == name && o.path == path)
                {
                    self.graph_selected = i;
                    self.version = version;
                    break;
                }
            }
            self.center_graph = true;
        }
    }
    pub fn explore_status(&self) -> String {
        let mut flags = vec![];
        if self.history {
            flags.push("HISTORY: archived flows".into());
        }
        if self.focus_key.is_some() {
            flags.push("PATH FOCUS".into());
        }
        if self.changes_only {
            flags.push("CHANGES ONLY".into());
        }
        if let Some(name) = &self.trace {
            flags.push(format!("TRACE {name}"));
        }
        if !self.message.is_empty() {
            flags.push(self.message.clone());
        }
        flags.join(" | ")
    }
    pub fn preview_key(&self) -> Option<String> {
        let record = self.selected_record()?;
        Some(
            serde_json::to_string(&(
                &record.flow.name,
                &record.updated_at,
                &record.project_root,
                &self.project,
                self.selection_path(),
                self.selected_frame()?.loc.as_ref()?,
            ))
            .unwrap(),
        )
    }
    pub fn accept_preview(&mut self, key: &str, result: Result<Vec<String>, String>) {
        self.busy = false;
        if self.preview && self.preview_key().as_deref() == Some(key) {
            self.preview_lines =
                result.unwrap_or_else(|e| vec![format!("Cannot preview source: {e}")]);
            self.preview_center = true;
        }
    }
    pub fn center_preview(&mut self, wrapped: &[String]) {
        if self.preview && self.preview_center {
            self.detail_offset = wrapped
                .iter()
                .position(|s| s.starts_with("> "))
                .unwrap_or(0)
                .saturating_sub(2);
            self.preview_center = false;
        }
    }
    pub fn accept_agents(&mut self, result: Result<Vec<crate::host::AgentTarget>, String>) {
        self.busy = false;
        let Some(panel) = &mut self.panel else {
            return;
        };
        if panel.kind != PanelKind::Agents {
            return;
        }
        panel.text.clear();
        match result {
            Ok(agents) if agents.is_empty() => panel.text.push("No idle agent in this workspace and project. Start an agent there, then try Generate again.".into()),
            Ok(agents) => panel.items = agents.into_iter().map(|a| (a.label.clone(), Choice::Agent(a))).collect(),
            Err(error) => panel.text.push(format!("Cannot list agents: {error}")),
        }
    }
    fn selected_project(&self) -> Option<std::path::PathBuf> {
        self.project.clone().or_else(|| {
            self.selected_record()
                .and_then(|r| r.project_root.as_ref().map(std::path::PathBuf::from))
        })
    }
    pub fn explore_action(&mut self, action: Action) -> Option<Effect> {
        match action {
            Action::Tools => self.tools_panel(),
            Action::Search => {
                self.panel = Some(Panel::new(PanelKind::Search));
                self.search_items();
            }
            Action::Focus => {
                self.focus_key = if self.focus_key.is_some() {
                    None
                } else {
                    self.selection_path()
                };
                self.refresh_explore();
            }
            Action::Changes => {
                self.changes_only = !self.changes_only;
                self.refresh_explore();
            }
            Action::ClearFilters => {
                self.focus_key = None;
                self.changes_only = false;
                self.trace = None;
                self.refresh_explore();
            }
            Action::ClearTrace => {
                self.trace = None;
                self.refresh_explore();
            }
            Action::Trace => {
                let mut types = std::collections::BTreeSet::new();
                fn walk(frames: &[Frame], types: &mut std::collections::BTreeSet<String>) {
                    for f in frames {
                        for text in [&f.input, &f.output].into_iter().flatten() {
                            types.extend(
                                text.split(|c: char| !c.is_alphanumeric() && c != '_')
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_owned),
                            );
                        }
                        walk(&f.calls, types);
                    }
                }
                for r in &self.records {
                    types.extend(r.flow.types.keys().cloned());
                    walk(&r.flow.frames, &mut types);
                }
                let mut panel = Panel::new(PanelKind::Types);
                panel.items = types
                    .into_iter()
                    .map(|name| (name.clone(), Choice::Type(name)))
                    .collect();
                if panel.items.is_empty() {
                    panel
                        .text
                        .push("No input or output types in these flows.".into());
                }
                self.panel = Some(panel);
            }
            Action::History => {
                self.history = !self.history;
                self.focus_key = None;
                self.changes_only = false;
                self.preview = false;
                self.update(self.all_records.clone());
            }
            Action::Archive if !self.busy => {
                if let Some(record) = self.selected_record() {
                    let effect = Effect::Archive(record.flow.name.clone(), !record.archived);
                    self.busy = true;
                    return Some(effect);
                }
            }
            Action::Lanes => {
                self.mode = Mode::Lanes;
                self.more = false;
                self.refresh_explore();
            }
            Action::Preview if !self.busy => {
                if let (Some(project), Some(loc), Some(key)) = (
                    self.selected_project(),
                    self.selected_frame().and_then(|f| f.loc.clone()),
                    self.preview_key(),
                ) {
                    self.preview = true;
                    self.detail = true;
                    self.detail_offset = 0;
                    self.preview_lines = vec!["Loading source...".into()];
                    self.busy = true;
                    return Some(Effect::Preview { project, loc, key });
                }
                self.message = "This call needs a source location and project folder.".into();
            }
            Action::Generate | Action::UpdateFlow if !self.busy => {
                self.generation_project = self.selected_project();
                self.generation_flow = if action == Action::UpdateFlow {
                    self.selected_record().map(|r| r.flow.clone())
                } else {
                    None
                };
                if self.generation_project.is_none()
                    || (action == Action::UpdateFlow && self.generation_flow.is_none())
                {
                    self.message = "Set a project folder and select a flow to update.".into();
                } else {
                    self.generation_target = None;
                    let mut panel = Panel::new(PanelKind::Compose);
                    panel.input = self
                        .generation_flow
                        .as_ref()
                        .map(|f| format!("Update the call path for {}", f.name))
                        .unwrap_or_default();
                    panel.text.push("Enter a function name or code path. No request is sent until you choose an agent and confirm.".into());
                    self.panel = Some(panel);
                }
            }
            Action::FindAgents if !self.busy => {
                if let Some(panel) = &self.panel
                    && panel.kind == PanelKind::Compose
                    && !panel.input.trim().is_empty()
                {
                    self.generation_subject = panel.input.clone();
                    if let Some(project) = self.generation_project.clone() {
                        let mut panel = Panel::new(PanelKind::Agents);
                        panel.text.push("Loading idle agents...".into());
                        self.panel = Some(panel);
                        self.busy = true;
                        return Some(Effect::Agents(project));
                    }
                }
            }
            Action::SendGeneration if !self.busy => {
                if let (Some(target), Some(project)) = (
                    self.generation_target.take(),
                    self.generation_project.clone(),
                ) {
                    self.panel = None;
                    self.busy = true;
                    self.message = "Sending request. Wait for the agent to publish a flow.".into();
                    return Some(Effect::Generate {
                        target,
                        project,
                        subject: self.generation_subject.clone(),
                        flow: self.generation_flow.clone(),
                    });
                }
            }
            _ => return None,
        }
        self.confirm = None;
        Some(Effect::None)
    }
}

pub fn related(
    graph: &Graph,
    seeds: &HashSet<usize>,
    ancestors: bool,
    descendants: bool,
) -> HashSet<usize> {
    // Keep ancestor and descendant walks separate. Walking both directions in
    // one flood-fill would include unrelated sibling branches.
    let walk = |reverse: bool| {
        let mut adjacency = vec![Vec::new(); graph.nodes.len()];
        for edge in &graph.edges {
            let (from, to) = if reverse {
                (edge.to, edge.from)
            } else {
                (edge.from, edge.to)
            };
            adjacency[from].push(to);
        }
        let mut found = seeds.clone();
        let mut queue: VecDeque<_> = seeds.iter().copied().collect();
        while let Some(node) = queue.pop_front() {
            for next in &adjacency[node] {
                if found.insert(*next) {
                    queue.push_back(*next);
                }
            }
        }
        found
    };
    let mut found = seeds.clone();
    if ancestors {
        found.extend(walk(true));
    }
    if descendants {
        found.extend(walk(false));
    }
    found
}

impl Graph {
    pub fn filter(&mut self, keep: &HashSet<usize>) {
        let mut mapping = vec![usize::MAX; self.nodes.len()];
        let mut index = 0;
        let mut next = 0;
        self.nodes.retain(|_| {
            let include = keep.contains(&index);
            if include {
                mapping[index] = next;
                next += 1;
            }
            index += 1;
            include
        });
        self.edges
            .retain(|e| keep.contains(&e.from) && keep.contains(&e.to));
        for edge in &mut self.edges {
            edge.from = mapping[edge.from];
            edge.to = mapping[edge.to];
        }
    }
    pub fn trace_type(&mut self, name: Option<&str>) {
        for node in &mut self.nodes {
            node.traced =
                name.is_some_and(|name| node.occurrences.iter().any(|o| mentions(o.frame(), name)));
            node.dimmed = name
                .is_some_and(|name| !node.occurrences.iter().any(|o| mentions(o.frame(), name)));
        }
    }
    pub fn layout_lanes(&mut self, expanded: bool) {
        self.layout(expanded);
        let names: std::collections::BTreeSet<_> = self
            .nodes
            .iter()
            .map(|n| {
                n.occurrences[0]
                    .frame()
                    .module
                    .clone()
                    .unwrap_or_else(|| "Other".into())
            })
            .collect();
        let step = self.node_width + 6;
        self.lanes = names
            .into_iter()
            .enumerate()
            .map(|(i, name)| (name, 1 + i * step))
            .collect();
        let mut layers = std::collections::BTreeMap::<usize, Vec<usize>>::new();
        for (i, node) in self.nodes.iter().enumerate() {
            layers.entry(node.depth).or_default().push(i);
        }
        let mut y = 2;
        for (depth, indices) in layers {
            let mut counts = vec![0; self.lanes.len()];
            for i in indices {
                let name = self.nodes[i].occurrences[0]
                    .frame()
                    .module
                    .as_deref()
                    .unwrap_or("Other");
                let lane = self.lanes.iter().position(|(n, _)| n == name).unwrap();
                self.nodes[i].x = self.lanes[lane].1;
                self.nodes[i].y = y + counts[lane] * (self.node_height + 2);
                counts[lane] += 1;
            }
            // Room for all outgoing tracks of the deepest box in this layer.
            y += counts.into_iter().max().unwrap_or(1) * (self.node_height + 2)
                + self
                    .edges
                    .iter()
                    .filter(|e| self.nodes[e.from].depth == depth)
                    .count()
                + 4;
        }
        self.width = self.lanes.len() * step + 4;
        self.height = y;
    }
}
