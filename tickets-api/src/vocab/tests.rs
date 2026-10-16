//! ACCESS-8's acceptance for the vocabulary: per type, `parse(display(x))
//! == x`; what ACCESS-5 lists as accepted today is accepted and what it
//! lists as refused is refused; the shortcode rules are one; the JSON is
//! the string it always was.

use super::*;
use crate::address::Code;
use crate::types::{Kind, Severity, Status};
use serde_json::json;

/// `text` read as `T`, through serde as the wire reads it.
fn de<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_value(json!(text)).map_err(|e| e.to_string())
}

/// Every accepted spelling reads, says itself, reads back to itself, and
/// serializes as the very string `Display` says; every refused one is
/// refused, by `FromStr` and by serde alike.
fn holds<T>(accepted: &[&str], refused: &[&str])
where
    T: std::str::FromStr<Err = String> + std::fmt::Display + PartialEq + std::fmt::Debug + serde::Serialize + serde::de::DeserializeOwned,
{
    for a in accepted {
        let x: T = a.parse().unwrap_or_else(|e| panic!("{a:?} refused: {e}"));
        assert_eq!(x.to_string().parse::<T>().as_ref(), Ok(&x), "{a:?} does not round-trip");
        assert_eq!(serde_json::to_value(&x).unwrap(), json!(x.to_string()), "{a:?}'s JSON");
        assert_eq!(de::<T>(a).as_ref(), Ok(&x), "{a:?} through serde");
    }
    for r in refused {
        assert!(r.parse::<T>().is_err(), "{r:?} accepted");
        assert!(de::<T>(r).is_err(), "{r:?} accepted through serde");
    }
}

#[test]
fn mng_1_and_its_upper_case_are_one_ticket_ref() {
    assert_eq!("mng-1".parse::<TicketRef>(), "MNG-1".parse::<TicketRef>());
    assert_eq!("mng-1".parse::<TicketRef>().unwrap().as_str(), "MNG-1");
    assert_eq!(de::<TicketRef>(" hypermemetic:menger/theme-3 ").unwrap().as_str(), "hypermemetic:MENGER/THEME-3");
    assert_eq!(de::<TicketRef>("plexus/_MENGER/board").unwrap().as_str(), "PLEXUS/_MENGER/board");
    assert!("PLEXUS/_MENGER/board".parse::<TicketRef>().unwrap().is_path());
    assert_eq!("theme.62-3".parse::<TicketRef>().unwrap().as_str(), "THEME.62-3");
}

#[test]
fn a_ticket_ref_takes_every_short_form_and_nothing_else() {
    holds::<TicketRef>(
        &["MNG-1", "mng-1", "MENGER/THEME-3", "hypermemetic:THEME-3", "PLEXUS/_MENGER/board", "THEME.62-3", "A1-0"],
        &["", " ", "MNG", "MNG-x", "THE ME-3", "_MENGER/board", "-3", "MNG-", "MENGER//THEME-3", "Acme:MNG-1", "THE.ME-3", "THEME-99999999999"],
    );
    let e = "MENGER".parse::<TicketRef>().unwrap_err();
    assert!(e.contains("names a shortcode"), "{e}");
}

#[test]
fn a_shortcode_ref_is_a_code_or_an_address_of_one() {
    holds::<ShortcodeRef>(
        &["MENGER", "menger", "PLEXUS/THEME", "acme:MENGER", "THEME.62", "A1"],
        &["", "MNG-1", "ME NGER", "x)", "A_B", "THE.ME", &"A".repeat(33)],
    );
}

#[test]
fn the_shortcode_rules_are_one() {
    // ACCESS-5 found six; each now asks `Code::parse`, so each takes the
    // same codes as a bare shortcode, and refuses the same.
    // ORG-9: and the same hyphenated codes, refusing the same malformed ones.
    for text in [
        "MENGER", "menger", "A1", "A_B", "a-b", "", " ", "THE ME", "THE.ME", &"A".repeat(32), &"A".repeat(33),
        "SYNAPSE-CC", "synapse-cc", "A-B-C", "X-1B", "A-", "-A", "A--B", "A-1", "A-B-2",
    ] {
        let code = Code::parse(text).is_ok();
        assert_eq!(text.parse::<ShortcodeRef>().is_ok() && !text.contains('.'), code, "ShortcodeRef {text:?}");
        assert_eq!(text.parse::<Project>().is_ok(), code, "Project {text:?}");
        assert_eq!(crate::deployments::project_of(text).is_ok(), code, "project_of {text:?}");
        // ACCESS-21: a scope is a ticket's too (`A-1`, `A-B-2`).
        let ticket = crate::address::Address::parse(text).is_ok_and(|a| a.names_ticket());
        assert_eq!(crate::members::scope_of(text).is_ok() && text.trim() != "*", code || ticket, "scope_of {text:?}");
        let in_filter = crate::filter::Filter::parse(&format!("in {text}")).is_ok();
        if !text.trim().is_empty() && !text.contains(' ') {
            assert_eq!(in_filter, code, "`in {text}`");
        }
    }
}

#[test]
fn a_cursor_is_a_page_s_next_folded() {
    holds::<Cursor>(&["MNG-9", "mng-9", "THEME.62-3"], &["", "MNG 9"]);
    assert_eq!("mng-9".parse::<Cursor>().unwrap().as_str(), "MNG-9");
}

#[test]
fn a_menger_path_starts_with_menger_and_names_something() {
    holds::<MengerPath>(&["_MENGER/board", "_MENGER/view/x"], &["", "_MENGER/", "MENGER/board", "board"]);
}

#[test]
fn a_commit_ref_is_moved_here_unchanged() {
    holds::<CommitRef>(&["77df9a0", "77DF9A0", "plexus-macros@4f7114a", &"a".repeat(64)], &["", "77df9a", " 77df9a0", "@77df9a0", "x@y@77df9a0", "ghijklm", &"a".repeat(65)]);
    let c: CommitRef = "Repo.x@ABCDEF0".parse().unwrap();
    assert_eq!((c.as_str(), c.repo(), c.hash()), ("Repo.x@abcdef0", Some("Repo.x"), "abcdef0"));
}

#[test]
fn a_label_is_the_label_rule() {
    holds::<Label>(&["needs-you", "Needs-You", "a1"], &["", "a b", "a_b", &"a".repeat(33)]);
    assert_eq!("Needs-You".parse::<Label>().unwrap().as_str(), "needs-you");
}

#[test]
fn the_small_names_hold_their_rules() {
    holds::<InputId>(&["landing", " landing ", "a b"], &["", " "]);
    holds::<ComponentName>(&["m-chart", "x-y.z"], &["", "chart", "M-chart", "1-a"]);
    holds::<Referrer>(&["m-chart", "MNG-1"], &["", "chart"]);
    assert!("m-chart".parse::<Referrer>().unwrap().is_component() && !"MNG-1".parse::<Referrer>().unwrap().is_component());
    holds::<Route>(&["#/", "#/ticket/DEMAND-2"], &["", "/ticket", "ticket"]);
    holds::<AnchorPath>(&["[data-tag=\"ticket DEMAND-1\"]"], &["", " "]);
    holds::<FacetField>(&["status", "has commits", "in"], &["", "colour", "Status"]);
    holds::<EnvironmentName>(&["prod", "Prod", "review/pr-412"], &["", "-x", "a b"]);
    holds::<Release>(&["20261012T220402Z-235ff62"], &["", "a b"]);
    holds::<ThemeName>(&["ocean", "Ocean"], &["", "1x", "a b", &"a".repeat(41)]);
    holds::<ThemeScope>(&["site", "SITE", "PANTRY", "pantry"], &["", "PANTRY-4"]);
    assert!("Site".parse::<ThemeScope>().unwrap().is_site());
    holds::<TokenName>(&["--accent", "accent"], &["", "---accent", "Accent", "--"]);
    holds::<TokenValue>(&["#00796b", "1px solid red"], &["", "red;", "a{", &"a".repeat(201)]);
    holds::<Under>(&["FILTER", "FILTER-9", "menger/theme"], &["", "a b"]);
}

#[test]
fn a_principal_is_the_identity_layer_s_grammar() {
    let uuid = "6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b";
    let hex = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
    let ok = [format!("idp:{uuid}"), format!("nostr:{hex}"), format!("key:{hex}"), "oidc:auth0/google-oauth2|1234".into(), "apikey:ci-1".into()];
    let bad = [
        "".to_string(),
        "nobody".into(),
        format!("idp:{}", uuid.to_uppercase()),
        "idp:not-a-uuid".into(),
        format!("nostr:{}", &hex[..63]),
        format!("key:{}", hex.to_uppercase()),
        "oidc:Auth0/x".into(),
        "oidc:auth0".into(),
        "oidc:auth0/".into(),
        "apikey:a\"b".into(),
        "idp:a:b".into(),
        "team:x".into(),
    ];
    holds::<Principal>(&ok.iter().map(String::as_str).collect::<Vec<_>>(), &bad.iter().map(String::as_str).collect::<Vec<_>>());
    // The server's identity layer answers the same, every case.
    #[cfg(feature = "server")]
    for t in ok.iter().chain(&bad) {
        let ours = t.trim().parse::<Principal>().is_ok();
        let theirs = t.trim().parse::<plexus_auth_core::identity::Principal>().is_ok();
        assert_eq!(ours, theirs, "{t:?}");
    }
}

#[test]
fn who_granted_and_who_holds_read_as_their_text() {
    let p = "idp:6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b";
    holds::<GrantedBy>(&[p, OPERATOR], &["", "nobody"]);
    assert_eq!(OPERATOR.parse::<GrantedBy>(), Ok(GrantedBy::Operator));
    holds::<Grantee>(&[p, "team:release-crew"], &["", "team:", "team:Release", "nobody"]);
    assert!(matches!("team:release-crew".parse::<Grantee>(), Ok(Grantee::Team(_))));
    holds::<TeamName>(&["release-crew", "a1"], &["", "1a", "a-", "A", &"a".repeat(33)]);
}

#[test]
fn a_scope_ref_is_scope_of_and_a_ceiling_its_entry() {
    holds::<ScopeRef>(
        &["*", "IDENTITY", "identity", "acme:", "acme:menger/theme", "IDENTITY-3", "identity-3", "under IDENTITY-3", "under menger/theme-3"],
        &["", "a b", "under X", "under IDENTITY", "under *", "Acme:"],
    );
    assert_eq!("identity".parse::<ScopeRef>().unwrap().as_str(), "IDENTITY");
    holds::<Ceiling>(&["editor@IDENTITY", "Editor@identity", "viewer@*", "tickets.get@MENGER-12"], &["", "editor", "editor@", "a b@*", "tickets.@*"]);
    assert_eq!("Admin@menger-12".parse::<Ceiling>().unwrap().as_str(), "admin@MENGER-12");
}

#[test]
fn a_kept_scope_reads_back_from_its_text() {
    use crate::members::Scope;
    for text in ["*", "acme:", "hypermemetic:THEME.62", "acme:MENGER"] {
        let s: Scope = de(text).unwrap();
        assert_eq!(serde_json::to_value(&s).unwrap(), json!(text));
    }
    for old in ["IDENTITY", "acme:menger", "Acme:"] {
        assert!(de::<Scope>(old).is_err(), "{old}");
    }
}

#[test]
fn prose_has_a_name_and_one_rule_each() {
    holds::<Title>(&["Wrap long titles", " padded "], &["", "   ", &"a".repeat(crate::limits::TITLE_MAX + 1)]);
    assert_eq!(" padded ".parse::<Title>().unwrap().as_str(), " padded ", "a title is kept as written");
    holds::<Body>(&["", "# a body\n\nwith $ and `code`"], &[]);
    holds::<Message>(&["folded into MNG-9", "", "  why  "], &["a\u{7}b"]);
    assert_eq!("  why  ".parse::<Message>().unwrap().given().as_deref(), Some("why"));
    assert_eq!("".parse::<Message>().unwrap().given(), None);
    holds::<Reason>(&["the import waits on this"], &["", "  "]);
    holds::<Query>(&["watch", "", "plcement wave"], &["a\u{0}b", &"a".repeat(QUERY_MAX + 1)]);
    holds::<Author>(&["owner", " Ada "], &["", "  ", &"a".repeat(257)]);
}

#[test]
fn a_filter_text_is_one_the_language_reads() {
    holds::<FilterText>(&["", "ready and kind build and in SCALE", "status \"In Review\" and entered \"In Review\" within 12h", "under FILTER-9"], &["kind wobbly", "status (", &"a".repeat(crate::limits::FILTER_MAX + 1)]);
    let f: FilterText = "kind build".parse().unwrap();
    assert_eq!(f.as_str(), "kind build", "kept as written");
    assert!(!f.filter().is_empty());
}

#[test]
fn a_zone_and_a_time_are_checked_for_their_shape() {
    holds::<Zone>(&["UTC", "Europe/Berlin", "America/Argentina/Buenos_Aires", "+02:00", "-0530", "+5", "Etc/GMT+2"], &["", "+25:00", "+02:60", "Europe Berlin", "1abc", "+x"]);
    holds::<Timestamp>(&["2026-10-12T22:04:19Z", "2026-10-12T22:04:19.5+02:00", "1792068529"], &["", "yesterday", "2026-10-12", "2026-10-12T22:04:19"]);
}

#[test]
fn the_enums_read_every_alias_and_say_their_canonical_spelling() {
    for (alias, canonical) in [("active", Status::Coding), ("in review", Status::InReview), ("in_review", Status::InReview), ("in-review", Status::InReview), ("inreview", Status::InReview), ("READY", Status::Ready), ("cancelled", Status::Canceled)] {
        assert_eq!(de::<Status>(alias), Ok(canonical), "{alias}");
    }
    assert!(de::<Status>("wobbly").unwrap_err().contains("unknown status"));
    assert_eq!(serde_json::to_value(Status::InReview).unwrap(), json!("In Review"));
    assert_eq!(de::<Kind>("Build"), Ok(Kind::Build));
    assert_eq!(de::<Severity>("high"), Ok(Severity::High));
    assert_eq!(de::<crate::members::Role>(" Editor "), Ok(crate::members::Role::Editor));
    assert_eq!(de::<CallerSource>("synapse"), Ok(CallerSource::Api));
    assert_eq!(de::<CallerSource>("merge"), Ok(CallerSource::Pipeline));
    assert!(de::<CallerSource>("cascade").unwrap_err().contains("the store's own"));
    assert!(de::<InputSource>("system").unwrap_err().contains("the store's own"));
    assert_eq!(de::<SeverityChange>("None"), Ok(SeverityChange::Clear));
    assert_eq!(de::<SeverityChange>("low"), Ok(SeverityChange::Set(Severity::Low)));
    assert_eq!(serde_json::to_value(SeverityChange::Clear).unwrap(), json!("none"));
    assert_eq!(de::<DeleteChildren>("Reparent"), Ok(DeleteChildren::Reparent));
    assert_eq!(de::<AmendedField>("title"), Ok(AmendedField::Title));
    // The schema stays the closed enum of the canonical spellings — an
    // `enum`, or (variants with docs) a `oneOf` of `enum`s and `const`s.
    let schema = serde_json::to_value(schemars::schema_for!(Status)).unwrap();
    let mut said: Vec<String> = Vec::new();
    for part in schema["oneOf"].as_array().cloned().unwrap_or_else(|| vec![schema.clone()]) {
        said.extend(part["enum"].as_array().into_iter().flatten().chain(part.get("const")).filter_map(|v| v.as_str().map(str::to_string)));
    }
    said.sort();
    let mut canonical: Vec<String> = Status::ALL.iter().map(|s| s.as_str().to_string()).collect();
    canonical.sort();
    assert_eq!(said, canonical, "{schema}");
}

#[test]
fn every_enum_a_method_takes_is_a_closed_enum_in_the_schema() {
    // A `oneOf` of `const`s (what a derive makes of documented variants)
    // reads to synapse as a tagged union: `need --status.type`.
    fn closed<T: schemars::JsonSchema>() -> Vec<String> {
        let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
        assert!(schema.get("oneOf").is_none() && schema["type"] == "string", "{schema}");
        schema["enum"].as_array().expect("an enum").iter().map(|v| v.as_str().unwrap().to_string()).collect()
    }
    assert_eq!(closed::<Status>().len(), Status::ALL.len());
    assert_eq!(closed::<Kind>(), Kind::ALL.map(|k| k.as_str().to_string()));
    assert_eq!(closed::<Severity>(), ["Critical", "High", "Medium", "Low"]);
    assert_eq!(closed::<crate::types::InputState>().len(), 5);
    assert_eq!(closed::<crate::members::Role>(), ["viewer", "deployer", "editor", "admin"]);
    assert_eq!(closed::<crate::comments::CommentState>(), ["open", "resolved"]);
    assert_eq!(closed::<crate::notes::NoteState>(), ["open", "upgraded"]);
    assert_eq!(closed::<crate::deployments::DeployState>(), ["started", "succeeded", "failed", "rolled_back"]);
    assert_eq!(closed::<crate::queue::Urgency>(), ["whenever", "today", "now", "blocking"]);
    assert_eq!(closed::<crate::themes::ThemeReach>(), ["frame", "ticket", "site"]);
    assert_eq!(closed::<crate::themes::Takeover>(), ["on", "off"]);
    assert_eq!(closed::<CallerSource>(), ["app", "api", "pipeline"]);
    assert_eq!(closed::<InputSource>(), ["app", "api"]);
    assert_eq!(closed::<DeleteChildren>(), ["reparent", "delete"]);
    assert_eq!(closed::<SeverityChange>(), ["Critical", "High", "Medium", "Low", "none"]);
}

#[test]
fn a_text_type_is_a_named_string_in_the_schema() {
    #[derive(schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Params {
        id: TicketRef,
        status: Status,
    }
    let schema = serde_json::to_value(schemars::schema_for!(Params)).unwrap();
    assert_eq!(schema["properties"]["id"]["$ref"], json!("#/$defs/TicketRef"), "{schema}");
    assert_eq!(schema["$defs"]["TicketRef"]["type"], json!("string"), "{schema}");
}

#[test]
fn a_shortcode_may_hold_hyphens_and_a_ticket_splits_at_its_last() {
    // ORG-9 (owner, 2026-10-16: "synapse-cc-1 should be a valid name … only
    // the final -N is protected").
    holds::<TicketRef>(
        &["SYNAPSE-CC-1", "synapse-cc-1", "PLEXUS/SYNAPSE-CC-12", "hypermemetic:SYNAPSE/SYNAPSE-CC-1", "SYNAPSE-CC.2-3", "SYNAPSE-CC/_MENGER/board"],
        &["A--B-1", "A-1-2", "-A-1", "A--B.2-3", "SYNAPSE-CC-", "SYNAPSE-CC"],
    );
    assert_eq!("synapse-cc-1".parse::<TicketRef>().unwrap().as_str(), "SYNAPSE-CC-1");
    holds::<ShortcodeRef>(&["SYNAPSE-CC", "synapse-cc", "PLEXUS/SYNAPSE-CC", "acme:SYNAPSE-CC", "SYNAPSE-CC.2"], &["A-", "-A", "A--B", "A-1", "SYNAPSE-CC-1"]);
    assert_eq!("synapse-cc".parse::<ShortcodeRef>().unwrap().as_str(), "SYNAPSE-CC");
    let e = "A-1".parse::<ShortcodeRef>().unwrap_err();
    assert!(e.contains("names a ticket"), "{e}");
    holds::<Under>(&["SYNAPSE-CC", "SYNAPSE-CC-1"], &["A--B"]);
    holds::<Project>(&["SYNAPSE-CC"], &["A-1", "A--B"]);
    holds::<ThemeScope>(&["SYNAPSE-CC"], &["A-1"]);
    holds::<ScopeRef>(&["SYNAPSE-CC", "acme:SYNAPSE-CC", "SYNAPSE-CC-1", "under SYNAPSE-CC-1"], &["A--B", "under SYNAPSE-CC"]);
    holds::<Ceiling>(&["editor@SYNAPSE-CC", "tickets.get@SYNAPSE-CC-1"], &["editor@A--B"]);
    assert_eq!("viewer@synapse-cc-1".parse::<Ceiling>().unwrap().as_str(), "viewer@SYNAPSE-CC-1");
    assert_eq!("viewer@synapse-cc".parse::<Ceiling>().unwrap().as_str(), "viewer@SYNAPSE-CC");
    // A kept scope holds a hyphenated identity, and never a ticket's key.
    use crate::members::Scope;
    for text in ["hypermemetic:SYNAPSE-CC", "hypermemetic:SYNAPSE-CC.2"] {
        let s: Scope = de(text).unwrap();
        assert_eq!(serde_json::to_value(&s).unwrap(), json!(text));
    }
    for bad in ["hypermemetic:SYNAPSE-CC-1", "hypermemetic:A--B", "hypermemetic:-A"] {
        assert!(de::<Scope>(bad).is_err(), "{bad}");
    }
}
