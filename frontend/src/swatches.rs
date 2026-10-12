//! THEME-3: a ` ```theme ` block renders as its **swatch sheet** in the
//! owning ticket's body — the body frames (md.rs), and in the frame the
//! block becomes the sheet, as a ` ```component ` block becomes its card.
//!
//! The sheet shows the theme beside `default`, in light and in dark: four
//! panels, each wearing its theme's tokens inline (so the frame's own theme
//! does not matter), each holding the paper and ink, a title in the display
//! face, a link, every status chip, every kind glyph, a highlighted code
//! block and a decision (the house's `m-decision`, skinned by the theme's
//! `::part` rules, scoped to its panels); then every token of the contract,
//! default and theme, light and dark. A block that is not a theme shows its
//! refusal — the line and why — as the server would say it.
//!
//! The theme is read from the block in hand (a preview is live while
//! editing) and flattened over its parent as the app holds it.

use leptos::prelude::GetUntracked;
use menger_tickets_api::theme_block;
use menger_tickets_api::themes::{self, Theme, DEFAULT_THEME};
use menger_tickets_api::tokens::TOKENS;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const STATUSES: [(&str, &str); 11] = [
    ("pending", "Pending"),
    ("ready", "Ready"),
    ("coding", "Coding"),
    ("in-review", "In Review"),
    ("blocked", "Blocked"),
    ("complete", "Complete"),
    ("idea", "Idea"),
    ("epic", "Epic"),
    ("superseded", "Superseded"),
    ("canceled", "Canceled"),
    ("doc", "Doc"),
];

const KINDS: [&str; 5] = ["scope", "build", "execution", "gap", "spike"];

/// The sheet's own layout — tokens only (the lint holds it to them).
const CSS: &str = ".mts{border:var(--border) solid var(--line);border-radius:var(--radius-lg);padding:.6rem .8rem;margin:.75rem 0;background:var(--card)}\
.mts-head{display:flex;flex-wrap:wrap;gap:.3rem .8rem;align-items:baseline;font:calc(12px * var(--text-scale))/1.5 var(--mono);color:var(--muted)}\
.mts-head b{color:var(--ink);font-weight:var(--weight-strong)}\
.mts-error{color:var(--err-fg);background:var(--err-bg);border:var(--border) solid var(--err-line);border-radius:var(--radius);padding:.25rem .5rem;font:calc(12px * var(--text-scale))/1.5 var(--mono)}\
.mts-mode{margin:.6rem 0 0}\
.mts-mode>h4{margin:0 0 .3rem;font:var(--weight-strong) calc(11px * var(--text-scale))/1.5 var(--mono);text-transform:uppercase;letter-spacing:.06em;color:var(--muted)}\
.mts-pair{display:grid;grid-template-columns:repeat(auto-fit,minmax(16rem,1fr));gap:.6rem}\
.mts-panel{background:var(--bg);color:var(--ink);border:var(--border) solid var(--line);border-radius:var(--radius-lg);padding:.55rem .7rem;font:calc(13px * var(--text-scale))/1.5 var(--font-text);min-width:0}\
.mts-name{font:calc(11px * var(--text-scale))/1.5 var(--mono);color:var(--muted);margin-bottom:.2rem}\
.mts-title{font:var(--weight-strong) calc(18px * var(--text-scale))/1.3 var(--font-display);margin:.1rem 0 .3rem}\
.mts-panel a{color:var(--accent)}\
.mts-panel .mts-surface{background:var(--panel);border:var(--border) solid var(--line);border-radius:var(--radius);padding:.3rem .5rem;margin:.4rem 0}\
.mts-chips{display:flex;flex-wrap:wrap;gap:.25rem;margin:.4rem 0}\
.mts-chip{font:calc(10.5px * var(--text-scale))/1.6 var(--mono);padding:0 .4rem;border-radius:var(--radius-md);background:var(--st-bg);color:var(--st-fg)}\
.mts-kind{display:inline-flex;align-items:center;gap:.25rem;font:calc(11px * var(--text-scale))/1.5 var(--mono);color:var(--muted);margin-right:.6rem}\
.mts-kind i{display:inline-block;width:.6rem;height:.6rem;border-radius:var(--radius-xs);background:var(--k)}\
.mts-panel pre{margin:.4rem 0;font-size:calc(12px * var(--text-scale))}\
.mts-tokens{width:100%;border-collapse:collapse;font:calc(11.5px * var(--text-scale))/1.5 var(--mono);margin-top:.4rem}\
.mts-tokens td,.mts-tokens th{border-bottom:var(--border) solid var(--line);padding:.1rem .35rem;text-align:left;vertical-align:middle}\
.mts-tokens th{color:var(--muted);font-weight:var(--weight-medium)}\
.mts-tokens .set{color:var(--accent)}\
.mts-sw{display:inline-block;width:.85rem;height:.85rem;border:var(--border) solid var(--line);border-radius:var(--radius-xs);vertical-align:-.15rem;margin-right:.3rem}";

/// One panel: `theme` (flattened) in one mode, every sample inside it.
fn panel(theme: &Theme, dark: bool, scope_id: &str) -> String {
    let mut style = format!("color-scheme:{}", if dark { "dark" } else { "light" });
    for (name, value) in theme.painted(dark) {
        style.push_str(&format!(";{name}:{value}"));
    }
    let mut out = format!(
        "<div class=\"mts-panel\" data-mts=\"{scope_id}\" data-mode=\"{}\" style=\"{}\">",
        if dark { "dark" } else { "light" },
        esc(&style)
    );
    out.push_str(&format!("<div class=\"mts-name\">{} · {}</div>", esc(&theme.name), if dark { "dark" } else { "light" }));
    out.push_str("<div class=\"mts-title\">The shelf remembers every move</div>");
    out.push_str("<div>Body text in the theme's face, a <a href=\"#\">link in its accent</a>, and <code>inline code</code>.</div>");
    out.push_str("<div class=\"mts-surface\">a panel, its hairline and <span style=\"color:var(--muted)\">muted words</span></div>");
    out.push_str("<div class=\"mts-chips\">");
    for (slug, word) in STATUSES {
        out.push_str(&format!("<span class=\"mts-chip\" style=\"--st-bg:var(--st-{slug}-bg);--st-fg:var(--st-{slug}-fg)\">{word}</span>"));
    }
    out.push_str("</div><div>");
    for k in KINDS {
        out.push_str(&format!("<span class=\"mts-kind\" style=\"--k:var(--kind-{k})\"><i></i>{k}</span>"));
    }
    out.push_str("</div>");
    out.push_str("<pre><code class=\"language-rust\">// a ledger never edits a line\nfn use_up(item: &amp;str, qty: i64) -&gt; Result&lt;(), Error&gt; {\n    let have = moves.sum(item)?; // 2\n    moves.append(item, -qty, \"now\")\n}</code></pre>");
    out.push_str(
        "<m-decision data-field=\"decision\"><m-question data-field=\"question\">Archive or delete expired events?</m-question>\
         <m-option data-field=\"option\" name=\"Archive\" value=\"archive\" recommended cost=\"a second store\" benefit=\"holds honoured\"></m-option>\
         <m-option data-field=\"option\" name=\"Delete\" value=\"delete\" cost=\"holds broken\" benefit=\"nothing to run\"></m-option>\
         <m-recommendation data-field=\"recommendation\">Archive: holds are real.</m-recommendation></m-decision>",
    );
    out.push_str("</div>");
    out
}

/// A token's value as a cell: a swatch for a colour, the value written.
fn cell(t: &menger_tickets_api::tokens::Token, value: &str, set: bool) -> String {
    let sw = if t.is_colour() && value != "auto" {
        format!("<i class=\"mts-sw\" style=\"background:{}\"></i>", esc(value))
    } else {
        String::new()
    };
    format!("<td{}>{sw}{}</td>", if set { " class=\"set\"" } else { "" }, esc(value))
}

/// The sheet for a block: its info string and its content.
pub(crate) fn sheet(info: &str, content: &str) -> String {
    let mut out = format!("<section class=\"mts\" data-menger-theme=\"{}\"><style>{CSS}</style>", esc(info.split_whitespace().nth(1).unwrap_or("")));
    let read = theme_block::info(info)
        .unwrap_or_else(|| Err(theme_block::BlockError { line: 0, message: "not a theme".into() }))
        .and_then(|(name, extends)| theme_block::parse(&name, extends.as_deref(), content));
    let def = match read {
        Ok(d) => d,
        Err(e) => {
            out.push_str(&format!(
                "<div class=\"mts-head\">theme <b>{}</b></div><p class=\"mts-error\">```{}: {} — the server refuses this block; nothing of it is kept</p></section>",
                esc(info.split_whitespace().nth(1).unwrap_or("?")),
                esc(info.trim()),
                esc(&e.to_string())
            ));
            return out;
        }
    };
    let own = Theme {
        name: def.name.clone(),
        light: def.light.clone(),
        dark: def.dark.clone(),
        extends: def.extends.clone(),
        rules: def.rules.clone(),
        imports: def.imports.clone(),
        ..Theme::default()
    };
    let held = crate::scopes::held().get_untracked();
    let theme = themes::flatten(&own.name, |n| if n == own.name { Some(&own) } else { held.own(n) });
    let base = Theme::default_theme();
    let scope = format!(".mts-panel[data-mts=\"{}\"]", def.name);
    let rules = themes::rules_css(&theme, Some(&scope));
    if !rules.is_empty() {
        out.push_str(&format!("<style>{rules}</style>"));
    }
    let said = def.light.len() + def.dark.len();
    out.push_str(&format!(
        "<div class=\"mts-head\"><span>theme <b>{}</b></span><span>extends <b>{}</b></span><span>{said} token value{} set</span><span>{} font import{}, {} rule{}</span></div>",
        esc(&def.name),
        esc(def.extends.as_deref().unwrap_or(DEFAULT_THEME)),
        if said == 1 { "" } else { "s" },
        def.imports.len(),
        if def.imports.len() == 1 { "" } else { "s" },
        def.rules.len(),
        if def.rules.len() == 1 { "" } else { "s" },
    ));
    for dark in [false, true] {
        out.push_str(&format!("<div class=\"mts-mode\"><h4>{}</h4><div class=\"mts-pair\">", if dark { "dark" } else { "light" }));
        out.push_str(&panel(&base, dark, DEFAULT_THEME));
        out.push_str(&panel(&theme, dark, &def.name));
        out.push_str("</div></div>");
    }
    out.push_str("<details><summary>every token</summary><table class=\"mts-tokens\"><thead><tr><th>token</th><th>group</th>");
    out.push_str(&format!("<th>default · light</th><th>{0} · light</th><th>default · dark</th><th>{0} · dark</th></tr></thead><tbody>", esc(&def.name)));
    for t in TOKENS.iter() {
        let (l, d) = (theme.value(t, false), theme.value(t, true));
        out.push_str(&format!("<tr><td>{}</td><td>{}</td>", t.name, esc(t.group)));
        out.push_str(&cell(t, t.light, false));
        out.push_str(&cell(t, &l, l != t.light));
        out.push_str(&cell(t, t.dark, false));
        out.push_str(&cell(t, &d, d != t.dark));
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></details></section>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_shows_the_theme_beside_default_in_both_modes_with_every_token() {
        let html = sheet("theme pantry", ":root { --accent: #d4572a; --font-display: \"Fraunces\", serif; }\n:root[data-theme=dark] { --accent: #f08a5d; }\nm-option::part(card) { border-radius: 14px; }");
        assert!(html.starts_with("<section class=\"mts\" data-menger-theme=\"pantry\">"), "{html}");
        assert_eq!(html.matches("class=\"mts-panel\"").count(), 4, "default and pantry, light and dark");
        assert!(html.contains("data-mts=\"pantry\" data-mode=\"light\"") && html.contains("data-mts=\"default\" data-mode=\"dark\""));
        assert!(html.contains("--accent:#d4572a") && html.contains("--accent:#f08a5d"), "the theme's values inline");
        assert!(html.contains("--font-display:&quot;Fraunces&quot;, serif"), "quotes escaped in the attribute");
        assert_eq!(html.matches("class=\"mts-chip\"").count(), 4 * 11, "every status, every panel");
        assert_eq!(html.matches("class=\"mts-kind\"").count(), 4 * 5);
        assert_eq!(html.matches("<m-decision").count(), 4);
        assert_eq!(html.matches("language-rust").count(), 4);
        assert!(html.contains(".mts-panel[data-mts=\"pantry\"] m-option::part(card){border-radius:14px;}"), "parts scoped to its panels");
        assert_eq!(html.matches("<tr><td>--").count(), TOKENS.len(), "every token in the table");
        assert!(html.contains("<td class=\"set\"><i class=\"mts-sw\" style=\"background:#d4572a\"></i>#d4572a</td>"));
    }

    #[test]
    fn a_block_that_is_not_a_theme_shows_its_refusal() {
        let html = sheet("theme pantry", ":root {\n  --nope: 1px;\n}");
        assert!(html.contains("class=\"mts-error\">```theme pantry: line 2: `--nope` is not a token"), "{html}");
        assert!(!html.contains("class=\"mts-panel\""));
    }
}
