//! COMMENT-3: `#/comments` — every comment, newest first, filtered, live.
//!
//! **The filters** are COMMENT-2's `list` filters, as the route's query:
//! `#/comments?ticket=DEMAND-2&under=RETURNS&state=open&author=owner&text=title`
//! ([`Filter::read`], [`Filter::query`]); the lens's own keys (`g`, `n`,
//! `f`) ride beside them untouched. The bar edits them in place (the query
//! rewritten, no history entry); a link that names them opens them.
//!
//! **The rows** are the app's copy of the comments (src/comments.rs),
//! which the page asks for and `comments.watch` keeps — a comment added
//! anywhere (another browser, an agent through synapse) appears, and a
//! resolve moves its row's state, as it happens. Every filter is decided
//! here, as the server decides it ([`Filter::admits`]) — but `under`, the
//! comment's ticket in a ticket's or a shortcode's lineage, which only the
//! server can say (it holds the tree): the page asks `comments.list { under
//! }` for the ids it admits, and asks again when the copy gains a comment.
//!
//! **A row** links back to its place: its page, with `?comment=<id>`, where
//! the app scrolls to the element and flashes it (src/annotate.rs); its
//! ticket; its state, author and time; its body; what was there (the label
//! and the excerpt); and Resolve, or Reopen.
//!
//! What decides — reading and writing the query, the filters, the rows'
//! order, a row's link — is pure and pinned by native tests.

use std::collections::HashSet;

use leptos::prelude::*;
use menger_tickets_api::{Comment, CommentState};
use wasm_bindgen_futures::spawn_local;

use crate::comments::{self, Held};
use crate::lens::{self, Pair};
use crate::{dom, fmt_relative, fmt_unix, now_unix};

/// The query keys the page owns, in the order it writes them.
pub(crate) const KEYS: [&str; 5] = ["ticket", "under", "state", "author", "text"];

/// The page's filters — each `None` when it is not given (a blank value is
/// none, as the server reads it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Filter {
    pub(crate) ticket: Option<String>,
    pub(crate) under: Option<String>,
    pub(crate) state: Option<CommentState>,
    pub(crate) author: Option<String>,
    pub(crate) text: Option<String>,
}

impl Filter {
    /// The filters a query names; the lens's keys and anything unknown are
    /// left alone, a value that does not decode or a state that is none is
    /// no filter, and the last of a repeated key wins.
    pub(crate) fn read(q: &str) -> Self {
        let mut f = Filter::default();
        for Pair { key, value } in lens::pairs(q) {
            let Some(v) = value.as_deref().and_then(lens::dec).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) else {
                continue;
            };
            match key.as_str() {
                "ticket" => f.ticket = Some(v.to_ascii_uppercase()),
                "under" => f.under = Some(v.to_ascii_uppercase()),
                "state" => f.state = v.parse().ok(),
                "author" => f.author = Some(v),
                "text" => f.text = Some(v),
                _ => {}
            }
        }
        f
    }

    /// The page's pairs, in [`KEYS`]' order.
    fn pairs(&self) -> Vec<Pair> {
        let value = |k: &str| -> Option<String> {
            match k {
                "ticket" => self.ticket.clone(),
                "under" => self.under.clone(),
                "state" => self.state.map(|s| s.as_str().to_string()),
                "author" => self.author.clone(),
                "text" => self.text.clone(),
                _ => None,
            }
        };
        KEYS.iter().filter_map(|k| value(k).map(|v| Pair::new(k, Some(lens::enc(&v))))).collect()
    }

    /// The query for these filters, in place of the page's keys in `q` —
    /// every other key (the lens's) kept where it was.
    pub(crate) fn query(&self, q: &str) -> String {
        let mut out: Vec<Pair> = lens::pairs(q).into_iter().filter(|p| !KEYS.contains(&p.key.as_str())).collect();
        out.extend(self.pairs());
        lens::join(&out)
    }

    pub(crate) fn is_empty(&self) -> bool {
        *self == Filter::default()
    }

    /// Whether `c` is one of the comments these filters list — every filter
    /// as `comments.list` reads it: the anchor's ticket exactly (any case),
    /// the state, the author exactly, the text in the body, the label or the
    /// excerpt (any ASCII case). `under` is the server's to say: `under_ids`
    /// are the ids it answered for this filter's `under` (`None`: not yet —
    /// nothing is admitted until it has).
    pub(crate) fn admits(&self, c: &Comment, under_ids: Option<&HashSet<u64>>) -> bool {
        let has = |hay: &str, needle: &str| hay.to_ascii_lowercase().contains(&needle.to_ascii_lowercase());
        self.ticket.as_deref().is_none_or(|t| c.anchor.ticket.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(t)))
            && self.state.is_none_or(|s| c.state == s)
            && self.author.as_deref().is_none_or(|a| c.author == a)
            && self.text.as_deref().is_none_or(|t| has(&c.body, t) || has(&c.anchor.label, t) || has(&c.anchor.excerpt, t))
            && (self.under.is_none() || under_ids.is_some_and(|ids| ids.contains(&c.id)))
    }
}

/// The rows: the comments `f` admits, newest first.
pub(crate) fn rows(all: &[Comment], f: &Filter, under_ids: Option<&HashSet<u64>>) -> Vec<Comment> {
    let mut out: Vec<Comment> = all.iter().filter(|c| f.admits(c, under_ids)).cloned().collect();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

/// Where a row goes back to: its page, naming it (`?comment=<id>`).
pub(crate) fn link(c: &Comment) -> String {
    let route = &c.anchor.route;
    let sep = if route.contains('?') { '&' } else { '?' };
    format!("{route}{sep}comment={}", c.id)
}

/// `#/comments` for one ticket's open comments — the side panel's link.
pub(crate) fn ticket_link(ticket: &str) -> String {
    let f = Filter { ticket: Some(ticket.to_string()), state: Some(CommentState::Open), ..Filter::default() };
    format!("#/comments?{}", f.query(""))
}

/// The count line.
pub(crate) fn count_line(shown: usize, all: usize, filtered: bool) -> String {
    let word = |n: usize| if n == 1 { "comment" } else { "comments" };
    if filtered {
        format!("{shown} of {all} {}", word(all))
    } else {
        format!("{all} {}", word(all))
    }
}

/// The route's query now.
fn query_now() -> String {
    lens::query_of(&dom::window().location().hash().unwrap_or_default()).to_string()
}

/// The page.
#[component]
pub(crate) fn CommentsPage() -> impl IntoView {
    comments::want();
    let held = comments::signal();
    let filter = RwSignal::new(Filter::read(&query_now()));
    // The bar's edits rewrite the query in place — no history entry.
    Effect::new(move |_| {
        let f = filter.get();
        let w = dom::window();
        let hash = w.location().hash().unwrap_or_default();
        let next = lens::with_query(&hash, &f.query(lens::query_of(&hash)));
        if next != hash {
            if let Ok(history) = w.history() {
                let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&next));
            }
        }
    });
    // `under`: the ids the server admits, asked again as the copy grows.
    let under = RwSignal::new(None::<(String, Result<HashSet<u64>, String>)>);
    {
        let held = held.clone();
        Effect::new(move |_| {
            let Some(u) = filter.with(|f| f.under.clone()) else {
                under.set(None);
                return;
            };
            // Asked again whenever the copy moves (a comment born anywhere).
            held.track();
            spawn_local(async move {
                let answer = comments::ids_under(u.clone()).await;
                if filter.try_with_untracked(|f| f.under.as_deref() == Some(u.as_str())) == Some(true) {
                    under.try_set(Some((u, answer)));
                }
            });
        });
    }
    let under_ids = move || -> Option<HashSet<u64>> {
        let want = filter.with(|f| f.under.clone())?;
        under.with(|u| u.as_ref().filter(|(k, _)| *k == want).and_then(|(_, r)| r.as_ref().ok().cloned()))
    };
    let refused = move || {
        let want = filter.with(|f| f.under.clone())?;
        under.with(|u| u.as_ref().filter(|(k, _)| *k == want).and_then(|(_, r)| r.as_ref().err().cloned()))
    };
    let shown = {
        let held = held.clone();
        Memo::new(move |_| {
            let ids = under_ids();
            held.with(|h| h.as_ref().map(|h| (rows(h.list(), &filter.get(), ids.as_ref()), h.list().len())))
        })
    };

    let field = move |key: &'static str, label: &'static str, hint: &'static str| {
        let value = move || {
            filter.with(|f| match key {
                "ticket" => f.ticket.clone(),
                "under" => f.under.clone(),
                "author" => f.author.clone(),
                _ => f.text.clone(),
            })
            .unwrap_or_default()
        };
        view! {
            <label class="cfield">
                <span class="lbl">{label}</span>
                <input
                    type="text"
                    data-filter=key
                    placeholder=hint
                    spellcheck="false"
                    autocomplete="off"
                    prop:value=value
                    on:change=move |ev| {
                        let v = event_target_value(&ev);
                        let v = Some(v.trim().to_string()).filter(|v| !v.is_empty());
                        filter.update(|f| match key {
                            "ticket" => f.ticket = v.map(|v| v.to_ascii_uppercase()),
                            "under" => f.under = v.map(|v| v.to_ascii_uppercase()),
                            "author" => f.author = v,
                            _ => f.text = v,
                        });
                    }
                />
            </label>
        }
    };

    view! {
        <div id="comments">
            <h2>"Comments"</h2>
            <p class="lede">
                "Notes left on the app and on ticket bodies. Leave one in annotation mode — "
                <kbd>"a"</kbd>
                " or Annotate in the header."
            </p>
            <div class="cbar">
                {field("ticket", "ticket", "a ticket, e.g. DEMAND-2")}
                {field("under", "under", "a shortcode or a ticket")}
                <label class="cfield">
                    <span class="lbl">"state"</span>
                    <select
                        data-filter="state"
                        prop:value=move || filter.with(|f| f.state.map_or("", |s| s.as_str()).to_string())
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            filter.update(|f| f.state = v.parse().ok());
                        }
                    >
                        <option value="">"any"</option>
                        <option value="open">"open"</option>
                        <option value="resolved">"resolved"</option>
                    </select>
                </label>
                {field("author", "author", "e.g. owner")}
                {field("text", "text", "in the body, the label, the excerpt")}
                <button class="creset" disabled=move || filter.with(Filter::is_empty) on:click=move |_| filter.set(Filter::default())>
                    "clear"
                </button>
            </div>
            {move || refused().map(|why| view! { <p class="crefused">"The server did not take "<code>"under"</code>": "{why}</p> })}
            {move || match shown.get() {
                None => view! { <p class="loading">"loading comments…"</p> }.into_any(),
                Some((list, all)) => {
                    let pending = filter.with(|f| f.under.is_some()) && under_ids().is_none() && refused().is_none();
                    let line = count_line(list.len(), all, !filter.with(Filter::is_empty));
                    view! {
                        <p class="ccount" id="comments-count">{if pending { "…".to_string() } else { line }}</p>
                        {if list.is_empty() && !pending {
                            view! { <p class="none">{if all == 0 { "No comments yet." } else { "No comment matches." }}</p> }.into_any()
                        } else {
                            view! {
                                <ul class="crows">
                                    <For each=move || list.clone() key=|c| (c.id, c.state) children=row />
                                </ul>
                            }
                            .into_any()
                        }}
                    }
                    .into_any()
                }
            }}
        </div>
    }
}

/// One comment's row.
fn row(c: Comment) -> impl IntoView {
    let id = c.id;
    let open = c.state == CommentState::Open;
    let href = link(&c);
    let ticket = c.anchor.ticket.clone();
    let busy = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);
    let flip = move |_| {
        busy.set(true);
        spawn_local(async move {
            let r = if open { comments::resolve(id).await } else { comments::reopen(id).await };
            busy.try_set(false);
            err.try_set(r.err());
        });
    };
    view! {
        <li class="crow" data-comment-id=id.to_string() data-state=c.state.as_str()>
            <div class="chead">
                <a class="cplace" href=href title=format!("{} — {}", c.anchor.route, c.anchor.path)>{c.anchor.label.clone()}</a>
                {ticket.map(|t| view! { <a class="code" href=format!("#/ticket/{t}")>{t.clone()}</a> })}
                <span class="cstate" data-state=c.state.as_str()>{c.state.as_str()}</span>
                <span class="cwho">{c.author.clone()}</span>
                <span class="cwhen" title=fmt_unix(c.at)>{fmt_relative(now_unix(), c.at)}</span>
                <span class="cid">"#"{id}</span>
            </div>
            // MENGER-14: its bare URLs are links, followed by the app.
            <div class="cbody" data-links="comment" data-ticket=c.anchor.ticket.clone()><crate::links::Linkified text=c.body.clone() /></div>
            {(!c.anchor.excerpt.is_empty()).then(|| view! { <div class="cexcerpt">"“"{c.anchor.excerpt.clone()}"”"</div> })}
            <div class="cacts">
                <span class="croute">{c.anchor.route.clone()}</span>
                <button class="cflip" data-act=if open { "resolve" } else { "reopen" } disabled=move || busy.get() on:click=flip>
                    {if open { "Resolve" } else { "Reopen" }}
                </button>
                {move || err.get().map(|e| view! { <span class="cerr">{e}</span> })}
            </div>
        </li>
    }
}

/// The open count across the copy, once held — the header's badge.
pub(crate) fn open_count(held: &Option<Held>) -> Option<usize> {
    held.as_ref().map(Held::open_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::tests::{comment, resolved};

    /// The query names the filters; the lens's keys and unknown ones are
    /// left alone; blank, undecodable and unknown-state values are no filter;
    /// and the filters write back where the page's keys were.
    #[test]
    fn the_query_is_the_filters() {
        let f = Filter::read("under=returns&state=open");
        assert_eq!(f, Filter { under: Some("RETURNS".into()), state: Some(CommentState::Open), ..Filter::default() });
        let f = Filter::read("g=kind%20build&ticket=demand-2&author=owner&text=show%20the%20parent&n=x&zzz=1");
        assert_eq!(f.ticket.as_deref(), Some("DEMAND-2"));
        assert_eq!(f.author.as_deref(), Some("owner"), "an author as written");
        assert_eq!(f.text.as_deref(), Some("show the parent"));
        assert_eq!(f.under, None);
        assert_eq!(Filter::read("state=closed&ticket=&text=%ZZ"), Filter::default(), "none of them is a filter");
        assert_eq!(Filter::read("state=resolved&state=open").state, Some(CommentState::Open), "the last wins");
        assert!(Filter::read("").is_empty());
        // Back to a query: the page's keys in its order, the lens's kept.
        let f = Filter { text: Some("a b&c".into()), state: Some(CommentState::Resolved), ticket: Some("DEMAND-2".into()), ..Filter::default() };
        assert_eq!(f.query("g=kind%20build&text=old&f=x"), "g=kind%20build&f=x&ticket=DEMAND-2&state=resolved&text=a%20b%26c");
        assert_eq!(Filter::read(&f.query("")), f, "round trip");
        assert_eq!(Filter::default().query("g=x&under=Y"), "g=x");
        assert_eq!(ticket_link("DEMAND-2"), "#/comments?ticket=DEMAND-2&state=open");
    }

    /// Every filter as the server's `list` reads it; `under` as the server
    /// answered; newest first.
    #[test]
    fn the_filters_admit_what_the_server_would() {
        let mut a = comment(1, "#/ticket/RETURNS-1", "#t-status", Some("RETURNS-1"));
        a.body = "Show the parent's TITLE too".into();
        let b = resolved(comment(2, "#/", "#annotate", None));
        let mut c = comment(3, "#/ticket/DEMAND-2", "[data-tag=\"ticket DEMAND-1\"]", Some("DEMAND-2"));
        c.author = "agent".into();
        c.anchor.label = "Parent chip".into();
        let all = vec![a.clone(), b.clone(), c.clone()];
        let ids = |f: &Filter, under: Option<&HashSet<u64>>| rows(&all, f, under).iter().map(|c| c.id).collect::<Vec<_>>();

        assert_eq!(ids(&Filter::default(), None), [3, 2, 1], "everything, newest first");
        assert_eq!(ids(&Filter { ticket: Some("returns-1".into()), ..Filter::default() }, None), [1], "the ticket, any case");
        assert_eq!(ids(&Filter { state: Some(CommentState::Open), ..Filter::default() }, None), [3, 1]);
        assert_eq!(ids(&Filter { state: Some(CommentState::Resolved), ..Filter::default() }, None), [2]);
        assert_eq!(ids(&Filter { author: Some("agent".into()), ..Filter::default() }, None), [3]);
        assert_eq!(ids(&Filter { author: Some("Agent".into()), ..Filter::default() }, None), Vec::<u64>::new(), "an author exactly");
        assert_eq!(ids(&Filter { text: Some("title".into()), ..Filter::default() }, None), [1], "in the body, any case");
        assert_eq!(ids(&Filter { text: Some("parent chip".into()), ..Filter::default() }, None), [3], "in the label");
        assert_eq!(ids(&Filter { text: Some("excerpt 2".into()), ..Filter::default() }, None), [2], "in the excerpt");
        // `under`: nothing until the server has said, then what it said.
        let under = Filter { under: Some("RETURNS".into()), ..Filter::default() };
        assert_eq!(ids(&under, None), Vec::<u64>::new());
        let said: HashSet<u64> = [1].into();
        assert_eq!(ids(&under, Some(&said)), [1]);
        // Every filter at once.
        let all_of = Filter { under: Some("RETURNS".into()), state: Some(CommentState::Open), text: Some("parent".into()), ..Filter::default() };
        assert_eq!(ids(&all_of, Some(&said)), [1]);
        assert_eq!(ids(&Filter { state: Some(CommentState::Resolved), ..all_of }, Some(&said)), Vec::<u64>::new());
    }

    /// A row links back to its page and names itself; the count line says
    /// how many of how many.
    #[test]
    fn a_row_links_back_to_its_place() {
        let c = comment(7, "#/ticket/DEMAND-2", "#t-status", Some("DEMAND-2"));
        assert_eq!(link(&c), "#/ticket/DEMAND-2?comment=7");
        let board = comment(8, "#/", "#annotate", None);
        assert_eq!(link(&board), "#/?comment=8");
        let mut odd = board.clone();
        odd.anchor.route = "#/waves/RETURNS?status=open".into();
        assert_eq!(link(&odd), "#/waves/RETURNS?status=open&comment=8");
        assert_eq!(count_line(3, 3, false), "3 comments");
        assert_eq!(count_line(1, 1, false), "1 comment");
        assert_eq!(count_line(1, 5, true), "1 of 5 comments");
    }
}
