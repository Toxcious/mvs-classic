//! The Court's pickup collection text (PickupPowerup_<type>.SpaceJamPickupVFXMulticast(collector) @854-1467 ->
//! UI_Functions PopText @1359): JUMP / POWER / SPEED, lifetime 1.1,
//! text colour the kind's, location 0, space 3, scale 1, centred, no delay, icons or audio, attached to the collector;
//! called directly, not queued per pawn, on every peer (a reliable multicast). Raised at the sim's `taken` stamp, once
//! per pickup; one seen late (a rollback's replay) is only marked. The pickups' particles are the match's effects
//! (fx/shared/m010_pickups.rs).

use mvs_sim::stages::the_court::pickups::CourtPowerup;
use mvs_sim::world::stage::StageOwn;

use crate::scenery::host::{PopSpec, SceneryHost};

/// A stamp seen this many ticks late or more raises nothing.
const LATE: i32 = 6;
const LIFETIME: f64 = 1.1;

/// A pickup class's text and its colour.
const KINDS: [(&str, &str, [f64; 4]); 3] = [
    ("PickupPowerup_Jump_C", "JUMP", [0.0, 1.0, 0.0, 1.0]),
    ("PickupPowerup_Power_C", "POWER", [1.0, 0.25, 0.0, 1.0]),
    ("PickupPowerup_Speed_C", "SPEED", [1.0, 1.0, 1.0, 1.0]),
];

#[derive(Debug, Default)]
pub(super) struct Pickups {
    taken_id: Option<i32>,
}

impl Pickups {
    pub(super) fn begin(&mut self) {
        self.taken_id = None;
    }

    pub(super) fn tick(&mut self, host: &mut SceneryHost) {
        let Some(w) = host.world() else { return };
        let Some(run) = &w.stage_run else { return };
        let StageOwn::Court(c) = &run.s else { return };
        let p: &CourtPowerup = &c.script.powerup;
        let Some(t) = &p.taken else { return };
        if self.taken_id == Some(t.id) {
            return;
        }
        self.taken_id = Some(t.id);
        let cls = w.names.str(t.cls);
        let Some(&(_, text, text_color)) = KINDS.iter().find(|k| k.0 == cls) else { return };
        if w.by_id(t.fighter).is_none() || w.flow.tick - t.tick >= LATE {
            return;
        }
        let pop = PopSpec {
            text: text.to_owned(),
            lifetime: LIFETIME,
            text_color,
            location: [0.0; 3],
            scale: 1.0,
            justification: 1,
            delay: 0.0,
            icon_right: None,
            fighter: usize::try_from(t.fighter).ok(),
            at: None,
        };
        host.pop(pop);
    }
}
