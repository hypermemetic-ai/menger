//! PERF-2, FILTER-13: the synthetic seed's write — [`Store::seed_synthetic`]
//! and its shapes — moved out of `mod.rs` (MENGER-29's budget; ACT-5 made
//! the room). The generator itself is [`super::synthetic`].

use super::*;

impl Store {
    /// PERF-2: bulk-seeds `spec.count` synthetic tickets, through this
    /// layer and nothing else. The tickets are generated in memory first
    /// (ids allocated the way `create` allocates them — max existing seq
    /// per epic, plus one — every `blocked_by` edge pointing at an
    /// already-generated ticket so the DAG is acyclic by construction,
    /// and each mirrored in its blocker's `unlocks`), then written
    /// through to the database in a few large transactions (one fsync
    /// per ~50k rows, never one per row), and only then inserted into
    /// the cache. Deterministic given `spec.seed`.
    ///
    /// Refuses a store that already holds tickets unless
    /// `spec.into_existing` — nothing is generated or written then.
    /// Nothing is broadcast: seeding runs before the server serves, so
    /// there is no subscriber to tell, and a million `Created` events
    /// would only lag the channel. Blocking I/O: call from a blocking
    /// context (the builder wraps it in `spawn_blocking`).
    pub fn seed_synthetic(&self, spec: SeedSpec) -> Result<SeedReport, StoreError> {
        self.seed_synthetic_shaped(spec, SeedShape::Flat)
    }

    /// FILTER-13: [`Store::seed_synthetic`] in a given shape — PERF-2's flat
    /// one, or FILTER-3 §6.5's nested one ([`SeedShape`]).
    pub fn seed_synthetic_shaped(&self, spec: SeedSpec, shape: SeedShape) -> Result<SeedReport, StoreError> {
        // Mutations stay serialized even here: a concurrent writer means
        // this was called while serving, which it must not be.
        let _serial = self
            .write_serial
            .try_lock()
            .map_err(|_| StoreError::Db("seed requested while another write is in flight".into()))?;
        let started = std::time::Instant::now();

        let (next_seq, existing): (HashMap<String, u32>, usize) = {
            let cache = self.tickets.read().expect("store lock poisoned");
            if cache.len() > 0 && !spec.into_existing {
                return Err(StoreError::SeedRefused {
                    existing: cache.len(),
                });
            }
            // SCALE-3: the per-shortcode sequence `create` allocates from,
            // so seeded ids continue exactly where a create's would.
            let next_seq = cache
                .ids
                .last_seqs()
                .map(|(epic, seq)| (epic.as_str().to_string(), seq))
                .collect();
            (next_seq, cache.len())
        };

        let generated = synthetic::generate(spec, &next_seq, shape);
        let generate = started.elapsed();
        let epics = {
            let mut seen: HashSet<&str> = HashSet::new();
            generated.iter().for_each(|t| {
                seen.insert(t.epic.as_str());
            });
            seen.len()
        };
        let edges: usize = generated.iter().map(|t| t.blocked_by.len()).sum();

        let persist_started = std::time::Instant::now();
        // SCALE-14: one write as a reader counts them ([`Store::full`]),
        // however many transactions carry it. A seed that fails part way
        // leaves it counted: some of it may be stored, none of it is in the
        // cache, and a reader that sees the two counts disagree reads rows
        // from the database instead of trusting its own.
        //
        // SCALE-28: the derived state cannot ride in each chunk's
        // transaction — it is built once the whole seed is in the cache, as
        // below — so the first chunk's transaction also says the stored
        // state is behind the tickets, and the transaction that stores it
        // says it no longer is. Killed in between, the file says so, and
        // the next open builds it from the tickets (as for a file from
        // before SCALE-28): at no instant does it claim a state it does not
        // hold. The children index is part of that state, kept by SQLite a
        // row at a time in parent order — not the order a seed writes in:
        // at 10⁶ it took the persist from ~10 s to ~15 s. So a seed at least
        // the size of the store it lands in drops it with its first chunk
        // and builds it again, sorted, with the rest (0.95 s at 10⁶).
        // SCALE-32: the ready set's index too — and it is built after the
        // counts it is over are written, not moved by each one.
        let rebuild_index = generated.len() >= existing;
        // SCALE-32: each ticket's count of unsatisfied blockers, worked out
        // before its row is written so the row is written with it. Counted
        // after the seed is in, the counts were written a second time, a row
        // each — 177,000 rows at 10⁶, most pages of the file rewritten: the
        // seed went 14.6 s → 21 s. A generated ticket's blockers are tickets
        // generated before it; a store's own are looked up, should one be
        // named. What the seed does to the store's own counts (a newborn that
        // resolves a forward reference, born done) the stored state after it
        // still settles.
        let open: Vec<u32> = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let born: HashMap<&str, Status> = generated.iter().map(|t| (t.id.as_str(), t.status)).collect();
            generated
                .iter()
                .map(|t| {
                    ready::open_blockers(&t.blocked_by, |b| {
                        born.get(b).copied().or_else(|| cache.get(b).map(|r| r.status))
                    })
                })
                .collect()
        };
        self.committing.fetch_add(1, Ordering::SeqCst);
        {
            let mut conn = self.db.lock().expect("db lock poisoned");
            let chunks = generated
                .chunks(synthetic::ROWS_PER_TRANSACTION)
                .zip(open.chunks(synthetic::ROWS_PER_TRANSACTION));
            for (i, (chunk, open)) in chunks.enumerate() {
                let tx = conn.transaction()?;
                if i == 0 {
                    stored::mark_stale(&tx, true)?;
                    if rebuild_index {
                        stored::drop_children_index(&tx)?;
                        stored::drop_ready_index(&tx)?;
                    }
                }
                db::insert_many(&tx, chunk, open)?;
                // SEARCH-2: indexed in the transaction that writes them.
                search::put_many(&tx, chunk)?;
                tx.commit()?;
            }
        }
        let persist = persist_started.elapsed();

        let form = apply(&self.tickets, |tickets| {
            tickets.at.reserve(generated.len());
            // SCALE-14: into the cache as rows — each body is dropped as its
            // ticket goes in, as a load never reads one.
            for t in generated {
                tickets.insert(Row::from(t));
            }
            // SCALE-4: the children index, and every parent's count from
            // it, built over the seeded store as a build does — first, because
            // the roll-ups walk it. A seed only ever adds children, so a
            // count set before it is either set again here or still right.
            let children = children::Children::build(tickets.order.values());
            count_children(&mut tickets.order, &tickets.at, &children);
            tickets.children = children;
            // MNG-10: built once over the seeded store, as a build does — not
            // kept per row: a seed into an existing store can add tickets
            // to its prefixes in any shape, and nothing is broadcast here.
            let rollups = rollup::Rollups::build(tickets.rows());
            tickets.rollups = rollups;
            // SCALE-3: the id indexes too, for the same reason — the seed
            // went into the cache without passing through `create`, and
            // one of its tickets may be an id the store already named.
            let ids = ids::Ids::build(tickets.rows());
            tickets.ids = ids;
            // SCALE-32: and the counts, the ready set and the status counts —
            // a seed's tickets arrive with their statuses and blockers set.
            let ready = ready::Ready::build(tickets.rows());
            tickets.ready = ready;
            // FILTER-4: and the cells, from the rows and those counts.
            let cells =
                cells::Cells::build(tickets.rows(), |id| tickets.ready.count(id), |id| tickets.inputs.asking(id));
            tickets.cells = cells;
            // FILTER-13: and the kept tree, from the rows, the children and
            // the cells.
            Cache::plant(
                &tickets.order,
                &tickets.at,
                &mut tickets.children,
                &tickets.cells,
                &tickets.ready,
                &tickets.inputs,
            );
            // WAVE-6: and every placement, over the seeded store.
            tickets.place_all();
            // ORG-3: and every shortcode the seed made, registered with the
            // namespace its root gives it, every row given its address.
            let minted = tickets.adopt_shortcodes();
            (tickets.form(), minted, tickets.shortcodes.clone())
        });
        let (form, minted, registry) = form;
        // SCALE-28: and all of it stored, in one transaction, which also
        // says the stored state is the tickets' again.
        {
            let mut conn = self.db.lock().expect("db lock poisoned");
            let tx = conn.transaction()?;
            stored::create_children_index(&tx)?;
            stored::write_all(&tx, &form)?;
            stored::create_ready_index(&tx)?;
            stored::mark_stale(&tx, false)?;
            tx.execute_batch(shortcodes::TABLES)?;
            if tx.query_row("SELECT count(*) FROM orgs", [], |r| r.get::<_, i64>(0))? == 0 {
                tx.execute("INSERT INTO orgs (id, name) VALUES (1, ?1)", [crate::address::OrgId::HYPERMEMETIC])?;
            }
            for sc in &minted {
                shortcodes::put(&tx, &registry, sc)?;
            }
            tx.commit()?;
        }
        Ok(SeedReport {
            count: spec.count,
            epics,
            edges,
            generate,
            persist,
            elapsed: started.elapsed(),
        })
    }
}
