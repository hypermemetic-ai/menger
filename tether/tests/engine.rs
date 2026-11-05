//! TETHER-7: the substrate call path alone — substrate's service built in this
//! process behind its gate, called with its own `ClaudeCodeClient` over
//! LOCAL-6's `HubTransport` as tether's identity. No socket anywhere.

use futures::StreamExt;
use plexus_client::ClientEvent;
use plexus_substrate::activations::claudecode::{ChatEvent, CreateResult, ListResult, Model};
use tether::Engine;

fn stub(dir: &std::path::Path) -> String {
    let stub = dir.join("claude-stub");
    std::fs::write(
        &stub,
        concat!(
            "#!/bin/sh\n",
            "echo '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"stub-session\",\"model\":\"stub\",\"cwd\":\".\",\"tools\":[]}'\n",
            "echo '{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"role\":\"assistant\",\"model\":\"stub\",\"content\":[{\"type\":\"text\",\"text\":\"in memory\"}]}}'\n",
            "echo '{\"type\":\"result\",\"subtype\":\"success\",\"session_id\":\"stub-session\",\"is_error\":false,\"result\":\"in memory\"}'\n",
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    stub.to_string_lossy().into_owned()
}

#[tokio::test]
async fn claudecode_through_its_own_client_in_memory_as_tether() {
    let dir = tempfile::tempdir().unwrap();
    assert!(plexus_substrate::activations::storage::set_data_dir(dir.path().join("data")));
    let engine = Engine::build(Some(stub(dir.path()))).await;
    let cc = engine.claudecode();
    let work = dir.path().to_string_lossy().into_owned();

    let created = cc.create_all("engine-1".into(), work, Model::Haiku, None, None, None).await.unwrap();
    assert!(matches!(created.as_slice(), [CreateResult::Ok { .. }]), "{created:?}");

    // Owned by tether: its list has it.
    let listed = cc.list_all().await.unwrap();
    match listed.as_slice() {
        [ListResult::Ok { sessions }] => assert!(sessions.iter().any(|s| s.name == "engine-1"), "{sessions:?}"),
        other => panic!("{other:?}"),
    }

    // A turn streams through.
    let mut chat = cc.chat("engine-1".into(), "hello".into(), None, None);
    let mut said = Vec::new();
    while let Some(ev) = chat.next().await {
        if let Ok(ClientEvent::Data(ev)) = ev {
            said.push(ev);
        }
    }
    assert!(said.iter().any(|e| matches!(e, ChatEvent::Content { text } if text == "in memory")), "{said:?}");
    assert!(matches!(said.last(), Some(ChatEvent::Complete { .. })), "{said:?}");

    // The gate holds tether to editor in the substrate inside it: `bash`
    // is admin's. (One service per process — two would race on its databases.)
    drop(chat);
    let t = engine.claudecode().into_transport();
    let mut s = plexus_client::Transport::call(&t, plexus_client::MethodRef::new("bash", "execute"), serde_json::json!({ "command": "true" }));
    match s.next().await {
        Some(Ok(plexus_client::StreamItem::Error { code, message, .. })) => {
            assert_eq!(code.as_deref(), Some("-32003"), "{message}");
        }
        other => panic!("bash is admin's: {other:?}"),
    }
}
