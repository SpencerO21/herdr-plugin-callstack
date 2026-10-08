use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use herdr_callstack::{
    graph::Graph,
    model::{Flow, Record, Scope},
    ui::{Action, Effect, Mode, Screen, Target, View},
};
use ratatui::{Terminal, backend::TestBackend};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn record(name: &str, frames: serde_json::Value) -> Arc<Record> {
    Arc::new(Record {
        flow: serde_json::from_value::<Flow>(serde_json::json!({"name":name,"frames":frames}))
            .unwrap(),
        scope: Scope {
            namespace: "local".into(),
            workspace: "w1".into(),
            session: "test".into(),
        },
        project_root: Some("/project".into()),
        updated_at: "now".into(),
        source_hashes: None,
        warnings: vec![],
        drifted_paths: vec![],
    })
}
fn records() -> Vec<Arc<Record>> {
    vec![
        record(
            "Checkout",
            serde_json::json!([{"fn":"submitOrder","loc":"order.rs:2","calls":[
                {"fn":"chargeCard","loc":"payment.rs:4","change":"added","cond":"total > 0","loop":"each item","in":"Order","out":"Charge","calls":[{"fn":"save","loc":"db.rs:3"}]},
                {"fn":"save","loc":"audit.rs:8","change":"removed","concurrent":true}
            ]}]),
        ),
        record(
            "Retry",
            serde_json::json!([{"fn":"retry","loc":"retry.rs:2","calls":[
                {"fn":"chargeCard","loc":"payment.rs:9","change":"modified","calls":[{"fn":"save","loc":"db.rs:10"}]}
            ]}]),
        ),
    ]
}
fn view() -> View {
    let records = records();
    let mut v = View::new(records[0].scope.clone(), None);
    v.update(records);
    v
}
fn mouse(x: usize, y: usize, kind: MouseEventKind) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: x as u16,
        row: y as u16,
        modifiers: KeyModifiers::NONE,
    })
}
fn click_action(v: &mut View, action: Action) -> Effect {
    let mut screen = v.render(100, 45);
    if !screen
        .hits
        .iter()
        .any(|h| matches!(h.target, Target::Action(a) if a == action))
    {
        let hit = screen
            .hits
            .iter()
            .find(|h| matches!(h.target, Target::Action(Action::More)))
            .unwrap();
        v.event(
            mouse(hit.x, hit.y, MouseEventKind::Down(MouseButton::Left)),
            &screen,
            Instant::now(),
        );
        screen = v.render(100, 45);
    }
    let hit = screen
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::Action(a) if a == action))
        .unwrap();
    v.event(
        mouse(hit.x, hit.y, MouseEventKind::Down(MouseButton::Left)),
        &screen,
        Instant::now(),
    )
}
fn terminal_text(screen: &Screen, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| screen.draw(f, true)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn merges_shared_calls_and_edges_but_not_different_files_or_projects() {
    let mut records = records();
    let graph = Graph::build(&records, true, false);
    assert_eq!(graph.nodes.len(), 5);
    let shared = graph
        .nodes
        .iter()
        .find(|n| n.occurrences[0].frame().function == "chargeCard")
        .unwrap();
    assert_eq!(shared.flow_count(), 2);
    assert_eq!(shared.markers(), "+~");
    assert_eq!(graph.edges.iter().filter(|e| e.flows.len() == 2).count(), 1);
    assert!(graph.edges.iter().any(|e| e.conditional));
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|n| n.occurrences[0].frame().function == "save")
            .count(),
        2
    );
    Arc::make_mut(&mut records[1]).project_root = Some("/another-project".into());
    assert_eq!(Graph::build(&records, true, false).nodes.len(), 7);
    assert_eq!(Graph::build(&records[..1], false, false).nodes.len(), 4);
}

#[test]
fn cycles_have_bounded_layout_and_return_connectors() {
    let records = vec![record(
        "cycle",
        serde_json::json!([{"fn":"a","loc":"a.rs:1","calls":[{"fn":"b","loc":"b.rs:1","calls":[{"fn":"a","loc":"a.rs:1"}]}]}]),
    )];
    let graph = Graph::build(&records, true, true);
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.height < 50);
    let buffer = graph.draw(100, 50, (0, 0), (0, 0), true, true);
    let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
    assert!(text.contains("return"));
}

#[test]
fn ratatui_draws_boxes_arrows_and_expanded_labels() {
    let mut v = view();
    click_action(&mut v, Action::Diagram);
    click_action(&mut v, Action::Size);
    let text = terminal_text(&v.render(120, 60), 120, 60);
    for expected in [
        "submitOrder",
        "chargeCard",
        "if total > 0",
        "loop each item",
        "Order -> Charge",
        "┌",
        "▼",
        "┆",
    ] {
        assert!(text.contains(expected), "missing {expected}\n{text}");
    }
}

#[test]
fn graph_mouse_selection_double_click_diff_and_versions_use_selected_source() {
    let mut v = view();
    click_action(&mut v, Action::Combined);
    assert_eq!(v.mode, Mode::Combined);
    let target = v
        .graph
        .nodes
        .iter()
        .position(|n| n.occurrences[0].frame().function == "chargeCard")
        .unwrap();
    let screen = v.render(100, 45);
    let hit = screen
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::Node(i) if i == target))
        .unwrap();
    let now = Instant::now();
    v.event(
        mouse(hit.x, hit.y, MouseEventKind::Down(MouseButton::Left)),
        &screen,
        now,
    );
    assert_eq!(v.graph_selected, target);
    assert!(v.detail);
    let screen = v.render(100, 45);
    let hit = screen
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::Node(i) if i == target))
        .unwrap();
    assert!(
        matches!(v.event(mouse(hit.x, hit.y, MouseEventKind::Down(MouseButton::Left)), &screen, now + Duration::from_millis(100)), Effect::Open(p, loc) if p.to_str() == Some("/project") && loc == "payment.rs:4")
    );
    v.busy = false;
    click_action(&mut v, Action::Version);
    assert_eq!(v.selected_record().unwrap().flow.name, "Retry");
    assert!(
        matches!(click_action(&mut v, Action::Diff), Effect::Diff(_, loc) if loc == "payment.rs:9")
    );
    v.busy = false;
    click_action(&mut v, Action::Delete);
    assert!(
        matches!(click_action(&mut v, Action::Confirm), Effect::Delete(name) if name == "Retry")
    );
}

#[test]
fn graph_controls_pan_resize_and_keep_mouse_targets_inside_viewport() {
    let mut v = view();
    click_action(&mut v, Action::Diagram);
    for action in [
        Action::Size,
        Action::More,
        Action::Right,
        Action::Down,
        Action::Left,
        Action::Up,
        Action::PageDown,
        Action::PageUp,
        Action::NodeNext,
        Action::NodePrev,
        Action::Center,
        Action::DetailDown,
        Action::DetailUp,
        Action::Next,
        Action::Prev,
    ] {
        click_action(&mut v, action);
    }
    for mode in [Action::Diagram, Action::Combined] {
        v.action(mode);
        for width in [29, 36, 80, 160] {
            for height in [16, 24, 40, 60] {
                for more in [false, true] {
                    v.more = more;
                    v.detail = true;
                    let screen = v.render(width, height);
                    assert!(screen.lines.len() <= height, "{width}x{height} more={more}");
                    assert!(
                        screen
                            .hits
                            .iter()
                            .all(|h| h.x < h.end && h.end < width && h.y < height)
                    );
                    if height >= 24 {
                        assert!(screen.lines[height - 3].text.starts_with("= same"));
                        let (top, buffer) = screen.diagram.as_ref().unwrap();
                        for hit in &screen.hits {
                            if matches!(hit.target, Target::Node(_)) {
                                assert!(
                                    hit.y >= *top && hit.y < top + usize::from(buffer.area.height)
                                );
                            }
                        }
                    } else {
                        assert!(
                            screen
                                .hits
                                .iter()
                                .any(|h| matches!(h.target, Target::Action(Action::Tree)))
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn large_graph_only_allocates_the_viewport_and_scroll_is_bounded() {
    let records = vec![record(
        "large",
        serde_json::json!([{"fn":"root","calls":(0..1000).map(|i| serde_json::json!({"fn":format!("child{i}")})).collect::<Vec<_>>()}]),
    )];
    let graph = Graph::build(&records, false, false);
    assert!(graph.width > 30_000);
    let buffer = graph.draw(80, 25, (30_000, 3), (0, 0), false, true);
    assert_eq!(buffer.content.len(), 2000);
    let mut v = view();
    v.action(Action::Diagram);
    v.camera = (usize::MAX, usize::MAX);
    v.action(Action::Down);
    v.render(80, 40);
    assert!(v.camera.0 <= v.graph.width && v.camera.1 <= v.graph.height);
}

#[test]
fn live_update_preserves_selection_and_empty_data_is_safe() {
    let mut v = view();
    v.action(Action::Combined);
    v.action(Action::NodeNext);
    v.action(Action::Version);
    let key = v.graph.nodes[v.graph_selected].key.clone();
    v.update(records());
    assert_eq!(v.graph.nodes[v.graph_selected].key, key);
    assert_eq!(v.selected_record().unwrap().flow.name, "Retry");
    v.update(vec![]);
    let screen = v.render(80, 40);
    assert!(screen.lines.iter().any(|l| l.text.contains("No flows")));
    assert!(matches!(v.action(Action::Open), Effect::None));
    v.action(Action::Version);
    v.action(Action::Size);
    v.action(Action::NodeNext);
}

#[test]
fn unicode_labels_are_clipped_and_no_color_keeps_selection_text() {
    let records = vec![record(
        "unicode",
        serde_json::json!([{"fn":"界界界界界界界界界界界界界界界","loc":"界.rs:1"}]),
    )];
    let graph = Graph::build(&records, false, false);
    for camera in [(0, 0), (3, 0), (10, 1)] {
        let buffer = graph.draw(29, 10, camera, (0, 0), false, false);
        assert_eq!(buffer.content.len(), 290);
        assert!(
            buffer
                .content
                .iter()
                .all(|c| c.fg == ratatui::style::Color::Reset)
        );
    }
    let buffer = graph.draw(40, 10, (0, 0), (0, 0), false, false);
    assert!(
        buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("> selected")
    );
}

#[test]
fn combined_routes_keep_conditions_and_mark_crossings() {
    let mut v = view();
    v.action(Action::Combined);
    v.action(Action::Size);
    let text = terminal_text(&v.render(120, 65), 120, 65);
    assert!(text.contains('╳'), "{text}");
    assert!(text.contains('┆') || text.contains('┄'), "{text}");
    assert!(text.contains('═') || text.contains('║'), "{text}");
}

#[test]
fn switching_to_tree_clears_diagram_cells_and_no_color_is_respected() {
    let mut v = view();
    v.action(Action::Diagram);
    let screen = v.render(100, 45);
    let mut terminal = Terminal::new(TestBackend::new(100, 45)).unwrap();
    terminal.draw(|f| screen.draw(f, false)).unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .all(|c| c.fg == ratatui::style::Color::Reset)
    );
    v.action(Action::Tree);
    let tree = v.render(100, 45);
    terminal.draw(|f| tree.draw(f, false)).unwrap();
    assert!(
        !terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.symbol() == "┌")
    );
}

#[test]
fn details_wheel_does_not_pan_graph_and_pan_reuses_layout() {
    let mut v = view();
    v.action(Action::Diagram);
    v.detail = true;
    let screen = v.render(80, 40);
    let (top, buffer) = screen.diagram.as_ref().unwrap();
    let camera = v.camera;
    let nodes = v.graph.nodes.as_ptr();
    v.event(
        mouse(
            0,
            top + usize::from(buffer.area.height) + 1,
            MouseEventKind::ScrollDown,
        ),
        &screen,
        Instant::now(),
    );
    assert_eq!(v.camera, camera);
    assert_eq!(v.detail_offset, 3);
    v.event(
        mouse(0, *top, MouseEventKind::ScrollDown),
        &screen,
        Instant::now(),
    );
    assert_eq!(v.camera.1, camera.1 + 3);
    assert_eq!(v.graph.nodes.as_ptr(), nodes);
}
