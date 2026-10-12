//! THEME-8: the themes vocabulary — what a theme is, the levels that adopt
//! one, and the one rule that decides who wears what. Wasm-safe, like
//! [`crate::notes`]: the server answers `themes.theme_for` with
//! [`resolve`], and the app runs the same function with the reader's own
//! choices on top, so the app, a frame and synapse agree.
//!
//! **A theme** (THEME-8's minimal notion; THEME-3 replaces how one is
//! written, not what it is) is a name and a map of token overrides over the
//! app's tokens ([`TOKENS`]), one map for light and one for dark. `default`
//! is the empty map: today's look. A token the theme sets for light only
//! also holds in dark when the token has one value in both (a face); a
//! colour set for light only leaves dark's default ([`Theme::value`]).
//!
//! **The levels**, outermost first: the **reader** (their own site theme,
//! and "turn off every takeover" — kept in the app's settings, never on the
//! server), the **site** (the shell, nav, board, Home — with or without a
//! ticket open), the **shortcode** (THEME-4: the ticket's, or the nearest
//! one hosting it that says anything), the **ticket view** (its header,
//! properties and body) and the **frame** (one body, or one component). A
//! level may **adopt** a theme, with a [`ThemeReach`] — `frame` (only the
//! frames inside it), `ticket` (itself and everything inside; the default)
//! or `site` (and the site, while it is the open ticket) — and may say
//! `takeover: off`: it and everything inside ignore the themes above and
//! wear `default`, or their own adoption. **The nearest level wins**
//! ([`resolve`]); a component takes its frame's theme.
//!
//! **A ticket says it in its body's frontmatter** ([`declared`]), like
//! MENGER-15's `view:`:
//!
//! ```yaml
//! ---
//! theme: pantry          # the ticket view adopts it (and its frame)
//! reach: ticket          # frame | ticket | site
//! takeover: off          # ignore every theme from above
//! frame-theme: ocean     # its frame alone adopts one
//! frame-takeover: off    # its frame alone ignores the themes above
//! ---
//! ```
//!
//! `reach: frame` is `frame-theme:` said the other way. The site and a
//! shortcode say it with `themes.set`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// The theme that overrides nothing.
pub const DEFAULT_THEME: &str = "default";

/// `themes.set`'s scope for the site.
pub const SITE: &str = "site";

/// The longest theme name, in bytes.
pub const NAME_MAX: usize = 40;

/// The longest token value a theme keeps, in bytes.
pub const VALUE_MAX: usize = 200;

/// One token of the contract: its name and its default in light and dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub name: &'static str,
    pub light: &'static str,
    pub dark: &'static str,
}

const fn t(name: &'static str, light: &'static str, dark: &'static str) -> Token {
    Token { name, light, dark }
}

const MONO: &str = r#"ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace"#;
const SANS: &str = r#"-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif"#;

/// THEME-8: the tokens a theme may set — every token of index.html's
/// `:root` (its light block, its dark blocks: a native test in the app holds
/// the two equal), each with its default in light and dark. The one list:
/// the server refuses a theme naming anything else, a frame receives
/// exactly these (frame.rs), and a region is painted with them.
pub const TOKENS: &[Token] = &[
    t("--bg", "#fbfbf9", "#101014"),
    t("--panel", "#f4f3ee", "#17171d"),
    t("--card", "#fff", "#1e1e26"),
    t("--ink", "#1f2328", "#d8d8e0"),
    t("--muted", "#8a8f98", "#7a7a88"),
    t("--line", "#e3e1d8", "#26262e"),
    t("--accent", "#0066cc", "#8ab4f8"),
    t("--hover", "#ecebe4", "#1f1f28"),
    t("--row-sel", "#cfe2f7", "#1f2d44"),
    t("--sel", "#e7eef7", "#1b2431"),
    t("--chip-bg", "#eef2f6", "#1e2430"),
    t("--chip-fg", "#5b6b7b", "#aab7c8"),
    t("--chip-line", "#dde3ea", "#2c3442"),
    t("--tree-line", "#c4c2b6", "#3a3a46"),
    t("--live", "#256b1a", "#5dbb63"),
    t("--err-bg", "#fdecec", "#2b1517"),
    t("--err-fg", "#a23", "#e89393"),
    t("--err-line", "#f3c5c5", "#5a2626"),
    t("--scrim", "rgba(31, 35, 40, .34)", "rgba(0, 0, 0, .55)"),
    t("--shadow", "#0003", "#0008"),
    t("--kind-scope", "#7a5d00", "#e3c56a"),
    t("--kind-build", "#0066cc", "#8ab4f8"),
    t("--kind-execution", "#256b1a", "#5dbb63"),
    t("--kind-gap", "#a23", "#e89393"),
    t("--kind-spike", "#5b3a8c", "#c7abf2"),
    t("--annot", "rgba(0, 102, 204, .32)", "rgba(138, 180, 248, .32)"),
    t("--annot-hi", "#0066cc", "#8ab4f8"),
    t("--mono", MONO, MONO),
    t("--sans", SANS, SANS),
    // MENGER-22: code, highlighted — highlight.js's classes, coloured by these.
    t("--code-keyword", "#cf222e", "#ff7b72"),
    t("--code-string", "#0a3069", "#a5d6ff"),
    t("--code-comment", "#656d76", "#8b949e"),
    t("--code-number", "#0550ae", "#79c0ff"),
    t("--code-title", "#8250df", "#d2a8ff"),
    t("--code-type", "#953800", "#ffa657"),
    t("--code-meta", "#116329", "#7ee787"),
    t("--code-builtin", "#6639ba", "#e3b341"),
    t("--code-attr", "#0550ae", "#79c0ff"),
    t("--code-variable", "#953800", "#ffa657"),
];

/// The token `name` names — with or without its leading `--`, any ASCII
/// case — or `None` when it is not one of [`TOKENS`].
pub fn token(name: &str) -> Option<&'static Token> {
    let n = name.trim().trim_start_matches("--").to_ascii_lowercase();
    TOKENS.iter().find(|t| t.name[2..] == n)
}

/// A value a token may take: not blank, at most [`VALUE_MAX`] bytes, and
/// nothing that could end its declaration, rule or `<style>` early.
pub fn value_ok(value: &str) -> bool {
    let v = value.trim();
    !v.is_empty() && v.len() <= VALUE_MAX && !v.contains(['<', '>', '{', '}', ';', '\\', '\n', '\r'])
}

/// A theme's name: lowercase ASCII letters, digits and `-`, starting with a
/// letter, at most [`NAME_MAX`] bytes. Spelled any case on the way in.
pub fn name_of(raw: &str) -> Result<String, String> {
    let n = raw.trim().trim_matches(['"', '\'']).to_ascii_lowercase();
    let ok = !n.is_empty()
        && n.len() <= NAME_MAX
        && n.starts_with(|c: char| c.is_ascii_lowercase())
        && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(n)
    } else {
        Err(format!(
            "`{}` is not a theme name: lowercase letters, digits and -, starting with a letter, at most {NAME_MAX}",
            raw.trim()
        ))
    }
}

/// How far an adoption reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThemeReach {
    /// Only the frames inside the level that adopts it.
    Frame,
    /// The level itself and everything inside it (the default).
    #[default]
    Ticket,
    /// And the site, while the level is open (a ticket, or a ticket of the
    /// shortcode).
    Site,
}

impl ThemeReach {
    pub const fn as_str(self) -> &'static str {
        match self {
            ThemeReach::Frame => "frame",
            ThemeReach::Ticket => "ticket",
            ThemeReach::Site => "site",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().trim_matches(['"', '\'']).to_ascii_lowercase().as_str() {
            "frame" => Ok(Self::Frame),
            "ticket" => Ok(Self::Ticket),
            "site" => Ok(Self::Site),
            other => Err(format!("unknown reach '{other}' (expected frame, ticket or site)")),
        }
    }
}

/// Whether a level takes the themes from above (`on`, the default) or
/// ignores them (`off`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Takeover {
    #[default]
    On,
    Off,
}

impl Takeover {
    pub const fn as_str(self) -> &'static str {
        match self {
            Takeover::On => "on",
            Takeover::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().trim_matches(['"', '\'']).to_ascii_lowercase().as_str() {
            "on" | "true" | "yes" => Ok(Self::On),
            "off" | "false" | "no" => Ok(Self::Off),
            other => Err(format!("unknown takeover '{other}' (expected on or off)")),
        }
    }
}

/// The level a region's theme came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThemeLevel {
    /// Nothing says anything: `default`.
    None,
    Reader,
    Site,
    Shortcode,
    Ticket,
    Frame,
}

impl ThemeLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            ThemeLevel::None => "none",
            ThemeLevel::Reader => "reader",
            ThemeLevel::Site => "site",
            ThemeLevel::Shortcode => "shortcode",
            ThemeLevel::Ticket => "ticket",
            ThemeLevel::Frame => "frame",
        }
    }
}

/// A theme as the server keeps it: its name and its overrides, by token
/// name (`--accent`), for light and for dark.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Theme {
    pub name: String,
    pub light: BTreeMap<String, String>,
    pub dark: BTreeMap<String, String>,
    /// When it was last defined, unix seconds (0 for `default`).
    pub updated_unix: i64,
}

impl Theme {
    /// `default`: nothing overridden.
    pub fn default_theme() -> Self {
        Theme { name: DEFAULT_THEME.into(), ..Default::default() }
    }

    /// What `token` paints under this theme in `dark` or light: the mode's
    /// override; in dark, else the light override when the token has one
    /// default for both (a face); else the token's default.
    pub fn value(&self, token: &Token, dark: bool) -> String {
        let own = if dark { &self.dark } else { &self.light };
        if let Some(v) = own.get(token.name) {
            return v.clone();
        }
        if dark && token.light == token.dark {
            if let Some(v) = self.light.get(token.name) {
                return v.clone();
            }
        }
        (if dark { token.dark } else { token.light }).to_string()
    }

    /// Every token, as this theme paints it in `dark` or light.
    pub fn painted(&self, dark: bool) -> Vec<(&'static str, String)> {
        TOKENS.iter().map(|t| (t.name, self.value(t, dark))).collect()
    }
}

/// `define`'s maps, checked: each key a token of [`TOKENS`] (written
/// canonically, `--name`), each value [`value_ok`]. `Err` names every key
/// and value refused.
pub fn check_overrides(mode: &str, raw: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut bad = Vec::new();
    for (k, v) in raw {
        match token(k) {
            None => bad.push(format!("{mode}: `{k}` is not a token (THEME-8: one of {})", TOKENS.iter().map(|t| t.name).collect::<Vec<_>>().join(" "))),
            Some(_) if !value_ok(v) => bad.push(format!("{mode}: `{k}`'s value `{}` is not a token's value (not blank, at most {VALUE_MAX} bytes, no < > {{ }} ; \\ or line break)", v.trim())),
            Some(t) => {
                out.insert(t.name.to_string(), v.trim().to_string());
            }
        }
    }
    if bad.is_empty() {
        Ok(out)
    } else {
        Err(bad.join("; "))
    }
}

/// What one level says.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Said {
    /// The theme it adopts, if any.
    pub theme: Option<String>,
    pub reach: ThemeReach,
    pub takeover: Takeover,
}

impl Said {
    /// Says nothing: adopts none and takes what comes from above.
    pub fn is_silent(&self) -> bool {
        self.theme.is_none() && self.takeover == Takeover::On
    }
}

/// What the site or a shortcode says, as `themes.set` keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ThemeSetting {
    /// `site`, or a shortcode (upper case).
    pub scope: String,
    pub theme: Option<String>,
    pub reach: ThemeReach,
    pub takeover: Takeover,
}

impl ThemeSetting {
    pub fn said(&self) -> Said {
        Said { theme: self.theme.clone(), reach: self.reach, takeover: self.takeover }
    }
}

/// Everything the levels say for one ticket, the reader aside — what
/// [`resolve`] decides from.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Levels {
    /// The site's theme (`None`: default).
    pub site: Option<String>,
    /// The nearest shortcode that says anything — the ticket's own, else
    /// the nearest hosting it (THEME-4) — and what it says.
    pub shortcode: Option<ThemeSetting>,
    /// What the ticket's frontmatter says of its view.
    pub ticket: Said,
    /// What it says of its frame alone.
    pub frame: Said,
}

/// The reader's own choices — the outermost level, kept by the app.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reader {
    /// Their site theme, in place of the site's.
    pub site: Option<String>,
    /// Every takeover off: they wear their own site theme (or `default`)
    /// everywhere, whatever the site, a shortcode, a ticket or a frame says.
    pub takeovers_off: bool,
}

/// What a region wears, and the level it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Worn {
    pub theme: String,
    pub level: ThemeLevel,
    /// The shortcode, for a shortcode's theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl Worn {
    fn of(theme: &str, level: ThemeLevel) -> Self {
        Worn { theme: theme.to_string(), level, from: None }
    }

    fn none() -> Self {
        Worn::of(DEFAULT_THEME, ThemeLevel::None)
    }
}

/// Who wears what: the site, the ticket view, the frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Regions {
    pub site: Worn,
    pub ticket: Worn,
    pub frame: Worn,
}

/// THEME-8's rule. The nearest level wins: a region wears its own level's
/// adoption, else what the level outside it wears — unless a level says
/// `takeover: off`, and then from there inward it is `default` (or that
/// level's own adoption). A `reach: frame` adoption paints only the frames
/// inside its level; a `reach: site` one (a ticket's, else its shortcode's —
/// the innermost wins) paints the site while the ticket is open. The reader
/// is outermost: their site theme stands in for the site's, and with every
/// takeover off they wear it — or `default` — everywhere, whatever the
/// site says. No ticket open is [`Levels`] with the site alone.
pub fn resolve(levels: &Levels, reader: &Reader) -> Regions {
    let site_theme = levels.site.as_deref().filter(|_| !reader.takeovers_off);
    let base = match (reader.site.as_deref(), site_theme) {
        (Some(r), _) => Worn::of(r, ThemeLevel::Reader),
        (None, Some(s)) => Worn::of(s, ThemeLevel::Site),
        (None, None) => Worn::none(),
    };
    if reader.takeovers_off {
        return Regions { site: base.clone(), ticket: base.clone(), frame: base };
    }
    let shortcode = levels.shortcode.as_ref().map(|s| (s.said(), Some(s.scope.clone())));
    let steps = [
        (ThemeLevel::Shortcode, shortcode.unwrap_or_default()),
        (ThemeLevel::Ticket, (levels.ticket.clone(), None)),
    ];
    let mut view = base.clone();
    let mut frame_only: Option<Worn> = None;
    let mut site = base.clone();
    for (level, (said, from)) in steps {
        let worn = |theme: &str| Worn { theme: theme.to_string(), level, from: from.clone() };
        if said.takeover == Takeover::Off {
            view = worn(DEFAULT_THEME);
            frame_only = None;
        }
        if let Some(theme) = said.theme.as_deref() {
            match said.reach {
                ThemeReach::Frame => frame_only = Some(worn(theme)),
                ThemeReach::Ticket => {
                    view = worn(theme);
                    frame_only = None;
                }
                ThemeReach::Site => {
                    view = worn(theme);
                    frame_only = None;
                    site = worn(theme);
                }
            }
        }
    }
    let mut frame = frame_only.unwrap_or_else(|| view.clone());
    if levels.frame.takeover == Takeover::Off {
        frame = Worn::of(DEFAULT_THEME, ThemeLevel::Frame);
    }
    if let Some(theme) = levels.frame.theme.as_deref() {
        frame = Worn::of(theme, ThemeLevel::Frame);
    }
    Regions { site, ticket: view, frame }
}

/// What a body's frontmatter says: of the ticket view, and of its frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Declared {
    pub ticket: Said,
    pub frame: Said,
}

/// A frontmatter's `key: value`, for a top-level key — present.rs's
/// reading of `view:`, a trailing ` # comment` dropped.
fn fm_value<'a>(fm: &'a str, key: &str) -> Option<&'a str> {
    fm.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        (k == key).then(|| v.split(" #").next().unwrap_or(v).trim())
    })
}

/// What a ticket's body declares ([`Declared`]), from its frontmatter:
/// `theme:`, `reach:`, `takeover:`, `frame-theme:`, `frame-takeover:`.
/// Anything unreadable is not said. `reach: frame` moves `theme:` to the
/// frame (unless `frame-theme:` names one).
pub fn declared(body: &str) -> Declared {
    let Some(fm) = crate::fields::split_frontmatter(body).0 else { return Declared::default() };
    declared_in(fm)
}

/// [`declared`], from the frontmatter alone (split off already).
pub fn declared_in(fm: &str) -> Declared {
    let theme = |key| fm_value(fm, key).and_then(|v| name_of(v).ok());
    let takeover = |key| fm_value(fm, key).and_then(|v| Takeover::parse(v).ok()).unwrap_or_default();
    let reach = fm_value(fm, "reach").and_then(|v| ThemeReach::parse(v).ok()).unwrap_or_default();
    let mut ticket = Said { theme: theme("theme"), reach, takeover: takeover("takeover") };
    let mut frame = Said { theme: theme("frame-theme"), reach: ThemeReach::Frame, takeover: takeover("frame-takeover") };
    if ticket.reach == ThemeReach::Frame {
        if frame.theme.is_none() {
            frame.theme = ticket.theme.take();
        } else {
            ticket.theme = None;
        }
        ticket.reach = ThemeReach::Ticket;
    }
    Declared { ticket, frame }
}

/// What the `themes` activation streams. Every method returns
/// `impl Stream<Item = ThemesEvent>`; a refusal is the in-band `Error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)] // `regions`, once per `theme_for`: not worth a box on the wire's type
pub enum ThemesEvent {
    /// A theme, as it is now (`define`, `get`, `list`; `watch` when one is
    /// defined).
    Theme { theme: Theme },
    /// What the site or a shortcode says, as it is now (`set`, `list`;
    /// `watch` when it moves). Saying nothing — no theme, takeover on — is
    /// the setting cleared.
    Setting { setting: ThemeSetting },
    /// `theme_for`'s answer: each region's theme and the level it came
    /// from, and what every level said.
    Regions { id: String, regions: Regions, levels: Levels },
    /// A slow `watch` lost `missed` changes: `list` again.
    Lagged { missed: u64 },
    /// In-band refusal, naming the problem; nothing was written.
    Error { message: String },
}

impl fmt::Display for ThemeReach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Takeover {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
#[path = "themes_tests.rs"]
mod tests;
