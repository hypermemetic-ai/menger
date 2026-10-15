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

pub use crate::tokens::{token, Token, TOKENS};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
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

/// ACCESS-8: read as [`ThemeReach::parse`] reads, so a parameter and the
/// wire take every spelling it takes.
impl std::str::FromStr for ThemeReach {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Self::parse(s)
    }
}

/// ACCESS-8: read as [`Takeover::parse`] reads.
impl std::str::FromStr for Takeover {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Self::parse(s)
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
/// name (`--accent`), for light and for dark — what it says itself; the
/// rest comes from the theme it `extends` ([`flatten`]). THEME-3: a theme
/// is a ticket's ` ```theme ` block ([`crate::theme_block`]), owned as a
/// component is (COMP-3's rules).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Theme {
    pub name: String,
    pub light: BTreeMap<String, String>,
    pub dark: BTreeMap<String, String>,
    /// When it was last defined, unix seconds (0 for `default`).
    pub updated_unix: i64,
    /// THEME-3: the theme it states its differences from (`None`: `default`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// THEME-3: the ticket whose body's block defines it — `None` for one
    /// `themes.define` made, or an orphan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// THEME-3: its block is gone from its owner's body; it is kept,
    /// read-only, while something uses it (a setting, a ticket's
    /// frontmatter, a live theme extending it), and a block naming it adopts it.
    #[serde(default)]
    pub orphan: bool,
    /// THEME-3: its `@font-face` and `m-*::part(…)` rules, as validated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    /// THEME-3: the https stylesheets it imports (a font's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
}

impl Theme {
    /// `default`: nothing overridden.
    pub fn default_theme() -> Self {
        Theme { name: DEFAULT_THEME.into(), ..Default::default() }
    }

    /// What `token` paints under this theme in `dark` or light: the mode's
    /// override; in dark, else the light override when the token is not a
    /// colour (THEME-2: a face, a size, a radius holds in both modes); else
    /// the token's default.
    pub fn value(&self, token: &Token, dark: bool) -> String {
        let own = if dark { &self.dark } else { &self.light };
        if let Some(v) = own.get(token.name) {
            return v.clone();
        }
        if dark && !token.is_colour() {
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

/// THEME-3: `name` as it paints — its own overrides over its parent's,
/// up the `extends` chain to `default` (a parent that is not held, or a
/// cycle, ends the chain there); its rules and imports the parent's first.
/// `lookup` answers a theme by name.
pub fn flatten<'a>(name: &str, lookup: impl Fn(&str) -> Option<&'a Theme>) -> Theme {
    let mut chain: Vec<&Theme> = Vec::new();
    let mut at = Some(name.to_string());
    while let Some(n) = at.take() {
        if n == DEFAULT_THEME || chain.iter().any(|t| t.name == n) {
            break;
        }
        let Some(t) = lookup(&n) else { break };
        chain.push(t);
        at = t.extends.clone();
    }
    let Some(own) = chain.first() else {
        return Theme { name: name.to_string(), ..Theme::default() };
    };
    let mut out = Theme { name: own.name.clone(), updated_unix: own.updated_unix, extends: own.extends.clone(), owner: own.owner.clone(), orphan: own.orphan, ..Theme::default() };
    for t in chain.iter().rev() {
        // A non-colour said for light holds in dark: a nearer theme saying
        // it for light only drops what a farther one said for dark.
        for (k, v) in &t.light {
            if token(k).is_some_and(|tk| !tk.is_colour()) && !t.dark.contains_key(k) {
                out.dark.remove(k);
            }
            out.light.insert(k.clone(), v.clone());
        }
        out.dark.extend(t.dark.iter().map(|(k, v)| (k.clone(), v.clone())));
        out.rules.extend(t.rules.iter().cloned());
        out.imports.extend(t.imports.iter().filter(|u| !out.imports.contains(u)).cloned().collect::<Vec<_>>());
    }
    out
}

/// THEME-3: the CSS a theme's rules and imports make — imports first (CSS
/// takes them nowhere else), then its `@font-face`s, then its `::part`
/// rules, each `::part` rule scoped under `scope` when one is given (a
/// swatch sheet's panel).
pub fn rules_css(theme: &Theme, scope: Option<&str>) -> String {
    let mut css: String = theme.imports.iter().map(|u| format!("@import url(\"{u}\");")).collect();
    for r in &theme.rules {
        match scope.filter(|_| !r.starts_with('@')) {
            Some(sc) => {
                let (sel, decls) = r.split_at(r.find('{').unwrap_or(0));
                let scoped: Vec<String> = sel.split(',').map(|s| format!("{sc} {s}")).collect();
                css.push_str(&format!("{}{decls}", scoped.join(",")));
            }
            None => css.push_str(r),
        }
    }
    css
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
    /// THEME-4: project themes off — no shortcode's setting reaches them
    /// (the shortcode level says nothing); the site's, a ticket's and a
    /// frame's still do.
    pub projects_off: bool,
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
    let shortcode = levels.shortcode.as_ref().filter(|_| !reader.projects_off).map(|s| (s.said(), Some(s.scope.clone())));
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
    /// A theme, as it is now (`define`, `get`, `list`).
    Theme { theme: Theme },
    /// THEME-3 (`watch`): a theme defined, its block written or amended,
    /// orphaned or adopted — as it is now.
    ThemeChanged { theme: Theme },
    /// THEME-3 (`watch`): a theme left the registry — an orphan nothing
    /// uses any more.
    ThemeRemoved { name: String },
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
