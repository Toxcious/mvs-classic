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

/// (online) The plate over the scoreboard portrait of a player whose link dropped (the referee's away).
pub const AWAY: &str = "Away";

/// (online) This player's own link dropped: the seconds left before the match ends without them; 0 (the time is up
/// and the match not ended yet) the line with no count.
pub fn reconnecting(seconds: u32) -> String {
    if seconds == 0 { "Reconnecting...".to_owned() } else { format!("Reconnecting... {seconds}") }
}
