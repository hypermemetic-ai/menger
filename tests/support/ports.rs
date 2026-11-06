//! CI-9: the loopback port a test hands its server. Under cargo-nextest every test is a
//! process of its own, all running at once, so a port the OS hands out (`bind 127.0.0.1:0`,
//! then dropped) can be handed to another process before the server binds it — another
//! test's server, or any client socket's local end (the OS picks those from the same
//! ephemeral range, 32768–60999 here): "Address already in use", or a server that never
//! answers. Under nextest a test holds a slot no test running beside it holds
//! (`NEXTEST_TEST_GLOBAL_SLOT`), so each slot takes its own block of ports below the
//! ephemeral range, which no client socket is given; a port is handed out only once per
//! process and only while nothing listens on it. Under cargo test (one binary at a time,
//! its tests threads of one process) the OS's port, never handed twice in this process.
#![allow(dead_code)]

use std::sync::Mutex;

/// Ports per slot: a test's servers, clients' fixtures and restarts, many times over.
const BLOCK: u16 = 400;
/// The first port of slot 0 — below the ephemeral range, above the well-known services.
const BASE: u16 = 10000;

static HANDED: Mutex<Vec<u16>> = Mutex::new(Vec::new());

fn free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// A loopback port nothing listens on, never handed to another test running beside this one.
pub fn free_port() -> u16 {
    let mut handed = HANDED.lock().unwrap_or_else(|e| e.into_inner());
    let slot = std::env::var("NEXTEST_TEST_GLOBAL_SLOT").ok().and_then(|s| s.parse::<u16>().ok());
    if let Some(slot) = slot.filter(|s| u32::from(*s) * u32::from(BLOCK) + u32::from(BASE) + u32::from(BLOCK) < 32768) {
        let first = BASE + slot * BLOCK;
        // start where the clock says, so the next test in this slot does not retry the ports
        // this one's servers may still hold in TIME_WAIT
        let start = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_micros()).unwrap_or(0) % u32::from(BLOCK)) as u16;
        for i in 0..BLOCK {
            let port = first + (start + i) % BLOCK;
            if !handed.contains(&port) && free(port) {
                handed.push(port);
                return port;
            }
        }
        panic!("slot {slot}: no free port in {first}..{}", first + BLOCK);
    }
    loop {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        if !handed.contains(&port) {
            handed.push(port);
            return port;
        }
    }
}
