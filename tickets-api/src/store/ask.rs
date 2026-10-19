//! APP-90: **`ask`** — a question for the owner filed on a ticket as a text
//! input (INPUT-1), the chat composer's `/ask`. The store appends
//!
//! ```html
//! <m-input id="ask-N" kind="text">
//! <m-question data-field="question">the question, escaped</m-question>
//! </m-input>
//! ```
//!
//! to the ticket's body, read under the write-serial lock (so no write
//! between the read and this one is lost) and written by [`Store::amend`]'s
//! own path: one write, one revision (TICKETS-7), the input declared and
//! `asking` moved in it, `amended` and `input_changed` broadcast.
//!
//! - **The id** is `ask-N`, the lowest `N` from 1 that no input of the
//!   ticket has ever had — in its body now, or removed from it (whose
//!   history would otherwise come back) — so it is unique in the ticket.
//! - **The question is text, never markup**: `& < > " '` and the backtick
//!   go as entities ([`escaped`]), so it can neither close the element, open
//!   another, nor start a code span that hides the closing tags from the
//!   reader; the reader decodes them back ([`crate::inputs`]). It is one
//!   line (the vocabulary's [`crate::vocab::Question`]), so no blank line
//!   ends the HTML block early.

use super::*;

/// The prefix of every input `ask` files.
pub(crate) const ASK_PREFIX: &str = "ask-";

/// `text` as an element's text: the characters markup, markdown's code span
/// or an attribute could read, as entities.
pub(crate) fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '`' => out.push_str("&#96;"),
            c => out.push(c),
        }
    }
    out
}

/// The lowest `ask-N` none of `taken` is.
pub(crate) fn free_id<'a>(taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: std::collections::HashSet<&str> = taken.into_iter().collect();
    (1..).map(|n| format!("{ASK_PREFIX}{n}")).find(|id| !taken.contains(id.as_str())).expect("an id is free")
}

/// `body` with the text input `id` asking `question` appended — a blank
/// line before it, so it starts its own HTML block.
pub(crate) fn with_question(body: &str, id: &str, question: &str) -> String {
    let head = body.trim_end();
    let gap = if head.is_empty() { "" } else { "\n\n" };
    format!(
        "{head}{gap}<m-input id=\"{id}\" kind=\"text\">\n<m-question data-field=\"question\">{}</m-question>\n</m-input>\n",
        escaped(question)
    )
}

impl Store {
    /// APP-90: files `question` on the ticket `on` is for as a new text
    /// input (see the module) and answers it — [`InputWrite::Changed`], the
    /// input as the store now holds it (`open`, no answer). Refused as
    /// [`Store::amend`] refuses the body it writes (a body past
    /// `--max-body-bytes`, malformed markup elsewhere in it), and
    /// [`StoreError::NotFound`] for no ticket — nothing written.
    pub async fn ask(&self, on: Permit<Editor>, question: &str) -> Result<InputWrite, StoreError> {
        let serial = self.serial().await;
        let ticket = self.get_unscoped(&crate::address::folded(on.id())).await?;
        let id = {
            let cache = self.tickets.read().expect("store lock poisoned");
            let (declared, _) = crate::inputs::read(&ticket.body);
            let known: Vec<String> = cache.inputs.of(&ticket.id).iter().map(|i| i.id.to_string()).chain(declared.into_iter().map(|d| d.id)).collect();
            free_id(known.iter().map(String::as_str))
        };
        let body = with_question(&ticket.body, &id, question);
        self.amend_under(serial, on, None, Some(body)).await?;
        let cache = self.tickets.read().expect("store lock poisoned");
        let input = cache.inputs.get(&ticket.id, &id).cloned().ok_or_else(|| StoreError::InputUnknown {
            ticket: ticket.id.to_string(),
            input: id.clone(),
            known: cache.inputs.of(&ticket.id).iter().map(|i| i.id.to_string()).collect(),
        })?;
        Ok(InputWrite::Changed(input))
    }
}
