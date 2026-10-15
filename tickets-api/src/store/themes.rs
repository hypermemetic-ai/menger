//! THEME-8: the **themes store** — every theme ([`crate::themes::Theme`]:
//! a name and its token overrides for light and dark) and what the site and
//! each shortcode say ([`ThemeSetting`]: the theme it adopts, how far it
//! reaches, whether it takes the themes above), kept beside the tickets.
//!
//! **Behind the cache, as everything stored is** (AGENTS.md): both tables
//! are read into the cache when the store opens ([`load`]) and every read
//! answers from it. A write — `define`, `set` — takes the write path every
//! write takes ([`super::Store::write`]): worked out under the write-serial
//! lock, committed in one transaction, put into the cache, then broadcast
//! on a channel of its own ([`super::Store::subscribe_themes`]), which no
//! ticket's write touches.
//!
//! **What a ticket says** is its body's frontmatter (`theme:`, `reach:`,
//! `takeover:`, `frame-*:`; [`crate::themes::declared`]) — read when it is
//! asked for, through the read every whole ticket takes (SCALE-14: a body
//! is never in the cache), never kept a second time. The shortcode's level
//! is the ticket's shortcode, else the nearest hosting it that says
//! anything (THEME-4's inheritance), read from the kept tree.
//!
//! **Stored** in two tables, `themes` (a row per theme, its maps as JSON)
//! and `theme_settings` (a row per scope that says anything — a setting
//! that says nothing is deleted). A file without them gains them, empty,
//! on open ([`migrate`]).

use super::{lineage, Authority, Store, StoreError};
use crate::address::ShortcodeId;
use crate::themes::{self, Levels, Regions, Takeover, Theme, ThemeReach, ThemeSetting, DEFAULT_THEME, SITE};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use tokio::sync::broadcast;

const TABLES: &str = "
        CREATE TABLE IF NOT EXISTS themes (
            name    TEXT PRIMARY KEY,
            light   TEXT NOT NULL,
            dark    TEXT NOT NULL,
            updated INTEGER NOT NULL,
            owner   TEXT,
            orphan  INTEGER NOT NULL DEFAULT 0,
            extends TEXT,
            rules   TEXT NOT NULL DEFAULT '[]',
            imports TEXT NOT NULL DEFAULT '[]'
        );
        CREATE TABLE IF NOT EXISTS theme_settings (
            scope    TEXT PRIMARY KEY,
            theme    TEXT,
            reach    TEXT NOT NULL,
            takeover TEXT NOT NULL
        );
    ";

/// A file without the themes tables gains them, empty, here — on every
/// open, a no-op once they are there.
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    let had: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('themes', 'theme_settings')",
        [],
        |r| r.get(0),
    )?;
    if had < 2 {
        conn.execute_batch(TABLES)?;
        tracing::warn!("THEME-8: the file gained the themes tables, empty, durably; every later open reads them");
    }
    Ok(())
}

/// A themes write, as `themes.watch` relays it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeChange {
    /// A theme as it is now: defined, its block written, orphaned or adopted.
    Changed(Theme),
    /// THEME-3: an orphan nothing uses left the registry.
    Removed(String),
    Set(ThemeSetting),
}

/// The themes as the cache holds them, and their channel.
#[derive(Debug, Clone)]
pub(crate) struct Themes {
    pub(super) by_name: BTreeMap<String, Theme>,
    /// By scope: `site`, or a shortcode (upper case).
    pub(super) settings: BTreeMap<String, ThemeSetting>,
    /// THEME-3: each theme a ticket's frontmatter names — the tickets.
    pub(super) refs: BTreeMap<String, std::collections::BTreeSet<String>>,
    pub(super) tx: broadcast::Sender<ThemeChange>,
}

impl Default for Themes {
    fn default() -> Self {
        Self { by_name: BTreeMap::new(), settings: BTreeMap::new(), refs: BTreeMap::new(), tx: broadcast::channel(super::BROADCAST_CAPACITY).0 }
    }
}

impl Themes {
    /// The setting the nearest of `epic` and the shortcodes hosting it
    /// (innermost first) holds.
    fn nearest(&self, chain: impl IntoIterator<Item = String>) -> Option<ThemeSetting> {
        chain.into_iter().find_map(|code| self.settings.get(&code).cloned())
    }
}

fn map_json(m: &BTreeMap<String, String>) -> String {
    serde_json::to_string(m).expect("a map of strings serializes")
}

fn json_map(s: &str) -> BTreeMap<String, String> {
    serde_json::from_str(s).unwrap_or_default()
}

/// Every theme and setting as stored — what a start reads. Kept beside the
/// channel the cache already has.
pub(super) fn load(conn: &Connection, into: &mut Themes) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT name, light, dark, updated, owner, orphan, extends, rules, imports FROM themes ORDER BY name")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let name: String = r.get(0)?;
        let list = |i: usize| -> rusqlite::Result<Vec<String>> { Ok(serde_json::from_str(&r.get::<_, String>(i)?).unwrap_or_default()) };
        let theme = Theme {
            name: name.clone(),
            light: json_map(&r.get::<_, String>(1)?),
            dark: json_map(&r.get::<_, String>(2)?),
            updated_unix: r.get(3)?,
            owner: r.get(4)?,
            orphan: r.get::<_, i64>(5)? != 0,
            extends: r.get(6)?,
            rules: list(7)?,
            imports: list(8)?,
        };
        into.by_name.insert(name, theme);
    }
    super::theme_registry::load_refs(conn, into)?;
    let mut stmt = conn.prepare("SELECT scope, theme, reach, takeover FROM theme_settings ORDER BY scope")?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let scope: String = r.get(0)?;
        let setting = ThemeSetting {
            scope: scope.clone(),
            theme: r.get(1)?,
            reach: ThemeReach::parse(&r.get::<_, String>(2)?).unwrap_or_default(),
            takeover: Takeover::parse(&r.get::<_, String>(3)?).unwrap_or_default(),
        };
        into.settings.insert(scope, setting);
    }
    Ok(())
}

pub(super) fn store_theme(conn: &Connection, t: &Theme) -> rusqlite::Result<()> {
    let list = |v: &Vec<String>| serde_json::to_string(v).expect("a list of strings serializes");
    conn.prepare_cached(
        "INSERT INTO themes (name, light, dark, updated, owner, orphan, extends, rules, imports) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (name) DO UPDATE SET light = excluded.light, dark = excluded.dark, updated = excluded.updated,
                 owner = excluded.owner, orphan = excluded.orphan, extends = excluded.extends, rules = excluded.rules, imports = excluded.imports",
    )?
    .execute(params![t.name, map_json(&t.light), map_json(&t.dark), t.updated_unix, t.owner, t.orphan as i64, t.extends, list(&t.rules), list(&t.imports)])?;
    Ok(())
}

fn store_setting(conn: &Connection, s: &ThemeSetting) -> rusqlite::Result<()> {
    if s.said().is_silent() {
        conn.prepare_cached("DELETE FROM theme_settings WHERE scope = ?1")?.execute(params![s.scope])?;
        return Ok(());
    }
    conn.prepare_cached(
        "INSERT INTO theme_settings (scope, theme, reach, takeover) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (scope) DO UPDATE SET theme = excluded.theme, reach = excluded.reach, takeover = excluded.takeover",
    )?
    .execute(params![s.scope, s.theme, s.reach.as_str(), s.takeover.as_str()])?;
    Ok(())
}

impl Store {
    /// THEME-8: defines the theme `name` — its overrides for light and dark,
    /// whole (a redefinition replaces both maps). A name that is not one
    /// ([`themes::name_of`]), `default` (which overrides nothing, always),
    /// a token that is not in [`themes::TOKENS`] or a value that could
    /// escape its declaration is [`StoreError::Invalid`], naming each, and
    /// nothing is kept.
    ///
    /// ACCESS-11: by `by` — a theme is the whole site's: the system, or
    /// editor (or admin) on `*`.
    pub async fn define_theme(&self, by: impl Into<Authority>, name: &str, light: BTreeMap<String, String>, dark: BTreeMap<String, String>) -> Result<Theme, StoreError> {
        by.into().editor_on(&crate::members::Scope::Everywhere, "a theme is the whole site's: defining one needs editor on *")?;
        let name = themes::name_of(name).map_err(StoreError::Invalid)?;
        if name == DEFAULT_THEME {
            return Err(StoreError::Invalid("`default` is the theme that overrides nothing; it cannot be defined".into()));
        }
        let light = themes::check_overrides("light", &light);
        let dark = themes::check_overrides("dark", &dark);
        let (light, dark) = match (light, dark) {
            (Ok(l), Ok(d)) => (l, d),
            (l, d) => {
                let why: Vec<String> = [l.err(), d.err()].into_iter().flatten().collect();
                return Err(StoreError::Invalid(why.join("; ")));
            }
        };
        let theme = Theme { name, light, dark, updated_unix: chrono::Utc::now().timestamp(), ..Theme::default() };
        let serial = self.serial().await;
        // THEME-3: a theme a ticket's block defines is that block's to change.
        if let Some(owner) = self.tickets.read().expect("store lock poisoned").themes.by_name.get(&theme.name).and_then(|t| t.owner.clone()) {
            return Err(StoreError::Invalid(format!("the theme `{}` is defined by {owner}'s ```theme block: change it there", theme.name)));
        }
        let (stored, kept) = (theme.clone(), theme.clone());
        self.write(
            serial,
            move |tx| Ok(store_theme(tx, &stored)?),
            move |cache, _| {
                cache.themes.by_name.insert(kept.name.clone(), kept);
                cache.themes.tx.clone()
            },
            move |(), tx, _| {
                let _ = tx.send(ThemeChange::Changed(theme.clone()));
                theme
            },
        )
        .await
    }

    /// THEME-8: every theme, by name (`default` first), from the cache.
    pub fn themes(&self) -> Vec<Theme> {
        let cache = self.tickets.read().expect("store lock poisoned");
        std::iter::once(Theme::default_theme()).chain(cache.themes.by_name.values().cloned()).collect()
    }

    /// THEME-8: the theme `name` (`default` is always there), from the cache.
    pub fn theme(&self, name: &str) -> Result<Theme, StoreError> {
        let name = themes::name_of(name).map_err(StoreError::Invalid)?;
        if name == DEFAULT_THEME {
            return Ok(Theme::default_theme());
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.themes.by_name.get(&name).cloned().ok_or_else(|| StoreError::Invalid(no_theme(&name, &cache.themes)))
    }

    /// THEME-8: what the site and every shortcode say — the site first,
    /// then by shortcode — from the cache. A scope that says nothing is not
    /// here.
    pub fn theme_settings(&self) -> Vec<ThemeSetting> {
        let cache = self.tickets.read().expect("store lock poisoned");
        let s = &cache.themes.settings;
        s.get(SITE).into_iter().chain(s.iter().filter(|(k, _)| *k != SITE).map(|(_, v)| v)).cloned().collect()
    }

    /// THEME-8: `scope` — `site`, or a shortcode that holds a ticket — says
    /// `theme` (`default` or none: adopts none), `reach` and `takeover`
    /// (each its default when not given), replacing what it said. The site
    /// adopts or not: it has no reach beyond itself and nothing above it to
    /// opt out of, so either is [`StoreError::Invalid`]; so is a theme not
    /// defined, or a shortcode that is none. Saying nothing clears it.
    ///
    /// ACCESS-11: by `by` — the system; for the site, admin on `*`; for a
    /// shortcode, editor (or admin) on exactly its scope.
    pub async fn set_theme(&self, by: impl Into<Authority>, scope: &str, theme: Option<&str>, reach: Option<ThemeReach>, takeover: Option<Takeover>) -> Result<ThemeSetting, StoreError> {
        let by = by.into();
        let raw = scope.trim();
        let site = raw.eq_ignore_ascii_case(SITE);
        // ORG-3: a shortcode by any short form of its address, kept by its
        // identity (its code, for every shortcode before ORG-3: the settings
        // stored then name it already); one naming several is refused.
        let scope = match site {
            true => SITE.to_string(),
            false => {
                let cache = self.tickets.read().expect("store lock poisoned");
                match cache.shortcode_named(raw)? {
                    Some(sc) => sc.as_str().to_string(),
                    None => raw.to_ascii_uppercase(),
                }
            }
        };
        let theme = match theme.map(str::trim).filter(|t| !t.is_empty()) {
            None => None,
            Some(t) => Some(themes::name_of(t).map_err(StoreError::Invalid)?).filter(|t| t != DEFAULT_THEME),
        };
        if site && (reach.is_some_and(|r| r != ThemeReach::Ticket) || takeover == Some(Takeover::Off)) {
            return Err(StoreError::Invalid("the site adopts a theme or none: it has no reach beyond itself and nothing above it to turn off".into()));
        }
        let setting = ThemeSetting { scope, theme, reach: reach.unwrap_or_default(), takeover: takeover.unwrap_or_default() };
        let serial = self.serial().await;
        {
            let cache = self.tickets.read().expect("store lock poisoned");
            if let Some(t) = setting.theme.as_deref().filter(|t| !cache.themes.by_name.contains_key(*t)) {
                return Err(StoreError::Invalid(no_theme(t, &cache.themes)));
            }
            if !site && !cache.rollups.groups().iter().any(|g| g.shortcode.as_str() == setting.scope) {
                return Err(StoreError::Invalid(format!("no shortcode {}: a theme is set on `site` or on a shortcode that holds a ticket", setting.scope)));
            }
            match site {
                true => by.admin_on(&crate::members::Scope::Everywhere, "the site's theme is everyone's: setting it needs admin on *")?,
                false => by.editor_on(&super::writs::scope_of(&cache, &ShortcodeId::new(&setting.scope)), "a shortcode's theme needs editor on it")?,
            }
        }
        // THEME-3: an orphan only this scope's old setting kept goes with it.
        let gone = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let mut named = super::theme_registry::set_names(&cache.themes);
            if let Some(old) = cache.themes.settings.get(&setting.scope).and_then(|s| s.theme.clone()) {
                if !cache.themes.settings.iter().any(|(k, s)| *k != setting.scope && s.theme.as_deref() == Some(old.as_str())) {
                    named.remove(&old);
                }
            }
            named.extend(setting.theme.clone());
            let tickets: std::collections::BTreeSet<String> = cache.themes.refs.keys().cloned().collect();
            super::theme_registry::unused(&cache.themes.by_name, &tickets, &named)
        };
        let moves = super::theme_registry::Moves { themes: gone.into_iter().map(|n| (n, None)).collect(), refs: None };
        let (stored, kept, rows) = (setting.clone(), setting.clone(), moves.clone());
        self.write(
            serial,
            move |tx| {
                store_setting(tx, &stored)?;
                Ok(super::theme_registry::store(tx, &rows)?)
            },
            move |cache, _| {
                if kept.said().is_silent() {
                    cache.themes.settings.remove(&kept.scope);
                } else {
                    cache.themes.settings.insert(kept.scope.clone(), kept);
                }
                let tx = cache.themes.tx.clone();
                super::theme_registry::take(&mut cache.themes, moves);
                tx
            },
            move |(), tx, _| {
                let _ = tx.send(ThemeChange::Set(setting.clone()));
                setting
            },
        )
        .await
    }

    /// THEME-8: what every level says of the ticket `id` — the site's theme
    /// and the nearest shortcode's setting from the cache, the ticket's and
    /// its frame's from its body's frontmatter (read as `get` reads it) —
    /// and who wears what ([`themes::resolve`], no reader). A ticket that is
    /// none — ACCESS-10: or one `who` may not see — is
    /// [`StoreError::NotFound`].
    pub async fn theme_for(&self, who: &(impl super::Reader + ?Sized), id: &str) -> Result<(String, Regions, Levels), StoreError> {
        let id = id.trim();
        let ticket = self.get(who, &if id.contains('/') { id.to_string() } else { id.to_ascii_uppercase() }).await?;
        let declared = themes::declared(&ticket.body);
        let levels = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let chain = std::iter::once(ticket.shortcode.clone())
                .chain(lineage::host_chain(&cache, &ticket.shortcode))
                .map(|sc| sc.as_str().to_string());
            Levels {
                site: cache.themes.settings.get(SITE).and_then(|s| s.theme.clone()),
                shortcode: cache.themes.nearest(chain),
                ticket: declared.ticket,
                frame: declared.frame,
            }
        };
        let regions = themes::resolve(&levels, &themes::Reader::default());
        Ok((String::from(ticket.id), regions, levels))
    }

    /// THEME-8: `themes.watch`'s channel — every define and set, in the
    /// order they were acknowledged.
    pub fn subscribe_themes(&self) -> broadcast::Receiver<ThemeChange> {
        self.tickets.read().expect("store lock poisoned").themes.tx.subscribe()
    }
}

/// A theme that is not defined, said with the ones that are.
fn no_theme(name: &str, themes: &Themes) -> String {
    let known: Vec<&str> = std::iter::once(DEFAULT_THEME).chain(themes.by_name.keys().map(String::as_str)).collect();
    format!("no theme `{name}` (defined: {})", known.join(", "))
}
