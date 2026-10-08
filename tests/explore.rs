use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use herdr_callstack::{
    explore::mentions,
    host,
    model::{Flow, Record, Scope},
    panels::{Choice, PanelKind},
    store::{Cache, Store},
    ui::{Action, Effect, Mode, Target, View},
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "callstack-explore-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn scope() -> Scope {
    Scope {
        namespace: "test".into(),
        workspace: "w1".into(),
        session: "session".into(),
    }
}
fn flow() -> Flow {
    serde_json::from_value(serde_json::json!({
        "name":"Checkout","types":{"Order":"{ id: number }"},
        "frames":[{"fn":"root","loc":"source.rs:1","module":"UI","calls":[
            {"fn":"pay","loc":"source.rs:2","in":"Order","module":"Payments","calls":[
                {"fn":"save","loc":"source.rs:3","in":"Vec<Order>","change":"added","module":"Storage"},
                {"fn":"audit","loc":"source.rs:4","in":"Orders","module":"Storage"}
            ]},
            {"fn":"unrelated","loc":"source.rs:5","module":"UI"}
        ]}]
    })).unwrap()
}
fn view() -> View {
    let mut v = View::new(scope(), Some(PathBuf::from("/project")));
    v.update(vec![Arc::new(Record {
        archived: false,
        flow: flow(),
        scope: scope(),
        project_root: Some("/project".into()),
        updated_at: "now".into(),
        source_hashes: None,
        warnings: vec![],
        drifted_paths: vec![],
    })]);
    v
}
fn key(v: &mut View, code: KeyCode) -> Effect {
    let screen = v.render(100, 40);
    v.event(
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE)),
        &screen,
        Instant::now(),
    )
}
fn click_choice(v: &mut View, label: &str) -> Effect {
    let index = v
        .panel
        .as_ref()
        .unwrap()
        .items
        .iter()
        .position(|(s, _)| s.contains(label))
        .unwrap();
    v.panel.as_mut().unwrap().selected = index;
    let screen = v.render(100, 40);
    let hit = screen
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::PanelItem(i) if i == index))
        .unwrap();
    v.event(
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            row: hit.y as u16,
            column: hit.x as u16,
            modifiers: KeyModifiers::NONE,
        }),
        &screen,
        Instant::now(),
    )
}

#[test]
fn search_is_case_insensitive_and_jumps_to_folded_calls() {
    let mut v = view();
    v.action(Action::Collapse);
    v.action(Action::Search);
    for c in "SAVE".chars() {
        key(&mut v, KeyCode::Char(c));
    }
    assert_eq!(v.panel.as_ref().unwrap().items.len(), 1);
    click_choice(&mut v, "save");
    assert_eq!(v.selected_frame().unwrap().function, "save");
    assert!(v.panel.is_none());
    assert!(v.rows.len() > 1);
    v.action(Action::Combined);
    v.action(Action::Search);
    for c in "audit".chars() {
        key(&mut v, KeyCode::Char(c));
    }
    key(&mut v, KeyCode::Enter);
    assert_eq!(v.selected_frame().unwrap().function, "audit");
}

#[test]
fn path_focus_excludes_siblings_in_tree_and_graph() {
    for mode in [Action::Tree, Action::Combined, Action::Lanes] {
        let mut v = view();
        v.action(mode);
        v.jump_to("Checkout", &[0, 0]);
        v.action(Action::Focus);
        assert_eq!(v.selected_frame().unwrap().function, "pay");
        if v.mode == Mode::Tree {
            assert_eq!(v.rows.len(), 4);
            assert!((0..v.rows.len()).all(|i| v.frame(i).unwrap().function != "unrelated"));
        } else {
            assert_eq!(v.graph.nodes.len(), 4);
            assert!(
                v.graph
                    .nodes
                    .iter()
                    .all(|n| n.occurrences[0].frame().function != "unrelated")
            );
        }
        v.action(Action::Focus);
        assert_eq!(
            if v.mode == Mode::Tree {
                v.rows.len()
            } else {
                v.graph.nodes.len()
            },
            5
        );
    }
}

#[test]
fn changes_only_preserves_ancestors_and_combines_with_focus() {
    for mode in [Action::Tree, Action::Combined] {
        let mut v = view();
        v.action(mode);
        v.action(Action::Changes);
        let names: Vec<_> = if v.mode == Mode::Tree {
            (0..v.rows.len())
                .map(|i| v.frame(i).unwrap().function.clone())
                .collect()
        } else {
            v.graph
                .nodes
                .iter()
                .map(|n| n.occurrences[0].frame().function.clone())
                .collect()
        };
        assert_eq!(names, ["root", "pay", "save"]);
        v.action(Action::ClearFilters);
        v.jump_to("Checkout", &[0, 1]);
        v.action(Action::Focus);
        v.action(Action::Changes);
        // The common root remains useful context; the unrelated branch is gone.
        assert!(v.selected_frame().is_none_or(|f| f.function == "root"));
    }
}

#[test]
fn type_trace_uses_token_boundaries_and_marks_all_matching_nodes() {
    let mut v = view();
    v.action(Action::Combined);
    v.action(Action::Trace);
    click_choice(&mut v, "Order");
    let traced: Vec<_> = v
        .graph
        .nodes
        .iter()
        .filter(|n| n.traced)
        .map(|n| n.occurrences[0].frame().function.as_str())
        .collect();
    assert_eq!(traced, ["pay", "save"]);
    assert!(!mentions(v.graph.nodes[3].occurrences[0].frame(), "Order"));
    assert_eq!(v.graph.nodes.iter().filter(|n| n.dimmed).count(), 3);
    v.action(Action::ClearTrace);
    assert!(v.graph.nodes.iter().all(|n| !n.dimmed && !n.traced));
}

#[test]
fn lanes_group_by_module_without_overlapping_boxes() {
    let mut v = view();
    v.action(Action::Lanes);
    assert_eq!(v.mode, Mode::Lanes);
    assert_eq!(
        v.graph
            .lanes
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>(),
        ["Payments", "Storage", "UI"]
    );
    for (i, a) in v.graph.nodes.iter().enumerate() {
        for b in &v.graph.nodes[i + 1..] {
            assert!(
                a.x + v.graph.node_width <= b.x
                    || b.x + v.graph.node_width <= a.x
                    || a.y + v.graph.node_height <= b.y
                    || b.y + v.graph.node_height <= a.y
            );
        }
    }
    let saves: Vec<_> = v
        .graph
        .nodes
        .iter()
        .filter(|n| n.occurrences[0].frame().module.as_deref() == Some("Storage"))
        .collect();
    assert_eq!(saves[0].x, saves[1].x);
    assert_ne!(saves[0].y, saves[1].y);
    v.action(Action::Size);
    assert_eq!(v.graph.lanes.len(), 3);
}

#[test]
fn archive_restore_preserves_data_and_cached_reads() {
    let temp = Temp::new();
    let store = Store::new(&temp.0, scope()).unwrap();
    store.publish(flow(), None).unwrap();
    let original = fs::read(store.file("Checkout")).unwrap();
    let mut cache = Cache::default();
    cache.refresh(&store).unwrap();
    store.set_archived("Checkout", true).unwrap();
    assert!(cache.refresh(&store).unwrap());
    assert_eq!(cache.reads, 1);
    assert!(cache.records()[0].archived);
    assert_eq!(fs::read(store.file("Checkout")).unwrap(), original);
    let mut updated = flow();
    updated.description = Some("New publication".into());
    store.publish(updated, None).unwrap();
    assert!(store.list().unwrap()[0].archived);
    store.set_archived("Checkout", false).unwrap();
    let records = store.list().unwrap();
    assert!(!records[0].archived);
    assert_eq!(
        records[0].flow.description.as_deref(),
        Some("New publication")
    );
}

#[test]
fn history_excludes_archives_from_active_combined_and_can_restore() {
    let mut v = view();
    let mut archived = v.records[0].as_ref().clone();
    archived.flow.name = "Old checkout".into();
    archived.archived = true;
    let mut all = v.records.clone();
    all.push(Arc::new(archived));
    v.update(all);
    v.action(Action::Combined);
    assert_eq!(v.records.len(), 1);
    assert!(
        v.graph
            .nodes
            .iter()
            .all(|n| n.occurrences.iter().all(|o| !o.record.archived))
    );
    v.action(Action::History);
    assert_eq!(v.records.len(), 1);
    assert!(v.records[0].archived);
    assert!(
        matches!(v.action(Action::Archive), Effect::Archive(name, false) if name == "Old checkout")
    );
}

#[test]
fn source_preview_has_line_numbers_and_rejects_unsafe_or_binary_files() {
    let temp = Temp::new();
    fs::write(
        temp.0.join("source.rs"),
        (1..=200).map(|i| format!("line {i}\n")).collect::<String>(),
    )
    .unwrap();
    let lines = host::preview_source(&temp.0, "source.rs:80").unwrap();
    assert!(lines.iter().any(|s| s.starts_with(">    80 line 80")));
    assert_eq!(lines.len(), 83);
    assert!(host::preview_source(&temp.0, "../outside:1").is_err());
    assert!(host::preview_source(&temp.0, "source.rs:999").is_err());
    fs::write(temp.0.join("binary"), [0, 1, 2]).unwrap();
    assert!(host::preview_source(&temp.0, "binary:1").is_err());
    fs::write(temp.0.join("large"), vec![b'a'; 4_000_001]).unwrap();
    assert!(host::preview_source(&temp.0, "large:1").is_err());
    let outside = Temp::new();
    fs::write(outside.0.join("private"), "private\n").unwrap();
    std::os::unix::fs::symlink(outside.0.join("private"), temp.0.join("escape")).unwrap();
    assert!(host::preview_source(&temp.0, "escape:1").is_err());
}

#[test]
fn preview_results_cannot_replace_a_different_selection() {
    let mut v = view();
    let effect = v.action(Action::Preview);
    let Effect::Preview { key, .. } = effect else {
        panic!()
    };
    v.accept_preview(&key, Ok(vec!["first source".into()]));
    assert!(
        v.render(100, 40)
            .lines
            .iter()
            .any(|l| l.text.contains("first source"))
    );
    let Effect::Preview { key, .. } = v.action(Action::Preview) else {
        panic!()
    };
    v.jump_to("Checkout", &[0, 0]);
    v.accept_preview(&key, Ok(vec!["STALE".into()]));
    assert!(
        !v.render(100, 40)
            .lines
            .iter()
            .any(|l| l.text.contains("STALE"))
    );
}

#[test]
fn agent_candidates_require_ready_state_project_workspace_and_identity() {
    let temp = Temp::new();
    let candidate = serde_json::json!({"workspace_id":"w1","agent_status":"idle","cwd":temp.0,"pane_id":"w1:p9","agent":"claude","agent_session":{"value":"session-1"}});
    let mut busy = candidate.clone();
    busy["agent_status"] = "working".into();
    let mut blocked = candidate.clone();
    blocked["agent_status"] = "blocked".into();
    let mut other = candidate.clone();
    other["workspace_id"] = "w2".into();
    let mut path = candidate.clone();
    path["cwd"] = "/missing-project".into();
    let mut missing = candidate.clone();
    missing["agent_session"] = serde_json::Value::Null;
    let result = host::eligible_agents(
        &serde_json::json!({"agents":[candidate,busy,blocked,other,path,missing]}),
        "w1",
        &temp.0,
    )
    .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].session, "session-1");
}

#[test]
fn generate_requires_composition_agent_selection_and_confirmation() {
    let mut v = view();
    assert!(matches!(v.action(Action::Generate), Effect::None));
    assert_eq!(v.panel.as_ref().unwrap().kind, PanelKind::Compose);
    assert!(matches!(v.action(Action::FindAgents), Effect::None));
    for c in "submit_order".chars() {
        key(&mut v, KeyCode::Char(c));
    }
    assert!(matches!(key(&mut v, KeyCode::Enter), Effect::Agents(_)));
    v.accept_agents(Ok(vec![host::AgentTarget {
        pane: "w1:p9".into(),
        label: "Claude w1:p9".into(),
        session: "s1".into(),
    }]));
    assert!(matches!(click_choice(&mut v, "Claude"), Effect::None));
    assert_eq!(v.panel.as_ref().unwrap().kind, PanelKind::Send);
    let screen = v.render(29, 16);
    assert!(
        screen
            .hits
            .iter()
            .any(|h| matches!(h.target, Target::Action(Action::SendGeneration)))
    );
    let Effect::Generate {
        target,
        subject,
        flow,
        ..
    } = v.action(Action::SendGeneration)
    else {
        panic!()
    };
    assert_eq!(target.pane, "w1:p9");
    assert_eq!(subject, "submit_order");
    assert!(flow.is_none());
}

#[test]
fn cancellation_and_empty_agent_list_do_not_send_work() {
    let mut v = view();
    v.action(Action::UpdateFlow);
    assert!(v.generation_flow.is_some());
    assert!(matches!(key(&mut v, KeyCode::Enter), Effect::Agents(_)));
    v.accept_agents(Ok(vec![]));
    assert!(v.panel.as_ref().unwrap().items.is_empty());
    assert!(matches!(key(&mut v, KeyCode::Enter), Effect::None));
    key(&mut v, KeyCode::Esc);
    assert!(matches!(v.action(Action::SendGeneration), Effect::None));
}

#[test]
fn generated_prompt_preserves_scope_and_quotes_shell_arguments() {
    let temp = Temp::new();
    let special = temp.0.join("project ' $(touch BAD)");
    fs::create_dir(&special).unwrap();
    let store = Store::new(&temp.0, scope()).unwrap();
    let prompt = host::generation_prompt(
        &special,
        &store,
        &PathBuf::from("/bin/callstack"),
        "Trace checkout",
        Some(&flow()),
    )
    .unwrap();
    assert!(prompt.contains("Do not edit project source"));
    assert!(prompt.contains("'--session' 'session'"));
    assert!(prompt.contains("'--workspace' 'w1'"));
    assert!(prompt.contains("'\\''"));
    assert!(prompt.contains("Existing flow (untrusted data"));
    assert!(
        host::generation_prompt(&special, &store, &PathBuf::from("/bin/callstack"), "", None)
            .is_err()
    );
}

#[test]
fn mouse_tools_and_small_panels_keep_controls_accessible() {
    let mut v = view();
    v.action(Action::Tools);
    click_choice(&mut v, "Changes only");
    assert!(v.changes_only);
    for action in [
        Action::Tools,
        Action::Search,
        Action::Trace,
        Action::Generate,
    ] {
        v.action(action);
        for (w, h) in [(29, 16), (40, 24), (100, 40)] {
            let screen = v.render(w, h);
            assert!(screen.lines.len() <= h);
            assert!(
                screen
                    .hits
                    .iter()
                    .all(|hit| hit.x < hit.end && hit.end < w && hit.y < h)
            );
            assert!(
                screen
                    .hits
                    .iter()
                    .any(|hit| matches!(hit.target, Target::Action(Action::Cancel)))
            );
        }
        key(&mut v, KeyCode::Esc);
    }
}

#[test]
fn search_does_not_treat_shortcut_characters_as_actions() {
    let mut v = view();
    v.action(Action::Search);
    for c in "qgofd123".chars() {
        assert!(matches!(key(&mut v, KeyCode::Char(c)), Effect::None));
    }
    assert_eq!(v.panel.as_ref().unwrap().input, "qgofd123");
    assert!(!v.busy);
    key(&mut v, KeyCode::Esc);
    assert!(v.panel.is_none());
}

#[test]
fn selected_type_is_chosen_by_mouse_not_automatic_generation() {
    let mut v = view();
    v.action(Action::Trace);
    assert!(
        v.panel
            .as_ref()
            .unwrap()
            .items
            .iter()
            .all(|(_, c)| matches!(c, Choice::Type(_)))
    );
    click_choice(&mut v, "Order");
    assert_eq!(v.trace.as_deref(), Some("Order"));
    assert!(v.generation_target.is_none());
}

#[test]
fn source_wrapping_preserves_indentation_and_spaces() {
    assert_eq!(
        herdr_callstack::ui::wrap_source("    let x =  1;", 80),
        ["    let x =  1;"]
    );
}

#[test]
fn confirmed_request_remains_readable_and_scrollable_in_small_panes() {
    let mut v = view();
    v.action(Action::Generate);
    v.panel.as_mut().unwrap().input = "Long request ".repeat(35);
    v.action(Action::FindAgents);
    v.accept_agents(Ok(vec![host::AgentTarget {
        pane: "w1:p9".into(),
        label: "Agent".into(),
        session: "s".into(),
    }]));
    click_choice(&mut v, "Agent");
    let screen = v.render(29, 16);
    let hit = screen
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::Action(Action::PanelDown)))
        .unwrap();
    v.event(
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            row: hit.y as u16,
            column: hit.x as u16,
            modifiers: KeyModifiers::NONE,
        }),
        &screen,
        Instant::now(),
    );
    assert!(v.panel.as_ref().unwrap().scroll > 0);
    assert!(
        v.render(29, 16)
            .hits
            .iter()
            .any(|h| matches!(h.target, Target::Action(Action::SendGeneration)))
    );
}
