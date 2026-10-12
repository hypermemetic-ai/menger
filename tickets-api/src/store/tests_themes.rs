//! THEME-8: the themes store — a theme defined and redefined, its refusals;
//! what the site and a shortcode say, and theirs; `theme_for` on every row
//! of "who wears what", the shortcode between, a hosted shortcode
//! inheriting; the channel; and all of it across a reopen.

use super::*;
use crate::themes::{Takeover, ThemeLevel, ThemeReach, ThemesEvent};

fn ticket(epic: &str, kind: Kind, body: &str, parent: Option<&str>) -> NewTicket {
    NewTicket {
        path: None,
        epic: epic.into(),
        title: format!("a {epic} ticket"),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent: parent.map(str::to_string),
        body: body.into(),
    }
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

async fn two_themes(store: &Store) {
    store.define_theme("ocean", map(&[("--accent", "#00796b"), ("bg", "#f3f8f8")]), map(&[("--accent", "#4db6ac")])).await.unwrap();
    store.define_theme("pantry", map(&[("--accent", "#d4572a"), ("--sans", "Georgia, serif")]), BTreeMap::new()).await.unwrap();
}

async fn regions(store: &Store, id: &str) -> [(String, ThemeLevel); 3] {
    let (_, r, _) = store.theme_for(id).await.unwrap();
    [(r.site.theme, r.site.level), (r.ticket.theme, r.ticket.level), (r.frame.theme, r.frame.level)]
}

fn w(theme: &str, level: ThemeLevel) -> (String, ThemeLevel) {
    (theme.to_string(), level)
}

#[tokio::test]
async fn a_theme_is_defined_redefined_and_refused_when_it_is_not_one() {
    let store = Store::new();
    two_themes(&store).await;
    let ocean = store.theme("Ocean").unwrap();
    assert_eq!(ocean.light, map(&[("--accent", "#00796b"), ("--bg", "#f3f8f8")]), "a token spelled without -- is canonical");
    assert!(ocean.updated_unix > 0);
    assert_eq!(store.themes().iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["default", "ocean", "pantry"]);
    assert!(store.theme("default").unwrap().light.is_empty());
    // Redefined whole.
    store.define_theme("ocean", map(&[("--ink", "#012")]), BTreeMap::new()).await.unwrap();
    assert_eq!(store.theme("ocean").unwrap().light, map(&[("--ink", "#012")]));
    assert!(store.theme("ocean").unwrap().dark.is_empty());
    // Refusals keep nothing.
    for (name, light, why) in [
        ("default", map(&[]), "cannot be defined"),
        ("Not A Name", map(&[]), "is not a theme name"),
        ("mint", map(&[("--nope", "#000")]), "`--nope` is not a token"),
        ("mint", map(&[("--ink", "red}")]), "`--ink`'s value"),
    ] {
        let err = store.define_theme(name, light, BTreeMap::new()).await.unwrap_err().to_string();
        assert!(err.contains(why), "{name}: {err}");
    }
    assert!(store.theme("mint").unwrap_err().to_string().contains("no theme `mint` (defined: default, ocean, pantry)"));
}

#[tokio::test]
async fn the_site_and_a_shortcode_say_what_they_adopt() {
    let store = Store::new();
    two_themes(&store).await;
    store.create(ticket("PANTRY", Kind::Scope, "", None)).await.unwrap();
    let mut rx = store.subscribe_themes();
    let s = store.set_theme("Site", Some("ocean"), None, None).await.unwrap();
    assert_eq!((s.scope.as_str(), s.theme.as_deref()), ("site", Some("ocean")));
    assert_eq!(rx.recv().await.unwrap(), ThemeChange::Set(s.clone()));
    let p = store.set_theme("pantry", Some("pantry"), Some(ThemeReach::Site), None).await.unwrap();
    assert_eq!((p.scope.as_str(), p.reach), ("PANTRY", ThemeReach::Site));
    assert_eq!(store.theme_settings(), [s.clone(), p.clone()], "the site first");
    // Refused: an undefined theme, a shortcode that is none, the site
    // reaching or opting out.
    for (scope, theme, reach, takeover, why) in [
        ("site", Some("mint"), None, None, "no theme `mint`"),
        ("NOPE", Some("ocean"), None, None, "no shortcode NOPE"),
        ("site", Some("ocean"), Some(ThemeReach::Site), None, "the site adopts a theme or none"),
        ("site", None, None, Some(Takeover::Off), "the site adopts a theme or none"),
    ] {
        let err = store.set_theme(scope, theme, reach, takeover).await.unwrap_err().to_string();
        assert!(err.contains(why), "{scope}: {err}");
    }
    // `default` is adopting none; saying nothing clears it.
    let cleared = store.set_theme("PANTRY", Some("default"), None, None).await.unwrap();
    assert!(cleared.theme.is_none());
    assert_eq!(store.theme_settings(), [s]);
    // The channel heard the define too.
    store.define_theme("mint", map(&[("--accent", "#2a7")]), BTreeMap::new()).await.unwrap();
    assert!(matches!(rx.recv().await.unwrap(), ThemeChange::Set(_)));
    assert!(matches!(rx.recv().await.unwrap(), ThemeChange::Set(_)));
    assert!(matches!(rx.recv().await.unwrap(), ThemeChange::Changed(t) if t.name == "mint"));
}

/// THEME-8's table, every row, through the store: the site theme kept, the
/// ticket's and its frame's said in the body's frontmatter.
#[tokio::test]
#[allow(clippy::type_complexity)]
async fn theme_for_holds_every_row_of_who_wears_what() {
    use ThemeLevel as L;
    let store = Store::new();
    two_themes(&store).await;
    let rows: [(Option<&str>, &str, [(String, ThemeLevel); 3]); 8] = [
        (None, "", [w("default", L::None), w("default", L::None), w("default", L::None)]),
        (Some("ocean"), "", [w("ocean", L::Site), w("ocean", L::Site), w("ocean", L::Site)]),
        (Some("ocean"), "takeover: off", [w("ocean", L::Site), w("default", L::Ticket), w("default", L::Ticket)]),
        (Some("ocean"), "frame-takeover: off", [w("ocean", L::Site), w("ocean", L::Site), w("default", L::Frame)]),
        (None, "theme: pantry", [w("default", L::None), w("pantry", L::Ticket), w("pantry", L::Ticket)]),
        (None, "theme: pantry\nreach: site", [w("pantry", L::Ticket), w("pantry", L::Ticket), w("pantry", L::Ticket)]),
        (None, "frame-theme: pantry", [w("default", L::None), w("default", L::None), w("pantry", L::Frame)]),
        (Some("ocean"), "theme: pantry\nframe-takeover: off", [w("ocean", L::Site), w("pantry", L::Ticket), w("default", L::Frame)]),
    ];
    store.create(ticket("ROW", Kind::Scope, "", None)).await.unwrap();
    for (i, (site, fm, want)) in rows.into_iter().enumerate() {
        store.set_theme("site", site, None, None).await.unwrap();
        let body = format!("---\n{fm}\n---\n<p>row {i}</p>");
        let t = store.create(ticket("ROW", Kind::Build, &body, Some("ROW-1"))).await.unwrap();
        assert_eq!(regions(&store, &t.id).await, want, "row {i}: site {site:?}, {fm:?}");
    }
    assert!(matches!(store.theme_for("ROW-99").await, Err(StoreError::NotFound(_))));
    // The answer's levels say what each level said.
    let (_, _, levels) = store.theme_for("ROW-9").await.unwrap();
    assert_eq!(levels.site.as_deref(), Some("ocean"));
    assert_eq!(levels.ticket.theme.as_deref(), Some("pantry"));
    assert_eq!(levels.frame.takeover, Takeover::Off);
    // An amend moves the answer with the body.
    store.amend("ROW-9", None, Some("no frontmatter".into())).await.unwrap();
    assert_eq!(regions(&store, "ROW-9").await, [w("ocean", L::Site), w("ocean", L::Site), w("ocean", L::Site)]);
}

/// The shortcode sits between the site and the ticket; a hosted shortcode
/// with nothing of its own takes its host's (THEME-4), its own overrides.
#[tokio::test]
async fn the_nearest_shortcode_says_it_for_its_tickets_and_hosted_ones() {
    use ThemeLevel as L;
    let store = Store::new();
    two_themes(&store).await;
    store.create(ticket("PANTRY", Kind::Scope, "", None)).await.unwrap();
    store.create(ticket("PANTRY", Kind::Build, "", Some("PANTRY-1"))).await.unwrap();
    // SHELF is hosted under PANTRY: its root's parent is there.
    store.create(ticket("SHELF", Kind::Scope, "", Some("PANTRY-1"))).await.unwrap();
    store.create(ticket("SHELF", Kind::Build, "", Some("SHELF-1"))).await.unwrap();
    store.set_theme("site", Some("ocean"), None, None).await.unwrap();
    store.set_theme("PANTRY", Some("pantry"), Some(ThemeReach::Site), None).await.unwrap();
    let r = regions(&store, "PANTRY-2").await;
    assert_eq!(r, [w("pantry", L::Shortcode), w("pantry", L::Shortcode), w("pantry", L::Shortcode)]);
    let (_, regions_, _) = store.theme_for("SHELF-2").await.unwrap();
    assert_eq!(regions_.ticket.from.as_deref(), Some("PANTRY"), "a hosted shortcode inherits from its host");
    store.set_theme("SHELF", None, None, Some(Takeover::Off)).await.unwrap();
    assert_eq!(regions(&store, "SHELF-2").await, [w("ocean", L::Site), w("default", L::Shortcode), w("default", L::Shortcode)]);
    assert_eq!(store.theme_for("SHELF-2").await.unwrap().1.ticket.from.as_deref(), Some("SHELF"));
}

/// Kept in the file: a reopen reads the themes and settings back.
#[tokio::test]
async fn themes_and_settings_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let store = Store::open(&path).unwrap();
        two_themes(&store).await;
        store.create(ticket("PANTRY", Kind::Scope, "", None)).await.unwrap();
        store.set_theme("site", Some("ocean"), None, None).await.unwrap();
        store.set_theme("PANTRY", Some("pantry"), Some(ThemeReach::Frame), Some(Takeover::Off)).await.unwrap();
        store.set_theme("PANTRY", Some("pantry"), Some(ThemeReach::Frame), Some(Takeover::Off)).await.unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.theme("ocean").unwrap().dark, map(&[("--accent", "#4db6ac")]));
    let settings = store.theme_settings();
    assert_eq!(settings.len(), 2);
    assert_eq!((settings[1].reach, settings[1].takeover), (ThemeReach::Frame, Takeover::Off));
    // The wire's events, as the activation yields them.
    let ev = ThemesEvent::Setting { setting: settings[0].clone() };
    assert_eq!(serde_json::to_value(&ev).unwrap()["setting"]["theme"], "ocean");
}
