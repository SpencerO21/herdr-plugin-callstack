use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use herdr_callstack::{
    host,
    model::{Flow, Record, Scope},
    store::{Cache, Store, read_limited},
    ui::{Action, Effect, Target, View, clip},
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "herdr-callstack-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn scope() -> Scope {
    Scope {
        namespace: "server-a".into(),
        workspace: "w1".into(),
        session: "agent-a".into(),
    }
}
fn flow(name: &str) -> Flow {
    serde_json::from_value(serde_json::json!({ "name": name, "status": "current", "frames": [{ "fn": "root", "loc": "source.rs:12", "calls": (0..30).map(|i| serde_json::json!({"fn":format!("child{i}"),"loc":format!("source.rs:{}",i+1)})).collect::<Vec<_>>() }], "types": { "T": "x".repeat(1500) } })).unwrap()
}
fn view() -> View {
    let records = ["A", "B"].map(|name| {
        Arc::new(Record {
            flow: flow(name),
            scope: scope(),
            project_root: Some("/project".into()),
            updated_at: "2026-10-08T00:00:00Z".into(),
            source_hashes: None,
            warnings: vec![],
            drifted_paths: vec![],
        })
    });
    let mut view = View::new(scope(), None);
    view.update(records.into());
    view
}
fn mouse(x: usize, y: usize) -> Event {
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x as u16,
        row: y as u16,
        modifiers: KeyModifiers::NONE,
    })
}
fn click(view: &mut View, action: Action) -> Effect {
    let mut screen = view.render(60, 25);
    if !screen
        .hits
        .iter()
        .any(|h| matches!(h.target, Target::Action(a) if a == action))
    {
        let more = screen
            .hits
            .iter()
            .find(|h| matches!(h.target, Target::Action(Action::More)))
            .unwrap();
        view.event(mouse(more.x, more.y), &screen, Instant::now());
        screen = view.render(60, 25);
    }
    let hit = screen
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::Action(a) if a == action))
        .unwrap();
    view.event(mouse(hit.x, hit.y), &screen, Instant::now())
}

#[test]
fn storage_preserves_scope_and_replaces_names() {
    let temp = Temp::new();
    let store = Store::new(&temp.0, scope()).unwrap();
    store.publish(flow("../A"), None).unwrap();
    let mut updated = flow("../A");
    updated.description = Some("Updated".into());
    store.publish(updated, None).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        store.list().unwrap()[0].flow.description.as_deref(),
        Some("Updated")
    );
    for key in 0..3 {
        let mut s = scope();
        match key {
            0 => s.namespace = "other".into(),
            1 => s.workspace = "other".into(),
            _ => s.session = "other".into(),
        }
        assert!(Store::new(&temp.0, s).unwrap().list().unwrap().is_empty());
    }
    store.delete("../A").unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn unchanged_files_are_never_reparsed() {
    let temp = Temp::new();
    let store = Store::new(&temp.0, scope()).unwrap();
    store.publish(flow("A"), None).unwrap();
    store.publish(flow("B"), None).unwrap();
    let mut cache = Cache::default();
    assert!(cache.refresh(&store).unwrap());
    assert_eq!(cache.reads, 2);
    let before = cache.records();
    for _ in 0..100 {
        assert!(!cache.refresh(&store).unwrap());
    }
    assert_eq!(cache.reads, 2);
    assert!(Arc::ptr_eq(&before[0], &cache.records()[0]));
    let mut changed = flow("A");
    changed.frames[0].function = "edit".into();
    store.publish(changed, None).unwrap();
    assert!(cache.refresh(&store).unwrap());
    assert_eq!(cache.reads, 3);
    assert!(Arc::ptr_eq(&before[1], &cache.records()[1]));
    store.delete("A").unwrap();
    assert!(cache.refresh(&store).unwrap());
    assert_eq!(cache.reads, 3);
}

#[test]
fn malformed_update_retains_previous_cache_and_recovers() {
    let temp = Temp::new();
    let store = Store::new(&temp.0, scope()).unwrap();
    store.publish(flow("A"), None).unwrap();
    let mut cache = Cache::default();
    cache.refresh(&store).unwrap();
    fs::write(store.file("A"), "{bad").unwrap();
    assert!(cache.refresh(&store).is_err());
    assert_eq!(cache.records()[0].flow.name, "A");
    store.publish(flow("A"), None).unwrap();
    assert!(cache.refresh(&store).unwrap());
}

#[test]
fn validation_rejects_bad_documents_and_limits_input() {
    let mut f = flow("A");
    f.frames.clear();
    assert!(f.validate().is_err());
    let mut f = flow("A");
    f.frames[0].change = Some("wrong".into());
    assert!(f.validate().is_err());
    let mut f = flow("A");
    f.frames[0].calls[0].function = "".into();
    assert!(f.validate().is_err());
    assert!(read_limited(&b"12345"[..], 4).is_err());
    assert!(
        serde_json::from_str::<Flow>(r#"{"name":"A","frames":[{"fn":"x","concurrent":"yes"}]}"#)
            .is_err()
    );
}

#[test]
fn every_mouse_control_remains_available() {
    let mut v = view();
    click(&mut v, Action::Next);
    assert_eq!(v.flow_name.as_deref(), Some("B"));
    click(&mut v, Action::Prev);
    assert_eq!(v.flow_name.as_deref(), Some("A"));
    click(&mut v, Action::Collapse);
    assert_eq!(v.rows.len(), 1);
    click(&mut v, Action::Expand);
    assert_eq!(v.rows.len(), 31);
    click(&mut v, Action::Details);
    assert!(v.detail);
    click(&mut v, Action::PageDown);
    v.render(60, 25);
    assert!(v.detail_offset > 0);
    click(&mut v, Action::Details);
    assert!(!v.detail);
    click(&mut v, Action::Down);
    assert_eq!(v.selected, 1);
    assert!(matches!(click(&mut v, Action::Open), Effect::Open(_, _)));
    // A slow host operation must not block local navigation or permit duplicate opens.
    click(&mut v, Action::Next);
    assert_eq!(v.flow_name.as_deref(), Some("B"));
    assert!(matches!(click(&mut v, Action::Open), Effect::None));
    v.busy = false;
    click(&mut v, Action::Delete);
    assert_eq!(v.confirm.as_deref(), Some("B"));
    click(&mut v, Action::Cancel);
    assert!(v.confirm.is_none());
    click(&mut v, Action::Delete);
    assert!(matches!(click(&mut v, Action::Confirm), Effect::Delete(name) if name == "B"));
    assert!(matches!(click(&mut v, Action::Quit), Effect::Quit));
}

#[test]
fn details_keep_tree_visible_and_source_separate() {
    let mut v = view();
    click(&mut v, Action::Details);
    let screen = v.render(80, 30);
    assert!(
        screen
            .hits
            .iter()
            .any(|h| matches!(h.target, Target::Row(_)))
    );
    assert!(screen.lines.iter().any(|l| l.text.starts_with("DETAILS |")));
    assert!(screen.lines.iter().any(|l| l.text.starts_with("Source: ")));
    assert!(screen.lines.iter().any(|l| l.color == 7));
    assert!(
        !screen
            .hits
            .iter()
            .any(|h| matches!(h.target, Target::Action(Action::Delete)))
    );
}

#[test]
fn details_wrap_at_words_and_preserve_long_names() {
    use herdr_callstack::ui::wrap;
    assert_eq!(
        wrap("Open Diff dn to inspect", 12),
        ["Open Diff dn", "to inspect"]
    );
    assert_eq!(wrap("abcdefghijk", 4), ["abcd", "efgh", "ijk"]);
    assert_eq!(wrap("界界界", 4), ["界界", "界"]);
}

#[test]
fn compact_layout_keeps_controls_and_footer_inside_pane() {
    for width in [29, 36, 60, 100] {
        for height in [16, 25, 40] {
            for more in [false, true] {
                for detail in [false, true] {
                    let mut v = view();
                    v.more = more;
                    v.detail = detail;
                    let screen = v.render(width, height);
                    assert_eq!(screen.lines.len(), height);
                    assert!(screen.lines[height - 3].text.starts_with("= same"));
                    assert!(
                        screen
                            .hits
                            .iter()
                            .all(|h| h.end < width && h.y < height - 3)
                    );
                    assert!(
                        screen
                            .hits
                            .iter()
                            .any(|h| matches!(h.target, Target::Row(_)))
                    );
                }
            }
        }
    }
}

#[test]
fn row_mouse_fold_double_click_and_wheel() {
    let mut v = view();
    let now = Instant::now();
    let screen = v.render(80, 25);
    let hit = screen
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::Fold(0)))
        .unwrap();
    v.event(mouse(hit.x, hit.y), &screen, now);
    assert_eq!(v.rows.len(), 1);
    click(&mut v, Action::Expand);
    let screen = v.render(80, 25);
    let hit = screen
        .hits
        .iter()
        .find(|h| matches!(h.target, Target::Row(1)))
        .unwrap();
    assert!(matches!(
        v.event(mouse(15, hit.y), &screen, now),
        Effect::None
    ));
    assert_eq!(v.selected, 1);
    assert!(
        matches!(v.event(mouse(15, hit.y), &screen, now + Duration::from_millis(100)), Effect::Open(_, loc) if loc == "source.rs:1")
    );
    let event = Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 10,
        modifiers: KeyModifiers::NONE,
    });
    v.event(event, &screen, now);
    assert_eq!(v.selected, 2);
}

#[test]
fn narrow_panes_wrap_buttons_and_keep_legend_and_message_separate() {
    let mut v = view();
    v.message = "Opened source".into();
    let screen = v.render(36, 30);
    for hit in &screen.hits {
        assert!(hit.end <= 35 && hit.y < 30);
    }
    assert!(screen.lines[27].text.starts_with("= same"));
    assert_eq!(screen.lines[28].text, "Opened source");
    assert_eq!(clip("ab界cd", 0, 4), "ab界");
    assert!(!clip("\x1b]52;clipboard\x07", 0, 80).contains('\x1b'));
}

#[test]
fn drawing_uses_only_the_memory_snapshot() {
    let temp = Temp::new();
    let store = Store::new(&temp.0, scope()).unwrap();
    store.publish(flow("A"), None).unwrap();
    let mut v = View::new(scope(), None);
    v.update(store.list().unwrap());
    store.delete("A").unwrap();
    for _ in 0..100 {
        v.render(100, 30);
        v.action(Action::Down);
    }
    assert_eq!(v.record().unwrap().flow.name, "A");
}

#[test]
fn source_resolution_handles_spaces_and_rejects_escape_paths() {
    let temp = Temp::new();
    let path = temp.0.join("source ' $(ignored).rs");
    fs::write(&path, "one\ntwo\n").unwrap();
    let source = host::source_location(&temp.0, "source ' $(ignored).rs:2:3").unwrap();
    assert_eq!(source.file, path.canonicalize().unwrap());
    assert_eq!(source.line, 2);
    std::os::unix::fs::symlink("/etc/hosts", temp.0.join("outside.rs")).unwrap();
    assert!(host::source_location(&temp.0, "outside.rs:1").is_err());
    assert!(host::source_location(&temp.0, "source ' $(ignored).rs:0").is_err());
}

#[test]
fn host_command_has_a_timeout() {
    let start = Instant::now();
    let result = host::run_json(
        "/bin/sh",
        &["-c".into(), "exec sleep 2".into()],
        Duration::from_millis(60),
    );
    assert!(result.unwrap_err().to_string().contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn cli_round_trip_and_old_javascript_record_format() {
    let temp = Temp::new();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_herdr-callstack"))
            .args(args)
            .args([
                "--workspace",
                "test",
                "--namespace",
                "local",
                "--session",
                "test",
                "--state-dir",
                temp.0.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let store = Store::new(
        &temp.0,
        Scope {
            namespace: "local".into(),
            workspace: "test".into(),
            session: "test".into(),
        },
    )
    .unwrap();
    store.ensure_dir().unwrap();
    let old = serde_json::json!({"flow":{"name":"Old JS flow","frames":[{"fn":"old"}]},"scope":store.scope,"updatedAt":"2026-10-08T17:00:00.000Z"});
    fs::write(store.file("Old JS flow"), serde_json::to_vec(&old).unwrap()).unwrap();
    let out = run(&["show", "Old JS flow", "--json"]);
    assert!(out.status.success());
    let file = temp.0.join("input.json");
    fs::write(&file, serde_json::to_vec(&flow("New flow")).unwrap()).unwrap();
    assert!(run(&["publish", file.to_str().unwrap()]).status.success());
    let out = run(&["list", "--json"]);
    let records: Vec<Record> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(records.len(), 2);
    assert!(run(&["view", "--once"]).status.success());
    assert!(run(&["delete", "New flow"]).status.success());
    assert!(!run(&["show", "New flow"]).status.success());
}

#[test]
fn concurrent_cli_publish_preserves_all_flows() {
    let temp = Temp::new();
    let mut children = vec![];
    for i in 0..12 {
        let mut child = Command::new(env!("CARGO_BIN_EXE_herdr-callstack"))
            .args([
                "publish",
                "-",
                "--workspace",
                "w1",
                "--namespace",
                "server-a",
                "--session",
                "agent-a",
                "--state-dir",
                temp.0.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&flow(&format!("flow-{i}"))).unwrap())
            .unwrap();
        children.push(child);
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    assert_eq!(
        Store::new(&temp.0, scope()).unwrap().list().unwrap().len(),
        12
    );
}

#[test]
fn publish_checks_files_lines_names_and_flow_consistency() {
    let temp = Temp::new();
    fs::write(temp.0.join("source.rs"), "fn valid() {}\n").unwrap();
    let f: Flow = serde_json::from_value(
        serde_json::json!({"name":"audit","status":"current","types":{"Unused":"string"},"frames":[
            {"fn":"missingName","loc":"source.rs:99","change":"added","concurrent":true},
            {"fn":"gone","loc":"missing.rs:1"}
        ]}),
    )
    .unwrap();
    let (hashes, warnings) = herdr_callstack::audit::publish_checks(&f, Some(&temp.0), &[]);
    assert!(hashes.unwrap()["source.rs"].is_some());
    for text in [
        "past the end",
        "function name not found",
        "Cannot read missing.rs",
        "Current flow",
        "Type Unused",
        "parallel call",
    ] {
        assert!(
            warnings.iter().any(|w| w.contains(text)),
            "Missing warning: {text}"
        );
    }
}

#[test]
fn source_drift_is_cached_detects_deletion_and_clears_on_republish() {
    let temp = Temp::new();
    let path = temp.0.join("source.rs");
    fs::write(&path, "fn valid() {}\n").unwrap();
    let f: Flow =
        serde_json::from_str(r#"{"name":"audit","frames":[{"fn":"valid","loc":"source.rs:1"}]}"#)
            .unwrap();
    let store = Store::new(&temp.0.join("state"), scope()).unwrap();
    let record = store
        .publish(f.clone(), Some(temp.0.to_string_lossy().into()))
        .unwrap();
    assert!(record.warnings.is_empty());
    let records = vec![Arc::new(record)];
    let mut cache = herdr_callstack::audit::SourceCache::default();
    assert!(cache.refresh(&records)[0].drifted_paths.is_empty());
    for _ in 0..50 {
        cache.refresh(&records);
    }
    assert_eq!(cache.reads, 1);
    fs::write(&path, "fn valid() { changed(); }\n").unwrap();
    assert_eq!(cache.refresh(&records)[0].drifted_paths, ["source.rs"]);
    let mut view = View::new(scope(), None);
    view.update(cache.refresh(&records));
    assert!(
        view.render(140, 25)
            .lines
            .iter()
            .any(|l| l.text.contains("SOURCE CHANGED"))
    );
    let updated = store
        .publish(f, Some(temp.0.to_string_lossy().into()))
        .unwrap();
    assert!(
        cache.refresh(&[Arc::new(updated)])[0]
            .drifted_paths
            .is_empty()
    );
    fs::remove_file(&path).unwrap();
    assert_eq!(cache.refresh(&records)[0].drifted_paths, ["source.rs"]);
    assert!(host::source_path(&temp.0, "source.rs:1").is_ok());
    assert!(host::source_location(&temp.0, "source.rs:1").is_err());
}

#[test]
fn diff_mouse_button_and_shortcut_return_a_background_effect() {
    let mut v = view();
    assert!(matches!(click(&mut v, Action::Diff), Effect::Diff(_, loc) if loc == "source.rs:12"));
    v.busy = false;
    let screen = v.render(100, 30);
    let key = Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('g'),
        KeyModifiers::NONE,
    ));
    assert!(matches!(
        v.event(key, &screen, Instant::now()),
        Effect::Diff(_, _)
    ));
}

#[test]
fn diff_command_limits_paths_and_includes_local_edits() {
    let temp = Temp::new();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&temp.0)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    let file = "source ' $(touch BAD).rs";
    fs::write(temp.0.join(file), "before\n").unwrap();
    fs::write(temp.0.join("other.rs"), "before\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "base",
    ]);
    git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    fs::write(temp.0.join(file), "branch\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "branch",
    ]);
    fs::write(temp.0.join(file), "local edit\n").unwrap();
    fs::write(temp.0.join("other.rs"), "excluded\n").unwrap();
    let result = Command::new("/bin/sh")
        .args(["-c", host::DIFF_COMMAND])
        .env("CALLSTACK_DIFF_FILE", file)
        .current_dir(&temp.0)
        .output()
        .unwrap();
    assert!(result.status.success());
    let diff = String::from_utf8(result.stdout).unwrap();
    assert!(diff.contains("+local edit") && diff.contains("-before"));
    assert!(!diff.contains("other.rs"));
    assert!(!temp.0.join("BAD").exists());
    git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    let result = Command::new("/bin/sh")
        .args(["-c", host::DIFF_COMMAND])
        .env("CALLSTACK_DIFF_FILE", file)
        .current_dir(&temp.0)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("-branch")
    );
}
