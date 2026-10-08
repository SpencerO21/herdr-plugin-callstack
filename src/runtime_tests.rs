use super::*;
use crate::model::{Flow, Scope};

#[test]
fn native_file_event_updates_the_view_before_the_fallback_check() {
    let root = std::env::temp_dir().join(format!("callstack-watcher-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let store = Store::new(
        &root,
        Scope {
            namespace: "test".into(),
            workspace: "test".into(),
            session: "watch".into(),
        },
    )
    .unwrap();
    store.ensure_dir().unwrap();
    let (tx, rx) = mpsc::channel();
    watch(store.clone(), tx);
    assert!(
        matches!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), Message::Data(records) if records.is_empty())
    );
    let flow: Flow =
        serde_json::from_str(r#"{"name":"Live","frames":[{"fn":"updated"}]}"#).unwrap();
    store.publish(flow, None).unwrap();
    assert!(
        matches!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), Message::Data(records) if records.len() == 1 && records[0].flow.name == "Live")
    );
    drop(rx);
    std::fs::remove_dir_all(root).unwrap();
}
