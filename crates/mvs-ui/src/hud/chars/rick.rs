//! Rick's portal input labels: made through the HUD's InputLabels after each
//! World step; nothing drawn of its own.

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;

use crate::element::ElementList;
use crate::hud::Texts;
use crate::hud::common::BrokerPlayer;
use crate::hud::input_label::{InputLabels, LabelParams, LabelSources};
use crate::slate::Tree;
use crate::text::Fonts;

/// The key of a portal's label: a key per label made.
fn portal_label(n: i64) -> i64 {
    -3001 - n
}

/// PortalInputMulticast(Base_Fighter, UI_On) @13586: a PopInputLabel (Action 2, the Label set @13879, text 14, icon 32,
/// PlayerIndex 0; Location (0, 100, 350), attached to the fighter) for each pawn in a portal's `label_for` that is
/// locally controlled and no bot's (UI_Functions PopInputLabel's own test); off Exits the first label attached to the
/// pawn (@14549-15304), or the portal's end clears them (@17449). The key is (portal, fighter).
// Unverified: locally controlled is read as BrokerPlayer.is_local.
#[derive(Debug)]
pub(crate) struct PortalLabels {
    players: Vec<BrokerPlayer>,
    /// (portal seq, fighter slot) -> the label's count.
    made: IndexMap<(i32, usize), i64>,
    seq: i64,
    /// The label's text (Texts.teleport).
    label: String,
}

impl PortalLabels {
    pub(crate) fn new(players: &[BrokerPlayer], texts: &Texts) -> PortalLabels {
        PortalLabels { players: players.to_vec(), made: IndexMap::new(), seq: 0, label: texts.teleport.clone() }
    }

    pub(crate) fn observe(&mut self, w: &World, _now: f64, real: f64, inputs: &mut InputLabels, order: &mut u64) {
        let mut seen: Vec<(i32, usize)> = Vec::new();
        for f in w.all_ids() {
            for &a in &w.fighter(f).items.spawned_actors {
                let ActorRef::Actor(id) = a else { continue };
                let Some(act) = w.actors.actors.get(id) else { continue };
                let WorldActorState::PortalActor(p) = &act.state else { continue };
                if act.dead {
                    continue;
                }
                for &i in &p.label_for {
                    let Ok(i) = usize::try_from(i) else { continue };
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
                        action: 2,
                        location: [0.0, 100.0, 350.0],
                        label: self.label.clone(),
                        text_size: 14.0,
                        attach: Some(i),
                        player: 0,
                        icon_size: 32.0,
                        custom_icon: None,
                        modifier: String::new(),
                    };
                    inputs.show_for(portal_label(n), p, real, order, &LabelSources::default());
                }
            }
        }
        self.made.retain(|k, n| {
            let keep = seen.contains(k);
            if !keep {
                inputs.hide_for(portal_label(*n));
            }
            keep
        });
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
