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
    // notes, themes, profiles, session — and tickets' children inputs,
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
    "tickets-api/src/session_activation.rs",
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

/// The activation types of the services that go: none is named anywhere in
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
    "SessionActivation",
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
    assert!(names("// .register(SessionActivation::offering(..))", "SessionActivation"));
}
