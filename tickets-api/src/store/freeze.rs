//! TICKETS-6: **freezing a scope** — owner, 2026-10-12: *"we need to be
//! able to freeze a scope so that no new children of that scope can be
//! created without first unfreezing it. I hate seeing scope growing."*;
//! 2026-10-13: *"freeze should also be a freeze of adding new numbers under
//! that shortcode."*
//!
//! **The mark.** A row's `frozen`, only ever true on a scope, moved by
//! [`Store::freeze`] alone (admin, both ways; idempotent). It moves the
//! ticket's cell (the filter cell `frozen`) and `updated_unix`, nothing
//! else; one [`Change::FrozenChanged`] when it moved (and its group's
//! `GroupChanged` when it roots a shortcode: the row's ❄).
//!
//! **What it refuses** is one check, [`super::rules::frozen_refusal`] (a
//! parent, and the shortcode a birth numbers in) and
//! [`super::rules::parent_refusal`] / [`super::rules::kind_refusal`], which
//! every write that gives a ticket a parent or a kind asks: a new child of
//! the frozen scope, a move under it, a new number in the shortcode it
//! roots, a new shortcode hosted under it, a reshortcode into its
//! shortcode, and its kind changed off a scope. A shortcode hosted under
//! one of its children is its own and keeps growing; moving a child out is
//! allowed.

use super::*;
use crate::access::{Admin, Permit};

impl Store {
    /// TICKETS-6: the ticket `on` names frozen (`frozen`) or unfrozen. Only
    /// a scope freezes (refused in band otherwise); unfreezing anything
    /// that is not frozen, or freezing what is, is no write and no
    /// broadcast, answered `FrozenChanged` with the ticket as it is.
    /// Otherwise the row commits (its `frozen` and `updated_unix`) with its
    /// cell move, then the cache, then one `FrozenChanged`.
    pub async fn freeze(&self, on: Permit<Admin>, frozen: bool) -> Result<Change, StoreError> {
        // ACCESS-11: of the ticket `on` is for.
        let id = crate::address::folded(on.id());
        let serial = self.serial().await;
        let (mut row, written) = self.row(&id)?;
        if frozen && row.kind != Kind::Scope {
            return Err(StoreError::Invalid(format!(
                "{id} is a {}, not a scope: only a scope freezes (nothing changed)",
                row.kind.as_str()
            )));
        }
        if row.frozen == frozen {
            return Ok(Change::FrozenChanged { ticket: self.full_one(row, written).await? });
        }
        row.frozen = frozen;
        row.updated_unix = chrono::Utc::now().timestamp();
        let (derived, under) = self.cell_derived(&row);
        let announced = derived.announced();
        let Derived { stored, moved } = derived;
        let commit = Self::row_commit(row);
        let cache = Arc::clone(&self.tickets);

        self.write(
            serial,
            move |tx| {
                let out = commit(tx)?;
                stored.write(tx)?;
                Ok(out)
            },
            move |cache, (row, _)| {
                // AGS-23: a shortcode root's ❄ is on its group's row.
                let groups = cache.amended(row.clone(), true);
                let moved = cache.take(moved);
                debug_assert!(moved.is_empty(), "a freeze moves no roll-up");
                groups
            },
            move |(row, body), groups, tx| {
                let change = Change::FrozenChanged { ticket: whole(row, body) };
                tx.send(change.clone());
                for group in groups {
                    tx.send(Change::GroupChanged { group });
                }
                if let Some(moves) = announced {
                    tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    tx.send(change);
                }
                change
            },
        )
        .await
    }
}
