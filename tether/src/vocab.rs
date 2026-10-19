//! Every parameter is a type (menger's ACCESS-8, MACRO-12's
//! `typed_params`): a value its rule refuses is `-32602 invalid_params`
//! naming the parameter, before the method runs.

macro_rules! text_type {
    ($(#[$doc:meta])* $name:ident, $rule:path) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                $rule(&text).map(|()| Self(text))
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(text: &str) -> Result<Self, String> {
                Self::try_from(text.to_string())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::try_from(<String as serde::Deserialize>::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

fn ticket_id(text: &str) -> Result<(), String> {
    let ok = !text.is_empty()
        && text.len() <= 128
        && text.contains('-')
        && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '/' | ':' | '.' | '_'));
    ok.then_some(()).ok_or_else(|| format!("`{text}` is not a ticket (MENGER-12, MENGER/ORG-3)"))
}

fn session_name(text: &str) -> Result<(), String> {
    let ok = !text.is_empty() && text.len() <= 160 && text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    ok.then_some(()).ok_or_else(|| format!("`{text}` is not a session's name (lower case, digits and -, as `start` named it)"))
}

fn prompt(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("a prompt says something".into());
    }
    (text.len() <= 256 * 1024).then_some(()).ok_or_else(|| format!("a prompt is at most 256 KiB; this one is {} bytes", text.len()))
}

fn work_dir(text: &str) -> Result<(), String> {
    let ok = text.starts_with('/') && text.len() <= 4096 && !text.chars().any(char::is_control);
    ok.then_some(()).ok_or_else(|| format!("`{text}` is not an absolute directory"))
}

text_type!(
    /// A menger ticket, by any short form of its address (`MENGER-12`,
    /// `MENGER/ORG-3`).
    TicketId,
    ticket_id
);

text_type!(
    /// A session's name, as `start` named it (`menger-12-1a2b3c4d`).
    SessionName,
    session_name
);

text_type!(
    /// What a session is asked: not blank, at most 256 KiB.
    Prompt,
    prompt
);

text_type!(
    /// An absolute directory on this machine.
    WorkDir,
    work_dir
);

/// TETHER-9: what a run is — CHAT-45's archetypes, each with a default
/// model in `~/.config/menger/archetypes.env` ([`crate::archetypes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Archetype {
    /// Answers in chat.
    Architect,
    /// A detached run that builds a ticket.
    Builder,
    /// Reads and reports.
    Analyst,
}

impl Archetype {
    pub const ALL: [Archetype; 3] = [Archetype::Architect, Archetype::Builder, Archetype::Analyst];

    pub const fn as_str(self) -> &'static str {
        match self {
            Archetype::Architect => "architect",
            Archetype::Builder => "builder",
            Archetype::Analyst => "analyst",
        }
    }
}

/// A session's name for `ticket`: its letters lowered, anything else `-`,
/// then eight of a fresh uuid.
pub fn name_for(ticket: &TicketId) -> SessionName {
    let stem: String = ticket.as_str().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let id = uuid::Uuid::new_v4().simple().to_string();
    SessionName(format!("{stem}-{}", &id[..8]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules() {
        assert!("MENGER-12".parse::<TicketId>().is_ok());
        assert!("MENGER/ORG-3".parse::<TicketId>().is_ok());
        assert!("hello world".parse::<TicketId>().is_err());
        assert!("MENGER".parse::<TicketId>().is_err());
        assert!("menger-12-1a2b3c4d".parse::<SessionName>().is_ok());
        assert!("Menger 12".parse::<SessionName>().is_err());
        assert!("  ".parse::<Prompt>().is_err());
        assert!("/tmp/x".parse::<WorkDir>().is_ok());
        assert!("rel/x".parse::<WorkDir>().is_err());
        let name = name_for(&"MENGER/ORG-3".parse().unwrap());
        assert!(name.as_str().starts_with("menger-org-3-"), "{name}");
        assert!(name.as_str().parse::<SessionName>().is_ok());
    }
}
