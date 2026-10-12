//! THEME-8: the rule, row by row — "who wears what" — and what a body's
//! frontmatter says, what a theme paints, and what `define` refuses.

use super::*;

fn said(fm: &str) -> Declared {
    declared(&format!("---\n{fm}\n---\nbody"))
}

/// The levels for a site theme and a body's frontmatter.
fn levels(site: Option<&str>, fm: &str) -> Levels {
    let d = said(fm);
    Levels { site: site.map(str::to_string), shortcode: None, ticket: d.ticket, frame: d.frame }
}

fn worn(r: &Regions) -> [(String, ThemeLevel); 3] {
    [(r.site.theme.clone(), r.site.level), (r.ticket.theme.clone(), r.ticket.level), (r.frame.theme.clone(), r.frame.level)]
}

fn themes(r: &Regions) -> [&str; 3] {
    [r.site.theme.as_str(), r.ticket.theme.as_str(), r.frame.theme.as_str()]
}

/// THEME-8's table, every row: the site theme, what the ticket says, what
/// its frame says → the site, the ticket view, the frame.
#[test]
fn every_row_of_who_wears_what() {
    let rows: [(Option<&str>, &str, [&str; 3]); 9] = [
        (None, "", ["default", "default", "default"]),
        (Some("ocean"), "", ["ocean", "ocean", "ocean"]),
        (Some("ocean"), "takeover: off", ["ocean", "default", "default"]),
        (Some("ocean"), "frame-takeover: off", ["ocean", "ocean", "default"]),
        (None, "theme: pantry", ["default", "pantry", "pantry"]),
        (None, "theme: pantry\nreach: site", ["pantry", "pantry", "pantry"]),
        (None, "frame-theme: pantry", ["default", "default", "pantry"]),
        // The frame's adoption said the other way: `reach: frame`.
        (None, "theme: pantry\nreach: frame", ["default", "default", "pantry"]),
        (Some("ocean"), "theme: pantry\nframe-takeover: off", ["ocean", "pantry", "default"]),
    ];
    for (site, fm, want) in rows {
        let r = resolve(&levels(site, fm), &Reader::default());
        assert_eq!(themes(&r), want, "site {site:?}, frontmatter {fm:?}");
    }
}

/// `theme_for` names the winning level for each region.
#[test]
fn each_region_names_the_level_it_came_from() {
    use ThemeLevel as L;
    let r = resolve(&levels(Some("ocean"), "theme: pantry\nframe-takeover: off"), &Reader::default());
    assert_eq!(worn(&r), [("ocean".into(), L::Site), ("pantry".into(), L::Ticket), ("default".into(), L::Frame)]);
    let r = resolve(&levels(None, ""), &Reader::default());
    assert_eq!(worn(&r), [("default".into(), L::None), ("default".into(), L::None), ("default".into(), L::None)]);
    let r = resolve(&levels(Some("ocean"), "takeover: off"), &Reader::default());
    assert_eq!(worn(&r), [("ocean".into(), L::Site), ("default".into(), L::Ticket), ("default".into(), L::Ticket)]);
    let r = resolve(&levels(None, "theme: pantry\nreach: site"), &Reader::default());
    assert_eq!(worn(&r), [("pantry".into(), L::Ticket), ("pantry".into(), L::Ticket), ("pantry".into(), L::Ticket)]);
    // Its own adoption beats its own opt-out.
    let r = resolve(&levels(Some("ocean"), "takeover: off\ntheme: pantry"), &Reader::default());
    assert_eq!(themes(&r), ["ocean", "pantry", "pantry"]);
}

/// The shortcode sits between the site and the ticket and acts as a ticket
/// does; the ticket, nearer, wins — and its `reach: site` beats the
/// shortcode's.
#[test]
fn the_shortcode_sits_between_the_site_and_the_ticket() {
    let code = |theme: Option<&str>, reach, takeover| ThemeSetting { scope: "PANTRY".into(), theme: theme.map(str::to_string), reach, takeover };
    let with = |s: ThemeSetting, fm: &str| Levels { shortcode: Some(s), ..levels(Some("ocean"), fm) };
    let r = resolve(&with(code(Some("pantry"), ThemeReach::Ticket, Takeover::On), ""), &Reader::default());
    assert_eq!(themes(&r), ["ocean", "pantry", "pantry"]);
    assert_eq!((r.ticket.level, r.ticket.from.as_deref()), (ThemeLevel::Shortcode, Some("PANTRY")));
    let r = resolve(&with(code(Some("pantry"), ThemeReach::Site, Takeover::On), ""), &Reader::default());
    assert_eq!(themes(&r), ["pantry", "pantry", "pantry"]);
    let r = resolve(&with(code(None, ThemeReach::Ticket, Takeover::Off), ""), &Reader::default());
    assert_eq!(themes(&r), ["ocean", "default", "default"], "a shortcode keeps the site theme out");
    let r = resolve(&with(code(Some("pantry"), ThemeReach::Frame, Takeover::On), ""), &Reader::default());
    assert_eq!(themes(&r), ["ocean", "ocean", "pantry"], "reach: frame paints only its frames");
    let r = resolve(&with(code(Some("pantry"), ThemeReach::Site, Takeover::On), "theme: mint\nreach: site"), &Reader::default());
    assert_eq!(themes(&r), ["mint", "mint", "mint"], "the ticket is nearer");
    let r = resolve(&with(code(Some("pantry"), ThemeReach::Site, Takeover::On), "takeover: off"), &Reader::default());
    assert_eq!(themes(&r), ["pantry", "default", "default"], "the ticket opts itself out, not the site");
    let r = resolve(&with(code(Some("pantry"), ThemeReach::Frame, Takeover::On), "theme: mint"), &Reader::default());
    assert_eq!(themes(&r), ["ocean", "mint", "mint"], "the nearer adoption covers the frame");
}

/// The reader is outermost: their site theme stands in for the site's, and
/// with every takeover off they wear it everywhere.
#[test]
fn the_reader_is_outermost() {
    let reader = Reader { site: Some("mint".into()), takeovers_off: false };
    let r = resolve(&levels(Some("ocean"), ""), &reader);
    assert_eq!(themes(&r), ["mint", "mint", "mint"]);
    assert_eq!(r.site.level, ThemeLevel::Reader);
    let r = resolve(&levels(Some("ocean"), "theme: pantry\nreach: site"), &reader);
    assert_eq!(themes(&r), ["pantry", "pantry", "pantry"], "a takeover still reaches them");
    let off = Reader { site: Some("mint".into()), takeovers_off: true };
    let r = resolve(&levels(Some("ocean"), "theme: pantry\nreach: site\nframe-theme: x"), &off);
    assert_eq!(themes(&r), ["mint", "mint", "mint"]);
    let off = Reader { site: None, takeovers_off: true };
    let r = resolve(&levels(Some("ocean"), "theme: pantry"), &off);
    assert_eq!(themes(&r), ["default", "default", "default"]);
    // No ticket open: the site's alone.
    let site = Levels { site: Some("ocean".into()), ..Default::default() };
    assert_eq!(themes(&resolve(&site, &Reader::default())), ["ocean", "ocean", "ocean"]);
}

/// The frontmatter: names any case, comments dropped, nonsense not said.
#[test]
fn a_body_says_it_in_its_frontmatter() {
    let d = said("theme: Pantry   # adopt\nreach: SITE\ntakeover: off");
    assert_eq!(d.ticket, Said { theme: Some("pantry".into()), reach: ThemeReach::Site, takeover: Takeover::Off });
    assert!(d.frame.is_silent());
    let d = said("theme: not a name\nreach: everywhere\ntakeover: maybe");
    assert!(d.ticket.is_silent() && d.ticket.reach == ThemeReach::Ticket);
    let d = said("theme: a\nreach: frame\nframe-theme: b");
    assert_eq!((d.ticket.theme, d.frame.theme.as_deref()), (None, Some("b")), "frame-theme wins over reach: frame");
    assert_eq!(declared("theme: pantry\n\nno frontmatter"), Declared::default());
    assert_eq!(declared(""), Declared::default());
}

/// A theme paints its overrides over the defaults; a face set for light
/// holds in dark, a colour set for light does not.
#[test]
fn a_theme_paints_its_overrides_over_the_defaults() {
    let mut th = Theme::default_theme();
    th.light.insert("--accent".into(), "#d4572a".into());
    th.light.insert("--sans".into(), "Inter, sans-serif".into());
    th.dark.insert("--bg".into(), "#171614".into());
    let accent = token("accent").unwrap();
    assert_eq!(th.value(accent, false), "#d4572a");
    assert_eq!(th.value(accent, true), "#8ab4f8", "a colour set for light only leaves dark's default");
    assert_eq!(th.value(token("--sans").unwrap(), true), "Inter, sans-serif", "a face holds in both");
    assert_eq!(th.value(token("--bg").unwrap(), true), "#171614");
    assert_eq!(th.painted(false).len(), TOKENS.len());
    assert_eq!(Theme::default_theme().painted(true).iter().find(|(n, _)| *n == "--bg").unwrap().1, "#101014");
}

/// `define`'s checks: known tokens only, any spelling; values that cannot
/// escape their declaration; every problem named at once.
#[test]
fn overrides_are_checked_and_named() {
    let map = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>();
    let ok = check_overrides("light", &map(&[("accent", " #f00 "), ("--BG", "rgba(0, 0, 0, .5)")])).unwrap();
    assert_eq!(ok, map(&[("--accent", "#f00"), ("--bg", "rgba(0, 0, 0, .5)")]));
    let err = check_overrides("dark", &map(&[("--nope", "#f00"), ("--ink", "red;}body{x:y"), ("--line", " ")])).unwrap_err();
    assert!(err.contains("dark: `--nope` is not a token"), "{err}");
    assert!(err.contains("`--ink`'s value"), "{err}");
    assert!(err.contains("`--line`'s value"), "{err}");
    assert_eq!(name_of(" Ocean ").unwrap(), "ocean");
    assert!(name_of("9lives").is_err() && name_of("a b").is_err() && name_of("").is_err());
    let names: std::collections::BTreeSet<_> = TOKENS.iter().map(|t| t.name).collect();
    assert_eq!(names.len(), TOKENS.len(), "a token listed twice");
    assert!(TOKENS.iter().all(|t| t.name.starts_with("--") && value_ok(t.light) && value_ok(t.dark)));
}

/// The wire's spelling.
#[test]
fn the_wire_spells_them_snake_case() {
    let ev = ThemesEvent::Setting {
        setting: ThemeSetting { scope: "site".into(), theme: Some("ocean".into()), reach: ThemeReach::Site, takeover: Takeover::Off },
    };
    let json = serde_json::to_value(&ev).unwrap();
    assert_eq!(json["type"], "setting");
    assert_eq!(json["setting"]["reach"], "site");
    assert_eq!(json["setting"]["takeover"], "off");
    let r = resolve(&levels(Some("ocean"), ""), &Reader::default());
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["frame"]["level"], "site");
    assert!(json["frame"].get("from").is_none());
}

/// THEME-3: `extends` — a theme states only what differs; the rest is its
/// parent's, up the chain to `default`; a non-colour said for light holds
/// in dark over a farther theme's dark; rules and imports parent-first; a
/// missing parent or a cycle ends the chain.
#[test]
fn a_theme_flattens_up_its_extends_to_default() {
    let th = |name: &str, extends: Option<&str>, light: &[(&str, &str)], dark: &[(&str, &str)], rules: &[&str]| Theme {
        name: name.into(),
        extends: extends.map(str::to_string),
        light: light.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        dark: dark.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        rules: rules.iter().map(|r| r.to_string()).collect(),
        imports: vec![format!("https://x.example/{name}.css")],
        ..Theme::default()
    };
    let all = [
        th("base", None, &[("--accent", "#111"), ("--ink", "#222")], &[("--sans", "Base"), ("--accent", "#999")], &["m-a::part(x){color:red;}"]),
        th("mid", Some("base"), &[("--sans", "Mid")], &[], &[]),
        th("top", Some("mid"), &[("--accent", "#333")], &[], &["m-b::part(y){color:blue;}"]),
        th("loop1", Some("loop2"), &[("--ink", "#1")], &[], &[]),
        th("loop2", Some("loop1"), &[("--ink", "#2")], &[], &[]),
        th("lost", Some("nowhere"), &[("--ink", "#3")], &[], &[]),
    ];
    let get = |n: &str| all.iter().find(|t| t.name == n);
    let top = flatten("top", get);
    assert_eq!(top.light.get("--accent").map(String::as_str), Some("#333"));
    assert_eq!(top.light.get("--ink").map(String::as_str), Some("#222"));
    assert_eq!(top.dark.get("--accent").map(String::as_str), Some("#999"), "a colour's dark stays the parent's");
    assert!(top.dark.get("--sans").is_none(), "mid's face for light holds in dark over base's dark face");
    assert_eq!(top.value(token("--sans").unwrap(), true), "Mid");
    assert_eq!(top.rules, ["m-a::part(x){color:red;}", "m-b::part(y){color:blue;}"]);
    assert_eq!(top.imports.len(), 3);
    assert_eq!(flatten("loop1", get).light.get("--ink").map(String::as_str), Some("#1"), "a cycle ends the chain");
    assert_eq!(flatten("lost", get).light.get("--ink").map(String::as_str), Some("#3"));
    assert!(flatten("unknown", get).light.is_empty());
    let css = rules_css(&top, Some(".sheet"));
    assert!(css.starts_with("@import url(\"https://x.example/base.css\");"), "{css}");
    assert!(css.contains(".sheet m-a::part(x){color:red;}"), "{css}");
    assert!(rules_css(&top, None).contains("m-b::part(y){color:blue;}"));
}
