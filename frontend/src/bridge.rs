//! COMP-5: **the bridge** — the host's side of a frame's `menger.call`
//! (COMP-1 pin 9: read-only).
//!
//! A frame cannot reach the app's WebSocket (its origin is opaque); it asks
//! the host instead: `menger:call { id, method, params }` over postMessage
//! (src/frame.rs checks the sender and its channel). The host answers over
//! the app's own connection, with the typed client the rest of the app uses
//! — never a raw call: [`parse_call`] turns the method and its params into a
//! [`Call`], one of the read methods with the activation's own
//! parameter names, or refuses it; [`run`] makes it. The answer is the
//! stream's items — every data event, an in-band `error` included, as the
//! wire spells them — or, when the call never got an answer (no
//! connection, a refused request), the reason, which the SDK turns into a
//! rejected promise.
//!
//! Read-only by construction: a method outside [`READ_METHODS`] is refused
//! here, before anything is sent — `set_status`, `amend`, `create`, every
//! other write, `answer` and `clear_answer` included, and `watch` (a frame
//! hears the stream as change notices, not by subscribing).
//!
//! INPUT-4: **the first write.** A frame records an answer to one of its own
//! ticket's inputs — `menger.input.submit(id, value, rationale?)`, sent as
//! `menger:submit { id, params: { input, value, rationale? } }` — and never
//! through `menger.call`. The host makes `tickets.answer` for it, with
//! source `app`, only when every check here passes ([`admit`],
//! [`parse_submit`], [`check_submit`]): a viewer's gesture on that very frame
//! is under way (IDENTITY-36, src/gesture.rs — not merely the app's own
//! transient activation, which any click anywhere in the app turns on); the frame renders a ticket's stored body (the host
//! stamped which on the frame — the frame cannot name one); the input is
//! declared there, and neither closed nor removed; the value answers it
//! (`inputs::check`, what the server runs). A refusal is the frame's
//! rejected promise, and nothing is sent; what the server answers
//! ([`recorded`]) is the recorded answer, or its own in-band refusal.
//!
//! INPUT-9: **the second write.** `menger.input.clear(id)`, sent as
//! `menger:clear { id, params: { input } }`, clears the answer recorded on
//! one of the frame's own ticket's inputs: `tickets.clear_answer` with
//! source `app`, under the same checks as a submit but for the value
//! ([`admit`], [`parse_clear`], [`check_clear`]) — a gesture under way, a
//! frame that renders a ticket, the input declared there and neither
//! closed nor removed. The answer ([`cleared`]) is the input as the clear
//! left it, or the server's refusal. Every other write stays refused.
//!
//! IDENTITY-36: **a frame reads its own subtree, and nothing else.** The
//! bridge answers over the viewer's own socket, so without a bound every
//! body could read whatever its viewer can — an admin's `*` — and an editor
//! of one shortcode could write a body that reads every other. The host
//! stamps each frame with its scope (`data-scope`: the shortcode of the
//! ticket whose stored body it renders — the frame cannot name one), and
//! every read is answered as an editor of that shortcode would read it:
//! FILTER-13's `under SHORTCODE`, the shortcode and every shortcode hosted
//! under it ([`Reach`], asked of the server's own `counts`). [`scope_call`]
//! narrows the call — `under` ANDed into the filter of `list`, `find` and
//! `inputs`; `get` and `blockers` of a ticket outside answered `not_found`
//! unasked; `waves` of a shortcode outside refused — and [`admit_event`]
//! drops whatever comes back from outside the reach anyway (a blocker in
//! another shortcode, a group or extent of one), so no title, body, input
//! or answer from outside reaches the frame. `health`'s count is the
//! reach's. `components` stays whole: the registry is the frames' code,
//! handed to every frame already. A frame with no scope (a help card for
//! an orphaned component) reads nothing but `components`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use menger_tickets_api::filter::Filter;
use menger_tickets_api::{inputs, Answer, AnswerSource, Input, InputState, InputValue, TicketsEvent};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::session::Client;

/// What a frame may call (COMP-1 pin 9) — the activation's read methods;
/// INPUT-4: `inputs` among them.
pub(crate) const READ_METHODS: [&str; 10] =
    ["get", "list", "find", "waves", "groups", "extents", "blockers", "health", "components", "inputs"];

/// INPUT-4: the SDK call a frame's answer is made by — named in every
/// refusal, so a component author finds it.
pub(crate) const SUBMIT: &str = "menger.input.submit";

/// INPUT-9: the SDK call a frame's clear is made by — its second write.
pub(crate) const CLEAR: &str = "menger.input.clear";

/// One read call, with the activation's parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Get { id: String },
    List {
        epic: Option<String>,
        status: Option<String>,
        parent: Option<String>,
        after: Option<String>,
        limit: Option<u32>,
        summary: Option<bool>,
        /// IDENTITY-36: the frame's own filter, if any — its scope is ANDed in.
        filter: Option<String>,
    },
    Find { query: String, after: Option<String>, limit: Option<u32>, filter: Option<String> },
    Waves { epics: Vec<String>, kinds: Option<Vec<String>> },
    Groups,
    Extents,
    Blockers { id: String },
    Health,
    Components,
    /// INPUT-4.
    Inputs { ticket: Option<String>, epic: Option<String>, state: Option<String>, filter: Option<String> },
}

impl Call {
    /// The activation's name for it.
    pub(crate) fn method(&self) -> &'static str {
        match self {
            Call::Get { .. } => "get",
            Call::List { .. } => "list",
            Call::Find { .. } => "find",
            Call::Waves { .. } => "waves",
            Call::Groups => "groups",
            Call::Extents => "extents",
            Call::Blockers { .. } => "blockers",
            Call::Health => "health",
            Call::Components => "components",
            Call::Inputs { .. } => "inputs",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdParams {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    epic: Option<String>,
    status: Option<String>,
    parent: Option<String>,
    after: Option<String>,
    limit: Option<u32>,
    summary: Option<bool>,
    filter: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindParams {
    query: String,
    after: Option<String>,
    limit: Option<u32>,
    filter: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WavesParams {
    epics: Vec<String>,
    kinds: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputsParams {
    ticket: Option<String>,
    epic: Option<String>,
    state: Option<String>,
    filter: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

/// The refusal for anything but a read — which names the two writes a
/// frame may make, and how (INPUT-4, INPUT-9).
pub(crate) fn read_only(method: &str) -> String {
    format!(
        "read-only: a frame may call only {} — not `{method}`; its only writes are `answer` and `clear_answer`, \
         made by {SUBMIT} and {CLEAR} on a viewer's click",
        READ_METHODS.join(", ")
    )
}

/// `method` with `params` (JSON — the frame's object as `JSON.stringify`
/// wrote it; `null` or nothing for none) as a [`Call`], or why not: a
/// method that is not a read ([`read_only`]), or params the method does not
/// take — an unknown name, a missing one, a wrong type, each named.
pub(crate) fn parse_call(method: &str, params: &str) -> Result<Call, String> {
    if !READ_METHODS.contains(&method) {
        return Err(read_only(method));
    }
    let params = match params.trim() {
        "" | "null" => "{}",
        p => p,
    };
    // The activation's params are named: an object, never a list or a value.
    if !params.starts_with('{') {
        return Err(format!("{method}: params must be an object, e.g. {{ id: \"MNG-2\" }}"));
    }
    fn take<T: DeserializeOwned>(method: &str, params: &str) -> Result<T, String> {
        serde_json::from_str(params).map_err(|e| format!("{method}: {e}"))
    }
    Ok(match method {
        "get" => Call::Get { id: take::<IdParams>(method, params)?.id },
        "list" => {
            let p: ListParams = take(method, params)?;
            Call::List {
                epic: p.epic,
                status: p.status,
                parent: p.parent,
                after: p.after,
                limit: p.limit,
                summary: p.summary,
                filter: p.filter,
            }
        }
        "find" => {
            let p: FindParams = take(method, params)?;
            Call::Find { query: p.query, after: p.after, limit: p.limit, filter: p.filter }
        }
        "waves" => {
            let p: WavesParams = take(method, params)?;
            Call::Waves { epics: p.epics, kinds: p.kinds }
        }
        "blockers" => Call::Blockers { id: take::<IdParams>(method, params)?.id },
        "inputs" => {
            let p: InputsParams = take(method, params)?;
            Call::Inputs { ticket: p.ticket, epic: p.epic, state: p.state, filter: p.filter }
        }
        other => {
            take::<NoParams>(other, params)?;
            match other {
                "groups" => Call::Groups,
                "extents" => Call::Extents,
                "health" => Call::Health,
                _ => Call::Components,
            }
        }
    })
}

/// Make the call over the app's connection: the stream's items, or why
/// there are none. Unscoped — a frame's call is [`run_scoped`]'s.
async fn run(client: &Client, call: Call) -> Result<Vec<TicketsEvent>, String> {
    let answer = match call {
        Call::Get { id } => client.get_all(id).await,
        Call::List { epic, status, parent, after, limit, summary, filter } => {
            client.list_all(epic, status, parent, after, limit, summary, filter, None, None).await
        }
        Call::Find { query, after, limit, filter } => client.find_all(query, after, limit, filter).await,
        Call::Waves { epics, kinds } => client.waves_all(epics, kinds).await,
        Call::Groups => client.groups_all().await,
        Call::Extents => client.extents_all().await,
        Call::Blockers { id } => client.blockers_all(id).await,
        Call::Health => client.health_all().await,
        Call::Components => client.components_all().await,
        Call::Inputs { ticket, epic, state, filter } => client.inputs_all(ticket, epic, state, filter).await,
    };
    answer.map_err(|e| crate::session::describe(&e))
}

// -- IDENTITY-36: a frame reads its own subtree ------------------------------

/// The shortcode of a ticket id (`EPIC-N`) or a path address
/// (`EPIC/_MENGER/…`) — the server's own reading (access.rs), uppercased.
pub(crate) fn shortcode_of(id: &str) -> String {
    id.trim().split(['-', '/']).next().unwrap_or_default().to_ascii_uppercase()
}

/// A shortcode the filter language reads as one word.
fn shortcode_ok(epic: &str) -> bool {
    !epic.is_empty() && epic.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The scope a frame that renders `ticket`'s stored body reads under: that
/// ticket's shortcode — `None` for no ticket, or one that names none.
pub(crate) fn scope_of(ticket: Option<&str>) -> Option<String> {
    ticket.map(shortcode_of).filter(|e| shortcode_ok(e))
}

/// The filter a frame scoped to `epic` reads under: `under EPIC`, ANDed
/// with the frame's own (`theirs`, when it says anything) — or why not: a
/// filter the language does not take, named.
pub(crate) fn scoped_filter(theirs: Option<&str>, epic: &str) -> Result<String, String> {
    if !shortcode_ok(epic) {
        return Err(format!("this frame's scope '{epic}' is no shortcode"));
    }
    let reach = Filter::parse(&format!("under {epic}")).map_err(|e| e.to_string())?;
    match theirs.map(str::trim).filter(|t| !t.is_empty()) {
        None => Ok(reach.format()),
        Some(t) => Ok(Filter::parse(t).map_err(|e| format!("filter: {e}"))?.and(&reach).format()),
    }
}

/// How far a frame scoped to `epic` reads: the shortcodes `under EPIC` —
/// itself and every shortcode hosted under it — as the server's `counts`
/// answers that filter for this viewer, and how many tickets they hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reach {
    pub(crate) epic: String,
    pub(crate) epics: HashSet<String>,
    pub(crate) tickets: u64,
}

impl Reach {
    /// What is known before the server says: the shortcode alone (never
    /// wider than its answer).
    pub(crate) fn only(epic: &str) -> Self {
        let epic = epic.to_ascii_uppercase();
        Self { epics: HashSet::from([epic.clone()]), epic, tickets: 0 }
    }

    /// No shortcode at all — a frame that renders no ticket's body: it
    /// hears components, and nothing of any ticket.
    pub(crate) fn none() -> Self {
        Self { epic: String::new(), epics: HashSet::new(), tickets: 0 }
    }

    /// From `counts { filter: "under EPIC" }`'s answer — or the server's
    /// refusal.
    pub(crate) fn from_counts(epic: &str, events: Vec<TicketsEvent>) -> Result<Self, String> {
        let mut reach = Self::only(epic);
        for ev in events {
            match ev {
                TicketsEvent::Count { epic, held } if held > 0 => {
                    reach.epics.insert(epic.to_ascii_uppercase());
                    reach.tickets += held;
                }
                TicketsEvent::Error { message } => return Err(message),
                _ => {}
            }
        }
        Ok(reach)
    }

    /// Whether the shortcode `epic` is inside.
    pub(crate) fn holds(&self, epic: &str) -> bool {
        self.epics.contains(&epic.to_ascii_uppercase())
    }

    /// Whether the ticket `id` (or path address) is inside.
    pub(crate) fn holds_id(&self, id: &str) -> bool {
        self.holds(&shortcode_of(id))
    }
}

thread_local! {
    /// The reach last asked for each scope — what a `watch` event is held
    /// to before it reaches a frame (src/frame.rs).
    static REACHES: RefCell<HashMap<String, Reach>> = RefCell::new(HashMap::new());
}

/// The reach known for `epic` — its shortcode alone until the server has
/// said more.
pub(crate) fn known_reach(epic: &str) -> Reach {
    REACHES.with(|r| r.borrow().get(&epic.to_ascii_uppercase()).cloned()).unwrap_or_else(|| Reach::only(epic))
}

/// Whether the server has said how far `epic` reaches.
pub(crate) fn reach_known(epic: &str) -> bool {
    REACHES.with(|r| r.borrow().contains_key(&epic.to_ascii_uppercase()))
}

/// A `group_changed` heard: a shortcode that now hangs outside a reach it
/// was in leaves it at once (narrowing needs no one's word); every reach it
/// may have moved — out of, or newly under — is returned, to be asked
/// again ([`reach_of`]: widening waits for the server).
pub(crate) fn note_group(group: &menger_tickets_api::Group) -> Vec<String> {
    REACHES.with(|r| moved(&mut r.borrow_mut(), group))
}

/// [`note_group`] on `reaches`.
pub(crate) fn moved(reaches: &mut HashMap<String, Reach>, group: &menger_tickets_api::Group) -> Vec<String> {
    let epic = group.epic.to_ascii_uppercase();
    let mut ask = Vec::new();
    for (scope, reach) in reaches.iter_mut() {
        if *scope == epic {
            continue;
        }
        let host_inside = group.host.as_deref().is_some_and(|h| reach.holds_id(h));
        match (reach.holds(&epic), host_inside) {
            // Out from under: gone now; what it hosts goes with the answer.
            (true, false) => {
                reach.epics.remove(&epic);
                ask.push(scope.clone());
            }
            // Newly under: in once the server says so.
            (false, true) => ask.push(scope.clone()),
            _ => {}
        }
    }
    ask.sort();
    ask
}

/// Ask the server how far `epic` reaches for this viewer, and remember it.
pub(crate) async fn reach_of(client: &Client, epic: &str) -> Result<Reach, String> {
    let filter = scoped_filter(None, epic)?;
    let events = client.counts_all(Some(filter), None).await.map_err(|e| crate::session::describe(&e))?;
    let reach = Reach::from_counts(epic, events)?;
    REACHES.with(|r| r.borrow_mut().insert(reach.epic.clone(), reach.clone()));
    Ok(reach)
}

/// A scoped call: one to make, or the answer already known (a ticket
/// outside the reach is `not_found`, as a ticket that is none).
#[derive(Clone, Debug)]
pub(crate) enum Scoped {
    Call(Call),
    Answer(Vec<TicketsEvent>),
}

/// `call`, as a frame scoped to `reach` may make it: `list`, `find` and
/// `inputs` under `under EPIC` (ANDed with their own filter); `get` and
/// `blockers` of a ticket outside answered `not_found` without asking;
/// `waves` of a shortcode outside refused, naming it; the rest as asked —
/// every answer is then held to the reach ([`admit_event`]). `reach: None`
/// (a frame that renders no ticket's body): `components` only.
pub(crate) fn scope_call(call: Call, reach: Option<&Reach>) -> Result<Scoped, String> {
    let Some(reach) = reach else {
        return match call {
            Call::Components => Ok(Scoped::Call(call)),
            other => Err(format!(
                "{}: this frame renders no ticket's body — it reads nothing but components",
                other.method()
            )),
        };
    };
    let epic = reach.epic.as_str();
    Ok(match call {
        Call::Get { id } | Call::Blockers { id } if !reach.holds_id(&id) => Scoped::Answer(vec![TicketsEvent::NotFound { id }]),
        Call::List { epic: e, status, parent, after, limit, summary, filter } => Scoped::Call(Call::List {
            epic: e,
            status,
            parent,
            after,
            limit,
            summary,
            filter: Some(scoped_filter(filter.as_deref(), epic)?),
        }),
        Call::Find { query, after, limit, filter } => {
            Scoped::Call(Call::Find { query, after, limit, filter: Some(scoped_filter(filter.as_deref(), epic)?) })
        }
        Call::Inputs { ticket, epic: e, state, filter } => {
            Scoped::Call(Call::Inputs { ticket, epic: e, state, filter: Some(scoped_filter(filter.as_deref(), epic)?) })
        }
        Call::Waves { epics, kinds } => {
            if let Some(outside) = epics.iter().find(|e| !reach.holds(e)) {
                return Err(format!(
                    "waves: {outside} is outside this frame's reach — a ticket's body reads its own shortcode, {epic}, \
                     and the shortcodes hosted under it"
                ));
            }
            Scoped::Call(Call::Waves { epics, kinds })
        }
        other => Scoped::Call(other),
    })
}

/// Whether — and as what — an event reaches a frame scoped to `reach`: a
/// ticket, row, input, group or count of a shortcode inside it, as it is
/// (a group hosted under a ticket outside stands at the top, as the server
/// draws it for a viewer without its host); a placement change with only
/// its moves inside; `health` with the reach's count; components, pages,
/// lags and refusals whole; anything of a shortcode outside — or an
/// aggregate over the viewer's whole store — not at all. Every call's
/// answer, and every `watch` event a frame hears, passes through here.
pub(crate) fn admit_event(ev: TicketsEvent, reach: &Reach) -> Option<TicketsEvent> {
    use TicketsEvent as E;
    let inside = |epic: &str| reach.holds(epic);
    let top = |mut group: menger_tickets_api::Group| {
        if group.host.as_deref().is_some_and(|h| !reach.holds_id(h)) {
            group.host = None;
        }
        group
    };
    match ev {
        E::Created { ref ticket }
        | E::Ticket { ref ticket }
        | E::StatusChanged { ref ticket, .. }
        | E::KindChanged { ref ticket, .. }
        | E::Amended { ref ticket, .. }
        | E::CommitsLinked { ref ticket, .. }
        | E::CommitsUnlinked { ref ticket, .. }
        | E::LabelsChanged { ref ticket, .. }
        | E::UnlocksChanged { ref ticket, .. }
        | E::ParentChanged { ref ticket, .. }
        | E::Blocker { ref ticket } => inside(&ticket.epic).then_some(ev),
        E::Summary { ref summary } | E::Placed { ref summary, .. } | E::Node { ref summary, .. } | E::Hit { ref summary, .. } => {
            inside(&summary.epic).then_some(ev)
        }
        E::ChildrenChanged { ref id, .. } | E::ChildRemoved { ref id, .. } | E::Member { ref id, .. } => {
            reach.holds_id(id).then_some(ev)
        }
        E::Input { ref input } | E::InputChanged { ref input } => reach.holds_id(&input.ticket).then_some(ev),
        E::Group { group } => inside(&group.epic).then(|| E::Group { group: top(group) }),
        E::GroupChanged { group } => inside(&group.epic).then(|| E::GroupChanged { group: top(group) }),
        E::Extent { ref epic, .. }
        | E::Count { ref epic, .. }
        | E::Entered { ref epic, .. }
        | E::Left { ref epic, .. }
        | E::Recount { ref epic, .. } => inside(epic).then_some(ev),
        E::PlacementsChanged { changes } => {
            let changes: Vec<_> = changes.into_iter().filter(|c| reach.holds_id(&c.id)).collect();
            (!changes.is_empty()).then_some(E::PlacementsChanged { changes })
        }
        // The reach's own count, never the viewer's whole store.
        E::Health { version, unix_now, .. } => Some(E::Health { version, unix_now, ticket_count: reach.tickets }),
        // A dangling blocker's id names no ticket: nothing to read.
        E::DanglingBlocker { .. }
        | E::Page { .. }
        | E::WaveCounts { .. }
        | E::Lagged { .. }
        | E::NotFound { .. }
        | E::Error { .. }
        | E::BodyTooLarge { .. }
        | E::Component { .. }
        | E::ComponentChanged { .. }
        | E::ComponentRemoved { .. } => Some(ev),
        // Aggregates over the viewer's whole store: no frame's.
        E::Facet { .. } | E::Searched { .. } => None,
    }
}

/// A call's answer held to `reach` ([`admit_event`]); a `get` (`get`: its
/// id) whose ticket did not pass is `not_found`, as a ticket that is none.
pub(crate) fn held_to(events: Vec<TicketsEvent>, reach: &Reach, get: Option<String>) -> Vec<TicketsEvent> {
    let mut held: Vec<TicketsEvent> = events.into_iter().filter_map(|ev| admit_event(ev, reach)).collect();
    if let Some(id) = get {
        if held.is_empty() {
            held.push(TicketsEvent::NotFound { id });
        }
    }
    held
}

/// A frame's read: `call` held to the reach of `scope` (the shortcode the
/// host stamped on the frame; `None`: it renders no ticket's body) — the
/// reach asked of the server ([`reach_of`]), the call narrowed
/// ([`scope_call`]) and made, its answer held to the reach ([`held_to`]).
pub(crate) async fn run_scoped(client: &Client, call: Call, scope: Option<&str>) -> Result<Vec<TicketsEvent>, String> {
    let reach = match scope {
        Some(epic) => Some(reach_of(client, epic).await.map_err(|e| format!("{}: {e}", call.method()))?),
        None => None,
    };
    let get = match &call {
        Call::Get { id } => Some(id.clone()),
        _ => None,
    };
    let events = match scope_call(call, reach.as_ref())? {
        Scoped::Answer(events) => return Ok(events),
        Scoped::Call(call) => run(client, call).await?,
    };
    Ok(match &reach {
        Some(reach) => held_to(events, reach, get),
        None => events,
    })
}

// -- INPUT-4: the first write -----------------------------------------------

/// `menger.input.submit(id, value, rationale?)` as the host reads it. No
/// ticket: a frame answers only the ticket whose body it renders, which the
/// host knows and the frame cannot name.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Submit {
    /// The input's id in the ticket's body.
    pub(crate) input: String,
    pub(crate) value: InputValue,
    #[serde(default)]
    pub(crate) rationale: Option<String>,
}

/// Whether a write — `sdk`, [`SUBMIT`] or (INPUT-9) [`CLEAR`], which every
/// refusal names — may go on at all, before its params are read: a viewer's
/// gesture on THIS frame is under way — `gesture`, the host's own reading
/// (IDENTITY-36, src/gesture.rs: a click or key in this very frame, not
/// anywhere in the app; a browser that cannot say is no gesture) — and the
/// frame renders a ticket's stored body (`ticket`, the one the host stamped
/// on the frame; none for an editor's preview or a help card). The ticket,
/// or why not.
pub(crate) fn admit(sdk: &str, ticket: Option<&str>, gesture: bool) -> Result<String, String> {
    if !gesture {
        return Err(format!("{sdk}: only inside a viewer's click — the app saw no click or key in this frame just now (nothing sent)"));
    }
    match ticket.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => Ok(t.to_string()),
        None => Err(format!(
            "{sdk}: this frame shows no ticket's stored body (an editor's preview, a help card) — it has no input to answer (nothing sent)"
        )),
    }
}

/// COMP-14: the refusal of every write from a cited window's frame — its
/// inputs are shown, and answered only in their own ticket. The host sends
/// nothing (the frame carries no `data-ticket` either).
pub(crate) fn display_only(sdk: &str) -> String {
    format!("{sdk}: display only — this is a cited window; its inputs are answered in their own ticket (nothing sent)")
}

/// The frame's params (JSON, as for a call) as a [`Submit`], or why not —
/// an unknown field (`ticket` included), a missing one, a value that is not
/// an [`InputValue`], each named.
pub(crate) fn parse_submit(params: &str) -> Result<Submit, String> {
    serde_json::from_str(params.trim()).map_err(|e| {
        format!(
            "{SUBMIT}: {e} — it takes (id, value, rationale?), a value {{type: \"choice\", option}} | \
             {{type: \"multi\", options}} | {{type: \"text\", text}} (nothing sent)"
        )
    })
}

/// An `inputs { ticket }` answer, read for `sdk`'s write: the ticket's
/// inputs, or what the server said instead.
pub(crate) fn inputs_of(sdk: &str, events: Vec<TicketsEvent>) -> Result<Vec<Input>, String> {
    let mut held = Vec::new();
    for ev in events {
        match ev {
            TicketsEvent::Input { input } => held.push(input),
            TicketsEvent::NotFound { id } => return Err(format!("{sdk}: {id} is not a ticket (nothing sent)")),
            TicketsEvent::Error { message } => return Err(format!("{sdk}: {message} (nothing sent)")),
            _ => {}
        }
    }
    Ok(held)
}

/// The checks both writes share, against `held` — the inputs the store
/// holds for `ticket`, the frame's: the input `id` is declared in its body
/// (one another ticket declares is not this frame's to write), it is not
/// removed from it, and not closed. The input, or why not, in the server's
/// words, naming `sdk`.
fn open_input<'a>(sdk: &str, ticket: &str, id: &str, held: &'a [Input]) -> Result<&'a Input, String> {
    let mine: Vec<&Input> = held.iter().filter(|i| i.ticket == ticket).collect();
    let Some(input) = mine.iter().find(|i| i.id == id) else {
        let declared: Vec<String> =
            mine.iter().filter(|i| i.state != InputState::Removed).map(|i| format!("'{}'", i.id)).collect();
        return Err(if declared.is_empty() {
            format!("{sdk}: {ticket} declares no input '{id}' — its body holds none (nothing sent)")
        } else {
            format!("{sdk}: {ticket} declares no input '{id}' — its inputs are {} (nothing sent)", declared.join(", "))
        });
    };
    match input.state {
        InputState::Removed => Err(format!("{sdk}: {ticket}'s '{id}' was removed from its body (nothing sent)")),
        InputState::Closed => {
            Err(format!("{sdk}: {ticket}'s '{id}' is closed: its answer is frozen until it is reopened (nothing sent)"))
        }
        InputState::Open | InputState::Answered => Ok(input),
    }
}

/// The host's own checks for a submit ([`open_input`]), and `submit.value`
/// answers the input — the value to send (a `multi`'s options in the
/// declared order), or why not, in the server's words.
pub(crate) fn check_submit(ticket: &str, submit: &Submit, held: &[Input]) -> Result<InputValue, String> {
    let id = submit.input.as_str();
    let input = open_input(SUBMIT, ticket, id, held)?;
    inputs::check(input.kind, &input.options, submit.value.clone())
        .map_err(|why| format!("{SUBMIT}: not an answer to {ticket}'s '{id}': {why} (nothing sent)"))
}

/// `tickets.answer`'s reply: the answer it recorded, or its refusal.
pub(crate) fn recorded(events: Vec<TicketsEvent>) -> Result<Answer, String> {
    for ev in events {
        match ev {
            TicketsEvent::InputChanged { input } | TicketsEvent::Input { input } => {
                return input.answer.ok_or_else(|| format!("{SUBMIT}: {}'s '{}' holds no answer", input.ticket, input.id))
            }
            TicketsEvent::NotFound { id } => return Err(format!("{SUBMIT}: {id} is not a ticket")),
            TicketsEvent::Error { message } => return Err(format!("{SUBMIT}: {message}")),
            _ => {}
        }
    }
    Err(format!("{SUBMIT}: the server recorded no answer"))
}

/// Answer `submit` against `ticket`'s inputs over the app's connection: the
/// store's inputs for it, the checks ([`check_submit`]), then
/// `tickets.answer` with source `app` — the recorded answer, or why not.
pub(crate) async fn submit(client: &Client, ticket: String, submit: Submit) -> Result<Answer, String> {
    let described = |e: plexus_client::ClientError| format!("{SUBMIT}: {}", crate::session::describe(&e));
    let held = inputs_of(SUBMIT, client.inputs_all(Some(ticket.clone()), None, None, None).await.map_err(described)?)?;
    let value = check_submit(&ticket, &submit, &held)?;
    let source = Some(AnswerSource::App.as_str().to_string());
    recorded(client.answer_all(ticket, submit.input, value, submit.rationale, source).await.map_err(described)?)
}

// -- INPUT-9: the second write ----------------------------------------------

/// `menger.input.clear(id)` as the host reads it — the input only; no
/// ticket, as for a submit.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Clear {
    /// The input's id in the ticket's body.
    pub(crate) input: String,
}

/// The frame's params (JSON) as a [`Clear`], or why not — an unknown field
/// (`ticket` included), a missing or mistyped `input`, each named.
pub(crate) fn parse_clear(params: &str) -> Result<Clear, String> {
    serde_json::from_str(params.trim()).map_err(|e| format!("{CLEAR}: {e} — it takes (id) (nothing sent)"))
}

/// The host's own checks for a clear: [`open_input`] — declared in the
/// frame's ticket, neither removed nor closed. There is no value to check.
pub(crate) fn check_clear(ticket: &str, clear: &Clear, held: &[Input]) -> Result<(), String> {
    open_input(CLEAR, ticket, &clear.input, held).map(|_| ())
}

/// `tickets.clear_answer`'s reply: the input as the clear left it (`open`,
/// the clear in its history; as it was when it held no answer), or the
/// server's refusal.
pub(crate) fn cleared(events: Vec<TicketsEvent>) -> Result<Input, String> {
    for ev in events {
        match ev {
            TicketsEvent::InputChanged { input } | TicketsEvent::Input { input } => return Ok(input),
            TicketsEvent::NotFound { id } => return Err(format!("{CLEAR}: {id} is not a ticket")),
            TicketsEvent::Error { message } => return Err(format!("{CLEAR}: {message}")),
            _ => {}
        }
    }
    Err(format!("{CLEAR}: the server cleared nothing"))
}

/// Clear `clear` against `ticket`'s inputs over the app's connection: the
/// store's inputs for it, the checks ([`check_clear`]), then
/// `tickets.clear_answer` with source `app` — the input, or why not.
pub(crate) async fn clear(client: &Client, ticket: String, clear: Clear) -> Result<Input, String> {
    let described = |e: plexus_client::ClientError| format!("{CLEAR}: {}", crate::session::describe(&e));
    let held = inputs_of(CLEAR, client.inputs_all(Some(ticket.clone()), None, None, None).await.map_err(described)?)?;
    check_clear(&ticket, &clear, &held)?;
    let source = Some(AnswerSource::App.as_str().to_string());
    cleared(client.clear_answer_all(ticket, clear.input, source).await.map_err(described)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// COMP-1 pin 9: the reads — INPUT-4: ten, `inputs` among them — each
    /// with the activation's parameter names, and nothing else.
    #[test]
    fn a_frame_may_make_the_ten_reads() {
        assert_eq!(parse_call("get", r#"{"id":"MNG-2"}"#), Ok(Call::Get { id: "MNG-2".into() }));
        assert_eq!(
            parse_call("list", r#"{"epic":"MNG","limit":5,"summary":true}"#),
            Ok(Call::List { epic: Some("MNG".into()), status: None, parent: None, after: None, limit: Some(5), summary: Some(true), filter: None })
        );
        assert_eq!(
            parse_call("list", "null"),
            Ok(Call::List { epic: None, status: None, parent: None, after: None, limit: None, summary: None, filter: None })
        );
        assert_eq!(
            parse_call("find", r#"{"query":"wave"}"#),
            Ok(Call::Find { query: "wave".into(), after: None, limit: None, filter: None })
        );
        assert_eq!(
            parse_call("waves", r#"{"epics":["PLEXUS"]}"#),
            Ok(Call::Waves { epics: vec!["PLEXUS".into()], kinds: None })
        );
        assert_eq!(
            parse_call("waves", r#"{"epics":["A","B"],"kinds":["build"]}"#),
            Ok(Call::Waves { epics: vec!["A".into(), "B".into()], kinds: Some(vec!["build".into()]) })
        );
        assert_eq!(parse_call("blockers", r#"{"id":"MNG-4"}"#), Ok(Call::Blockers { id: "MNG-4".into() }));
        assert_eq!(parse_call("groups", "{}"), Ok(Call::Groups));
        assert_eq!(parse_call("extents", ""), Ok(Call::Extents));
        assert_eq!(parse_call("health", "null"), Ok(Call::Health));
        assert_eq!(parse_call("components", "{}"), Ok(Call::Components));
        assert_eq!(parse_call("inputs", "null"), Ok(Call::Inputs { ticket: None, epic: None, state: None, filter: None }));
        assert_eq!(
            parse_call("inputs", r#"{"ticket":"INPUT-9","epic":"INPUT","state":"open"}"#),
            Ok(Call::Inputs { ticket: Some("INPUT-9".into()), epic: Some("INPUT".into()), state: Some("open".into()), filter: None })
        );
        for m in READ_METHODS {
            let params = match m {
                "get" | "blockers" => r#"{"id":"X-1"}"#,
                "find" => r#"{"query":""}"#,
                "waves" => r#"{"epics":["X"]}"#,
                _ => "{}",
            };
            assert!(parse_call(m, params).is_ok(), "{m}");
        }
    }

    /// Every write — `answer` and the other input writes included: a frame
    /// answers only through `menger.input.submit`, and (INPUT-9) clears
    /// only through `menger.input.clear` — and `watch`, `ready_set`, a name
    /// that is no method, is refused before anything is sent, saying
    /// "read-only", what is allowed, and the two writes and how they are
    /// made.
    #[test]
    fn anything_else_is_read_only() {
        for m in [
            "set_status", "set_kind", "amend", "create", "set_parent", "link_commits", "unlink_commits", "watch", "ready_set", "answer",
            "clear_answer", "close_input", "reopen_input", "", "GET", "menger.tickets.get", "submit", "menger.input.submit",
            "clear", "menger.input.clear",
        ] {
            let err = parse_call(m, "{}").unwrap_err();
            assert!(err.starts_with("read-only: "), "{m}: {err}");
            assert!(err.contains("get, list, find, waves, groups, extents, blockers, health, components, inputs"), "{err}");
            assert!(
                err.contains(
                    "its only writes are `answer` and `clear_answer`, made by menger.input.submit and menger.input.clear \
                     on a viewer's click"
                ),
                "{err}"
            );
        }
        assert_eq!(
            parse_call("set_status", r#"{"id":"A-1","status":"Complete"}"#),
            Err("read-only: a frame may call only get, list, find, waves, groups, extents, blockers, health, components, inputs \
                 — not `set_status`; its only writes are `answer` and `clear_answer`, made by menger.input.submit and \
                 menger.input.clear on a viewer's click"
                .into())
        );
        let err = parse_call("answer", r#"{"ticket":"A-1","input":"d1","value":{"type":"choice","option":"a"}}"#).unwrap_err();
        assert!(err.starts_with("read-only: ") && err.contains("not `answer`"), "{err}");
        // INPUT-9: `clear_answer` too — a frame clears through menger.input.clear only.
        let err = parse_call("clear_answer", r#"{"ticket":"A-1","input":"d1"}"#).unwrap_err();
        assert!(err.starts_with("read-only: ") && err.contains("not `clear_answer`"), "{err}");
    }

    /// Params the method does not take are named, never dropped silently.
    #[test]
    fn wrong_params_are_named() {
        let err = parse_call("get", "{}").unwrap_err();
        assert!(err.starts_with("get: ") && err.contains("missing field `id`"), "{err}");
        let err = parse_call("waves", r#"{"epic":"PLEXUS"}"#).unwrap_err();
        assert!(err.contains("unknown field `epic`"), "{err}");
        let err = parse_call("waves", r#"{"epics":"PLEXUS"}"#).unwrap_err();
        assert!(err.contains("invalid type"), "{err}");
        let err = parse_call("health", r#"{"x":1}"#).unwrap_err();
        assert!(err.starts_with("health: ") && err.contains("unknown field `x`"), "{err}");
        let err = parse_call("list", r#"{"limit":-1}"#).unwrap_err();
        assert!(err.starts_with("list: "), "{err}");
        for not_an_object in ["[]", "[\"MNG-2\"]", "\"MNG-2\"", "5", "true"] {
            let err = parse_call("groups", not_an_object).unwrap_err();
            assert!(err.starts_with("groups: params must be an object"), "{not_an_object}: {err}");
        }
        // INPUT-4: `inputs`, with its own names.
        let err = parse_call("inputs", r#"{"id":"INPUT-9"}"#).unwrap_err();
        assert!(err.starts_with("inputs: ") && err.contains("unknown field `id`"), "{err}");
        let err = parse_call("inputs", r#"{"ticket":9}"#).unwrap_err();
        assert!(err.starts_with("inputs: ") && err.contains("invalid type"), "{err}");
        let err = parse_call("inputs", r#"{"state":["open"]}"#).unwrap_err();
        assert!(err.starts_with("inputs: ") && err.contains("invalid type"), "{err}");
    }

    // -- INPUT-4: the first write ----------------------------------------------

    use menger_tickets_api::{InputAction, InputHistoryEntry, InputKind, InputOption};

    fn option(value: &str) -> InputOption {
        InputOption { value: value.into(), label: value.to_uppercase(), recommended: false }
    }

    fn input(ticket: &str, id: &str, kind: InputKind, options: &[&str], state: InputState) -> Input {
        Input {
            ticket: ticket.into(),
            id: id.into(),
            kind,
            question: None,
            options: options.iter().map(|o| option(o)).collect(),
            recommendation: None,
            state,
            answer: None,
            history: Vec::<InputHistoryEntry>::new(),
        }
    }

    fn choice(option: &str) -> InputValue {
        InputValue::Choice { option: option.into() }
    }

    fn submitted(input: &str, value: InputValue) -> Submit {
        Submit { input: input.into(), value, rationale: None }
    }

    /// What `inputs { ticket: "DEMO-1" }` holds in these tests: a choice, a
    /// multi, a text, one closed and one removed from the body — and what
    /// another ticket declares, which a DEMO-1 frame must never answer.
    fn held() -> Vec<Input> {
        vec![
            input("DEMO-1", "d1", InputKind::Choice, &["a", "b"], InputState::Open),
            input("DEMO-1", "checks", InputKind::Multi, &["unit", "wire", "e2e"], InputState::Answered),
            input("DEMO-1", "notes", InputKind::Text, &[], InputState::Open),
            input("DEMO-1", "frozen", InputKind::Choice, &["a", "b"], InputState::Closed),
            input("DEMO-1", "gone", InputKind::Choice, &["a", "b"], InputState::Removed),
            input("DEMO-2", "other", InputKind::Choice, &["x", "y"], InputState::Open),
        ]
    }

    /// Accepted with a click on this frame (IDENTITY-36: src/gesture.rs; a
    /// browser that cannot say is refused there), a frame that renders a ticket, params the SDK sends, an
    /// input its ticket declares, a value that answers it — the value to
    /// send.
    #[test]
    fn a_submit_inside_a_click_for_this_tickets_input_is_sent() {
        assert_eq!(admit(SUBMIT, Some("DEMO-1"), true), Ok("DEMO-1".into()));
        assert_eq!(admit(SUBMIT, Some(" DEMO-1 "), true), Ok("DEMO-1".into()));
        let s = parse_submit(r#"{"input":"d1","value":{"type":"choice","option":"b"},"rationale":"holds are real"}"#).unwrap();
        assert_eq!(s, Submit { input: "d1".into(), value: choice("b"), rationale: Some("holds are real".into()) });
        assert_eq!(check_submit("DEMO-1", &s, &held()), Ok(choice("b")));
        let s = parse_submit(r#"{"input":"d1","value":{"type":"choice","option":"a"}}"#).unwrap();
        assert_eq!(s.rationale, None);
        // A multi's options go in the declared order, each once; an answered
        // input takes a new answer.
        let s = parse_submit(r#"{"input":"checks","value":{"type":"multi","options":["e2e","unit","e2e"]}}"#).unwrap();
        assert_eq!(check_submit("DEMO-1", &s, &held()), Ok(InputValue::Multi { options: vec!["unit".into(), "e2e".into()] }));
        let s = submitted("notes", InputValue::Text { text: "ship it, today".into() });
        assert_eq!(check_submit("DEMO-1", &s, &held()), Ok(InputValue::Text { text: "ship it, today".into() }));
    }

    /// Refused without a click, and in a frame that renders no ticket's
    /// stored body — before the params are read.
    #[test]
    fn a_submit_with_no_click_or_no_ticket_is_refused() {
        let err = admit(SUBMIT, Some("DEMO-1"), false).unwrap_err();
        assert!(err.starts_with("menger.input.submit: only inside a viewer's click"), "{err}");
        assert!(err.ends_with("(nothing sent)"), "{err}");
        for none in [None, Some(""), Some("  ")] {
            let err = admit(SUBMIT, none, true).unwrap_err();
            assert!(err.contains("this frame shows no ticket's stored body"), "{none:?}: {err}");
        }
        // No gesture outranks no ticket: the first thing said is the click.
        assert!(admit(SUBMIT, None, false).unwrap_err().contains("only inside a viewer's click"));
    }

    /// A frame cannot name a ticket — the field is refused — so another
    /// ticket's input is one its own ticket does not declare.
    #[test]
    fn another_tickets_input_is_refused() {
        let err = parse_submit(r#"{"ticket":"DEMO-2","input":"other","value":{"type":"choice","option":"x"}}"#).unwrap_err();
        assert!(err.contains("unknown field `ticket`"), "{err}");
        let s = submitted("other", choice("x"));
        assert_eq!(
            check_submit("DEMO-1", &s, &held()),
            Err("menger.input.submit: DEMO-1 declares no input 'other' — its inputs are 'd1', 'checks', 'notes', 'frozen' \
                 (nothing sent)"
                .into())
        );
        // A ticket whose body declares none.
        let err = check_submit("DEMO-3", &submitted("d1", choice("a")), &held()).unwrap_err();
        assert!(err.contains("DEMO-3 declares no input 'd1' — its body holds none"), "{err}");
        // DEMO-2's own frame answers it.
        assert_eq!(check_submit("DEMO-2", &s, &held()), Ok(choice("x")));
    }

    /// Refused for a value that does not answer the input — not one of its
    /// options, another kind, a blank reply — and for a closed or removed
    /// input, whatever the value.
    #[test]
    fn a_bad_value_or_a_closed_or_removed_input_is_refused() {
        let err = check_submit("DEMO-1", &submitted("d1", choice("zzz")), &held()).unwrap_err();
        assert!(err.starts_with("menger.input.submit: not an answer to DEMO-1's 'd1': 'zzz' is not one of its options ('a', 'b')"), "{err}");
        let err = check_submit("DEMO-1", &submitted("d1", InputValue::Text { text: "a".into() }), &held()).unwrap_err();
        assert!(err.contains("the input is a choice and the value a text"), "{err}");
        let err = check_submit("DEMO-1", &submitted("checks", InputValue::Multi { options: vec!["unit".into(), "smoke".into()] }), &held())
            .unwrap_err();
        assert!(err.contains("'smoke' not among its options"), "{err}");
        let err = check_submit("DEMO-1", &submitted("notes", InputValue::Text { text: "  ".into() }), &held()).unwrap_err();
        assert!(err.contains("a reply is not blank"), "{err}");
        let err = check_submit("DEMO-1", &submitted("frozen", choice("a")), &held()).unwrap_err();
        assert_eq!(err, "menger.input.submit: DEMO-1's 'frozen' is closed: its answer is frozen until it is reopened (nothing sent)");
        let err = check_submit("DEMO-1", &submitted("gone", choice("a")), &held()).unwrap_err();
        assert_eq!(err, "menger.input.submit: DEMO-1's 'gone' was removed from its body (nothing sent)");
        // Malformed values are named before any check.
        for bad in [
            r#"{"input":"d1","value":"a"}"#,
            r#"{"input":"d1","value":{"type":"bogus","option":"a"}}"#,
            r#"{"input":"d1","value":{"type":"choice"}}"#,
            r#"{"input":"d1"}"#,
            r#"{"value":{"type":"choice","option":"a"}}"#,
            r#"{"input":5,"value":{"type":"choice","option":"a"}}"#,
            r#"{"input":"d1","value":{"type":"choice","option":"a"},"source":"api"}"#,
            "null",
            "",
        ] {
            let err = parse_submit(bad).unwrap_err();
            assert!(err.starts_with("menger.input.submit: ") && err.contains("it takes (id, value, rationale?)"), "{bad}: {err}");
        }
    }

    /// The server's reply: the answer it recorded, or its own refusal.
    #[test]
    fn the_reply_is_the_recorded_answer_or_the_servers_refusal() {
        let mut changed = input("DEMO-1", "d1", InputKind::Choice, &["a", "b"], InputState::Answered);
        let answer = Answer { value: choice("b"), rationale: Some("why".into()), source: AnswerSource::App, at_unix: 1_791_400_000 };
        changed.answer = Some(answer.clone());
        assert_eq!(recorded(vec![TicketsEvent::InputChanged { input: changed }]), Ok(answer));
        assert_eq!(
            recorded(vec![TicketsEvent::Error { message: "DEMO-1's 'd1' is closed".into() }]),
            Err("menger.input.submit: DEMO-1's 'd1' is closed".into())
        );
        assert_eq!(recorded(vec![TicketsEvent::NotFound { id: "DEMO-1".into() }]), Err("menger.input.submit: DEMO-1 is not a ticket".into()));
        assert!(recorded(vec![]).is_err());
        // `inputs { ticket }`: its inputs, or the server's refusal.
        let held = inputs_of(SUBMIT, held().into_iter().map(|input| TicketsEvent::Input { input }).collect()).unwrap();
        assert_eq!(held.len(), 6);
        assert!(inputs_of(SUBMIT, vec![TicketsEvent::NotFound { id: "NOPE-1".into() }]).unwrap_err().contains("NOPE-1 is not a ticket"));
    }

    // -- INPUT-9: the second write -----------------------------------------------

    fn clearing(input: &str) -> Clear {
        Clear { input: input.into() }
    }

    /// Accepted on a click for an open input of the frame's ticket — one
    /// holding an answer, or none (the server then writes nothing) — with
    /// the params the SDK sends.
    #[test]
    fn a_clear_inside_a_click_for_this_tickets_open_input_is_sent() {
        assert_eq!(admit(CLEAR, Some("DEMO-1"), true), Ok("DEMO-1".into()));
        assert_eq!(parse_clear(r#"{"input":"checks"}"#), Ok(clearing("checks")));
        for id in ["d1", "checks", "notes"] {
            assert_eq!(check_clear("DEMO-1", &clearing(id), &held()), Ok(()), "{id}");
        }
        assert_eq!(check_clear("DEMO-2", &clearing("other"), &held()), Ok(()), "DEMO-2's own frame clears it");
    }

    /// Refused without a click, in a frame that renders no ticket, for
    /// another ticket's input (the frame cannot name one), and for a closed
    /// or removed input — each naming `menger.input.clear`.
    #[test]
    fn a_clear_with_no_click_for_another_ticket_or_a_closed_input_is_refused() {
        let err = admit(CLEAR, Some("DEMO-1"), false).unwrap_err();
        assert_eq!(
            err,
            "menger.input.clear: only inside a viewer's click — the app saw no click or key in this frame just now (nothing sent)"
        );
        let err = admit(CLEAR, None, true).unwrap_err();
        assert!(err.starts_with("menger.input.clear: this frame shows no ticket's stored body"), "{err}");
        let err = parse_clear(r#"{"ticket":"DEMO-2","input":"other"}"#).unwrap_err();
        assert!(err.starts_with("menger.input.clear: ") && err.contains("unknown field `ticket`"), "{err}");
        assert_eq!(
            check_clear("DEMO-1", &clearing("other"), &held()),
            Err("menger.input.clear: DEMO-1 declares no input 'other' — its inputs are 'd1', 'checks', 'notes', 'frozen' \
                 (nothing sent)"
                .into())
        );
        assert_eq!(
            check_clear("DEMO-1", &clearing("frozen"), &held()),
            Err("menger.input.clear: DEMO-1's 'frozen' is closed: its answer is frozen until it is reopened (nothing sent)".into())
        );
        assert_eq!(
            check_clear("DEMO-1", &clearing("gone"), &held()),
            Err("menger.input.clear: DEMO-1's 'gone' was removed from its body (nothing sent)".into())
        );
        let err = check_clear("DEMO-3", &clearing("d1"), &held()).unwrap_err();
        assert!(err.contains("DEMO-3 declares no input 'd1' — its body holds none"), "{err}");
        // A value, a source, or anything else the SDK does not send is named.
        for bad in [
            r#"{"input":"d1","value":{"type":"choice","option":"a"}}"#,
            r#"{"input":"d1","source":"api"}"#,
            r#"{"input":5}"#,
            "{}",
            "null",
            "",
        ] {
            let err = parse_clear(bad).unwrap_err();
            assert!(err.starts_with("menger.input.clear: ") && err.ends_with("it takes (id) (nothing sent)"), "{bad}: {err}");
        }
    }

    /// The server's reply: the input as the clear left it (or as it was,
    /// holding no answer), or its own refusal.
    #[test]
    fn the_clears_reply_is_the_input_or_the_servers_refusal() {
        let mut open = input("DEMO-1", "d1", InputKind::Choice, &["a", "b"], InputState::Open);
        open.history = vec![
            InputHistoryEntry { action: InputAction::Answered, value: Some(choice("a")), rationale: None, source: AnswerSource::App, at_unix: 1 },
            InputHistoryEntry { action: InputAction::Cleared, value: None, rationale: None, source: AnswerSource::App, at_unix: 2 },
        ];
        assert_eq!(cleared(vec![TicketsEvent::InputChanged { input: open.clone() }]), Ok(open.clone()));
        assert_eq!(cleared(vec![TicketsEvent::Input { input: open.clone() }]), Ok(open), "held no answer: nothing written");
        assert_eq!(
            cleared(vec![TicketsEvent::Error { message: "DEMO-1's 'd1' is closed".into() }]),
            Err("menger.input.clear: DEMO-1's 'd1' is closed".into())
        );
        assert_eq!(cleared(vec![TicketsEvent::NotFound { id: "DEMO-1".into() }]), Err("menger.input.clear: DEMO-1 is not a ticket".into()));
        assert_eq!(cleared(vec![]), Err("menger.input.clear: the server cleared nothing".into()));
        assert!(inputs_of(CLEAR, vec![TicketsEvent::Error { message: "x".into() }]).unwrap_err().starts_with("menger.input.clear: x"));
    }

    // -- IDENTITY-36: a frame reads its own subtree ------------------------------

    use menger_tickets_api::{Group, Kind, PlacementChange, Placement, Status, Ticket, TicketSummary};

    fn tkt(id: &str) -> Ticket {
        let (epic, seq) = id.rsplit_once('-').expect("EPIC-N");
        Ticket {
            labels: Vec::new(),
            id: id.to_string(),
            epic: epic.to_string(),
            seq: seq.parse().expect("seq"),
            title: format!("the title of {id}"),
            status: Status::Ready,
            kind: Kind::Build,
            severity: None,
            blocked_by: vec![],
            unlocks: vec![],
            parent: None,
            child_count: 0,
            commits: vec![],
            path: None,
            placement: Some(Placement::Wave { n: 1 }),
            body: format!("the body of {id}"),
            created_unix: 0,
            updated_unix: 0,
        }
    }

    fn row(id: &str) -> TicketSummary {
        TicketSummary::from(&tkt(id))
    }

    fn group(epic: &str, host: Option<&str>) -> Group {
        Group { epic: epic.into(), total: 3, done: 1, docs: 0, root: Some(format!("{epic}-1")), root_row: None, host: host.map(Into::into) }
    }

    /// AREA: its own shortcode and SUB, hosted under it — what `counts {
    /// filter: "under AREA" }` answers; OTHER is outside.
    fn area() -> Reach {
        Reach::from_counts(
            "area",
            vec![
                TicketsEvent::Count { epic: "AREA".into(), held: 4 },
                TicketsEvent::Count { epic: "SUB".into(), held: 2 },
                TicketsEvent::Count { epic: "EMPTY".into(), held: 0 },
            ],
        )
        .unwrap()
    }

    fn kept(ev: TicketsEvent) -> Option<String> {
        admit_event(ev, &area()).map(|e| serde_json::to_string(&e).unwrap())
    }

    /// The scope is the stamped ticket's shortcode — of an id or a path
    /// address — and nothing the filter language would read as more than
    /// one word.
    #[test]
    fn a_frames_scope_is_its_tickets_shortcode() {
        assert_eq!(scope_of(Some("MNG-12")), Some("MNG".into()));
        assert_eq!(scope_of(Some("plexus/_MENGER/board")), Some("PLEXUS".into()));
        assert_eq!(scope_of(Some(" ART-9 ")), Some("ART".into()));
        assert_eq!(scope_of(None), None);
        for bad in ["", "  ", "-3", "A B-1", "x)or(y-1"] {
            assert_eq!(scope_of(Some(bad)), None, "{bad:?}");
        }
    }

    /// The reach is what `counts` under the scope holds — the shortcode
    /// itself always, a shortcode holding nothing never — and the server's
    /// refusal is the call's.
    #[test]
    fn the_reach_is_the_counts_under_the_scope() {
        let r = area();
        assert_eq!(r.epic, "AREA");
        assert_eq!(r.epics, HashSet::from(["AREA".to_string(), "SUB".to_string()]));
        assert_eq!(r.tickets, 6);
        assert!(r.holds("sub") && r.holds_id("SUB-2") && r.holds_id("AREA/_MENGER/board"));
        assert!(!r.holds("OTHER") && !r.holds_id("OTHER-1") && !r.holds("EMPTY"));
        assert_eq!(Reach::from_counts("AREA", vec![TicketsEvent::Error { message: "no access yet".into() }]), Err("no access yet".into()));
        let only = Reach::only("new");
        assert_eq!(only.epics, HashSet::from(["NEW".to_string()]));
        assert!(Reach::none().epics.is_empty() && !Reach::none().holds_id("A-1"));
    }

    /// `under EPIC` alone, or ANDed with the frame's own filter; a filter
    /// the language does not take is refused, named.
    #[test]
    fn the_scope_is_anded_into_the_frames_filter() {
        assert_eq!(scoped_filter(None, "AREA"), Ok("under AREA".into()));
        assert_eq!(scoped_filter(Some("  "), "AREA"), Ok("under AREA".into()));
        let f = scoped_filter(Some("status Ready"), "AREA").unwrap();
        assert_eq!(Filter::parse(&f).unwrap(), Filter::parse("status Ready and under AREA").unwrap(), "{f}");
        // An `or` in theirs cannot escape the scope: the scope is its own group.
        let f = scoped_filter(Some("in OTHER or status Ready"), "AREA").unwrap();
        assert_eq!(Filter::parse(&f).unwrap(), Filter::parse("(in OTHER or status Ready) and under AREA").unwrap(), "{f}");
        let f = scoped_filter(Some("under OTHER"), "AREA").unwrap();
        assert!(f.contains("under OTHER") && f.contains("under AREA"), "{f}");
        assert!(scoped_filter(Some("status (Ready"), "AREA").unwrap_err().starts_with("filter: "));
        assert!(scoped_filter(None, "A B").is_err());
    }

    fn call(c: Call) -> Call {
        match scope_call(c, Some(&area())).unwrap() {
            Scoped::Call(c) => c,
            Scoped::Answer(a) => panic!("answered unasked: {a:?}"),
        }
    }

    /// What each read becomes under the scope: the filter-taking three get
    /// `under AREA` (ANDed with their own); `get`/`blockers` outside are
    /// `not_found` unasked; `waves` outside is refused; the rest go as
    /// asked, to be held to the reach on the way back.
    #[test]
    fn each_read_is_narrowed_to_the_reach() {
        let under = |f: &Option<String>| f.as_deref().and_then(|f| Filter::parse(f).ok());
        match call(parse_call("list", r#"{"epic":"OTHER","limit":5}"#).unwrap()) {
            Call::List { epic, limit, filter, .. } => {
                assert_eq!((epic.as_deref(), limit), (Some("OTHER"), Some(5)), "their params kept");
                assert_eq!(under(&filter), Filter::parse("under AREA").ok());
            }
            other => panic!("{other:?}"),
        }
        match call(parse_call("list", r#"{"filter":"kind build"}"#).unwrap()) {
            Call::List { filter, .. } => assert_eq!(under(&filter), Filter::parse("kind build and under AREA").ok()),
            other => panic!("{other:?}"),
        }
        match call(parse_call("find", r#"{"query":"x","filter":"not category done"}"#).unwrap()) {
            Call::Find { query, filter, .. } => {
                assert_eq!(query, "x");
                assert_eq!(under(&filter), Filter::parse("not category done and under AREA").ok());
            }
            other => panic!("{other:?}"),
        }
        match call(parse_call("inputs", r#"{"ticket":"OTHER-3","state":"open"}"#).unwrap()) {
            Call::Inputs { ticket, filter, .. } => {
                assert_eq!(ticket.as_deref(), Some("OTHER-3"));
                assert_eq!(under(&filter), Filter::parse("under AREA").ok(), "another ticket's inputs: none pass");
            }
            other => panic!("{other:?}"),
        }
        // Inside: asked. Outside: not_found, never asked.
        assert!(matches!(call(Call::Get { id: "SUB-2".into() }), Call::Get { .. }));
        assert!(matches!(call(Call::Blockers { id: "AREA-1".into() }), Call::Blockers { .. }));
        for c in [Call::Get { id: "OTHER-1".into() }, Call::Get { id: "OTHER/_MENGER/board".into() }, Call::Blockers { id: "OTHER-1".into() }] {
            match scope_call(c.clone(), Some(&area())).unwrap() {
                Scoped::Answer(a) => assert!(matches!(&a[..], [TicketsEvent::NotFound { .. }]), "{c:?}: {a:?}"),
                Scoped::Call(c) => panic!("asked: {c:?}"),
            }
        }
        assert!(matches!(call(Call::Waves { epics: vec!["area".into(), "SUB".into()], kinds: None }), Call::Waves { .. }));
        let err = scope_call(Call::Waves { epics: vec!["AREA".into(), "OTHER".into()], kinds: None }, Some(&area())).unwrap_err();
        assert!(err.starts_with("waves: OTHER is outside this frame's reach"), "{err}");
        for c in [Call::Groups, Call::Extents, Call::Health, Call::Components] {
            assert!(matches!(scope_call(c.clone(), Some(&area())), Ok(Scoped::Call(_))), "{c:?}");
        }
    }

    /// A frame with no scope reads components, and nothing else.
    #[test]
    fn a_frame_with_no_scope_reads_only_components() {
        assert!(matches!(scope_call(Call::Components, None), Ok(Scoped::Call(Call::Components))));
        for c in [
            Call::Get { id: "A-1".into() },
            Call::List { epic: None, status: None, parent: None, after: None, limit: None, summary: None, filter: None },
            Call::Find { query: String::new(), after: None, limit: None, filter: None },
            Call::Waves { epics: vec!["A".into()], kinds: None },
            Call::Groups,
            Call::Extents,
            Call::Blockers { id: "A-1".into() },
            Call::Health,
            Call::Inputs { ticket: None, epic: None, state: None, filter: None },
        ] {
            let m = c.method();
            let err = scope_call(c, None).unwrap_err();
            assert_eq!(err, format!("{m}: this frame renders no ticket's body — it reads nothing but components"));
        }
    }

    /// Nothing of a shortcode outside the reach passes — a ticket, a row, a
    /// blocker, an input and its answer, a group, a count, a move — and
    /// what is inside passes as it was.
    #[test]
    fn only_the_reach_reaches_a_frame() {
        for id in ["AREA-2", "SUB-1"] {
            assert!(kept(TicketsEvent::Ticket { ticket: tkt(id) }).is_some(), "{id}");
            assert!(kept(TicketsEvent::Amended { ticket: tkt(id), changed: vec!["body".into()] }).is_some());
            assert!(kept(TicketsEvent::Summary { summary: row(id) }).is_some());
        }
        let outside = || tkt("OTHER-7");
        for ev in [
            TicketsEvent::Ticket { ticket: outside() },
            TicketsEvent::Created { ticket: outside() },
            TicketsEvent::StatusChanged { ticket: outside(), previous: Status::Pending },
            TicketsEvent::KindChanged { ticket: outside(), previous: Kind::Gap },
            TicketsEvent::Amended { ticket: outside(), changed: vec!["body".into()] },
            TicketsEvent::CommitsLinked { ticket: outside(), added: vec![] },
            TicketsEvent::CommitsUnlinked { ticket: outside(), removed: vec![] },
            TicketsEvent::LabelsChanged { ticket: outside(), added: vec!["needs-you".into()], removed: vec![] },
            TicketsEvent::UnlocksChanged { ticket: outside(), added: "AREA-2".into() },
            TicketsEvent::ParentChanged { ticket: outside(), previous: Some("AREA-1".into()) },
            TicketsEvent::Blocker { ticket: outside() },
            TicketsEvent::Summary { summary: row("OTHER-7") },
            TicketsEvent::Placed { summary: row("OTHER-7"), placement: Placement::Wave { n: 1 }, waiting_on: vec![] },
            TicketsEvent::Node { summary: row("OTHER-7"), matched: true, below: 0 },
            TicketsEvent::Hit { summary: row("OTHER-7"), title: vec![], snippet: vec![], corrected: false },
            TicketsEvent::ChildrenChanged { id: "OTHER-1".into(), added: "OTHER-7".into(), child_count: 1 },
            TicketsEvent::ChildRemoved { id: "OTHER-1".into(), removed: "OTHER-7".into(), child_count: 0 },
            TicketsEvent::Member { id: "OTHER-7".into(), matched: true },
            TicketsEvent::Input { input: input("OTHER-7", "d1", InputKind::Choice, &["a"], InputState::Answered) },
            TicketsEvent::InputChanged { input: input("OTHER-7", "d1", InputKind::Choice, &["a"], InputState::Answered) },
            TicketsEvent::Group { group: group("OTHER", None) },
            TicketsEvent::GroupChanged { group: group("OTHER", Some("AREA-1")) },
            TicketsEvent::Extent { epic: "OTHER".into(), last_seq: 9 },
            TicketsEvent::Count { epic: "OTHER".into(), held: 9 },
            TicketsEvent::Entered { id: "OTHER-7".into(), epic: "OTHER".into(), seq: 7 },
            TicketsEvent::Left { id: "OTHER-7".into(), epic: "OTHER".into(), seq: 7 },
            TicketsEvent::Recount { epic: "OTHER".into(), held: 9 },
            TicketsEvent::PlacementsChanged { changes: vec![PlacementChange { id: "OTHER-7".into(), placement: Placement::Done }] },
            TicketsEvent::Facet { field: "status".into(), value: "Ready".into(), held: 40 },
            TicketsEvent::Searched { matched: 40, next: None, words: vec![] },
        ] {
            let json = serde_json::to_string(&ev).unwrap();
            assert_eq!(kept(ev), None, "reached the frame: {json}");
        }
        // Inside, the input, its answer and its history pass whole.
        assert!(kept(TicketsEvent::InputChanged { input: input("SUB-1", "d1", InputKind::Choice, &["a"], InputState::Open) }).is_some());
        // A placement change keeps only its moves inside.
        let moved = admit_event(
            TicketsEvent::PlacementsChanged {
                changes: vec![
                    PlacementChange { id: "OTHER-7".into(), placement: Placement::Done },
                    PlacementChange { id: "SUB-1".into(), placement: Placement::Wave { n: 2 } },
                ],
            },
            &area(),
        );
        match moved {
            Some(TicketsEvent::PlacementsChanged { changes }) => assert_eq!(changes.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["SUB-1"]),
            other => panic!("{other:?}"),
        }
        // A group inside, hosted under a ticket outside, stands at the top.
        match admit_event(TicketsEvent::Group { group: group("SUB", Some("OTHER-2")) }, &area()) {
            Some(TicketsEvent::Group { group }) => assert_eq!(group.host, None),
            other => panic!("{other:?}"),
        }
        match admit_event(TicketsEvent::GroupChanged { group: group("SUB", Some("AREA-1")) }, &area()) {
            Some(TicketsEvent::GroupChanged { group }) => assert_eq!(group.host.as_deref(), Some("AREA-1")),
            other => panic!("{other:?}"),
        }
        // `health` counts the reach, not the viewer's whole store.
        match admit_event(TicketsEvent::Health { version: "v".into(), unix_now: 1, ticket_count: 5000 }, &area()) {
            Some(TicketsEvent::Health { ticket_count, .. }) => assert_eq!(ticket_count, 6),
            other => panic!("{other:?}"),
        }
        // Components, pages, lags, refusals and the wave counts pass.
        for ev in [
            TicketsEvent::ComponentRemoved { name: "m-x".into() },
            TicketsEvent::Page { next: None, remaining: 0, first: None },
            TicketsEvent::Lagged { missed: 3 },
            TicketsEvent::NotFound { id: "OTHER-1".into() },
            TicketsEvent::Error { message: "x".into() },
            TicketsEvent::DanglingBlocker { id: "NOPE-1".into() },
            TicketsEvent::WaveCounts { waves: vec![1], missing: 0, done: 0 },
        ] {
            assert!(kept(ev).is_some());
        }
    }

    /// A frame with no scope hears components and nothing of any ticket.
    #[test]
    fn a_frame_with_no_scope_hears_components_alone() {
        let none = Reach::none();
        assert!(admit_event(TicketsEvent::Ticket { ticket: tkt("AREA-1") }, &none).is_none());
        assert!(admit_event(TicketsEvent::Count { epic: "AREA".into(), held: 1 }, &none).is_none());
        assert!(admit_event(TicketsEvent::ComponentRemoved { name: "m-x".into() }, &none).is_some());
    }

    /// A call's answer, held: a `get` that lost its ticket is `not_found`;
    /// `blockers` keeps only the blockers inside.
    #[test]
    fn a_calls_answer_is_held_to_the_reach() {
        let got = held_to(vec![TicketsEvent::Ticket { ticket: tkt("OTHER-1") }], &area(), Some("OTHER-1".into()));
        assert!(matches!(&got[..], [TicketsEvent::NotFound { id }] if id == "OTHER-1"), "{got:?}");
        let got = held_to(vec![TicketsEvent::Ticket { ticket: tkt("SUB-1") }], &area(), Some("SUB-1".into()));
        assert!(matches!(&got[..], [TicketsEvent::Ticket { ticket }] if ticket.id == "SUB-1"), "{got:?}");
        let got = held_to(
            vec![
                TicketsEvent::Blocker { ticket: tkt("AREA-3") },
                TicketsEvent::Blocker { ticket: tkt("OTHER-4") },
                TicketsEvent::DanglingBlocker { id: "GONE-1".into() },
            ],
            &area(),
            None,
        );
        let ids: Vec<String> = got
            .iter()
            .map(|e| match e {
                TicketsEvent::Blocker { ticket } => ticket.id.clone(),
                TicketsEvent::DanglingBlocker { id } => id.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(ids, ["AREA-3", "GONE-1"]);
    }

    /// A shortcode that moves out from under a reach leaves it at once, and
    /// the reach is asked again; one that moves under it is asked about
    /// (it joins on the server's word); anything else moves nothing.
    #[test]
    fn a_regrouped_shortcode_moves_the_reaches() {
        let mut reaches = HashMap::from([("AREA".to_string(), area())]);
        assert!(moved(&mut reaches, &group("SUB", Some("AREA-1"))).is_empty(), "still under");
        assert!(moved(&mut reaches, &group("AREA", None)).is_empty(), "the scope itself");
        assert!(moved(&mut reaches, &group("OTHER", None)).is_empty(), "never in, not now");
        assert_eq!(moved(&mut reaches, &group("NEW", Some("SUB-1"))), ["AREA"], "newly under");
        assert!(!reaches["AREA"].holds("NEW"), "not before the server says");
        assert_eq!(moved(&mut reaches, &group("SUB", Some("OTHER-1"))), ["AREA"], "out from under");
        assert!(!reaches["AREA"].holds("SUB"), "gone at once");
        let mut reaches = HashMap::from([("AREA".to_string(), area())]);
        assert_eq!(moved(&mut reaches, &group("SUB", None)), ["AREA"], "to the top");
        assert!(!reaches["AREA"].holds("SUB"));
    }
}
