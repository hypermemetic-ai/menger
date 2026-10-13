//! FILTER-26: **the nav's filter made global, in one click.** While the
//! nav holds a clause the reader added ([`crate::lens::promotable`]), its
//! filter bar and its filter menu offer **Make global**, and the palette
//! `promote filter` ([`promote_command`]). Done, the global filter becomes
//! `global and nav` less the nav's default (`not menger doc`, FILTER-21) —
//! [`crate::lens::promoted`], the pure rewrite — and the nav goes back to
//! its default ([`crate::lens::Lens::promote`]): the lens bar shows the new
//! global filter, the URL carries it as `g=` and drops `n=`, and every
//! surface follows. A toast offers one **Undo**, which puts both back
//! ([`crate::lens::Lens::restore`]).
//!
//! The toast is this module's own (mounted once, [`PromoteToast`]), not the
//! ticket menu's: its Undo restores filters, not tickets.

use std::cell::Cell;

use leptos::prelude::*;

use crate::dom;
use crate::lens::{Level, Lens, Promotion};

/// How long the Undo is offered.
const TOAST_MS: i32 = 12_000;

/// The palette's words for Make global — `promote filter` (and `make
/// global`) — each with how much must be typed before it is offered, so
/// `pro` or `make` is still a search.
const WORDS: [(&str, usize); 2] = [("promote filter", 4), ("make global", 6)];

/// FILTER-26: whether what is typed in the palette asks for Make global —
/// the beginning (`prom`, `make g` or more; any case, spaces collapsed) of
/// `promote filter` or `make global`.
pub(crate) fn promote_command(query: &str) -> bool {
    let typed = query.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    WORDS.iter().any(|(w, min)| typed.len() >= *min && w.starts_with(&typed))
}

/// The toast's words: what the global filter now is.
pub(crate) fn toast_words(global: &str) -> String {
    if global.is_empty() {
        "The nav's filter is back to its default".to_string()
    } else {
        format!("Global filter: {global}")
    }
}

/// What the toast shows: its words, and what Undo puts back.
#[derive(Clone)]
struct Shown {
    text: String,
    before: Promotion,
}

thread_local! {
    /// The toast's state, once [`PromoteToast`] is mounted.
    static TOAST: Cell<Option<(RwSignal<Option<Shown>>, StoredValue<u32>)>> = const { Cell::new(None) };
}

/// FILTER-26: Make global, from the bar, the menu or the palette — the
/// nav's menu closed, the toast with its Undo shown. Nothing, while the nav
/// has nothing to move.
pub(crate) fn make_global(lens: Lens) {
    lens.pop(Level::Nav).close(false);
    let Some(before) = lens.promote() else { return };
    let text = toast_words(&lens.text(Level::Global));
    if let Some((toast, gen)) = TOAST.with(Cell::get) {
        let g = gen.get_value().wrapping_add(1);
        gen.set_value(g);
        toast.set(Some(Shown { text, before }));
        leptos::task::spawn_local(async move {
            dom::sleep_ms(TOAST_MS).await;
            if gen.get_value() == g {
                toast.try_set(None);
            }
        });
    }
}

/// FILTER-26: **Make global** — in the nav's filter bar (`menu` false) or
/// at the top of its filter menu (`menu` true); drawn while the nav holds a
/// clause the reader added.
#[component]
pub(crate) fn MakeGlobal(#[prop(optional)] menu: bool) -> impl IntoView {
    let lens = crate::lens::use_lens();
    let title = "the global filter becomes the global filter and the nav's; the nav goes back to its default";
    move || {
        lens.can_promote().then(|| {
            if menu {
                view! {
                    <div class="fpromote">
                        <button
                            type="button"
                            data-act="make-global"
                            title=title
                            on:mousedown=|ev| ev.prevent_default()
                            on:click=move |_| make_global(lens)
                        >
                            "Make global"
                        </button>
                        <span class="fpromote-hint">"every view shows what the nav shows"</span>
                    </div>
                }
                .into_any()
            } else {
                view! {
                    <button class="reset promote" type="button" data-act="make-global" title=title on:click=move |_| make_global(lens)>
                        "Make global"
                    </button>
                }
                .into_any()
            }
        })
    }
}

/// FILTER-26: Make global's toast — mounted once by the App.
#[component]
pub(crate) fn PromoteToast() -> impl IntoView {
    let lens = crate::lens::use_lens();
    let toast = RwSignal::new(None::<Shown>);
    let gen = StoredValue::new(0u32);
    TOAST.with(|t| t.set(Some((toast, gen))));
    move || {
        toast.get().map(|s| {
            let before = s.before.clone();
            view! {
                <div id="ftoast" role="status">
                    <span class="msg">{s.text}</span>
                    <button
                        data-act="undo-make-global"
                        on:click=move |_| {
                            lens.restore(before.clone());
                            toast.set(None);
                        }
                    >
                        "Undo"
                    </button>
                    <button on:click=move |_| toast.set(None)>"dismiss"</button>
                </div>
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{promote_command, toast_words};

    /// The palette's `promote filter` (or `make global`): its beginning,
    /// four letters on, any case; anything else stays a search.
    #[test]
    fn the_palette_offers_promote_filter() {
        for yes in ["promote filter", "Promote Filter", "prom", "promote", "promote  fil", " promote filter ", "make global", "make g"] {
            assert!(promote_command(yes), "{yes:?}");
        }
        for no in ["", "p", "pro", "promote filters", "promote demand", "filter", "make", "global", "PROMOTE-3"] {
            assert!(!promote_command(no), "{no:?}");
        }
    }

    #[test]
    fn the_toast_says_the_new_global_filter() {
        assert_eq!(toast_words("status Ready and kind build"), "Global filter: status Ready and kind build");
        assert_eq!(toast_words(""), "The nav's filter is back to its default");
    }
}
