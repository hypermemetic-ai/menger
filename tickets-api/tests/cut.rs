//! V2-4: the tickets service holds the tickets and nothing else. "The cut,
//! named" on V2-4 lists what goes — the other services' activations, their
//! store modules, the body parsers that served them, the webhook worker, the
//! frames' SDK generator — and acceptance 4 holds the crate to it: no module
//! names chats, comments, deployments, themes, media, notes, components or
//! profiles. This file is that check, over the tree. V2-20: questions
//! (`tickets inputs`) and the queue came back, and left it. V2-24: media
//! came back as a service of its own (`media/`, `menger-media`, registered
//! in `src/builder.rs`), so its activation's name and its wire test left
//! the list — the tickets crate itself still holds no media module.

use std::path::{Path, PathBuf};

/// The repository's root: this crate's parent.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("tickets-api has a parent").to_path_buf()
}

/// The paths that go, from the repository's root — each a file or a
/// directory. Grouped as the ticket names them.
const GOES: &[&str] = &[
    // The activations: chats, comments, deployments (and webhook), media,
    // notes, themes, profiles — and tickets' children components and assign.
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
    "tickets-api/src/activation/components.rs",
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
    "tickets-api/src/store/components.rs",
    "tickets-api/src/store/profiles.rs",
    "tickets-api/src/store/assignees.rs",
    "tickets-api/src/store/threads.rs",
    // …and their tests, gone with them.
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
    "tickets-api/src/store/tests_media.rs",
    "tickets-api/src/store/tests_notes.rs",
    "tickets-api/src/store/tests_profiles.rs",
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
    "tests/wire_assign.rs",
    "tests/wire_deployments.rs",
    "tests/wire_themes.rs",
    "tests/deploy_record.rs",
    "tests/webhook_secret.rs",
];

/// The activation types of the services that go (`session` stays, with
/// members and orgs: the browser's sign-in; V2-25: `epics` went too): none is named anywhere in
/// `tickets-api/src` or `src` — not declared, not registered, not left in a
/// comment.
const TYPES: &[&str] = &[
    "ChatsActivation",
    "CommentsActivation",
    "DeploymentsActivation",
    "WebhookActivation",
    "NotesActivation",
    "ThemesActivation",
    "ProfilesActivation",
    "MessageInputsActivation",
    "ComponentsActivation",
    // V2-25: epics out of v2 (owner, 2026-11-01).
    "EpicsActivation",
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
    "chats", "comments", "deployments", "media", "notes", "themes", "theme_registry", "components", "profiles", "assignees", "threads",
];

/// The crate's modules of the services that went, and the body parsers that
/// served them: none is declared in `lib.rs`.
const CRATE_MODULES: &[&str] = &[
    "chats", "chats_activation", "chat_inputs", "chat_reply", "chat_tickets", "comments", "comments_activation",
    "deployments", "deployments_activation", "media", "media_activation", "notes", "notes_activation", "themes",
    "themes_activation", "assignees", "mockups", "component_docs", "html_runs", "theme_block",
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
    assert_eq!(declared("pub(crate) mod chats;\nmod tests_chats;\n// mod comments;\n    pub mod themes {", STORE_MODULES), ["chats", "themes"]);
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

/// V2-8, acceptance 1: the words of what went, singular and plural, which
/// README and AGENTS.md must no longer say — v1's services, its mockups, its
/// browser toolchain and the old word for a service. V2-20: the queue's
/// words left: it is back. Not
/// `theme` (`data-theme` stays) and not `deployed` (the pipeline deploys).
const GONE_WORDS: &[&str] = &[
    "chats", "chat", "comments", "comment", "deployments", "deployment", "themes", "trunk",
    "Leptos", "sdk-codegen", "hub", "hubs", "mockup", "mockups",
];

/// V2-10: the words deploy/README.md alone must also no longer say — the
/// deploy page of v1's app, the webhook that requests reached, and the
/// landing's screenshots of v1's app (`landing-media`).
const DEPLOY_GONE_WORDS: &[&str] = &["webhook", "webhooks", "#/deploy", "#/deployments", "landing-media"];

/// The pipeline's doc, held to the rule from V2-10.
const DEPLOY_README: &str = "deploy/README.md";

/// The pipeline's log file, `~/.local/share/menger-deploy/deployments.log`:
/// in [`DEPLOY_README`] the exact span is no word of what went.
const DEPLOY_LOG: &str = "deployments.log";

/// The words of what went that the doc `file` must not say: [`GONE_WORDS`],
/// and for [`DEPLOY_README`] [`DEPLOY_GONE_WORDS`] too — but not the
/// singular `deployment`, an ordinary word of the pipeline's (a deploy is
/// one; the activation is what went).
fn gone_words(file: &str) -> Vec<&'static str> {
    let mut words = GONE_WORDS.to_vec();
    if file == DEPLOY_README {
        words.retain(|w| *w != "deployment");
        words.extend_from_slice(DEPLOY_GONE_WORDS);
    }
    words
}

/// The tag of v1: the first line of each file that carries it is the one
/// that names what went and where it lives.
const V1_TAG: &str = "v0.0.1";

/// The owner's naming rule in AGENTS.md (2026-10-29), which names the word
/// it bans: its exact heading.
const NAMING_HEADING: &str = "## Naming: a Plexus backend is a *service*, never a *hub*";

/// Whether `word` occurs in `text` as a word of its own, ignoring case:
/// `names()`'s identifier rule over lower-cased text (`Chats`, `the queue`
/// and `trunk build` do; `GitHub`, `HubTransport` and `leptos_dom` do not).
fn says(text: &str, word: &str) -> bool {
    names(&text.to_ascii_lowercase(), &word.to_ascii_lowercase())
}

/// The lines of the doc `file` (its text `text`) that still say a word of
/// what went, each `file:line: the line`. Allowed: the first line of the
/// file that contains [`V1_TAG`], whatever it says (every later one is held
/// to the rule), and in AGENTS.md alone the section under
/// [`NAMING_HEADING`], once, through the next `## ` heading or the end.
fn stale(file: &str, text: &str) -> Vec<String> {
    let words = gone_words(file);
    let mut out = Vec::new();
    let mut tag_seen = false;
    let mut naming_seen = false;
    let mut in_naming = false;
    for (n, line) in text.lines().enumerate() {
        if line.starts_with("## ") {
            in_naming = file == "AGENTS.md" && !naming_seen && line == NAMING_HEADING;
            naming_seen |= in_naming;
        }
        if in_naming {
            continue;
        }
        if line.contains(V1_TAG) && !tag_seen {
            tag_seen = true;
            continue;
        }
        let tested = if file == DEPLOY_README { line.replace(DEPLOY_LOG, "") } else { line.to_string() };
        if words.iter().any(|w| says(&tested, w)) {
            out.push(format!("{file}:{}: {line}", n + 1));
        }
    }
    out
}

/// What is wrong with the doc `file`: empty, never naming [`V1_TAG`], or
/// [`stale`] lines.
fn failures(file: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    if text.trim().is_empty() {
        out.push(format!("{file} is empty"));
    }
    if !text.contains(V1_TAG) {
        out.push(format!("{file} never names {V1_TAG}: say what went and where it lives"));
    }
    out.extend(stale(file, text));
    out
}

/// V2-8, acceptance 1: a grep of README.md and AGENTS.md for the words of
/// what went finds nothing but, in each file, the first line naming v1's tag
/// (what went and where it lives), and AGENTS.md's naming rule. Each file
/// must be there, hold text and name the tag: an empty file is no pass.
/// V2-10, acceptance 1: deploy/README.md too, with [`DEPLOY_GONE_WORDS`]
/// beside the rest — its first `v0.0.1` line allowed, no naming section.
#[test]
fn the_docs_say_what_v2_is() {
    let root = root();
    let mut left = Vec::new();
    for file in ["README.md", "AGENTS.md", DEPLOY_README] {
        let text = std::fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        left.extend(failures(file, &text));
    }
    assert!(
        left.is_empty(),
        "V2-8, V2-10: {} line(s) of README.md / AGENTS.md / {DEPLOY_README} still say what v2 is not ({}; in {DEPLOY_README} also {}):\n  {}",
        left.len(),
        GONE_WORDS.join(", "),
        DEPLOY_GONE_WORDS.join(", "),
        left.join("\n  ")
    );
}

/// The docs' check checks (V2-8, acceptance 1): a word of its own in any
/// case matches, a word inside a longer one does not; the allowance is the
/// first `v0.0.1` line of each file, and only AGENTS.md's exact naming
/// section is let through.
#[test]
fn a_gone_word_is_found_only_as_a_word_of_its_own() {
    for (text, word) in [
        ("Chats are the `chats` activation", "chats"),
        ("a chat of one", "chat"),
        ("run trunk build", "trunk"),
        ("the Leptos app", "Leptos"),
        ("a leptos crate", "Leptos"),
        ("rerun scripts/sdk-codegen.sh", "sdk-codegen"),
        ("a hub, a Hub", "hub"),
        ("we don't call them hubs", "hubs"),
        ("the hub-codegen generator", "hub"),
        ("COMMENTS, DEPLOYMENTS", "comments"),
        ("a Mockup of the board", "mockup"),
    ] {
        assert!(says(text, word), "{word:?} should be found in {text:?}");
    }
    for (text, word) in [
        ("GitHub and github.com", "hub"),
        ("GitHub's API", "hub"),
        ("HubTransport::as_", "hub"),
        ("HubTransport::as_", "hubs"),
        ("leptos_dom", "Leptos"),
        ("the data-theme attribute", "themes"),
        ("deployed to test", "deployment"),
        ("a sdk-codegenerator", "sdk-codegen"),
        ("chatter", "chat"),
    ] {
        assert!(!says(text, word), "{word:?} should not be found in {text:?}");
    }

    // An empty file, or one that never names the tag, is no pass.
    assert_eq!(failures("README.md", ""), ["README.md is empty", "README.md never names v0.0.1: say what went and where it lives"]);
    assert_eq!(failures("AGENTS.md", "# menger\n"), ["AGENTS.md never names v0.0.1: say what went and where it lives"]);
    assert!(failures("README.md", "# menger\nv1 is tag v0.0.1.\n").is_empty());

    // The allowance is the first line with the tag, whatever it says.
    let readme = "# menger\nv1 (tag v0.0.1) had chats, comments and a chat.\nNo hub here.\n";
    assert_eq!(stale("README.md", readme), ["README.md:3: No hub here."]);
    let twice = "Gone at v0.0.1: chats.\nAlso at v0.0.1: themes.\nOn GitHub.\n";
    assert_eq!(stale("README.md", twice), ["README.md:2: Also at v0.0.1: themes."]);
    let plain_first = "v1 is tag v0.0.1.\nWhat went at v0.0.1: chats.\n";
    assert_eq!(stale("README.md", plain_first), ["README.md:2: What went at v0.0.1: chats."], "the first tag line spends the allowance");
    // One allowance per file, not one across both.
    assert!(stale("README.md", readme.lines().take(2).collect::<Vec<_>>().join("\n").as_str()).is_empty());
    assert!(stale("AGENTS.md", "v0.0.1 had chats\n").is_empty());

    // The naming section: AGENTS.md's exact heading, once, to the next `## `.
    let agents = format!("# menger\n{NAMING_HEADING}\nwe don't call them hubs; a hub is a service.\n### the sweep\nno hub.\n## Next\nthe comment\n");
    assert_eq!(stale("AGENTS.md", &agents), ["AGENTS.md:7: the comment"]);
    assert_eq!(stale("README.md", &agents).len(), 4, "the naming section, its heading too, is AGENTS.md's alone");
    assert_eq!(stale("AGENTS.md", "## Naming things\na hub\n"), ["AGENTS.md:2: a hub"], "only the exact heading");
    let again = format!("{NAMING_HEADING}\na hub\n## Other\n{NAMING_HEADING}\nhubs\n");
    assert_eq!(
        stale("AGENTS.md", &again),
        [format!("AGENTS.md:4: {NAMING_HEADING}"), "AGENTS.md:5: hubs".to_string()],
        "the section is let through once"
    );
    assert!(stale("AGENTS.md", &format!("{NAMING_HEADING}\na hub\n")).is_empty(), "the section runs to the end of the file");
}

/// The deploy-only words (V2-10): held in deploy/README.md, not in the
/// other two; found as words of their own.
#[test]
fn the_pipeline_doc_has_words_of_its_own() {
    let text = "# deploy\nGone at v0.0.1: the webhook.\nset the webhook\nopen #/deploy\nsee #/deployments\nno landing-media\n";
    assert_eq!(
        stale(DEPLOY_README, text),
        [
            "deploy/README.md:3: set the webhook",
            "deploy/README.md:4: open #/deploy",
            "deploy/README.md:5: see #/deployments",
            "deploy/README.md:6: no landing-media",
        ]
    );
    assert_eq!(
        stale("README.md", text),
        ["README.md:5: see #/deployments"],
        "the deploy-only words are deploy/README.md's alone (`deployments` is everyone's)"
    );
    assert!(says("Webhooks, retried", "webhooks"));
    assert!(!says("deploy/pipeline landing", "landing-media"));
    let naming = format!("v0.0.1\n{NAMING_HEADING}\na hub\n");
    assert_eq!(stale(DEPLOY_README, &naming).len(), 2, "the naming allowance is AGENTS.md's alone");
    // The singular is an ordinary word there, and the log file's exact span is exempt there alone.
    assert!(stale(DEPLOY_README, "the test deployment\n").is_empty());
    assert_eq!(stale("README.md", "the test deployment\n"), ["README.md:1: the test deployment"]);
    assert!(stale(DEPLOY_README, "each run is a line in `~/.local/share/menger-deploy/deployments.log`\n").is_empty());
    assert_eq!(
        stale(DEPLOY_README, "deployments.log and deployments record\n"),
        ["deploy/README.md:1: deployments.log and deployments record"]
    );
    assert_eq!(stale("README.md", "see deployments.log\n"), ["README.md:1: see deployments.log"]);
    assert_eq!(failures(DEPLOY_README, ""), ["deploy/README.md is empty", "deploy/README.md never names v0.0.1: say what went and where it lives"]);
}

/// The paths the doc `text` names that begin `deploy/` or `scripts/`, each
/// `(line, path)`: in a backticked span or a bare word, starting the span or
/// the word (after whitespace, a backtick, `(`, `[`, `"`, `'` or `=`;
/// written `./deploy/…` it is taken without its `./`), running to
/// whitespace, a backtick, `)` or `,` (and, as no path here holds one, `"`,
/// `'`, `]`, `;` or `|`), a trailing `:` or `.` stripped. A placeholder —
/// `<`…`>`, `…`, a glob `*`, a variable `$` or a brace `{` — is skipped:
/// it names no one path. `menger-deploy/…` and `../deploy/…` do not begin
/// so and are not taken.
fn doc_paths(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for prefix in ["deploy/", "scripts/"] {
            for (at, _) in line.match_indices(prefix) {
                let head = &line[..at];
                let head = head.strip_suffix("./").unwrap_or(head);
                let before = head.chars().next_back();
                if before.is_some_and(|c| !(c.is_whitespace() || "`([\"'=".contains(c))) {
                    continue;
                }
                let rest = &line[at..];
                let end = rest.find(|c: char| c.is_whitespace() || "`),\"']|;".contains(c)).unwrap_or(rest.len());
                let path = rest[..end].trim_end_matches([':', '.']);
                if path.contains(['<', '>', '…', '*', '$', '{']) {
                    continue;
                }
                out.push((n + 1, path.to_string()));
            }
        }
    }
    out
}

/// V2-10, acceptance 1: every `deploy/…` and `scripts/…` path
/// deploy/README.md names is in the tree, as a file or a directory — the
/// doc names no script V2-9 removed and none that never was.
#[test]
fn the_pipeline_doc_names_what_exists() {
    let root = root();
    let text = std::fs::read_to_string(root.join(DEPLOY_README)).unwrap_or_else(|e| panic!("{DEPLOY_README}: {e}"));
    let paths = doc_paths(&text);
    let unnamed: Vec<&str> = ["deploy/pipeline", "deploy/record", "deploy/runner.mjs"]
        .into_iter()
        .filter(|want| !paths.iter().any(|(_, p)| p == want))
        .collect();
    assert!(unnamed.is_empty(), "V2-10: {DEPLOY_README} never names {unnamed:?}: the pipeline, its record and its runner (FRAME-8: the bundle's signer is ../plexus-ui's plexus-ui)");
    let missing: Vec<String> =
        paths.iter().filter(|(_, p)| !root.join(p).exists()).map(|(n, p)| format!("{DEPLOY_README}:{n}: {p}")).collect();
    assert!(missing.is_empty(), "V2-10: {} path(s) {DEPLOY_README} names are not in the tree:\n  {}", missing.len(), missing.join("\n  "));
}

/// The subcommands the script `text` accepts, from its dispatch — the last
/// `case $cmd in` … `esac`, what runs (the header comment is what the script
/// prints for an unknown word, and may lag it): `(live, retired)`, an arm
/// that calls a `cmd_…` live, one that does not (it says so and exits 2)
/// retired. `*)` is no subcommand.
fn script_subcommands(text: &str) -> (Vec<String>, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().rposition(|l| l.trim() == "case $cmd in").expect("deploy/pipeline has no `case $cmd in` dispatch");
    let (mut live, mut retired) = (Vec::new(), Vec::new());
    let mut i = start + 1;
    while i < lines.len() && lines[i].trim() != "esac" {
        let line = lines[i].trim();
        let words = line.split_once(')').map(|(w, _)| w).filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '-' || c == '|'));
        let Some(words) = words else {
            i += 1;
            continue;
        };
        let mut body = String::new();
        while i < lines.len() {
            body.push_str(lines[i]);
            body.push('\n');
            i += 1;
            if lines[i - 1].contains(";;") {
                break;
            }
        }
        let into = if body.contains("cmd_") { &mut live } else { &mut retired };
        into.extend(words.split('|').map(str::to_string));
    }
    (live, retired)
}

/// The subcommands the doc `text` names, each `(line, word)`: the word after
/// `deploy/pipeline ` at the start of a line inside a fenced block (after
/// any `NAME=value ` prefixes: `IMAGE_STORE=ghcr deploy/pipeline image`), or
/// at the start of a backticked span outside one. A flag, a placeholder or
/// nothing after it is no subcommand.
fn doc_subcommands(text: &str) -> Vec<(usize, String)> {
    const CMD: &str = "deploy/pipeline ";
    let word = |s: &str| -> Option<String> {
        let mut s = s.trim_start();
        loop {
            let first = s.split_whitespace().next()?;
            let Some((name, _)) = first.split_once('=') else { break };
            let mut chars = name.chars();
            if !chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
                break;
            }
            s = s[first.len()..].trim_start();
        }
        let s = s.strip_prefix(CMD)?;
        let w: String = s.trim_start().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        w.starts_with(|c: char| c.is_ascii_lowercase()).then_some(w)
    };
    let mut out = Vec::new();
    let mut fenced = false;
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            out.extend(word(line).map(|w| (n + 1, w)));
        } else {
            out.extend(line.split('`').skip(1).step_by(2).filter_map(word).map(|w| (n + 1, w)));
        }
    }
    out
}

/// V2-10, acceptance 2: deploy/README.md and `deploy/pipeline` agree on the
/// subcommands, each way. The script's are read from its dispatch `case`
/// (see [`script_subcommands`]) — what it does, not what its header says.
/// Every live one is named in the doc (its command table: a fenced line or a
/// backticked span); every one the doc names is one the script accepts,
/// live or retired (`install`, `deploy`, `sync`, `promote`, `rollback`,
/// `logs`, `media` — the doc may name them as retired, and need not).
#[test]
fn the_pipeline_doc_and_the_script_agree() {
    let root = root();
    let script = std::fs::read_to_string(root.join("deploy/pipeline")).expect("deploy/pipeline");
    let doc = std::fs::read_to_string(root.join(DEPLOY_README)).unwrap_or_else(|e| panic!("{DEPLOY_README}: {e}"));
    let (live, retired) = script_subcommands(&script);
    assert!(live.contains(&"build".to_string()) && retired.contains(&"media".to_string()), "the dispatch was not read: live {live:?}, retired {retired:?}");
    let named = doc_subcommands(&doc);
    let omitted: Vec<&String> = live.iter().filter(|w| !named.iter().any(|(_, n)| n == *w)).collect();
    let unknown: Vec<String> = named
        .iter()
        .filter(|(_, w)| !live.contains(w) && !retired.contains(w))
        .map(|(n, w)| format!("{DEPLOY_README}:{n}: deploy/pipeline {w}"))
        .collect();
    assert!(
        omitted.is_empty() && unknown.is_empty(),
        "V2-10: {DEPLOY_README} and deploy/pipeline disagree.\n{} subcommand(s) the script runs that the doc never names: {omitted:?}\n{} the doc names that the script has not:\n  {}",
        omitted.len(),
        unknown.len(),
        unknown.join("\n  ")
    );
}

/// The parsers check: the paths a line names and skips, the dispatch's
/// live and retired arms, the subcommands a doc names in a fence and a span.
#[test]
fn the_pipeline_doc_parsers_read_what_they_should() {
    let doc = "Run `deploy/pipeline build --branch v2` (see deploy/README.md).\n\
               The signer, deploy/ui-sign.mjs: and scripts/ui-build.sh, then deploy/env/.\n\
               Not menger-deploy/outbox, not ../deploy/x, not deploy/env/<env>.env nor deploy/… nor scripts/*.sh nor deploy/$X.\n\
               \"deploy/record\", [scripts/sweep/install.sh]|x; deploy/a.b.\n\
               MENGER_RUNNER=deploy/runner.mjs then ./scripts/disk-check.sh and `deploy/env/app-test.env`: there.\n";
    assert_eq!(
        doc_paths(doc),
        [
            (1, "deploy/pipeline".to_string()),
            (1, "deploy/README.md".to_string()),
            (2, "deploy/ui-sign.mjs".to_string()),
            (2, "deploy/env/".to_string()),
            (2, "scripts/ui-build.sh".to_string()),
            (4, "deploy/record".to_string()),
            (4, "deploy/a.b".to_string()),
            (4, "scripts/sweep/install.sh".to_string()),
            (5, "deploy/runner.mjs".to_string()),
            (5, "deploy/env/app-test.env".to_string()),
            (5, "scripts/disk-check.sh".to_string()),
        ]
    );

    let script = "case $cmd in\n  build|smoke)\n    LOG=1 ;;\nesac\ncmd_x() {\n  case $1 in\n    a) b ;;\n  esac\n}\n\
                  case $cmd in\n  install|deploy)\n    say retired\n    exit 2 ;;\n  media)\n    say gone\n    exit 2 ;;\n  build)    cmd_build \"$@\" ;;\n  remote-secret) cmd_remote_secret \"$@\" ;;\n  *) usage; exit 2 ;;\nesac\n";
    assert_eq!(
        script_subcommands(script),
        (vec!["build".to_string(), "remote-secret".to_string()], vec!["install".to_string(), "deploy".to_string(), "media".to_string()])
    );

    let doc = "# x\n```sh\ndeploy/pipeline build --branch v2 --no-gate   # a release\n  deploy/pipeline remote test --image I\n# deploy/pipeline status\nIMAGE_STORE=ghcr deploy/pipeline image R\ndeploy/pipeline --help\n```\n\
               `deploy/pipeline suite R` runs it; `deploy/pipeline` alone prints help; `deploy/pipeline <cmd>`; `remote-secret`; deploy/pipeline releases.\n\
               ```\ndeploy/pipeline remote-secret test\n```\n";
    assert_eq!(
        doc_subcommands(doc),
        [
            (3, "build".to_string()),
            (4, "remote".to_string()),
            (6, "image".to_string()),
            (9, "suite".to_string()),
            (11, "remote-secret".to_string()),
        ]
    );
}
