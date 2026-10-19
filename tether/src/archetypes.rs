//! TETHER-9: an archetype's model — CHAT-45's one file, which the chat
//! bridge and every launcher read (`scripts/chat-bridge/models.mjs`):
//! `MENGER_MODEL_<ARCHETYPE>=<model>` lines, read at each run's start (an
//! edit needs no restart), a line missing the built-in default. substrate
//! runs Claude Code's three aliases, so a model is `opus`, `sonnet` or
//! `haiku`, by its alias or an id naming it (`claude-sonnet-5-5`).

use std::path::Path;

use plexus_substrate::activations::claudecode::Model;

use crate::vocab::Archetype;

/// The built-in default (models.mjs's `ARCHETYPES`).
pub const fn built_in(a: Archetype) -> Model {
    match a {
        Archetype::Architect | Archetype::Builder => Model::Opus,
        Archetype::Analyst => Model::Sonnet,
    }
}

/// A model's id or alias as substrate's model; `None` when it names none of
/// the three (a `fable` substrate cannot run).
pub fn model_of(word: &str) -> Option<Model> {
    let w = word.trim().trim_matches(|c| c == '"' || c == '\'').to_ascii_lowercase();
    ["opus", "sonnet", "haiku"]
        .into_iter()
        .find(|m| w == *m || (w.starts_with("claude-") && w.split('-').any(|p| p == *m)))
        .and_then(Model::from_str)
}

/// `a`'s model from `file`: its line, else the built-in. A line that names
/// no model substrate runs is refused, naming it.
pub fn model_for(file: &Path, a: Archetype) -> Result<Model, String> {
    let Ok(text) = std::fs::read_to_string(file) else { return Ok(built_in(a)) };
    let key = format!("MENGER_MODEL_{}", a.as_str().to_ascii_uppercase());
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        if k.trim() == key {
            return model_of(v).ok_or_else(|| {
                format!("{}:{}: `{}` is not a model substrate runs (opus, sonnet or haiku)", file.display(), i + 1, v.trim())
            });
        }
    }
    Ok(built_in(a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archetypes_line_names_its_model() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("archetypes.env");
        assert_eq!(model_for(&f, Archetype::Analyst), Ok(Model::Sonnet), "no file: the built-in");
        std::fs::write(&f, "# c\nMENGER_MODEL_BUILDER=claude-sonnet-5-5\nexport MENGER_MODEL_ANALYST=haiku # cheap\nMENGER_MODEL_ARCHITECT=fable\n").unwrap();
        assert_eq!(model_for(&f, Archetype::Builder), Ok(Model::Sonnet));
        assert_eq!(model_for(&f, Archetype::Analyst), Ok(Model::Haiku));
        let refused = model_for(&f, Archetype::Architect).unwrap_err();
        assert!(refused.contains(":4:") && refused.contains("fable"), "{refused}");
        assert_eq!(model_of("claude-opus-5-5"), Some(Model::Opus));
        assert_eq!(model_of("gpt"), None);
    }
}
