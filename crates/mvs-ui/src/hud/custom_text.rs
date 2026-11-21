//! The words of the port's own HUD features, which the beta has no line for.

/// The countdown card's win condition line under the experimental stock rule. style: the TeamStyle (0 1v1, 1 2v2,
/// 2 Free For All).
pub fn stock_win_condition(style: usize, stocks: i32) -> String {
    let lives = format!("{stocks} {}", if stocks == 1 { "stock" } else { "stocks" });
    if style == 1 {
        format!("Every player has {lives}, last team standing wins")
    } else {
        format!("Every player has {lives}, last one standing wins")
    }
}

/// (Waves) The run's counter over the scoreboard: the ring-outs so far, out of the run's target in 100 Ring-Outs.
pub fn waves_counter(ring_outs: u32, target: Option<u32>) -> String {
    match target {
        Some(t) => format!("RING-OUTS {ring_outs} / {t}"),
        None => format!("RING-OUTS {ring_outs}"),
    }
}

/// (Waves) The run's clock in the scoreboard's clock: "m:ss", with hundredths ("m:ss.cc") when the run is against
/// the clock (100 Ring-Outs).
pub fn waves_clock(seconds: f64, hundredths: bool) -> String {
    // (whole hundredths, so the shown parts never round up past each other)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a clamped, floored count")]
    let cs = (seconds.max(0.0) * 100.0).floor().min(f64::from(u32::MAX)) as u64;
    let (m, s) = (cs / 6000, cs / 100 % 60);
    if hundredths { format!("{m}:{s:02}.{:02}", cs % 100) } else { format!("{m}:{s:02}") }
}

/// (Chaos Wheel) The rule's name: the card's label while a mutator is on, its name line in the plain first window,
/// and the countdown card's line under the win condition.
pub const WHEEL_RULE: &str = "CHAOS WHEEL";
/// (Chaos Wheel) The card's label in the plain first window.
pub const WHEEL_FIRST: &str = "GET READY";
/// (Chaos Wheel) The card's label once the next mutator is drawn.
pub const WHEEL_NEXT: &str = "NEXT";

/// (online) The plate over the scoreboard portrait of a player whose link dropped (the referee's away).
pub const AWAY: &str = "Away";

/// (online) This player's own link dropped: the seconds left before the match ends without them; 0 (the time is up
/// and the match not ended yet) the line with no count.
pub fn reconnecting(seconds: u32) -> String {
    if seconds == 0 { "Reconnecting...".to_owned() } else { format!("Reconnecting... {seconds}") }
}
