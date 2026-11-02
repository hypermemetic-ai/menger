//! **The plan** — the defaults a fresh `quota.db` starts from, as data: a
//! default cap per source, how many reservations of a source may be open at
//! once, the box guard's two lines, how long a reservation is held. The
//! built-in plan is `plans/free-1k.json` (the owner's answer to V2-31's
//! `defaults`); `--quota-plan FILE` gives another. A plan is read once, into
//! a fresh file — after that the file's own rows are the truth, and
//! `quota.set` moves them.

use crate::sources;
use serde::Deserialize;
use std::collections::BTreeMap;

/// The plan built in: `plans/free-1k.json`.
pub const BUILT_IN: &str = include_str!("../plans/free-1k.json");

/// A cap: a number of the source's unit, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cap {
    /// At most this many.
    At(u64),
    /// No cap; usage is still counted and shown.
    Unlimited,
}

impl Cap {
    /// As stored: `NULL` for unlimited.
    pub fn stored(self) -> Option<i64> {
        match self {
            Cap::At(n) => Some(n.min(i64::MAX as u64) as i64),
            Cap::Unlimited => None,
        }
    }

    pub fn of_stored(v: Option<i64>) -> Self {
        match v {
            Some(n) => Cap::At(n.max(0) as u64),
            None => Cap::Unlimited,
        }
    }

    /// On the wire: the number, or `None` for unlimited.
    pub fn number(self) -> Option<u64> {
        match self {
            Cap::At(n) => Some(n),
            Cap::Unlimited => None,
        }
    }

    /// The larger of two (unlimited is the largest).
    pub fn max(self, other: Cap) -> Cap {
        match (self, other) {
            (Cap::Unlimited, _) | (_, Cap::Unlimited) => Cap::Unlimited,
            (Cap::At(a), Cap::At(b)) => Cap::At(a.max(b)),
        }
    }
}

impl std::fmt::Display for Cap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Cap::At(n) => write!(f, "{n}"),
            Cap::Unlimited => f.write_str("unlimited"),
        }
    }
}

/// Reads a cap: `unlimited`, a whole number, or a number with a unit —
/// `KB`, `MB`, `GB`, `TB` (powers of 1000) or `KiB`, `MiB`, `GiB`, `TiB`
/// (powers of 1024); `2 GB`, `200MB`, `1.5GiB`, `300`.
pub fn parse_cap(text: &str) -> Result<Cap, String> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("unlimited") || t.eq_ignore_ascii_case("none") {
        return Ok(Cap::Unlimited);
    }
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '_' || c == ',')).unwrap_or(t.len());
    let (num, unit) = (t[..split].replace(['_', ','], ""), t[split..].trim());
    let mult: u64 = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" | "k" => 1_000,
        "mb" | "m" => 1_000_000,
        "gb" | "g" => 1_000_000_000,
        "tb" | "t" => 1_000_000_000_000,
        "kib" => 1 << 10,
        "mib" => 1 << 20,
        "gib" => 1 << 30,
        "tib" => 1 << 40,
        _ => return Err(format!("`{t}` is not a cap: unlimited, a number, or a number with KB, MB, GB, TB (or KiB … TiB)")),
    };
    let n: f64 = num.parse().map_err(|_| format!("`{t}` is not a cap: unlimited, a number, or a number with a unit (2 GB)"))?;
    if !n.is_finite() || n < 0.0 {
        return Err(format!("`{t}` is not a cap: it must be 0 or more"));
    }
    Ok(Cap::At((n * mult as f64).round() as u64))
}

/// The box guard's two lines, in percent of the data disk used.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct BoxLines {
    /// Admins are warned from here (`quota.table`'s `box`, a log line).
    pub warn_percent: f64,
    /// Writes by anyone capped are refused `box_full` from here.
    pub full_percent: f64,
}

impl Default for BoxLines {
    fn default() -> Self {
        Self { warn_percent: 70.0, full_percent: 85.0 }
    }
}

/// A plan, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub name: String,
    /// The default cap per source; a source not named is unlimited.
    pub default: BTreeMap<String, Cap>,
    /// At most this many reservations of a source open at once, per principal.
    pub concurrent: BTreeMap<String, u32>,
    pub box_lines: BoxLines,
    /// How long a reservation is held before it is swept (an abandoned upload).
    pub hold_secs: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    said: Option<String>,
    #[serde(default)]
    default: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    concurrent: BTreeMap<String, u32>,
    #[serde(default, rename = "box")]
    box_lines: Option<BoxLines>,
    #[serde(default)]
    hold_secs: Option<i64>,
}

impl Plan {
    /// The plan in `text` (JSON); every source it names must be one, and
    /// every cap read.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: Raw = serde_json::from_str(text).map_err(|e| format!("the plan: {e}"))?;
        let known = |s: &str| sources::source(s).map(|_| ()).ok_or_else(|| format!("the plan names `{s}`, which is no source (the sources: {})", sources::names()));
        let mut default = BTreeMap::new();
        for (s, v) in raw.default {
            known(&s)?;
            let cap = match &v {
                serde_json::Value::Number(n) => n.as_u64().map(Cap::At).ok_or_else(|| format!("the plan's {s}: {n} is not a whole number"))?,
                serde_json::Value::String(t) => parse_cap(t).map_err(|e| format!("the plan's {s}: {e}"))?,
                other => return Err(format!("the plan's {s}: {other} is not a cap")),
            };
            default.insert(s, cap);
        }
        for s in raw.concurrent.keys() {
            known(s)?;
        }
        let box_lines = raw.box_lines.unwrap_or_default();
        if !(0.0..=100.0).contains(&box_lines.warn_percent) || !(0.0..=100.0).contains(&box_lines.full_percent) {
            return Err("the plan's box lines are percents, 0 to 100".into());
        }
        Ok(Plan { name: raw.name, default, concurrent: raw.concurrent, box_lines, hold_secs: raw.hold_secs.unwrap_or(3600).clamp(60, 7 * 86_400) })
    }

    /// The plan built in (`plans/free-1k.json`).
    pub fn built_in() -> Self {
        Self::parse(BUILT_IN).expect("the built-in plan reads")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_read_with_and_without_units() {
        assert_eq!(parse_cap("unlimited"), Ok(Cap::Unlimited));
        assert_eq!(parse_cap("300"), Ok(Cap::At(300)));
        assert_eq!(parse_cap("2 GB"), Ok(Cap::At(2_000_000_000)));
        assert_eq!(parse_cap("200MB"), Ok(Cap::At(200_000_000)));
        assert_eq!(parse_cap("1.5GiB"), Ok(Cap::At(1_610_612_736)));
        assert_eq!(parse_cap("1_000"), Ok(Cap::At(1000)));
        assert!(parse_cap("lots").is_err());
        assert!(parse_cap("-1").is_err());
        assert!(parse_cap("3 parsecs").is_err());
        assert_eq!(Cap::At(5).max(Cap::At(9)), Cap::At(9));
        assert_eq!(Cap::At(5).max(Cap::Unlimited), Cap::Unlimited);
    }

    #[test]
    fn the_built_in_plan_is_the_owners_free_tier() {
        let p = Plan::built_in();
        assert_eq!(p.name, "free-1k");
        assert_eq!(p.default["media.bytes"], Cap::At(2_000_000_000));
        assert_eq!(p.default["docs.bytes"], Cap::At(200_000_000));
        assert_eq!(p.default["tickets.bytes"], Cap::At(50_000_000));
        assert_eq!(p.default["media.fetch.calls"], Cap::At(300));
        assert_eq!(p.default["media.fetch.bytes"], Cap::At(200_000_000));
        assert_eq!(p.default["media.egress.bytes"], Cap::At(2_000_000_000));
        assert_eq!(p.default["tickets.count"], Cap::Unlimited);
        assert_eq!(p.concurrent["media.bytes"], 2);
        assert_eq!((p.box_lines.warn_percent, p.box_lines.full_percent), (70.0, 85.0));
    }

    #[test]
    fn a_plan_naming_no_source_is_refused() {
        let e = Plan::parse(r#"{"name": "x", "default": {"media.nothing": 5}}"#).unwrap_err();
        assert!(e.contains("media.nothing") && e.contains("media.bytes"), "{e}");
    }
}
