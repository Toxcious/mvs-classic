//! The net service's custom lobbies against the oracle's: its netproc test (its memory half) as the tape has it,
//! every page event and backend call equal (common/tape.rs).

#[path = "common/tape.rs"]
mod tape;

#[test]
fn netproc_tape() {
    let Some(t) = tape::load("app/tests/netproc.ts") else { return };
    let r = tape::replay(&t);
    // Deviation (KNOCK_WAIT_MS): the host waits out the guest's whole knock window, 12 s for the oracle's 5 s, so the
    // kick of the member that never links (the tape's at 29790) comes after the tape's end.
    // World 1's service 0 is compared up to its match's away (tape::SvcReport::cut).
    let known = [tape::Known { world: 2, service: 0, tape_t: 29790.0, port_t: None }];
    tape::check("netproc", &r, &known);
}
