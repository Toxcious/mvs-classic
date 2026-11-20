//! (the port's own) The Waves mode's menus state: the variant and stage the player picked, the
//! run the program started, and the last run's result for the result page. Its words are in `custom_text.rs`.
//!
//! The way in: Match Options' Waves tab (a card per variant), the Lab's map selector over every stage, then the
//! pre-match screen with the player alone (its fighter, skin and perks); its end makes the run's match
//! (`waves_match`). The way out: the program calls `finish` at the run's end and opens the main menu, whose home
//! screen shows the result page (`widgets/home.rs`) with Play Again.

use mvs_settings::HydraSettings;
use mvs_settings::stats::{StatsFile, WavesRecord};
use mvs_sim::modes::waves::{TARGET, WavesEnd, WavesStatus};
use mvs_sim::setup::{MatchPlayer, WavesVariant, perk_list};

use crate::bundle::{FighterInfo, RosterEntry};
use crate::menu::controller::{Cx, Handler};
use crate::menu::lobby::loadout::{local_banner, local_ring_out, picked_skin, ported_setup, with_local_loadout};
use crate::menu::lobby::{LobbyMatch, PreMatchPick};
use crate::menu::manager::{DialogId, DialogParams, Manager};
use crate::menu::screens::pre_match::setup::{PreMatchPlayer, PreMatchSetup, SoloMode};

/// The mode's state across screens (`Session::waves`).
#[derive(Debug, Default)]
pub struct WavesState {
    /// The variant the player picked; None: the mode's card was not opened since the last run.
    pub variant: Option<WavesVariant>,
    /// The stage the player picked (a map's key, as the Lab's map pick gives it).
    pub stage: Option<String>,
    /// The last run's result, for the result page; taken by the page.
    pub last: Option<WavesResult>,
    /// The pre-match screen is open for a run: its end makes the run's match (`PreMatchScreen::take_end`).
    pub choosing: bool,
    /// The fighter, skin and perks of the last run started (Play Again's).
    pub pick: Option<WavesPick>,
    /// The seed the run's cast was drawn from, taken by the program for the match (`MatchStart`'s seed), so the
    /// World's draws and the loaded cast agree.
    pub seed: Option<i32>,
}

/// The player's pick for a run, as the pre-match screen locked it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WavesPick {
    /// The roster key.
    pub character: String,
    /// The SkinData name ("" the default).
    pub skin: String,
    /// The four perk slots ("" an empty one).
    pub perks: Vec<String>,
}

/// A finished run as the result page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct WavesResult {
    pub variant: WavesVariant,
    /// The player's character key.
    pub character: String,
    pub ring_outs: u32,
    /// The count that clears the run (100 Ring-Outs), or None (Endless).
    pub target: Option<u32>,
    /// The run's clock at its end.
    pub seconds: f64,
    /// 100 Ring-Outs reached its target.
    pub cleared: bool,
    /// The run beat the fighter's best for the variant (and was saved).
    pub new_record: bool,
    /// The fighter's best for the variant after this run: ring-outs (Endless) or seconds (100 Ring-Outs), or None.
    pub best: Option<f64>,
}

/// Seconds of `ticks` of the run's clock.
fn seconds(ticks: u32) -> f64 {
    f64::from(ticks) / f64::from(mvs_sim::consts::TICK_HZ)
}

/// A record's best for `variant`: ring-outs (Endless, None at 0) or seconds (100 Ring-Outs, None when no run was
/// cleared).
pub fn best_of(r: &WavesRecord, variant: WavesVariant) -> Option<f64> {
    match variant {
        WavesVariant::Endless => (r.endless_best > 0).then(|| f64::from(r.endless_best)),
        WavesVariant::RingOuts100 => r.ring_outs_100_ticks.map(seconds),
    }
}

/// The best of every fighter for `variant` (the Waves cards' line): the best value and its character key.
pub fn best_overall(stats: &mut StatsFile, variant: WavesVariant) -> Option<(f64, String)> {
    let w = stats.waves();
    let mut best: Option<(f64, String)> = None;
    for (k, r) in &w.fighters {
        let Some(b) = best_of(r, variant) else { continue };
        let better = match (&best, variant) {
            (None, _) => true,
            (Some((x, _)), WavesVariant::Endless) => b > *x,
            (Some((x, _)), WavesVariant::RingOuts100) => b < *x,
        };
        if better {
            best = Some((b, k.clone()));
        }
    }
    best
}

impl WavesState {
    /// The end of a run: its result, the record saved to `stats` when it is a new best, and `last` set.
    /// Endless keeps the most ring-outs; 100 Ring-Outs keeps the fastest cleared run (a fall saves no time). Each
    /// variant counts its runs.
    pub fn finish(&mut self, stats: &mut StatsFile, character: &str, status: &WavesStatus) -> WavesResult {
        let cleared = status.end == Some(WavesEnd::Cleared);
        let mut w = stats.waves();
        let r = w.fighters.entry(character.to_string()).or_default();
        let new_record = match status.variant {
            WavesVariant::Endless => {
                r.endless_runs = r.endless_runs.saturating_add(1);
                let new = status.ring_outs > r.endless_best;
                if new {
                    r.endless_best = status.ring_outs;
                }
                new
            }
            WavesVariant::RingOuts100 => {
                r.ring_outs_100_runs = r.ring_outs_100_runs.saturating_add(1);
                let new = cleared && r.ring_outs_100_ticks.is_none_or(|t| status.ticks < t);
                if cleared {
                    r.ring_outs_100_cleared = r.ring_outs_100_cleared.saturating_add(1);
                }
                if new {
                    r.ring_outs_100_ticks = Some(status.ticks);
                }
                new
            }
        };
        let best = best_of(r, status.variant);
        stats.set_waves(w);
        let result = WavesResult {
            variant: status.variant,
            character: character.to_string(),
            ring_outs: status.ring_outs,
            target: status.target,
            seconds: seconds(status.ticks),
            cleared,
            new_record,
            best,
        };
        self.choosing = false;
        self.last = Some(result.clone());
        result
    }
}

/// The local player's account id on the pre-match screen (the arcade's).
pub const ME_ID: &str = crate::menu::arcade::ME_ID;

/// The pre-match screen's set-up for a run: the local player (`username`) alone on team 0 (a 1v1's style) in the
/// screen's solo layout titled with the mode and `variant` (`title`), the picked map and its level. Deviation: the
/// screen runs as a custom game's whose leader is this player (its Back to Lobby button goes to the main menu, and
/// its 45 s timer runs); the run has no lobby of its own.
pub fn pre_match_setup(username: &str, variant: WavesVariant, map: Option<String>, level: String) -> PreMatchSetup {
    PreMatchSetup {
        players: vec![PreMatchPlayer {
            account_id: ME_ID.to_string(),
            username: username.to_string(),
            team: 0,
            is_bot: false,
            ..PreMatchPlayer::default()
        }],
        me: 0,
        team_style: 0,
        leader: true,
        map,
        level,
        solo: Some(SoloMode { title: title(variant) }),
        ..PreMatchSetup::default()
    }
}

/// The mode's name with the variant's ("Waves - 100 Ring-Outs"): the pre-match screen's and the result page's title.
pub fn title(variant: WavesVariant) -> String {
    use crate::menu::custom_text as t;
    let v = match variant {
        WavesVariant::RingOuts100 => t::WAVES_100,
        WavesVariant::Endless => t::WAVES_ENDLESS,
    };
    format!("{} - {v}", t::WAVES_RESULT_TITLE)
}

/// What a run's match is made from besides the pick: the player's preferences and the menu data.
pub struct WavesSource<'a> {
    pub hydra: &'a HydraSettings,
    pub input: &'a mvs_settings::InputSettings,
    pub roster: &'a [RosterEntry],
    pub fighters: &'a indexmap::IndexMap<String, FighterInfo>,
    pub username: &'a str,
}

/// A run's match: `mvs_sim::modes::waves::setup` with the local player (the pick's character, skin and perks, this
/// machine's ring-out, banner, taunts and input preferences) and the enemies' `cast`, on `map`. A character the
/// port cannot draw plays the default one.
pub fn waves_match(
    variant: WavesVariant,
    pick: &WavesPick,
    map: Option<String>,
    cast: Vec<String>,
    src: &WavesSource<'_>,
) -> LobbyMatch {
    let skin = Some(picked_skin(src.fighters.get(&pick.character), &pick.skin)).filter(|s| !s.is_empty());
    let player = MatchPlayer {
        character: Some(pick.character.clone()).filter(|c| !c.is_empty()),
        skin,
        perks: perk_list(&pick.perks),
        ..MatchPlayer::default()
    };
    let mut setup = mvs_sim::modes::waves::setup(variant, player, cast);
    let drawable = |c: &str| src.fighters.contains_key(c);
    ported_setup(&mut setup, &drawable);
    let (ring_out, banner) = (local_ring_out(src.hydra), local_banner(src.hydra));
    with_local_loadout(
        &mut setup,
        src.hydra,
        src.roster,
        &mut |_, _| ring_out.clone(),
        &mut |_, _| banner.clone(),
        &mut |_, _| Some(crate::menu::prefs::sim_input_prefs(src.input, 0)),
    );
    LobbyMatch { setup, me: 0, usernames: vec![src.username.to_string()], map, humans: None, lobby: None }
}

/// A run of the session's variant, stage and pick: a new seed (kept for the program, `WavesState::seed`), the
/// cast drawn from it over the fighters the port can draw, and its match. None: no variant or pick yet.
pub fn start_run(cx: &mut Cx<'_>) -> Option<LobbyMatch> {
    let w = &cx.view.session.waves;
    let (variant, pick, map) = (w.variant?, w.pick.clone()?, w.stage.clone());
    // (the clock's time mixed in: the menus' random numbers start from one seed in every session)
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    #[allow(clippy::cast_possible_truncation)] // (the clock's low bits)
    let seed = new_seed(cx.view.manager.random(), nanos as u64);
    cx.view.session.waves.seed = Some(seed);
    let cast = mvs_sim::modes::waves::draw_cast(seed, &cast_roster(cx.view.roster()));
    let src = WavesSource {
        hydra: &cx.prefs.hydra,
        input: &cx.prefs.input,
        roster: cx.view.roster(),
        fighters: cx.view.fighters(),
        username: &cx.prefs.profile.name,
    };
    Some(waves_match(variant, &pick, map, cast, &src))
}

/// The pre-match screen's end with the picks (BPI_CleanUpAndStartMatch): the local pick kept for Play Again, then
/// the run's match (`start_run`).
pub fn start_from_picks(cx: &mut Cx<'_>, picks: &[PreMatchPick]) -> Option<LobbyMatch> {
    let mine = picks.iter().find(|p| p.account_id == ME_ID)?;
    cx.view.session.waves.pick = Some(WavesPick {
        character: mine.character.clone(),
        skin: mine.skin.clone(),
        perks: mine.perks.iter().map(|p| p.clone().unwrap_or_default()).collect(),
    });
    start_run(cx)
}

/// The level a run's match opens from the main menu (Play Again): the map's, else a name of the mode's own. The
/// program starts the carried match for any level but its own (the Lab's, the main menu's, the tutorial's).
pub const WAVES_LEVEL: &str = "Waves";

/// The result page's lines: the variant's title, then the body (the count or the time, NEW RECORD, the fighter's
/// best).
pub fn result_text(r: &WavesResult, fighter: &str) -> (String, String) {
    use crate::menu::custom_text as t;
    let title = title(r.variant);
    let mut lines = Vec::new();
    match r.variant {
        WavesVariant::RingOuts100 if r.cleared => {
            lines.push(format!("{} {}", t::WAVES_CLEARED, t::waves_time(r.seconds)));
        }
        WavesVariant::RingOuts100 => {
            let target = r.target.unwrap_or(TARGET);
            lines.push(format!("{} {} / {target}", t::WAVES_FELL, r.ring_outs));
        }
        WavesVariant::Endless => lines.push(t::waves_ring_outs(r.ring_outs)),
    }
    if r.new_record {
        lines.push(t::WAVES_NEW_RECORD.to_string());
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // (a whole count)
    let best = match (r.variant, r.best) {
        (WavesVariant::RingOuts100, Some(s)) => t::waves_time(s),
        (WavesVariant::Endless, Some(n)) => t::waves_ring_outs(n as u32),
        (WavesVariant::RingOuts100, None) => t::WAVES_NO_BEST.to_string(),
        (WavesVariant::Endless, None) => t::WAVES_NO_RING_OUTS.to_string(),
    };
    // (a blank line between the run and the record)
    lines.push(String::new());
    lines.push(t::waves_best(fighter, &best));
    (title, lines.join("\n"))
}

/// The result page (the home screen's, once the main menu is ready after a run): the session's last result, taken,
/// as a dialog of the menus' own with Play Again (`again`) and Back (`back`). None: no run ended since.
pub fn show_result(cx: &mut Cx<'_>, again: Handler, back: Handler) -> Option<DialogId> {
    let r = cx.view.session.waves.last.take()?;
    let fighter = cx
        .view
        .roster()
        .iter()
        .find(|e| e.fighter.as_deref() == Some(r.character.as_str()))
        .map_or_else(|| r.character.clone(), |e| e.display_name.clone());
    let (title, body) = result_text(&r, &fighter);
    let params = DialogParams {
        title,
        body,
        button_left_label: crate::menu::custom_text::WAVES_PLAY_AGAIN.to_string(),
        button_right_label: crate::menu::custom_text::WAVES_BACK.to_string(),
        ..DialogParams::default()
    };
    let id = Manager::show_dialog(cx, params);
    if let Some(d) = cx.view.dialog(id) {
        d.button_left_clicked().add(again);
        d.button_right_clicked().add(back);
    }
    Some(id)
}

/// Play Again: a new run of the last one's variant, stage and pick, straight to its match (a new seed, a new cast).
/// False: nothing to play again.
pub fn play_again(cx: &mut Cx<'_>) -> bool {
    let Some(m) = start_run(cx) else { return false };
    let level = m
        .map
        .as_deref()
        .and_then(|n| cx.view.map(n))
        .map(|i| i.data.level)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| WAVES_LEVEL.to_string());
    crate::menu::lobby::with_carry(|c| c.start = Some(Box::new(m)));
    Manager::open_level_now(cx, &level, "");
    true
}

/// A new run's seed, in 1..2^31: the clock's `nanos` mixed with the menus' random number in [0, 1).
pub fn new_seed(random: f64, nanos: u64) -> i32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // (in [0, 2^31) by the clamp)
    let r = (random * 2_147_483_648.0).clamp(0.0, 2_147_483_647.0) as u64;
    #[allow(clippy::cast_possible_truncation)] // (31 bits)
    let s = ((nanos ^ r) & 0x7fff_ffff) as i32;
    s.max(1)
}

/// The cast's roster: the keys of the fighters the port can draw, in roster order.
pub fn cast_roster(roster: &[RosterEntry]) -> Vec<String> {
    roster.iter().filter_map(|r| r.fighter.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_sim::setup::MatchMode;

    fn status(variant: WavesVariant, ring_outs: u32, ticks: u32, end: WavesEnd) -> WavesStatus {
        let target = (variant == WavesVariant::RingOuts100).then_some(100);
        WavesStatus { variant, ring_outs, target, ticks, alive: 0, end: Some(end) }
    }

    #[test]
    fn endless_keeps_the_most_ring_outs() {
        let (mut st, mut stats) = (WavesState::default(), StatsFile::memory());
        let r = st.finish(&mut stats, "shaggy", &status(WavesVariant::Endless, 12, 3000, WavesEnd::PlayerOut));
        assert!(r.new_record && r.best == Some(12.0) && !r.cleared && r.ring_outs == 12);
        assert_eq!(st.last.as_ref(), Some(&r));
        let r = st.finish(&mut stats, "shaggy", &status(WavesVariant::Endless, 7, 3000, WavesEnd::PlayerOut));
        assert!(!r.new_record && r.best == Some(12.0));
        let rec = stats.waves().fighters["shaggy"];
        assert_eq!((rec.endless_best, rec.endless_runs, rec.ring_outs_100_runs), (12, 2, 0));
        // another fighter's records are its own; a run with no ring-out is no record
        let r = st.finish(&mut stats, "finn", &status(WavesVariant::Endless, 0, 60, WavesEnd::PlayerOut));
        assert!(!r.new_record && r.best.is_none());
        assert_eq!(best_overall(&mut stats, WavesVariant::Endless), Some((12.0, "shaggy".to_string())));
    }

    #[test]
    fn a_fall_in_100_saves_no_time() {
        let (mut st, mut stats) = (WavesState::default(), StatsFile::memory());
        let r = st.finish(&mut stats, "shaggy", &status(WavesVariant::RingOuts100, 64, 9000, WavesEnd::PlayerOut));
        assert!(!r.new_record && !r.cleared && r.best.is_none() && r.ring_outs == 64);
        let rec = stats.waves().fighters["shaggy"];
        assert_eq!((rec.ring_outs_100_ticks, rec.ring_outs_100_runs, rec.ring_outs_100_cleared), (None, 1, 0));
        // a cleared run keeps its time, a slower one does not replace it, a faster one does
        let r = st.finish(&mut stats, "shaggy", &status(WavesVariant::RingOuts100, 100, 36_000, WavesEnd::Cleared));
        assert!(r.new_record && r.cleared && r.seconds == 600.0 && r.best == Some(600.0));
        let r = st.finish(&mut stats, "shaggy", &status(WavesVariant::RingOuts100, 100, 40_000, WavesEnd::Cleared));
        assert!(!r.new_record && r.best == Some(600.0));
        let r = st.finish(&mut stats, "shaggy", &status(WavesVariant::RingOuts100, 100, 30_000, WavesEnd::Cleared));
        assert!(r.new_record && r.best == Some(500.0));
        let rec = stats.waves().fighters["shaggy"];
        assert_eq!((rec.ring_outs_100_ticks, rec.ring_outs_100_runs, rec.ring_outs_100_cleared), (Some(30_000), 4, 3));
        assert_eq!(rec.endless_runs, 0);
        assert_eq!(best_overall(&mut stats, WavesVariant::RingOuts100), Some((500.0, "shaggy".to_string())));
    }

    #[test]
    fn the_start_makes_the_runs_match() {
        let (hydra, input) = (HydraSettings::default(), mvs_settings::InputSettings::default());
        let mut fighters = indexmap::IndexMap::new();
        fighters.insert("shaggy".to_string(), FighterInfo::default());
        let src = WavesSource { hydra: &hydra, input: &input, roster: &[], fighters: &fighters, username: "me" };
        let pick = WavesPick {
            character: "shaggy".into(),
            skin: String::new(),
            perks: vec!["perk_a".into(), String::new(), "perk_b".into(), String::new()],
        };
        let cast = vec!["finn".to_string(), "jake".to_string()];
        let m = waves_match(WavesVariant::Endless, &pick, Some("M011".into()), cast.clone(), &src);
        assert_eq!((m.me, m.usernames.as_slice(), m.map.as_deref()), (0, &["me".to_string()][..], Some("M011")));
        assert!(m.humans.is_none() && m.lobby.is_none());
        let s = &m.setup;
        assert_eq!(s.players.len(), 1);
        let p = &s.players[0];
        assert!(!p.bot && p.team == Some(0) && p.character.as_deref() == Some("shaggy"));
        assert_eq!(p.perks, ["perk_a", "perk_b"]);
        assert_eq!(s.mode, Some(MatchMode::Waves { variant: WavesVariant::Endless, cast }));
        assert!(s.allow_hazards && s.rules.is_none());
        // a fighter the port cannot draw plays the default one
        let other = WavesPick { character: "velma".into(), ..pick };
        let m = waves_match(WavesVariant::RingOuts100, &other, None, Vec::new(), &src);
        assert!(m.setup.players[0].character.is_none());
        assert!(matches!(m.setup.mode, Some(MatchMode::Waves { variant: WavesVariant::RingOuts100, .. })));
    }

    #[test]
    fn result_lines_per_variant() {
        use crate::menu::custom_text as t;
        let r = WavesResult {
            variant: WavesVariant::Endless,
            character: "shaggy".into(),
            ring_outs: 0,
            target: None,
            seconds: 12.0,
            cleared: false,
            new_record: false,
            best: None,
        };
        assert!(result_text(&r, "Shaggy").1.ends_with(t::WAVES_NO_RING_OUTS));
        let fell = WavesResult { variant: WavesVariant::RingOuts100, ring_outs: 7, target: Some(100), ..r.clone() };
        let body = result_text(&fell, "Shaggy").1;
        assert!(body.starts_with("Rung out at 7 / 100\n") && body.ends_with(t::WAVES_NO_BEST));
        // (the time floors as the match's clock does)
        let won = WavesResult { cleared: true, seconds: 61.999, best: Some(61.999), ..fell };
        assert!(result_text(&won, "Shaggy").1.contains("1:01.99"));
    }

    #[test]
    fn the_pre_match_is_solo_with_the_modes_title() {
        let s = pre_match_setup("me", WavesVariant::Endless, Some("M011".into()), "Level".into());
        assert_eq!(s.players.len(), 1);
        assert_eq!(s.solo.as_ref().map(|m| m.title.as_str()), Some("Waves - Endless"));
        assert_eq!(title(WavesVariant::RingOuts100), "Waves - 100 Ring-Outs");
    }

    #[test]
    fn seeds_are_positive() {
        assert_eq!(new_seed(0.0, 0), 1);
        assert_eq!(new_seed(0.999_999_999_9, 0), 2_147_483_647);
        assert!(new_seed(0.5, 0) > 1);
        // (the clock moves the seed of the same random number)
        assert_ne!(new_seed(0.25, 1_000), new_seed(0.25, 2_000));
        assert_eq!(new_seed(0.0, 1 << 40), 1);
    }
}
