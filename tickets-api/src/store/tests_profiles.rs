//! IDENTITY-46: a principal's profile — kept from what its sign-in says,
//! each field the last word that named it, written only when it says
//! something new, and kept across a reopen.

use super::*;
use crate::members::{Profile, Scope};

fn all() -> crate::access::AdminOf {
    SYS.admin_of(&Scope::Everywhere)
}

const QQP: &str = "oidc:workos/user_01QQP";

fn github() -> Profile {
    Profile {
        principal: QQP.into(),
        provider: Some("workos".into()),
        backend: Some("workos".into()),
        method: Some("githuboauth".into()),
        subject: Some("user_01QQP".into()),
        email: Some("qqp@example.com".into()),
        email_verified: Some(true),
        name: Some("Q. Q. P.".into()),
        picture: Some("https://avatars.example.com/qqp.png".into()),
        signed_in_at: Some(1_000),
        ..Profile::default()
    }
}

#[tokio::test]
async fn a_sign_in_is_kept_and_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profiles.db");
    {
        let store = Store::open(&path).unwrap();
        assert!(store.profile(&all(), QQP).is_none());
        assert!(store.note_profile(SYS, github()).await.unwrap());
        assert!(store.knows_profile(&github()));
        assert!(!store.note_profile(SYS, github()).await.unwrap(), "the same word writes nothing");
    }
    let store = Store::open(&path).unwrap();
    let p = store.profile(&all(), QQP).unwrap();
    assert_eq!((p.method.as_deref(), p.email.as_deref(), p.email_verified), (Some("githuboauth"), Some("qqp@example.com"), Some(true)));
    assert!(p.first_seen > 0 && p.updated >= p.first_seen);
}

#[tokio::test]
async fn an_identity_that_says_less_keeps_what_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("profiles.db")).unwrap();
    store.note_profile(SYS, github()).await.unwrap();
    // A minted session: the name and the token's kind, no email, no subject.
    let minted = Profile { principal: QQP.into(), method: Some("oidc".into()), name: Some(" Q. Q. P. ".into()), ..Profile::default() };
    assert!(store.note_profile(SYS, minted).await.unwrap(), "the method moved");
    let p = store.profile(&all(), QQP).unwrap();
    assert_eq!(p.method.as_deref(), Some("oidc"));
    assert_eq!((p.email.as_deref(), p.subject.as_deref(), p.signed_in_at), (Some("qqp@example.com"), Some("user_01QQP"), Some(1_000)));
    // A blank says nothing; an older sign-in never moves the last one back.
    let blank = Profile { principal: QQP.into(), email: Some("  ".into()), signed_in_at: Some(10), method: Some("oidc".into()), ..Profile::default() };
    assert!(store.knows_profile(&blank));
    assert!(!store.note_profile(SYS, blank).await.unwrap());
}

#[tokio::test]
async fn profiles_list_the_newest_sign_in_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("profiles.db")).unwrap();
    store.note_profile(SYS, github()).await.unwrap();
    let later = Profile { principal: "key:abc".into(), provider: Some("key".into()), signed_in_at: Some(i64::MAX / 2), ..Profile::default() };
    store.note_profile(SYS, later).await.unwrap();
    let order: Vec<String> = store.profiles(&all()).into_iter().map(|p| p.principal).collect();
    assert_eq!(order, ["key:abc", QQP]);
    assert!(store.profiles(&SYS.admin_of(&Scope::stored("hypermemetic:").unwrap())).is_empty(), "an admin of less reads none");
    assert!(!store.note_profile(SYS, Profile { principal: " ".into(), ..github() }).await.unwrap(), "no principal, nothing kept");
}
