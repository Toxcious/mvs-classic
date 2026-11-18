//! Buff_AirSuperiority: a shared class's effects, made for every match after
//! the characters'. The buff is mvs-sim chars/jake/perks.rs's (Buff_Jake_SafetyNet gives it to Jake's team mates).
//!
//! ReceiveBeginPlay @96, not on a dedicated server (@297): VFX = SpawnEmitterAttached(PS_Fizzy_Targeting_Up, the owner's
//! K2_GetRootComponent, no socket, (0, 0, -100), rotation 0, scale 1, KeepRelativeOffset, auto destroy) @373, no Team;
//! ReceiveEndPlay @928 -> @48: a valid VFX K2_DestroyComponent @10 (gone at once).
//! RefreshAirMoves @493 (the looping timer: the sim's projectileDeath event with this class): PS_BigJump the same way
//! @827, kept by nothing (it ends by itself) and riding the owner's root.
//! The sim has no event for the buff's BeginPlay: the buff is seen here on the fighter (as electric.rs's gauntlets). A
//! CanRefresh give of a live buff keeps it (no new BeginPlay), as here.
//! Unverified: the buff's BeginPlay seen as the buff's arrival on the fighter (the sim has no event for it).

use std::collections::BTreeSet;

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::chars::jake::perks::AIR_SUPERIORITY;
use mvs_sim::events::Event;
use mvs_vfx::SystemId;
use mvs_vfx::math::V3;

use crate::fx::chars::{CharFx, CharFxHooks, ProjRule};
use crate::fx::{Follow, Fx};

const TARGETING: &str = "PS_Fizzy_Targeting_Up";
const BIG_JUMP: &str = "PS_BigJump";
const UNDER: V3 = [0.0, 0.0, -100.0];

/// What its effects keep between steps: by the fighter's id while it has the buff, the targeting system (None: not
/// loaded); the systems warned of.
#[derive(Debug, Default)]
pub(crate) struct AirSuperiority {
    shown: IndexMap<i32, Option<SystemId>>,
    warned: BTreeSet<String>,
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] =
    &[(AIR_SUPERIORITY, ProjRule { own_emitter: Some(big_jump), ..ProjRule::NONE })];

fn me(c: &mut CharFx) -> &mut AirSuperiority {
    match c {
        CharFx::AirSuperiority(s) => s,
        _ => unreachable!("an AirSuperiority rule entered by another module"),
    }
}

impl AirSuperiority {
    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: air superiority: {ps} is not loaded");
        }
        false
    }

    fn clear(&mut self, fx: &mut Fx) {
        for h in self.shown.drain(..).filter_map(|(_, h)| h) {
            fx.player.kill(h);
        }
    }
}

impl CharFxHooks for AirSuperiority {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        let cls = w.names.get(AIR_SUPERIORITY);
        for f in w.all_ids() {
            let fi = w.fighter(f);
            let id = fi.id;
            let on = cls.is_some_and(|c| fi.buffs.buffs.iter().any(|b| b.frames > 0.0 && b.cls == c));
            if on == self.shown.contains_key(&id) {
                continue;
            }
            if !on {
                if let Some(h) = self.shown.shift_remove(&id).flatten() {
                    fx.player.kill(h);
                }
                continue;
            }
            let h = if self.has(fx, TARGETING) {
                fx.spawn_follow(w, TARGETING, Follow::RootScaled { id, loc: UNDER }, 1.0, &[])
            } else {
                None
            };
            self.shown.insert(id, h);
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// RefreshAirMoves' PS_BigJump on the owner's root.
fn big_jump(c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx, .. } = *e else { return false };
    if !vfx.is_some_and(|v| w.names.str(v) == BIG_JUMP) {
        return false;
    }
    if me(c).has(fx, BIG_JUMP) {
        fx.spawn_follow(w, BIG_JUMP, Follow::RootScaled { id: fighter, loc: UNDER }, 1.0, &[]);
    }
    true
}
