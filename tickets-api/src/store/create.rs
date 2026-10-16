//! The create write (MNG-1 onward), out of `store/mod.rs` (MENGER-29's
//! budget): one ticket born — its shortcode found or minted (ORG-3), its
//! parent, blockers and path checked — and, NOTE-2, the note it came from
//! marked in the same write.

use super::*;

impl Store {
    /// [`Store::create`], and — NOTE-2 — the note `from` (if any) marked as
    /// the ticket it became, in the same write.
    /// ACCESS-11: filed as `at` allows — checked under the write-serial
    /// lock, once the parent (or root) and the blockers are resolved.
    pub(super) async fn create_from(&self, at: Filing, new: NewTicket, from: Option<u64>, act: Act) -> Result<(Ticket, Option<crate::notes::Note>), StoreError> {
        // ORG-3: the shortcode as the reader names it — its code, or any
        // short form of its address — parsed here; which shortcode it is (or
        // whether it mints one) is decided under the lock, the namespace
        // first ([`Cache::target`]).
        let asked = match Address::parse(&new.epic) {
            Ok(a) if !a.names_ticket() => a,
            _ => return Err(StoreError::InvalidEpic(new.epic)),
        };
        let epic = asked.to_string();
        self.check_body(None, &new.body)?;
        self.check_title(None, &new.title)?;
        if !new.kind.has_lifecycle() && !new.blocked_by.is_empty() {
            return Err(StoreError::DocBlocked {
                epic,
                blocked_by: new.blocked_by,
            });
        }
        // WAVE-10: a path is an address menger keeps for its own documents —
        // `_MENGER/…`, on a doc; where it lives and whether it is free are
        // checked under the lock, with the rest.
        if let Some(path) = &new.path {
            if !is_menger_path(path) {
                return Err(StoreError::PathNotReserved { path: path.clone() });
            }
            if new.kind != Kind::Doc {
                return Err(StoreError::PathOnWork {
                    path: path.clone(),
                    kind: new.kind,
                });
            }
        }
        // ORG-3: any short form, resolved under the lock.
        let requested_parent = new
            .parent
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        // COMP-3: what the body defines and references — read here, from the
        // body in hand, before the lock; a malformed ```component block is
        // refused now. Whether a name it defines is another ticket's is the
        // registry's to say, under the lock.
        let (uses, problems) = components::read(&new.body);
        if let Some(problem) = problems.into_iter().next() {
            return Err(StoreError::component(None, problem));
        }
        // INPUT-3: and the inputs it declares — refused now, naming the input,
        // when the markup is malformed or two share an id.
        let (declared, problems) = crate::inputs::read(&new.body);
        if let Some(problem) = problems.into_iter().next() {
            return Err(StoreError::InputMarkup { id: None, problem });
        }

        let serial = self.serial().await;
        // NOTE-2: the note a ticket is made from — open, read under the lock
        // so no other upgrade can take it first — or nothing happens.
        let note = match from {
            None => None,
            Some(id) => {
                let cache = self.tickets.read().expect("store lock poisoned");
                let n = cache.notes.get(id).cloned().ok_or_else(|| StoreError::NoteNotFound {
                    id,
                    newest: cache.notes.next_id().checked_sub(1).filter(|n| *n > 0),
                })?;
                if let Some(ticket) = n.ticket {
                    return Err(StoreError::NoteUpgraded { id, ticket });
                }
                Some(n)
            }
        };
        // Under the write-serial lock the cache mirrors the database
        // exactly, so the shortcode's highest seq + 1 is the next id —
        // monotonic across restarts (the cache was loaded from the
        // database) and race-free (writers are serialized). SCALE-3: that
        // highest seq is kept in the cache, beside the tickets and moved
        // by every birth, so allocating is a lookup rather than a walk of
        // every ticket in the store. This read phase computes everything
        // and mutates nothing: a rejection leaves no trace, in the
        // tickets or in what is kept about them.
        let (ticket, touched, unlocks_added, parent_epic, derived, registered, asked, under) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let tickets = &cache;
            // ORG-3: the parent by any short form — one naming several is
            // refused, naming each; one naming none is refused below, by the
            // id the ticket would have had.
            let parent_named = match requested_parent.as_deref() {
                None => None,
                Some(p) => match tickets.find_ticket(p, shortcodes::OrgReach::All) {
                    shortcodes::Found::One(key) => Some(Ok(key)),
                    shortcodes::Found::Many(candidates) => {
                        return Err(StoreError::Ambiguous { text: p.to_string(), candidates });
                    }
                    shortcodes::Found::None => Some(Err(p.to_ascii_uppercase())),
                },
            };
            let parent_row = parent_named.as_ref().and_then(|p| p.as_ref().ok()).and_then(|k| tickets.get(k));
            // ORG-3: the shortcode — the parent's own, the one of that code
            // in the parent's namespace, the one the address resolves to — or
            // one minted with this ticket (`minted`), its namespace checked.
            let (sc, minted) = match tickets.target(&asked, parent_row, new.kind.roots_a_shortcode())? {
                shortcodes::Target::In(sc) => (sc, None),
                shortcodes::Target::Mint(s) => (s.id.clone(), Some(s)),
            };
            // ORG-3: every blocker by any short form — one not born yet by the
            // key it will be born as; one naming several, refused.
            let blocked_by: Vec<TicketName> = new.blocked_by.iter().map(|b| tickets.name_ticket(b)).collect::<Result<_, _>>()?;
            // ACT-5: past every deleted number too — a number is never reused;
            // and the first of a shortcode is the one born into none held.
            let (seq, first) = (tickets.next_seq(&sc), tickets.ids.held(&sc) == 0);
            let id = sc.key(seq);

            // MNG-12: every shortcode starts with a scope. `seq == 1` is
            // exactly "this prefix has no tickets yet" (ids are never
            // reused), so the newborn would be the shortcode's root.
            // SDLC-11: or a doc — a doc collection's root.
            if first && !new.kind.roots_a_shortcode() {
                return Err(StoreError::FirstMustBeScope {
                    epic,
                    kind: new.kind,
                });
            }
            // ORG-5 (owner, 2026-10-13: a child scope roots its own
            // shortcode): a scope is born only as its shortcode's root —
            // under a parent, in a shortcode of its own hosted there.
            if new.kind == Kind::Scope {
                if let Some(root) = shortcode_root(tickets, &sc) {
                    return Err(rules::scope_not_root(parent_row.map(|p| p.id.as_str()), &epic, &root.id));
                }
            }

            // SDLC-11: a doc never completes, so nothing may wait on one —
            // neither by naming a doc that exists, nor by having named, before
            // it was born, the id a doc would now take.
            let docs = rules::docs_among(tickets, &blocked_by);
            if !docs.is_empty() {
                return Err(StoreError::BlockedByDoc { id, docs });
            }
            if !new.kind.has_lifecycle() {
                let by = tickets.ids.waiting_on(&id);
                if !by.is_empty() {
                    return Err(StoreError::DocAwaited { id, by });
                }
            }

            if let Some(path) = find_cycle(tickets, &id, &blocked_by) {
                return Err(StoreError::Cycle { id, path });
            }
            // ACT-5: nor on a deleted ticket — it is never born again.
            tickets.tombstones.refuse_blockers(&id, &blocked_by)?;
            // MNG-14 (owner, 2026-09-06): a parent named must exist; a
            // non-root's parent must be inside its own shortcode, and when
            // omitted it defaults to the shortcode's scope root. The root
            // itself (seq == 1) is the exception: its parent, if any, is
            // the shortcode it hangs under — a sub-shortcode — in any
            // prefix; omitted, it stays a top-level root.
            let parent = match (&parent_named, first) {
                (Some(Err(p)), _) => {
                    return Err(StoreError::ParentNotFound { id, parent: p.clone() });
                }
                (Some(Ok(p)), _) => {
                    let pt = tickets.get(p).expect("resolved above");
                    if !first && pt.shortcode != sc {
                        return Err(StoreError::ParentOutsideShortcode {
                            id,
                            parent: p.clone(),
                        });
                    }
                    if let Some(path) = find_parent_cycle(tickets, &id, p) {
                        return Err(StoreError::ParentCycle { id, path });
                    }
                    Some(p.clone())
                }
                (None, true) => None,
                (None, _) => shortcode_root(tickets, &sc).map(|r| r.id.clone()).map(String::from),
            };
            // ACCESS-11: the proofs name exactly where it is filed and what
            // it waits on — the ticket checked is the ticket written.
            let org = minted.as_ref().map(|m| m.ns.org.clone()).or_else(|| tickets.shortcodes.org(&sc).cloned());
            at.check(tickets, parent.as_deref(), org.as_ref(), &blocked_by)?;

            // WAVE-10: a `_MENGER/` document lives directly under its
            // shortcode's scope root — which it cannot be itself (`seq == 1`)
            // — and its path is one no ticket of the shortcode holds yet.
            if let Some(path) = &new.path {
                let Some(root) = (!first).then(|| shortcode_root(tickets, &sc)).flatten() else {
                    return Err(StoreError::PathNoRoot {
                        epic,
                        path: path.clone(),
                    });
                };
                if parent.as_deref() != Some(root.id.as_str()) {
                    return Err(StoreError::PathParent {
                        path: path.clone(),
                        parent: parent.unwrap_or_default(),
                        root: root.id.to_string(),
                    });
                }
                if let Some(holder) = tickets.paths.get(&(sc.clone(), path.clone())) {
                    return Err(StoreError::PathTaken {
                        epic,
                        path: path.clone(),
                        holder: holder.clone(),
                    });
                }
            }

            // COMP-3: what the body does to the registry — refused, nothing
            // stored and no id spent, when it defines a name another ticket
            // owns. Last of the checks: a body without components is refused
            // for nothing it was not refused for before.
            let registered = tickets.plan_registry(&id, &uses, None)?;
            // INPUT-3: the newborn's inputs, born open.
            let asked = tickets.inputs.plan(&id, declared);

            // Dangling forward references to this id resolve now: their
            // owners are the newborn's `unlocks` from birth. SCALE-3: the
            // cache keeps them by the id they name, already in (epic, seq)
            // order — only ids that do not exist yet, so it holds the few
            // forward references there are, never every edge.
            let unlocks = tickets.ids.waiting_on(&id);

            let now = chrono::Utc::now().timestamp();
            let mut ticket = Ticket {
                id: crate::address::TicketId::new(id.clone()),
                // ORG-3: its letters and address as the write leaves the
                // registry — set below, once the birth is worked out.
                epic: minted.as_ref().map_or_else(|| tickets.shortcodes.code(&sc), |m| m.code.clone()),
                shortcode: sc.clone(),
                address: String::new(),
                seq,
                title: new.title,
                // SDLC-11: `Doc` for a doc, which it never leaves.
                status: Status::at_birth(new.kind),
                kind: new.kind,
                severity: new.severity,
                blocked_by,
                unlocks,
                hidden_blocked_by: 0,
                hidden_unlocks: 0,
                parent_hidden: false,
                archived: None,
                parent: parent.clone().map(crate::address::TicketId::new),
                // SCALE-4: none, unless a hand-edited database already
                // names this id as some ticket's parent — the index has
                // kept those under the id, as `list { parent }` finds them.
                child_count: tickets.children.count(&id),
                // SDLC-9: linked after birth, by `link_commits`.
                commits: Vec::new(),
                // MENGER-12: labelled after birth, by `label`.
                labels: Vec::new(),
                // WAVE-10: checked above.
                path: new.path,
                // WAVE-6: placed below, with the derived state.
                placement: None,
                body: new.body,
                created_unix: now,
                updated_unix: now,
                moved_from: Vec::new(),
            };

            // Symmetry, in one place: every existing ticket this create
            // touches is cloned once and updated in place, so a ticket
            // that is blocker twice over is written once. Updated clones
            // only — the cache changes after the database commit.
            // SCALE-4: the parent is not one of them — its edge is the
            // newborn's `parent`, and the count is the cache's.
            let mut touched: HashMap<TicketName, Row> = HashMap::new();
            let mut unlocks_added: Vec<TicketName> = Vec::new();
            for blocker_id in &ticket.blocked_by {
                let mirrored = touched
                    .get(blocker_id)
                    .or_else(|| tickets.get(blocker_id))
                    .map(|b| b.unlocks.iter().any(|u| *u == id));
                // None: a dangling blocker — nothing exists to update.
                if mirrored != Some(false) {
                    continue;
                }
                let updated = touched
                    .entry(blocker_id.clone())
                    .or_insert_with(|| {
                        tickets.get(blocker_id).expect("blocker resolved above").clone()
                    });
                updated.unlocks.push(TicketName::new(id.clone()));
                updated.updated_unix = now;
                unlocks_added.push(blocker_id.clone());
            }
            // A parent that is also a blocker rides out whole in its
            // `UnlocksChanged`, after the `ChildrenChanged` that counts the
            // newborn: it carries the count it is about to have, or a
            // client applying the two in order would undo the first.
            if let Some(both) = parent.as_ref().and_then(|p| touched.get_mut(p.as_str())) {
                both.child_count += 1;
            }
            // The parent's shortcode, which `watch { epic }` files its
            // `ChildrenChanged` under — another one only for a scope root
            // hosted by another shortcode.
            let parent_epic = parent
                .as_deref()
                .map(|p| tickets.get(p).expect("parent checked above").shortcode.clone());
            // SCALE-28: and what the birth does to the derived state — its
            // shortcode's run, the forward references, its parent's count,
            // the roll-ups — worked out now, while nothing has changed.
            let newborn = Row::from(&ticket);
            // MENGER-12: an input born open makes the newborn `asking`. ORG-3:
            // a minted shortcode joins the registry with it.
            let derived = tickets.birth(&newborn, asked.list.as_deref().is_some_and(inputs::asking_in), minted.as_ref())?;
            {
                let registry = derived.moved.namespaces.as_ref().map_or(&tickets.shortcodes, |m| &m.after);
                ticket.epic = registry.code(&ticket.shortcode);
                ticket.address = registry.ticket_address(&ticket.shortcode, ticket.seq);
            }
            let newborn = Row::from(&ticket);
            // WAVE-6: born placed — `created` carries it.
            ticket.placement = derived.moved.placement().flatten();
            // FILTER-13: what an open `watch { filter }` asking `under` sees
            // of it — the newborn, and (a hand edit's) the subtree that named
            // its id as a parent, and any shortcode it re-hosts.
            let under = self.under_before(tickets, || {
                // FILTER-30: and each blocker whose `unlocks` it writes.
                let mut t = touched.values().fold(lineage::Touched::of_moves(derived.announced().as_deref()), |t, b| t.row(b));
                if tickets.children.kids(&newborn.id).is_some() {
                    t.subtree(tickets, &newborn.id, None);
                }
                if let Some(after) = &derived.moved.rollups {
                    t.hosts(tickets, after, tickets.rows().with(&newborn), Some(&newborn.shortcode));
                }
                t
            });
            (ticket, touched, unlocks_added, parent_epic, derived, registered, asked, under)
        };

        // MENGER-13: its first status, a transition from nothing — recorded
        // with the birth, appended with its row, said after its `Created`.
        let first = {
            let cache = self.tickets.read().expect("store lock poisoned");
            crate::types::Transition {
                id: cache.log.next_id(),
                ticket: ticket.id.clone(),
                field: crate::types::TransitionField::Status,
                from: None,
                to: ticket.status.as_str().to_string(),
                at_unix: ticket.created_unix,
                actor: act.actor,
                name: act.name,
                source: act.source,
                message: act.message,
                cause: None,
                cause_ticket: None,
            }
        };
        let (logged, appended, said) = (first.clone(), first.clone(), first);
        let born_key = OrderKey { shortcode: ticket.shortcode.clone(), seq: ticket.seq };

        // SCALE-14: the newborn is written whole — the one ticket a write
        // holds with its body, the caller's — and each blocker it touched
        // as its row, the body left as stored. The blockers ride out whole
        // in their `UnlocksChanged`, so their bodies are read back in the
        // same transaction. SCALE-28: and the derived state it moves, in
        // that transaction too.
        let born = ticket.clone();
        let newborn = Row::from(&ticket);
        let rows: Vec<Row> = touched.values().cloned().collect();
        // FILTER-4: the newborn's cell, said last of all.
        let announced = derived.announced();
        let Derived { stored, moved } = derived;
        // WAVE-6: the placements it moved, said last.
        let placed = moved.placed.as_ref().and_then(Change::placed);
        // COMP-3: and the registry it moved — stored in the transaction,
        // taken by the cache with the rows, said after everything else.
        let mut registered = registered;
        let registry_moves = std::mem::take(&mut registered.moves);
        let registry_rows = registry_moves.clone();
        // INPUT-3: and its inputs — stored, taken, said last of all.
        let mut asked = asked;
        let input_list = asked.list.take();
        let input_rows = input_list.clone();
        let born_id = ticket.id.clone();
        // NOTE-2: and the note it was made from, pointing at it.
        let upgraded = note.map(|n| crate::notes::Note {
            ticket: Some(ticket.id.to_string()),
            upgraded_at: Some(ticket.created_unix),
            ..n
        });
        let (note_row, note_kept) = (upgraded.clone(), upgraded.clone());
        let cache = Arc::clone(&self.tickets);
        self.write(
            serial,
            move |tx| {
                db::upsert(tx, &born)?;
                history::record(tx, &logged)?;
                if let Some(n) = &note_row {
                    notes::store(tx, n)?;
                }
                // SEARCH-2: and its row in the search index.
                search::put(tx, &born.id, &born.title, &born.body)?;
                if let Some(list) = &input_rows {
                    inputs::store(tx, &born.id, list)?;
                }
                let blockers = rows
                    .into_iter()
                    .map(|row| {
                        db::update_row(tx, &row)?;
                        let body = db::body(tx, &row.id)?.ok_or_else(|| missing(&row.id))?;
                        Ok((row.id.to_string(), whole(row, body)))
                    })
                    .collect::<Result<HashMap<String, Ticket>, StoreError>>()?;
                stored.write(tx)?;
                components::store(tx, &registry_rows)?;
                Ok(blockers)
            },
            move |cache, _| {
                let out = cache.created(newborn, touched.into_values(), moved);
                cache.log.append(&appended, &born_key);
                if let Some(n) = note_kept {
                    cache.notes.put(n);
                }
                cache.take_registry(registry_moves);
                if let Some(list) = input_list {
                    cache.inputs.take(&born_id, list);
                }
                out
            },
            move |blockers, (groups, child_count), tx| {
                let _ = tx.send(Change::Created {
                    ticket: ticket.clone(),
                });
                let _ = tx.send(Change::Transition { shortcode: ticket.shortcode.clone(), transition: said });
                if let (Some(parent_id), Some(epic), Some(child_count)) =
                    (&ticket.parent, parent_epic, child_count)
                {
                    let _ = tx.send(Change::ChildrenChanged {
                        id: parent_id.to_string(),
                        shortcode: epic,
                        added: ticket.id.to_string(),
                        child_count,
                    });
                }
                for blocker_id in unlocks_added {
                    let _ = tx.send(Change::UnlocksChanged {
                        ticket: blockers[blocker_id.as_str()].clone(),
                        added: TicketName::from(&ticket.id),
                    });
                }
                broadcast_groups(tx, groups);
                if let Some(placed) = placed {
                    let _ = tx.send(placed);
                }
                for change in Change::components(registered) {
                    let _ = tx.send(change);
                }
                for change in Change::inputs(&ticket.shortcode, asked) {
                    let _ = tx.send(change);
                }
                if let Some(moves) = announced {
                    let _ = tx.send(Change::CellsMoved { moves });
                }
                if let Some(change) = under.and_then(|u| u.after(&cache)) {
                    let _ = tx.send(change);
                }
                (ticket, upgraded)
            },
        )
        .await
    }
}
