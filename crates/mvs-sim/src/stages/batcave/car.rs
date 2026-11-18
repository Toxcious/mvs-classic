//! Batcave's two Batmobiles (BatcaveWallDestructible_C, a DestructibleObjectBP_C child with its own overrides beside
//! WallBP's: `world::wall_bp` holds the shared HP / stage part). Offsets are the class's
//! ubergraph's unless named.
//! - BeginPlay @4945 -> @4253: InitStage(0) (Stages [1, 20] on both placed cars: HP 1).
//! - ReceiveAnyDamage @4273 -> @215: HP - Damage, BPI_DamageVisuals @82 (DestructibleObjectBP's multicast: this class's
//!   DamageVisuals @7001 -> @5780, no parent call, so no Ak_Damage_Environment post), then HP <= 0: InitStage(CurrentStage plus 1) @191 (the class's InitStage: CurrentStage >= Stages.length or DestroyImmediately, false on both cars:
//!   BPI_Destroy -> DestroyDestructible). The second stage's 20 HP replaces the remainder: two hits break a car at
//!   least (the second of 20 or more).
//! - DamageVisuals @5780: IsDead: nothing. Else DamageMontage played @2958 (`hit_at` / montage), then with authority
//!   @3382 (the sim is): 0 < HP <= 20 and !AlmostDeadVFXOn: AlmostDeadVFXOn = true, SetTopLights @3610 (@6788: the four
//!   PS_BatmobileHit_PlatformLights at VFX_Light01..04, then a DoOnce @5565 -> @5627 Env_Audio_Emit ManualStart: the
//!   alarm), then K2_SetTimerDelegate(SetTopLights, 2.9, looping) @2850 and (SetFrontLights, 0.4, looping) @2746
//!   (SetFrontLights @6157: the two PS_BatmobileHit_Lights at VFX_CarLightR / L; Array_Clear(Front Lights), then
//!   Array_Add of Temp_object_Variable_1 / _0 @5891 / @6056, temps no statement writes, not the spawns' return values:
//!   the Front / Top Lights arrays hold nulls). The non-authority branch @3629 (sfx_map_batcave_batmobile_damage_electric
//!   at the causer) is a client's: not posted.
//! - DestroyDestructible @6991 -> @303: Destroyed = true, CollisionBox, SkeletalMesh, DamageBox and Box1
//!   SetCollisionEnabled(NoCollision) at once @353-467 (no 0.1 s Delay as WallBP's); IsDead: nothing more. Else
//!   Comp_EdgeTrace.On Destroyed @15, Delay(1.5) @535 -> @590 PS_BatcaveWaterfallForeGround_Splashing.Deactivate (the
//!   drawing's: `splash_off_at`), DestroyMontage played @627 (its OnCompleted @2516: Color 0.075 @2543), Color 0.55
//!   @1051, IsDead = true @1109, the alarm's AkComponent.Stop @1207, the loop over Front Lights (@1261-2020: nulls, so
//!   it destroys nothing), both light timers cleared @2021-2041 (the Top Lights emitters are left as they are).
//! - ResetDestructible (BPI_Reset, DestructibleObjectBP @421) @6996 -> @3832: RespawnMontage played; only its
//!   OnCompleted @2714 -> @2210 restores: Destroyed false, the four components QueryAndPhysics, InitStage(0). IsDead
//!   and AlmostDeadVFXOn are never cleared, so a car broken once makes no damage montage, no lights and no second
//!   break montage. The level script's ResetLevelObjects calls it (Map_Batcave @2454-2645: right, then left), only at
//!   PandaGameMode StartNewMatch (`index`).
//! - Box1 overlaps (the AnimBP's HasWieght and two sounds on the Ak_JumpLand component): begin @4950: a
//!   BP_FighterCharacter not crouching (CMC IsCrouching) is AddUnique'd to OverlappingFighters; newly added:
//!   sfx_map_batcave_batmobile_weight_add_land @5327, HasWieght true @5517. End @4278: a fighter whose
//!   BPI_WasCrouchingPreviousFrame is false (BaseFighter @13504: the capsule's half height now > PF_CapsuleHalfHeight,
//!   which the pawn's tick saves while walking @33775): with one fighter in the list, HasWieght false and
//!   sfx_map_batcave_batmobile_weight_lift_jump @4794; then that fighter is removed @4868. One that ends its overlap
//!   while "crouching previous frame" stays in the list (as the build).
//! - CollisionBox OnComponentHit @3831: nothing.
//!
//! Deviation: the stage bind writes no actorMontages (StageData.actorMontages), so the three montages are not ActorAnim
//! plays: their completions are counted here in ticks (the montage lengths, rate 1, from 0: Retract 3 s, Hit
//! 0.8333333 s, Spawn 2.6666667 s) in the stage's leftover pass, not the mesh's TG_PrePhysics anim tick; a new play on
//! the mesh interrupts the one before (no OnCompleted). The Retract montage's AnimNotify_TrackByCamera finds no
//! Comp_Actor_KeepOnCamera on the car: nothing.

use super::own;
use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::boxes::CH_DAMAGE;
use crate::events::{Event, SoundKey};
use crate::fighter::common::warn_text;
use crate::fighter::traces::edges::edges_removed;
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, no_collision};
use crate::math::max;
use crate::math::ue::size2d;
use crate::name;
use crate::names::Name;
use crate::registry::entries::StageActorEntry;
use crate::world::World;
use crate::world::pawns::{all_list, put_fighters};
use crate::world::stage::{stage_clear_timer, stage_delay, stage_set_timer};
use crate::world::stage_actors::{StageActorOwn, StageHurt};
use crate::world::wall_bp::{
    WallActor, WallPart, init_stage, ref_name, wall, wall_any_damage, wall_create, wall_hurt_shapes, wall_mut,
    wall_solids_enable,
};

/// This file's event numbers (`StageEntry::timer` `ev`) are `CAR_EV` and up, below `BATS_EV` (`bats`).
pub const CAR_EV: i32 = 200;
const EV_TOP: i32 = CAR_EV + 1; // Top Lights Timer (SetTopLights, 2.9 s looping)
const EV_FRONT: i32 = CAR_EV + 2; // Front Lights Timer (SetFrontLights, 0.4 s looping)
const EV_SPLASH: i32 = CAR_EV + 3; // the Delay(1.5) @535 -> @590
// The keys: the two timer handles and the Delay node.
const KEY_TOP: i32 = 1;
const KEY_FRONT: i32 = 2;
const KEY_SPLASH: i32 = 3;

const SFX_ALARM: Name = name!("sfx_map_batcave_batmobile_alarm"); // the Env_Audio_Emit child actor's AkEvent (Map_Batcave)
const SFX_ADD: Name = name!("sfx_map_batcave_batmobile_weight_add_land");
const SFX_LIFT: Name = name!("sfx_map_batcave_batmobile_weight_lift_jump");

/// The montages' lengths in ticks (SequenceLength x 60, rounded up; see the Deviation above); `None`: no length here.
fn frames(montage: &str) -> Option<i32> {
    match montage {
        "BatMobile_Retract_Montage" => Some(180),
        "BatMobile_Hit_Montage" => Some(50),
        "BatMobile_Spawn_Montage" => Some(160),
        _ => None,
    }
}

/// The places the sounds are posted at, sim (static data: the record leaves it out).
#[derive(Clone, Debug, Default)]
struct CarGeo {
    /// The Env_Audio_Emit child actor.
    alarm: [Real; 2],
    /// Ak_JumpLand (on the SkeletalMesh, no relative place: the mesh's origin).
    jump: [Real; 2],
    /// Box1, sim: x, y, half x, half y.
    box1: Option<[Real; 4]>,
}

/// `BatcaveCar`.
#[derive(State, Debug, Default)]
pub struct BatcaveCar {
    /// Its `WallActor` part.
    pub wall: WallPart,
    /// IsDead: set by the first DestroyDestructible, never cleared.
    pub is_dead: bool,
    /// AlmostDeadVFXOn: never cleared.
    pub almost_dead: bool,
    /// SetTopLights' DoOnce closed (the alarm's ManualStart made).
    pub alarm_once: bool,
    /// The tick AlmostDeadVFXOn was set (the first SetTopLights), -1.
    pub lights_at: i32,
    /// The tick of the last SetTopLights (the four platform lights spawned again), -1.
    pub top_at: i32,
    /// The tick of the last SetFrontLights (the two car lights spawned again), -1.
    pub front_at: i32,
    /// The loop over Front Lights ran (@1261-2020), till the next SetFrontLights: the array holds nulls (the temps
    /// above), so nothing is destroyed; read by nothing (the drawing lets the lights finish).
    pub front_off: bool,
    /// The tick the Delay(1.5) link deactivated the waterfall splash, -1.
    pub splash_off_at: i32,
    /// The Color vector parameter last set on the mesh's materials (0.55, 0.075), -1 the material's own.
    pub color: Real,
    /// The montage on the SkeletalMesh now ("" none): Retract, Hit or Spawn.
    pub montage: String,
    /// The tick it was played, -1.
    pub montage_at: i32,
    /// It ran to its end (OnCompleted made).
    pub montage_done: bool,
    /// OverlappingFighters: the fighters' ids, in add order.
    pub riders: Vec<i32>,
    /// BatMobile_AnimBlueprint HasWieght.
    pub weight: bool,
    /// The fighters whose root capsule overlaps Box1 now (the overlap state), ids in begin order.
    pub in_box1: Vec<i32>,
    /// RespawnMontage.
    pub respawn_montage: String,
    /// Comp_EdgeTrace.Enabled: false once On Destroyed has run (its polygon gone).
    pub edges: bool,
    #[state(skip)]
    geo: CarGeo,
}

// The two placed cars' child places, UE world, from Map_Batcave: Box (the root) at the actor's place, yaw 90, scale
// (+-1.2, 1.2, 1.2); SkeletalMesh (class template) at (-1297, 354, -2122) yaw 180 under it; the instance's
// Env_Audio_Emit RelativeLocation under the mesh: left (-900.1071, 445.86853, 2122), right (-925.52374, 425.96194,
// 2122). Composed: the mesh at UE Y +-773.6, Z -1813.4; the emitter at UE Y 1853.73 (left), -1884.23 (right), Z 733.
fn place_of(name: &str) -> Option<(Real, Real)> {
    match name {
        "BatcaveWallDestructible_left" => Some((1853.73, 773.6)),
        "BatcaveWallDestructible_right" => Some((-1884.23, -773.6)),
        _ => None,
    }
}
const MESH_Z: Real = -1813.4;
const ALARM_Z: Real = 733.0;

fn car_ref(w: &World, a: WallActor) -> &BatcaveCar {
    match &w.stage_run.as_ref().expect("a stage run").actors[a.0 as usize].own {
        StageActorOwn::BatcaveCar(c) => c,
        o => panic!("stage actor {} is not a BatcaveCar: {o:?}", a.0),
    }
}

fn car_mut(w: &mut World, a: WallActor) -> &mut BatcaveCar {
    match &mut w.stage_run.as_mut().expect("a stage run").actors[a.0 as usize].own {
        StageActorOwn::BatcaveCar(c) => c,
        o => panic!("stage actor {} is not a BatcaveCar: {o:?}", a.0),
    }
}

fn index_of(w: &World, a: WallActor) -> i32 {
    i32::try_from(w.stage_run.as_ref().expect("a stage run").actors[a.0 as usize].index).expect("an index")
}

fn alarm_key(index: i32) -> SoundKey {
    SoundKey { tag: name!("batcaveCarAlarm"), n: index }
}

fn jump_key(index: i32) -> SoundKey {
    SoundKey { tag: name!("batcaveCarJump"), n: index }
}

/// The fighter's root capsule against an axis-aligned box on the play plane (as `chars::bugs::safe`).
fn capsule_in_box(w: &World, v: FighterId, b: [Real; 4]) -> bool {
    let r = cap_radius(w, v);
    let half = capsule_half(w, v) - r;
    let (vx, ay) = (w.fighter(v).body.x, actor_y(w, v));
    let dx = max(max(b[0] - b[2] - vx, 0.0), vx - (b[0] + b[2]));
    let dy = max(max(b[1] - b[3] - (ay + half), 0.0), ay - half - (b[1] + b[3]));
    size2d([dx, dy]) < r
}

fn play(w: &mut World, a: WallActor, montage: &str) {
    if frames(montage).is_none() {
        let cls = w.stage_run.as_ref().expect("a stage run").actors[a.0 as usize].cls.name();
        warn_text(
            w,
            &format!(
                "{cls}: montage {} has no length here: it never completes",
                if montage.is_empty() { "(none)" } else { montage }
            ),
        );
    }
    let tick = w.flow.tick;
    let c = car_mut(w, a);
    c.montage = montage.to_owned();
    c.montage_at = tick;
    c.montage_done = false;
}

/// DamageVisuals @5780 (authority).
fn car_visuals(w: &mut World, a: WallActor) {
    if car_ref(w, a).is_dead {
        return;
    }
    let m = wall(w, a).damage_montage.clone();
    play(w, a, &m);
    let tick = w.flow.tick;
    let p = wall_mut(w, a);
    p.hit_at = tick;
    p.hit_montage = m;
    let hp = p.hp;
    if !(hp <= 20.0 && !car_ref(w, a).almost_dead && hp > 0.0) {
        return;
    }
    let c = car_mut(w, a);
    c.almost_dead = true;
    c.lights_at = tick;
    set_top_lights(w, a);
    let index = index_of(w, a);
    stage_set_timer(w, index, KEY_TOP, 2.9, true, EV_TOP);
    stage_set_timer(w, index, KEY_FRONT, 0.4, true, EV_FRONT);
}

/// SetTopLights @6788: the platform lights (the drawing's: `top_at`), the alarm's DoOnce.
fn set_top_lights(w: &mut World, a: WallActor) {
    let tick = w.flow.tick;
    let c = car_mut(w, a);
    c.top_at = tick;
    if c.alarm_once {
        return;
    }
    c.alarm_once = true;
    let at = c.geo.alarm;
    let index = index_of(w, a);
    w.events.push(Event::Sound {
        fighter: -1,
        ev: SFX_ALARM,
        x: Some(at[0]),
        y: Some(at[1]),
        hit: None,
        pooled: false,
        key: Some(alarm_key(index)),
    });
}

/// DestroyDestructible @303.
fn car_destroy(w: &mut World, a: WallActor) {
    let tick = w.flow.tick;
    let p = wall_mut(w, a);
    p.destroyed = true;
    p.broken_at = tick;
    p.off_at = tick;
    set_collision(w, a, false);
    if car_ref(w, a).is_dead {
        return;
    }
    car_mut(w, a).edges = false;
    let index = index_of(w, a);
    edges_removed(w, u32::try_from(index).expect("a placed index"));
    stage_delay(w, index, KEY_SPLASH, 1.5, EV_SPLASH);
    let m = wall(w, a).destroy_montage.clone();
    play(w, a, &m);
    let c = car_mut(w, a);
    c.color = 0.55;
    c.is_dead = true;
    w.events.push(Event::SoundStop {
        fighter: -1,
        key: alarm_key(index),
        fade: None,
        ev: None,
        curve: None,
        cut: false,
    });
    car_mut(w, a).front_off = true;
    stage_clear_timer(w, index, KEY_FRONT);
    stage_clear_timer(w, index, KEY_TOP);
}

/// The four components' SetCollisionEnabled: the solids, and Box1's overlaps updated at once (end / begin events).
fn set_collision(w: &mut World, a: WallActor, on: bool) {
    wall_solids_enable(w, a, on);
    box1_overlaps(w, a, on);
}

/// ResetDestructible @3832 (BPI_Reset).
pub fn car_reset(w: &mut World, a: WallActor) {
    let m = car_ref(w, a).respawn_montage.clone();
    play(w, a, &m);
}

/// The montage's OnCompleted: Retract @2516 (Color 0.075), Spawn @2714 -> @2210 (the restore); Hit: nothing.
fn completed(w: &mut World, a: WallActor) {
    car_mut(w, a).montage_done = true;
    let (destroy, respawn) = (wall(w, a).destroy_montage.clone(), car_ref(w, a).respawn_montage.clone());
    let cur = car_ref(w, a).montage.clone();
    if cur == destroy {
        car_mut(w, a).color = 0.075;
    } else if cur == respawn {
        let p = wall_mut(w, a);
        p.destroyed = false;
        p.off_at = -1;
        set_collision(w, a, true);
        init_stage(w, a, 0, car_destroy);
    }
}

/// Box1's overlap state against the fighters' root capsules (off: every overlap ends).
// Deviation: tested once a frame in the stage's leftover pass and at the collision switches, not along each
// fighter's move (UpdateOverlaps); a fighter with its collision off or dead (its pawn gone) overlaps nothing.
fn box1_overlaps(w: &mut World, a: WallActor, on: bool) {
    let b = car_ref(w, a).geo.box1;
    let all = all_list(w);
    for &f in &all {
        let now =
            on && b.is_some_and(|b| w.fighter(f).life.dead <= 0.0 && !no_collision(w, f) && capsule_in_box(w, f, b));
        let id = w.fighter(f).id;
        let i = car_ref(w, a).in_box1.iter().position(|&v| v == id);
        if now && i.is_none() {
            car_mut(w, a).in_box1.push(id);
            begin_overlap(w, a, f);
        } else if !now && let Some(i) = i {
            car_mut(w, a).in_box1.remove(i);
            end_overlap(w, a, f);
        }
    }
    put_fighters(w, all);
}

/// @4950: not crouching: AddUnique, the land sound and HasWieght for a new one.
fn begin_overlap(w: &mut World, a: WallActor, f: FighterId) {
    let id = w.fighter(f).id;
    if w.fighter(f).capsule.cmc_crouched || car_ref(w, a).riders.contains(&id) {
        return;
    }
    car_mut(w, a).riders.push(id);
    jump_sound(w, a, SFX_ADD);
    car_mut(w, a).weight = true;
}

/// @4278: BPI_WasCrouchingPreviousFrame false: the lift sound with one fighter in the list, then RemoveItem.
fn end_overlap(w: &mut World, a: WallActor, f: FighterId) {
    if was_crouching(w, f) {
        return;
    }
    if car_ref(w, a).riders.len() == 1 {
        car_mut(w, a).weight = false;
        jump_sound(w, a, SFX_LIFT);
    }
    let id = w.fighter(f).id;
    let c = car_mut(w, a);
    if let Some(i) = c.riders.iter().position(|&x| x == id) {
        c.riders.remove(i);
    }
}

/// PF_Post_Event_On_AkComponent_WithInstigator(Ak_JumpLand, event, the fighter): the car's own component.
// Deviation: the Instigator (cha_locally_controlled, the skin switch Lib_Audio reads from it) is not carried.
fn jump_sound(w: &mut World, a: WallActor, ev: Name) {
    let at = car_ref(w, a).geo.jump;
    let index = index_of(w, a);
    w.events.push(Event::Sound {
        fighter: -1,
        ev,
        x: Some(at[0]),
        y: Some(at[1]),
        hit: None,
        pooled: false,
        key: Some(jump_key(index)),
    });
}

/// PF_CapsuleHalfHeight per fighter id (BaseFighter @33775: saved by the pawn's tick while walking): the stage state's
/// (`BatcaveOwn::pf`), on every path an overlap ends (the frame's pass, a break's collision switch). A minion that has
/// not walked yet has no entry: PF_CapsuleHalfHeight starts at 0 (its only writer is the walking tick).
fn was_crouching(w: &World, f: FighterId) -> bool {
    capsule_half(w, f) > own(w).pf.get(&w.fighter(f).id).copied().unwrap_or(0.0)
}

/// A car's part of the stage's leftover pass (`index`): its montage's end, then Box1's overlaps; the end test reads the
/// fighters' PF_CapsuleHalfHeight as the frame before left it (`BatcaveOwn::pf`).
// Deviation: the pawn saves it in its own tick of this frame, ahead of its move; the port reads last frame's (the end
// overlap's test is then "crouching when last seen walking a frame ago").
pub fn car_frame(w: &mut World, a: WallActor) {
    let c = car_ref(w, a);
    let n = frames(&c.montage);
    let due = c.montage_at >= 0 && !c.montage_done && n.is_some_and(|n| w.flow.tick - c.montage_at >= n);
    if due {
        completed(w, a);
    }
    if !wall(w, a).destroyed {
        box1_overlaps(w, a, true);
    }
}

/// `carKind.create`.
pub fn car_create(w: &mut World, slot: u32) {
    let data = std::sync::Arc::clone(&w.data);
    let index = w.stage_run.as_ref().expect("a stage run").actors[slot as usize].index;
    let d = &data.stage.placed.as_ref().expect("placed actors")[index as usize];
    let wall = wall_create(w, WallActor(slot), [0.0; 3]);
    let respawn = ref_name(d.p.get("RespawnMontage")).to_owned();
    if d.p.get("RespawnMontage").is_none() {
        warn_text(w, &format!("{} {}: no RespawnMontage", d.cls, d.name));
    }
    let pl = place_of(&d.name);
    if pl.is_none() {
        warn_text(
            w,
            &format!("{} {}: not one of Map_Batcave's two cars (its sound places are unknown)", d.cls, d.name),
        );
    }
    let solids = data.stage.solids.as_deref().unwrap_or(&[]);
    let s = d
        .solids
        .iter()
        .flatten()
        .filter_map(|&id| usize::try_from(id).ok().and_then(|i| solids.get(i)))
        .find(|x| x.name.split('.').nth(1) == Some("Box1"));
    if s.is_none() {
        warn_text(w, &format!("{} {}: no Box1 solid in the stage data", d.cls, d.name));
    }
    let loc1 = d.loc.get(1).copied().unwrap_or(0.0);
    let geo = CarGeo {
        alarm: [-pl.map_or(crate::math::narrow(loc1), |p| p.0), ALARM_Z],
        jump: [-pl.map_or(crate::math::narrow(loc1), |p| p.1), MESH_Z],
        box1: s.map(|s| [-s.center[1], s.center[2], s.half_extent[1], s.half_extent[2]].map(crate::math::narrow)),
    };
    w.stage_run.as_mut().expect("a stage run").actors[slot as usize].own = StageActorOwn::BatcaveCar(BatcaveCar {
        wall,
        lights_at: -1,
        top_at: -1,
        front_at: -1,
        splash_off_at: -1,
        color: -1.0,
        montage_at: -1,
        respawn_montage: respawn,
        edges: true,
        geo,
        ..BatcaveCar::default()
    });
}

/// The DamageBox while its collision is on (off from DestroyDestructible to the Spawn montage's end).
fn car_hurt(w: &World, slot: u32, out: &mut Vec<StageHurt>) -> bool {
    if wall(w, WallActor(slot)).destroyed {
        return false;
    }
    out.extend_from_slice(wall_hurt_shapes(w, WallActor(slot)));
    true
}

fn car_any_damage(w: &mut World, slot: u32, dmg: Real, _causer: ActorRef) {
    wall_any_damage(w, WallActor(slot), dmg, car_destroy, Some(car_visuals));
}

/// `carKind`'s entry.
pub fn car_entry(class: crate::actors::StageActorClass) -> StageActorEntry {
    StageActorEntry {
        class,
        // the DamageBox while its collision is on
        hurt_type: Some(i32::from(CH_DAMAGE)),
        friendly_fire: false,
        can_take_damage: true,
        // Comp_Pawn_DamageHandler HitPauseMultiplier 0.5 on both placed cars (Map_Batcave); no Comp_Actor_Damage /
        // Comp_HitPause: no victim pause, no tier; Comp_Actor_Destructible FriendlyFire: the class default (false)
        hit_pause_multiplier: Some(0.5),
        // Unverified: no PhysMaterialOverride on the DamageBox (class or instances): the light hit sound's SurfaceType 6
        surface: Some(6),
        create: Some(car_create),
        any_damage: Some(car_any_damage),
        hit_by_enemy: None,
        hit_by_ally: None,
        knocked_back: None,
        hurt: Some(car_hurt),
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

/// A due Delay or timer of a car (`ev` `CAR_EV` and up).
pub fn car_timer(w: &mut World, a: WallActor, ev: i32) {
    let tick = w.flow.tick;
    match ev {
        EV_TOP => set_top_lights(w, a),
        EV_FRONT => {
            let c = car_mut(w, a);
            c.front_at = tick;
            c.front_off = false;
        }
        EV_SPLASH => car_mut(w, a).splash_off_at = tick,
        _ => {}
    }
}
