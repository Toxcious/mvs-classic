//! Harley's Jack-in-the-box texts: made through the HUD's InputLabels and
//! PopTexts after each World step; nothing drawn of its own.

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;

use crate::element::ElementList;
use crate::hud::common::BrokerPlayer;
use crate::hud::input_label::{InputLabels, LabelParams, LabelSources};
use crate::hud::pop::{PopParams, PopTexts};
use crate::slate::Tree;
use crate::text::Fonts;

/// The key of a box's label: a key per label made.
fn box_label(n: i64) -> i64 {
    -2001 - n
}

/// HarleyJackInTheBox's two texts. JumpInputPopLabel(fighter) @6889-8010, when a teammate begins to overlap the armed
/// box (@3177-3483): for a locally controlled fighter without a label of this box, UI_Functions PopInputLabel with
/// Action 3 (Jump), an empty Label, PlayerIndex = the fighter's, text 16, icon 60 x 60 @7703; Lifetime 0, Location
/// (0, -20, 300), attached to the fighter; made only for a pawn that is locally controlled and no bot's. CleanUp
/// @8181-8532 (the end of the overlap; every key at ReceiveEndPlay @8026): gone at once. The sim's `allies` while the
/// box is active are the map's keys.
/// JumpCountResetPopText(fighter) @6253-6888, from AfterJumpTriggered @9009: for a locally controlled fighter
/// UI_Functions PopText "Jumps Reset!", lifetime 0.5, colour (1, 0.895364, 0.016631, 1), location (0, 0, 250),
/// attached, called directly (not queued). The sim keeps the fighter in `reset_for`, once a box.
// Unverified: a bot's pawn is locally controlled offline, so the text shows over a bot that used the box, as the
// graph has it; the label does not (PopInputLabel's own bot test).
#[derive(Debug)]
pub(crate) struct BoxLabels {
    players: Vec<BrokerPlayer>,
    /// (box seq, fighter slot) -> the label's count.
    made: IndexMap<(i32, usize), i64>,
    seq: i64,
    /// The boxes whose reset text was sent.
    resets: Vec<i32>,
}

impl BoxLabels {
    pub(crate) fn new(players: &[BrokerPlayer]) -> BoxLabels {
        BoxLabels { players: players.to_vec(), made: IndexMap::new(), seq: 0, resets: Vec::new() }
    }

    #[allow(clippy::too_many_arguments, reason = "the call surface of CharHud::observe")]
    pub(crate) fn observe(
        &mut self,
        w: &World,
        now: f64,
        real: f64,
        inputs: &mut InputLabels,
        pops: &mut PopTexts,
        jumps_reset: &str,
        order: &mut u64,
    ) {
        let mut seen: Vec<(i32, usize)> = Vec::new();
        let mut boxes: Vec<i32> = Vec::new();
        for f in w.all_ids() {
            for &a in &w.fighter(f).items.spawned_actors {
                let ActorRef::Actor(id) = a else { continue };
                let Some(act) = w.actors.actors.get(id) else { continue };
                let WorldActorState::JackBox(j) = &act.state else { continue };
                boxes.push(act.seq);
                if let Ok(r) = usize::try_from(j.reset_for)
                    && !self.resets.contains(&act.seq)
                {
                    self.resets.push(act.seq);
                    if self.players.get(r).is_some_and(|b| b.is_local) {
                        let p =
                            PopParams::attached(r, jumps_reset, 0.5, [1.0, 0.895364, 0.016631, 1.0], [0.0, 0.0, 250.0]);
                        pops.add(p, now, order);
                    }
                }
                if act.dead || !j.active {
                    continue;
                }
                for &g in &j.allies {
                    let Ok(i) = usize::try_from(w.fighter(g).id) else { continue };
                    if !self.players.get(i).is_some_and(|b| b.is_local && !b.is_bot) {
                        continue;
                    }
                    let key = (act.seq, i);
                    seen.push(key);
                    if self.made.contains_key(&key) {
                        continue;
                    }
                    let n = self.seq;
                    self.seq += 1;
                    self.made.insert(key, n);
                    let p = LabelParams {
                        action: 3,
                        location: [0.0, -20.0, 300.0],
                        label: String::new(),
                        text_size: 16.0,
                        attach: Some(i),
                        player: i,
                        icon_size: 60.0,
                        custom_icon: None,
                        modifier: String::new(),
                    };
                    inputs.show_for(box_label(n), p, real, order, &LabelSources::default());
                }
            }
        }
        self.made.retain(|k, n| {
            let keep = seen.contains(k);
            if !keep {
                inputs.hide_for(box_label(*n));
            }
            keep
        });
        self.resets.retain(|id| boxes.contains(id));
    }

    pub(crate) fn draw_world(
        &mut self,
        _fonts: &mut Fonts,
        _out: &mut ElementList,
        _dpi: f64,
        _project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        Vec::new()
    }

    pub(crate) fn draw_screen(
        &mut self,
        _fonts: &mut Fonts,
        _out: &mut ElementList,
        _dpi: f64,
        _real: f64,
        _project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        Vec::new()
    }

    pub(crate) fn roots(&self) -> Vec<(&'static str, &Tree)> {
        Vec::new()
    }
}
