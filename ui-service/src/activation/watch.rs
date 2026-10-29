//! UI-9: `ui.watch` as its stream — the store's changes (a `live` on every
//! pointer move, a `slot_changed` on every set and clear), the caller held
//! as `tickets.watch` holds it (ACCESS-12, `access::Following`): verified
//! again when the grants move, a sign-out or the expiry comes; refused, the
//! stream ends with one `watch_closed { reason, message }`.

use crate::events::UiEvent;
use crate::store::{UiChange, UiStore};
use async_stream::stream;
use futures::{Stream, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use menger_tickets_api::access::{Access, Following, VerifiedUser};
use menger_tickets_api::store::Store;
use std::sync::Arc;

/// The stream: the caller followed from the call (`Following::of`, the
/// tickets store's grants generation), then [`changes`].
pub(super) fn watch_stream(store: Arc<UiStore>, access: Access, tickets: Option<Arc<Store>>, user: VerifiedUser) -> impl Stream<Item = UiEvent> + Send + 'static {
    let opened = match tickets {
        Some(t) => Following::of(&access, &t, user).map(|f| changes(store, Some(f))),
        None => Err("ui.watch: this server holds no caller's grants to follow it by".to_string()),
    };
    stream! {
        match opened {
            Ok(inner) => {
                let mut inner = Box::pin(inner);
                while let Some(e) = inner.next().await {
                    yield e;
                }
            }
            Err(message) => yield UiEvent::Error { message },
        }
    }
}

/// Waits for what could change the caller — never, with nobody to hold.
async fn caller_moved(following: &mut Option<Following>) {
    match following {
        Some(f) => f.changed().await,
        None => std::future::pending().await,
    }
}

/// The store's changes as the watch says them, held to `following` (none:
/// nobody to hold — the tests'): subscribed at the call, before the first
/// poll, and opened with `live` for the pointer as it stands.
pub(super) fn changes(store: Arc<UiStore>, following: Option<Following>) -> impl Stream<Item = UiEvent> + Send + 'static {
    // Subscribed first, then the pointer read: no move between is missed.
    let mut rx = store.subscribe();
    let opening = store.pointer().map(|p| p.map(|p| p.hash));
    stream! {
        let mut following = following;
        match opening {
            Ok(version) => yield UiEvent::Live { version },
            Err(e) => {
                yield UiEvent::Error { message: e.to_string() };
                return;
            }
        }
        loop {
            let woke = tokio::select! {
                biased;
                () = caller_moved(&mut following) => None,
                r = rx.recv() => Some(r),
            };
            // ACCESS-12: the caller as it stands, before anything is relayed.
            if let Some(f) = following.as_mut() {
                if let Err(closed) = f.again() {
                    yield UiEvent::WatchClosed { reason: closed.reason, message: closed.message };
                    return;
                }
            }
            match woke {
                None => continue,
                Some(Ok(UiChange::Live { version })) => yield UiEvent::Live { version: Some(version) },
                Some(Ok(UiChange::SlotChanged(s))) => yield UiEvent::SlotChanged { layout: s.layout, slot: s.slot, component: s.component, version: s.version },
                Some(Err(RecvError::Lagged(missed))) => yield UiEvent::Lagged { missed },
                Some(Err(RecvError::Closed)) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NewVersion;

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
