//! Comp_Pawn_TeleportToAlly's DashedVFX (sim fighter/match_side.rs: the dash to
//! a team mate): a shared class's effects, made for every match after the characters'.
//!
//! DashMovement @6758: with no valid DashedVFX, DashedVFX = SpawnEmitterAttached(DashVFX, the owner's Mesh, Root,
//! location 0, rotation 0, scale 1, KeepRelativeOffset, auto destroy) @7299 and its Team = Conv_IntToFloat(the owner's
//! Comp_Pawn_SkinManager.GetColorIndex) @7634 (no 0.6 for the local player online); a DashedVFX still valid keeps it.
//! DestroyVFX @8918 (not on a dedicated server): a valid DashedVFX K2_DestroyComponent @9017, behind the shield's
//! Delay(MoveToTime), the launch and Delay(0.5) (@803-1053). The sim's ally vfx_born is the tick DashMovement spawned
//! it in, -1 again at DestroyVFX. DashVFX: the pawn's Comp_Pawn_TeleportToAlly instance (Wonder Woman's
//! PS_WonderWomanDashAllyTrail; None for the others of the roster: Shaggy's trail is his montages' PlayVFX notify).

use std::collections::{BTreeSet, HashMap};

use mvs_sim::World;
use mvs_vfx::SystemId;

use crate::fx::Fx;
use crate::fx::attach::text;
use crate::fx::chars::{CharFxHooks, ProjRule};
use mvs_vfx::math::IDENTITY;

// Deviation: the sim's respawn clears vfx_born, so a trail still on at a respawn goes then, not at DestroyVFX.
// Unverified: it is not added to the visibility tracking (no AddVFX in the graph), so a hidden fighter's trail stays
// drawn.

/// What its effects keep between steps: each fighter's trail by its id (absent: none spawned; None: DashVFX made
/// nothing), and the systems warned of.
#[derive(Debug, Default)]
pub(crate) struct AllyDash {
    trails: HashMap<i32, Option<SystemId>>,
    warned: BTreeSet<String>,
}

impl AllyDash {
    fn clear(&mut self, fx: &mut Fx) {
        for h in self.trails.drain().filter_map(|(_, h)| h) {
            fx.player.kill(h);
        }
    }
}

impl CharFxHooks for AllyDash {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        for f in w.all_ids() {
            let id = w.fighter(f).id;
            let on = w.fighter(f).team.ally.vfx_born >= 0.0;
            if on == self.trails.contains_key(&id) {
                continue;
            }
            if !on {
                if let Some(Some(h)) = self.trails.remove(&id) {
                    fx.player.kill(h);
                }
                continue;
            }
            let props = mvs_sim::fighter::control::pawn_comp_in(&w.data, w, f, "Comp_Pawn_TeleportToAlly_GEN_VARIABLE");
            let Some(ps) = props.and_then(|p| text(p, "DashVFX")).filter(|s| !s.is_empty()) else {
                self.trails.insert(id, None); // (no DashVFX: SpawnEmitterAttached of None makes nothing)
                continue;
            };
            let follow = fx.socket_follow(w, id, Some("Root"), [0.0; 3], &IDENTITY, [1.0; 3]);
            let team = f64::from(fx.color_index(id));
            let h = fx.attach_to_mesh(w, id, ps, "Root", follow, &[("Team", team)]);
            if h.is_none() && self.warned.insert(ps.to_owned()) {
                eprintln!("ally dash: {ps} is not loaded");
            }
            self.trails.insert(id, h);
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
