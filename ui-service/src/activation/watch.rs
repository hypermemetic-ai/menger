//! UI-9: `ui.watch` as its stream — the store's changes (a `live` on every
//! pointer move, a `slot_changed` on every set and clear), the caller held
//! as `tickets.watch` holds it (ACCESS-12, `access::Following`): verified
//! again when the grants move, a sign-out or the expiry comes; refused, the
//! stream ends with one `watch_closed { reason, message }`.

use crate::events::UiEvent;
use crate::store::UiStore;
use futures::Stream;
use menger_tickets_api::access::{Access, Following, VerifiedUser};
use menger_tickets_api::store::Store;
use std::sync::Arc;

/// The stream. UI-9 phase 1: unimplemented, in band.
pub(super) fn watch_stream(store: Arc<UiStore>, access: Access, tickets: Option<Arc<Store>>, user: VerifiedUser) -> impl Stream<Item = UiEvent> + Send + 'static {
    let _ = (access, tickets, user);
    changes(store, None)
}

/// The store's changes as the watch says them, held to `following` (none:
/// nobody to hold — the tests'): subscribed at the call, before the first
/// poll, and opened with `live` for the pointer as it stands.
pub(super) fn changes(store: Arc<UiStore>, following: Option<Following>) -> impl Stream<Item = UiEvent> + Send + 'static {
    let _ = (store, following);
    futures::stream::once(std::future::ready(UiEvent::Error { message: "unimplemented: ui.watch (UI-9)".into() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NewVersion;
    use futures::StreamExt;

    fn version(n: u32) -> NewVersion {
        let mut b = tar::Builder::new(Vec::new());
        let body = format!("<!doctype html><title>v{n}</title>");
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, "index.html", body.as_bytes()).unwrap();
        let manifest = serde_json::json!({ "abi": "engine/1", "contract": format!("c{n}"), "routes": {}, "components": {}, "slots": {} }).to_string();
        NewVersion { bundle: b.into_inner().unwrap(), manifest, commit: None, pushed_by: "key:admin".into(), pushed_at: n as i64 }
    }

    /// A write between the call and the stream's first poll still arrives:
    /// the watch is subscribed at the call.
    #[tokio::test]
    async fn the_watch_subscribes_at_the_call() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(UiStore::open(&dir.path().join(crate::UI_DB_FILE)).unwrap());
        let v1 = store.push(version(1)).unwrap().version;
        let watch = changes(Arc::clone(&store), None);
        // Not yet polled.
        let v2 = store.push(version(2)).unwrap().version;
        let mut watch = Box::pin(watch);
        let mut heard: Vec<UiEvent> = Vec::new();
        while let Ok(Some(e)) = tokio::time::timeout(std::time::Duration::from_millis(500), watch.next()).await {
            heard.push(e);
        }
        let opened = |v: &String| *v == v1 || *v == v2;
        assert!(matches!(heard.first(), Some(UiEvent::Live { version: Some(v) }) if opened(v)), "opens with live for the pointer as it stood: {heard:?}");
        assert_eq!(heard.last(), Some(&UiEvent::Live { version: Some(v2.clone()) }), "the push made before the first poll is heard: {heard:?}");
        assert!(heard.len() <= 2, "nothing else: {heard:?}");
    }
}
