//! The net service's queue and party against the oracle's: its queue and party tests (their memory
//! halves) as the tape has them, every page event and backend call equal (common/tape.rs).

#[path = "common/tape.rs"]
mod tape;

#[test]
fn queue_tape() {
    let Some(t) = tape::load("app/tests/queue.ts") else { return };
    tape::check("queue", &tape::replay(&t), &[]);
}

#[test]
fn party_tape() {
    let Some(t) = tape::load("app/tests/party.ts") else { return };
    // Deviation: the tape's worlds 3 to 9 queue a party on a relay by lobby search (the leader's lobby, the member's
    // follow), which a relay no longer does: its one queue core holds every queuer (relay_host.rs). They are not
    // compared; relay_queue.rs's party tests check that queue (a party filled with bots, with a solo queuer, the party
    // ending while it queues, a member who never joins). Worlds 0 to 2 (presence, the party itself) are.
    let mut r = tape::replay(&t);
    r.retain(|s| !(3..=9).contains(&s.world));
    tape::check("party", &r, &[]);
}
