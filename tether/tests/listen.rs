//! TETHER-9: tether listens to a throwaway menger and acts on it — an
//! answer resumes the session that asked or starts one after the grace, an
//! input closed in the grace is left alone, a change request and a comment
//! on a held ticket are acted on, and a restart keeps the sessions, what
//! each asked, and the position (what came while it was down is acted on
//! once, nothing before twice).

mod support;

use serde_json::{json, Value};
use support::*;

fn input(id: &str, q: &str) -> String {
    format!("<m-input id=\"{id}\" kind=\"text\"><m-question data-field=\"question\">{q}</m-question></m-input>\n")
}

async fn create(m: &jsonrpsee::ws_client::WsClient, title: &str, body: &str) -> String {
    let made = one(m, "menger", "tickets.create", json!({ "epic": "TT", "title": title, "kind": "build", "parent": "TT-1", "body": body })).await;
    made["ticket"]["id"].as_str().or(made["id"].as_str()).unwrap_or_else(|| panic!("{made}")).to_string()
}

async fn answer(m: &jsonrpsee::ws_client::WsClient, ticket: &str, input: &str, text: &str) {
    one(m, "menger", "tickets.inputs.answer", json!({ "ticket": ticket, "input": input, "value": { "type": "text", "text": text } })).await;
}

async fn sessions(t: &jsonrpsee::ws_client::WsClient, ticket: &str) -> Vec<Value> {
    call(t, "tether", "sessions.list", json!({ "ticket": ticket })).await.unwrap().into_iter().map(|e| e["session"].clone()).collect()
}

/// The sessions on `ticket` once they number `n` and none runs.
async fn settled(t: &jsonrpsee::ws_client::WsClient, ticket: &str, n: usize, turns: u64) -> Vec<Value> {
    until(&format!("{n} settled session(s) on {ticket} at {turns} turns"), || async {
        let s = sessions(t, ticket).await;
        (s.len() == n && s.iter().all(|s| s["state"] != "running") && s.iter().any(|s| s["turns"].as_u64() == Some(turns))).then_some(s)
    })
    .await
}

async fn connected(t: &jsonrpsee::ws_client::WsClient) -> Value {
    until("tether listening", || async {
        let s = call(t, "tether", "listen.status", json!({})).await.ok()?;
        (s.first()?["status"]["connected"] == true).then(|| s[0]["status"].clone())
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn tether_listens_acts_and_keeps_its_place_across_a_restart() {
    let tmp = scratch();
    let dir = tmp.path();
    let (_menger, mp) = menger(dir).await;
    let owner = signed_in(&OWNER, mp).await;
    let agent = signed_in(&AGENT, mp).await;
    one(&owner, "menger", "tickets.create", json!({ "epic": "TT", "title": "tether's tests", "kind": "scope" })).await;
    let tt2 = create(&owner, "answered, nobody holds it", &input("pick", "Which colour?")).await;
    let tt3 = create(&owner, "closed in the grace", &input("q", "Go?")).await;
    let tt4 = create(&owner, "a session asks", "work").await;
    let tt5 = create(&owner, "changes requested", "work").await;
    let tt6 = create(&owner, "answered while tether is down", &input("late", "And?")).await;

    let tp = free_port();
    let tether_proc = tether(dir, mp, tp, &["--accept-keys", "--grace-secs", "1"]).await;
    let t = signed_in(&OPERATOR, tp).await;
    let status = connected(&t).await;
    assert_eq!(status["principal"], principal_of(&AGENT), "{status}");

    // 1. An answer no session of tether's asked: after the grace, a session
    //    on the ticket, told the answer and how to close it.
    answer(&owner, &tt2, "pick", "blue").await;
    let s = settled(&t, &tt2, 1, 1).await;
    assert_eq!(s[0]["origin"], "answer", "{s:?}");
    assert_eq!(s[0]["state"], "finished");
    let first = calls(dir).into_iter().find(|c| c.contains(&format!("`pick` on {tt2}"))).expect("the stub was told the answer");
    assert!(first.contains("blue") && first.contains(&format!("inputs close --ticket {tt2} --input pick")), "{first}");

    // 2. An answer another agent acts on in the grace (closes it): left alone.
    answer(&owner, &tt3, "q", "yes").await;
    one(&agent, "menger", "tickets.inputs.close", json!({ "ticket": tt3, "input": "q" })).await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert!(sessions(&t, &tt3).await.is_empty(), "closed in the grace: not tether's");

    // 3. A session asks (an input appears on its ticket while its turn
    //    runs); the answer resumes that session, not a new one.
    let started = one(&t, "tether", "sessions.start", json!({ "ticket": tt4, "prompt": "SLOW: ask the owner", "model": "haiku" })).await;
    let name = started["session"]["name"].as_str().unwrap().to_string();
    one(&agent, "menger", "tickets.amend", json!({ "id": tt4, "body": format!("work\n{}", input("color", "Which?")) })).await;
    let s = settled(&t, &tt4, 1, 1).await;
    assert_eq!(s[0]["asked"], json!(["color"]), "{s:?}");
    answer(&owner, &tt4, "color", "red").await;
    let s = settled(&t, &tt4, 1, 2).await;
    assert_eq!(s[0]["name"], name.as_str());
    let resumed = calls(dir).into_iter().find(|c| c.contains(&format!("`color` on {tt4}"))).expect("resumed with the answer");
    assert!(resumed.contains("--resume stub-session") && resumed.contains("red"), "{resumed}");

    // 4. A change request on a queued ticket: a session that makes them.
    one(&agent, "menger", "tickets.queue", json!({ "id": tt5, "urgency": "now", "reason": "look" })).await;
    one(&owner, "menger", "tickets.request_changes", json!({ "id": tt5, "body": "make it teal" })).await;
    let s = settled(&t, &tt5, 1, 1).await;
    assert_eq!(s[0]["origin"], "reply", "{s:?}");
    assert!(calls(dir).iter().any(|c| c.contains("requested changes") && c.contains("make it teal")));

    // 5. A comment on a ticket a session asked on resumes it; one on a
    //    ticket nobody holds and nothing queues is not tether's.
    let anchor = |id: &str| json!({ "route": format!("#/ticket/{id}"), "path": "body", "label": "Body" });
    one(&owner, "menger", "comments.add", json!({ "anchor": anchor(&tt4), "body": "also blue" })).await;
    one(&owner, "menger", "comments.add", json!({ "anchor": anchor(&tt3), "body": "just a note" })).await;
    settled(&t, &tt4, 1, 3).await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(sessions(&t, &tt3).await.is_empty());

    // 6. tether goes down; an answer comes; tether comes back.
    drop(tether_proc);
    answer(&owner, &tt6, "late", "while you were out").await;
    let _tether_again = tether(dir, mp, tp, &["--accept-keys", "--grace-secs", "1"]).await;
    let t = signed_in(&OPERATOR, tp).await;
    connected(&t).await;
    let s = settled(&t, &tt6, 1, 1).await;
    assert_eq!(s[0]["origin"], "answer");
    // What it kept: TT-4's session and what it asked; nothing acted on twice.
    let s4 = sessions(&t, &tt4).await;
    assert_eq!((s4.len(), s4[0]["asked"].clone(), s4[0]["turns"].as_u64()), (1, json!(["color"]), Some(3)), "{s4:?}");
    assert_eq!(sessions(&t, &tt2).await.len(), 1, "the answer before the restart is not acted on again");
    // and an answer after it still finds the session that asked.
    answer(&owner, &tt4, "color", "green").await;
    settled(&t, &tt4, 1, 4).await;
    let st = call(&t, "tether", "listen.status", json!({})).await.unwrap();
    assert!(st[0]["status"]["acted"].as_u64().unwrap() >= 2, "{st:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_takes_its_archetypes_model_and_posts_its_report() {
    let tmp = scratch();
    let dir = tmp.path();
    std::fs::write(dir.join("archetypes.env"), "MENGER_MODEL_ANALYST=claude-haiku-5-5\n").unwrap();
    let (_menger, mp) = menger(dir).await;
    let owner = signed_in(&OWNER, mp).await;
    let agent = signed_in(&AGENT, mp).await;
    one(&owner, "menger", "tickets.create", json!({ "epic": "TT", "title": "tether's tests", "kind": "scope" })).await;
    let tt2 = create(&owner, "a run", "work").await;
    let chat = one(&agent, "menger", "chats.open", json!({ "ticket": tt2 })).await;
    let chat_id = chat["chat"]["id"].as_u64().unwrap_or_else(|| panic!("{chat}"));

    let tp = free_port();
    let _tether = tether(dir, mp, tp, &["--no-listen", "--permission-mode", "bypassPermissions"]).await;
    let t = signed_in(&OPERATOR, tp).await;

    let started = one(&t, "tether", "runs.start", json!({ "ticket": tt2, "brief": "read the ticket and report", "archetype": "analyst", "chat": chat_id })).await;
    let run = &started["session"];
    assert_eq!((run["origin"].as_str(), run["model"].as_str(), run["archetype"].as_str()), (Some("run"), Some("haiku"), Some("analyst")), "{run}");
    let name = run["name"].as_str().unwrap().to_string();

    // Its report, in the chat, by tether's key.
    let said = until("the report in the chat", || async {
        let ms = call(&agent, "menger", "chats.messages", json!({ "chat": chat_id })).await.ok()?;
        ms.into_iter().find(|m| m.to_string().contains(&name))
    })
    .await;
    assert!(said.to_string().contains("stub did it") && said.to_string().contains("finished"), "{said}");
    let c = calls(dir).join("\n");
    assert!(c.contains("--permission-mode bypassPermissions") && c.contains("read the ticket and report") && c.contains("--model haiku"), "{c}");

    // Listed as a run (and a session); a run that runs is stopped.
    let runs = call(&t, "tether", "runs.list", json!({})).await.unwrap();
    assert!(runs.iter().any(|r| r["session"]["name"] == name.as_str()), "{runs:?}");
    let long = one(&t, "tether", "runs.start", json!({ "ticket": tt2, "brief": "FOREVER" })).await;
    assert_eq!(long["session"]["model"], "opus", "a builder, by the built-in default");
    let long = long["session"]["name"].as_str().unwrap().to_string();
    let stopped = one(&t, "tether", "runs.stop", json!({ "name": long })).await;
    assert_eq!(stopped["type"], "stopped", "{stopped}");
    let st = one(&t, "tether", "listen.status", json!({})).await;
    assert_eq!(st["type"], "off");
}
