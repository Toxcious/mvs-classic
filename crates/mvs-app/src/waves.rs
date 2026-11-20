//! (the port's own) A Waves run's program side: its match from the command line, its HUD set-up and counter, and its
//! end: a short beat once the sim says the run is over, the record saved, back to the main menu, whose home opens
//! the result page.

use mvs_sim::World;
use mvs_sim::modes::waves::{self, ENEMY_SLOT, WavesStatus};
use mvs_sim::setup::{MatchMode, MatchPlayer, WavesVariant};
use mvs_ui::hud::{HudPlayer, HudSetup, MODE_WAVES, WavesHud};

use crate::app::{MatchKind, MatchStart};

/// How long a run that ended goes on before the match ends (ticks, 2 s): the last ring-out seen before the fade.
/// On the sim's clock, so the pause menu holds it.
pub const END_BEAT: i32 = 120;

/// The tick rate of the run's clock (`WavesStatus::ticks`).
const TICKS_PER_SECOND: f64 = 60.0;

/// `--waves`'s value as a variant: `endless` or `100`.
pub fn variant_of(v: &str) -> Option<WavesVariant> {
    match v {
        "endless" => Some(WavesVariant::Endless),
        "100" => Some(WavesVariant::RingOuts100),
        _ => None,
    }
}

/// A Waves run as the command line starts it: the local player on `character` (None: the default one) with the
/// local loadout, ring-out and input preferences an offline match's player gets, on `stage`, its cast drawn from
/// `seed` over the whole roster. `bot`: (dev) a match bot of that level in the player's seat, which the sim drives
/// (a run that lasts, for the frame-cost bench).
pub fn waves_match(
    variant: WavesVariant,
    character: Option<&str>,
    bot: Option<u8>,
    settings: &mvs_settings::Settings,
    roster: &[mvs_ui::bundle::RosterEntry],
    seed: i32,
    stage: &str,
) -> MatchStart {
    let keys: Vec<String> = mvs_protocol::roster::CHARACTERS.iter().map(|&k| k.to_owned()).collect();
    let cast = waves::draw_cast(seed, &keys);
    let player = MatchPlayer { character: character.map(str::to_owned), ..MatchPlayer::default() };
    let mut setup = waves::setup(variant, player, cast);
    if let (Some(level), Some(p)) = (bot, setup.players.first_mut()) {
        p.bot = true;
        p.difficulty = Some(mvs_sim::Real::from(level));
    }
    let own_ring_out = mvs_ui::menu::lobby::loadout::local_ring_out(&settings.hydra);
    mvs_ui::menu::lobby::loadout::with_local_loadout(
        &mut setup,
        &settings.hydra,
        roster,
        &mut |_, _| own_ring_out.clone(),
        &mut |_, _| String::new(),
        &mut |_, _| Some(mvs_ui::menu::prefs::sim_input_prefs(&settings.input, 0)),
    );
    let mut start = MatchStart::new(setup, MatchKind::Waves { variant }, seed);
    stage.clone_into(&mut start.stage);
    start.usernames = vec![settings.profile.name.clone()];
    start
}

/// The HUD set-up of a Waves match: context `MODE_WAVES`, the player on slot 0 and one bot player for
/// `ENEMY_SLOT`, the slot every enemy is bound to (the cast's first character, team 1, no name), no score.
pub fn hud_setup(world: &World, me: usize, usernames: &[String], ui: &mvs_settings::UiSettings) -> HudSetup {
    let setup = &world.setup;
    let player = setup.players.first();
    let mut players = vec![HudPlayer {
        team: 0,
        bot: false,
        username: usernames.first().cloned().unwrap_or_default(),
        character: player.and_then(|p| p.character.clone()),
    }];
    let enemy = setup.mode.as_ref().and_then(|m| m.cast().first().cloned());
    debug_assert_eq!(usize::try_from(ENEMY_SLOT).ok(), Some(players.len()));
    players.push(HudPlayer { team: 1, bot: true, username: String::new(), character: enemy });
    HudSetup {
        mode: MODE_WAVES,
        me,
        players,
        max_score: -1,
        stocks: false,
        local_play: false,
        online: false,
        arcade: None,
        ui: ui.clone(),
    }
}

/// Whether the World's set-up is a Waves run's.
pub fn is_waves(world: &World) -> bool {
    matches!(world.setup.mode, Some(MatchMode::Waves { .. }))
}

/// The HUD's counter of a run's status.
pub fn hud_of(s: &WavesStatus) -> WavesHud {
    WavesHud { ring_outs: s.ring_outs, target: s.target, seconds: f64::from(s.ticks) / TICKS_PER_SECOND }
}

/// The characters the HUD warms its pictures for: the match's fighters, and in a Waves run its cast (the enemies
/// spawn later, their damage readouts on `ENEMY_SLOT`).
pub fn hud_chars(world: &World) -> Vec<&str> {
    let mut chars: Vec<&str> = world.players.iter().map(|&f| world.names.str(world.fighter(f).ch)).collect();
    if is_waves(world) {
        // (a mode World has no fighters at its start: the player's character is set-up player 0's)
        let player = world.setup.players.first().and_then(|p| p.character.as_deref());
        let player = player.unwrap_or(&world.data.default_character);
        let cast = world.setup.mode.iter().flat_map(MatchMode::cast).map(String::as_str);
        for k in std::iter::once(player).chain(cast) {
            if !chars.contains(&k) {
                chars.push(k);
            }
        }
    }
    chars
}

/// A run's end once its beat is over: its result for the result page, its record saved to the stats file when it
/// is the fighter's new best (`WavesState::finish`).
pub fn finish(session: &mut mvs_ui::menu::session::Session, world: &World, status: &WavesStatus) {
    let character = world.setup.players.first().and_then(|p| p.character.as_deref());
    let character = character.unwrap_or(&world.data.default_character);
    let mvs_ui::menu::session::Session { waves, stats, .. } = session;
    let r = waves.finish(stats, character, status);
    eprintln!(
        "mvs: waves {:?} {}: {} ring-outs in {:.1} s{}",
        r.variant,
        r.character,
        r.ring_outs,
        r.seconds,
        if r.new_record { ", a new record" } else { "" }
    );
}

/// What the run's end asks of the match loop this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EndStep {
    /// Nothing: the run goes on, or its end's beat does.
    None,
    /// The fade over the beat's last moment begins.
    Fade,
    /// The beat is over: the run's record saved, the match ends.
    Finish(WavesStatus),
}

/// The run's end, followed frame by frame on the World's tick.
#[derive(Debug, Default)]
pub struct EndBeat {
    /// The tick the sim was first seen to say the run was over.
    ended_at: Option<i32>,
    faded: bool,
    finished: bool,
}

impl EndBeat {
    /// One frame: `status` as the World stands after the frame's ticks, `tick` the World's, `fade` the fade's length
    /// (ticks).
    pub fn frame(&mut self, status: Option<WavesStatus>, tick: i32, fade: i32) -> EndStep {
        let Some(s) = status.filter(|s| s.end.is_some()) else { return EndStep::None };
        if self.finished {
            return EndStep::None;
        }
        let since = tick.saturating_sub(*self.ended_at.get_or_insert(tick));
        if since >= END_BEAT {
            self.finished = true;
            return EndStep::Finish(s);
        }
        if !self.faded && since >= END_BEAT - fade {
            self.faded = true;
            return EndStep::Fade;
        }
        EndStep::None
    }

    /// The match is left before the beat is over: the run's status to finish with when the sim has settled the run
    /// and it was not finished yet (once), else None.
    pub fn settle(&mut self, status: Option<WavesStatus>) -> Option<WavesStatus> {
        let s = status.filter(|s| s.end.is_some() && !self.finished)?;
        self.finished = true;
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use mvs_sim::modes::waves::WavesEnd;

    use super::*;

    fn status(end: Option<WavesEnd>) -> WavesStatus {
        WavesStatus { variant: WavesVariant::Endless, ring_outs: 12, target: None, ticks: 600, alive: 3, end }
    }

    #[test]
    fn the_variant_switch_takes_endless_and_100() {
        assert_eq!(variant_of("endless"), Some(WavesVariant::Endless));
        assert_eq!(variant_of("100"), Some(WavesVariant::RingOuts100));
        assert_eq!(variant_of("50"), None);
    }

    #[test]
    fn the_counter_counts_seconds_from_ticks() {
        let h = hud_of(&WavesStatus { target: Some(100), ..status(None) });
        assert_eq!(h, WavesHud { ring_outs: 12, target: Some(100), seconds: 10.0 });
    }

    #[test]
    fn the_end_fades_then_finishes_once_after_its_beat() {
        let mut e = EndBeat::default();
        assert_eq!(e.frame(None, 0, 18), EndStep::None);
        assert_eq!(e.frame(Some(status(None)), 60, 18), EndStep::None);
        let over = status(Some(WavesEnd::PlayerOut));
        assert_eq!(e.frame(Some(over), 300, 18), EndStep::None);
        // (a paused frame: the tick holds, so does the beat)
        assert_eq!(e.frame(Some(over), 300, 18), EndStep::None);
        assert_eq!(e.frame(Some(over), 401, 18), EndStep::None);
        assert_eq!(e.frame(Some(over), 402, 18), EndStep::Fade);
        assert_eq!(e.frame(Some(over), 410, 18), EndStep::None);
        assert_eq!(e.frame(Some(over), 420, 18), EndStep::Finish(over));
        assert_eq!(e.frame(Some(over), 421, 18), EndStep::None);
        assert_eq!(e.settle(Some(over)), None);
    }

    #[test]
    fn a_run_left_in_its_beat_settles_once_and_one_still_going_not_at_all() {
        let mut e = EndBeat::default();
        assert_eq!(e.settle(Some(status(None))), None);
        let over = status(Some(WavesEnd::Cleared));
        assert_eq!(e.frame(Some(over), 300, 18), EndStep::None);
        assert_eq!(e.settle(Some(over)), Some(over));
        assert_eq!(e.settle(Some(over)), None);
        assert_eq!(e.frame(Some(over), 500, 18), EndStep::None);
    }
}
