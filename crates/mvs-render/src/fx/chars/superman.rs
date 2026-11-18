//! Superman's effects: the Ice Breath pellets' PS_IceBreath and
//! his spawned actors' components (the Heat Vision sweep's eyes and pre-impact, the laser damage's
//! explosion, the air Ice Breath stream, the Locomotive Charge marker's bounds). The marker's SkillShotIndicator
//! is the HUD's.

use std::f64::consts::{FRAC_PI_2, PI};

use indexmap::IndexMap;
use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::{WorldActor, WorldActorState};
use mvs_sim::chars::superman::aim_punch::AimMarker;
use mvs_sim::chars::superman::ice::IceStream;
use mvs_sim::chars::superman::laser::{LaserDamage, LaserSweep};
use mvs_sim::events::Event;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::{actor_y, facing};
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_vfx::{Attachment, SystemId};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{att, text, xyz};
use crate::fx::{Follow, Fx};

/// One sim step (s).
const STEP: f64 = 1.0 / 60.0;

/// What Superman's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Superman {
    /// Per Ice Breath pellet (sequence number): its PS_IceBreath.
    pellets: IndexMap<i32, Option<SystemId>>,
    /// Per spawned actor of the classes (sequence number): its view.
    actors: IndexMap<i32, ActorFx>,
    /// The presentation ticks seen (an actor's view not seen this tick goes).
    tick: u64,
}

impl CharFxHooks for Superman {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_actors(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.pellets.clear();
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.pellets.clear();
        self.clear(fx);
    }
}

const PELLET: ProjRule = ProjRule {
    made: Some(pellet_made),
    tick: Some(pellet_tick),
    end: Some(pellet_end),
    systems: Some(pellet_systems),
    ..ProjRule::NONE
};

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    ("Superman_IceBreath_C", PELLET),
    ("Superman_IceBreathEducated_C", PELLET),
    // The Heat Vision sweeps are World actors: entered for own_emitter only (the ground effect, laser.rs's event).
    (SWEEP, ProjRule { own_emitter: Some(ground_effect), ..ProjRule::NONE }),
    (SWEEP_AIR, ProjRule { own_emitter: Some(ground_effect), ..ProjRule::NONE }),
];

/// SupermanGroundLazer SpawnGroundEffectMulticast (Attach false) @11000-11644: SpawnEmitterAtLocation(
/// PS_SupermanLaserPreImpact, the ImpactPoint, rotator (Roll 0, Pitch, Yaw 90), scale (s, s, 0.5)) @11392 with Team =
/// the owner's colour index as a float (no 0.6 select) @11531-11644, set where BaseFighter is valid. The sim raises it
/// as a projectileDeath event of the sweep (mvs-sim chars/superman/laser.rs: its rot and s); the shared spawn would
/// draw it with one scale and the 0.6 select.
fn ground_effect(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { fighter, vfx, x, y, ox, rot, scale, .. } = *e else { return false };
    if vfx.map(|v| w.names.str(v)) != Some(PRE_IMPACT) {
        return false;
    }
    let s = f64::from(scale);
    let team = [("Team", f64::from(fx.color_index(fighter)))];
    let params: &[(&str, f64)] = if w.by_id(fighter).is_some() { &team } else { &[] };
    fx.player.spawn(
        PRE_IMPACT,
        [f64::from(ox), f64::from(-x), f64::from(y)],
        rot.map(f64::from),
        [s, s, GROUND_EFFECT_Z_SCALE],
        params,
    );
    true
}

/// The ground effect's scale Z (MakeVector(s, s, 0.5) @11341).
const GROUND_EFFECT_Z_SCALE: f64 = 0.5;

fn me(c: &mut CharFx) -> &mut Superman {
    match c {
        CharFx::Superman(s) => s,
        _ => unreachable!("a Superman rule entered by another module"),
    }
}

/// The pellet: PS_IceBreath (PS_SupermanIceBreathPuffs, relative location (-1, 0, 1), relative scale
/// 0.16) under the root Capsule (bAbsoluteScale), turned with the actor (bRotationFollowsVelocity), scaled with the
/// actor's scale; BeginPlay @4773 Team = the colour index as a float (no local 0.6).
fn pellet_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let comp = actor_def(w, ActorRef::Projectile(st.id), pr.cls)
        .and_then(|d| d.components.get("PS_IceBreath"))
        .and_then(Value::as_obj);
    let Some(comp) = comp else { return };
    let Some(template) = text(comp, "Template") else { return };
    let rel = xyz(comp.get("RelativeLocation"), 0.0);
    let rs = xyz(comp.get("RelativeScale3D"), 1.0)[0];
    let team = f64::from(fx.color_index(pr.owner_id));
    let f = Follow::Projectile { seq: st.seq, rel, rel_rot: IDENTITY, scale: [rs; 3] };
    let h = fx.spawn_follow(w, template, f, 1.0, &[("Team", team)]);
    me(c).pellets.insert(st.seq, h);
}

/// ReceiveTick @3939-4232: SetActorScale3D(scale + 10 x DeltaSeconds), then PS_IceBreath's Scale parameter the new
/// scale + 10 x DeltaSeconds.
fn pellet_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    if let Some(Some(h)) = me(c).pellets.get(&st.seq) {
        fx.player.set_param(*h, "Scale", f64::from(pr.scale) + 10.0 * STEP);
    }
}

fn pellet_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(Some(h)) = me(c).pellets.shift_remove(&st.seq) {
        fx.player.kill(h);
    }
}

/// PS_IceBreath, for a reflect's pause.
fn pellet_systems(c: &CharFx, st: &ProjFxState) -> Vec<SystemId> {
    match c {
        CharFx::Superman(s) => s.pellets.get(&st.seq).copied().flatten().into_iter().collect(),
        _ => Vec::new(),
    }
}

// ---- Superman's spawned actors (World actors: laser.rs LaserSweep / LaserDamage, aim_punch.rs AimMarker,
// ice.rs IceStream), read by class through the fields named below. SupermanFlightHit, SupermanGrabListener,
// SupermanUpSpecialListener: no visible component.

const SWEEP: &str = "SupermanGroundLazer_C";
const SWEEP_AIR: &str = "SupermanGroundLazerAir_C";
const DAMAGE: &str = "SupermanGroundLazerDamage_C";
const STREAM: &str = "Superman_IceBreath_Stream_C";
const MARKER: &str = "SupermanAimPunch_C";
const EYES: &str = "PS_SupermanLaserEyes";
const PRE_IMPACT: &str = "PS_SupermanLaserPreImpact";
const IMPACT: &str = "PS_SupermanLaserImpact";
const ICE_STREAM: &str = "PS_SupermanIceBreathStream";
const BOUNDS: &str = "PS_SupermanSkillBounds";
const EYE_BONES: [&str; 2] = ["fml_un_R_eyeBall_end", "fml_un_L_eyeBall_end"];
const BOUNDS_SCALE: f64 = 1.05 * 1.2;
const EXPLOTION_SCALE: f64 = 1.5;
const ONE: V3 = [1.0; 3];

/// One spawned actor's view.
#[derive(Debug)]
struct ActorFx {
    fx: Vec<Option<SystemId>>,
    /// The damage: Explotion is active; the marker: its bounds are made (or hidden for good).
    on: bool,
    /// The eyes' and the stream's places.
    follows: Vec<Follow>,
    seen: u64,
    /// The marker's bounds: AnglePoint's offset from the owner at UpdateSkillshotUI, their rows and scale.
    off: (f64, f64),
    rot: Rows,
    scale: f64,
}

impl ActorFx {
    fn new() -> ActorFx {
        ActorFx { fx: Vec::new(), on: false, follows: Vec::new(), seen: 0, off: (0.0, 0.0), rot: IDENTITY, scale: 1.0 }
    }
}

fn rad(d: f64) -> f64 {
    d * PI / 180.0
}

/// Rotator (pitch, yaw +-90, 0) of the direction (dy, dz) written into rows (FRotationMatrix; the yaw's cosine is 0):
/// nothing written for a zero direction.
fn look_rows(out: &mut Rows, dy: f64, dz: f64) {
    let l = dy.hypot(dz);
    if l < 1e-6 {
        return;
    }
    let (cp, sp, sy) = (dy.abs() / l, dz / l, if dy < 0.0 { -1.0 } else { 1.0 });
    *out = [0.0, cp * sy, sp, -sy, 0.0, 0.0, 0.0, -sp * sy, cp];
}

fn rows_of(a: &Attachment) -> Rows {
    let r = &a.rot;
    [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2]]
}

/// The pawn's side: yaw -90 facing right, +90 left.
fn side_rows(right: bool) -> Rows {
    rotation_rows(0.0, if right { -FRAC_PI_2 } else { FRAC_PI_2 }, 0.0)
}

impl Superman {
    fn clear(&mut self, fx: &mut Fx) {
        for v in self.actors.drain(..).map(|(_, v)| v) {
            for h in v.fx.into_iter().flatten() {
                fx.player.kill(h);
            }
        }
    }

    /// Each live actor of the classes gets its view at first sight, placed each presentation tick
    /// (its follows each drawn frame); an actor gone takes its components with it (K2_DestroyActor).
    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        self.tick += 1;
        for (_, a) in w.actors.actors.iter() {
            if a.dead {
                continue;
            }
            let c = w.names.str(a.cls);
            if ![SWEEP, SWEEP_AIR, DAMAGE, STREAM, MARKER].contains(&c) {
                continue;
            }
            let v = match self.actors.get_mut(&a.seq) {
                Some(v) => v,
                None => {
                    let v = match &a.state {
                        WorldActorState::LaserSweep(_) => sweep_view(fx, w, a.owner_id),
                        WorldActorState::IceStream(s) => stream_view(fx, w, a.owner_id, w.names.str(s.socket)),
                        _ => ActorFx::new(),
                    };
                    self.actors.entry(a.seq).or_insert(v)
                }
            };
            v.seen = self.tick;
            let size = a.size_scale.unwrap_or(1.0);
            match &a.state {
                WorldActorState::LaserSweep(s) => place_sweep(fx, w, v, s, facing(w, a.owner) > 0.0),
                WorldActorState::LaserDamage(d) => place_damage(fx, v, d, a.owner_id, f64::from(size)),
                WorldActorState::IceStream(s) => place_stream(fx, w, v, s, f64::from(size)),
                WorldActorState::AimMarker(m) => place_marker(fx, w, v, m, a),
                _ => {}
            }
        }
        let tick = self.tick;
        let player = &mut fx.player;
        self.actors.retain(|_, v| {
            let keep = v.seen == tick;
            if !keep {
                for h in v.fx.iter().flatten() {
                    player.kill(*h);
                }
            }
            keep
        });
    }
}

/// SupermanGroundLazer / SupermanGroundLazerAir (the Heat Vision sweep, attached to the pawn by its spawner):
/// components L_LaserEye and R_LaserEyes (PS_SupermanLaserEyes) and PS_SupermanLaserPreImpact (bHiddenInGame) under
/// the Capsule (bAbsoluteScale); the Cylinder mesh is bHiddenInGame and nothing shows it: not drawn. BeginPlay
/// @4639-4755: Team on the three = SelectFloat(0.6, the colour index, IsLocalOnlinePlayer). ReceiveTick ->
/// SetBeamVisuals @7777: EyeRotation = FindLookAtRotation(start, end) of the trace (in the play plane), EyeScaleX =
/// |start - (hit ? ImpactPoint : end)| / 2; R_LaserEyes.K2_SetWorldTransform(GetSocketLocation(fml_un_R_eyeBall_end),
/// EyeRotation, (EyeScaleX, 1, 1)) @10003, L_LaserEye the same at fml_un_L_eyeBall_end @10246;
/// PS_SupermanLaserPreImpact.SetHiddenInGame(not hit) @7838 and K2_SetWorldLocation(ImpactPoint) on a hit @8053 (its
/// rotation stays the pawn's). SpawnGroundEffectMulticast @9288 (PS_SupermanLaserPreImpact at each hit) is the sim's
/// projectileDeath event, the class rule's own emitter (ground_effect).
// Deviation: the transforms are written in the actor's tick and the components then ride the pawn until the next;
// here the eye bones' place of each drawn frame is used (the socket follows).
// Unverified: a hidden component goes on ticking.
fn sweep_view(fx: &mut Fx, w: &World, owner: i32) -> ActorFx {
    let mut v = ActorFx::new();
    let team = fx.team(owner);
    for bone in EYE_BONES {
        let f = fx.socket_follow(w, owner, Some(bone), [0.0; 3], &IDENTITY, ONE);
        let at = fx.place(w, &f).unwrap_or_else(|| att([0.0; 3], &IDENTITY, Some(ONE)));
        v.fx.push(fx.player.spawn_attached(EYES, &at, 1.0, &[("Team", team)]));
        v.follows.push(f);
    }
    let pre = att([0.0; 3], &side_rows(true), Some(ONE));
    v.fx.push(fx.player.spawn_attached(PRE_IMPACT, &pre, 1.0, &[("Team", team)]));
    v
}

fn place_sweep(fx: &mut Fx, w: &World, v: &ActorFx, s: &LaserSweep, right: bool) {
    let mut rot = IDENTITY;
    look_rows(&mut rot, f64::from(-(s.ex - s.sx)), f64::from(s.ey - s.sy));
    let sx = (s.ex - s.sx).hypot(s.ey - s.sy) / 2.0;
    for (k, f) in v.follows.iter().enumerate() {
        if let (Some(Some(h)), Some(a)) = (v.fx.get(k), fx.place(w, f)) {
            fx.player.attach(*h, &att(a.loc, &rot, Some([f64::from(sx), 1.0, 1.0])));
        }
    }
    if let Some(Some(h)) = v.fx.get(2) {
        fx.player.attach(*h, &att([0.0, f64::from(-s.ex), f64::from(s.ey)], &side_rows(right), Some(ONE)));
        fx.player.set_visible(*h, s.hit);
    }
}

/// SupermanGroundLazerDamage: Explotion (PS_SupermanLaserImpact, relative scale 1.5 under the Capsule; the Sphere
/// mesh is bHiddenInGame: not drawn). BeginPlay @248 Deactivate; ActivateGroundLaserExplosionMulticast @3455
/// Activate(false) and Team = the colour index as a float @3700 (no 0.6); the end @71 Deactivate, the actor
/// destroyed 0.066 s later. Spawned with rotation 0 (@3973); the root's scale is the sim's size scale.
// Unverified: the component auto-activates with the actor and is deactivated in BeginPlay; whether a particle of
// that first activation is seen is not measured (drawn from the activation on).
fn place_damage(fx: &mut Fx, v: &mut ActorFx, d: &LaserDamage, owner: i32, size: f64) {
    let s = EXPLOTION_SCALE * size;
    let at = att([0.0, f64::from(-d.x), f64::from(d.y)], &IDENTITY, Some([s; 3]));
    if d.exploding != v.on {
        v.on = d.exploding;
        if !d.exploding {
            if let Some(Some(h)) = v.fx.first() {
                fx.player.deactivate(*h);
            }
            return;
        }
        for h in v.fx.drain(..).flatten() {
            fx.player.kill(h);
        }
        let team = f64::from(fx.color_index(owner));
        v.fx.push(fx.player.spawn_attached(IMPACT, &at, 1.0, &[("Team", team)]));
        return;
    }
    if let Some(Some(h)) = v.fx.first() {
        fx.player.attach(*h, &at);
    }
}

/// Superman_IceBreath_Stream (the air Ice Breath; attached to the pawn at the notify's socket 'VB HeadSocket', a
/// virtual bone on the head): IceStream (PS_SupermanIceBreathStream) under the Capsule. BeginPlay @174: Team = the
/// owner's BPI_GetColorValue (0 when the owner is no fighter); ReceiveTick @690:
/// IceStream.K2_SetRelativeRotation(Roll 0, Pitch Rotation, Yaw 0).
fn stream_view(fx: &mut Fx, w: &World, owner: i32, socket: &str) -> ActorFx {
    let mut v = ActorFx::new();
    let f = fx.socket_follow(w, owner, Some(socket), [0.0; 3], &IDENTITY, ONE);
    let team = f64::from(fx.color_index(owner));
    let at = fx.place(w, &f).unwrap_or_else(|| att([0.0; 3], &IDENTITY, Some(ONE)));
    v.fx.push(fx.player.spawn_attached(ICE_STREAM, &at, 1.0, &[("Team", team)]));
    v.follows.push(f);
    v
}

fn place_stream(fx: &mut Fx, w: &World, v: &ActorFx, s: &IceStream, size: f64) {
    let (Some(Some(h)), Some(f)) = (v.fx.first(), v.follows.first()) else { return };
    let Some(a) = fx.place(w, f) else { return };
    let rot = compose_rows(&rotation_rows(rad(f64::from(s.rotation)), 0.0, 0.0), &rows_of(&a));
    // (the root's scale, the sim's size scale, on IceStream)
    let scale = match a.scale {
        Some(sc) => Some(sc.map(|k| k * size)),
        None => (size != 1.0).then_some([size; 3]),
    };
    fx.player.attach(*h, &att(a.loc, &rot, scale));
}

/// SupermanAimPunch (the Locomotive Charge marker): the Sphere mesh is bHiddenInGame (not drawn); the
/// SkillShotIndicator widget is the HUD's. UpdateSkillshotUI, once (@10242): SpawnEmitterAttached(
/// PS_SupermanSkillBounds, the instigator's root, None, AnglePoint, FacingLeft ? (0, 90, 180) : (0, -90, 180) (Pitch,
/// Yaw, Roll), (1.05, 1.05, 1.05) x (IsSniper ? SniperAimDistance / (AimDistance / 1.2) : 1.2), KeepWorldPosition)
/// @10622, for a locally controlled owner (@10091); HideUI @11873 destroys it. The sim's `bounds` is 0 before that
/// call, else the facing then.
// Deviation: not drawn: the two line widgets (the pawn's AimPunchLine_Right / _Left, UI_Skillshot_Aimpunch_Line)
// that UpdateSkillshotUI sizes and turns @10890-11478.
// The emitter keeps its world offset from the pawn's root (KeepWorldPosition under the root: the pawn's turn would
// carry it, and he does not turn inside the montage).
fn place_marker(fx: &mut Fx, w: &World, v: &mut ActorFx, m: &AimMarker, a: &WorldActor) {
    if m.hidden {
        for h in v.fx.drain(..).flatten() {
            fx.player.kill(h);
        }
        v.on = true;
        return;
    }
    let f = a.owner;
    let (ox, oy) = (w.fighter(f).body.x, actor_y(w, f));
    if !v.on {
        // (ownClient: offline every fighter, online the local one)
        if m.bounds == 0.0 || (fx.online && a.owner_id != fx.me as i32) {
            return;
        }
        v.on = true;
        v.off = (f64::from(m.pivot_x - ox), f64::from(m.pivot_y - oy));
        v.scale = if m.sniper {
            let p = actor_def(w, ActorRef::Fighter(f), a.cls).map(|d| &d.p);
            let g = |k: &str| p.and_then(|p| p.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
            1.05 * (g("SniperAimDistance") / (g("AimDistance") / 1.2))
        } else {
            BOUNDS_SCALE
        };
        v.rot = rotation_rows(0.0, if m.bounds < 0.0 { FRAC_PI_2 } else { -FRAC_PI_2 }, PI);
        let at = att([0.0, -(f64::from(ox) + v.off.0), f64::from(oy) + v.off.1], &v.rot, Some([v.scale; 3]));
        v.fx.push(fx.player.spawn_attached(BOUNDS, &at, 1.0, &[]));
        return;
    }
    if let Some(Some(h)) = v.fx.first() {
        fx.player
            .attach(*h, &att([0.0, -(f64::from(ox) + v.off.0), f64::from(oy) + v.off.1], &v.rot, Some([v.scale; 3])));
    }
}
