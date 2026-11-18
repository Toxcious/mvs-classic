//! Shaggy's effects: Buff_ScoobySnack's emitters on whoever has it, his rage's
//! ScoobySnackBox glow and pickup, the ally teleport burst's blast. ShaggyShockwaveActor's emitters and decal are
//! [`shockwave`] (the first of the shared classes' modules).

pub(in crate::fx) mod shockwave;

use indexmap::IndexMap;
use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::events::Event;
use mvs_sim::fighter::common::actor_def;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, compose_rows, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{D2R, att, mul_row, rot_rows, text, xyz};
use crate::fx::{Follow, Fx};

pub const SNACK: &str = "Buff_ScoobySnack_C";
const PICKUP: &str = "PS_ShaggyScoobySnack_Pickup";
const HAND: &str = "PS_ShaggyScoobySnackBuff";
const AURA: &str = "PS_ShaggyRageAuraBuff_Loop";
const CANCEL: &str = "PS_WonderWomanBracers_Cancel";
const SNACK_BOX: &str = "ScoobySnackBox_C";
const PICKUP_VFX: &str = "PS_ShaggyScoobySnacksPickup";

/// What Shaggy's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Shaggy {
    /// Per fighter id: the buff's components while it is on (ParticleSystems).
    snack: IndexMap<i32, Vec<Option<SystemId>>>,
    /// Per snack box (actor sequence number): its glow.
    boxes: IndexMap<i32, Option<SystemId>>,
    /// Per teleport burst (actor sequence number): the blast once made.
    bursts: IndexMap<i32, SystemId>,
}

impl CharFxHooks for Shaggy {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_snack_boxes(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_snack(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (SNACK_BOX, ProjRule { own_emitter: Some(pickup_emitter), ..ProjRule::NONE }),
    // Shaggy_AllyTeleportBurst: the component (PS_ShaggyRageSPN_Explosion) is the shared path's;
    // ActivateObject's BlastVisuals adds SpawnEmitterAttached(PS_ShaggyRageSPN_Explosion, Capsule, SnapToTarget,
    // rotation 0, GrowthScale x 0.1), which goes with the actor.
    ("Shaggy_AllyTeleportBurst_C", ProjRule { tick: Some(burst_tick), end: Some(burst_end), ..ProjRule::NONE }),
];

fn me(c: &mut CharFx) -> &mut Shaggy {
    match c {
        CharFx::Shaggy(s) => s,
        _ => unreachable!("a Shaggy rule entered by another module"),
    }
}

/// The pickup emitter: PickupVisuals @503, no Team.
fn pickup_emitter(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx, x, y, ox, scale, .. } = *e else { return false };
    if !vfx.is_some_and(|v| w.names.str(v) == PICKUP_VFX) {
        return false;
    }
    fx.player.spawn(PICKUP_VFX, [f64::from(ox), f64::from(-x), f64::from(y)], [0.0; 3], [f64::from(scale); 3], &[]);
    true
}

/// The blast once the burst is activated.
fn burst_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    if !pr.activated || s.bursts.contains_key(&st.seq) {
        return;
    }
    let g = actor_def(w, ActorRef::Projectile(st.id), pr.cls)
        .and_then(|d| d.p.get("GrowthScale"))
        .map_or(1.0, |v| xyz(Some(v), 1.0)[0])
        * 0.1;
    let team = fx.team(pr.owner_id);
    let f = Follow::Projectile { seq: st.seq, rel: [0.0; 3], rel_rot: IDENTITY, scale: [g / f64::from(pr.scale); 3] };
    if let Some(h) = fx.spawn_follow(w, "PS_ShaggyRageSPN_Explosion", f, 1.0, &[("Team", team)]) {
        s.bursts.insert(st.seq, h);
    }
}

fn burst_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(h) = me(c).bursts.shift_remove(&st.seq) {
        fx.player.kill(h);
    }
}

/// Buff_ScoobySnack on fighter `id` (a buff with frames left).
pub fn has_snack(w: &World, id: i32) -> bool {
    let Some(f) = w.by_id(id) else { return false };
    w.fighter(f).buffs.buffs.iter().any(|b| w.names.str(b.cls) == SNACK && b.frames > 0.0)
}

impl Shaggy {
    /// The place step (once a presentation tick). BPI After Status Update Multicast with BuffApplied: SpawnVFX @98,
    /// all four SpawnEmitterAttached on the owner's mesh, KeepRelativeOffset, Team = SelectFloat(0.6, ColorIndex,
    /// IsLocalOnlinePlayer): @325 PICKUP on Root, @942 / @1563 HAND on L_Hand / R_Hand, @2300 AURA on Spine_Socket.
    /// RemoveVFX: each destroyed @615, then SpawnEmitterAttached(CANCEL, R_Hand @866 / L_Hand @1555, scale 0.25 /
    /// Mesh.RelativeScale3D); an ending actor runs it twice (BuffRemoved @810, ReceiveEndPlay @1187).
    // Deviation: a spawn that replaces a live buff is not seen; the spending hit's RemoveVFX and EndPlay draw as one
    // removal (two Cancel bursts a hand where the build draws three). The visibility tracking is not ported.
    fn place_snack(&mut self, fx: &mut Fx, w: &World) {
        // destroySnackFx (another class's, asked since the last place): the components go now, the list stays held
        for id in std::mem::take(&mut fx.snack_destroy) {
            if let Some(on) = self.snack.get_mut(&id) {
                for h in on.drain(..).flatten() {
                    fx.player.kill(h);
                }
            }
        }
        let ids: Vec<i32> = fx.pawns.keys().copied().collect();
        for id in ids {
            let has = has_snack(w, id);
            let had = self.snack.contains_key(&id);
            if has == had {
                continue;
            }
            if has {
                let team = fx.team(id);
                let on = [(PICKUP, "Root"), (HAND, "L_Hand"), (HAND, "R_Hand"), (AURA, "Spine_Socket")]
                    .map(|(ps, s)| fx.attach_socket(w, id, ps, s, [0.0; 3], &IDENTITY, [1.0; 3], &[("Team", team)]));
                self.snack.insert(id, on.to_vec());
            } else {
                for h in self.snack.shift_remove(&id).unwrap_or_default().into_iter().flatten() {
                    fx.player.kill(h);
                }
                let team = fx.team(id);
                let s = 0.25 / fx.mesh_relative_scale(w, id);
                for _ in 0..2 {
                    for hand in ["R_Hand", "L_Hand"] {
                        fx.attach_socket(w, id, CANCEL, hand, [0.0; 3], &IDENTITY, [s; 3], &[("Team", team)]);
                    }
                }
            }
        }
    }

    /// The snack box's view: the PS_ShaggyScoobySnack component under the ScoobySnackStaticMesh, the root at
    /// yaw 90 rolled by the box's spin.
    fn place_snack_boxes(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        for (aid, a) in w.actors.actors.iter() {
            let WorldActorState::SnackBox(b) = &a.state else { continue };
            if a.dead || w.names.str(a.cls) != SNACK_BOX {
                continue;
            }
            seen.push(a.seq);
            let cs = actor_def(w, ActorRef::Actor(aid), a.cls).map(|d| &d.components);
            let comp = |n: &str| cs.and_then(|c| c.get(n)).and_then(Value::as_obj);
            let m = comp("ScoobySnackStaticMesh");
            let g = comp("PS_ShaggyScoobySnack");
            let rel_loc = xyz(m.and_then(|m| m.get("RelativeLocation")), 0.0);
            let rel_rot = rot_rows(m.and_then(|m| m.get("RelativeRotation")));
            let rel_scale = xyz(m.and_then(|m| m.get("RelativeScale3D")), 1.0);
            let glow_rel = xyz(g.and_then(|g| g.get("RelativeLocation")), 0.0);
            let root =
                compose_rows(&rotation_rows(0.0, 90.0 * D2R, 0.0), &rotation_rows(0.0, 0.0, f64::from(b.spin) * D2R));
            let loc = [0.0, -b.x, b.y];
            let d = mul_row(rel_loc, &root);
            let mesh = [0, 1, 2].map(|k| f64::from(loc[k]) + d[k]);
            let rows = compose_rows(&rel_rot, &root);
            let gl = [0, 1, 2].map(|k| {
                mesh[k]
                    + rel_scale[0] * glow_rel[0] * rows[k]
                    + rel_scale[1] * glow_rel[1] * rows[3 + k]
                    + rel_scale[2] * glow_rel[2] * rows[6 + k]
            });
            let at = att(gl.map(f64::from), &rows, None);
            match self.boxes.get(&a.seq) {
                Some(Some(h)) => fx.player.attach(*h, &at),
                Some(None) => {}
                None => {
                    let team = fx.team(a.owner_id);
                    let h = g
                        .and_then(|g| text(g, "Template"))
                        .and_then(|t| fx.player.spawn_attached(t, &at, 1.0, &[("Team", team)]));
                    self.boxes.insert(a.seq, h);
                }
            }
        }
        let gone: Vec<i32> = self.boxes.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(Some(h)) = self.boxes.shift_remove(&k) {
                fx.player.kill(h);
            }
        }
    }

    fn clear(&mut self, fx: &mut Fx) {
        for h in self.snack.values().flatten().flatten() {
            fx.player.kill(*h);
        }
        for h in self.boxes.values().flatten() {
            fx.player.kill(*h);
        }
        for h in self.bursts.values() {
            fx.player.kill(*h);
        }
        *self = Shaggy::default();
    }
}
