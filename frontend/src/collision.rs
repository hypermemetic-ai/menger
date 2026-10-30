//! TICKETS-36: **the collision page** — what a name that fits several tickets
//! opens. The owner, 2026-10-30: *"a collision in a url like /<COLLISION-1>
//! should show all the collisions and their parents"*.
//!
//! `#/ticket/<name>` asks `get`; when the name fits several tickets the store
//! answers `ambiguous { id, candidates, choices }` (TICKETS-35), and this page
//! draws one row per choice, in the store's order: the address (its hosts
//! muted, its id in the accent), kind, status, title, the parent (its id and
//! title, a link by its uid), and where it lives (its epic; its organization
//! too when the choices span several). A row opens its ticket at its own
//! `#/t/<uid>/<ID>-<slug>`. A choice out of the reader's reach is its address
//! alone and is no link.
//!
//! Keys (src/bindings.rs, `Context::Collision`): ↑ ↓ (k j) choose, ⏎ opens,
//! `c` copies the chosen ticket's link (`<origin>/t/<uid>/…`), Esc goes to
//! the board (through `app.escape`: after an open overlay, before a
//! selection the page does not show). The line under the rows says what
//! was copied.
//!
//! What it draws from `choices` is pure here ([`rows`], [`heading`],
//! [`lede`], [`step`]) and pinned by native tests.

use std::cell::RefCell;

use leptos::prelude::*;
use menger_tickets_api::{Choice, Kind, Status};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::{cache, dom, links, menu, nav, st_class};

/// A choice's parent, as its row draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parent {
    /// Its id as a row shows it (`PERF-1`).
    pub(crate) id: String,
    pub(crate) title: String,
    /// Its link, by its uid.
    pub(crate) href: String,
}

/// One row of the page: a choice, as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// What hosts it, spaced (`hypermemetic : MENGER / STORE /`) — muted.
    pub(crate) hosts: String,
    /// Its id as its address ends (`PERF-19`) — the accent.
    pub(crate) id: String,
    /// Its key (the one menu's, `data-ticket-id`); none out of reach.
    pub(crate) key: Option<String>,
    /// Its route, `#/t/<uid>/<ID>-<slug>`; none for a choice out of reach.
    pub(crate) href: Option<String>,
    /// Its permalink's path, `/t/<uid>/<ID>-<slug>` (Copy link).
    pub(crate) path: Option<String>,
    pub(crate) kind: Option<Kind>,
    pub(crate) status: Option<Status>,
    pub(crate) title: Option<String>,
    pub(crate) parent: Option<Parent>,
    /// Where it lives: its epic, `org · epic` when the choices span orgs.
    pub(crate) place: Option<String>,
}

impl Row {
    /// Whether the reader may open it.
    pub(crate) fn reachable(&self) -> bool {
        self.href.is_some()
    }
}

/// An address as its row draws it: its hosts (spaced, ending in their
/// separator) and its id — what follows the last `/` or `:`.
pub(crate) fn split(address: &str) -> (String, String) {
    match address.rfind(['/', ':']) {
        Some(at) => {
            let hosts = address[..=at].replace(':', " : ").replace('/', " / ");
            (hosts.split_whitespace().collect::<Vec<_>>().join(" "), address[at + 1..].to_string())
        }
        None => (String::new(), address.to_string()),
    }
}

/// The rows `choices` draw, in their order. A choice with a uid opens at its
/// route by uid; one without (out of the reader's reach, or a server's
/// before TICKETS-35) is its address alone.
pub(crate) fn rows(choices: &[Choice]) -> Vec<Row> {
    let first_org = choices.iter().find_map(|c| c.org.as_ref());
    let orgs_differ = choices.iter().filter_map(|c| c.org.as_ref()).any(|o| Some(o) != first_org);
    choices
        .iter()
        .map(|c| {
            let (hosts, id) = split(&c.address);
            let title = c.title.clone().unwrap_or_default();
            let link = c.uid.filter(|u| u.is_some()).zip(c.id.as_ref()).map(|(uid, key)| (links::uid_href(&uid, key, &title), format!("/t/{uid}/{}", links::tail(key, &title))));
            let reachable = link.is_some();
            let place = c.epic.as_ref().filter(|_| reachable).map(|epic| match (&c.org, orgs_differ) {
                (Some(org), true) if Some(org) != first_org => format!("{org} · {epic}"),
                _ => epic.to_string(),
            });
            let parent = c.parent.as_ref().filter(|_| reachable).map(|p| Parent {
                id: crate::epics::shown_id(&p.id),
                title: p.title.clone(),
                href: match p.uid.is_some() {
                    true => links::uid_href(&p.uid, &p.id, &p.title),
                    false => links::name_href(&p.address),
                },
            });
            let (href, path) = link.map_or((None, None), |(h, p)| (Some(h), Some(p)));
            Row {
                hosts,
                id,
                key: c.id.as_ref().filter(|_| reachable).map(|k| k.to_string()),
                href,
                path,
                kind: c.kind.filter(|_| reachable),
                status: c.status.filter(|_| reachable),
                title: c.title.clone().filter(|_| reachable),
                parent,
                place,
            }
        })
        .collect()
}

/// A count in words, as a heading says it (two to twelve), else digits.
fn count(n: usize) -> String {
    const WORDS: [&str; 13] = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"];
    WORDS.get(n).map_or_else(|| n.to_string(), |w| w.to_string())
}

/// The page's heading: what the name does — `names two tickets`.
pub(crate) fn heading(n: usize) -> String {
    format!("names {} tickets", count(n))
}

/// The line under it, before the muted half: why — the shortcodes that
/// share the name's letters, or how many answer to the name.
pub(crate) fn lede(asked: &str, rows: &[Row]) -> String {
    let code = |r: &Row| r.id.rsplit_once('-').map(|(c, _)| c.to_string());
    let codes: Vec<Option<String>> = rows.iter().map(code).collect();
    let n = count(rows.len());
    let mut n_up = n.clone();
    if let Some(first) = n_up.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    match codes.first() {
        Some(Some(c)) if codes.iter().all(|x| x.as_ref() == Some(c)) => format!("{n_up} shortcodes are named {c}, each in its own place."),
        _ => format!("{n_up} tickets answer to {asked}."),
    }
}

/// The row a key moves the choice to from `at`: the next reachable one
/// down (or up), wrapping; `at` when none is.
pub(crate) fn step(at: usize, rows: &[Row], down: bool) -> usize {
    let n = rows.len();
    for k in 1..=n {
        let i = if down { (at + k) % n } else { (at + n * k - k) % n };
        if rows[i].reachable() {
            return i;
        }
    }
    at
}

/// The first row that opens, else the first.
fn first(rows: &[Row]) -> usize {
    rows.iter().position(Row::reachable).unwrap_or(0)
}

/// TICKETS-36: what a choice tells the app — its uid and key, its parent's —
/// so a row opened goes straight to its page.
pub(crate) fn learn(c: &Choice) {
    if let (Some(uid), Some(key)) = (c.uid, c.id.as_ref()) {
        cache::learn_pair(uid, key, c.title.as_deref().unwrap_or_default());
    }
    if let Some(p) = &c.parent {
        cache::learn_pair(p.uid, &p.id, &p.title);
    }
}

// ---------------------------------------------------------------------
// The page

/// The page while it is up: its rows, the one chosen, what its line says.
#[derive(Clone)]
struct Page {
    rows: Vec<Row>,
    at: RwSignal<usize>,
    said: RwSignal<String>,
}

thread_local! {
    static PAGE: RefCell<Option<Page>> = const { RefCell::new(None) };
}

/// The row `i` chosen — and focused, from a key.
fn choose(p: &Page, i: usize, focus: bool) {
    p.at.set(i);
    if focus {
        if let Some(el) = dom::window().document().and_then(|d| d.get_element_by_id(&format!("choice-{i}"))) {
            if let Ok(el) = el.dyn_into::<web_sys::HtmlElement>() {
                let _ = el.focus();
            }
        }
    }
}

/// Open row `i`: its ticket's page, by uid.
fn open(p: &Page, i: usize) {
    if let Some(href) = p.rows.get(i).and_then(|r| r.href.clone()) {
        let _ = dom::window().location().set_hash(&href);
    }
}

/// Copy row `i`'s link, and say so.
fn copy(p: &Page, i: usize) {
    let Some(path) = p.rows.get(i).and_then(|r| r.path.clone()) else {
        p.said.set("Only its address is yours to see: it has no link to copy.".into());
        return;
    };
    let link = format!("{}{path}", dom::window().location().origin().unwrap_or_default());
    let said = p.said;
    spawn_local(async move {
        let words = match menu::copy_text(&link).await {
            menu::Copied::Manual => format!("The clipboard is not available — the link: {link}"),
            _ => format!("copied {link}"),
        };
        said.try_set(words);
    });
}

/// A key on the collision page: ↑ ↓ (k j) choose, ⏎ opens, `c` copies —
/// true when it was one (the caller prevents its default).
pub(crate) fn key(ev: &web_sys::KeyboardEvent) -> bool {
    let Some(p) = PAGE.with(|p| p.borrow().clone()) else { return false };
    let at = p.at.get_untracked();
    match bindings::hit(ev, Context::Collision) {
        Some("collision.next") => choose(&p, step(at, &p.rows, true), true),
        Some("collision.prev") => choose(&p, step(at, &p.rows, false), true),
        Some("collision.open") => open(&p, at),
        Some("collision.copy") => copy(&p, at),
        _ => return false,
    }
    true
}

/// Esc on the collision page: the board. True when the page was up.
pub(crate) fn escape() -> bool {
    if PAGE.with(|p| p.borrow().is_none()) {
        return false;
    }
    let _ = dom::window().location().set_hash("/");
    true
}

/// The collision page: the name asked, and one row per choice.
#[component]
pub(crate) fn CollisionPage(asked: String, choices: Vec<Choice>) -> impl IntoView {
    let rows = rows(&choices);
    let page = Page { at: RwSignal::new(first(&rows)), said: RwSignal::new(String::new()), rows: rows.clone() };
    PAGE.with(|p| *p.borrow_mut() = Some(page.clone()));
    on_cleanup(|| PAGE.with(|p| *p.borrow_mut() = None));
    let at = page.at;
    let said = page.said;
    let shown = crate::epics::shown_id(&asked);
    let label = format!("tickets named {shown}");
    let items = rows
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let page = page.clone();
            let reachable = r.reachable();
            let on_click = move |ev: web_sys::MouseEvent| {
                let target = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
                let within = |sel: &str| target.as_ref().and_then(|t| t.closest(sel).ok().flatten()).is_some();
                if within("a") {
                    return; // the parent's link: followed as a link
                }
                if !reachable {
                    return;
                }
                choose(&page, i, false);
                if within(".copy") {
                    copy(&page, i);
                } else {
                    open(&page, i);
                }
            };
            let kind = r.kind.map(|k| k.as_str().to_string());
            let status = r.status;
            view! {
                <li
                    id=format!("choice-{i}")
                    class="choice"
                    class:out=!reachable
                    role="option"
                    data-ticket-id=r.key.clone()
                    tabindex=move || (reachable && at.get() == i).then_some("0").unwrap_or("-1")
                    aria-selected=move || if at.get() == i && reachable { "true" } else { "false" }
                    aria-disabled=(!reachable).then_some("true")
                    on:click=on_click
                    on:focus=move |_| if reachable { at.set(i) }
                >
                    <span class="kind" data-kind=kind.clone() title=kind.clone() aria-hidden="true">{kind.as_deref().map(nav::kind_glyph).unwrap_or("·")}</span>
                    <span class="addr"><span class="hosts">{r.hosts.clone()}</span><span class="id">{r.id.clone()}</span></span>
                    {status.map(|s| view! { <span class=format!("cst {}", st_class(s.as_str()))>{s.to_string()}</span> })}
                    {r.title.clone().map(|t| view! { <span class="ctitle">{t}</span> })}
                    {r.parent.clone().map(|p| view! {
                        <span class="cparent">"under"<a href=p.href.clone() title=format!("{} — {}", p.id, p.title)>{p.id.clone()}</a><span class="pt">{p.title.clone()}</span></span>
                    })}
                    {(!reachable).then(|| view! { <span class="cout">"not yours to read"</span> })}
                    {reachable.then(|| view! {
                        <span class="meta">
                            <span>{r.place.clone().unwrap_or_default()}</span>
                            <span class="link">{r.path.clone().unwrap_or_default()}</span>
                            <button class="copy" type="button">"copy link"</button>
                        </span>
                    })}
                </li>
            }
        })
        .collect_view();
    view! {
        <div id="collision">
            <p class="crumbs"><a href="#/">"← board"</a></p>
            <div class="chead"><span class="idchip">{shown.clone()}</span><h1>{heading(choices.len())}</h1></div>
            <p class="lede">{lede(&asked, &page.rows)}" "<span class="muted">"Pick the one you meant; each opens at a link of its own."</span></p>
            <ol class="choices" role="listbox" aria-label=label>{items}</ol>
            <p class="foot">
                <span class="keys"><kbd>"↑"</kbd>" "<kbd>"↓"</kbd>" choose · "<kbd>"⏎"</kbd>" open · "<kbd>"c"</kbd>" copy link · "<kbd>"esc"</kbd>" board"</span>
                <span class="said" aria-live="polite">{move || said.get()}</span>
            </p>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::{ChoiceParent, TicketId, Uid};

    fn uid(s: &str) -> Uid {
        s.parse().unwrap()
    }

    /// The mockup's three (synthetic), and one the reader may not read.
    fn choices() -> Vec<Choice> {
        let c = |u: &str, id: &str, address: &str, org: &str, epic: &str, kind: Kind, status: Status, title: &str, parent: Option<ChoiceParent>| Choice {
            uid: Some(uid(u)),
            id: Some(TicketId::new(id)),
            address: address.into(),
            org: Some(org.parse().unwrap()),
            epic: Some(epic.parse().unwrap()),
            kind: Some(kind),
            status: Some(status),
            title: Some(title.into()),
            parent,
        };
        let p = |u: &str, id: &str, address: &str, title: &str| Some(ChoiceParent { uid: uid(u), id: TicketId::new(id), address: address.into(), title: title.into() });
        vec![
            c("7k3mq2xa", "THEME-3", "hypermemetic:MENGER/APP/THEME-3", "hypermemetic", "genesis", Kind::Build, Status::Ready,
                "Dark mode follows the system until you pick one", p("m4c8tz1e", "THEME-1", "hypermemetic:MENGER/APP/THEME-1", "The app's look")),
            c("q2r9wd4c", "THEME.62-3", "hypermemetic:PLEXUS/THEME-3", "hypermemetic", "genesis", Kind::Gap, Status::Coding,
                "A service's tokens leak into another's frame", p("b6yk0nf3", "THEME.62-1", "hypermemetic:PLEXUS/THEME-1", "Themes across services")),
            c("h8vn3k0p", "THEME.70-3", "acme:THEME-3", "acme", "spring", Kind::Spike, Status::Draft,
                "Spring catalogue colours — which four swatches print true", p("x1d7qa9r", "THEME.70-1", "acme:THEME-1", "acme's look")),
            Choice { address: "hypermemetic:OPS/THEME-3".into(), ..Choice::default() },
        ]
    }

    #[test]
    fn an_address_is_its_hosts_muted_and_its_id() {
        assert_eq!(split("hypermemetic:MENGER/STORE/PERF-19"), ("hypermemetic : MENGER / STORE /".into(), "PERF-19".into()));
        assert_eq!(split("hypermemetic:PLEXUS/PERF-19"), ("hypermemetic : PLEXUS /".into(), "PERF-19".into()));
        assert_eq!(split("acme:PERF-2"), ("acme :".into(), "PERF-2".into()));
        assert_eq!(split("PERF-2"), (String::new(), "PERF-2".into()));
        assert_eq!(split("acme:OPS/SYNAPSE-CC-9"), ("acme : OPS /".into(), "SYNAPSE-CC-9".into()));
    }

    /// One row per choice, in the store's order, each with its address,
    /// kind, status, title, parent (a link by its uid) and place — and a
    /// link of its own by uid; one out of reach is its address alone.
    #[test]
    fn each_choice_is_a_row_with_its_parent_linked_by_uid() {
        let r = rows(&choices());
        assert_eq!(r.len(), 4);
        let a = &r[0];
        assert_eq!((a.hosts.as_str(), a.id.as_str()), ("hypermemetic : MENGER / APP /", "THEME-3"));
        assert_eq!(a.href.as_deref(), Some("#/t/7k3mq2xa/THEME-3-dark-mode-follows-the-system-until-you-pick-one"));
        assert_eq!(a.path.as_deref(), Some("/t/7k3mq2xa/THEME-3-dark-mode-follows-the-system-until-you-pick-one"));
        assert_eq!((a.kind, a.status), (Some(Kind::Build), Some(Status::Ready)));
        assert_eq!(a.title.as_deref(), Some("Dark mode follows the system until you pick one"));
        assert_eq!(a.parent, Some(Parent { id: "THEME-1".into(), title: "The app's look".into(), href: "#/t/m4c8tz1e/THEME-1-the-apps-look".into() }));
        let b = &r[1];
        assert_eq!((b.hosts.as_str(), b.id.as_str()), ("hypermemetic : PLEXUS /", "THEME-3"));
        assert_eq!(b.href.as_deref(), Some("#/t/q2r9wd4c/THEME-3-a-services-tokens-leak-into-anothers-frame"), "a namesake's key is shown as its id");
        assert_eq!(b.parent.as_ref().map(|p| (p.id.as_str(), p.href.as_str())), Some(("THEME-1", "#/t/b6yk0nf3/THEME-1-themes-across-services")));
        // The organizations differ: the one the first choice is not in is named.
        let places: Vec<Option<&str>> = r.iter().map(|x| x.place.as_deref()).collect();
        assert_eq!(places, [Some("genesis"), Some("genesis"), Some("acme · spring"), None]);
        // Out of reach: the address alone, no link.
        let d = &r[3];
        assert_eq!((d.hosts.as_str(), d.id.as_str()), ("hypermemetic : OPS /", "THEME-3"));
        assert!(!d.reachable() && d.path.is_none() && d.title.is_none() && d.kind.is_none() && d.status.is_none() && d.parent.is_none() && d.place.is_none());
        // A choice with no parent the reader reads draws none.
        let mut lone = choices();
        lone[0].parent = None;
        assert_eq!(rows(&lone)[0].parent, None);
        // One organization: the epic alone.
        assert!(rows(&choices()[..2]).iter().all(|x| x.place.as_deref() == Some("genesis")));
    }

    #[test]
    fn the_heading_and_lede_say_how_many_and_why() {
        assert_eq!(heading(2), "names two tickets");
        assert_eq!(heading(3), "names three tickets");
        assert_eq!(heading(12), "names twelve tickets");
        assert_eq!(heading(13), "names 13 tickets");
        let r = rows(&choices()[..3]);
        assert_eq!(lede("THEME-3", &r), "Three shortcodes are named THEME, each in its own place.");
        let mut mixed = choices();
        mixed[1].address = "hypermemetic:PLEXUS/PALETTE-3".into();
        assert_eq!(lede("hypermemetic:THEME-3", &rows(&mixed[..2])), "Two tickets answer to hypermemetic:THEME-3.");
    }

    /// ↑ ↓ choose among the rows the reader may open, wrapping; a row out of
    /// reach is passed over.
    #[test]
    fn the_keys_choose_among_the_rows_that_open() {
        let r = rows(&choices());
        assert_eq!(step(0, &r, true), 1);
        assert_eq!(step(1, &r, true), 2);
        assert_eq!(step(2, &r, true), 0, "past the last that opens, the first — the one out of reach passed over");
        assert_eq!(step(0, &r, false), 2, "above the first, the last that opens");
        assert_eq!(step(2, &r, false), 1);
        let none = rows(&choices()[3..]);
        assert_eq!(step(0, &none, true), 0, "nothing opens: the choice stays");
    }
}
