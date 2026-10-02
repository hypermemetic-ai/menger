//! The `mermaid` activation's vocabulary — the one declaration, shared by
//! the service and every client (COMPILE-4).
//!
//! A diagram gets a [`Verdict`]: mermaid accepted it (and says what type of
//! diagram it is), or mermaid rejected it (its message, and the line), or
//! the service could not ask mermaid (the Node program is missing, died, or
//! outlived the limit). `parse` answers with the verdict itself; `check`
//! answers one [`MermaidEvent::Diagram`] per block, the verdict inside it as
//! `result` — the same three shapes, the same tags, one level down.
//!
//! The rejection and the failure are different on purpose. A `parse_error`
//! is about the diagram: fix the source. An `error` is about the service:
//! the source may be fine, nobody could tell.

use serde::{Deserialize, Serialize};

/// What became of one diagram.
///
/// Wire shape (internally tagged, like every event enum in menger):
/// `{"type":"parsed","diagram_type":"flowchart-v2"}`,
/// `{"type":"parse_error","message":"Parse error on line 2: …","line":2}`,
/// `{"type":"error","message":"…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Verdict {
    /// mermaid accepted the diagram. `diagram_type` is mermaid's own name
    /// for what it found (`flowchart-v2`, `sequence`, `pie`, …).
    Parsed { diagram_type: String },
    /// mermaid rejected the diagram. `message` is mermaid's, verbatim —
    /// what the browser shows in place of the drawing. `line` is the
    /// 1-based line within the source where mermaid says the error is,
    /// when it says (`null` otherwise: an unknown diagram type names none).
    ///
    /// It is a line of the source as given, which is not always the number
    /// in `message`: mermaid numbers the text it parsed, after dropping
    /// frontmatter, `%%` comments, directives and leading blank lines.
    ParseError { message: String, line: Option<u32> },
    /// The diagram could not be checked: the Node program is missing a
    /// piece (named, with the command that provides it), died, or took
    /// longer than the limit. Says nothing about the diagram.
    Error { message: String },
}

/// Every event the `mermaid` activation yields.
///
/// `parse` yields exactly one of `parsed`, `parse_error` or `error`.
/// `check` yields one `diagram` per mermaid block, in order, then ends — or,
/// when the service cannot check anything at all (a missing piece), one
/// `error` in place of the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MermaidEvent {
    /// [`Verdict::Parsed`], as `parse`'s answer.
    Parsed { diagram_type: String },
    /// [`Verdict::ParseError`], as `parse`'s answer.
    ParseError { message: String, line: Option<u32> },
    /// In-band failure: the call could not be served. From `parse`, the
    /// diagram's [`Verdict::Error`]; from `check`, a missing piece, and the
    /// stream ends.
    Error { message: String },
    /// One mermaid block of a `check`ed markdown body. `index` counts the
    /// blocks from 1 in document order; `line` is the 1-based line, within
    /// the markdown, of the block's first source line — the line after its
    /// opening fence — so a `parse_error` at `result.line` L is at markdown
    /// line `line + L - 1`.
    Diagram {
        index: u32,
        line: u32,
        result: Verdict,
    },
}

impl From<Verdict> for MermaidEvent {
    fn from(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Parsed { diagram_type } => MermaidEvent::Parsed { diagram_type },
            Verdict::ParseError { message, line } => MermaidEvent::ParseError { message, line },
            Verdict::Error { message } => MermaidEvent::Error { message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The wire casing is the contract (the ticket's "Provides" row), not
    /// style: these are what synapse prints and what a client matches on.
    #[test]
    fn the_wire_shapes() {
        let parsed = MermaidEvent::Parsed {
            diagram_type: "flowchart-v2".into(),
        };
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            json!({"type": "parsed", "diagram_type": "flowchart-v2"})
        );
        let rejected = MermaidEvent::ParseError {
            message: "Parse error on line 2:".into(),
            line: Some(2),
        };
        assert_eq!(
            serde_json::to_value(&rejected).unwrap(),
            json!({"type": "parse_error", "message": "Parse error on line 2:", "line": 2})
        );
        let unplaced = MermaidEvent::ParseError {
            message: "No diagram type detected".into(),
            line: None,
        };
        assert_eq!(
            serde_json::to_value(&unplaced).unwrap(),
            json!({"type": "parse_error", "message": "No diagram type detected", "line": null}),
            "every parse_error carries the same keys"
        );
        let failed = MermaidEvent::Error {
            message: "Node.js is not installed".into(),
        };
        assert_eq!(
            serde_json::to_value(&failed).unwrap(),
            json!({"type": "error", "message": "Node.js is not installed"})
        );
        let diagram = MermaidEvent::Diagram {
            index: 2,
            line: 14,
            result: Verdict::ParseError {
                message: "Parse error on line 3:".into(),
                line: Some(3),
            },
        };
        assert_eq!(
            serde_json::to_value(&diagram).unwrap(),
            json!({"type": "diagram", "index": 2, "line": 14,
                   "result": {"type": "parse_error", "message": "Parse error on line 3:", "line": 3}})
        );
    }

    /// `result` is "as for `parse`": a verdict serializes exactly as the
    /// event `parse` would have answered with.
    #[test]
    fn a_verdict_is_the_event_parse_answers() {
        for verdict in [
            Verdict::Parsed {
                diagram_type: "pie".into(),
            },
            Verdict::ParseError {
                message: "m".into(),
                line: Some(1),
            },
            Verdict::ParseError {
                message: "m".into(),
                line: None,
            },
            Verdict::Error {
                message: "e".into(),
            },
        ] {
            assert_eq!(
                serde_json::to_value(&verdict).unwrap(),
                serde_json::to_value(MermaidEvent::from(verdict.clone())).unwrap()
            );
        }
    }
}
