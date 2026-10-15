//! ORG-4: the organizations the reader holds a role in, and the header's
//! switcher between them.
//!
//! - **What is held**: `orgs.list` on every connection ([`refresh`]) — the
//!   organizations the reader holds a role in (every one for a role on
//!   `*`, and with login off).
//! - **The switcher** ([`OrgSwitcher`]) shows in the header only when the
//!   reader holds more than one. Choosing one reads every page within it:
//!   the lens ANDs `under <org>:` into every level's effective filter
//!   ([`crate::lens::Lens::org`]), so the board, the nav, the wave boards,
//!   the inputs page and the palette answer from that organization alone.
//!   "All" reads every one held.
//! - **The URL carries it**: `o=<org>` in the route's query, carried to
//!   every route the app moves to, as `g=` is; kept per viewer in storage.
//!   A URL naming an org the reader does not hold reads every one.
//!
//! What decides — which org a URL or a stored choice means, the filter it
//! adds — is pure here, pinned by native tests.

use leptos::prelude::*;
use menger_tickets_api::filter::Filter;
use menger_tickets_api::{Org, OrgsEvent};

thread_local! {
    /// The organizations the reader holds a role in — empty until the first
    /// answer.
    static HELD: ArcRwSignal<Vec<Org>> = ArcRwSignal::new(Vec::new());
}

/// The organizations held, as a signal.
pub(crate) fn held() -> ArcRwSignal<Vec<Org>> {
    HELD.with(Clone::clone)
}

/// The query key the chosen organization rides under.
pub(crate) const KEY: &str = "o";

/// Where the chosen organization is kept per viewer.
pub(crate) const STORAGE_KEY: &str = "menger.org";

/// Asks `orgs.list` on the session's connection (every connect): the
/// switcher's organizations.
pub(crate) fn refresh() {
    let Some(client) = crate::session::orgs_client() else { return };
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(events) = client.list_all().await {
            let orgs: Vec<Org> = events
                .into_iter()
                .filter_map(|e| match e {
                    OrgsEvent::Org { org } => Some(org),
                    _ => None,
                })
                .collect();
            held().set(orgs);
        }
    });
}

/// The organization `chosen` means among `orgs` — its name now (an old
/// name, an alias, is that one) — when the reader holds more than one and
/// it is one of them; `None`: every one held.
pub(crate) fn chosen_of(chosen: Option<&str>, orgs: &[Org]) -> Option<String> {
    let c = chosen.map(str::trim).filter(|c| !c.is_empty())?;
    if orgs.len() < 2 {
        return None;
    }
    orgs.iter().find(|o| o.name.as_str() == c || o.aliases.iter().any(|a| a == c)).map(|o| o.name.to_string())
}

/// `f` read within the organization `org` — `under org: and f` — or `f`
/// as it is.
pub(crate) fn within(org: Option<&str>, f: &Filter) -> Filter {
    match org.and_then(|o| Filter::parse(&format!("under {o}:")).ok()) {
        Some(o) => o.and(f),
        None => f.clone(),
    }
}

/// ORG-4: the header's organization switcher — only when the reader holds
/// more than one: "all", then each by name.
#[component]
pub(crate) fn OrgSwitcher() -> impl IntoView {
    let lens = crate::lens::use_lens();
    let orgs = held();
    let shown = {
        let orgs = orgs.clone();
        Memo::new(move |_| orgs.with(|o| o.len() > 1))
    };
    move || {
        shown.get().then(|| {
            let names: Vec<String> = orgs.with(|o| o.iter().map(|o| o.name.to_string()).collect());
            let now = lens.org().get();
            view! {
                <select
                    class="org-switch"
                    title="the organization every page reads within"
                    aria-label="organization"
                    on:change=move |ev| {
                        let v = event_target_value(&ev);
                        lens.set_org((!v.is_empty()).then_some(v));
                    }
                >
                    <option value="" selected=now.is_none()>"all orgs"</option>
                    {names
                        .into_iter()
                        .map(|n| {
                            let selected = now.as_deref() == Some(n.as_str());
                            view! { <option value=n.clone() selected=selected>{n.clone()}</option> }
                        })
                        .collect_view()}
                </select>
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::OrgId;

    fn org(name: &str, aliases: &[&str]) -> Org {
        Org {
            name: OrgId::new(name).unwrap(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
            roots: Vec::new(),
            shortcodes: 0,
            tickets: 0,
        }
    }

    #[test]
    fn a_choice_is_an_org_the_reader_holds_among_several() {
        let two = [org("hypermemetic", &[]), org("acme", &["acme-old"])];
        assert_eq!(chosen_of(Some("acme"), &two).as_deref(), Some("acme"));
        assert_eq!(chosen_of(Some("acme-old"), &two).as_deref(), Some("acme"), "an old name is the org");
        assert_eq!(chosen_of(Some("beta"), &two), None, "one not held reads every one");
        assert_eq!(chosen_of(None, &two), None);
        assert_eq!(chosen_of(Some("acme"), &two[1..]), None, "one org: nothing to choose");
    }

    #[test]
    fn a_page_reads_within_the_org() {
        let f = Filter::parse("status Ready").unwrap();
        assert_eq!(within(Some("acme"), &f).format(), "under ACME: and status Ready");
        assert_eq!(within(None, &f).format(), "status Ready");
        assert_eq!(within(Some("acme"), &Filter::default()).format(), "under ACME:");
    }
}
