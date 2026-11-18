//! Scooby's Haunted Mansion's placed actors with a sim: ScoobyWall_C (the two
//! breakable side walls), ScoobyElevator_C (the two trapdoor floors) and ScoobyElevatorSwitch_C (the two switches that
//! lower and raise them). Offsets are each class's ubergraph's unless named.
//!
//! ScoobyElevatorSwitch_C (never ticks: no PrimaryActorTick stored, its latent actions run in the leftover pass):
//! - BeginPlay @2630 (authority, not a dedicated server: @389): AllowHazards false -> K2_DestroyActor @501 (`mod.rs`
//!   create). Deviation: the dedicated server's AllHumanPlayersConnected binding (@2695-2855 -> @2871) is not ported.
//! - ReceiveAnyDamage @572: Damage > 0 -> ScoobyElevatorSwitchFlipped @631: ElevatorEnabled false: nothing; else
//!   ElevatorEnabled false, ScoobyElevatorHitbox.SetGenerateOverlapEvents(false) @652 (the hurt box gone: unverified, the
//!   setter updates no overlap itself: the port's hit test reads the flag at the next test), the state toggled and
//!   MoveScoobyElevatorMulticast(state) @769: the switch montage, Platform_Audio_Start @1720 (platform_switch at the
//!   switch, platform_move on the elevator, its Comp_EdgeTrace UpdateIntersectionsEveryFrame true), gear Emissive 1, the
//!   trapdoor montage, the connected wall's gears when it is valid (@1131), PlayWallChainVisuals @2105 (particle 0, the
//!   wall chain montage, Delay(0.35) @2339 -> @281 particle 1), PlayTrapChainAnims @2394, then MoveComponentTo of the
//!   elevator's DefaultSceneRoot @1597 to its current location + (0, 0, +/-2850) over 1.65 s, linear (`stage_move_to`:
//!   99 updates of 28.788 cm in the leftover pass).
//! - The move's link @15: Platform_Audio_Stop @1977 (platform_stop on the elevator, Delay(0) @2050 -> @516: the edge
//!   flag false the next frame), the switch's end pose, gear Emissive 0, ElevatorEnabled and the hitbox's overlaps on.
//!
//! ScoobyElevator_C: BoxCollision is a solid Floor box all round (a StageRun solid: the based follow one frame late and
//! the depenetration of a box that rose into a capsule are the shared movers', `world::stage`, with their FLAGs).
//! DustVFXCollision's BeginOverlap (@27, any Pawn: the fighters here) with DustGate false: PS_Mansion_Dustfall and
//! platform_creak at the root, DustGate true, Delay(1.5) @300 -> @15 DustGate false.
//!
//! ScoobyWall_C: ReceiveAnyDamage @524 -> DamageScoobyWall: HP - Damage (no clamp: a negative Damage heals), HP <= 0:
//! PlayDestroyVFXMulticast (@288: the destroy emitter and wall_break at ScoobyWallDestroyVFXTransform) then
//! K2_DestroyActor (the floor box, the hit box and the edge component at once); else PlayImpactVisualsMulticast (@10:
//! the impact emitter, the wall montage, wall_hit at ScoobyWallImpactVFXTransform). No death delay, no restore.
//!
//! The fields the drawing reads (the gears, trapdoors, switches, chains, the gear emissive):
//! switch: state, enabled, overlaps, move_at / move_up (the multicast: switch montage Release (false) / Retract
//! (true), the trap chains' montages, the wall chain particle 0 and montage), chain1_at (wall chain particle 1),
//! done_at (the end pose: Down (false) / Up (true) of move_up), dead; elevator: x, y (the root now), move_at / move_up
//! (the trapdoor MoveDown / MoveUp montage), emissive, dust_at / dust_x / dust_y (PS_Mansion_Dustfall), edge_update;
//! wall: hp, hit_at (impact emitter and montage), gear_at / gear_up (the gears' Release (false) / Retract (true)),
//! broken_at (the destroy emitter), dead.

use std::sync::Arc;

use mvs_data::stage::{SolidKind, StagePlacedData, StageSolidData};
use mvs_data::value::Value;

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, StageActorClass};
use crate::combat::boxes::{CH_DAMAGE, WorldSphere};
use crate::events::{Event, SoundKey};
use crate::fighter::common::warn_text;
use crate::fighter::traces::edges::edges_removed;
use crate::math::max;
use crate::registry::entries::StageActorEntry;
use crate::world::World;
use crate::world::stage::{StagePiece, StageState, stage_delay, stage_move_to, stage_solid_enable};
use crate::world::stage_actors::{StageActorOwn, StageHurt};

pub const WALL: &str = "ScoobyWall_C";
pub const ELEVATOR: &str = "ScoobyElevator_C";
pub const SWITCH: &str = "ScoobyElevatorSwitch_C";

// The latent nodes (their key: the node's offset) and their links (the event the stage's timer hook gets).
/// The switch's MoveComponentTo @1597 -> @15.
pub const MOVE_LINK: i32 = 15;
/// The switch's Delay(0) @2050 -> @516.
pub const STOP_DELAY: i32 = 2050;
pub const STOP_LINK: i32 = 516;
/// The switch's Delay(0.35) @2339 -> @281.
pub const CHAIN_DELAY: i32 = 2339;
pub const CHAIN_LINK: i32 = 281;
/// The elevator's Delay(1.5) @300 -> @15.
pub const DUST_DELAY: i32 = 300;
pub const DUST_LINK: i32 = 15;

/// `ScoobyWall`: its own state, its `StageActorOwn` variant.
#[derive(State, Debug, Default)]
pub struct ScoobyWall {
    /// ScoobyWallHP (float32, no clamp).
    pub hp: Real,
    /// The world tick of the last PlayImpactVisualsMulticast, -1 none.
    pub hit_at: i32,
    /// The world tick of PlayDestroyVFXMulticast and K2_DestroyActor, -1.
    pub broken_at: i32,
    /// The world tick of the last PlayScoobyWallGearAnim, -1 none.
    pub gear_at: i32,
    /// Its Up: Retract (true) or Release (false).
    pub gear_up: bool,
}

/// `ScoobyElevator`: its own state, its `StageActorOwn` variant.
#[derive(State, Debug, Default)]
pub struct ScoobyElevator {
    /// The world tick of the last trapdoor montage (MoveDownAnim / MoveUpAnim), -1 none.
    pub move_at: i32,
    pub move_up: bool,
    /// GearMaterial's Emissive (1 while moving).
    pub emissive: Real,
    /// DustGate.
    pub dust_gate: bool,
    /// The world tick of the last PS_Mansion_Dustfall, -1 none.
    pub dust_at: i32,
    /// Where (the root then, sim).
    pub dust_x: Real,
    pub dust_y: Real,
    /// Comp_EdgeTrace.UpdateIntersectionsEveryFrame (read by nothing: the edges take an elevator's crossings at its
    /// place now, unverified).
    pub edge_update: bool,
    /// The fighters (ids) whose capsule overlaps DustVFXCollision at the last test.
    pub dust_in: Vec<i32>,
}

/// `ScoobySwitch`: its own state, its `StageActorOwn` variant.
#[derive(State, Debug, Default)]
pub struct ScoobySwitch {
    /// ScoobyElevatorSwitchState (true: up).
    pub state: bool,
    /// ElevatorEnabled.
    pub enabled: bool,
    /// ScoobyElevatorHitbox's bGenerateOverlapEvents.
    pub overlaps: bool,
    /// ConnectedScoobyElevator's StageData.placed index (`mod.rs` create), -1 none.
    pub elevator: i32,
    /// ConnectedScoobyWall's, -1 none.
    pub wall: i32,
    /// The world tick of the last MoveScoobyElevatorMulticast, -1 none.
    pub move_at: i32,
    /// Its Up.
    pub move_up: bool,
    /// The world tick of the wall chain's second particle (the Delay(0.35) link), -1 none.
    pub chain1_at: i32,
    /// The world tick of the last move's end (the switch's end pose), -1 none.
    pub done_at: i32,
}

// ---- the static data of an actor (read off the stage data where it is used: nothing of it is state)

fn run(w: &World) -> &StageState {
    w.stage_run.as_ref().expect("Scooby's Haunted Mansion: a stage state")
}

fn run_mut(w: &mut World) -> &mut StageState {
    w.stage_run.as_mut().expect("Scooby's Haunted Mansion: a stage state")
}

fn placed(w: &World, index: u32) -> &StagePlacedData {
    let all = w.data.stage.placed.as_deref().unwrap_or(&[]);
    &all[usize::try_from(index).expect("a placed index")]
}

/// `actorOf` / `actorAt`: the stage actor of a StageData.placed index (a negative one is none).
pub fn slot_of(w: &World, index: i32) -> Option<u32> {
    let i = u32::try_from(index).ok()?;
    let k = run(w).actors.iter().position(|a| a.index == i)?;
    u32::try_from(k).ok()
}

/// The part of a solid's name after the actor's (`ScoobyWall1.ScoobyWallHitCollision`).
fn part(s: &StageSolidData) -> &str {
    s.name.split('.').nth(1).unwrap_or("")
}

/// The actor's solids as (id, data).
fn solids_of<'a>(w: &'a World, d: &'a StagePlacedData) -> impl Iterator<Item = (i32, &'a StageSolidData)> {
    let all = w.data.stage.solids.as_deref().unwrap_or(&[]);
    d.solids.iter().flatten().filter_map(move |&id| Some((id, all.get(usize::try_from(id).ok()?)?)))
}

/// The DamageDestructible box (the wall's ScoobyWallHitCollision, the switch's ScoobyElevatorHitbox) as hurt shapes.
fn hurt_boxes(w: &World, d: &StagePlacedData, hurt_part: &str) -> Vec<StageHurt> {
    let mut hurt = Vec::new();
    for (_, s) in solids_of(w, d) {
        if part(s) != hurt_part {
            continue;
        }
        // (the right wall's and the right switch's yaw -179.99995 is on the world's axes: the stage import writes a box)
        if s.kind != SolidKind::Box {
            warn_text(w, &format!("{} {}: {hurt_part} is a {:?}, its box taken unrotated", d.cls, d.name, s.kind));
        }
        let (c, e) = (&s.center, &s.half_extent);
        hurt.push(StageHurt::Box(WorldSphere {
            x: crate::math::narrow(-c[1]),
            y: crate::math::narrow(c[2]),
            z: Some(crate::math::narrow(c[0])),
            r: 0.0,
            bx: Some([
                crate::math::narrow(e[1]),
                0.0,
                0.0,
                0.0,
                crate::math::narrow(e[2]),
                0.0,
                0.0,
                0.0,
                crate::math::narrow(e[0]),
            ]),
            ..WorldSphere::default()
        }));
    }
    if hurt.is_empty() {
        warn_text(w, &format!("{} {}: no {hurt_part} solid in the stage data", d.cls, d.name));
    }
    hurt
}

/// The elevator's BoxCollision: its StageData.solids id, -1 none.
fn floor_of(w: &World, d: &StagePlacedData) -> i32 {
    let f = solids_of(w, d).find(|(_, s)| part(s) == "BoxCollision").map_or(-1, |(id, _)| id);
    if f < 0 {
        warn_text(w, &format!("{} {}: no BoxCollision solid in the stage data", d.cls, d.name));
    }
    f
}

/// The root at its data place, sim.
fn root_of(d: &StagePlacedData) -> (Real, Real) {
    (crate::math::narrow(-d.loc[1]), crate::math::narrow(d.loc[2]))
}

/// The root's UE Z (float32: MoveComponentTo's target is computed from the world location).
fn root_z(d: &StagePlacedData) -> Real {
    crate::math::narrow(d.loc[2])
}

/// A transform value's translation as sim (x, y).
fn transform_at(v: Option<&Value>) -> Option<(Real, Real)> {
    let t = v?.as_obj()?.get("Translation")?.as_obj()?;
    Some((
        crate::math::narrow(-t.get("Y").and_then(Value::as_f64).unwrap_or(0.0)),
        crate::math::narrow(t.get("Z").and_then(Value::as_f64).unwrap_or(0.0)),
    ))
}

/// The elevator's DustVFXCollision (an overlap shape of the stage data): its centre from the root, half extents, sim.
struct Dust {
    dx: Real,
    dy: Real,
    hw: Real,
    hh: Real,
}

fn dust_of(w: &World, d: &StagePlacedData) -> Option<Dust> {
    let root = root_of(d);
    let ds = solids_of(w, d).find(|(_, s)| part(s) == "DustVFXCollision").map(|(_, s)| s);
    let Some(s) = ds else {
        warn_text(w, &format!("{} {}: no DustVFXCollision shape in the stage data", d.cls, d.name));
        return None;
    };
    Some(Dust {
        dx: crate::math::narrow(-s.center[1] - f64::from(root.0)),
        dy: crate::math::narrow(s.center[2] - f64::from(root.1)),
        hw: crate::math::narrow(s.half_extent[1]),
        hh: crate::math::narrow(s.half_extent[2]),
    })
}

/// The name a reference value (its path) points at (".../Map_ScoobyDoo.Map_ScoobyDoo:PersistentLevel.ScoobyElevator2"
/// -> ScoobyElevator2).
pub fn ref_actor(v: Option<&Value>) -> &str {
    match v.and_then(Value::as_str) {
        Some(s) => s.rsplit(['.', ':']).next().unwrap_or(""),
        None => "",
    }
}

// ---- the actors' state

fn wall_mut(w: &mut World, slot: u32) -> &mut ScoobyWall {
    match &mut run_mut(w).actors[slot as usize].own {
        StageActorOwn::ScoobyWall(a) => a,
        _ => panic!("stage actor {slot} is no ScoobyWall"),
    }
}

fn elevator_mut(w: &mut World, slot: u32) -> &mut ScoobyElevator {
    match &mut run_mut(w).actors[slot as usize].own {
        StageActorOwn::ScoobyElevator(a) => a,
        _ => panic!("stage actor {slot} is no ScoobyElevator"),
    }
}

fn switch(w: &World, slot: u32) -> &ScoobySwitch {
    match &run(w).actors[slot as usize].own {
        StageActorOwn::ScoobySwitch(a) => a,
        _ => panic!("stage actor {slot} is no ScoobySwitch"),
    }
}

pub fn switch_mut(w: &mut World, slot: u32) -> &mut ScoobySwitch {
    match &mut run_mut(w).actors[slot as usize].own {
        StageActorOwn::ScoobySwitch(a) => a,
        _ => panic!("stage actor {slot} is no ScoobySwitch"),
    }
}

/// `base`: the actor at its data place (the class's own state set by its create).
fn place_actor(w: &mut World, slot: u32, own: StageActorOwn) {
    let data = Arc::clone(&w.data);
    let a = &mut run_mut(w).actors[slot as usize];
    let d = &data.stage.placed.as_deref().unwrap_or(&[])[a.index as usize];
    (a.x, a.y) = root_of(d);
    a.own = own;
}

/// PF_Post_Event_At_Location(event, place, no Instigator): the pooled AkComponent at the place.
fn pooled(w: &mut World, ev: crate::names::Name, x: Real, y: Real) {
    w.events.push(Event::Sound { fighter: -1, ev, x: Some(x), y: Some(y), hit: None, pooled: true, key: None });
}

/// The elevator actor's own game object (PF_Post_Event(event, ConnectedScoobyElevator)): the key its stop reaches.
fn elevator_key(index: u32) -> SoundKey {
    SoundKey { tag: crate::name!("scoobyElevator"), n: i32::try_from(index).expect("a placed index") }
}

/// K2_DestroyActor: out of the hit tests, every component's collision off at once.
pub fn destroy_actor(w: &mut World, slot: u32) {
    let index = {
        let a = &mut run_mut(w).actors[slot as usize];
        a.dead = true;
        a.index
    };
    let data = Arc::clone(&w.data);
    for &id in placed_data(&data, index).solids.iter().flatten() {
        let on_list = usize::try_from(id).is_ok_and(|i| i < run(w).solids.len());
        if on_list {
            stage_solid_enable(w, u32::try_from(id).expect("a solid id"), false);
        }
    }
}

fn placed_data(data: &mvs_data::GameData, index: u32) -> &StagePlacedData {
    &data.stage.placed.as_deref().unwrap_or(&[])[index as usize]
}

// ---- ScoobyWall_C

/// ScoobyWall_C's entry.
pub fn wall_entry() -> StageActorEntry {
    StageActorEntry {
        create: Some(wall_create),
        hurt: Some(wall_hurt),
        hurt_type: Some(i32::from(CH_DAMAGE)),
        // Unverified: no PhysMaterialOverride on ScoobyWallHitCollision: the light hit sound's SurfaceType 6 is taken.
        surface: Some(6),
        any_damage: Some(wall_any_damage),
        ..base(StageActorClass::ScoobyWall)
    }
}

/// The class's components' defaults (FriendlyFire false, CanTakeDamage true, HitPauseMultiplier 1) and no hooks.
const fn base(class: StageActorClass) -> StageActorEntry {
    StageActorEntry {
        class,
        hurt_type: None,
        friendly_fire: false,
        can_take_damage: true,
        hit_pause_multiplier: None,
        surface: None,
        create: None,
        any_damage: None,
        hit_by_enemy: None,
        hit_by_ally: None,
        knocked_back: None,
        hurt: None,
        sounds: &[],
        damage_disabled: None,
        invulnerable: None,
        current_damage: None,
        victim_pause: None,
        mesh: None,
        reset: None,
        solid_hit: None,
    }
}

fn wall_create(w: &mut World, slot: u32) {
    let data = Arc::clone(&w.data);
    let d = placed_data(&data, run(w).actors[slot as usize].index);
    let hp = crate::math::narrow(d.p.get("ScoobyWallHP").and_then(Value::as_f64).unwrap_or(10.0));
    hurt_boxes(w, d, "ScoobyWallHitCollision");
    place_actor(
        w,
        slot,
        StageActorOwn::ScoobyWall(ScoobyWall { hp, hit_at: -1, broken_at: -1, gear_at: -1, gear_up: false }),
    );
}

fn wall_hurt(w: &World, slot: u32, out: &mut Vec<StageHurt>) -> bool {
    let a = &run(w).actors[slot as usize];
    if a.dead {
        return false;
    }
    out.extend(hurt_boxes(w, placed(w, a.index), "ScoobyWallHitCollision"));
    true
}

/// ReceiveAnyDamage @524 -> DamageScoobyWall.
fn wall_any_damage(w: &mut World, slot: u32, dmg: Real, _causer: ActorRef) {
    let tick = w.flow.tick;
    let (index, hp) = {
        let a = &run(w).actors[slot as usize];
        let StageActorOwn::ScoobyWall(s) = &a.own else { panic!("stage actor {slot} is no ScoobyWall") };
        (a.index, (s.hp - dmg))
    };
    wall_mut(w, slot).hp = hp;
    let data = Arc::clone(&w.data);
    let d = placed_data(&data, index);
    if hp <= 0.0 {
        // PlayDestroyVFXMulticast: ScoobyWallDestroyVFX and wall_break
        wall_mut(w, slot).broken_at = tick;
        let at = transform_at(d.p.get("ScoobyWallDestroyVFXTransform")).unwrap_or_else(|| root_of(d));
        pooled(w, crate::name!("sfx_map_scoobydoo_wall_break"), at.0, at.1);
        // the owner's OnDestroyed: Comp_EdgeTrace's polygon leaves the generator (`dead`), its riders detach
        destroy_actor(w, slot);
        edges_removed(w, index);
    } else {
        // PlayImpactVisualsMulticast: ScoobyWallImpactVFX, ScoobyWallImpactAnim, wall_hit
        wall_mut(w, slot).hit_at = tick;
        let at = transform_at(d.p.get("ScoobyWallImpactVFXTransform")).unwrap_or_else(|| root_of(d));
        pooled(w, crate::name!("sfx_map_scoobydoo_wall_hit"), at.0, at.1);
    }
}

// ---- ScoobyElevator_C

/// ScoobyElevator_C's entry.
pub fn elevator_entry() -> StageActorEntry {
    StageActorEntry { create: Some(elevator_create), ..base(StageActorClass::ScoobyElevator) }
}

fn elevator_create(w: &mut World, slot: u32) {
    let data = Arc::clone(&w.data);
    let d = placed_data(&data, run(w).actors[slot as usize].index);
    floor_of(w, d);
    dust_of(w, d);
    let a = ScoobyElevator {
        move_at: -1,
        move_up: false,
        emissive: 0.0,
        dust_gate: false,
        dust_at: -1,
        dust_x: 0.0,
        dust_y: 0.0,
        edge_update: false,
        dust_in: Vec::new(),
    };
    place_actor(w, slot, StageActorOwn::ScoobyElevator(a));
}

/// The root where its BoxCollision's move left it (the root moves every component).
fn place_elevator(w: &mut World, slot: u32) {
    let data = Arc::clone(&w.data);
    let index = run(w).actors[slot as usize].index;
    let d = placed_data(&data, index);
    let floor = floor_of(w, d);
    let root = root_of(d);
    let (dx, dy) =
        usize::try_from(floor).ok().and_then(|i| run(w).solids.get(i)).map_or((0.0, 0.0), |s| (s.m.dx, s.m.dy));
    let a = &mut run_mut(w).actors[slot as usize];
    a.x = root.0 + dx;
    a.y = root.1 + dy;
}

/// The elevators' DustVFXCollision BeginOverlap, tested in the leftover pass (`mod.rs` tick), after the moves of the
/// frame: a fighter whose capsule overlaps the box now and did not at the last test begins an overlap.
/// Deviation: UE fires BeginOverlap inside the move that makes it (the fighter's CMC, or the root's MoveComponentTo
/// update); the port tests once a frame behind the moves, so a Delay(1.5) started here counts from the next frame. Any
/// Pawn begins it: the fighters alone are tested (a Pawn-type spawned actor is not).
pub fn elevator_dust(w: &mut World, slot: u32) {
    place_elevator(w, slot);
    // DustVFXCollision (root-relative (0, 0, 60), scale (15, 8, 1), BoxExtent 32: H (480, 256, 32) about the root, its
    // depth holding the play plane), moved with the root
    let data = Arc::clone(&w.data);
    let (index, ex, ey) = {
        let a = &run(w).actors[slot as usize];
        (a.index, a.x, a.y)
    };
    let Some(g) = dust_of(w, placed_data(&data, index)) else { return };
    let (cx, cy) = (ex + g.dx, ey + g.dy);
    for f in w.all_ids() {
        let fi = w.fighter(f);
        let id = fi.id;
        let over = if fi.life.dead <= 0.0 && !crate::fighter::no_collision(w, f) {
            let r = crate::fighter::cap_radius(w, f);
            let half = crate::fighter::capsule_half(w, f) - r;
            let ay = crate::fighter::actor_y(w, f);
            let dx = max((fi.body.x - cx).abs() - g.hw, 0.0);
            let dy = max(max(cy - g.hh - (ay + half), ay - half - (cy + g.hh)), 0.0);
            dx * dx + dy * dy < r * r
        } else {
            false
        };
        let tick = w.flow.tick;
        let e = elevator_mut(w, slot);
        let k = e.dust_in.iter().position(|&x| x == id);
        let was = k.is_some();
        if over && !was {
            e.dust_in.push(id);
        } else if let (false, Some(k)) = (over, k) {
            e.dust_in.remove(k);
        }
        if !over || was || e.dust_gate {
            continue;
        }
        // @42-300: PS_Mansion_Dustfall and platform_creak at the root, DustGate true, Delay(1.5)
        e.dust_at = tick;
        e.dust_x = ex;
        e.dust_y = ey;
        e.dust_gate = true;
        pooled(w, crate::name!("sfx_map_scoobydoo_platform_creak"), ex, ey);
        stage_delay(w, i32::try_from(index).expect("a placed index"), DUST_DELAY, 1.5, DUST_LINK);
    }
}

/// The Delay(1.5)'s link @15.
pub fn elevator_dust_link(w: &mut World, slot: u32) {
    elevator_mut(w, slot).dust_gate = false;
}

// ---- ScoobyElevatorSwitch_C

/// ScoobyElevatorSwitch_C's entry.
pub fn switch_entry() -> StageActorEntry {
    StageActorEntry {
        create: Some(switch_create),
        // ScoobyElevatorHitbox while it generates overlaps
        hurt: Some(switch_hurt),
        hurt_type: Some(i32::from(CH_DAMAGE)),
        // Unverified: no PhysMaterialOverride on ScoobyElevatorHitbox: SurfaceType 6 for the light hit sound.
        surface: Some(6),
        // ReceiveAnyDamage @572: Damage > 0 -> ScoobyElevatorSwitchFlipped
        any_damage: Some(switch_any_damage),
        ..base(StageActorClass::ScoobyElevatorSwitch)
    }
}

fn switch_create(w: &mut World, slot: u32) {
    let data = Arc::clone(&w.data);
    let d = placed_data(&data, run(w).actors[slot as usize].index);
    let flag = |k: &str| d.p.get(k).and_then(Value::as_bool);
    let a = ScoobySwitch {
        state: flag("ScoobyElevatorSwitchState").unwrap_or(true),
        enabled: flag("ElevatorEnabled").unwrap_or(true),
        overlaps: true,
        elevator: -1,
        wall: -1,
        move_at: -1,
        move_up: false,
        chain1_at: -1,
        done_at: -1,
    };
    hurt_boxes(w, d, "ScoobyElevatorHitbox");
    place_actor(w, slot, StageActorOwn::ScoobySwitch(a));
}

fn switch_hurt(w: &World, slot: u32, out: &mut Vec<StageHurt>) -> bool {
    let a = &run(w).actors[slot as usize];
    if a.dead || !switch(w, slot).overlaps {
        return false;
    }
    out.extend(hurt_boxes(w, placed(w, a.index), "ScoobyElevatorHitbox"));
    true
}

fn switch_any_damage(w: &mut World, slot: u32, dmg: Real, _causer: ActorRef) {
    if dmg > 0.0 {
        switch_flipped(w, slot);
    }
}

/// ScoobyElevatorSwitchFlipped @631-768.
fn switch_flipped(w: &mut World, slot: u32) {
    let s = switch_mut(w, slot);
    if !s.enabled {
        return;
    }
    s.enabled = false;
    s.overlaps = false;
    s.state = !s.state;
    let up = s.state;
    move_multicast(w, slot, up);
}

/// MoveScoobyElevatorMulticast(Up) @769-1719.
fn move_multicast(w: &mut World, slot: u32, up: bool) {
    let tick = w.flow.tick;
    let data = Arc::clone(&w.data);
    let (index, sx, sy) = {
        let a = &run(w).actors[slot as usize];
        (a.index, a.x, a.y)
    };
    let (el, wall) = {
        let s = switch_mut(w, slot);
        // the switch's Release / Retract montage
        s.move_at = tick;
        s.move_up = up;
        (s.elevator, s.wall)
    };
    // Platform_Audio_Start @1720
    pooled(w, crate::name!("sfx_map_scoobydoo_platform_switch"), sx, sy);
    let Some(es) = slot_of(w, el) else {
        warn_text(w, "ScoobyElevatorSwitch: no ConnectedScoobyElevator in the stage data (its move not made)");
        return;
    };
    place_elevator(w, es);
    let (eindex, ex, ey) = {
        let a = &run(w).actors[es as usize];
        (a.index, a.x, a.y)
    };
    w.events.push(Event::Sound {
        fighter: -1,
        ev: crate::name!("sfx_map_scoobydoo_platform_move"),
        x: Some(ex),
        y: Some(ey),
        hit: None,
        pooled: false,
        key: Some(elevator_key(eindex)),
    });
    {
        let e = elevator_mut(w, es);
        e.edge_update = true;
        e.emissive = 1.0;
        // the trapdoor's MoveDownAnim / MoveUpAnim
        e.move_at = tick;
        e.move_up = up;
    }
    if let Some(ws) = slot_of(w, wall)
        && !run(w).actors[ws as usize].dead
    {
        // PlayScoobyWallGearAnim(Up)
        let g = wall_mut(w, ws);
        g.gear_at = tick;
        g.gear_up = up;
    }
    // PlayWallChainVisuals @2105: particle 0 and the wall chain montage now (move_at), particle 1 at the Delay's link
    let owner = i32::try_from(index).expect("a placed index");
    stage_delay(w, owner, CHAIN_DELAY, 0.35, CHAIN_LINK);
    // PlayTrapChainAnims @2394: the trap chains' montages now (move_at)
    // MoveComponentTo(DefaultSceneRoot, (X, Y, Z + 2850 x SelectFloat(1, -1, Up)), 1.65 s, linear, Move) @1265-1597
    let ed = placed_data(&data, eindex);
    let floor = floor_of(w, ed);
    let Some(sdy) = usize::try_from(floor).ok().and_then(|i| run(w).solids.get(i)).map(|s| s.m.dy) else { return };
    let sd = placed_data(&data, index);
    let travel = |k: &str| sd.p.get(k).and_then(Value::as_f64).unwrap_or(f64::from(Real::NAN));
    let (dist, time) = (travel("ElevatorTravelDistance"), travel("ElevatorTravelTime"));
    let rz = root_z(ed);
    let sdx = run(w).solids[usize::try_from(floor).expect("a solid id")].m.dx;
    let z = (rz + sdy) + crate::math::narrow(dist * if up { 1.0 } else { -1.0 });
    let piece = StagePiece::Solid(u32::try_from(floor).expect("a solid id"));
    stage_move_to(w, piece, sdx, z - rz, crate::math::narrow(time), owner, MOVE_LINK, 0, false, false);
}

/// The MoveComponentTo's link @15: Platform_Audio_Stop @1977 (platform_stop on the elevator, Delay(0) @2050), the end
/// pose, gear Emissive 0, ElevatorEnabled and the hitbox's overlaps (authority @206-247).
pub fn switch_move_done(w: &mut World, slot: u32) {
    let tick = w.flow.tick;
    let (index, el) = (run(w).actors[slot as usize].index, switch(w, slot).elevator);
    let es = slot_of(w, el);
    if let Some(es) = es {
        place_elevator(w, es);
        let eindex = run(w).actors[es as usize].index;
        w.events.push(Event::SoundStop {
            fighter: -1,
            key: elevator_key(eindex),
            fade: None,
            ev: Some(crate::name!("sfx_map_scoobydoo_platform_stop")),
            curve: None,
            cut: false,
        });
    }
    stage_delay(w, i32::try_from(index).expect("a placed index"), STOP_DELAY, 0.0, STOP_LINK);
    let s = switch_mut(w, slot);
    s.done_at = tick;
    s.enabled = true;
    s.overlaps = true;
    if let Some(es) = es {
        elevator_mut(w, es).emissive = 0.0;
    }
}

/// The Delay(0)'s link @516: the elevator's Comp_EdgeTrace UpdateIntersectionsEveryFrame false.
pub fn switch_stop_link(w: &mut World, slot: u32) {
    if let Some(es) = slot_of(w, switch(w, slot).elevator) {
        elevator_mut(w, es).edge_update = false;
    }
}

/// The Delay(0.35)'s link @281: the wall chain's particle 1 at WallChainParticle1Transform.
pub fn switch_chain_link(w: &mut World, slot: u32) {
    let tick = w.flow.tick;
    switch_mut(w, slot).chain1_at = tick;
}
