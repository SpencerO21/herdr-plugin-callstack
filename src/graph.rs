//! Cached graph structure and a viewport-sized Ratatui drawing surface.
use crate::{
    model::{Frame, Record},
    ui::{clip, style},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct Occurrence {
    pub record: Arc<Record>,
    pub path: Vec<usize>,
}
impl Occurrence {
    pub fn frame(&self) -> &Frame {
        let mut frames = &self.record.flow.frames;
        for (level, index) in self.path.iter().enumerate() {
            let frame = &frames[*index];
            if level + 1 == self.path.len() {
                return frame;
            }
            frames = &frame.calls;
        }
        unreachable!("graph occurrences always contain a frame path")
    }
    pub fn changed(&self) -> bool {
        self.frame()
            .loc
            .as_ref()
            .and_then(|loc| crate::host::location_parts(loc).ok())
            .is_some_and(|(path, _)| self.record.drifted_paths.iter().any(|p| p == path))
    }
}

#[derive(Debug)]
pub struct Node {
    pub dimmed: bool,
    pub traced: bool,
    pub key: String,
    pub occurrences: Vec<Occurrence>,
    pub depth: usize,
    pub x: usize,
    pub y: usize,
}
impl Node {
    pub fn markers(&self) -> String {
        self.occurrences
            .iter()
            .map(|o| o.frame().marker())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn flow_count(&self) -> usize {
        self.occurrences
            .iter()
            .map(|o| &o.record.flow.name)
            .collect::<BTreeSet<_>>()
            .len()
    }
}
#[derive(Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub conditional: bool,
    pub flows: BTreeSet<String>,
    from_port: usize,
    to_port: usize,
    track: usize,
}
#[derive(Default, Debug)]
pub struct Graph {
    pub lanes: Vec<(String, usize)>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub width: usize,
    pub height: usize,
    pub node_width: usize,
    pub node_height: usize,
}

impl Graph {
    pub fn build(records: &[Arc<Record>], combined: bool, expanded: bool) -> Self {
        struct Builder {
            graph: Graph,
            nodes: HashMap<String, usize>,
            edges: HashMap<(usize, usize), usize>,
            combined: bool,
        }
        impl Builder {
            fn walk(
                &mut self,
                record: &Arc<Record>,
                frames: &[Frame],
                path: &mut Vec<usize>,
                parent: Option<usize>,
            ) {
                for (index, frame) in frames.iter().enumerate() {
                    path.push(index);
                    // Do not join unrelated functions merely because their names match.
                    // No-location calls stay local to their flow.
                    let source = frame
                        .loc
                        .as_ref()
                        .and_then(|loc| crate::host::location_parts(loc).ok())
                        .map(|(p, _)| p);
                    let key = if self.combined {
                        serde_json::to_string(&(
                            &record.project_root,
                            &frame.function,
                            source,
                            source.is_none().then_some(&record.flow.name),
                        ))
                        .unwrap()
                    } else {
                        serde_json::to_string(&(&record.flow.name, &path)).unwrap()
                    };
                    let next = self.graph.nodes.len();
                    let node = *self.nodes.entry(key.clone()).or_insert(next);
                    if node == next {
                        self.graph.nodes.push(Node {
                            dimmed: false,
                            traced: false,
                            key,
                            occurrences: vec![],
                            depth: path.len() - 1,
                            x: 0,
                            y: 0,
                        });
                    }
                    self.graph.nodes[node].depth = self.graph.nodes[node].depth.max(path.len() - 1);
                    self.graph.nodes[node].occurrences.push(Occurrence {
                        record: record.clone(),
                        path: path.clone(),
                    });
                    if let Some(from) = parent {
                        let next = self.graph.edges.len();
                        let edge = *self.edges.entry((from, node)).or_insert(next);
                        if edge == next {
                            self.graph.edges.push(Edge {
                                from,
                                to: node,
                                conditional: false,
                                flows: BTreeSet::new(),
                                from_port: 0,
                                to_port: 0,
                                track: 0,
                            });
                        }
                        self.graph.edges[edge].conditional |= frame.cond.is_some();
                        self.graph.edges[edge]
                            .flows
                            .insert(record.flow.name.clone());
                    }
                    self.walk(record, &frame.calls, path, Some(node));
                    path.pop();
                }
            }
        }
        let mut builder = Builder {
            graph: Self::default(),
            nodes: HashMap::new(),
            edges: HashMap::new(),
            combined,
        };
        for record in records {
            builder.walk(record, &record.flow.frames, &mut vec![], None);
        }
        builder.graph.layout(expanded);
        builder.graph
    }

    pub fn layout(&mut self, expanded: bool) {
        self.lanes.clear();
        self.node_width = if expanded { 36 } else { 26 };
        self.node_height = if expanded { 9 } else { 5 };
        let mut layers = BTreeMap::<usize, Vec<usize>>::new();
        for (i, node) in self.nodes.iter().enumerate() {
            layers.entry(node.depth).or_default().push(i);
        }
        let widest = layers.values().map(Vec::len).max().unwrap_or(0);
        let step = self.node_width + 6;
        self.width = widest * step + 4;
        let mut tracks = HashMap::<usize, usize>::new();
        let mut incoming = vec![0; self.nodes.len()];
        let mut outgoing = vec![0; self.nodes.len()];
        for edge in &mut self.edges {
            let track = tracks.entry(self.nodes[edge.from].depth).or_default();
            edge.track = *track;
            *track += 1;
            incoming[edge.to] += 1;
            outgoing[edge.from] += 1;
        }
        let mut in_index = vec![0; self.nodes.len()];
        let mut out_index = vec![0; self.nodes.len()];
        for edge in &mut self.edges {
            in_index[edge.to] += 1;
            out_index[edge.from] += 1;
            edge.from_port =
                1 + (self.node_width - 2) * out_index[edge.from] / (outgoing[edge.from] + 1);
            edge.to_port = 1 + (self.node_width - 2) * in_index[edge.to] / (incoming[edge.to] + 1);
        }
        // Depth comes from bounded input paths. Backward/cyclic edges use a side
        // connector, never an unbounded "push children down" relaxation loop.
        let mut y = 1;
        for (depth, nodes) in layers {
            let offset = (widest - nodes.len()) * step / 2;
            for (column, index) in nodes.into_iter().enumerate() {
                self.nodes[index].x = 1 + offset + column * step;
                self.nodes[index].y = y;
            }
            y += self.node_height + (tracks.get(&depth).copied().unwrap_or(0) + 2).max(4);
        }
        self.height = y;
        // Keep outgoing and incoming vertical tracks on different columns.
        // Otherwise two unrelated edges can overlap and resemble a junction.
        for edge in &mut self.edges {
            for (node, port, parity) in [
                (edge.from, &mut edge.from_port, 1),
                (edge.to, &mut edge.to_port, 0),
            ] {
                if (self.nodes[node].x + *port) % 2 != parity {
                    if *port > 1 {
                        *port -= 1;
                    } else {
                        *port += 1;
                    }
                }
            }
        }
    }

    pub fn draw(
        &self,
        width: u16,
        height: u16,
        camera: (usize, usize),
        selected: (usize, usize),
        expanded: bool,
        colors: bool,
    ) -> Buffer {
        let mut canvas = Canvas {
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
            camera,
            colors,
            connections: vec![0; usize::from(width) * usize::from(height)],
            join: true,
        };
        for edge in &self.edges {
            let from = &self.nodes[edge.from];
            let to = &self.nodes[edge.to];
            let x1 = from.x + edge.from_port;
            let y1 = from.y + self.node_height;
            let x2 = to.x + edge.to_port;
            let y2 = to.y.saturating_sub(1);
            let color = if edge.from == selected.0 || edge.to == selected.0 {
                36
            } else {
                90
            };
            let horizontal = if edge.conditional {
                '┄'
            } else if edge.flows.len() > 1 {
                '═'
            } else {
                '─'
            };
            let vertical = if edge.conditional {
                '┆'
            } else if edge.flows.len() > 1 {
                '║'
            } else {
                '│'
            };
            if y2 > y1 {
                let middle = y1 + 1 + edge.track;
                canvas.vertical(x1, y1, middle, vertical, color);
                canvas.horizontal(x1, x2, middle, horizontal, color);
                canvas.vertical(x2, middle, y2, vertical, color);
            } else {
                let side = self.width.saturating_sub(2);
                canvas.horizontal(x1, side, y1, horizontal, color);
                canvas.vertical(side, y2, y1, vertical, color);
                canvas.horizontal(x2, side, y2, horizontal, color);
                canvas.text(side.saturating_sub(6), y1, "return", style(color), 6);
            }
        }
        canvas.join = false;
        for edge in &self.edges {
            let to = &self.nodes[edge.to];
            canvas.put(
                to.x + edge.to_port,
                to.y.saturating_sub(1),
                '▼',
                style(if edge.to == selected.0 { 36 } else { 90 }),
            );
        }
        for (index, node) in self.nodes.iter().enumerate() {
            if node.x + self.node_width <= camera.0
                || node.x >= camera.0 + usize::from(width)
                || node.y + self.node_height <= camera.1
                || node.y >= camera.1 + usize::from(height)
            {
                continue;
            }
            let markers = node.markers();
            let color = if node.dimmed {
                90
            } else if node.traced {
                36
            } else {
                match markers.as_str() {
                    "+" => 32,
                    "-" => 31,
                    "~" => 33,
                    "=" => 0,
                    _ => 33,
                }
            };
            let border = if node.dimmed {
                style(90)
            } else if index == selected.0 {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                style(color)
            };
            let occurrence = &node.occurrences[if index == selected.0 {
                selected.1.min(node.occurrences.len() - 1)
            } else {
                0
            }];
            let frame = occurrence.frame();
            let mut labels = vec![format!(
                "{}{markers} {}",
                if index == selected.0 && node.traced {
                    ">* "
                } else if index == selected.0 {
                    "> "
                } else if node.traced {
                    "* "
                } else {
                    ""
                },
                frame.function
            )];
            let mut tags = vec![];
            if node.flow_count() > 1 {
                tags.push(format!("{} flows", node.flow_count()));
            }
            if node.occurrences.iter().any(Occurrence::changed) {
                tags.push("SOURCE CHANGED".into());
            }
            if node
                .occurrences
                .iter()
                .any(|o| o.frame().concurrent == Some(true))
            {
                tags.push("parallel".into());
            }
            if expanded {
                labels.push(tags.join(" | "));
                labels.push(frame.loc.clone().unwrap_or_default());
                labels.push(format!(
                    "{} -> {}",
                    frame.input.as_deref().unwrap_or("()"),
                    frame.output.as_deref().unwrap_or("()")
                ));
                labels.push(
                    frame
                        .cond
                        .as_ref()
                        .map(|s| format!("if {s}"))
                        .unwrap_or_default(),
                );
                labels.push(
                    frame
                        .loop_context
                        .as_ref()
                        .map(|s| format!("loop {s}"))
                        .unwrap_or_default(),
                );
                labels.push(if node.occurrences.len() > 1 {
                    format!("Version: {}", occurrence.record.flow.name)
                } else {
                    "Click: details".into()
                });
            } else {
                if node.occurrences.iter().any(|o| o.frame().cond.is_some()) {
                    tags.push("if".into());
                }
                if node
                    .occurrences
                    .iter()
                    .any(|o| o.frame().loop_context.is_some())
                {
                    tags.push("loop".into());
                }
                labels.push(tags.join(" | "));
                labels.push(if index == selected.0 {
                    "> selected".into()
                } else {
                    "Click: details".into()
                });
            }
            // Draw only visible cells. No full-graph bitmap is allocated.
            for y in node.y..node.y + self.node_height {
                for x in node.x.max(camera.0)
                    ..(node.x + self.node_width).min(camera.0 + usize::from(width))
                {
                    canvas.put(x, y, ' ', Style::default());
                }
            }
            let right = node.x + self.node_width - 1;
            let bottom = node.y + self.node_height - 1;
            canvas.horizontal(node.x, right, node.y, '─', color);
            canvas.horizontal(node.x, right, bottom, '─', color);
            canvas.vertical(node.x, node.y, bottom, '│', color);
            canvas.vertical(right, node.y, bottom, '│', color);
            for (x, y, c) in [
                (node.x, node.y, '┌'),
                (right, node.y, '┐'),
                (node.x, bottom, '└'),
                (right, bottom, '┘'),
            ] {
                canvas.put(x, y, c, border);
            }
            for (line, text) in labels.iter().take(self.node_height - 2).enumerate() {
                canvas.text(
                    node.x + 1,
                    node.y + 1 + line,
                    text,
                    if line == 0 {
                        border
                    } else if node.dimmed {
                        style(90)
                    } else {
                        Style::default()
                    },
                    self.node_width - 2,
                );
            }
        }
        if !self.lanes.is_empty() && height > 0 {
            for x in 0..width {
                canvas.buffer[(x, 0)].reset();
            }
            for (name, x) in &self.lanes {
                canvas.text(*x, camera.1, name, style(36), self.node_width);
            }
        }
        canvas.buffer
    }
}

struct Canvas {
    buffer: Buffer,
    camera: (usize, usize),
    colors: bool,
    connections: Vec<u8>,
    join: bool,
}
impl Canvas {
    fn put(&mut self, x: usize, y: usize, c: char, style: Style) {
        if x < self.camera.0 || y < self.camera.1 {
            return;
        }
        let (x, y) = (x - self.camera.0, y - self.camera.1);
        if x < usize::from(self.buffer.area.width) && y < usize::from(self.buffer.area.height) {
            self.buffer[(x as u16, y as u16)]
                .set_char(c)
                .set_style(if self.colors { style } else { Style::default() });
        }
    }
    fn horizontal(&mut self, x1: usize, x2: usize, y: usize, c: char, color: u8) {
        if y < self.camera.1 || y - self.camera.1 >= usize::from(self.buffer.area.height) {
            return;
        }
        for x in x1.min(x2).max(self.camera.0)
            ..=x1
                .max(x2)
                .min(self.camera.0 + usize::from(self.buffer.area.width))
        {
            if self.join {
                self.connect(
                    x,
                    y,
                    u8::from(x > x1.min(x2)) | (u8::from(x < x1.max(x2)) << 1),
                    c,
                    color,
                );
            } else {
                self.put(x, y, c, style(color));
            }
        }
    }
    fn vertical(&mut self, x: usize, y1: usize, y2: usize, c: char, color: u8) {
        if x < self.camera.0 || x - self.camera.0 >= usize::from(self.buffer.area.width) {
            return;
        }
        for y in y1.min(y2).max(self.camera.1)
            ..=y1
                .max(y2)
                .min(self.camera.1 + usize::from(self.buffer.area.height))
        {
            if self.join {
                self.connect(
                    x,
                    y,
                    (u8::from(y > y1.min(y2)) << 2) | (u8::from(y < y1.max(y2)) << 3),
                    c,
                    color,
                );
            } else {
                self.put(x, y, c, style(color));
            }
        }
    }
    fn connect(&mut self, x: usize, y: usize, directions: u8, straight: char, color: u8) {
        if x < self.camera.0
            || y < self.camera.1
            || x - self.camera.0 >= usize::from(self.buffer.area.width)
            || y - self.camera.1 >= usize::from(self.buffer.area.height)
        {
            return;
        }
        let index = (y - self.camera.1) * usize::from(self.buffer.area.width) + x - self.camera.0;
        self.connections[index] |= directions;
        let c = match self.connections[index] {
            5 => '┘',
            6 => '└',
            9 => '┐',
            10 => '┌',
            7 => '┴',
            11 => '┬',
            13 => '┤',
            14 => '├',
            15 => '╳', // Crossing routes, not a call junction.
            _ => straight,
        };
        self.put(x, y, c, style(color));
    }
    fn text(&mut self, x: usize, y: usize, text: &str, style: Style, width: usize) {
        if y < self.camera.1
            || y >= self.camera.1 + usize::from(self.buffer.area.height)
            || x + width <= self.camera.0
            || x >= self.camera.0 + usize::from(self.buffer.area.width)
        {
            return;
        }
        let skip = self.camera.0.saturating_sub(x);
        let draw_x = x.saturating_sub(self.camera.0);
        let width = width
            .saturating_sub(skip)
            .min(usize::from(self.buffer.area.width) - draw_x);
        let text = clip(text, skip, width);
        self.buffer.set_stringn(
            draw_x as u16,
            (y - self.camera.1) as u16,
            text,
            width,
            if self.colors { style } else { Style::default() },
        );
    }
}
