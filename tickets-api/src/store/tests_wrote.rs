//! TICKETS-39: every change a write broadcasts goes on the attributed
//! channel with the writer the write ran as — the caller's act, or the act a
//! write is handed — and a change outside any call's act names no one.

use super::history::Act;
use super::*;

fn new(epic: &str, title: &str) -> NewTicket {
    NewTicket { path: None, epic: epic.into(), title: title.into(), kind: Kind::Scope, severity: None, blocked_by: Vec::new(), parent: None, body: String::new() }
}

fn as_(actor: &str) -> Act {
    Act { actor: actor.into(), name: Some(actor.trim_start_matches("idp:").into()), ..Act::default() }
}

fn drained(rx: &mut broadcast::Receiver<Wrote>) -> Vec<(Option<String>, &'static str)> {
    let mut out = Vec::new();
    while let Ok(w) = rx.try_recv() {
        let kind = match &w.change {
            Change::Created { .. } => "created",
            Change::Amended { .. } => "amended",
            Change::CellsMoved { .. } => "cells",
            Change::Revised { .. } => "revised",
            _ => "other",
        };
        out.push((w.by.as_ref().map(|b| b.actor.clone()), kind));
    }
    out
}

#[tokio::test]
async fn each_change_carries_the_writer_of_its_write() {
    let store = Store::new();
    store.create(SYS, new("W", "one")).await.unwrap();
    let mut rx = store.subscribe_wrote();
    acting(as_("idp:ada"), store.amend(SYS.permit("W-1"), Some("ada's".into()), None)).await.unwrap();
    acting(as_("idp:bob"), store.amend(SYS.permit("W-1"), Some("bob's".into()), None)).await.unwrap();
    let heard = drained(&mut rx);
    let by = |who: &str| heard.iter().filter(|(b, _)| b.as_deref() == Some(who)).map(|(_, k)| *k).collect::<Vec<_>>();
    assert!(by("idp:ada").contains(&"amended") && by("idp:ada").contains(&"revised"), "{heard:?}");
    assert!(by("idp:bob").contains(&"amended"), "{heard:?}");
    assert_eq!(by("idp:ada").len() + by("idp:bob").len(), heard.len(), "every change is one writer's: {heard:?}");
    // Outside any call's act: no one.
    store.amend(SYS.permit("W-1"), Some("nobody's".into()), None).await.unwrap();
    assert!(drained(&mut rx).iter().all(|(b, _)| b.is_none()));
}

#[tokio::test]
async fn a_write_handed_an_act_is_that_act_s() {
    let store = Store::new();
    store.create(SYS, new("W", "one")).await.unwrap();
    let mut rx = store.subscribe_wrote();
    let ask = Update { title: Some("by hand".into()), ..Update::default() };
    store.update_by(SYS.updating("W-1"), ask, as_("idp:cy")).await.unwrap();
    let heard = drained(&mut rx);
    assert!(!heard.is_empty() && heard.iter().all(|(b, _)| b.as_deref() == Some("idp:cy")), "{heard:?}");
}

#[tokio::test]
async fn nobody_listening_costs_nothing() {
    let store = Store::new();
    store.create(SYS, new("W", "one")).await.unwrap();
    assert_eq!(store.tx.wrote.receiver_count(), 0);
    acting(as_("idp:ada"), store.amend(SYS.permit("W-1"), Some("t".into()), None)).await.unwrap();
}
