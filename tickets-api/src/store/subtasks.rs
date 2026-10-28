//! TICKETS-28: a subtask born — `<parent>.N`, the next free number, hidden
//! (TICKETS-27), in `create`'s own write: its rules, its permits, its
//! transition and revision. The key and the shortcode's number are two
//! things (TICKETS-26): the birth spends the next number of its shortcode,
//! as a rename into `.N` leaves it spent.

use super::*;

impl Cache {
    /// The key of `parent`'s next subtask: past the highest `.N` it holds,
    /// and past every one a delete, a rename or a pin took out — a number is
    /// never given out twice.
    pub(super) fn next_subtask(&self, parent: &str) -> String {
        let held = self
            .children
            .of(parent)
            .into_iter()
            .filter_map(|(shortcode, seq)| self.order.get(&OrderKey { shortcode, seq }))
            .filter_map(|row| row.id.as_str().strip_prefix(parent)?.strip_prefix('.')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        (held + 1..)
            .map(|n| format!("{parent}.{n}"))
            .find(|key| !self.at.contains_key(key) && self.tombstones.get(key).is_none() && self.shortcodes.pin(key).is_none())
            .expect("a number is free")
    }

    /// `parent`'s letters (its shortcode's code), by any short form — what a
    /// subtask files its epic under. `None`: no such ticket.
    fn code_of(&self, parent: &str) -> Option<String> {
        match self.find_ticket(parent, shortcodes::OrgReach::All) {
            shortcodes::Found::One(key) => self.get(&key).map(|row| self.shortcodes.code(&row.shortcode).to_string()),
            _ => None,
        }
    }
}

impl Store {
    /// TICKETS-28: [`Store::create_by`] for a subtask of `new.parent`, which
    /// must be named — its key the parent's with the next free `.N`, its
    /// shortcode the parent's (`new.epic` is ignored), `hidden` true.
    /// Everything else is `create`'s: the kind, severity, blockers and body
    /// as given, the proofs of `at`, one write. A scope or a doc (which root
    /// a shortcode) is refused.
    pub async fn create_subtask_by(&self, at: impl Into<Filing>, mut new: NewTicket, act: Act) -> Result<Ticket, StoreError> {
        let Some(parent) = new.parent.clone().filter(|p| !p.trim().is_empty()) else {
            return Err(StoreError::Invalid("a subtask needs its parent — nothing written".into()));
        };
        let code = self.tickets.read().expect("store lock poisoned").code_of(parent.trim());
        let Some(code) = code else {
            return Err(StoreError::Invalid(format!("no ticket {} to hang a subtask under — nothing written", parent.trim())));
        };
        new.epic = code;
        self.create_from(at.into(), new, None, act, true).await.map(|(ticket, _)| ticket)
    }

    /// [`Store::create_subtask_by`] as nobody signed in.
    pub async fn create_subtask(&self, at: impl Into<Filing>, new: NewTicket) -> Result<Ticket, StoreError> {
        self.create_subtask_by(at, new, Act::default()).await
    }
}
