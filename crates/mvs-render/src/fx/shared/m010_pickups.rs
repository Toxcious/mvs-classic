//! The Court's three powerup pickups (PickupPowerup_Jump_C / _Power_C / _Speed_C) and their buffs' particles,
//! drawn from the sim's powerup chain (stages/the_court/pickups.rs:
//! `CourtPowerup::pickup` / `taken`) through the match's emitter path: the collector's root and his mesh's Root socket
//! are the match's (`Fx`), not the scenery's. The systems are the scenery file's (the block `court`'s `pickups`).
//! Graphs: PickupPowerup_<type>, Pickup<type>Buff (offsets their ubergraphs').
//! - Nothing before the arm: PowerupStaticMesh and the type component are hidden in game.
//! - The arm (the BeginPlay Delay's resume @15, the sim's `armed`): SpawnEmitterAttached(PS_M010_Powerup_Intro) @53
//!   with SetFloatParameter(Color, 0 / .5 / 1) @149, then SpawnEmitterAttached(PS_M010_Powerup_<type>) @203, both on
//!   the type component PS_M010_Powerup_<type> (on the root at location and rotation 0, scale 0.6: the template),
//!   location 0, rotation 0, scale 1, KeepRelativeOffset: at the pickup's place, world scale 0.6. Spawned in the latent
//!   pass: first ticked the next frame. Killed with the pickup (its K2_DestroyActor @764, or the Lab reset's: the
//!   spawned components are the actor's own).
//! - SpaceJamPickupVFXMulticast(collector) @854-1467 (the sim's `taken`): UI_Functions PopText @1359 (the Court block's:
//!   scenery/the_court/pickups.rs), then SpawnEmitterAttached(PS_M010_Powerup_<type>_Outro, the collector's K2_GetRootComponent, no socket, 0,
//!   rot(0, outroYaw, 0), 1, KeepRelativeOffset) @1467: the capsule's place and yaw (-90 facing right, +90 left), its
//!   actor scale, turned by the outro's yaw.
//! - The buffs (PickupJumpBuff_C ..): BeginPlay SpawnEmitterAttached(PS_C016_PowerUp_<type>_Buff, the owner's
//!   BPI_GetMainMesh, Root, 0, rot(0, 90, 0), 1, SnapToTargetIncludingScale) @165, K2_DestroyComponent at
//!   ReceiveDestroyed @301: shown on the mesh's Root socket while the fighter holds the buff (Fighter.buffs by class).
//! - A stamp seen FX_LATE ticks late or more (a rollback that played it again) spawns nothing.
// Deviation: one buff particle per fighter and class; a second buff actor of the class the build might spawn while one
// lives (a pickup taken again inside its 10 s) is not drawn twice.
// Unverified: the outro's follower keeps the collector's actor scale of the collection;
// the outro lasts a moment of the collector's life.
// Unverified: the arm's systems not being in the match's effects is not warned.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::stages::the_court::pickups::CourtPowerup;
use mvs_sim::world::stage::StageOwn;
use mvs_vfx::math::{IDENTITY, Rows, rotation_rows};
use mvs_vfx::{Attachment, SystemId};

use crate::fx::chars::{CharFxHooks, ProjRule};
use crate::fx::{Follow, Fx};

/// A stamp seen this many ticks late or more spawns nothing.
const FX_LATE: i32 = 6;
/// PS_M010_Powerup_<type>_GEN_VARIABLE RelativeScale3D.
const TYPE_SCALE: [f64; 3] = [0.6, 0.6, 0.6];
const INTRO: &str = "PS_M010_Powerup_Intro";

/// A pickup class's drawing (PICKUP_KINDS' `introColor`, `outro`, `outroYaw`, `particles`).
struct Kind {
    cls: &'static str,
    intro_color: f64,
    outro: &'static str,
    outro_yaw: f64,
    particles: &'static str,
}

const KINDS: [Kind; 3] = [
    Kind {
        cls: "PickupPowerup_Jump_C",
        intro_color: 0.0,
        outro: "PS_M010_Powerup_Jump_Outro",
        outro_yaw: 0.0,
        particles: "PS_M010_Powerup_Jump",
    },
    Kind {
        cls: "PickupPowerup_Power_C",
        intro_color: 0.5,
        outro: "PS_M010_Powerup_Power_Outro",
        outro_yaw: -90.0,
        particles: "PS_M010_Powerup_Power",
    },
    Kind {
        cls: "PickupPowerup_Speed_C",
        intro_color: 1.0,
        outro: "PS_M010_Powerup_Speed_Outro",
        outro_yaw: 0.0,
        particles: "PS_M010_Powerup_Speed",
    },
];

/// The buffs' particles (BUFF_PARTICLES), by buff class.
const BUFFS: [(&str, &str); 3] = [
    ("PickupJumpBuff_C", "PS_C016_PowerUp_Jump_Buff"),
    ("PickupPowerBuff_C", "PS_C016_PowerUp_Power_Buff"),
    ("PickupSpeedBuff_C", "PS_C016_PowerUp_Speed_Buff"),
];

fn kind_of(cls: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.cls == cls)
}

/// What its effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct M010Pickups {
    /// The pickup whose arm effects are spawned.
    shown_id: Option<i32>,
    shown: Vec<SystemId>,
    taken_id: Option<i32>,
    /// A fighter's (by pawn id) buff particle by buff class.
    buff_fx: IndexMap<i32, BTreeMap<&'static str, SystemId>>,
}

impl M010Pickups {
    fn kill(&mut self, fx: &mut Fx) {
        for h in self.shown.drain(..) {
            fx.player.kill(h);
        }
        self.shown_id = None;
    }

    fn clear(&mut self, fx: &mut Fx) {
        self.kill(fx);
        // (the buffs' particles go with the effects cleared)
        self.buff_fx.clear();
    }
}

/// The Court's powerup chain of the World's stage (None on another stage or before its create).
fn powerup(w: &World) -> Option<&CourtPowerup> {
    match &w.stage_run.as_ref()?.s {
        StageOwn::Court(c) => Some(&c.script.powerup),
        _ => None,
    }
}

impl CharFxHooks for M010Pickups {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        let Some(p) = powerup(w) else { return };
        let tick = w.flow.tick;
        // the arm
        match &p.pickup {
            Some(k) if !k.dead && k.armed >= 0 => {
                if self.shown_id != Some(k.id) {
                    self.kill(fx);
                    self.shown_id = Some(k.id);
                    if let Some(kind) = kind_of(w.names.str(k.cls))
                        && tick - k.armed < FX_LATE
                    {
                        let at = Attachment {
                            loc: [0.0, f64::from(-k.x), f64::from(k.y)],
                            rot: rows3(&IDENTITY),
                            scale: Some(TYPE_SCALE),
                        };
                        let intro = fx.player.spawn_level(INTRO, &at, true, &[("Color", kind.intro_color)], 0, false);
                        let ty = fx.player.spawn_level(kind.particles, &at, true, &[], 0, false);
                        self.shown.extend(intro.into_iter().chain(ty));
                    }
                }
            }
            _ => {
                if self.shown_id.is_some() {
                    self.kill(fx);
                }
            }
        }
        // the collection
        if let Some(t) = &p.taken
            && self.taken_id != Some(t.id)
        {
            self.taken_id = Some(t.id);
            if let Some(kind) = kind_of(w.names.str(t.cls))
                && let Some(fid) = w.by_id(t.fighter)
                && tick - t.tick < FX_LATE
            {
                let b = &w.fighter(fid).body;
                let s = b.actor_scale.map_or([b.size_scale; 3], |s| [s[0], s[0], s[1]]);
                let rel = rotation_rows(0.0, kind.outro_yaw.to_radians(), 0.0);
                let f = Follow::Root { id: t.fighter, rel, up: 0.0, scale: Some(s.map(f64::from)) };
                fx.spawn_follow(w, kind.outro, f, 1.0, &[]);
            }
        }
        // the buffs' particles
        let buff_rot: Rows = rotation_rows(0.0, 90f64.to_radians(), 0.0);
        for &fid in &w.players {
            let fi = w.fighter(fid);
            let id = fi.id;
            let held = self.buff_fx.entry(id).or_default();
            for (cls, system) in BUFFS {
                let has = fi.buffs.buffs.iter().any(|b| w.names.str(b.cls) == cls);
                let h = held.get(cls).copied();
                if has && !h.is_some_and(|h| fx.player.alive(h)) {
                    let f = fx.socket_follow(w, id, Some("Root"), [0.0; 3], &buff_rot, [1.0; 3]);
                    if let Some(h) = fx.attach_to_mesh(w, id, system, "Root", f, &[]) {
                        held.insert(cls, h);
                    }
                } else if !has && let Some(h) = held.remove(cls) {
                    fx.player.kill(h);
                }
            }
        }
    }

    /// A Lab reset: the pickup goes with it, and so the arm's effects.
    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

fn rows3(r: &Rows) -> [[f64; 3]; 3] {
    [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]]
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
