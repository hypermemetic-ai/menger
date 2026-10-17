//! Plexus RPC surface for diagrams — **the one declaration** (COMPILE-4).
//!
//! This `impl` is both halves of the `mermaid` activation, as
//! tickets-api's is of `tickets` (MFE-11):
//!
//! | build | what the macro emits from it |
//! |---|---|
//! | `server` (default; the `menger-diagrams` binary) | the `Activation` impl, dispatch, the schema, these bodies |
//! | `default-features = false` (a client: menger handing blocks off, wasm32 or not) | `MermaidClient<T>` — `parse`/`parse_all`, `check`/`check_all` — over the activation's own [`MermaidEvent`] |
//!
//! The crate's `server` feature gates the server half of the expansion
//! (MACRO-13's default); the struct and the server-only imports sit behind the same
//! `cfg`. A renamed parameter here is a compile error in every client.

#[cfg(feature = "server")]
use crate::blocks::mermaid_blocks;
#[cfg(feature = "server")]
use crate::node::{Driver, Unavailable};
use crate::types::{Markdown, MermaidEvent, MermaidSource};
#[cfg(feature = "server")]
use async_stream::stream;
#[cfg(feature = "server")]
use futures::Stream;

/// The activation. Server-only: it holds the Node driver.
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct MermaidActivation {
    driver: Driver,
}

#[cfg(feature = "server")]
impl MermaidActivation {
    pub fn new(driver: Driver) -> Self {
        Self { driver }
    }
}

// The attribute is NOT `#[cfg]`-gated: it has to run in the client-only
// build too, or there would be no client.
#[plexus_macros::activation(
    description = "Parse mermaid diagrams with the bundle the app serves, alone or in markdown",
    typed_params,
)]
impl MermaidActivation {
    /// mermaid's verdict on one diagram. Yields exactly one event:
    /// `parsed { diagram_type }` when mermaid accepts it, `parse_error {
    /// message, line }` when it does not (`line` 1-based within `source`,
    /// or null when mermaid names none), or `error { message }` when the
    /// service could not ask (a missing piece, named with the command that
    /// provides it; the Node program died; the 5 s limit).
    ///
    /// The source is only ever parsed — `mermaid.parse`, never render,
    /// under `securityLevel: 'strict'` — so no content can make the
    /// service run anything.
    #[plexus_macros::method(
        description = "Parse one mermaid diagram: parsed { diagram_type } | parse_error { message, line } | error { message }",
        params(source = "The diagram's source — what goes between the ```mermaid fences")
    )]
    pub async fn parse(&self, source: MermaidSource) -> impl Stream<Item = MermaidEvent> + Send + 'static {
        let driver = self.driver.clone();
        stream! {
            yield match driver.parse(source.into_string()).await {
                Ok(verdict) => verdict.into(),
                Err(Unavailable(message)) => MermaidEvent::Error { message },
            };
        }
    }

    /// Every mermaid block in a markdown body — by the app's own rule
    /// (`frontend/src/md.rs`: a fenced block whose info string's first word
    /// is `mermaid`, frontmatter excluded; and, ART-5, in a body holding
    /// HTML, a `<pre class="mermaid">` in an HTML block, alone or inside a
    /// field element or a component — see `blocks.rs`) — parsed in order. Yields one
    /// `diagram { index, line, result }` per block, `index` from 1 and
    /// `line` the block's first source line within the markdown, `result`
    /// as `parse` would answer; then the stream ends. No blocks, no items.
    /// A diagram that kills the program or outlives the limit is that
    /// diagram's `error` result, and the rest are still parsed; a missing
    /// piece is one `error` in place of the rest, since none of it can be
    /// (a body with no blocks needs no Node, and still yields nothing).
    #[plexus_macros::method(
        description = "Parse every mermaid diagram in a ticket body — a ```mermaid fence, or a <pre class=\"mermaid\"> in its HTML: one diagram { index, line, result } per diagram, in order",
        params(markdown = "A ticket body — markdown, HTML, or markdown with HTML")
    )]
    pub async fn check(
        &self,
        markdown: Markdown,
    ) -> impl Stream<Item = MermaidEvent> + Send + 'static {
        let driver = self.driver.clone();
        stream! {
            for block in mermaid_blocks(markdown.as_str()) {
                match driver.parse(block.source).await {
                    Ok(result) => yield MermaidEvent::Diagram {
                        index: block.index,
                        line: block.line,
                        result,
                    },
                    Err(Unavailable(message)) => {
                        yield MermaidEvent::Error { message };
                        return;
                    }
                }
            }
        }
    }
}
