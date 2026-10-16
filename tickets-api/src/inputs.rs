//! INPUT-3: a body's **inputs** — the components in it that ask for an
//! answer (INPUT-1, pin 5) — read as the app renders the body, and an
//! answer's value checked against what an input declares. Wasm-safe, like
//! [`crate::fields`], whose scan it reads with: the store reads every body
//! with it, and the app (INPUT-4's host, INPUT-5's components) can read and
//! check the same way.
//!
//! **The markup** ([`read`]):
//!
//! ```html
//! <m-decision data-field="decision" id="landing">            <!-- kind="choice" is the default -->
//!   <m-question data-field="question">Where does it land?</m-question>
//!   <m-option data-field="option" name="Archive" value="archive" recommended>…</m-option>
//!   <m-option data-field="option" name="Delete">…</m-option>  <!-- value: its name, "Delete" -->
//!   <m-recommendation data-field="recommendation">Archive: holds are real.</m-recommendation>
//! </m-decision>
//!
//! <m-input id="checks" kind="multi">
//!   <m-question>Which checks ran?</m-question>
//!   <m-choice value="unit">Unit tests</m-choice>
//!   <m-choice value="wire" recommended>The wire test</m-choice>
//! </m-input>
//!
//! <m-input id="notes" kind="text"><m-question>Anything else?</m-question></m-input>
//! ```
//!
//! - An **input** is an `<m-decision>` or an `<m-input>` with an `id` (not
//!   blank) — one without is not an input, as every decision written before
//!   INPUT-3 is not. The id is unique in its ticket.
//! - Its **kind**: an `<m-decision>` is a `choice` — `kind` omitted, or
//!   `kind="choice"`; an `<m-input>` is `kind="multi"` or `kind="text"`, and
//!   says which. (Case-insensitive.)
//! - Its **options**, in order: an `<m-decision>`'s `<m-option>`s — the
//!   `value`, else the `name`; labelled by the `name`, else the value — and
//!   an `<m-input>`'s `<m-choice>`s — the `value`; labelled by its text, else
//!   the value. `recommended` marks one. A text input has none.
//! - Its **question** is its first `<m-question>`'s text; its
//!   **recommendation** its first `<m-recommendation>`'s text, else the
//!   label of its first option marked `recommended`.
//!
//! An input's parts are the elements inside it at any depth, but not inside
//! an `<m-decision>` or `<m-input>` nested in it, which holds its own (and is
//! an input itself when it has an id). An `<m-choice>` in a decision and an
//! `<m-option>` in an `<m-input>` are not options. Read as [`crate::fields`]
//! reads a body: raw HTML only — a code span, a fence or indented code is
//! text, so an example of the markup is never an input — outside comments,
//! the frontmatter split off. Text is an element's tags dropped, entities
//! decoded, whitespace collapsed.
//!
//! **Refused** ([`Problem`]) — a `create` or `amend` holding one changes
//! nothing, in-band, naming the input: two inputs with one id; a kind an
//! element does not take (or an `<m-input>` with none); an option with no
//! value; an option value declared twice in one input; a text input with
//! options.

use crate::component_docs::text;
use crate::fields::{marked, Element};
use crate::types::{InputKind, InputOption, InputValue};
use std::collections::HashSet;
use std::fmt;

/// The elements an input is written with.
const TAGS: &[&str] = &["m-decision", "m-input", "m-option", "m-choice", "m-question", "m-recommendation"];

/// What a body declares of one input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub id: String,
    pub kind: InputKind,
    pub question: Option<String>,
    pub options: Vec<InputOption>,
    pub recommendation: Option<String>,
}

/// Input markup a write refuses — each names the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// A second input in the body has the id `input`.
    Twice { input: String },
    /// The element (`m-decision` / `m-input`) does not take the kind
    /// written — `None`: an `<m-input>` that names none.
    Kind { input: String, tag: String, kind: Option<String> },
    /// An option (`m-option` / `m-choice`) of `input` with no value.
    NoValue { input: String, tag: String },
    /// `input` declares the option `value` twice.
    OptionTwice { input: String, value: String },
    /// A text input holding options.
    TextOptions { input: String },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Twice { input } => write!(
                f,
                "the body declares the input '{input}' twice: an input's id is unique in its ticket"
            ),
            Self::Kind { input, tag, kind: None } => write!(
                f,
                "the input '{input}' is an <{tag}> without a kind: write kind=\"multi\" or kind=\"text\""
            ),
            Self::Kind { input, tag, kind: Some(kind) } if tag == "m-decision" => write!(
                f,
                "the input '{input}' is an <m-decision> with kind=\"{kind}\": a decision is a choice — leave kind \
                 out (or write kind=\"choice\"), or make it an <m-input kind=\"multi\"> or <m-input kind=\"text\">"
            ),
            Self::Kind { input, tag, kind: Some(kind) } => write!(
                f,
                "the input '{input}' is an <{tag}> with kind=\"{kind}\": an <m-input> is kind=\"multi\" or \
                 kind=\"text\" — a choice is an <m-decision>"
            ),
            Self::NoValue { input, tag } if tag == "m-option" => write!(
                f,
                "an <m-option> of the input '{input}' has no value: give it value=\"…\" (or a name)"
            ),
            Self::NoValue { input, tag } => write!(f, "an <{tag}> of the input '{input}' has no value: give it value=\"…\""),
            Self::OptionTwice { input, value } => write!(
                f,
                "the input '{input}' declares the option '{value}' twice: an option's value is unique in its input"
            ),
            Self::TextOptions { input } => write!(
                f,
                "the input '{input}' is kind=\"text\", which takes a reply, not options: remove its <m-choice>s, or \
                 make it kind=\"multi\""
            ),
        }
    }
}

/// Whether `body` may hold input markup — it names `m-decision` or
/// `m-input`, in any case. A body that does not is not parsed.
pub fn mentions(body: &str) -> bool {
    let has = |needle: &[u8]| body.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle));
    body.contains('<') && (has(b"<m-decision") || has(b"<m-input"))
}

/// The inputs `body` declares, in the body's order, and what in it a write
/// refuses — see the module doc. An input with a problem is left out (of
/// two with one id, the second); a caller that refuses, refuses on any.
pub fn read(body: &str) -> (Vec<Declared>, Vec<Problem>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    if mentions(body) {
        let mut seen = HashSet::new();
        walk(&marked(body, TAGS), &mut out, &mut problems, &mut seen);
    }
    (out, problems)
}

fn is_input(tag: &str) -> bool {
    matches!(tag, "m-decision" | "m-input")
}

/// Every input in `elements` and inside them, in document order.
fn walk(elements: &[Element], out: &mut Vec<Declared>, problems: &mut Vec<Problem>, seen: &mut HashSet<String>) {
    for e in elements {
        if is_input(&e.tag) {
            if let Some(id) = e.attr("id").map(str::trim).filter(|id| !id.is_empty()) {
                if !seen.insert(id.to_string()) {
                    problems.push(Problem::Twice { input: id.to_string() });
                } else {
                    match declare(id, e) {
                        Ok(d) => out.push(d),
                        Err(p) => problems.push(p),
                    }
                }
            }
        }
        walk(&e.parts, out, problems, seen);
    }
}

/// The input's own parts: inside it at any depth, not inside a nested input
/// element.
fn own<'e>(parts: &'e [Element], out: &mut Vec<&'e Element>) {
    for p in parts {
        if is_input(&p.tag) {
            continue;
        }
        out.push(p);
        own(&p.parts, out);
    }
}

/// A non-blank attribute value, trimmed.
fn value_of<'e>(e: &'e Element, name: &str) -> Option<&'e str> {
    e.attr(name).map(str::trim).filter(|v| !v.is_empty())
}

/// A non-blank text.
fn some_text(content: &str) -> Option<String> {
    Some(text(content)).filter(|t| !t.is_empty())
}

fn declare(id: &str, e: &Element) -> Result<Declared, Problem> {
    let written = e.attr("kind").map(str::to_string);
    let kind = match (e.tag.as_str(), written.as_deref().map(str::parse::<InputKind>)) {
        ("m-decision", None | Some(Ok(InputKind::Choice))) => InputKind::Choice,
        ("m-input", Some(Ok(k @ (InputKind::Multi | InputKind::Text)))) => k,
        (tag, _) => {
            return Err(Problem::Kind {
                input: id.to_string(),
                tag: tag.to_string(),
                kind: written,
            })
        }
    };
    let option_tag = if kind == InputKind::Choice { "m-option" } else { "m-choice" };
    let mut parts = Vec::new();
    own(&e.parts, &mut parts);
    let mut options: Vec<InputOption> = Vec::new();
    for o in parts.iter().filter(|p| p.tag == option_tag) {
        if kind == InputKind::Text {
            return Err(Problem::TextOptions { input: id.to_string() });
        }
        let name = value_of(o, "name");
        let value = match (value_of(o, "value"), option_tag) {
            (Some(v), _) => v,
            (None, "m-option") => name.ok_or_else(|| Problem::NoValue {
                input: id.to_string(),
                tag: option_tag.to_string(),
            })?,
            (None, _) => {
                return Err(Problem::NoValue {
                    input: id.to_string(),
                    tag: option_tag.to_string(),
                })
            }
        };
        if options.iter().any(|x| x.value == value) {
            return Err(Problem::OptionTwice {
                input: id.to_string(),
                value: value.to_string(),
            });
        }
        let label = match option_tag {
            "m-option" => name.map(str::to_string),
            _ => some_text(&o.content),
        };
        options.push(InputOption {
            value: value.to_string(),
            label: label.unwrap_or_else(|| value.to_string()),
            recommended: o.attr("recommended").is_some(),
        });
    }
    let first = |tag: &str| parts.iter().find(|p| p.tag == tag).and_then(|p| some_text(&p.content));
    let recommendation = first("m-recommendation").or_else(|| options.iter().find(|o| o.recommended).map(|o| o.label.clone()));
    Ok(Declared {
        id: id.to_string(),
        kind,
        question: first("m-question"),
        options,
        recommendation,
    })
}

/// `value` as an answer to an input of `kind` declaring `options` — the
/// value to record (a `multi`'s options in the declared order, each once),
/// or why it is not one: a value of another kind; a `choice` naming no
/// option it declares; a `multi` naming one it does not (none at all is an
/// answer: nothing ticked); a blank `text`.
pub fn check(kind: InputKind, options: &[InputOption], value: InputValue) -> Result<InputValue, String> {
    if value.kind() != kind {
        return Err(format!(
            "the input is a {kind} and the value a {} — answer it with {{\"type\": \"{kind}\", …}}",
            value.kind()
        ));
    }
    let declared = || {
        if options.is_empty() {
            "it declares none".to_string()
        } else {
            options.iter().map(|o| format!("'{}'", o.value)).collect::<Vec<_>>().join(", ")
        }
    };
    match value {
        InputValue::Choice { option } => {
            if options.iter().any(|o| o.value == option) {
                Ok(InputValue::Choice { option })
            } else {
                Err(format!("'{option}' is not one of its options ({})", declared()))
            }
        }
        InputValue::Multi { options: picked } => {
            let unknown: Vec<String> = picked
                .iter()
                .filter(|p| !options.iter().any(|o| &o.value == *p))
                .map(|p| format!("'{p}'"))
                .collect();
            if !unknown.is_empty() {
                return Err(format!("{} not among its options ({})", unknown.join(", "), declared()));
            }
            Ok(InputValue::Multi {
                options: options.iter().filter(|o| picked.contains(&o.value)).map(|o| o.value.clone()).collect(),
            })
        }
        InputValue::Text { text } => {
            if text.trim().is_empty() {
                Err("a reply is not blank — inputs.clear clears an answer".to_string())
            } else {
                Ok(InputValue::Text { text })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(value: &str, label: &str, recommended: bool) -> InputOption {
        InputOption {
            value: value.into(),
            label: label.into(),
            recommended,
        }
    }

    fn one(body: &str) -> Declared {
        let (mut found, problems) = read(body);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(found.len(), 1, "{found:?}");
        found.remove(0)
    }

    fn problem(body: &str) -> Problem {
        let (_, mut problems) = read(body);
        assert_eq!(problems.len(), 1, "{problems:?}");
        problems.remove(0)
    }

    /// The house decision as ART-9 writes it, with an id — a choice, its
    /// question, its options (value, else name), the recommendation.
    #[test]
    fn a_decision_with_an_id_is_a_choice_input() {
        let body = r#"<m-decision data-field="decision" id="landing">

<m-question data-field="question">Expired events: **archive** or delete?</m-question>

<m-option data-field="option" name="Archive" value="archive" recommended cost="a second store">Cold storage.</m-option>
<m-option data-field="option" name="Delete" cost="holds broken">Removed.</m-option>

<m-recommendation data-field="recommendation">Archive: holds are real.</m-recommendation>

</m-decision>
"#;
        assert_eq!(
            one(body),
            Declared {
                id: "landing".into(),
                kind: InputKind::Choice,
                question: Some("Expired events: **archive** or delete?".into()),
                options: vec![option("archive", "Archive", true), option("Delete", "Delete", false)],
                recommendation: Some("Archive: holds are real.".into()),
            }
        );
        // In one HTML block, and with kind written.
        let tight = r#"<m-decision id="d" kind="Choice"><m-question>Q?</m-question><m-option name="A"></m-option><m-option value="b" name="B" recommended></m-option></m-decision>"#;
        let d = one(tight);
        assert_eq!((d.kind, d.question.as_deref()), (InputKind::Choice, Some("Q?")));
        assert_eq!(d.options, vec![option("A", "A", false), option("b", "B", true)]);
        assert_eq!(d.recommendation.as_deref(), Some("B"), "the recommended option's label, with no m-recommendation");
    }

    /// `m-input` with `kind="multi"` (its `m-choice`s, labelled by their
    /// text) and `kind="text"` (no options).
    #[test]
    fn an_m_input_is_multi_or_text() {
        let multi = one(
            "<m-input id=\"checks\" kind=\"multi\">\n<m-question>Which <em>checks</em> ran?</m-question>\n\
             <m-choice value=\"unit\">Unit &amp; store tests</m-choice>\n<m-choice value=\"wire\" recommended></m-choice>\n</m-input>\n",
        );
        assert_eq!(multi.kind, InputKind::Multi);
        assert_eq!(multi.question.as_deref(), Some("Which checks ran?"));
        assert_eq!(multi.options, vec![option("unit", "Unit & store tests", false), option("wire", "wire", true)]);
        assert_eq!(multi.recommendation.as_deref(), Some("wire"));
        let text = one("<m-input id=\"notes\" kind=\"text\"><m-question>Anything else?</m-question></m-input>");
        assert_eq!((text.kind, text.options.len(), text.recommendation), (InputKind::Text, 0, None));
    }

    /// No id, no input: every decision written before INPUT-3.
    #[test]
    fn an_element_without_an_id_is_not_an_input() {
        for body in [
            "<m-decision data-field=\"decision\"><m-option name=\"A\"></m-option></m-decision>",
            "<m-decision id=\"\"><m-option name=\"A\"></m-option></m-decision>",
            "<m-decision id=\"  \"></m-decision>",
            "<m-input kind=\"text\"></m-input>",
            "plain prose with m-decision in it",
        ] {
            assert_eq!(read(body), (vec![], vec![]), "{body}");
        }
    }

    /// Code is text: an example of the markup — a fence, a code span,
    /// indented code — and a comment and the frontmatter declare nothing.
    #[test]
    fn markup_in_code_comments_or_frontmatter_is_not_an_input() {
        for body in [
            "```html\n<m-decision id=\"x\"></m-decision>\n```\n",
            "Write `<m-decision id=\"x\">` to ask.",
            "Like so:\n\n    <m-decision id=\"x\"></m-decision>\n",
            "<!-- <m-decision id=\"x\"></m-decision> -->\n",
            "---\nnote: <m-decision id=\"x\">\n---\nbody\n",
            "<pre><code>&lt;m-decision id=\"x\"&gt;</code></pre>",
        ] {
            assert_eq!(read(body), (vec![], vec![]), "{body}");
        }
    }

    /// Inputs in body order; a nested input holds its own parts.
    #[test]
    fn inputs_come_in_body_order_and_a_nested_one_holds_its_own_parts() {
        let body = "<m-input id=\"b\" kind=\"text\"></m-input>\n\n\
                    <m-decision id=\"a\"><m-option name=\"X\"></m-option>\
                    <m-input id=\"c\" kind=\"multi\"><m-question>Inner?</m-question><m-choice value=\"1\"></m-choice></m-input>\
                    <m-choice value=\"stray\"></m-choice></m-decision>\n";
        let (found, problems) = read(body);
        assert!(problems.is_empty(), "{problems:?}");
        let ids: Vec<&str> = found.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["b", "a", "c"]);
        assert_eq!(found[1].options, vec![option("X", "X", false)], "an m-choice in a decision is not an option");
        assert_eq!(found[1].question, None, "the inner question is the inner input's");
        assert_eq!(found[2].question.as_deref(), Some("Inner?"));
        assert_eq!(found[2].options, vec![option("1", "1", false)]);
    }

    #[test]
    fn what_a_write_refuses_names_the_input() {
        let twice = "<m-input id=\"x\" kind=\"text\"></m-input>\n\n<m-decision id=\"x\"><m-option name=\"A\"></m-option></m-decision>\n";
        let (found, problems) = read(twice);
        assert_eq!(found.len(), 1, "the first is kept");
        assert_eq!(problems, vec![Problem::Twice { input: "x".into() }]);
        assert!(problems[0].to_string().contains("'x' twice"));

        let cases: [(&str, Problem, &str); 6] = [
            (
                "<m-decision id=\"d\" kind=\"multi\"></m-decision>",
                Problem::Kind { input: "d".into(), tag: "m-decision".into(), kind: Some("multi".into()) },
                "a decision is a choice",
            ),
            (
                "<m-input id=\"i\"></m-input>",
                Problem::Kind { input: "i".into(), tag: "m-input".into(), kind: None },
                "without a kind",
            ),
            (
                "<m-input id=\"i\" kind=\"choice\"></m-input>",
                Problem::Kind { input: "i".into(), tag: "m-input".into(), kind: Some("choice".into()) },
                "a choice is an <m-decision>",
            ),
            (
                "<m-decision id=\"d\"><m-option><span slot=\"name\">A</span></m-option></m-decision>",
                Problem::NoValue { input: "d".into(), tag: "m-option".into() },
                "has no value",
            ),
            (
                "<m-input id=\"i\" kind=\"multi\"><m-choice value=\"a\"></m-choice><m-choice value=\" a \"></m-choice></m-input>",
                Problem::OptionTwice { input: "i".into(), value: "a".into() },
                "the option 'a' twice",
            ),
            (
                "<m-input id=\"t\" kind=\"text\"><m-choice value=\"a\"></m-choice></m-input>",
                Problem::TextOptions { input: "t".into() },
                "takes a reply, not options",
            ),
        ];
        for (body, want, says) in cases {
            let got = problem(body);
            assert_eq!(got, want, "{body}");
            assert!(got.to_string().contains(says), "{got}");
        }
        assert!(problem("<m-input id=\"c\" kind=\"multi\"><m-choice>no value</m-choice></m-input>")
            .to_string()
            .contains("<m-choice> of the input 'c' has no value"));
    }

    #[test]
    fn a_value_is_checked_against_the_kind_and_the_options() {
        let options = [option("a", "A", false), option("b", "B", false), option("c", "C", false)];
        let choice = |o: &str| InputValue::Choice { option: o.into() };
        let multi = |os: &[&str]| InputValue::Multi { options: os.iter().map(|o| o.to_string()).collect() };
        assert_eq!(check(InputKind::Choice, &options, choice("b")), Ok(choice("b")));
        assert!(check(InputKind::Choice, &options, choice("z")).unwrap_err().contains("'z' is not one of its options ('a', 'b', 'c')"));
        assert!(check(InputKind::Choice, &[], choice("a")).unwrap_err().contains("it declares none"));
        assert_eq!(check(InputKind::Multi, &options, multi(&["c", "a", "c"])), Ok(multi(&["a", "c"])), "declared order, once");
        assert_eq!(check(InputKind::Multi, &options, multi(&[])), Ok(multi(&[])), "nothing ticked");
        assert!(check(InputKind::Multi, &options, multi(&["a", "y", "z"])).unwrap_err().starts_with("'y', 'z' not among"));
        let text = InputValue::Text { text: " a reply, with commas ".into() };
        assert_eq!(check(InputKind::Text, &[], text.clone()), Ok(text), "kept as given");
        assert!(check(InputKind::Text, &[], InputValue::Text { text: "  ".into() }).unwrap_err().contains("not blank"));
        let wrong = check(InputKind::Choice, &options, InputValue::Text { text: "a".into() }).unwrap_err();
        assert!(wrong.contains("the input is a choice and the value a text"), "{wrong}");
    }

    #[test]
    fn values_are_tagged_on_the_wire() {
        use serde_json::json;
        for (v, want) in [
            (InputValue::Choice { option: "a".into() }, json!({"type": "choice", "option": "a"})),
            (InputValue::Multi { options: vec!["a".into()] }, json!({"type": "multi", "options": ["a"]})),
            (InputValue::Text { text: "t".into() }, json!({"type": "text", "text": "t"})),
        ] {
            assert_eq!(serde_json::to_value(&v).unwrap(), want);
            assert_eq!(serde_json::from_value::<InputValue>(want).unwrap(), v);
        }
        for k in [InputKind::Choice, InputKind::Multi, InputKind::Text] {
            assert_eq!(serde_json::to_value(k).unwrap(), json!(k.as_str()));
            assert_eq!(k.as_str().to_uppercase().parse::<InputKind>(), Ok(k));
        }
        for s in crate::types::InputState::ALL {
            assert_eq!(serde_json::to_value(s).unwrap(), json!(s.as_str()));
            assert_eq!(s.as_str().parse::<crate::types::InputState>(), Ok(s));
        }
    }
}
