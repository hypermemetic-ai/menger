//! AGS-28's native tests: the table's own sense (every chord parses, ids
//! are unique, no two bindings share a chord where both are live), matching
//! and sequences, the Mac's spelling, search — and the source lint, which
//! holds every handler to the table.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn t() -> Vec<Entry> {
    table_with(&[])
}

fn key(k: &str) -> Press {
    Press { key: k.to_string(), ..Press::default() }
}

fn shifted(k: &str) -> Press {
    Press { key: k.to_string(), shift: true, ..Press::default() }
}

fn at(mut p: Press, ts: f64) -> Press {
    p.at = ts;
    p
}

fn click() -> Press {
    Press { button: Some(Button::Click), ..Press::default() }
}

#[test]
fn every_chord_parses_and_every_id_is_one_binding() {
    let mut ids = BTreeSet::new();
    for b in BINDINGS {
        assert!(ids.insert(b.id), "{} twice", b.id);
        assert!(!b.chords.is_empty() && !b.contexts.is_empty() && !b.does.trim().is_empty(), "{}", b.id);
        for c in b.chords {
            Chord::parse(c).unwrap_or_else(|e| panic!("{}: {e}", b.id));
        }
    }
    assert_eq!(t().iter().map(|e| e.chords.len()).sum::<usize>(), BINDINGS.iter().map(|b| b.chords.len()).sum::<usize>());
    for bad in ["", "J", "Shift+", "Hyper+k", "Shift+Shift+k", "Esc", "g h j", "Click Click", "Numpad"] {
        assert!(Chord::parse(bad).is_err(), "{bad:?} parsed");
    }
}

/// Acceptance 1: no conflicts — and the test is live: a planted binding
/// sharing a chord with one already there is named, with it.
#[test]
fn no_two_bindings_share_a_chord_where_both_are_live() {
    let found = conflicts(&t());
    assert!(found.is_empty(), "AGS-28: chords that collide:\n{}", found.join("\n"));

    static PLANTED: [Binding; 2] = [
        Binding { id: "test.g", contexts: &[Context::Board], chords: &["g"], group: Group::Help, does: "x", native: false },
        Binding { id: "test.z", contexts: &[Context::Global], chords: &["Escape"], group: Group::Help, does: "x", native: false },
    ];
    let mut planted = t();
    planted.extend(PLANTED.iter().map(|b| Entry { binding: b, chords: b.chords.iter().map(|c| Chord::parse(c).unwrap()).collect() }));
    let found = conflicts(&planted).join("\n");
    assert!(found.contains("test.g") && found.contains("go.board"), "a key that begins a sequence: {found}");
    assert!(found.contains("test.z") && found.contains("app.escape"), "the same chord: {found}");
    assert!(!found.contains("dialog.close"), "a dialog's Esc is not live with the page's: {found}");
}

/// Every pair of bindings that collide: the same chord, or a single step
/// that is the first of another's sequence, in contexts live at once.
fn conflicts(table: &[Entry]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in table.iter().enumerate() {
        for b in &table[i + 1..] {
            let live = a.binding.contexts.iter().any(|x| b.binding.contexts.iter().any(|y| x.live_with(*y)));
            if !live {
                continue;
            }
            for ca in &a.chords {
                for cb in &b.chords {
                    let prefix = |s: &Chord, q: &Chord| s.steps.len() == 1 && q.steps.len() == 2 && q.steps[0] == s.steps[0];
                    if ca == cb || prefix(ca, cb) || prefix(cb, ca) {
                        out.push(format!("{} ({}) and {} ({})", a.binding.id, ca.show(false), b.binding.id, cb.show(false)));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn contexts_are_live_together_as_focus_allows() {
    use Context::*;
    assert!(Global.live_with(Nav) && Nav.live_with(Global) && Anywhere.live_with(Palette));
    assert!(!Nav.live_with(Board) && !Board.live_with(TicketPage) && !Nav.live_with(TicketPage), "focus is in one of them");
    assert!(!Palette.live_with(Global) && !Menu.live_with(Dialog), "a modal context owns its keys");
    assert!(Chip.live_with(Nav) && Ticket.live_with(Board) && Link.live_with(TicketPage));
    assert!(!Link.live_with(Nav) && !Board.live_with(Link), "a body's links are on no row");
    for a in Context::ALL {
        for b in Context::ALL {
            assert_eq!(a.live_with(b), b.live_with(a), "{a:?} {b:?}");
        }
    }
}

#[test]
fn keys_match_with_their_case_and_their_modifiers() {
    let t = t();
    let nav = |p: Press| pick_single(&t, &p, Context::Nav, false);
    assert_eq!(nav(key("j")), Some("move.next"));
    assert_eq!(nav(key("ArrowDown")), Some("move.next"));
    assert_eq!(nav(key("J")), Some("move.next"), "caps lock: J without Shift is j");
    assert_eq!(nav(shifted("J")), Some("select.extend-down"));
    assert_eq!(nav(shifted("ArrowDown")), Some("select.extend-down"));
    assert_eq!(nav(key(" ")), Some("select.toggle"));
    assert_eq!(nav(shifted(" ")), None, "Shift+Space is no toggle");
    assert_eq!(nav(Press { key: "a".into(), ctrl: true, ..Press::default() }), Some("select.all"));
    assert_eq!(nav(Press { key: "j".into(), alt: true, ..Press::default() }), None, "Alt is written or not matched");
    assert_eq!(nav(Press { key: "j".into(), ctrl: true, ..Press::default() }), None, "Ctrl too");
    // a symbol ignores the Shift its layout needs; the numpad's * is its own
    assert_eq!(nav(shifted("*")), Some("nav.expand-subtree"), "Shift+8");
    assert_eq!(nav(Press { key: "*".into(), code: "NumpadMultiply".into(), shift: true, ..Press::default() }), Some("nav.expand-all"));
    assert_eq!(nav(Press { key: "*".into(), code: "NumpadMultiply".into(), ..Press::default() }), Some("nav.expand-subtree"));
    let global = |p: Press| pick_single(&t, &p, Context::Global, false);
    assert_eq!(global(shifted("?")), Some("help.keys"));
    assert_eq!(global(key("f")), Some("filter.view"));
    assert_eq!(global(shifted("F")), Some("filter.global"));
    assert_eq!(global(shifted("A")), Some("annotate.toggle"));
    assert_eq!(global(key("Escape")), Some("app.escape"));
    // gestures
    let rows = |p: Press| pick_single(&t, &p, Context::Board, false);
    assert_eq!(rows(click()), Some("select.only"));
    assert_eq!(rows(Press { shift: true, ..click() }), Some("select.range"));
    assert_eq!(rows(Press { shift: true, ctrl: true, ..click() }), Some("select.range"));
    assert_eq!(rows(Press { ctrl: true, ..click() }), Some("select.toggle"));
    assert_eq!(rows(Press { alt: true, ..click() }), None);
    assert_eq!(rows(key("Click")), None, "a key named Click is no click");
    assert_eq!(pick_single(&t, &Press { alt: true, ..click() }, Context::Chip, false), Some("chip.filter-everywhere"));
    assert_eq!(pick_single(&t, &Press { button: Some(Button::RightClick), ..Press::default() }, Context::Ticket, false), Some("menu.open"));
    assert_eq!(pick_single(&t, &shifted("F10"), Context::Ticket, false), Some("menu.open"));
    // a frame's posts
    assert_eq!(pick_single(&t, &Press::key("A"), Context::Frame, false), Some("annotate.toggle"));
    assert_eq!(pick_single(&t, &Press::key("z"), Context::Frame, false), Some("view.full-screen"));
}

/// MENGER-6's chord, as the table says it: ⌘K on a Mac (Ctrl+K is a
/// field's kill-line there), Ctrl+K or ⌘K elsewhere; no Shift, no Alt.
#[test]
fn mod_is_cmd_on_a_mac_and_ctrl_or_cmd_elsewhere() {
    let t = t();
    let k = |ctrl, meta, alt, shift, key: &str, mac| {
        pick_single(&t, &Press { key: key.into(), ctrl, meta, alt, shift, ..Press::default() }, Context::Anywhere, mac) == Some("palette.toggle")
    };
    assert!(k(false, true, false, false, "k", true), "⌘K on a Mac");
    assert!(k(false, true, false, false, "K", true), "caps lock");
    assert!(!k(true, false, false, false, "k", true), "Ctrl+K on a Mac");
    assert!(k(true, false, false, false, "k", false), "Ctrl+K elsewhere");
    assert!(k(false, true, false, false, "k", false), "⌘K elsewhere too");
    assert!(!k(true, false, false, true, "k", false), "Shift keeps it the browser's");
    assert!(!k(true, false, true, false, "k", false), "Alt too");
    assert!(!k(false, false, false, false, "k", false), "a bare k is the board's");
    assert!(!k(true, false, false, false, "j", false));
    // a Mac's ⌘-click toggles; its Ctrl+click is a right-click (the menu)
    let rows = |p: Press, mac| pick_single(&t, &p, Context::Nav, mac);
    assert_eq!(rows(Press { meta: true, ..click() }, true), Some("select.toggle"));
    assert_eq!(rows(Press { ctrl: true, ..click() }, true), None);
}

/// A sequence is armed by its owner, answered by its second key within
/// [`SEQUENCE_MS`] — whoever asks first — and the same keydown seen by a
/// second listener neither re-arms nor ends it.
#[test]
fn sequences_arm_complete_and_time_out_in_one_place() {
    let t = t();
    let (id, next) = pick(&t, &at(key("g"), 1.0), Context::Global, false, None, 1000.0);
    assert_eq!(id, Some(PREFIX));
    let Next::Arm(armed) = next else { panic!("g arms") };
    // the same keydown, seen by the ticket page's listener: nothing
    assert_eq!(pick(&t, &at(key("g"), 1.0), Context::TicketPage, false, Some(&armed), 1000.0), (None, Next::Keep));
    // `h` in the nav after `g`: the nav's collapse stands down — it is Global's
    assert_eq!(pick(&t, &at(key("h"), 2.0), Context::Nav, false, Some(&armed), 1400.0), (None, Next::Keep));
    assert_eq!(pick(&t, &at(key("h"), 2.0), Context::Global, false, Some(&armed), 1400.0), (Some("go.board"), Next::Disarm));
    assert_eq!(pick(&t, &at(key("n"), 2.0), Context::Global, false, Some(&armed), 1400.0), (Some("go.needs"), Next::Disarm));
    // too late: `h` is the nav's again
    assert_eq!(pick(&t, &at(key("h"), 2.0), Context::Nav, false, Some(&armed), 1000.0 + SEQUENCE_MS), (Some("nav.collapse"), Next::Keep));
    // another key ends the sequence and is itself
    assert_eq!(pick(&t, &at(key("j"), 2.0), Context::Nav, false, Some(&armed), 1200.0), (Some("move.next"), Next::Disarm));
    // the nav does not arm what it does not own
    assert_eq!(pick(&t, &at(key("g"), 3.0), Context::Nav, false, None, 1000.0), (None, Next::Keep));
}

/// Acceptance 1: the Mac's spelling — ⌘ for Mod, ⌥ for Alt, ⇧ for Shift —
/// and everyone else's.
#[test]
fn chords_are_spelled_for_the_platform() {
    let show = |c: &str, mac| Chord::parse(c).unwrap().show(mac);
    assert_eq!(show("Mod+k", true), "⌘K");
    assert_eq!(show("Mod+k", false), "Ctrl+K");
    assert_eq!(show("Alt+Click", true), "⌥-click");
    assert_eq!(show("Alt+Click", false), "Alt+click");
    assert_eq!(show("Shift+Mod+Click", true), "⇧⌘-click");
    assert_eq!(show("Shift+Mod+Click", false), "Ctrl+Shift+click");
    assert_eq!(show("Mod+Enter", true), "⌘Enter");
    assert_eq!(show("Shift+f", true), "F", "a capital is a capital");
    assert_eq!(show("Shift+j", false), "J");
    assert_eq!(show("Shift+ArrowDown", false), "Shift+↓");
    assert_eq!(show("Shift+ArrowDown", true), "⇧↓");
    assert_eq!(show("Shift+Numpad*", false), "Shift+* (numpad)");
    assert_eq!(show("g h", true), "g h");
    assert_eq!(show("Escape", false), "Esc");
    assert_eq!(show("Space", false), "Space");
    assert_eq!(show("Click", false), "Click");
    assert_eq!(show("RightClick", false), "Right-click");
    // AGS-29: the nav row's double-click is a gesture like the others.
    assert_eq!(show("DoubleClick", false), "Double-click");
    let dbl = Press { button: Some(Button::DoubleClick), ..Press::default() };
    assert_eq!(pick_single(&t(), &dbl, Context::Nav, false), Some("nav.toggle"));
    assert_eq!(pick_single(&t(), &dbl, Context::Board, false), None, "the board has nothing to double-click");
    assert_eq!(pick_single(&t(), &click(), Context::Nav, false), Some("select.only"), "a click is not a double-click");
    assert_eq!(show("Shift+F10", false), "Shift+F10");
    let palette = t().into_iter().find(|e| e.binding.id == "palette.toggle").unwrap();
    assert_eq!(keys_text(&palette, true), "⌘K");
    let range = t().into_iter().find(|e| e.binding.id == "select.range").unwrap();
    assert_eq!(keys_text(&range, false), "Shift+click · Ctrl+Shift+click");
    assert_eq!(where_text(range.binding), "in the nav · on the board");
}

/// Acceptance 1: search — a key keeps the rows with that chord, however it
/// is spelled; words keep the rows that say them all.
#[test]
fn search_narrows_by_a_key_or_by_words() {
    let t = t();
    let ids = |q: &str, mac| search(&t, q, mac).iter().map(|e| e.binding.id).collect::<Vec<_>>();
    assert_eq!(ids("", false).len(), BINDINGS.len());
    assert_eq!(ids("j", false), ["move.next"]);
    assert_eq!(ids("J", false), ["select.extend-down"], "the capital is Shift+j");
    assert_eq!(ids("*", false), ["nav.expand-subtree"]);
    assert_eq!(ids("g h", false), ["go.board"]);
    assert_eq!(ids("ctrl+k", false), ["palette.toggle"]);
    assert_eq!(ids("⌘K", true), ["palette.toggle"]);
    assert_eq!(ids("Mod+k", false), ["palette.toggle"], "the table's own spelling");
    assert_eq!(ids("alt+click", false), ["chip.filter-everywhere"]);
    assert_eq!(ids("esc", false), ["app.escape", "dialog.close", "form.cancel"]);
    assert!(ids("q", false).is_empty(), "a key nothing answers");
    assert_eq!(ids("global filter", false), ["filter.global", "chip.filter-everywhere"]);
    assert_eq!(ids("SUBTREE", false), ["nav.expand-subtree"]);
    assert!(ids("annotation", false).contains(&"annotate.toggle"));
    assert!(ids("in the nav collapse", false).iter().all(|id| id.starts_with("nav.")), "words of where count too");
    assert!(ids("zzz nothing", false).is_empty());
}

#[test]
fn an_override_replaces_a_bindings_chords_and_nothing_else() {
    let t = table_with(&[("nav.expand", vec!["Shift+ArrowRight".into(), "not a chord".into()]), ("no.such", vec!["x".into()])]);
    assert_eq!(pick_single(&t, &shifted("ArrowRight"), Context::Nav, false), Some("nav.expand"));
    assert_eq!(pick_single(&t, &key("l"), Context::Nav, false), None, "its own chord is replaced");
    assert_eq!(pick_single(&t, &key("h"), Context::Nav, false), Some("nav.collapse"));
}

// ---------------------------------------------------------------------
// The lint: every handler asks the table

/// Files the lint reads past, and why — the table itself, nothing else. A
/// widget that must read a key itself is listed here with its reason.
fn exempt(file: &str) -> Option<&'static str> {
    match file {
        "bindings.rs" => Some("the table, and the only reader of an event's keys"),
        _ => None,
    }
}

/// A line marked so reads a `key` that is no keyboard's (a StorageEvent's).
const PASS: &str = "bindings-lint: ";

/// The key names a handler would compare with.
const KEY_NAMES: &[&str] = &[
    "\"Escape\"", "\"Esc\"", "\"Enter\"", "\"Tab\"", "\"ArrowUp\"", "\"ArrowDown\"", "\"ArrowLeft\"", "\"ArrowRight\"", "\"Home\"", "\"End\"",
    "\"PageUp\"", "\"PageDown\"", "\"ContextMenu\"", "\"F10\"", "\"Backspace\"", "\"Spacebar\"",
];

/// What the lint refuses on one shipped line.
fn verdict(line: &str) -> Option<&'static str> {
    if line.contains(PASS) {
        return None;
    }
    for m in [".code()", ".shift_key()", ".alt_key()", ".ctrl_key()", ".meta_key()", ".button()", ".buttons()"] {
        if line.contains(m) {
            return Some("an event's modifiers, button or code");
        }
    }
    // `ev.key()` — an event's key (a node's `.key()` is not one)
    for (i, _) in line.match_indices(".key()") {
        let ident: String = line[..i].chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect();
        if ident == "ev" || ident == "event" || ident.ends_with("_ev") {
            return Some("an event's key");
        }
    }
    if line.contains("from_str(\"key\")") {
        return Some("an event's key, read by name");
    }
    if KEY_NAMES.iter().any(|k| line.contains(k)) {
        return Some("a key's name");
    }
    // `key == "z"` — a key read before, compared (a field's `p.key` is not one)
    for pat in ["key == \"", "key != \""] {
        for (i, _) in line.match_indices(pat) {
            let before = line[..i].chars().last();
            if !before.is_some_and(|c| c == '.' || c.is_ascii_alphanumeric() || c == '_') {
                return Some("a key string compared");
            }
        }
    }
    None
}

fn src() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every shipped source the lint reads — test modules and `_tests.rs` files
/// out — by its path under src/.
fn shipped() -> Vec<(String, String)> {
    let mut files = crate::tokens_lint::rust_files(&src());
    files.sort();
    files
        .into_iter()
        .filter(|p| !p.file_name().unwrap().to_string_lossy().ends_with("_tests.rs"))
        .map(|p| {
            let rel = p.strip_prefix(src()).unwrap().to_string_lossy().to_string();
            (rel, crate::tokens_lint::rust_shipped(&std::fs::read_to_string(&p).unwrap()))
        })
        .collect()
}

fn lint(file: &str, text: &str) -> Vec<String> {
    let name = file.rsplit('/').next().unwrap_or(file);
    if exempt(name).is_some() {
        return Vec::new();
    }
    text.lines().enumerate().filter_map(|(i, l)| verdict(l).map(|k| format!("frontend/src/{file}:{}: {k}: {}", i + 1, l.trim()))).collect()
}

/// Acceptance 2: no handler outside the table reads a key, a button or a
/// modifier.
#[test]
fn no_handler_compares_a_raw_key_outside_the_table() {
    let found: Vec<String> = shipped().iter().flat_map(|(f, t)| lint(f, t)).collect();
    assert!(
        found.is_empty(),
        "AGS-28: a handler reads the event itself — declare the key in src/bindings.rs's BINDINGS and match \
         `bindings::hit(&ev, Context::…)` (or hit_mouse / is / key_hit) by id:\n{}",
        found.join("\n")
    );
}

/// The lint is live: what it refuses, planted, is named; what it allows
/// passes.
#[test]
fn the_lint_catches_a_planted_key() {
    for bad in [
        "if ev.key() == \"Escape\" {",
        "match ev.key().as_str() {",
        "\"j\" if ev.shift_key() => true,",
        "if ev.button() != 0 { return; }",
        "let tab = Reflect::get(&ev, &JsValue::from_str(\"key\"))",
        "} else if key == \"z\" {",
        "\"ArrowDown\" | \"ArrowUp\" => {}",
    ] {
        assert!(verdict(bad).is_some(), "the lint missed {bad:?}");
        assert!(!lint("nav.rs", bad).is_empty());
        assert!(lint("bindings.rs", bad).is_empty(), "the table reads keys");
    }
    for fine in [
        "match bindings::hit(&ev, Context::Nav) {",
        "let keys = |v: Vec<Node>| v.iter().map(|n| n.key().to_string())",
        ".find(|p| p.key == \"comment\")",
        "label: \"Delete\".into()",
        "if ev.key().as_deref() == Some(KEY) { // bindings-lint: a StorageEvent's key",
        "out.join(\" \")",
    ] {
        assert!(verdict(fine).is_none(), "the lint refused {fine:?}");
    }
}

/// Every string literal in `text` — escapes, char literals (`'"'`) and raw
/// strings read past as Rust reads them.
fn literals(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '\'' if c.get(i + 1) == Some(&'\\') => {
                i += 2;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
            }
            '\'' if c.get(i + 2) == Some(&'\'') => i += 3,
            'r' if (c.get(i + 1) == Some(&'"') || c.get(i + 1) == Some(&'#')) && (i == 0 || !(c[i - 1].is_alphanumeric() || c[i - 1] == '_')) => {
                let hashes = c[i + 1..].iter().take_while(|x| **x == '#').count();
                let start = i + 1 + hashes + 1;
                if c.get(i + 1 + hashes) != Some(&'"') {
                    i += 1;
                    continue;
                }
                let close = format!("\"{}", "#".repeat(hashes));
                let rest: String = c[start..].iter().collect();
                let end = rest.find(&close).unwrap_or(rest.len());
                out.push(rest[..end].to_string());
                i = start + rest[..end].chars().count() + close.chars().count();
            }
            '"' => {
                let mut lit = String::new();
                i += 1;
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\\' {
                        i += 1;
                    }
                    if i < c.len() {
                        lit.push(c[i]);
                    }
                    i += 1;
                }
                out.push(lit);
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Acceptance 1: every binding's id is dispatched somewhere — the ids each
/// handler names are read from the sources — and no handler names an id the
/// table does not hold.
#[test]
fn every_binding_is_dispatched_and_every_dispatched_id_is_a_binding() {
    let files = shipped();
    let mut named: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let namespaces: BTreeSet<&str> = BINDINGS.iter().filter_map(|b| b.id.split_once('.').map(|(ns, _)| ns)).collect();
    let mut strays = Vec::new();
    for (file, text) in &files {
        if file == "bindings.rs" {
            continue;
        }
        for lit in literals(text) {
            let lit = lit.as_str();
            if binding(lit).is_some() {
                named.entry(binding(lit).unwrap().id).or_default().push(file.clone());
            } else if let Some((ns, rest)) = lit.split_once('.') {
                let id_like = !rest.is_empty() && rest.chars().all(|c| c.is_ascii_lowercase() || c == '-');
                if namespaces.contains(ns) && id_like {
                    strays.push(format!("{file}: {lit:?}"));
                }
            }
        }
    }
    let missing: Vec<&str> = BINDINGS
        .iter()
        .filter(|b| !b.native && !named.contains_key(b.id))
        .map(|b| b.id)
        .collect();
    assert!(missing.is_empty(), "AGS-28: bindings no handler dispatches: {missing:?}");
    assert!(strays.is_empty(), "AGS-28: handlers name ids the table does not hold:\n{}", strays.join("\n"));
}
