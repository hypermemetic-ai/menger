//! **Who is on a document** (V2-29, the owner: *"we should see that they
//! have a connection open on that document too"*). Each open connection on
//! a document is a presence entry — its principal, its name, person or
//! agent, a colour, and its caret and selection as the browser's Yjs
//! awareness says them — relayed to everyone on that document through
//! `watch` (`present`, `left`).
//!
//! An entry is **held** by an open `docs.watch { id, session }`: it comes
//! with the watch and goes when the watch does (the connection closed, the
//! page left). `docs.here` moves its caret. A writer with no watch open —
//! an agent's `docs.save` over synapse — is present while it writes, and
//! **idles out** [`IDLE_MS`] after its last word; a held entry idles out
//! after [`HELD_IDLE_MS`] of silence and comes back with its next.

use crate::events::Present;
use crate::store::{now_ms, Change, DocsStore};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// How long an entry no watch holds stays after its last word.
pub const IDLE_MS: i64 = 30_000;
/// How long a held entry stays silent before it idles out.
pub const HELD_IDLE_MS: i64 = 10 * 60_000;

/// The colours a presence is drawn in — chosen by its principal, so a
/// person keeps theirs across documents (the carets' own, as y-codemirror
/// draws them).
pub const COLOURS: [&str; 8] = ["#d9480f", "#2b8a3e", "#1971c2", "#9c36b5", "#e8590c", "#0c8599", "#c2255c", "#5f3dc4"];

#[derive(Debug, Clone)]
struct Entry {
    present: Present,
    org: String,
    held: u32,
    last: i64,
}

/// Every presence entry, by document and session.
#[derive(Debug, Default)]
pub struct Presence {
    entries: Mutex<HashMap<(String, String), Entry>>,
}

/// Who an entry is: what the caller's identity says.
#[derive(Debug, Clone)]
pub struct Who {
    pub principal: String,
    pub name: String,
    pub agent: bool,
}

/// The colour of `principal`.
pub fn colour_of(principal: &str) -> &'static str {
    let h = principal.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    COLOURS[(h as usize) % COLOURS.len()]
}

impl DocsStore {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), Entry>> {
        self.presence.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `who` is on `id` (in `org`) as `session` — held by a watch when
    /// `held`, else until it idles out; its caret as `client` and
    /// `awareness` say, when they do. Said as `present` to everyone on it.
    #[allow(clippy::too_many_arguments)]
    pub fn arrive(&self, id: &str, org: &str, session: &str, who: &Who, held: bool, client: Option<u64>, awareness: Option<String>) -> Present {
        self.sweep();
        let key = (id.to_string(), session.to_string());
        let mut entries = self.entries();
        let e = entries.entry(key).or_insert_with(|| Entry {
            present: Present { id: id.to_string(), session: session.to_string(), principal: who.principal.clone(), name: who.name.clone(), agent: who.agent, colour: colour_of(&who.principal).to_string(), client: None, awareness: None, at: now_ms() },
            org: org.to_string(),
            held: 0,
            last: now_ms(),
        });
        if held {
            e.held += 1;
        }
        if client.is_some() {
            e.present.client = client;
        }
        if awareness.is_some() {
            e.present.awareness = awareness;
        }
        e.last = now_ms();
        e.present.at = e.last;
        let present = e.present.clone();
        drop(entries);
        self.said(Change::Present { org: org.to_string(), present: present.clone() });
        present
    }

    /// Whether `session` on `id` is someone else's than `principal`.
    pub fn taken(&self, id: &str, session: &str, principal: &str) -> bool {
        self.entries().get(&(id.to_string(), session.to_string())).is_some_and(|e| e.present.principal != principal)
    }

    /// A watch holding `session` on `id` ended: the entry goes when no
    /// other watch holds it. Said as `left`.
    pub fn leave(&self, id: &str, session: &str) {
        let key = (id.to_string(), session.to_string());
        let mut entries = self.entries();
        let gone = match entries.get_mut(&key) {
            Some(e) if e.held > 1 => {
                e.held -= 1;
                None
            }
            Some(_) => entries.remove(&key),
            None => None,
        };
        drop(entries);
        if let Some(e) = gone {
            self.said(Change::Left { id: e.present.id, org: e.org, session: e.present.session, client: e.present.client });
        }
    }

    /// Everyone on `id` now.
    pub fn present(&self, id: &str) -> Vec<Present> {
        self.sweep();
        let mut out: Vec<Present> = self.entries().values().filter(|e| e.present.id == id).map(|e| e.present.clone()).collect();
        out.sort_by(|a, b| a.at.cmp(&b.at).then(a.session.cmp(&b.session)));
        out
    }

    /// The entries idle past their time, gone — each said as `left`.
    pub fn sweep(&self) {
        let now = now_ms();
        let mut entries = self.entries();
        let idle: Vec<(String, String)> = entries.iter().filter(|(_, e)| now - e.last > if e.held > 0 { HELD_IDLE_MS } else { IDLE_MS }).map(|(k, _)| k.clone()).collect();
        let gone: Vec<Entry> = idle.into_iter().filter_map(|k| entries.remove(&k)).collect();
        drop(entries);
        for e in gone {
            self.said(Change::Left { id: e.present.id, org: e.org, session: e.present.session, client: e.present.client });
        }
    }
}

/// Holds a watch's presence: the entry goes when this does (the stream
/// dropped — its connection closed).
pub struct Held {
    pub(crate) store: Arc<DocsStore>,
    pub(crate) id: String,
    pub(crate) session: String,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.store.leave(&self.id, &self.session);
    }
}
