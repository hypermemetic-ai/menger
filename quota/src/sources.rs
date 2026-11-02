//! **The sources** — every data input the quota service counts, by name.
//! A source is a name, a unit and a window: `media.bytes` counts bytes for
//! good, `media.fetch.calls` counts calls per day. A name not here is
//! refused (`unknown_source`), so a new input registers one here — one row.

use serde::{Deserialize, Serialize};

/// What a source counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Bytes,
    Count,
    Calls,
}

impl Unit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Unit::Bytes => "bytes",
            Unit::Count => "count",
            Unit::Calls => "calls",
        }
    }
}

/// Over what it is counted: for good (`total`, given back only by
/// `credit`), or per UTC day (`day`, a windowed counter that starts at
/// nothing each day).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Window {
    Total,
    Day,
}

impl Window {
    pub const fn as_str(self) -> &'static str {
        match self {
            Window::Total => "total",
            Window::Day => "day",
        }
    }

    /// The window `now` (unix seconds) falls in: `0` for a total, the UTC
    /// day's number for a day.
    pub fn at(self, now: i64) -> i64 {
        match self {
            Window::Total => 0,
            Window::Day => now.div_euclid(86_400),
        }
    }
}

/// One source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    pub name: &'static str,
    pub unit: Unit,
    pub window: Window,
    /// What counts against it, in words.
    pub what: &'static str,
}

/// Every source, in the order the surface shows them.
pub const SOURCES: &[Source] = &[
    Source { name: "media.bytes", unit: Unit::Bytes, window: Window::Total, what: "files uploaded (media.put), kept by their hash" },
    Source { name: "media.fetch.bytes", unit: Unit::Bytes, window: Window::Day, what: "bytes the server fetched for you (unfurl, fetch)" },
    Source { name: "media.fetch.calls", unit: Unit::Calls, window: Window::Day, what: "fetches the server made for you (unfurl, fetch)" },
    Source { name: "media.egress.bytes", unit: Unit::Bytes, window: Window::Day, what: "media served to you at /media/<hash>" },
    Source { name: "docs.bytes", unit: Unit::Bytes, window: Window::Total, what: "document bodies and their revisions" },
    Source { name: "docs.count", unit: Unit::Count, window: Window::Total, what: "documents created" },
    Source { name: "tickets.bytes", unit: Unit::Bytes, window: Window::Total, what: "ticket bodies and their revisions" },
    Source { name: "tickets.count", unit: Unit::Count, window: Window::Total, what: "tickets created" },
    Source { name: "questions.count", unit: Unit::Count, window: Window::Total, what: "questions asked" },
];

/// The source named `name`; `None` for a name that is none.
pub fn source(name: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|s| s.name == name)
}

/// Every source's name, for a refusal that names them.
pub fn names() -> String {
    SOURCES.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_is_named_once_and_the_windows_turn_daily() {
        let mut seen = std::collections::BTreeSet::new();
        for s in SOURCES {
            assert!(seen.insert(s.name), "{} twice", s.name);
            assert_eq!(source(s.name), Some(s));
        }
        assert!(source("media.nothing").is_none());
        assert_eq!(Window::Total.at(1_793_609_096), 0);
        assert_eq!(Window::Day.at(86_400 * 3 + 5), 3);
        assert_eq!(Window::Day.at(86_400 * 3 - 1), 2);
    }
}
