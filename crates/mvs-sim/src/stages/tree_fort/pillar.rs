//! Tree Fort's destructible bounce trees: TreehouseSidePillar_C and its child
//! TreehouseSidePillar_Child_C (no function of its own, no component override: TreehouseSidePillar_Child), one kind for
//! both. Offsets are the ubergraph's or the function's.
//! - BeginPlay @4881 -> @5416 (offline: not a dedicated server): AllowHazards false destroys it (@5888); else @5110
//!   Starting HP = TreePillarHP and the Bones list (@5388 -> @1008). The stage's create does the test (`super::create`).
//! - ReceiveAnyDamage @6787: nothing while IsDying; PlayHitAnimation (hitAt), TreehouseSidePillarDamage(Damage), then
//!   sfx_map_treehouse_pillar_hit at the DamageHitBox. TreehouseSidePillarDamage: HP - Damage, TreeFortPillarDamageVFX
//!   (damageAt), the damage stage (0 over 2/3 of Starting HP, 1 over 1/3, else 2: @87-344); a new stage is
//!   UpdateDamageMesh (sfx_map_treehouse_pillar_damage, the hit montage again); HP <= 0: DeathVisuals(true).
//! - DeathVisuals @3679 (a DoOnce): IsDying, the five collisions off at once (@4174-4326),
//!   sfx_map_treehouse_pillar_destroy, the death montage and a Delay of its length (Montage_Play's plain return,
//!   1.6666666 s: 100 frames) to K2_DestroyActor (@15).
//! - ReceiveTick @5942: the standing bounce of every BP_FighterCharacter whose MovementBase's owner is this tree (@2502
//!   -> @1153: TreeFortTrampolineBounce with an empty hit), then HandleActorBounceLimitTime, then the wiggles (@4830 ->
//!   @2553).
//! - The hits (`world::stage::stage_solid_hit_by`: a fighter's movement, a projectile's or an item's move):
//!   TreePillarTopCollision (the round top, a polygon here) -> TreeFortTrampolineBounce, TreePillarCollision (the box)
//!   -> TreeFortPillarSideBounce; a fighter takes the BP_FighterCharacter branch, a projectile or an item the
//!   BPI_SpawnedActor one.
//!
//! The fields the drawing reads: hp, damage_stage, is_right, dying_at, hit_at, bounce_at, wiggles; also damage_at (the
//! damage VFX: PS_Treefortbounce at TreePillarCollision, rotation yaw 90, scale 1.5) and bounce_fx (the bounce visual's
//! PS_Treefortbounce: place and pitch, MakeRotator(0, pitch, 90), scale 1).

use std::sync::Arc;

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, StageActorClass};
use crate::combat::boxes::{CH_DAMAGE, WorldSphere};
use crate::consts::{DELTA_SECONDS, GRAVITY_SCALE};
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::PlatformRef;
use crate::fighter::common::warn_text;
use crate::fighter::moves::Impulse;
use crate::fighter::moves::reset_jump_counts;
use crate::math::ue;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::StageActorEntry;
use crate::registry::props::{flag, flag_or, num};
use crate::world::World;
use crate::world::stage::stage_delay;
use crate::world::stage::stage_solid_enable;
use crate::world::stage_actors::{StageActorOwn, StageHurt};
use mvs_data::stage::StagePlacedData;

/// One record of WiggleInfo (JakeStretch_Struct): PointNumber (the index into the AnimBP's Bones the record drives),
/// WiggleDirection (unit, UE axes) and HitTime (GetGameTimeInSeconds, world seconds).
#[derive(State, Debug, Default)]
pub struct PillarWiggle {
    pub bone: i32,
    pub dx: Real,
    pub dy: Real,
    pub dz: Real,
    pub t: Real,
}

/// `bounceFx`: the bounce visual's emitter: sim place (z: UE X) and pitch (degrees).
#[derive(State, Debug, Default)]
pub struct BounceFx {
    pub x: Real,
    pub y: Real,
    pub z: Real,
    pub pitch: Real,
}

/// `TreePillar`: its own state, its `StageActorOwn` variant.
#[derive(State, Debug, Default)]
pub struct Pillar {
    /// TreePillarHP (float32, not clamped).
    pub hp: Real,
    /// Starting HP.
    pub start_hp: Real,
    /// Damage Stage: 0, 1, 2.
    pub damage_stage: i32,
    pub is_right: bool,
    /// TrampolineBounceDoDmg.
    pub do_dmg: bool,
    /// The world tick of DeathVisuals (IsDying), -1 alive.
    pub dying_at: i32,
    /// The tick of the last SideTree_Hit_AnimMontage play (PlayHitAnimation, UpdateDamageMesh), -1 none.
    pub hit_at: i32,
    /// The tick of the last TreeFortPillarDamageVFX, -1 none.
    pub damage_at: i32,
    /// The tick of the last bounce visual, -1 none.
    pub bounce_at: i32,
    pub bounce_fx: Option<BounceFx>,
    pub wiggles: Vec<PillarWiggle>,
    /// Side_Tree_AnimBp's Bones (8 vectors, UE axes, flat x y z per index) as the wiggle pass writes them (@2808,
    /// @3267): they persist where the removal quirk drops a live record, so the drawing reads them, not `wiggles`.
    /// The AnimBP's property copies take Bones[1..4] and [7] to its ModifyBone nodes: Bones[5] and [6] drive no bone.
    pub bones: Vec<Real>,
    /// BounceLimitActors (fighter ids) and BounceLimitTimers, index by index.
    pub limit_actors: Vec<i32>,
    pub limit_timers: Vec<Real>,
    /// The spawned actors of BounceLimitActors and their timers (the port keeps them apart from the fighters' ids,
    /// which the record holds; each list keeps the one array's order, so the finds and the removals are the same).
    pub limit_spawned: Vec<ActorRef>,
    pub limit_spawned_timers: Vec<Real>,
    /// StartingBounceLimitTime.
    pub limit_time: Real,
    /// What the actor's place gives (`GEO`): made at create from the data, never changed. Not state.
    #[state(skip)]
    geo: Arc<Geo>,
}

/// What the actor's place gives: its solids by role, its hurt shapes, the DamageHitBox's place (the sounds), the
/// mesh's bones.
#[derive(Debug, Default)]
struct Geo {
    /// [DamageHitBox, TreePillarCollision, TreePillarTopCollision] ids (`StagePlacedData.solids`).
    solids: Vec<i32>,
    /// The TreePillarCollision id (the side bounce), -1 none.
    boxed: i32,
    /// The TreePillarTopCollision id (the trampoline bounce), -1 none.
    top: i32,
    hurt: Vec<StageHurt>,
    /// DamageHitBox's centre, sim.
    hit_box: (Real, Real),
    /// The actor's location Y, UE.
    ue_y: Real,
    /// GetSocketLocation(Bones[i]) for i 0..6, UE (6: out of the list, None: the component's place).
    bones: Vec<[Real; 3]>,
    /// GetSocketLocation(Root), UE.
    root: [Real; 3],
}

// The class's component templates (TreehouseSidePillar _GEN_VARIABLE; no instance overrides them in the three
// levels): TreePillarDamageHitCapsule rel (-0.00036621094, 5, 274.8639), pitch 90, CapsuleRadius 240, HalfHeight 500
// (DamageDestructible); SkeletalMesh rel (70, 0, 199), scale 0.8.
const CAPSULE_REL: [Real; 3] = [-0.000_366_210_94, 5.0, 274.8639];
const CAPSULE_R: Real = 240.0;
const CAPSULE_HH: Real = 500.0;
const MESH_REL: [Real; 3] = [70.0, 0.0, 199.0];
const MESH_SCALE: Real = 0.8;
// L_Tree_Base_Skeleton's reference pose, component space (both meshes' skeleton; bone order Trunk, Root, Top, TopR,
// TopL, Bot_L_01, Bot_R_01, Wind, Wind1, Wind2). The Bones list (@1008: every bone but Trunk and the three Wind,
// AddUnique in bone order): Root, Top, TopR, TopL, Bot_L_01, Bot_R_01.
// Deviation: the skeleton's reference pose stands in for the mesh's own (the meshes' JSON has no RefSkeleton) and for
// the animated pose GetSocketLocation reads (the idle, the hit and death montages move the bones).
const BONES: [[Real; 3]; 6] = [
    [0.0, 0.0, 0.0],          // Root
    [0.0, 0.0, 364.017],      // Top
    [0.0, -259.009, 243.442], // TopR
    [0.0, 250.211, 243.442],  // TopL
    [0.0, 309.004, -26.762],  // Bot_L_01
    [0.0, -322.885, -33.515], // Bot_R_01
];
/// GetNumBones.
const NUM_BONES: usize = 10;
/// Default__Side_Tree_AnimBp_C Bones: 8 zero vectors.
const BONE_SLOTS: usize = 8;
/// The visual's loop (TreeFortTrampolineBounceVisual @51-1625): i from 1 to NumBones - 1, mesh bone i skipped when it
/// is Trunk or a Wind (bones 7, 8, 9), else Bones[i] (not mesh bone i) measured: i 1..6.
fn mesh_bone_skipped(i: usize) -> bool {
    i == 0 || i >= 7
}

/// LocalMinBounceSpeed.
const MIN_BOUNCE: Real = 1600.0;
/// LocalMaxBounceSpeed.
const MAX_BOUNCE: Real = 2500.0;
/// LocalBounceSpeedMod.
const BOUNCE_MOD: Real = 1.1;
/// LocalMaxBounceAngle.
const MAX_ANGLE: Real = 20.0;
/// TreeFortPillarSideBounce LocalBounceSpeedMod.
const SIDE_MOD: Real = 0.65;
/// TreehouseSidePillarDamage(6.0) @4038.
const SELF_DAMAGE: Real = 6.0;
/// BPI_SetBlockFastFallTime(0.2) @6404.
const BLOCK_FF_TIME: Real = 0.2;
/// Side_Tree_Death_*_Montage SequenceLength (Montage_Play's return, ReturnValueType 0).
const DEATH_SECONDS: Real = 1.666_666_6;
/// The Delay node's key and the event it runs (K2_DestroyActor).
pub const DEATH_DELAY: i32 = 1;

const SFX_HIT: Name = name!("sfx_map_treehouse_pillar_hit");
const SFX_DAMAGE: Name = name!("sfx_map_treehouse_pillar_damage");
const SFX_DESTROY: Name = name!("sfx_map_treehouse_pillar_destroy");
const SFX_BOUNCE: Name = name!("sfx_map_treehouse_trampoline_tree");
const RTPC_BOUNCE: Name = name!("map_treehouse_trampoline");
/// TreeFortPillarSideBounce's AudioBounce(0.7) @919.
const SIDE_BOUNCE_MAGNITUDE: Real = 0.7;

fn v3(v: &[f64]) -> [Real; 3] {
    [0, 1, 2].map(|i| crate::math::narrow(v.get(i).copied().unwrap_or(0.0)))
}

/// `geo`: what the actor's place gives.
fn make_geo(d: &StagePlacedData, w: &World) -> Geo {
    let (s, l) = (v3(&d.scale), v3(&d.loc));
    let at = |rel: [Real; 3], k: Real, b: [Real; 3]| -> [Real; 3] {
        [l[0] + s[0] * (rel[0] + k * b[0]), l[1] + s[1] * (rel[1] + k * b[1]), l[2] + s[2] * (rel[2] + k * b[2])]
    };
    let ids: Vec<i32> = d.solids.clone().unwrap_or_default();
    let solids = w.data.stage.solids.as_deref().unwrap_or(&[]);
    let by = |part: &str| -> i32 {
        let found = ids.iter().copied().find(|&i| {
            usize::try_from(i).ok().and_then(|i| solids.get(i)).is_some_and(|s| s.name.split('.').nth(1) == Some(part))
        });
        match found {
            Some(id) => id,
            None => {
                warn_text(w, &format!("{} {}: no {part} solid in the stage data", d.cls, d.name));
                -1
            }
        }
    };
    let (hb, boxed, top) = (by("TreePillarDamageHitBox"), by("TreePillarCollision"), by("TreePillarTopCollision"));
    let mut hurt = Vec::new();
    let hd = usize::try_from(hb).ok().and_then(|i| solids.get(i));
    if let Some(hd) = hd {
        let (c, e) = (v3(&hd.center), v3(&hd.half_extent));
        // the box in sim axes: x = -UE Y, y = UE Z, depth = UE X (no rotation: the levels place the trees unrotated)
        hurt.push(StageHurt::Box(WorldSphere {
            x: -c[1],
            y: c[2],
            z: Some(c[0]),
            r: 0.0,
            bx: Some([e[1], 0.0, 0.0, 0.0, e[2], 0.0, 0.0, 0.0, e[0]]),
            ..WorldSphere::default()
        }));
    }
    // the capsule pitched 90: its axis along X; GetScaledCapsuleRadius = R x min(|scale X|, |scale Y|), half height x
    // |scale Z|
    let cc = at(CAPSULE_REL, 1.0, [0.0; 3]);
    let r = CAPSULE_R * min(s[0].abs(), s[1].abs());
    let half = max(CAPSULE_HH * s[2].abs() - r, 0.0);
    hurt.push(StageHurt::Depth { x: -cc[1], y: cc[2], z0: cc[0] - half, z1: cc[0] + half, r });
    let mesh = at(MESH_REL, 1.0, [0.0; 3]);
    let bone = |b: [Real; 3]| at(MESH_REL, MESH_SCALE, b);
    let mut bones: Vec<[Real; 3]> = BONES.iter().map(|&b| bone(b)).collect();
    bones.push(mesh);
    Geo {
        solids: ids,
        boxed,
        top,
        hurt,
        hit_box: hd.map_or((-l[1], l[2]), |hd| (-v3(&hd.center)[1], v3(&hd.center)[2])),
        ue_y: l[1],
        bones,
        root: bone(BONES[0]),
    }
}

fn pillar(w: &World, slot: u32) -> &Pillar {
    match &w.stage_run.as_ref().expect("a stage run").actors[slot as usize].own {
        StageActorOwn::Pillar(p) => p,
        o => panic!("stage actor {slot} is no TreePillar: {o:?}"),
    }
}

fn pillar_mut(w: &mut World, slot: u32) -> &mut Pillar {
    match &mut w.stage_run.as_mut().expect("a stage run").actors[slot as usize].own {
        StageActorOwn::Pillar(p) => p,
        o => panic!("stage actor {slot} is no TreePillar: {o:?}"),
    }
}

fn geo(w: &World, slot: u32) -> Arc<Geo> {
    Arc::clone(&pillar(w, slot).geo)
}

/// The owner number of the stage actor `slot` (`a.index`).
fn owner(w: &World, slot: u32) -> i32 {
    crate::world::stage_actors::stage_actor_index(w, slot)
}

/// PF_Post_Event_At_Location with no Instigator: the game state's pooled AkComponent at the place.
fn sound(w: &mut World, ev: Name, at: (Real, Real)) {
    w.events.push(Event::Sound { fighter: -1, ev, x: Some(at.0), y: Some(at.1), hit: None, pooled: true, key: None });
}

/// GetGameTimeInSeconds (the world clock the fighters keep).
fn now(w: &World) -> Real {
    w.all_ids().first().map_or(0.0, |&f| w.fighter(f).clock.time)
}

/// K2_GetActorLocation (the play plane: X 0).
fn ue_loc(w: &World, f: FighterId) -> [Real; 3] {
    [0.0, -w.fighter(f).body.x, crate::fighter::actor_y(w, f)]
}

/// LaunchCharacter(v, true, true): PendingLaunchVelocity, taken in the fighter's next CMC tick (UE v: X dropped).
fn launch(w: &mut World, f: FighterId, v: [Real; 3]) {
    let m = &mut w.fighter_mut(f).moves;
    m.impulse = Some(Impulse { vx: -v[1], vy: v[2], xy: true, z: true, kind: 0, d: None });
    m.hit_launch = true; // a direct LaunchCharacter: the CMC takes it under the stun too
}

/// TreehouseSidePillarDamage(Damage).
fn damage(w: &mut World, slot: u32, dmg: Real) {
    let tick = w.flow.tick;
    let g = geo(w, slot);
    let a = pillar_mut(w, slot);
    a.hp -= dmg;
    a.damage_at = tick; // TreeFortPillarDamageVFX
    let third = a.start_hp / 3.0;
    let two = a.start_hp - third;
    let st = if a.hp > two {
        0
    } else if a.hp > (two - third) {
        1
    } else {
        2
    };
    let dead = a.hp <= 0.0;
    if st != a.damage_stage {
        a.damage_stage = st; // UpdateDamageMesh: the mesh and material of the stage, the sound, the hit montage
        a.hit_at = tick;
        sound(w, SFX_DAMAGE, g.hit_box);
    }
    if dead {
        death_visuals(w, slot, true);
    }
}

/// DeathVisuals(PlaySFX), once.
fn death_visuals(w: &mut World, slot: u32, sfx: bool) {
    if pillar(w, slot).dying_at >= 0 {
        return;
    }
    let tick = w.flow.tick;
    pillar_mut(w, slot).dying_at = tick;
    let g = geo(w, slot);
    solids_off(w, &g); // the capsule: hurt
    if sfx {
        sound(w, SFX_DESTROY, g.hit_box);
    }
    let o = owner(w, slot);
    stage_delay(w, o, DEATH_DELAY, DEATH_SECONDS, DEATH_DELAY);
}

fn solids_off(w: &mut World, g: &Geo) {
    for &id in &g.solids {
        let n = w.stage_run.as_ref().expect("a stage run").solids.len();
        if let Ok(i) = u32::try_from(id)
            && (i as usize) < n
        {
            stage_solid_enable(w, i, false);
        }
    }
}

/// TreeFortTrampolineBounceAudio(magnitude, the pawn): SpawnAkComponentAtLocation(the tree's place) @150, the RTPC
/// map_treehouse_trampoline = magnitude on it @263, sfx_map_treehouse_trampoline_tree posted on it with the pawn as
/// Instigator @320.
fn bounce_audio(w: &mut World, slot: u32, f: FighterId, magnitude: Real) {
    let instigator = w.fighter(f).id;
    let (x, y) = {
        let a = &w.stage_run.as_ref().expect("a stage run").actors[slot as usize];
        (a.x, a.y)
    };
    // (BounceMagnitudeParam: a float)
    let value = magnitude;
    w.events.push(Event::SpawnedSound { instigator, ev: SFX_BOUNCE, x, y, rtpc: RTPC_BOUNCE, value });
}

/// TreeFortTrampolineBounceVisual(the actor's place, the bounce direction), UE vectors.
fn bounce_visual(w: &mut World, slot: u32, at: [Real; 3], dir: [Real; 3]) {
    let g = geo(w, slot);
    let mut closest = 0;
    let mut best: Real = 5000.0;
    for i in 1..NUM_BONES {
        if mesh_bone_skipped(i) {
            continue;
        }
        let b = g.bones[i];
        let d = ue::dist(at, b);
        if d < best {
            best = d;
            closest = i;
        }
    }
    if closest == 0 {
        return;
    }
    let u = [g.root[0] - at[0], g.root[1] - at[1], g.root[2] - at[2]];
    // GetDirectionUnitVector(at, Root) (142f6db80: the safe normal, tolerance 1e-8)
    let n = ue::safe_normal(u, ue::SMALL_NUMBER);
    let t = now(w);
    let tick = w.flow.tick;
    let a = pillar_mut(w, slot);
    a.wiggles.push(PillarWiggle {
        bone: i32::try_from(closest).expect("a bone index"),
        dx: n[0],
        dy: n[1],
        dz: n[2],
        t,
    });
    // MakeRotFromX(dir) (142f74290: MakeFromX + FMatrix::Rotator); the emitter's pitch @1182 SelectFloat(Pitch,
    // 90 - Pitch + 90, Yaw > 0): Pitch when Yaw > 0, else 180 - Pitch
    let [pitch, yaw, _] = ue::rot_from_x(dir);
    let p = [at[0] + n[0] * 100.0, at[1] + n[1] * 100.0, at[2] + n[2] * 100.0];
    a.bounce_at = tick;
    a.bounce_fx =
        Some(BounceFx { x: -p[1], y: p[2], z: p[0], pitch: if yaw > 0.0 { pitch } else { 90.0 - pitch + 90.0 } });
}

/// TreeFortTrampolineBounce(HitActor, Hit): `hit` false is the tick's standing bounce (an empty FHitResult).
fn trampoline(w: &mut World, slot: u32, f: FighterId) {
    let fid = w.fighter(f).id;
    if pillar(w, slot).limit_actors.contains(&fid) {
        return;
    }
    if w.fighter(f).wall.wall_sliding {
        return; // @530
    }
    let g = geo(w, slot);
    let (bvx, bvy) = crate::combat::hit::launch::bounce_velocity(w, f);
    let speed = ue::size2d([bvx, bvy]); // LocalActorSpeed: VSize
    // @2471-3023 (and @4172-4751, the same with no hit): up x FClamp(speed, 1600, 2500), turned about X by (Y - the
    // tree's Y) / -420 x 20 degrees, x 1.1. The tree is unrotated: its up vector is Z.
    let c = min(max(speed, MIN_BOUNCE), MAX_BOUNCE);
    let ang = ((ue_loc(w, f)[1] - g.ue_y) / -420.0) * MAX_ANGLE;
    // RotateAngleAxis(up x c, ang, (1, 0, 0)) (the node normalises the axis by InvSqrt), then x LocalBounceSpeedMod
    let x_axis = [ue::inv_sqrt(1.0), 0.0, 0.0];
    let r = ue::rotate_angle_axis([0.0, 0.0, c], ang, x_axis);
    let mut v = [r[0] * BOUNCE_MOD, r[1] * BOUNCE_MOD, r[2] * BOUNCE_MOD];
    // @3050-3356: (0, Y x 0.5, |Y x 0.5| + max(Z, 0))
    let hy = v[1] * 0.5;
    v = [0.0, hy, (hy.abs() + max(v[2], 0.0))];
    if w.fighter(f).hit.hitstun > 0.0 {
        // K2_TimerExistsHandle(HitstunTimer) @3383
        w.fighter_mut(f).attackers.force_additive_kb = true; // SetForceAdditiveKnockback(true) @3471
        v = ue::clamped_to_size(v, MIN_BOUNCE, 9999.0);
    } else {
        v = ue::clamped_to_size(v, MIN_BOUNCE, MAX_BOUNCE);
    }
    let magnitude = min(max((speed - 1000.0) / 2000.0, 0.0), 1.0); // MapRangeClamped(speed, 1000, 3000, 0, 1) @3608
    bounce_audio(w, slot, f, magnitude);
    launch(w, f, v);
    w.fighter_mut(f).fast_fall.ff_triggered = false; // BPI_SetFastFallTriggered(false)
    fighter_bounce(w, f);
    let at = ue_loc(w, f);
    bounce_visual(w, slot, at, v);
    // HasAuthority and TrampolineBounceDoDmg @3966
    if pillar(w, slot).do_dmg {
        damage(w, slot, SELF_DAMAGE);
    }
    let a = pillar_mut(w, slot);
    let lt = a.limit_time;
    a.limit_actors.push(fid); // AddActorBounceLimit
    a.limit_timers.push(lt);
}

/// TreeFortFighterBounce @6252: BFReset Gravity, ResetJump(0), BPI_SetBlockFastFallTime(0.2).
fn fighter_bounce(w: &mut World, f: FighterId) {
    {
        let m = &mut w.fighter_mut(f).moves;
        m.net_gravity = Some(GRAVITY_SCALE);
        m.gravity_mod_count = 0;
    }
    reset_jump_counts(w, f);
    let ff = &mut w.fighter_mut(f).fast_fall;
    if BLOCK_FF_TIME > ff.block_ff_time.unwrap_or(0.0) {
        ff.block_ff_time = Some(BLOCK_FF_TIME);
    }
}

/// TreeFortPillarSideBounce(HitActor, Hit): (nx, ny) the hit's ImpactNormal (sim, as the tree's event gets it).
#[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated test keeps the NaN result")]
fn side_bounce(w: &mut World, slot: u32, f: FighterId, nx: Real, ny: Real) {
    if !(w.fighter(f).hit.hitstun > 0.0) {
        return; // HitstunTimer @121
    }
    let (vx, vy) = crate::combat::hit::launch::bounce_velocity(w, f);
    let d = vx * nx + vy * ny;
    // MirrorVectorByNormal (sim axes: the reflection is the same)
    let (mx, my) = (vx - 2.0 * d * nx, vy - 2.0 * d * ny);
    if !(ny > 0.9) {
        launch(w, f, [0.0, (-mx * SIDE_MOD), (my * SIDE_MOD)]); // ImpactNormal.Z > 0.9: no launch @463
    }
    let at = ue_loc(w, f);
    bounce_visual(w, slot, at, [0.0, -mx, my]);
    w.fighter_mut(f).fast_fall.ff_triggered = false;
    bounce_audio(w, slot, f, SIDE_BOUNCE_MAGNITUDE);
}

/// A spawned actor's place, velocity (BPI_GetParentVelocity) and whether its capsule simulates physics (UE place).
// Unverified: an item's BPI_GetParentVelocity is taken as its hit velocity (`Item::hit_velocity`, the parent's tick
// copy of its velocity), a projectile's as its parent_vx / parent_vy (SpawnedActorParentVelocity).
fn spawned(w: &World, by: ActorRef) -> Option<([Real; 3], [Real; 3], bool)> {
    match by {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            Some(([0.0, -p.x, p.y], [0.0, -p.parent_vx, p.parent_vy], false))
        }
        ActorRef::Item(id) => {
            let it = w.actors.items.get(id)?;
            let v = it.hit_velocity();
            Some(([0.0, -it.x, it.y], [0.0, -v.x, v.y], it.simulating_physics()))
        }
        _ => None,
    }
}

/// A simulating capsule's AddImpulse(v, None, true) (a velocity change), else the ProjectileMovement's Velocity = v.
fn spawned_velocity(w: &mut World, by: ActorRef, v: [Real; 3], simulating: bool) {
    match by {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.vx = -v[1];
                p.vy = v[2];
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id) {
                if simulating {
                    it.vx += -v[1];
                    it.vy += v[2];
                } else if it.movement_active() {
                    it.vx = -v[1];
                    it.vy = v[2];
                }
            }
        }
        _ => {}
    }
}

/// TreeFortTrampolineBounce's BPI_SpawnedActor branch (@571-2208): up x FClamp(|parent velocity|, 1000, 2500) turned
/// about X by (Y - the tree's Y) / -420 x 20 degrees (no speed mod), the capsule's impulse or the movement's Velocity
/// (@4772-5456), BounceVisualMulticast at the actor, AudioBounceMulticast(0.7, null), AddActorBounceLimitMulticast.
fn spawned_trampoline(w: &mut World, slot: u32, by: ActorRef) {
    if pillar(w, slot).limit_spawned.contains(&by) {
        return;
    }
    let Some((at, pv, simulating)) = spawned(w, by) else { return };
    let g = geo(w, slot);
    let c = min(max(ue::size(pv), 1000.0), MAX_BOUNCE);
    let ang = ((at[1] - g.ue_y) / -420.0) * MAX_ANGLE;
    let x_axis = [ue::inv_sqrt(1.0), 0.0, 0.0];
    let v = ue::rotate_angle_axis([0.0, 0.0, c], ang, x_axis);
    spawned_velocity(w, by, v, simulating);
    let at = spawned(w, by).map_or(at, |s| s.0);
    bounce_visual(w, slot, at, v);
    // AudioBounceMulticast(0.7, null) @2165: TreeFortTrampolineBounceAudio (ubergraph 6186) returns at @29 on
    // IsValid(BouncingActor) false: no sound.
    let a = pillar_mut(w, slot);
    let lt = a.limit_time;
    a.limit_spawned.push(by);
    a.limit_spawned_timers.push(lt);
}

/// TreeFortPillarSideBounce's BPI_SpawnedActor branch (@952-2779): MirrorVectorByNormal(parent velocity, the hit's
/// ImpactNormal) x 1, the capsule's impulse or the movement's Velocity, BounceVisualMulticast at the actor; no sound.
fn spawned_side_bounce(w: &mut World, slot: u32, by: ActorRef, nx: Real, ny: Real) {
    let Some((at, pv, simulating)) = spawned(w, by) else { return };
    let (vx, vy) = (-pv[1], pv[2]);
    let d = vx * nx + vy * ny;
    let (mx, my) = (vx - 2.0 * d * nx, vy - 2.0 * d * ny);
    let v = [0.0, -mx, my];
    spawned_velocity(w, by, v, simulating);
    let at = spawned(w, by).map_or(at, |s| s.0);
    bounce_visual(w, slot, at, v);
}

/// HandleActorBounceLimitTime: every timer less the world's delta, then the spent ones out (index kept on a removal).
fn bounce_limit_time(a: &mut Pillar) {
    for t in &mut a.limit_timers {
        *t -= DELTA_SECONDS;
    }
    let mut i = 0;
    while i < a.limit_timers.len() {
        if a.limit_timers[i] <= 0.0 {
            a.limit_timers.remove(i);
            a.limit_actors.remove(i);
        } else {
            i += 1;
        }
    }
    for t in &mut a.limit_spawned_timers {
        *t -= DELTA_SECONDS;
    }
    let mut i = 0;
    while i < a.limit_spawned_timers.len() {
        if a.limit_spawned_timers[i] <= 0.0 {
            a.limit_spawned_timers.remove(i);
            a.limit_spawned.remove(i);
        } else {
            i += 1;
        }
    }
}

/// TreeTrampolineWiggleCurve (Panda_Main/Blueprints/Curves): its keys [Time, Value, ArriveTangent, LeaveTangent], the
/// first six RCIM_Cubic, the last RCIM_Linear; no extrapolation set (RCCE_Constant: the end values outside).
const WIGGLE_KEYS: [[Real; 4]; 7] = [
    [0.0, -2.048_909_7e-8, 0.0, 0.0],
    [0.03, 3.0, -0.018_955_15, -0.018_955_15],
    [0.15, -1.2, -0.009_055_188, -0.009_055_188],
    [0.282_727_93, 0.6, 0.018_950_965, 0.018_950_965],
    [0.353_941_14, -0.399_999_98, -0.011_204_494, -0.011_204_494],
    [0.453_941_14, 0.200_000_05, -0.000_194_907_62, -0.000_194_907_62],
    [0.5, -5.093_472_3e-9, 0.0, 0.0],
];
/// Keys 0..5 cubic, key 6 linear (its segment: none after it).
const WIGGLE_CUBIC: usize = 6;
const WIGGLE_SCALE: Real = 150.0;

/// FRichCurve::Eval: a cubic segment is the Bezier of P0, P0 + Leave x gap / 3, P3 - Arrive x gap / 3, P3 (tangent
/// weights none).
fn wiggle_curve(t: Real) -> Real {
    let k = &WIGGLE_KEYS;
    if t <= k[0][0] {
        return k[0][1];
    }
    if t >= k[k.len() - 1][0] {
        return k[k.len() - 1][1];
    }
    let mut i = 0;
    while k[i + 1][0] <= t {
        i += 1;
    }
    let (a, b) = (k[i], k[i + 1]);
    let gap = b[0] - a[0];
    let u = (t - a[0]) / gap;
    if i >= WIGGLE_CUBIC {
        return a[1] + (b[1] - a[1]) * u;
    }
    let p1 = a[1] + (a[3] * gap) / 3.0;
    let p2 = b[1] - (b[2] * gap) / 3.0;
    let v = 1.0 - u;
    v * v * v * a[1] + 3.0 * v * v * u * p1 + 3.0 * v * u * u * p2 + u * u * u * b[1]
}

/// The wiggle pass (@4830 -> @2553), records in order: one past 0.5 s sets AnimBp.Bones[PointNumber] to zero (@2808)
/// and its index is collected (AddUnique); one within it sets AnimBp.Bones[PointNumber] = WiggleDirection x
/// WiggleCurve(age) x 150 (@3267). Then the collected indices are removed in turn, each by its number in the list as it
/// is then (@30-306: no shift for the ones before it), so a later index can miss its record or take another one: the
/// bone of a dropped live record keeps its last value (`bones` holds them; the drawing reads it).
fn wiggle_tick(w: &mut World, slot: u32) {
    if pillar(w, slot).wiggles.is_empty() {
        return;
    }
    let t = now(w);
    let a = pillar_mut(w, slot);
    let mut gone: Vec<usize> = Vec::new();
    for i in 0..a.wiggles.len() {
        let r = &a.wiggles[i];
        let age = t - r.t;
        // Array_Set(bSizeToFit false): an index past the 8 entries writes nothing (PointNumber is 1..6 here)
        let o = usize::try_from(r.bone).expect("a bone index") * 3;
        if age > 0.5 {
            if o + 2 < a.bones.len() {
                a.bones[o] = 0.0;
                a.bones[o + 1] = 0.0;
                a.bones[o + 2] = 0.0;
            }
            if !gone.contains(&i) {
                gone.push(i);
            }
        } else if o + 2 < a.bones.len() {
            let k = wiggle_curve(age) * WIGGLE_SCALE;
            a.bones[o] = r.dx * k;
            a.bones[o + 1] = r.dy * k;
            a.bones[o + 2] = r.dz * k;
        }
    }
    for i in gone {
        if i < a.wiggles.len() {
            a.wiggles.remove(i);
        }
    }
}

/// ReceiveTick (`StageEntry::pre_tick` of the tree's owner number).
pub fn tick(w: &mut World, slot: u32) {
    let g = geo(w, slot);
    for f in w.all_ids() {
        // GetMovementBase's owner is this tree: he walks on its box or its top (the CMC's base while walking)
        let fi = w.fighter(f);
        let on = fi.body.on_floor
            && match fi.platforms.base {
                Some(PlatformRef::Solid(b)) => {
                    let run = w.stage_run.as_ref().expect("a stage run");
                    let s = i64::from(run.boxes[b as usize].solid);
                    s == i64::from(g.boxed) || s == i64::from(g.top)
                }
                // the round top as a polygon (StageSolidPoly: its index is the solid's)
                Some(PlatformRef::Poly(i)) => i64::from(i) == i64::from(g.boxed) || i64::from(i) == i64::from(g.top),
                _ => false,
            };
        if on {
            trampoline(w, slot, f);
        }
    }
    bounce_limit_time(pillar_mut(w, slot));
    wiggle_tick(w, slot);
}

/// BeginPlay with AllowHazards off (@5888) and the death Delay's link (@15): K2_DestroyActor.
pub fn destroy(w: &mut World, slot: u32) {
    w.stage_run.as_mut().expect("a stage run").actors[slot as usize].dead = true;
    let g = geo(w, slot);
    solids_off(w, &g);
}

/// `create`: the actor's own state from its placed data.
fn create(w: &mut World, slot: u32) {
    let index = w.stage_run.as_ref().expect("a stage run").actors[slot as usize].index;
    let data = Arc::clone(&w.data);
    let d = &data.stage.placed.as_ref().expect("placed actors")[index as usize];
    let g = make_geo(d, w);
    let hp = num(&d.p, "TreePillarHP").unwrap_or(15.0);
    let p = Pillar {
        hp,
        start_hp: hp,
        damage_stage: 0,
        is_right: flag_or(&d.p, "IsRight", true),
        do_dmg: flag(&d.p, "TrampolineBounceDoDmg"),
        dying_at: -1,
        hit_at: -1,
        damage_at: -1,
        bounce_at: -1,
        bounce_fx: None,
        wiggles: Vec::new(),
        bones: vec![0.0; BONE_SLOTS * 3],
        limit_actors: Vec::new(),
        limit_timers: Vec::new(),
        limit_spawned: Vec::new(),
        limit_spawned_timers: Vec::new(),
        limit_time: num(&d.p, "StartingBounceLimitTime").unwrap_or(0.05),
        geo: Arc::new(g),
    };
    w.stage_run.as_mut().expect("a stage run").actors[slot as usize].own = StageActorOwn::Pillar(p);
}

/// TreePillarDamageHitBox and TreePillarDamageHitCapsule (DamageDestructible), off from DeathVisuals on.
fn hurt(w: &World, slot: u32, out: &mut Vec<StageHurt>) -> bool {
    let a = pillar(w, slot);
    if a.dying_at >= 0 {
        return false;
    }
    out.extend(a.geo.hurt.iter().cloned());
    true
}

fn any_damage(w: &mut World, slot: u32, dmg: Real, _causer: ActorRef) {
    if pillar(w, slot).dying_at >= 0 {
        return; // IsDying @6787
    }
    let tick = w.flow.tick;
    pillar_mut(w, slot).hit_at = tick; // PlayHitAnimation
    damage(w, slot, dmg);
    let at = geo(w, slot).hit_box;
    sound(w, SFX_HIT, at);
}

fn solid_hit(w: &mut World, slot: u32, solid: u32, by: ActorRef, nx: Real, ny: Real) {
    let g = geo(w, slot);
    let s = i64::from(solid);
    let top = s == i64::from(g.top);
    if !top && s != i64::from(g.boxed) {
        return;
    }
    match by {
        ActorRef::Fighter(f) if top => trampoline(w, slot, f),
        ActorRef::Fighter(f) => side_bounce(w, slot, f, nx, ny),
        _ if top => spawned_trampoline(w, slot, by),
        _ => spawned_side_bounce(w, slot, by, nx, ny),
    }
}

/// The kind of both classes (`pillarKind`).
pub fn entry(class: StageActorClass) -> StageActorEntry {
    StageActorEntry {
        class,
        hurt_type: Some(i32::from(CH_DAMAGE)),
        friendly_fire: false,
        can_take_damage: true,
        hit_pause_multiplier: None,
        // Unverified: no PhysMaterialOverride on its components: the light hit sound's SurfaceType 6 is taken.
        surface: Some(6),
        create: Some(create),
        any_damage: Some(any_damage),
        hit_by_enemy: None,
        hit_by_ally: None,
        knocked_back: None,
        hurt: Some(hurt),
        sounds: &[],
        damage_disabled: None,
        invulnerable: None,
        current_damage: None,
        victim_pause: None,
        mesh: None,
        reset: None,
        solid_hit: Some(solid_hit),
    }
}
