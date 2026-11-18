//! Buff_ProjectileBlock's shield (School Me Once, the sim's Finn shop): a
//! shared class's effects, made for every match after the characters'.
//!
//! Its graph:
//! - ReceiveBeginPlay, not a dedicated server @1035: VFX = SpawnEmitterAttached(PS_FinnShield, the owner's active mesh,
//!   None, GetActiveMeshVFXScale x (0, 0, 100) @1549, rotation 0, scale (clamp(s.X x 0.5, 0, 0.75), clamp(s.Y x 0.8,
//!   0, 1.2), clamp(s.Z x 0.8, 0, 1.3)) @1257-1494, SnapToTarget, auto destroy) @1666: the location is relative to
//!   the mesh (no socket).
//! - ReceiveEndPlay @2619: VFX.K2_DestroyComponent @2895.
//! - then @1773 -> @15: VFX.SetFloatParameter(Team, SelectFloat(0.6, the owner's BPI_GetColorValue, IsLocalOnline))
//!   @114-220 (Fx::team), and binds the owner's colour change @278-301 (@404-609 sets Team again).
//!
//! It is not added to the visibility tracking. GetActiveMeshVFXScale is 1 for every fighter of the port, so the scale is
//! (0.5, 0.8, 0.8) and the place 100 cm up the mesh. The sim has no event for it: it is seen here as the buff being on
//! the fighter (as the electric gauntlets).

use std::collections::HashMap;

use mvs_sim::World;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, V3};

use crate::fx::Fx;
use crate::fx::chars::{CharFxHooks, ProjRule};

// Deviation: OnMeshChanged @3669 (the shield spawned again on the new mesh) is not modelled (one mesh per fighter), nor
// the colour change's Team @404-609 (the Team is read at the spawn).

const PS: &str = "PS_FinnShield";
const BUFF: &str = "Buff_ProjectileBlock_C";
const LOC: V3 = [0.0, 0.0, 100.0];
const SCALE: V3 = [0.5, 0.8, 0.8];

/// What its effects keep between steps: each fighter's shield by its id.
#[derive(Debug, Default)]
pub(crate) struct ProjectileBlock {
    shields: HashMap<i32, SystemId>,
}

impl ProjectileBlock {
    fn clear(&mut self, fx: &mut Fx) {
        for (_, h) in self.shields.drain() {
            fx.player.kill(h);
        }
    }
}

impl CharFxHooks for ProjectileBlock {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        for f in w.all_ids() {
            let id = w.fighter(f).id;
            let list = &w.fighter(f).buffs.buffs;
            let on = list.iter().any(|b| b.frames > 0.0 && w.names.str(b.cls) == BUFF);
            if on == self.shields.contains_key(&id) {
                continue;
            }
            if let Some(h) = self.shields.remove(&id) {
                fx.player.kill(h);
            }
            if on {
                let follow = fx.socket_follow(w, id, None, LOC, &IDENTITY, SCALE);
                let team = fx.team(id);
                if let Some(h) = fx.attach_to_mesh(w, id, PS, "None", follow, &[("Team", team)]) {
                    self.shields.insert(id, h);
                }
            }
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
