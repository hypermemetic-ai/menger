//! INPUT-3: the writes of one ticket input — `answer`, `clear_answer`,
//! `close_input`, `reopen_input` — out of `store/mod.rs` (MENGER-29's
//! budget) when each took a `Who`: the source, and the caller's
//! principal and name, kept on the history entry.

use super::inputs::{self, Who};
use super::{lineage, queue, Change, Derived, InputWrite, Store, StoreError};
use crate::access::{Editor, Permit};
use crate::types::{AnswerSource, InputState, InputValue};
use std::sync::Arc;

impl Store {
    /// INPUT-3: records `value` as the answer to `ticket`'s input `input` —
    /// checked against its kind and options ([`crate::inputs::check`]; a
    /// `multi`'s set kept in the declared order), with `rationale` (a blank
    /// one is none), who (`who`: its source, principal and name) and the time; one history entry, every call,
    /// the same value again included. Refused when `ticket` names no ticket
    /// ([`StoreError::NotFound`]), the input is not declared, removed or
    /// closed, or the value does not answer it — nothing written. The body
    /// is never touched, nor the ticket's row.
    pub async fn answer(
        &self,
        on: Permit<Editor>,
        input: &str,
        value: InputValue,
        rationale: Option<String>,
        who: impl Into<Who>,
    ) -> Result<InputWrite, StoreError> {
        self.input_write(on.id(), input, inputs::Op::Answer { value, rationale, who: who.into() }).await
    }

    /// INPUT-3: clears the input's answer — `open` again, the clear in its
    /// history, with `source` (INPUT-9: `app` when the host clears it for a
    /// frame). One holding no answer is [`InputWrite::Unchanged`]: nothing
    /// written. Refused as `answer` is, but for the value: closed, removed.
    pub async fn clear_answer(&self, on: Permit<Editor>, input: &str, who: impl Into<Who>) -> Result<InputWrite, StoreError> {
        self.input_write(on.id(), input, inputs::Op::Clear { who: who.into() }).await
    }

    /// INPUT-3: closes the input — frozen, with its answer or without one;
    /// an answer or a clear is refused until it is reopened. A closed one is
    /// [`InputWrite::Unchanged`]. Refused when removed.
    pub async fn close_input(&self, on: Permit<Editor>, input: &str) -> Result<InputWrite, StoreError> {
        self.close_input_as(on, input, AnswerSource::Api).await
    }

    /// [`Store::close_input`], saying who closed it.
    pub async fn close_input_as(&self, on: Permit<Editor>, input: &str, who: impl Into<Who>) -> Result<InputWrite, StoreError> {
        self.input_write(on.id(), input, inputs::Op::Close { who: who.into() }).await
    }

    /// INPUT-3: reopens a closed input — `answered` again when it holds an
    /// answer, else `open`. One not closed is [`InputWrite::Unchanged`].
    /// Refused when removed.
    pub async fn reopen_input(&self, on: Permit<Editor>, input: &str) -> Result<InputWrite, StoreError> {
        self.reopen_input_as(on, input, AnswerSource::Api).await
    }

    /// [`Store::reopen_input`], saying who reopened it.
    pub async fn reopen_input_as(&self, on: Permit<Editor>, input: &str, who: impl Into<Who>) -> Result<InputWrite, StoreError> {
        self.input_write(on.id(), input, inputs::Op::Reopen { who: who.into() }).await
    }

    /// INPUT-3: one change to one input, through the write path every write
    /// takes (MNG-20): worked out under the write-serial lock from the
    /// cache's record of the input, one history row and the input's state
    /// committed, the cache moved, `InputChanged` broadcast. A change that
    /// changes nothing writes and says nothing.
    async fn input_write(&self, ticket: &str, input: &str, op: inputs::Op) -> Result<InputWrite, StoreError> {
        // ORG-3: by any address; TICKETS-35: a permit's key read as a key.
        let ticket = self.keyed(ticket)?;
        let input = input.trim().to_string();
        let serial = self.serial().await;
        let (epic, current, row) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let row = cache.get(&ticket).ok_or_else(|| StoreError::NotFound(ticket.clone()))?;
            let current = cache.inputs.get(&ticket, &input).cloned().ok_or_else(|| StoreError::InputUnknown {
                ticket: ticket.clone(),
                input: input.clone(),
                known: cache.inputs.of(&ticket).iter().map(|i| i.id.to_string()).collect(),
            })?;
            (row.shortcode.clone(), current, row.clone())
        };
        let moved = inputs::apply(&current, op, chrono::Utc::now().timestamp()).map_err(|refused| match refused {
            inputs::Refused::Removed => StoreError::InputRemoved { ticket, input },
            inputs::Refused::Withdrawn => StoreError::InputWithdrawn { ticket, input },
            inputs::Refused::Closed => StoreError::InputClosed { ticket, input },
            inputs::Refused::Invalid(reason) => StoreError::InputInvalid { ticket, input, reason },
        })?;
        let Some((next, entry)) = moved else {
            return Ok(InputWrite::Unchanged(current));
        };
        // MENGER-12: an input opened or settled may move the ticket in or out
        // of `asking` — its cell, in the same write.
        let (asked, asks) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            (cache.inputs.asking(&row.id), cache.inputs.asking_with(&row.id, &next))
        };
        // FILTER-30: an answer or a clear touches the ticket.
        let touches = inputs::touches(&entry);
        // MENGER-39: closing the input a queue entry names takes it out.
        let closing = (next.state == InputState::Closed).then(|| {
            let cache = self.tickets.read().expect("store lock poisoned");
            queue::Closing::of(&cache, &row.id, &next.id, entry.at_unix)
        });
        let (derived, mut under) = match asked != asks {
            true => {
                let (d, u) = self.cell_derived(&row, Some(asks));
                (Some(d), u)
            }
            // V2-20: a close or a reopen touches nothing, but `asked` reads
            // an input's state: `touch_before` holds both kinds of watch.
            false => {
                let cache = self.tickets.read().expect("store lock poisoned");
                (None, self.touch_before(&cache, &[(&row.shortcode, row.seq)]))
            }
        };
        if let (Some(Some(_)), None) = (&closing, &derived) {
            let cache = self.tickets.read().expect("store lock poisoned");
            under = self.under_before(&cache, || lineage::Touched::default().row(&row));
        }
        let closing = closing.flatten();
        let (closing_tx, closing_cache, queue_epic) = (closing.clone(), closing.clone(), epic.clone());
        let at = entry.at_unix;
        let announced = derived.as_ref().and_then(Derived::announced);
        let (cells_stored, moved) = match derived {
            Some(Derived { stored, moved }) => (Some(stored), Some(moved)),
            None => (None, None),
        };
        let (stored, kept) = (next.clone(), next.clone());
        let cache = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                inputs::record(tx, &stored, &entry)?;
                if let Some(cells) = &cells_stored {
                    cells.write(tx)?;
                }
                if let Some(c) = &closing_tx {
                    c.commit(tx)?;
                }
                Ok(())
            },
            move |cache, _| {
                cache.inputs.put(kept);
                if let Some(c) = &closing_cache {
                    c.apply(cache);
                }
                if touches {
                    cache.log.touch(&row.shortcode, row.seq, at);
                }
                if let Some(moved) = moved {
                    let groups = cache.take(moved);
                    debug_assert!(groups.is_empty(), "an input moves no roll-up");
                }
            },
            move |(), (), tx| {
                let _ = tx.send(Change::InputChanged {
                    shortcode: epic,
                    input: next.clone(),
                });
                if let Some(c) = closing {
                    tx.send(c.change(queue_epic));
                }
                if let Some(moves) = announced {
                    tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    tx.send(change);
                }
                InputWrite::Changed(next)
            },
        )
        .await
    }
}
