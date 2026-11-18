//! ShaggyShockwaveActor_C, Shaggy's raged down special, and its data children (
//! `Shockwave`; its `ActorKind` and its hits through the world: `actors.rs`). The state below is the class's own
//! fields; the `SimActor` ones (`dead`, `sizeScale`, the charge and the granted buffs) are the
//! [`crate::actors::WorldActor`]'s.
//!
//! No movement, attachment or HitBox: four spheres are dropped onto the ground ahead, 0.15 s apart, and each runs one
//! pfgDoDamage over its overlaps. Spawned by the Spawner notifies of Shaggy_Rage_D_Montage and the air smash's Land
//! section (Root socket, identity transform, not attached); the rage ends through the montages' own RemoveRage
//! notifies. Sim space: x = -UE Y, y = UE Z.

use crate::Real;
use crate::State;
use crate::actors::world_actor::WorldActorState;
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::boxes::WorldSphere;
use crate::combat::elem::charge_buff;
use crate::combat::hit::hurt_hit;
use crate::combat::hits::{
    actor_hit_enemy, ally_hit, apply_projectile_hit, dealer, hit_buffs_ally, hit_buffs_call, projectile_blocks,
};
use crate::combat::spawned_hits::{damage_actor_call, damage_spawned_actor_call};
use crate::combat::{HitPacket, OnHitBuffRow, hits::DamageCall, hits::do_damage};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::anim::{notify_data, sample_boxes};
use crate::fighter::common::{actor_data, latent_frames, warn_once};
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::moves::V2;
use crate::fighter::traces::{response_to, sphere_overlap_actors};
use crate::fighter::{mesh_wx, mesh_wy};
use crate::math as js;
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{all_fighters, flag, num, num_in, obj, rows};
use crate::world::World;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::{SpawnSpotA, SpawnSpotR, spawn_spot, spawned_moved};
use crate::world::stage_query::Shapes;
use mvs_collision::Ground;
use mvs_data::value::{Props, Value};

/// The DecalAlpha timeline: length 0.1 s, Alpha linear 1 -> 0.
const DECAL_FADE: Real = 0.1;

/// The five sphere components, in the stages' order; the last is the stage-0 fallback.
const COMPS: [&str; 5] = ["Shockwave0", "Shockwave1", "Shockwave2", "Shockwave3", "ShockwaveDefault"];

/// `Shockwave`.
#[derive(State, Debug, Default)]
pub struct Shockwave {
    pub attack_id: i32,
    /// The actor origin: the Root socket + 200 forward (BeginPlay AddActorWorldOffset).
    pub x: Real,
    pub y: Real,
    /// TravellingLeft (BPI_IsFacingLeft of the instigator) as -1.
    pub facing: Real,
    /// HitDamage before the per-stage writes (the class's; a stage's hit is read from the class data's, the same
    /// values, by `stage_hit`).
    pub hit: HitPacket,
    /// Shockwave0..3 and ShockwaveDefault: local X (forward from the actor origin), local Z and SphereRadius x scale.
    /// The cooked components carry no SphereRadius, so it is USphereComponent's constructor default 32.
    pub spheres: Vec<[Real; 3]>,
    /// The world Z spawn_shockwave's K2_SetWorldLocation put each sphere at (its ImpactPoint), by sphere; `None`
    /// until the first.
    pub placed: Option<Vec<Option<Real>>>,
    /// `starting_height`, then the previous stage's ground Z.
    pub height: Real,
    pub stage: i32,
    /// Processings left on the pending latent Delay.
    pub wait: i32,
    pub then: Option<Then>,
    /// The Decal component's DecalMIC Alpha scalar: 1 from the construction script until stage 3 plays the
    /// DecalAlpha timeline. View only.
    pub decal_alpha: Real,
    /// The DecalAlpha timeline's position, -1 before PlayFromStart.
    pub decal_t: Real,
    /// The stage's ground box (the constructor's `ground`); its `platforms` are the World's trace blockers.
    pub ground: Ground,
}

/// What the pending Delay resumes into (`then`).
#[derive(State, Copy, PartialEq, Eq, Debug)]
pub enum Then {
    Stage,
    Destroy,
}

/// One stage's sphere as `fire` places it: the component (0..3 a stage's, 4 the ShockwaveDefault fallback), where it
/// stands, its radius, whether spawn_shockwave ran (false for the fallback, which plays no audio or visuals), and its
/// UseVacuumHit (the final hitpause only without it).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fired {
    pub n: i32,
    pub comp: usize,
    pub x: Real,
    pub y: Real,
    pub r: Real,
    pub spawned: bool,
    pub vacuum: bool,
}

/// `ShockHit`: one stage's hit as `shockHits` reads it: the sphere, the stage's HitDamage and the OnHitBuffs rows.
#[derive(Debug)]
pub struct ShockHit {
    pub fired: Fired,
    pub hit: Props,
    pub buffs: Vec<Props>,
}

/// A SphereTraceSingle's hit: the hit sphere's Location Z, the ImpactPoint Z, and whether it started inside.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Trace {
    loc: Real,
    impact: Real,
    initial: bool,
}

/// The latent Delays `fire` sets, in processings (`latentFrames` of 0.15 s, 0.5 s and the decal fade).
#[derive(Clone, Copy, Debug)]
pub struct Delays {
    pub stage: i32,
    pub destroy: i32,
    pub decal: i32,
}

impl Delays {
    pub fn of_build() -> Self {
        let fr = |s: Real| to_int32(latent_frames(s));
        Delays { stage: fr(0.15), destroy: fr(0.5), decal: fr(DECAL_FADE) }
    }
}

impl Shockwave {
    /// The constructor past the placement: the spheres from the class's components.
    pub fn new(
        attack_id: i32,
        facing: Real,
        x: Real,
        y: Real,
        hit: HitPacket,
        components: &Props,
        ground: Ground,
    ) -> Self {
        let spheres = COMPS
            .iter()
            .map(|k| {
                let c = obj(components, k);
                let rel = |key: &str, sub: &str| c.and_then(|c| num_in(c, key, sub));
                [
                    rel("RelativeLocation", "X").unwrap_or(0.0),
                    rel("RelativeLocation", "Z").unwrap_or(0.0),
                    c.and_then(|c| num(c, "SphereRadius")).unwrap_or(32.0) * rel("RelativeScale3D", "X").unwrap_or(1.0),
                ]
            })
            .collect();
        Shockwave {
            attack_id,
            x,
            y,
            facing,
            hit,
            spheres,
            placed: None,
            height: 0.0,
            stage: 0,
            wait: 0,
            then: None,
            decal_alpha: 1.0,
            decal_t: -1.0,
            ground,
        }
    }

    /// ReceiveBeginPlay (authority): AddActorWorldOffset(200 forward), the ground probe from 5 above to 400 below the
    /// origin (starting_height = the hit sphere's Location Z; the SceneRoot Z without a hit), then stage 0.
    /// `boxes`: what blocks the ground trace besides the floor block (World.blockers('trace') with the polygon solids).
    pub fn begin(&mut self, boxes: Shapes, k: Real, d: Delays) -> Vec<Fired> {
        self.x += 200.0 * self.facing;
        let t = self.trace(boxes, self.x, self.y + 5.0, self.y - 400.0);
        self.height = t.map_or(self.y, |t| t.loc);
        self.fire(boxes, k, d)
    }

    /// The actor tick's latent actions. A Delay made in BeginPlay is first counted down in the spawn frame; one made
    /// by a stage resuming from a Delay is added after that frame's processing, so it starts counting on the next.
    // Unverified: the actor's tick slot (taken after the fighters, with the projectiles).
    pub fn step(&mut self, dead: &mut bool, boxes: Shapes, k: Real, d: Delays) -> Vec<Fired> {
        if *dead {
            return Vec::new();
        }
        // Unverified: the timeline component's first update is taken as the tick after PlayFromStart.
        if self.decal_t >= 0.0 && self.decal_t < DECAL_FADE {
            self.decal_t = js::min(DECAL_FADE, self.decal_t + DELTA_SECONDS);
            self.decal_alpha = js::max(0.0, 1.0 - self.decal_t / DECAL_FADE);
        }
        let Some(then) = self.then else { return Vec::new() };
        self.wait -= 1;
        if self.wait > 0 {
            return Vec::new();
        }
        if then == Then::Destroy {
            *dead = true;
            return Vec::new();
        }
        self.fire(boxes, k, d)
    }

    /// spawn_shockwave(sphere N, next N + 1, height, N) and shockwave_hit. Trace 1 at the sphere's XY from height +
    /// 100 down to height - 120 (a blocking hit that does not start in penetration); the sphere goes to its
    /// ImpactPoint. Trace 2 at the next sphere's XY around that Z: when it hits too, this stage vacuums toward the
    /// next one. The spheres are the root's children: their relative X and radius at the root's scale `k`
    /// (SetSizeScale comes after BeginPlay, so stage 0 is at 1).
    fn fire(&mut self, boxes: Shapes, k: Real, d: Delays) -> Vec<Fired> {
        let n = self.stage;
        let i = usize::try_from(n).expect("a stage of 0..3");
        let f = self.facing * k;
        let sp = self.spheres[i];
        let x = self.x + sp[0] * f;
        let t1 = self.trace(boxes, x, self.height + 100.0, self.height - 120.0);
        let Some(t1) = t1.filter(|t| !t.initial) else {
            self.then = Some(Then::Destroy);
            self.wait = d.destroy;
            if n > 0 {
                return Vec::new();
            }
            // Stage 0 without ground: shockwave_hit(ShockwaveDefault, 0, false, false) where the sphere stands.
            let dd = self.spheres[4];
            return vec![Fired {
                n,
                comp: 4,
                x: self.x + dd[0] * f,
                y: self.y,
                r: dd[2] * k,
                spawned: false,
                vacuum: false,
            }];
        };
        let mut vacuum = false;
        if n < 3 {
            let t2 = self.trace(boxes, self.x + self.spheres[i + 1][0] * f, t1.impact + 100.0, t1.impact - 120.0);
            vacuum = t2.is_some_and(|t| !t.initial);
        }
        self.height = t1.impact;
        // collision_sphere.K2_SetWorldLocation(ImpactPoint) @885
        let placed = self.placed.get_or_insert_with(Vec::new);
        if placed.len() <= i {
            placed.resize(i + 1, None);
        }
        placed[i] = Some(t1.impact);
        if n < 3 {
            self.stage += 1;
            self.then = Some(Then::Stage);
            self.wait = d.stage;
        } else {
            // Decal.SetDecalMaterial, DecalAlpha (0.1 s timeline) -> DecalAlpha__FinishedFunc -> Delay(0.5) -> destroy.
            self.then = Some(Then::Destroy);
            self.wait = d.decal + d.destroy;
            self.decal_t = 0.0;
        }
        vec![Fired { n, comp: i, x, y: t1.impact, r: sp[2] * k, spawned: true, vacuum }]
    }

    /// The five spheres (Custom, object type HitBox, ProjectileDestructible: Overlap, on from the spawn) where they
    /// stand, at their relative place under the root (its scale too) until spawn_shockwave moves one to its
    /// ImpactPoint: what a ProjectileBarrier's StopProjectiles meets.
    // Unverified: a sphere moved by K2_SetWorldLocation is taken to keep its world Z when the root's scale changes
    // after (the build keeps the relative transform the move wrote).
    pub fn hit_shapes(&self, k: Real, out: &mut Vec<WorldSphere>) {
        let f = self.facing * k;
        for (i, sp) in self.spheres.iter().enumerate().take(5) {
            let at = self.placed.as_ref().and_then(|p| p.get(i).copied().flatten());
            out.push(WorldSphere {
                x: self.x + sp[0] * f,
                y: at.unwrap_or(self.y + sp[1] * k),
                r: sp[2] * k,
                ..WorldSphere::default()
            });
        }
    }

    /// SphereTraceSingle (radius 0.5, ProjectileGroundTrace, fighters ignored) straight down from z0 to z1 at x
    /// against the floor block. The stage's pass-through platforms block the trace as the floor does (profile
    /// Platform), and so do the pieces of a C019_Ship and the polygon solids: the first shape from the top is the hit.
    fn trace(&self, s: Shapes, x: Real, z0: Real, z1: Real) -> Option<Trace> {
        let mut best = trace_box(&self.ground, x, z0, z1);
        for p in s.boxes {
            if best.is_some_and(|b| b.initial) {
                break;
            }
            let t = trace_box(p, x, z0, z1);
            if let Some(t) = t
                && (t.initial || best.is_none_or(|b| t.impact > b.impact))
            {
                best = Some(t);
            }
        }
        // a polygon solid: the shape traces' sphere swept down
        for q in s.polys {
            if best.is_some_and(|b| b.initial) {
                break;
            }
            if let Some((loc, impact, initial)) = q.down(x, z0, z1, 0.5)
                && (initial || best.is_none_or(|b| impact > b.impact))
            {
                best = Some(Trace { loc, impact, initial });
            }
        }
        best
    }
}

fn trace_box(g: &Ground, x: Real, z0: Real, z1: Real) -> Option<Trace> {
    let r = 0.5;
    if x < crate::math::narrow(g.left - r) || x > crate::math::narrow(g.right + r) {
        return None;
    }
    if z0 - crate::math::narrow(r) < crate::math::narrow(g.top)
        && z0 + crate::math::narrow(r) > crate::math::narrow(g.bottom)
    {
        return Some(Trace { loc: z0, impact: z0, initial: true });
    }
    if z0 - crate::math::narrow(r) < crate::math::narrow(g.top)
        || z1 - crate::math::narrow(r) > crate::math::narrow(g.top)
    {
        return None;
    }
    Some(Trace { loc: crate::math::narrow(g.top + r), impact: crate::math::narrow(g.top), initial: false })
}

/// shockwave_hit's writes to HitDamage: Damage = DamageAmounts[N]; HitPauseTime, OnHitEffectOverride and IgnoreKBI
/// by UseVacuumHit; VacuumInfo = VacuumConfig with UseVacuumKnockBack and VacuumSocketOffset (VacuumLocations[N], X
/// signed toward the travel direction in UE Y: +1 left).
fn stage_hit(p: &Props, facing: Real, n: i32, vacuum: bool) -> Props {
    let i = usize::try_from(n).unwrap_or(0);
    let mut h = obj(p, "HitDamage").cloned().unwrap_or_default();
    let at = |k: &str| p.get(k).and_then(Value::as_arr).and_then(|a| a.get(i));
    let loc = at("VacuumLocations").and_then(Value::as_obj);
    let damage = at("DamageAmounts").and_then(Value::as_f64).unwrap_or(0.0);
    h.insert("Damage".into(), Value::Num(damage));
    let pause = if vacuum { num(p, "VacuumHitPause") } else { num(p, "FinalHitPauseTime") };
    h.insert("HitPauseTime".into(), Value::Num(f64::from(pause.unwrap_or(0.0))));
    let effect = if vacuum { None } else { p.get("FinalHitEffectOverride").cloned() };
    h.insert("OnHitEffectOverride".into(), effect.unwrap_or(Value::Null));
    h.insert("IgnoreKBI".into(), Value::Bool(vacuum));
    let mut vi = obj(p, "VacuumConfig").cloned().unwrap_or_default();
    vi.insert("UseVacuumKnockBack".into(), Value::Bool(vacuum));
    let lx = loc.and_then(|l| num(l, "X")).unwrap_or(0.0);
    let ly = loc.and_then(|l| num(l, "Y")).unwrap_or(0.0);
    let mut off = Props::new();
    off.insert("X".into(), Value::Num(f64::from(lx * if facing < 0.0 { 1.0 } else { -1.0 })));
    off.insert("Y".into(), Value::Num(f64::from(ly)));
    vi.insert("VacuumSocketOffset".into(), Value::Obj(off));
    h.insert("VacuumInfo".into(), Value::Obj(vi));
    h
}

/// OnHitBuffs: empty on vacuum hits, else 'On Hit Buffs Param' (Buff_Weakened_C x 5).
fn stage_buffs(p: &Props, vacuum: bool) -> Vec<Props> {
    if vacuum {
        return Vec::new();
    }
    obj(p, "On Hit Buffs Param").map(|o| rows(o, "OnHitBuffs")).unwrap_or_default()
}

fn sw_of(w: &World, id: ActorId) -> Option<&Shockwave> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::Shockwave(s) => Some(s),
        _ => None,
    }
}

/// The kind's `spawn`: HandleActorPostSpawn's placement (the Root socket plus the notify translation, yaw from the
/// facing), then PostActorConstruction's FindTeleportSpot, ahead of BeginPlay.
fn spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let n = &notify_data(w, &data, f, r.n).p;
    let Some(a) = actor_data(w, &data, ActorRef::Fighter(f), r.cls) else {
        return (WorldActorState::Shockwave(Shockwave::default()), true);
    };
    let facing = crate::fighter::facing(w, f);
    let t = obj(n, "Transforms").and_then(|t| obj(t, "Translation"));
    let (ty, tz) = (t.and_then(|t| num(t, "Y")).unwrap_or(0.0), t.and_then(|t| num(t, "Z")).unwrap_or(0.0));
    let socket = crate::registry::props::text(n, "Socket").unwrap_or("");
    let ch = w.fighter(f).ch;
    // Deviation: the box index's socket shape is looked up for Root alone (`boxes.byName.get('socket:' + Socket)`):
    //   every notify that spawns the class names Root; another socket warns as a missing one does.
    let si =
        (socket == "Root").then(|| w.fixed.samplers.get(&ch)?.by_name.get(&name!("socket:Root")).copied()).flatten();
    let mut at = None;
    if let Some(si) = si
        && let Some(o) = w.fixed.samplers.get(&ch).and_then(|b| b.offsets.get(usize::try_from(si).ok()?).copied())
        && let mut pose = Vec::new()
        && sample_boxes(w, f, &mut pose)
    {
        let o = usize::try_from(o).expect("a box offset");
        at = Some((f64::from(pose[o]), f64::from(pose[o + 1])));
    }
    let (fwd, up) = at.unwrap_or_else(|| {
        warn_once(w, name!("spawn socket missing"));
        (0.0, 0.0)
    });
    let x = mesh_wx(w, f) + crate::math::narrow((fwd + f64::from(ty)) * f64::from(facing));
    let y = mesh_wy(w, f) + crate::math::narrow(up) + tz;
    let ground = w.stage.ground;
    let hit = HitPacket::from_props(&w.names, obj(&a.p, "HitDamage").unwrap_or(&Props::new()), None); // actor.p.HitDamage ?? {}
    let mut sw = Shockwave::new(r.attack_id, facing, x, y, hit, &a.components, ground);
    let spot = spawn_spot(
        w,
        &SpawnSpotR { cls: r.cls, p: n.clone() },
        &SpawnSpotA { components: a.components.clone() },
        sw.x,
        sw.y,
    );
    sw.x = spot.x;
    sw.y = spot.y;
    (WorldActorState::Shockwave(sw), false)
}

/// Runs `fire` / `step` on the actor's state with the world's trace blockers and its scale.
fn advance(w: &mut World, id: ActorId, first: bool) -> Vec<Fired> {
    let boxes = w.blocker_shapes(BlockKind::Trace);
    let d = Delays::of_build();
    let Some(a) = w.actors.actors.get_mut(id) else { return Vec::new() };
    // The root's scale (the WorldActor's `size_scale`, `SimActor.sizeScale`): the spheres' places and radii under it.
    let k = a.size_scale.unwrap_or(1.0);
    let WorldActorState::Shockwave(sw) = &mut a.state else { return Vec::new() };
    if first { sw.begin(boxes.view(), k, d) } else { sw.step(&mut a.dead, boxes.view(), k, d) }
}

fn begin_play(w: &mut World, id: ActorId) {
    let fired = advance(w, id, true);
    shock_hits(w, id, &fired);
}

fn step(w: &mut World, id: ActorId) {
    let fired = advance(w, id, false);
    shock_hits(w, id, &fired);
}

fn hit_shapes(w: &World, id: ActorId, out: &mut Vec<WorldSphere>) -> bool {
    let Some(a) = w.actors.actors.get(id) else { return false };
    let Some(sw) = sw_of(w, id) else { return false };
    if a.dead {
        return false;
    }
    sw.hit_shapes(a.size_scale.unwrap_or(1.0), out);
    true
}

/// The kind: `spawn` is HandleActorPostSpawn's placement, `begin_play` its ReceiveBeginPlay (the first stage, behind
/// the parent's charge copy and GrantBuffsFromInstigator), `step` the actor tick's latent actions. Die With Instigator,
/// the Lab reset and the buff children's EndPlay are the World's. `size`: `fire` and `hit_shapes` read the scale.
pub fn entry(class: WorldActorClass) -> ActorEntry {
    let hooks = WorldActorHooks {
        spawn: Some(spawn),
        begin_play: Some(begin_play),
        step: Some(step),
        hit_shapes: Some(hit_shapes),
        size: Some(SizeHooks::default()),
        ..WorldActorHooks::default()
    };
    ActorEntry::new(ActorClass::World(class), ActorHooks::World(hooks))
}

/// A shockwave stage (shockwave_hit): GetOverlappingActors(null) of the sphere @713, and pfgDoDamage @1235 on every
/// actor but the Instigator (@1010), whatever its team. A fighter of another team: an invulnerable one takes the
/// dodge branch (DodgingFighters, BPI_SetDodgedSuccessfully), else the hit. A team mate (CanBeHitBy false): the
/// OnHitBuffs rows of an ally (@5931-6835) and NotifyAllyOfHit (@21344), nothing for an invulnerable one (@4608 ->
/// @7452 pops). The other actors: [`wave_actor_hits`]. Then, for a spawned stage, Spawn Shockwave Visuals (the impact
/// emitter at the ImpactPoint, slowed for FinalHitPauseTime after a non-vacuum hit) and Shockwave Audio
/// (AK_Rock_Explosion at the ImpactPoint).
/// First the overlap update of the move that came before: an enemy ProjectileBarrier's StopProjectiles begin overlap
/// destroys the wave ahead of shockwave_hit's GetOverlappingActors @713, so it deals none of the hits.
// Deviation: the sphere is tested against the hurtboxes as a circle in the (Y, Z) plane; the fighters are taken first,
// then the other actors (the build's list is the overlap order).
// Unverified: a wave destroyed that way deals no later hit (stock DestroyActor unregisters its components; not read in
// the exe).
pub fn shock_hits(w: &mut World, id: ActorId, fired: &[Fired]) {
    let me = ActorRef::Actor(id);
    spawned_moved(w, me);
    let Some(a) = w.actors.actors.get(id) else { return };
    if a.dead {
        return;
    }
    let (f, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(f), cls) else { return };
    let p = &def.p;
    let Some(sw) = sw_of(w, id) else { return };
    let (facing, attack_id) = (sw.facing, sw.attack_id);
    for fr in fired {
        let h = ShockHit { fired: *fr, hit: stage_hit(p, facing, fr.n, fr.vacuum), buffs: stage_buffs(p, fr.vacuum) };
        // the packet and its rows as the hit calls take them
        let hp = HitPacket::from_props(&w.names, &h.hit, None);
        let rows = buff_rows(w, &h.buffs);
        let mut hit = false;
        // a minion too (GetOverlappingActors(null))
        for vic in all_fighters(w) {
            let v = w.fighter(vic);
            if vic == f || v.life.dead != 0.0 || crate::fighter::no_hit(w, vic) {
                continue;
            }
            let mut vp = Vec::new();
            if !sample_boxes(w, vic, &mut vp) {
                continue;
            }
            let sphere = WorldSphere { x: fr.x, y: fr.y, r: fr.r, ..WorldSphere::default() };
            if hurt_hit(w, vic, &[sphere], &vp).is_none() {
                continue;
            }
            if projectile_blocks(w, &dealer(w, me), vic) {
                hit_buffs_call(w, f, &rows); // the entry rows run ahead of the pop (@0-1265)
                continue;
            }
            if w.fighter(vic).team.team == w.fighter(f).team.team {
                hit_buffs_call(w, f, &rows);
                // read after the entry rows, as the getter is: a row may change the count
                if crate::fighter::invulnerable(w, vic) {
                    continue;
                }
                let mult = charge_buff(w, me);
                hit_buffs_ally(w, f, vic, &rows, mult);
                ally_hit(w, &dealer(w, me), vic, &hp);
                continue;
            }
            if crate::fighter::invulnerable(w, vic) {
                hit_buffs_call(w, f, &rows);
                let kb = num(&h.hit, "BaseKnockBack").unwrap_or(0.0);
                let tick = w.flow.tick;
                attack_dodged(w, me, vic, attack_id, kb, tick, None, false);
                continue;
            }
            dodge_over(w, me, vic);
            let v = w.fighter(vic);
            let at = V2 { x: v.body.x, y: v.body.y };
            if apply_projectile_hit(w, &dealer(w, me), vic, &hp, &rows, at, None, &[], 0.0) {
                hit = true; // a failed hit is no hit
                actor_hit_enemy(w, &dealer(w, me), vic);
            }
        }
        wave_actor_hits(w, me, COMPS[fr.comp], fr.x, fr.y, fr.r, &h.hit, &h.buffs);
        if !fr.spawned {
            continue;
        }
        let hit_pause = if hit && !fr.vacuum { num(p, "FinalHitPauseTime").unwrap_or(0.0) } else { 0.0 };
        let fighter = w.fighter(f).id;
        w.events.push(Event::ShockwaveVfx { fighter, cls, n: fr.n, x: fr.x, y: fr.y, facing, hit_pause });
        if let Some(ev) = crate::registry::props::name_of(w, p, "AK_Rock_Explosion")
            && flag(p, "AK_Rock_Explosion")
        {
            w.events.push(Event::Sound {
                fighter,
                ev,
                x: Some(fr.x),
                y: Some(fr.y),
                hit: None,
                pooled: false,
                key: None,
            });
        }
    }
}

/// The object types the port gathers bodies of: an overlap list is taken over these.
const GATHERED: [Real; 12] = [0.0, 1.0, 2.0, 3.0, 8.0, 10.0, 11.0, 14.0, 16.0, 18.0, 15.0, 19.0];
/// `OBJ.HitBox`.
const OBJ_HIT_BOX: Real = 7.0;

/// The gathered object types a component template does not ignore: the bodies it can
/// overlap.
pub fn overlap_types(comp: &Props) -> Vec<Real> {
    let ignore = name!("ECR_Ignore");
    GATHERED.iter().copied().filter(|&o| response_to(Some(comp), o) != ignore).collect()
}

/// GetOverlappingActors(null) of a query dealer's sphere of object type HitBox (`comp`, a component of the dealer's
/// class: ShaggyShockwaveActor's and C017_ShockwaveActor's Shockwave<n>) for the actors that are no fighter, and the
/// dealer's pfgDoDamage(null, the actor, the packet, the rows) on each ([`actor_victim_hit`]). An actor is listed
/// when one of its bodies is of an object type the sphere does not ignore and answers HitBox with Overlap or Block.
/// Not here: the Destructible object type (5), of which the port gathers no body.
#[allow(clippy::too_many_arguments, reason = "the parameters")]
pub fn wave_actor_hits(
    w: &mut World,
    dealer: ActorRef,
    comp: &str,
    x: Real,
    y: Real,
    r: Real,
    hit: &Props,
    rows: &[Props],
) {
    let Some((owner, cls)) = dealer_owner(w, dealer) else { return };
    let data = w.data.clone();
    let Some(c) = actor_data(w, &data, ActorRef::Fighter(owner), cls).and_then(|d| obj(&d.components, comp)) else {
        return;
    };
    let types = overlap_types(c);
    let found = sphere_overlap_actors(w, x, y, r, &types, &[Some(dealer)], OBJ_HIT_BOX);
    if found.is_empty() {
        return;
    }
    let mut packet = hit.clone();
    if !rows.is_empty() {
        let mut o = Props::new();
        o.insert("OnHitBuffs".into(), Value::Arr(rows.iter().cloned().map(Value::Obj).collect()));
        packet.insert("OnHitBuffs".into(), Value::Obj(o));
    }
    for o in found {
        if dealer_dead(w, dealer) {
            return;
        }
        // the fighters: the caller's loop
        if !matches!(o, ActorRef::Fighter(_)) {
            actor_victim_hit(w, dealer, o, &packet, rows, None);
        }
    }
}

/// A dealer actor's owner and class.
fn dealer_owner(w: &World, d: ActorRef) -> Option<(FighterId, Name)> {
    match d {
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|a| (a.owner, a.cls)),
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).and_then(|p| Some((p.owner?, p.cls))),
        _ => None,
    }
}

fn dealer_dead(w: &World, d: ActorRef) -> bool {
    match d {
        ActorRef::Actor(id) => w.actors.actors.get(id).is_none_or(|a| a.dead),
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_none_or(|p| p.dead),
        _ => false,
    }
}

/// `actorKey`: an actor's key in a dealer's HitEachActorOnlyOnce list.
pub fn actor_key(w: &World, o: ActorRef) -> Option<(u8, i32)> {
    match o {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| (b'p', p.seq)),
        ActorRef::Item(id) => w.actors.items.get(id).map(|i| (b'i', i.seq)),
        ActorRef::Actor(id) => w.actors.actors.get(id).map(|a| (b'a', a.seq)),
        _ => None,
    }
}

/// pfgDoDamage of a spawned dealer's handler on an actor that is no fighter: the entry's OnHitBuffs rows (@0-1265),
/// CheckIfActorHasBeenHit with HitEachActorOnlyOnce (@2883: the dealer's list `once`, by [`actor_key`]), the
/// IsInvulnerableToProjectiles pop for a dealer that is a projectile (@4300-4598), then the call's steps through the
/// world: a projectile's (World.damageSpawnedActor), a World.actors one's: with IsProjectile and neither
/// DestroyProjectiles nor ReflectProjectiles the @19122 broadcasts alone, else World.damageActor (the hit location
/// the victim's place); an item or another kind goes to World.doDamage.
/// `rows` is the call's OnHitBuffsParam on every route (the entry rows, the ally rows of a HitActor of the dealer's
/// team, the victim's): the callers pass it explicitly (ShaggyShockwaveActor shockwave_hit @1235: empty on a vacuum
/// hit, else On Hit Buffs Param; C017_ShockwaveActor @1592: On Hit Buffs Param; HarleyBomb @3879: On Hit Buffs), so
/// the handler's own rows (the dealer's class rows for its own HitBoxDamage) are never taken here. The shockwave's
/// HitDamage at the call is the stage's (shockwave_hit writes it @448-620), the packet it passes.
// Unverified: the actor is entered in the dealer's list once it passes the pops; IsInvulnerableToProjectiles is the
// class default. A caller whose packets never have HitEachActorOnlyOnce passes no list.
pub fn actor_victim_hit(
    w: &mut World,
    dealer: ActorRef,
    o: ActorRef,
    packet: &Props,
    rows: &[Props],
    once: Option<&mut indexmap::IndexSet<(u8, i32)>>,
) {
    let victim = victim_of(w, o);
    let Some(v) = victim else { return };
    if v.dead {
        return;
    }
    let Some((owner, _)) = dealer_owner(w, dealer) else { return };
    let call_rows = buff_rows(w, rows);
    hit_buffs_call(w, owner, &call_rows);
    let key = if flag(packet, "HitEachActorOnlyOnce") && once.is_some() { actor_key(w, o) } else { None };
    if let (Some(k), Some(set)) = (key, once.as_deref())
        && set.contains(&k)
    {
        return;
    }
    let data = w.data.clone();
    let dest = v
        .owner
        .and_then(|vo| actor_data(w, &data, ActorRef::Fighter(vo), v.cls))
        .and_then(|d| obj(&d.components, "Comp_Actor_Destructible"));
    if dest.is_some_and(|d| flag(d, "IsInvulnerableToProjectiles")) && dealer_is_projectile(w, dealer) {
        return;
    }
    if let (Some(k), Some(set)) = (key, once) {
        set.insert(k);
    }
    let hp = HitPacket::from_props(&w.names, packet, None);
    // OnHitBuffsParam: the caller's rows on every route, the ally rows and the victim's included.
    let call = DamageCall { hit_box: false, comp: None, clank: false, rows: Some(&call_rows) };
    match o {
        ActorRef::Projectile(b) => {
            damage_spawned_actor_call(w, dealer, b, &hp, &call);
        }
        ActorRef::Actor(b) => {
            if dest.is_some_and(|d| flag(d, "IsProjectile")) && !flag(packet, "DestroyProjectiles") {
                if flag(packet, "ReflectProjectiles") {
                    warn_once(w, name!("reflect of a World.actors actor not ported"));
                }
                return; // @19122
            }
            let at = V2 { x: v.x.unwrap_or(0.0), y: v.y.unwrap_or(0.0) };
            let at = dealer_at(w, dealer, at, v);
            damage_actor_call(w, dealer, b, &hp, Some(at), &call);
        }
        _ => {
            do_damage(w, dealer, o, &hp, &call);
        }
    }
}

/// What `actorVictimHit` reads of its victim: its death, its place, its owner and class.
#[derive(Clone, Copy)]
struct Victim {
    dead: bool,
    x: Option<Real>,
    y: Option<Real>,
    owner: Option<FighterId>,
    cls: Name,
}

fn victim_of(w: &World, o: ActorRef) -> Option<Victim> {
    Some(match o {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            Victim { dead: p.dead, x: Some(p.x), y: Some(p.y), owner: p.owner, cls: p.cls }
        }
        ActorRef::Actor(id) => {
            let a = w.actors.actors.get(id)?;
            let at = world_actor_at(w, id);
            Victim { dead: a.dead, x: at.map(|v| v.x), y: at.map(|v| v.y), owner: Some(a.owner), cls: a.cls }
        }
        ActorRef::Item(id) => {
            let i = w.actors.items.get(id)?;
            Victim { dead: i.dead, x: None, y: None, owner: None, cls: i.cls }
        }
        _ => Victim { dead: false, x: None, y: None, owner: None, cls: Name::NONE },
    })
}

/// A world actor's place (`a.x`, `a.y`) where its state has one.
fn world_actor_at(w: &World, id: crate::actors::ActorId) -> Option<V2> {
    crate::actors::world_actor::world_actor_place_of(w, id).map(|(x, y, _)| V2 { x, y })
}

/// `{x: a.x ?? dealer.x, y: a.y ?? dealer.y}`.
fn dealer_at(w: &World, dealer: ActorRef, at: V2, v: Victim) -> V2 {
    let d = match dealer {
        ActorRef::Actor(id) => world_actor_at(w, id),
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| V2 { x: p.x, y: p.y }),
        _ => None,
    };
    V2 { x: v.x.unwrap_or(d.map_or(at.x, |d| d.x)), y: v.y.unwrap_or(d.map_or(at.y, |d| d.y)) }
}

/// `dealerIsProjectile` of a dealer actor.
fn dealer_is_projectile(w: &World, dealer: ActorRef) -> bool {
    let Some((owner, cls)) = dealer_owner(w, dealer) else { return false };
    crate::combat::hits::class_is_projectile(w, Some(owner), cls)
}

/// OnHitBuffs rows as the hit calls take them.
fn buff_rows(w: &World, rows: &[Props]) -> Vec<OnHitBuffRow> {
    rows.iter().map(|r| OnHitBuffRow::from_props(&w.names, r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: Delays = Delays { stage: 9, destroy: 30, decal: 6 };

    fn wave(ground: Ground) -> Shockwave {
        let mut comps = Props::new();
        for (i, k) in COMPS.iter().enumerate() {
            let mut c = Props::new();
            let mut rel = Props::new();
            rel.insert("X".into(), Value::Num(100.0 * i as f64));
            rel.insert("Z".into(), Value::Num(10.0));
            c.insert("RelativeLocation".into(), Value::Obj(rel));
            comps.insert((*k).into(), Value::Obj(c));
        }
        Shockwave::new(3, -1.0, 0.0, 50.0, HitPacket::default(), &comps, ground)
    }

    #[test]
    fn stages_walk_down_the_floor() {
        let g = Ground { left: -1000.0, right: 1000.0, bottom: -100.0, top: 0.0 };
        let mut sw = wave(g);
        let f = sw.begin(Shapes::NONE, 1.0, D);
        assert_eq!(sw.x, -200.0);
        assert_eq!(sw.height, 0.0);
        assert_eq!(f, vec![Fired { n: 0, comp: 0, x: -200.0, y: 0.0, r: 32.0, spawned: true, vacuum: true }]);
        assert_eq!((sw.then, sw.wait, sw.stage), (Some(Then::Stage), 9, 1));
        let mut dead = false;
        for _ in 0..8 {
            assert!(sw.step(&mut dead, Shapes::NONE, 1.0, D).is_empty());
        }
        let f = sw.step(&mut dead, Shapes::NONE, 1.0, D);
        assert_eq!(f[0].x, -300.0);
        assert_eq!(f[0].n, 1);
    }

    #[test]
    fn no_ground_falls_back_at_stage_0() {
        let g = Ground { left: 500.0, right: 1000.0, bottom: -100.0, top: 0.0 };
        let mut sw = wave(g);
        let f = sw.begin(Shapes::NONE, 1.0, D);
        assert_eq!(f, vec![Fired { n: 0, comp: 4, x: -600.0, y: 50.0, r: 32.0, spawned: false, vacuum: false }]);
        assert_eq!((sw.then, sw.wait), (Some(Then::Destroy), 30));
    }

    #[test]
    fn stage_hit_signs_the_vacuum_offset() {
        let mut p = Props::new();
        let mut loc = Props::new();
        loc.insert("X".into(), Value::Num(20.0));
        p.insert("VacuumLocations".into(), Value::Arr(vec![Value::Obj(loc)]));
        p.insert("DamageAmounts".into(), Value::Arr(vec![Value::Num(4.0)]));
        let h = stage_hit(&p, -1.0, 0, true);
        assert_eq!(num(&h, "Damage"), Some(4.0));
        let off = obj(&h, "VacuumInfo").and_then(|v| obj(v, "VacuumSocketOffset")).and_then(|o| num(o, "X"));
        assert_eq!(off, Some(20.0));
        assert_eq!(h.get("OnHitEffectOverride"), Some(&Value::Null));
    }
}
