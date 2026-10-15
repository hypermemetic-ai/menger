//! THEME-8: theme scopes — who wears what, painted.
//!
//! **Three regions, each its own token scope.** The site is the document's
//! root (the shell, nav, board, Home: its tokens written on `<html>`'s own
//! style); the ticket view is `#ticket-page`; a ticket's frame is the box
//! its body sits in (`.mscope-frame`, around the frame — or around a plain
//! body, which is its own frame). A region that wears the theme of the
//! region outside it says nothing and inherits; one that wears another has
//! every token of its theme written on it (a rule in `<style
//! id="menger-scopes">`), so a `default` ticket view inside an `ocean` site
//! is exactly the app's own look. A frame reads its tokens where it sits
//! (src/frame.rs), so it wears its region's theme too.
//!
//! **Who wears what** is [`menger_tickets_api::themes::resolve`] — the
//! server's own rule — over what each level says: the site's theme and
//! each shortcode's setting, kept here from `themes.list` and
//! `themes.watch` ([`refresh`]); the open ticket's frontmatter, read from
//! the body the page holds, live as it moves ([`open`]); the shortcode
//! level as `themes.theme_for` answers it (the ticket's shortcode, else the
//! nearest hosting it — asked on entry and whenever a setting moves; the
//! ticket's own shortcode stands in until it answers); and the reader's own
//! choices, outermost, kept in this browser ([`ReaderGroup`] on
//! `#/settings`).
//!
//! **One repaint path** ([`repaint`], THEME-7): every cause ends there —
//! a theme picked (settings, the palette's `theme <name>`, another tab's
//! pick heard through `storage`), a theme defined or its ```theme block
//! amended (`themes.watch`'s `theme_changed`, so an open tab follows an
//! edit live), a setting moved, a project entered or left (a ticket opened,
//! left or its frontmatter amended) and the light/dark toggle
//! (src/theme.rs), which is only its special case. It paints the three
//! regions, then — only when what is painted moved ([`paint_key`]: the
//! mode, each region's theme, its tokens, its rules and fonts) — re-themes
//! every frame from where it sits (`menger:theme`; the SDK's
//! `on('theme')` tells a component that draws in JS to redraw) and redraws
//! the diagrams: in place, no reload, no blank frame. Everything else the
//! app draws — shell, nav, board, ticket page, menus, toasts, dialogs, the
//! full-screen bar, a mockup's chrome, code — paints through the tokens
//! (THEME-2's lint holds every colour to them), so writing them is the
//! whole repaint.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use futures::StreamExt;
use leptos::prelude::*;
use menger_tickets_api::themes::{self, Declared, Levels, Reader, Regions, Theme, ThemeSetting, DEFAULT_THEME, SITE};
use menger_tickets_api::ThemesEvent;
use plexus_client::ClientEvent;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::frame::Scheme;
use crate::{dom, frame, mermaid, session};

/// The `<style>` the ticket view's and the frame's tokens are written in.
const STYLE_ID: &str = "menger-scopes";
/// THEME-3: the `<style>` the worn themes' fonts are written in.
const FONTS_ID: &str = "menger-theme-fonts";
/// The ticket view's element (TicketPage's).
pub(crate) const TICKET_SELECTOR: &str = "#ticket-page";
/// The class of the box a ticket's body — its frame — sits in.
pub(crate) const FRAME_CLASS: &str = "mscope-frame";
/// localStorage: the reader's choices.
pub(crate) const READER_KEY: &str = "menger.themes.reader";
/// localStorage: the site's tokens as last painted — index.html's pre-paint
/// script writes them on `<html>` before the wasm loads, so a reload never
/// flashes the default under a site theme.
pub(crate) const SITE_KEY: &str = "menger.themes.site";

/// The themes and the settings, as the app holds them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Held {
    themes: BTreeMap<String, Theme>,
    /// By scope: `site`, or a shortcode.
    settings: BTreeMap<String, ThemeSetting>,
}

impl Held {
    /// Apply a `list` or `watch` event; whether the copy moved.
    pub(crate) fn apply(&mut self, ev: &ThemesEvent) -> bool {
        match ev {
            ThemesEvent::Theme { theme } | ThemesEvent::ThemeChanged { theme } => {
                let moved = self.themes.get(&theme.name) != Some(theme);
                self.themes.insert(theme.name.clone(), theme.clone());
                moved
            }
            ThemesEvent::ThemeRemoved { name } => self.themes.remove(name).is_some(),
            ThemesEvent::Setting { setting } => {
                let silent = setting.said().is_silent();
                let was = self.settings.get(&setting.scope).cloned();
                if silent {
                    self.settings.remove(&setting.scope);
                } else {
                    self.settings.insert(setting.scope.clone(), setting.clone());
                }
                was.as_ref() != (!silent).then_some(setting)
            }
            _ => false,
        }
    }

    /// The site's theme, if it adopts one.
    pub(crate) fn site(&self) -> Option<String> {
        self.settings.get(SITE).and_then(|s| s.theme.clone())
    }

    /// Every theme's name, `default` first.
    pub(crate) fn names(&self) -> Vec<String> {
        std::iter::once(DEFAULT_THEME.to_string()).chain(self.themes.keys().filter(|n| *n != DEFAULT_THEME).cloned()).collect()
    }

    /// The theme `name` as it paints — THEME-3: its own overrides over its
    /// parent's, up its `extends` to `default` ([`themes::flatten`]);
    /// `default` (nothing overridden) when it is, or is not one the app holds.
    pub(crate) fn theme(&self, name: &str) -> Theme {
        if !self.themes.contains_key(name) {
            return Theme::default_theme();
        }
        themes::flatten(name, |n| self.themes.get(n))
    }

    /// THEME-3: the theme `name`'s own block as the app holds it (not flattened).
    pub(crate) fn own(&self, name: &str) -> Option<&Theme> {
        self.themes.get(name)
    }
}

/// The open ticket: what its body declares, and the shortcode level the
/// server answered (`None` until it has).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Open {
    pub(crate) id: String,
    pub(crate) epic: String,
    pub(crate) declared: Declared,
    pub(crate) shortcode: Option<Option<ThemeSetting>>,
    /// THEME-4: the shortcodes hosting `epic`, nearest first, as the app's
    /// shortcode tree (the groups' hosts) has them — what stands in for the
    /// server's answer until it lands, so entering a hosted shortcode paints
    /// its inherited theme at once, not the site's first.
    pub(crate) hosts: Vec<String>,
}

/// What every level says now: the site's, the open ticket's (none: the
/// site's alone).
pub(crate) fn levels(held: &Held, open: Option<&Open>) -> Levels {
    let site = held.site();
    match open {
        None => Levels { site, ..Default::default() },
        Some(o) => Levels {
            site,
            shortcode: o
                .shortcode
                .clone()
                .unwrap_or_else(|| std::iter::once(&o.epic).chain(&o.hosts).find_map(|sc| held.settings.get(sc).cloned())),
            ticket: o.declared.ticket.clone(),
            frame: o.declared.frame.clone(),
        },
    }
}

/// THEME-3: the custom property a region names the theme it wears with, so
/// a frame reads it where it sits (as it reads its tokens) and is handed
/// that theme's rules — its fonts and `::part`s (src/frame.rs).
pub(crate) const THEME_MARK: &str = "--menger-theme";

/// What a region wearing `theme` writes, in `scheme`: every token, then
/// the theme's name ([`THEME_MARK`]).
pub(crate) fn declarations(theme: &Theme, scheme: Scheme) -> String {
    theme
        .painted(scheme == Scheme::Dark)
        .into_iter()
        .map(|(name, value)| format!("{name}:{value}"))
        .chain(std::iter::once(format!("{THEME_MARK}:{}", theme.name)))
        .collect::<Vec<_>>()
        .join(";")
}

/// THEME-3: the CSS a frame wearing `name` takes — its imports, `@font-face`s
/// and `::part` rules, its parents' first; empty for `default` or a theme
/// the app does not hold.
pub(crate) fn rules_for(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() || name == DEFAULT_THEME {
        return String::new();
    }
    let held = HELD.with(|h| h.get_untracked());
    themes::rules_css(&held.theme(name), None)
}

/// THEME-3: the app's own document takes the fonts of every theme a region
/// wears (their imports and `@font-face`s; a `::part` rule reaches only a
/// frame's components) — `<style id="menger-theme-fonts">`.
pub(crate) fn fonts_css(held: &Held, r: &Regions) -> String {
    let mut worn: Vec<&str> = vec![&r.site.theme, &r.ticket.theme, &r.frame.theme];
    worn.dedup();
    let mut imports: Vec<String> = Vec::new();
    let mut faces: Vec<String> = Vec::new();
    for name in worn.into_iter().filter(|n| *n != DEFAULT_THEME) {
        let t = held.theme(name);
        imports.extend(t.imports.iter().filter(|u| !imports.contains(u)).cloned().collect::<Vec<_>>());
        faces.extend(t.rules.iter().filter(|r| r.starts_with("@font-face") && !faces.contains(r)).cloned().collect::<Vec<_>>());
    }
    imports.iter().map(|u| format!("@import url(\"{u}\");")).chain(faces).collect()
}

/// The scoped rules: the ticket view's, when it wears other than the site;
/// the frame's, when it wears other than the ticket view.
pub(crate) fn scoped_css(held: &Held, r: &Regions, scheme: Scheme) -> String {
    let mut css = String::new();
    if r.ticket.theme != r.site.theme {
        css.push_str(&format!("{TICKET_SELECTOR}{{{}}}", declarations(&held.theme(&r.ticket.theme), scheme)));
    }
    if r.frame.theme != r.ticket.theme {
        css.push_str(&format!(".{FRAME_CLASS}{{{}}}", declarations(&held.theme(&r.frame.theme), scheme)));
    }
    css
}

/// The reader's choices as stored: `{"site": "mint" | null, "takeovers_off":
/// bool, "projects_off": bool}` (THEME-4: a record without it is "on").
pub(crate) fn reader_of(stored: Option<&str>) -> Reader {
    let Some(v) = stored.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) else { return Reader::default() };
    Reader {
        site: v["site"].as_str().and_then(|s| themes::name_of(s).ok()),
        takeovers_off: v["takeovers_off"].as_bool().unwrap_or(false),
        projects_off: v["projects_off"].as_bool().unwrap_or(false),
    }
}

pub(crate) fn reader_text(r: &Reader) -> String {
    serde_json::json!({ "site": r.site, "takeovers_off": r.takeovers_off, "projects_off": r.projects_off }).to_string()
}

thread_local! {
    static HELD: ArcRwSignal<Held> = ArcRwSignal::new(Held::default());
    static OPEN: ArcRwSignal<Option<Open>> = ArcRwSignal::new(None);
    static READER: ArcRwSignal<Reader> = ArcRwSignal::new(load_reader());
    /// Moves whenever a setting does: the open ticket asks `theme_for` again.
    static EPOCH: ArcRwSignal<u32> = ArcRwSignal::new(0);
    /// The newest `theme_for` asked; an older answer is dropped.
    static ASKED: Cell<u32> = const { Cell::new(0) };
    /// Whether a `list` has landed: until one has, nothing is painted over
    /// what index.html's pre-paint script put on the root.
    static LOADED: Cell<bool> = const { Cell::new(false) };
    /// What was painted last — nothing moves when nothing changed.
    static PAINTED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The watch's pump on this connection — ended when a new one starts.
    static PUMP: RefCell<Option<futures::future::AbortHandle>> = const { RefCell::new(None) };
}

fn storage() -> Option<web_sys::Storage> {
    dom::window().local_storage().ok().flatten()
}

fn load_reader() -> Reader {
    reader_of(storage().and_then(|s| s.get_item(READER_KEY).ok().flatten()).as_deref())
}

/// The reader's choices, as a signal.
pub(crate) fn reader() -> ArcRwSignal<Reader> {
    READER.with(Clone::clone)
}

/// The themes the app holds, as a signal.
pub(crate) fn held() -> ArcRwSignal<Held> {
    HELD.with(Clone::clone)
}

/// The reader chose: kept in this browser, painted at once.
pub(crate) fn set_reader(r: Reader) {
    if let Some(s) = storage() {
        let _ = s.set_item(READER_KEY, &reader_text(&r));
    }
    READER.with(|x| x.set(r));
}

/// Who wears what now.
pub(crate) fn regions() -> Regions {
    let held = HELD.with(|h| h.get_untracked());
    let open = OPEN.with(|o| o.get_untracked());
    let reader = READER.with(|r| r.get_untracked());
    themes::resolve(&levels(&held, open.as_ref()), &reader)
}

/// Mounted once (App): every change of what a level says repaints — and
/// (THEME-7) a reader's pick in another tab of this browser reaches this
/// one through `storage`, so every open page follows it.
pub(crate) fn install() {
    let (held, open, reader) = (HELD.with(Clone::clone), OPEN.with(Clone::clone), READER.with(Clone::clone));
    Effect::new(move |_| {
        held.track();
        open.track();
        reader.track();
        repaint();
    });
    let cb = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::StorageEvent)>::new(move |ev: web_sys::StorageEvent| {
        if ev.key().as_deref() == Some(READER_KEY) { // bindings-lint: a StorageEvent's key
            let next = load_reader();
            READER.with(|r| {
                if r.with_untracked(|r| *r != next) {
                    r.set(next);
                }
            });
        }
    });
    let _ = dom::window().add_event_listener_with_callback("storage", cb.as_ref().unchecked_ref());
    cb.forget(); // lives as long as the app
}

/// THEME-7: the reader picks `name` as their site theme (`None`: the
/// site's) — from settings or the palette; kept, and painted at once.
pub(crate) fn pick(name: Option<String>) {
    let mut r = READER.with(|r| r.get_untracked());
    r.site = name;
    set_reader(r);
}

/// THEME-7: what a repaint paints, as one string — the mode, the site's
/// tokens, the scoped rules, the fonts, each region's theme and the rules a
/// frame wearing it is handed. Equal: nothing moved, nothing is done.
pub(crate) fn paint_key(held: &Held, r: &Regions, scheme: Scheme, site_css: &str, scoped: &str, fonts: &str) -> String {
    let rules: String = [&r.site.theme, &r.ticket.theme, &r.frame.theme]
        .into_iter()
        .filter(|n| n.as_str() != DEFAULT_THEME)
        .map(|n| themes::rules_css(&held.theme(n), None))
        .collect::<Vec<_>>()
        .join("|");
    format!("{}|{site_css}|{scoped}|{fonts}|{}|{}|{}|{rules}", scheme.as_str(), r.site.theme, r.ticket.theme, r.frame.theme)
}

/// THEME-7's one repaint path (THEME-8 began it). Paints the three regions
/// for the mode painted now; when anything moved, every frame takes its
/// region's tokens and the diagrams are drawn again. Nothing here can throw.
pub(crate) fn repaint() {
    if !LOADED.with(Cell::get) {
        return;
    }
    let w = dom::window();
    let Some(doc) = w.document() else { return };
    let Some(root) = doc.document_element() else { return };
    let held = HELD.with(|h| h.get_untracked());
    let r = regions();
    let scheme = frame::painted_scheme();
    let site = held.theme(&r.site.theme);
    let site_css = if r.site.theme == DEFAULT_THEME { String::new() } else { declarations(&site, scheme) };
    let scoped = scoped_css(&held, &r, scheme);
    let fonts = fonts_css(&held, &r);
    let key = paint_key(&held, &r, scheme, &site_css, &scoped, &fonts);
    if PAINTED.with(|p| p.borrow().as_deref() == Some(key.as_str())) {
        return;
    }
    let first = PAINTED.with(|p| p.replace(Some(key)).is_none());
    // The site: its tokens on the root's own style (nothing for default).
    if let Some(el) = root.dyn_ref::<web_sys::HtmlElement>() {
        let style = el.style();
        for name in frame::theme_tokens().chain(std::iter::once(THEME_MARK)) {
            let _ = style.remove_property(name);
        }
        for decl in site_css.split(';').filter(|d| !d.is_empty()) {
            if let Some((name, value)) = decl.split_once(':') {
                let _ = style.set_property(name, value);
            }
        }
    }
    if let Some(s) = storage() {
        let _ = if site_css.is_empty() { s.remove_item(SITE_KEY) } else { s.set_item(SITE_KEY, &site_css) };
    }
    // The ticket view and the frame: one stylesheet, whatever is mounted.
    let style = doc.get_element_by_id(STYLE_ID).or_else(|| {
        let el = doc.create_element("style").ok()?;
        el.set_id(STYLE_ID);
        doc.head()?.append_child(&el).ok()?;
        Some(el)
    });
    if let Some(style) = style {
        style.set_text_content(Some(&scoped));
    }
    // THEME-3: the fonts the worn themes bring, in the app's own document.
    let fonts_el = doc.get_element_by_id(FONTS_ID).or_else(|| {
        let el = doc.create_element("style").ok()?;
        el.set_id(FONTS_ID);
        doc.head()?.append_child(&el).ok()?;
        Some(el)
    });
    if let Some(el) = fonts_el {
        el.set_text_content(Some(&fonts));
    }
    // What each region wears, said on the root (for a reader's tools, and
    // the headless checks).
    for (attr, worn) in [("data-theme-site", &r.site), ("data-theme-ticket", &r.ticket), ("data-theme-frame", &r.frame)] {
        let _ = root.set_attribute(attr, &format!("{} {}", worn.theme, worn.level.as_str()));
    }
    if !first {
        frame::retheme_all();
        mermaid::rerender_all();
    }
}

/// The ticket `id`'s page is open, its body `body`: its frontmatter read as
/// it moves, the shortcode level asked of the server on entry, on every
/// moved setting and on every new connection. Leaving it puts the site's
/// back.
pub(crate) fn open(id: String, body: RwSignal<Option<String>>, socket: RwSignal<u32>) {
    let epic = crate::bridge::shortcode_of(&id);
    let open = OPEN.with(Clone::clone);
    // THEME-4: the hosting chain from the shortcode tree the app keeps.
    let sctree = use_context::<crate::lens::Lens>().map(|l| l.sctree());
    let next = move |open: &ArcRwSignal<Option<Open>>, id: &String, epic: &String, tracked: bool| {
        let declared = if tracked { body.with(|b| b.as_deref().map(themes::declared)) } else { body.with_untracked(|b| b.as_deref().map(themes::declared)) }.unwrap_or_default();
        let hosts_now = |t: &Option<crate::filter::ScTree>| t.as_ref().map(|t| hosts_of(&t.hosting(epic)));
        let hosts = sctree.and_then(|t| if tracked { t.with(hosts_now) } else { t.with_untracked(hosts_now) }).unwrap_or_default();
        let next = match open.get_untracked() {
            Some(o) if o.id == *id => Open { declared, hosts, ..o },
            _ => Open { id: id.clone(), epic: epic.clone(), declared, shortcode: None, hosts },
        };
        if open.with_untracked(|o| o.as_ref() != Some(&next)) {
            open.set(Some(next));
        }
    };
    // THEME-4: said at once, not in an effect — going from one ticket to
    // another, the page left may clear the open ticket before this one's
    // effects run, and the repaint would see none between them (a detour
    // by the site's theme). Set here, in the same turn, it sees only this.
    next(&open, &id, &epic, false);
    {
        let (id, open, epic) = (id.clone(), open.clone(), epic.clone());
        Effect::new(move |_| next(&open, &id, &epic, true));
    }
    {
        let id = id.clone();
        let epoch = EPOCH.with(Clone::clone);
        Effect::new(move |_| {
            epoch.track();
            if socket.get() > 0 {
                ask(id.clone());
            }
        });
    }
    on_cleanup(move || {
        if open.with_untracked(|o| o.as_ref().is_some_and(|o| o.id == id)) {
            open.set(None);
        }
    });
}

/// THEME-4: the shortcodes of a hosting chain (`ScTree::hosting`'s host
/// ticket, then its shortcode, up the chain), nearest first.
pub(crate) fn hosts_of(chain: &[String]) -> Vec<String> {
    chain.iter().skip(1).step_by(2).cloned().collect()
}

/// `themes.theme_for { id }`: the shortcode level the server reads (the
/// nearest shortcode that says anything, through the hosting tree).
fn ask(id: String) {
    let Some(client) = session::themes_client() else { return };
    let n = ASKED.with(|a| {
        let n = a.get().wrapping_add(1);
        a.set(n);
        n
    });
    spawn_local(async move {
        let Ok(ticket) = crate::typed::typed(&id) else { return };
        let Ok(events) = client.theme_for_all(ticket).await else { return };
        if ASKED.with(Cell::get) != n {
            return;
        }
        for ev in events {
            if let ThemesEvent::Regions { levels, .. } = ev {
                OPEN.with(|o| {
                    o.maybe_update(|open| match open {
                        Some(open) if open.id.eq_ignore_ascii_case(&id) && open.shortcode.as_ref() != Some(&levels.shortcode) => {
                            open.shortcode = Some(levels.shortcode.clone());
                            true
                        }
                        _ => false,
                    })
                });
            }
        }
    });
}

/// A new connection: `themes.watch` subscribed on it, then `themes.list`.
pub(crate) fn refresh() {
    let Some(client) = session::themes_client() else { return };
    let (abort, reg) = futures::future::AbortHandle::new_pair();
    if let Some(old) = PUMP.with(|p| p.borrow_mut().replace(abort)) {
        old.abort();
    }
    let pump = async move {
        let mut stream = client.watch();
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(ClientEvent::Data(ThemesEvent::Lagged { .. })) => fetch(),
                Ok(ClientEvent::Data(ev)) => heard(&ev),
                Ok(_) => {}
                Err(_) => break,
            }
        }
    };
    spawn_local(async move {
        let _ = futures::future::Abortable::new(pump, reg).await;
    });
    fetch();
}

fn heard(ev: &ThemesEvent) {
    HELD.with(|h| h.maybe_update(|held| held.apply(ev)));
    if matches!(ev, ThemesEvent::Setting { .. }) {
        EPOCH.with(|e| e.update(|n| *n = n.wrapping_add(1)));
    }
}

fn fetch() {
    let Some(client) = session::themes_client() else { return };
    spawn_local(async move {
        match client.list_all().await {
            Ok(events) => {
                let mut held = Held::default();
                for ev in &events {
                    match ev {
                        ThemesEvent::Error { message } => leptos::logging::log!("themes: {message}"),
                        ev => {
                            held.apply(ev);
                        }
                    }
                }
                LOADED.with(|l| l.set(true));
                HELD.with(|h| h.set(held));
                // The same copy paints nothing new on its own: say so.
                repaint();
            }
            Err(e) => leptos::logging::log!("themes: {}", session::describe(&e)),
        }
    });
}

/// `#/settings`: the reader's own — their site theme, project themes off
/// (THEME-4), and every takeover off.
#[component]
pub(crate) fn ReaderGroup() -> impl IntoView {
    let reader = reader();
    let held = held();
    let names = {
        let held = held.clone();
        move || held.with(Held::names)
    };
    let (r1, r2, r3, r4) = (reader.clone(), reader.clone(), reader.clone(), reader.clone());
    view! {
        <fieldset class="group" id="settings-themes">
            <legend>"Themes"</legend>
            <p class="now">
                "Yours, in this browser — above the site's, a shortcode's and a ticket's (THEME-8)."
            </p>
            <div class="field">
                <label for="theme-reader-site">"Your site theme"</label>
                <div class="control">
                    <select
                        id="theme-reader-site"
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            let mut r = r1.get_untracked();
                            r.site = (!v.is_empty()).then_some(v);
                            set_reader(r);
                        }
                    >
                        <option value="" selected=move || r2.with(|r| r.site.is_none())>"the site's"</option>
                        {move || {
                            let r3 = r3.clone();
                            names()
                                .into_iter()
                                .map(|n| {
                                    let (v, label, me) = (n.clone(), n.clone(), n);
                                    let r = r3.clone();
                                    view! { <option value=v selected=move || r.with(|r| r.site.as_deref() == Some(me.as_str()))>{label}</option> }
                                })
                                .collect_view()
                        }}
                    </select>
                </div>
            </div>
            <div class="field">
                <label for="theme-projects-off">"Project themes off"</label>
                <div class="control">
                    <input
                        id="theme-projects-off"
                        type="checkbox"
                        prop:checked=move || r4.with(|r| r.projects_off)
                        on:change=move |ev| {
                            let checked = event_target_checked(&ev);
                            let mut r = load_reader();
                            r.projects_off = checked;
                            set_reader(r);
                        }
                    />
                    <span class="hint">" a project (a shortcode) never restyles menger for you — the board and every ticket wear your site theme, or the site's"</span>
                </div>
            </div>
            <div class="field">
                <label for="theme-takeovers-off">"Every takeover off"</label>
                <div class="control">
                    <input
                        id="theme-takeovers-off"
                        type="checkbox"
                        prop:checked=move || reader.with(|r| r.takeovers_off)
                        on:change=move |ev| {
                            let checked = event_target_checked(&ev);
                            let mut r = load_reader();
                            r.takeovers_off = checked;
                            set_reader(r);
                        }
                    />
                    <span class="hint">" wear your site theme (or default) everywhere, whatever a site, shortcode, ticket or frame adopts"</span>
                </div>
            </div>
        </fieldset>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::themes::{Said, Takeover, ThemeReach};

    fn theme(name: &str, accent: &str) -> Theme {
        let mut t = Theme { name: name.into(), ..Default::default() };
        t.light.insert("--accent".into(), accent.into());
        t
    }

    fn held() -> Held {
        let mut h = Held::default();
        h.apply(&ThemesEvent::Theme { theme: theme("ocean", "#00796b") });
        h.apply(&ThemesEvent::Theme { theme: theme("pantry", "#d4572a") });
        h
    }

    fn setting(scope: &str, theme: Option<&str>) -> ThemeSetting {
        ThemeSetting { scope: scope.into(), theme: theme.map(str::to_string), reach: ThemeReach::Ticket, takeover: Takeover::On }
    }

    /// The copy takes whole themes and settings; a setting that says
    /// nothing is gone.
    /// THEME-6: a face, a size or an ink inherits as a value, resolved where
    /// it is declared — on body, with the site's tokens. The ticket view
    /// and a frame's box say them again where their own tokens are, so a
    /// theme either wears alone (reach: ticket, frame-theme) sets its text too.
    #[test]
    fn the_ticket_view_resolves_its_own_text() {
        let html = include_str!("../index.html");
        let rule = html.lines().find(|l| l.trim_start().starts_with(&format!("{TICKET_SELECTOR}, .{FRAME_CLASS} {{"))).expect("the regions' rule");
        for want in ["var(--font-text)", "var(--text-size)", "var(--text-leading)", "color: var(--ink)", "background: var(--bg)"] {
            assert!(rule.contains(want), "{TICKET_SELECTOR} and .{FRAME_CLASS} must say {want} themselves: {rule}");
        }
    }

    #[test]
    fn the_copy_takes_themes_and_settings() {
        let mut h = held();
        assert_eq!(h.names(), ["default", "ocean", "pantry"]);
        assert!(h.apply(&ThemesEvent::Setting { setting: setting("site", Some("ocean")) }));
        assert!(!h.apply(&ThemesEvent::Setting { setting: setting("site", Some("ocean")) }), "held as it is");
        assert_eq!(h.site().as_deref(), Some("ocean"));
        assert!(h.apply(&ThemesEvent::Setting { setting: setting("site", None) }));
        assert_eq!(h.site(), None);
        assert_eq!(h.theme("nope"), Theme::default_theme(), "a theme the app does not hold paints default");
    }

    /// The open ticket's levels: its frontmatter, its shortcode's setting
    /// until the server answers with the nearest one.
    #[test]
    fn the_open_tickets_levels() {
        let mut h = held();
        h.apply(&ThemesEvent::Setting { setting: setting("site", Some("ocean")) });
        h.apply(&ThemesEvent::Setting { setting: setting("PANTRY", Some("pantry")) });
        let mut open = Open { id: "PANTRY-2".into(), epic: "PANTRY".into(), declared: themes::declared("---\nframe-takeover: off\n---\n"), shortcode: None, hosts: vec![] };
        let l = levels(&h, Some(&open));
        assert_eq!((l.site.as_deref(), l.shortcode.as_ref().and_then(|s| s.theme.as_deref())), (Some("ocean"), Some("pantry")));
        let r = themes::resolve(&l, &Reader::default());
        assert_eq!([r.site.theme.as_str(), r.ticket.theme.as_str(), r.frame.theme.as_str()], ["ocean", "pantry", "default"]);
        // The server says no shortcode says anything (none for this one).
        open.shortcode = Some(None);
        assert_eq!(levels(&h, Some(&open)).shortcode, None);
        assert_eq!(levels(&h, None), Levels { site: Some("ocean".into()), ..Default::default() });
        assert_eq!(open.declared.frame, Said { theme: None, reach: ThemeReach::Frame, takeover: Takeover::Off });
        // THEME-4: a hosted shortcode, before the server answers, takes the
        // nearest host's setting from the tree the app keeps.
        let hosted = Open { id: "LARDER-3".into(), epic: "LARDER".into(), declared: Declared::default(), shortcode: None, hosts: hosts_of(&["PANTRY-1".into(), "PANTRY".into(), "HOME-1".into(), "HOME".into()]) };
        assert_eq!(hosted.hosts, ["PANTRY", "HOME"]);
        assert_eq!(levels(&h, Some(&hosted)).shortcode.map(|s| s.scope), Some("PANTRY".into()));
        let mut own = hosted.clone();
        h.apply(&ThemesEvent::Setting { setting: setting("LARDER", Some("ocean")) });
        assert_eq!(levels(&h, Some(&own)).shortcode.map(|s| s.scope), Some("LARDER".into()), "its own setting wins");
        own.shortcode = Some(None);
        assert_eq!(levels(&h, Some(&own)).shortcode, None, "the server's answer, once it lands");
    }

    /// A region wearing its outer region's theme writes nothing; another
    /// writes every token of its own, in the mode painted.
    #[test]
    fn a_region_writes_its_tokens_only_when_it_differs() {
        let h = held();
        let l = |fm: &str| {
            let d = themes::declared(&format!("---\n{fm}\n---\n"));
            Levels { site: Some("ocean".into()), shortcode: None, ticket: d.ticket, frame: d.frame }
        };
        let r = themes::resolve(&l(""), &Reader::default());
        assert_eq!(scoped_css(&h, &r, Scheme::Light), "");
        let r = themes::resolve(&l("takeover: off"), &Reader::default());
        let css = scoped_css(&h, &r, Scheme::Light);
        assert!(css.starts_with("#ticket-page{--bg:#fbfbf9;"), "{css}");
        assert!(css.contains("--accent:#0066cc"), "default over ocean: {css}");
        assert!(!css.contains(".mscope-frame"), "{css}");
        let r = themes::resolve(&l("theme: pantry\nframe-takeover: off"), &Reader::default());
        let css = scoped_css(&h, &r, Scheme::Dark);
        assert!(css.contains("#ticket-page{--bg:#101014;") && css.contains("--accent:#8ab4f8"), "pantry sets accent for light only: {css}");
        assert!(css.contains(".mscope-frame{--bg:#101014"), "{css}");
        // Every token, then THEME-3's mark naming the theme.
        let decl = declarations(&h.theme("ocean"), Scheme::Light);
        assert_eq!(decl.matches(';').count() + 1, themes::TOKENS.len() + 1);
        assert!(decl.ends_with(";--menger-theme:ocean"), "{decl}");
    }

    /// The reader's record round-trips; anything unreadable is no choice.
    #[test]
    fn the_readers_choices_round_trip() {
        let r = Reader { site: Some("mint".into()), takeovers_off: true, projects_off: false };
        assert_eq!(reader_of(Some(&reader_text(&r))), r);
        let r = Reader { site: None, takeovers_off: false, projects_off: true };
        assert_eq!(reader_of(Some(&reader_text(&r))), r, "THEME-4: project themes off");
        assert_eq!(reader_of(Some(r#"{"site": "mint", "takeovers_off": false}"#)).projects_off, false, "a record from before THEME-4: on");
        assert_eq!(reader_of(Some("{")), Reader::default());
        assert_eq!(reader_of(Some(r#"{"site": "Not A Name"}"#)), Reader::default());
        assert_eq!(reader_of(None), Reader::default());
    }

    /// index.html's pre-paint script writes the site's tokens this module
    /// keeps, under the same key, guarded.
    #[test]
    fn the_pre_paint_script_writes_the_site_tokens_kept() {
        let html = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/index.html")).unwrap();
        let line = html.lines().find(|l| l.contains("<script>") && l.contains(SITE_KEY)).expect("the pre-paint script");
        assert!(line.contains(&format!("try{{var s=localStorage.getItem(\"{SITE_KEY}\");")), "{line}");
        assert!(line.contains("style.cssText=s}catch(e){}"), "{line}");
    }

    /// THEME-7: what a repaint compares — the mode moves it even when every
    /// region wears default (the toggle re-themes frames and redraws
    /// diagrams), and so does a theme's rule, its tokens unchanged (an
    /// amended `::part` reaches every frame wearing it).
    #[test]
    fn a_repaint_moves_on_the_mode_the_tokens_and_the_rules() {
        let mut h = held();
        let r = themes::resolve(&Levels::default(), &Reader::default());
        let key = |h: &Held, r: &Regions, s: Scheme| {
            let site = if r.site.theme == DEFAULT_THEME { String::new() } else { declarations(&h.theme(&r.site.theme), s) };
            paint_key(h, r, s, &site, &scoped_css(h, r, s), &fonts_css(h, r))
        };
        assert_ne!(key(&h, &r, Scheme::Light), key(&h, &r, Scheme::Dark), "default everywhere, the mode alone");
        let r = themes::resolve(&Levels { site: Some("pantry".into()), ..Default::default() }, &Reader::default());
        let before = key(&h, &r, Scheme::Light);
        assert_eq!(before, key(&h, &r, Scheme::Light), "nothing moved");
        let mut p = h.own("pantry").cloned().unwrap();
        p.rules = vec!["m-ring::part(ring){outline:2px solid var(--accent);}".into()];
        assert!(h.apply(&ThemesEvent::ThemeChanged { theme: p.clone() }));
        assert_ne!(before, key(&h, &r, Scheme::Light), "a rule alone moves it");
        let mid = key(&h, &r, Scheme::Light);
        p.light.insert("--accent".into(), "#c2185b".into());
        assert!(h.apply(&ThemesEvent::ThemeChanged { theme: p }));
        assert_ne!(mid, key(&h, &r, Scheme::Light), "an amended token moves it");
        // The reader's pick is a region's theme like any other.
        let picked = themes::resolve(&Levels::default(), &Reader { site: Some("ocean".into()), takeovers_off: false, projects_off: false });
        assert_eq!(picked.site.theme, "ocean");
    }

    /// THEME-3: the watch's theme_changed and theme_removed move the copy; a
    /// theme paints flattened over what it extends; the worn themes' fonts
    /// reach the app's document, their ::parts do not.
    #[test]
    fn blocks_arrive_flattened_and_bring_their_fonts() {
        let mut h = Held::default();
        let mut base = theme("base", "#111111");
        base.light.insert("--ink".into(), "#222222".into());
        base.rules = vec!["@font-face{font-family:\"Fraunces\";src:url(https://x.example/f.woff2);}".into(), "m-code::part(frame){border-radius:14px;}".into()];
        base.imports = vec!["https://fonts.example/a.css".into()];
        let mut top = theme("top", "#333333");
        top.extends = Some("base".into());
        assert!(h.apply(&ThemesEvent::ThemeChanged { theme: base }));
        assert!(h.apply(&ThemesEvent::ThemeChanged { theme: top.clone() }));
        assert!(!h.apply(&ThemesEvent::ThemeChanged { theme: top }), "the same theme again moves nothing");
        let flat = h.theme("top");
        assert_eq!(flat.light.get("--accent").map(String::as_str), Some("#333333"));
        assert_eq!(flat.light.get("--ink").map(String::as_str), Some("#222222"), "from base");
        let r = themes::resolve(&Levels { site: Some("top".into()), ..Default::default() }, &Reader::default());
        let fonts = fonts_css(&h, &r);
        assert_eq!(fonts, "@import url(\"https://fonts.example/a.css\");@font-face{font-family:\"Fraunces\";src:url(https://x.example/f.woff2);}");
        assert!(themes::rules_css(&h.theme("top"), None).contains("m-code::part(frame)"), "a frame takes the parts");
        assert!(h.apply(&ThemesEvent::ThemeRemoved { name: "base".into() }));
        assert!(!h.theme("top").light.contains_key("--ink"), "its parent gone, the chain ends at default");
        assert!(!h.apply(&ThemesEvent::ThemeRemoved { name: "base".into() }));
    }

}
