//! V2-4: the tickets service holds the tickets and nothing else. "The cut,
//! named" on V2-4 lists what goes — the other services' activations, their
//! store modules, the body parsers that served them, the webhook worker, the
//! frames' SDK generator — and acceptance 4 holds the crate to it: no module
//! names chats, comments, deployments, themes, the queue, media, notes,
//! inputs, components or profiles. This file is that check, over the tree.

use std::path::{Path, PathBuf};

/// The repository's root: this crate's parent.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("tickets-api has a parent").to_path_buf()
}

/// The paths that go, from the repository's root — each a file or a
/// directory. Grouped as the ticket names them.
const GOES: &[&str] = &[
    // The activations: chats, comments, deployments (and webhook), media,
    // notes, themes, profiles — and tickets' children inputs,
    // components, the queue (await_reply, request_changes, queued, snooze,
    // unqueue) and assign.
    "tickets-api/src/chats_activation.rs",
    "tickets-api/src/chats_activation",
    "tickets-api/src/comments_activation.rs",
    "tickets-api/src/comments_activation",
    "tickets-api/src/deployments_activation.rs",
    "tickets-api/src/deployments_activation",
    "tickets-api/src/media_activation.rs",
    "tickets-api/src/media_activation",
    "tickets-api/src/notes_activation.rs",
    "tickets-api/src/themes_activation.rs",
    "tickets-api/src/members_activation/profiles.rs",
    "tickets-api/src/activation/inputs.rs",
    "tickets-api/src/activation/components.rs",
    "tickets-api/src/activation/queue.rs",
    "tickets-api/src/activation/await_reply.rs",
    "tickets-api/src/activation/assign.rs",
    // Their vocabularies (acceptance 4: no module names them).
    "tickets-api/src/chats.rs",
    "tickets-api/src/chat_inputs.rs",
    "tickets-api/src/chat_reply.rs",
    "tickets-api/src/comments.rs",
    "tickets-api/src/deployments.rs",
    "tickets-api/src/media.rs",
    "tickets-api/src/notes.rs",
    "tickets-api/src/themes.rs",
    "tickets-api/src/themes_tests.rs",
    "tickets-api/src/inputs.rs",
    "tickets-api/src/queue.rs",
    "tickets-api/src/assignees.rs",
    // Their store modules.
    "tickets-api/src/store/chats.rs",
    "tickets-api/src/store/chats",
    "tickets-api/src/store/comments.rs",
    "tickets-api/src/store/deployments.rs",
    "tickets-api/src/store/deployments",
    "tickets-api/src/store/media.rs",
    "tickets-api/src/store/notes.rs",
    "tickets-api/src/store/themes.rs",
    "tickets-api/src/store/theme_registry.rs",
    "tickets-api/src/store/queue.rs",
    "tickets-api/src/store/ask.rs",
    "tickets-api/src/store/inputs.rs",
    "tickets-api/src/store/input_writes.rs",
    "tickets-api/src/store/components.rs",
    "tickets-api/src/store/profiles.rs",
    "tickets-api/src/store/assignees.rs",
    "tickets-api/src/store/replies.rs",
    "tickets-api/src/store/threads.rs",
    // …and their tests, gone with them.
    "tickets-api/src/store/tests_ask.rs",
    "tickets-api/src/store/tests_assignees.rs",
    "tickets-api/src/store/tests_chat_inputs.rs",
    "tickets-api/src/store/tests_chat_reply.rs",
    "tickets-api/src/store/tests_chat_tickets.rs",
    "tickets-api/src/store/tests_chats.rs",
    "tickets-api/src/store/tests_chats_deletes.rs",
    "tickets-api/src/store/tests_chats_grants.rs",
    "tickets-api/src/store/tests_chats_outside.rs",
    "tickets-api/src/store/tests_comments.rs",
    "tickets-api/src/store/tests_components.rs",
    "tickets-api/src/store/tests_deployments.rs",
    "tickets-api/src/store/tests_inputs.rs",
    "tickets-api/src/store/tests_media.rs",
    "tickets-api/src/store/tests_notes.rs",
    "tickets-api/src/store/tests_profiles.rs",
    "tickets-api/src/store/tests_queue.rs",
    "tickets-api/src/store/tests_queue_many.rs",
    "tickets-api/src/store/tests_queue_views.rs",
    "tickets-api/src/store/tests_replies.rs",
    "tickets-api/src/store/tests_theme_registry.rs",
    "tickets-api/src/store/tests_themes.rs",
    "tickets-api/src/store/tests_threads.rs",
    // The body parsers that served them: a body is stored as text.
    "tickets-api/src/mockups.rs",
    "tickets-api/src/component_docs.rs",
    "tickets-api/src/html_runs.rs",
    "tickets-api/src/theme_block.rs",
    "tickets-api/src/theme_block_tests.rs",
    "tickets-api/src/chat_tickets.rs",
    // The webhook worker.
    "src/webhooks.rs",
    "src/webhooks",
    // The frames' SDK generator and its manifest test.
    "scripts/sdk-codegen.sh",
    "tickets-api/tests/sdk_manifest.rs",
    // The wire tests of what went, removed with it.
    "tests/wire_chats.rs",
    "tests/wire_ask.rs",
    "tests/wire_assign.rs",
    "tests/wire_deployments.rs",
    "tests/wire_media.rs",
    "tests/wire_themes.rs",
    "tests/deploy_record.rs",
    "tests/webhook_secret.rs",
];

/// The activation types of the services that go (`session` stays, with
/// members, orgs and epics: the browser's sign-in): none is named anywhere in
/// `tickets-api/src` or `src` — not declared, not registered, not left in a
/// comment.
const TYPES: &[&str] = &[
    "ChatsActivation",
    "CommentsActivation",
    "DeploymentsActivation",
    "WebhookActivation",
    "MediaActivation",
    "NotesActivation",
    "ThemesActivation",
    "ProfilesActivation",
    "InputsActivation",
    "MessageInputsActivation",
    "ComponentsActivation",
];

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// Whether `word` occurs in `text` as an identifier of its own (not inside
/// a longer one: `MessageInputsActivation` is not `InputsActivation`).
fn names(text: &str, word: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(ident) && !after.is_some_and(ident)
    })
}

#[test]
fn the_tree_holds_none_of_the_other_services() {
    let root = root();
    let left: Vec<&str> = GOES.iter().copied().filter(|p| root.join(p).exists()).collect();

    let mut files = Vec::new();
    sources(&root.join("tickets-api/src"), &mut files);
    sources(&root.join("src"), &mut files);
    let mut named = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        for ty in TYPES {
            if names(&text, ty) {
                named.push(format!("{} names {ty}", file.strip_prefix(&root).unwrap().display()));
            }
        }
    }

    assert!(
        left.is_empty() && named.is_empty(),
        "V2-4's cut: {} path(s) that go are still here:\n  {}\n{} mention(s) of the other services' activations:\n  {}",
        left.len(),
        left.join("\n  "),
        named.len(),
        named.join("\n  ")
    );
}

/// The check checks: the identifier rule, on what it must and must not see.
#[test]
fn a_name_is_found_only_as_an_identifier_of_its_own() {
    assert!(names("use crate::ChatsActivation;", "ChatsActivation"));
    assert!(names("ChatsActivation::with_access(..)", "ChatsActivation"));
    assert!(!names("MessageInputsActivation", "InputsActivation"));
    assert!(!names("ChatsActivationX", "ChatsActivation"));
    assert!(names("// .register(ChatsActivation::with_access(..))", "ChatsActivation"));
}

/// The store modules the cut names: none is declared in `store/mod.rs`.
const STORE_MODULES: &[&str] = &[
    "chats", "comments", "deployments", "media", "notes", "themes", "theme_registry", "queue", "ask", "inputs",
    "input_writes", "components", "profiles", "assignees", "replies", "threads",
];

/// The crate's modules of the services that went, and the body parsers that
/// served them: none is declared in `lib.rs`.
const CRATE_MODULES: &[&str] = &[
    "chats", "chats_activation", "chat_inputs", "chat_reply", "chat_tickets", "comments", "comments_activation",
    "deployments", "deployments_activation", "media", "media_activation", "notes", "notes_activation", "themes",
    "themes_activation", "inputs", "queue", "assignees", "mockups", "component_docs", "html_runs", "theme_block",
];

/// The `mod <name>` declarations in `text` (any visibility, inline or not),
/// of those in `names`.
fn declared<'a>(text: &str, names: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with("//") {
            continue;
        }
        let Some(at) = line.find("mod ") else { continue };
        if !line[..at].split_whitespace().all(|w| w == "pub" || w.starts_with("pub(")) {
            continue;
        }
        let rest = &line[at + 4..];
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if let Some(n) = names.iter().find(|n| **n == name) {
            out.push(*n);
        }
    }
    out
}

#[test]
fn no_module_of_the_other_services_is_declared() {
    assert_eq!(declared("pub(crate) mod chats;\nmod tests_chats;\n// mod queue;\n    pub mod themes {", STORE_MODULES), ["chats", "themes"]);
    let root = root();
    let store = std::fs::read_to_string(root.join("tickets-api/src/store/mod.rs")).unwrap();
    let lib = std::fs::read_to_string(root.join("tickets-api/src/lib.rs")).unwrap();
    let (in_store, in_lib) = (declared(&store, STORE_MODULES), declared(&lib, CRATE_MODULES));
    assert!(in_store.is_empty() && in_lib.is_empty(), "V2-4: store/mod.rs declares {in_store:?}; lib.rs declares {in_lib:?}");
}

/// The binary's flags and environment variables for the services that
/// went: none is left in `src/main.rs`.
#[test]
fn the_binary_has_no_flag_for_them() {
    let main = std::fs::read_to_string(root().join("src/main.rs")).unwrap();
    let left: Vec<&str> = ["chat-member", "chat_member", "MENGER_CHAT_MEMBERS", "seed-synthetic-chat", "SEED_SYNTHETIC_CHAT", "webhook", "WEBHOOK"]
        .into_iter()
        .filter(|w| main.contains(w))
        .collect();
    assert!(left.is_empty(), "V2-4: src/main.rs still names {left:?}");
}

/// V2-8, acceptance 1: the words of what went, which README and AGENTS.md
/// must no longer say — v1's services, its queue, its browser toolchain and
/// the old word for a service.
const GONE_WORDS: &[&str] = &["chats", "comments", "deployments", "themes", "queue", "trunk", "Leptos", "sdk-codegen", "hub"];

/// The tag of v1: the one line in each file that names what went and where
/// it lives carries it.
const V1_TAG: &str = "v0.0.1";

/// Whether `word` occurs in `text` as a word of its own, ignoring case:
/// `names()`'s identifier rule over lower-cased text (`Chats`, `the queue`
/// and `trunk build` do; `GitHub`, `queued` and `leptos_dom` do not).
fn says(text: &str, word: &str) -> bool {
    names(&text.to_ascii_lowercase(), &word.to_ascii_lowercase())
}

/// The lines of the doc `file` (its text `text`) that still say a word of
/// what went, each `file:line: the line`. Allowed: the first line carrying
/// [`V1_TAG`] (the one that names what went; a second such line gets no
/// allowance), and in AGENTS.md the owner's `## Naming` section, from its
/// heading to the next `## ` heading or the end, which names the word it
/// bans.
fn stale(file: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut allowance_used = false;
    let mut in_naming = false;
    for (n, line) in text.lines().enumerate() {
        if line.starts_with("## ") {
            in_naming = file == "AGENTS.md" && line.starts_with("## Naming");
        }
        if in_naming {
            continue;
        }
        if !GONE_WORDS.iter().any(|w| says(line, w)) {
            continue;
        }
        if line.contains(V1_TAG) && !allowance_used {
            allowance_used = true;
            continue;
        }
        out.push(format!("{file}:{}: {line}", n + 1));
    }
    out
}

/// V2-8, acceptance 1: a grep of README.md and AGENTS.md for the words of
/// what went finds nothing but the one line naming what went (tag v0.0.1),
/// and AGENTS.md's naming rule.
#[test]
fn the_docs_say_what_v2_is() {
    let root = root();
    let mut left = Vec::new();
    for file in ["README.md", "AGENTS.md"] {
        let text = std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        left.extend(stale(file, &text));
    }
    assert!(
        left.is_empty(),
        "V2-8: {} line(s) of README.md / AGENTS.md still say what v2 is not ({}):\n  {}",
        left.len(),
        GONE_WORDS.join(", "),
        left.join("\n  ")
    );
}

/// The docs' check checks (V2-8, acceptance 1): a word of its own in any
/// case matches, a word inside a longer one does not, the allowance line
/// is one per file, and only AGENTS.md's naming section is let through.
#[test]
fn a_gone_word_is_found_only_as_a_word_of_its_own() {
    for (text, word) in [
        ("Chats are the `chats` activation", "chats"),
        ("ask in the queue", "queue"),
        ("run trunk build", "trunk"),
        ("the Leptos app", "Leptos"),
        ("a leptos crate", "Leptos"),
        ("rerun scripts/sdk-codegen.sh", "sdk-codegen"),
        ("a hub, a Hub", "hub"),
        ("the hub-codegen generator", "hub"),
        ("COMMENTS, DEPLOYMENTS", "comments"),
    ] {
        assert!(says(text, word), "{word:?} should be found in {text:?}");
    }
    for (text, word) in [
        ("GitHub and github.com", "hub"),
        ("ghcr.io/hypermemetic-ai/menger", "hub"),
        ("HubTransport::as_", "hub"),
        ("`queued` is a filter cell", "queue"),
        ("leptos_dom", "Leptos"),
        ("a chat of one", "chats"),
        ("a sdk-codegenerator", "sdk-codegen"),
    ] {
        assert!(!says(text, word), "{word:?} should not be found in {text:?}");
    }

    let readme = "# menger\nv1 (tag v0.0.1) had chats, comments and a queue.\nNo hub here.\n";
    assert_eq!(stale("README.md", readme), ["README.md:3: No hub here."]);
    let twice = "Gone at v0.0.1: chats.\nAlso at v0.0.1: themes.\nOn GitHub.\n";
    assert_eq!(stale("README.md", twice), ["README.md:2: Also at v0.0.1: themes."]);

    let agents = "# menger\n## Naming: a Plexus backend is a service\nwe don't call them hubs; a hub is a service.\n### the sweep\nno hub.\n## Next\nthe queue\n";
    assert_eq!(stale("AGENTS.md", agents), ["AGENTS.md:7: the queue"]);
    assert_eq!(stale("README.md", agents).len(), 3, "the naming section is AGENTS.md's alone");
    let last = "## Naming\na hub\n";
    assert!(stale("AGENTS.md", last).is_empty(), "the section runs to the end of the file");
}
