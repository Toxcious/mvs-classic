//! UKismetSystemLibrary's shape traces and overlaps against the bodies the match holds, in the play plane (sim x =
//! -UE Y, y = UE Z): CapsuleTraceMultiForObjects, SphereTraceMultiForObjects, BoxTraceSingleForObjects,
//! SphereTraceSingle / CapsuleTraceSingle / BoxTraceSingle (by trace channel), SphereOverlapActors and
//! BoxOverlapActors; BaseFighter's StandingOnEdge. The stage's
//! solids, their marks and the edges' spawned owners are [`solids`].
//!
//! The native rules these rest on:
//! - CapsuleTraceMultiForObjects: initial overlaps are in the result (bStartPenetrating, Time 0, the penetration
//!   normal); the hits are sorted by Time, a non-blocking hit before a blocking one at equal Time, by an insertion sort
//!   under 9 hits, so equal keys keep the order PhysX gave.
//! - A multi object sweep (SphereTraceMultiForObjects 142b31ff0): CalcQueryHitType gives Touch in the prefilter and
//!   Block in the conversion, so every shape of the asked object types the whole sweep meets is returned, bBlockingHit
//!   true, hits behind the first one too; a zero-length sweep (Start = End) gives one hit per shape it starts in. The
//!   single object trace keeps the first hit.
//! - An object query looks at the body's object type only, not at its responses; a trace by channel at the body's
//!   response to the channel: Block for a single trace.
//! - The object type query indices (zero-based, as Blueprint arrays hold them): 0 WorldStatic, 1 WorldDynamic, 2 Pawn,
//!   3 PhysicsBody, 4 Vehicle, 5 Destructible, 6 PlatformDetection, 7 HitBox, 8 DamageDestructible, 9 KnockBack,
//!   10 Platform, 11 ProjectileDestructible, 12 Cleanse, 13 FadeCheck, 14 Item, 15 Portal, 16 ItemDestructible,
//!   17 WorldStaticDetection, 18 HurtBox, 19 BlockTrace (DefaultEngine.ini's object channels in order after the six
//!   stock ones). The trace type queries: 0 Visibility, 1 Camera, 2 WallJump, 3 ProjectileGroundTrace,
//!   4 ProjectileExplode, 5 ProjectileGroundNoPlatform.
//! - bIgnoreSelf: the world context object's actor, or the first actor among its outers (a component's owner), is
//!   ignored. The caller passes that actor as `own`.
//!
//! The bodies gathered, by object type (the component's serialized ObjectType, else its profile's ObjectTypeName; a
//! profile without one, Floor and KnockBack, is taken at the serialized WorldStatic, unverified):
//! - 0 WorldStatic: the stage block (Floor), a fighter's KnockBack sphere (FullBody) while a Hitframe naming it is on
//!   (BaseFighter HitBoxesOff([FullBody]) at BeginPlay).
//! - The stage's solids (StageData.solids) with their collision on, after the platforms: the object type of the
//!   profile (Platform for a one-way one; a Custom one, whose type the stage data does not write, is warned and left
//!   out of the object queries: unverified), the trace channels their responses block (StageSolidData flags), a box
//!   section as a box, any other as its convex polygon.
//! - 1 WorldDynamic: a projectile's root Capsule that saves no profile (the shape's OverlapAllDynamic) or a Custom one
//!   with no ObjectType, while its query collision is on; the classes' own bodies (`bodies`); an item's PickupArea
//!   (profile DetectPawn) out of a hand, with its collision on.
//! - 2 Pawn: a fighter's root capsule (CollisionCylinder, profile Pawn: the ACharacter constructor), not while its
//!   capsule is off (CapsuleComponent.SetCollisionEnabled(NoCollision)); a fighter under SetActorEnableCollision(false)
//!   (the pull, Buff_Teleporting3) has no body at all. The skeletal meshes give no body: no pawn's mesh names a
//!   PhysicsAsset.
//! - 3 PhysicsBody: a projectile's root Capsule (ProjectileNavNoPlatform / ProjectileNavPlatform) while its query
//!   collision is on; the classes' own bodies.
//! - 8 DamageDestructible: C019_MortarAim's Hurtbox, an upright capsule at the Aim.
//! - 10 Platform: the stage's pass-through platforms (BasePlatformBP Box, profile Platform), C019_Ship's pieces and the
//!   SpawnedPlatformParent boxes with their collision on.
//! - 11 ProjectileDestructible: a projectile's HurtBox sphere while hurtRadius > 0, with the template's query
//!   collision on (a graph's SetCollisionEnabled on it is not followed); the classes' own bodies.
//! - 14 Item, 16 ItemDestructible: an item's root Capsule and its Sphere, out of a hand.
//! - 18 HurtBox: a fighter's hurt capsules (the ones UpdateHurtBoxes turned off too: Comp_Pawn_HitBoxHandler @10603
//!   changes SetGenerateOverlapEvents only, which a query does not read) and HurtBoxFloorBounce, always (BaseFighter's
//!   construction script 1407-1586 places it, BFBounce turns it with UpdateHurtBoxes 8601, 139859).
//! - 15 Portal: the maps' BlockPortalCollision_C Box (StageData.queryBodies; profile Custom, object type
//!   ECC_GameTraceChannel13, every channel Ignore, so an object query alone meets it), after the solids; and the
//!   actors' own bodies.
//! - 19 BlockTrace: the actors' own bodies alone.
//!
//! Not gathered (a query that asks for the type is warned): 6 PlatformDetection (BaseFighter's Box), 7 HitBox, the
//! rest.
//! Unverified: of 1 WorldDynamic only the bodies above (a query for it is not warned): the root Capsule of a world
//! actor class without its own bodies (warned by the class check), BaseFighter's Capsule 42 / 96 (the shape default
//! OverlapAllDynamic; its fighter is met through its Pawn capsule by a query that asks for both) and the blast box's
//! Trigger boxes are left out; and the actors of the World's typed lists other than ships and mortars (markers, aim
//! punches, accelerators); of a mortar only its Hurtbox.
//!
//! Unverified: the order of hits of equal Time (every hit of a zero-length sweep) is PhysX's, not read: here the gather
//! order, the level's bodies first (the stage block, then its platforms), then the ships' pieces and the
//! SpawnedPlatformParent boxes, then per fighter its root capsule, HurtBoxFloorBounce, its KnockBack spheres and its
//! hurt capsules, then the projectiles (root, HurtBox, own bodies) and the items (root, Sphere, own bodies) in their
//! lists' order, then the mortars' Hurtboxes, then the world actors' own bodies.
//! Deviation: each body is a shape of the play plane, the stage's and the spawned boxes extruded along the depth, a
//! hurt capsule its segment in the plane, a KnockBack sphere tested at its depth; a touch (distance equal to the radii)
//! counts as an overlap; PhysX's sweep inflation and contact offsets are not read; ImpactPoint is the query's surface
//! point under its centre on a face (a corner: the corner); Normal and ImpactNormal are one vector (the swept shape's
//! normal at the touch; at a start inside, the least penetration).
//! Not handled: a Disguise's clone shapes and a pawn with its normal hitboxes off.

pub mod edges;
pub mod solids;

pub use solids::{
    KinematicOfOut, Solid, Solids, StageSolidPoly, capsule_sweep_all, collide_polys, edge_begin_play, is_static_ground,
    is_static_solid, kinematic_of, mark_static_solid, poly_extent, poly_flags, poly_height_at, poly_walkable_at,
    section, solid_blockers, solid_blockers_marked, solid_box_ground, solid_object_type, spawned_edges_end,
    sphere_static, stage_solids, static_grounds, static_of, static_solids, statics_among, walk_out_of_polys, with_poly,
    with_statics,
};

use crate::Real;
use crate::actors::{ActorRef, ItemId, ProjectileId};
use crate::fighter::FighterId;
use crate::fighter::body::PlatformRef;
use crate::name;
use crate::names::Name;
use crate::registry::entries::ActorHooks;
use crate::world::World;
use mvs_collision::traces::{Query, Shape, Tracer};
use mvs_data::value::{Props, Value};

/// `TraceHit`: a query's hit (FHitResult as BreakHitResult gives it, in sim space).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TraceHit {
    /// 0..1 of the sweep; 0 for an initial overlap.
    pub time: Real,
    pub distance: Real,
    /// bBlockingHit.
    pub blocking: bool,
    /// bStartPenetrating (BreakHitResult's bInitialOverlap).
    pub initial: bool,
    /// Location: the query shape's centre at the hit (the start for an initial overlap).
    pub x: Real,
    pub y: Real,
    /// ImpactPoint (an initial overlap: the body surface along the push-out normal).
    pub ix: Real,
    pub iy: Real,
    /// Normal (= ImpactNormal).
    pub nx: Real,
    pub ny: Real,
    /// PenetrationDepth (initial overlaps).
    pub depth: Real,
    /// The hit body's object type query index.
    pub object_type: i32,
    /// HitComponent.GetCollisionProfileName().
    pub profile: Name,
    /// The component's name (without _GEN_VARIABLE); the stage block `Floor`, a stage platform `Box`.
    pub component: Name,
    /// HitActor: `None` for the level's own bodies (the stage block, its platforms and solids).
    pub actor: Option<ActorRef>,
    /// A stage platform's index, else -1.
    pub platform: i32,
    /// The floor the body is (`TraceBody.floor`).
    pub floor: Option<PlatformRef>,
}

/// `KinematicMove`: a kinematic solid's move of the frame.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct KinematicMove {
    pub mx: Real,
    pub my: Real,
    pub tpx: Option<Real>,
    pub tpy: Option<Real>,
}

/// `BodyInfo`: a component template's body as the queries read it: profile name, object type
/// (`type`; -1 for none the port reads), query collision and the blocked trace channels (bits `1 << channel`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BodyInfo {
    pub profile: Name,
    pub kind: i32,
    pub on: bool,
    pub blocks: i32,
}

/// `TraceBody`: a body a trace tests, sim space: the core segment (a, b) (a point: both
/// ends equal; an upright capsule of radius r and half height h: (x, y -+ (h - r))), grown by the half extents
/// (hx, hy) (a box) and rounded by r (a sphere or capsule). z: the depth of a body round in depth (a sphere), NaN for
/// one extruded along it. blocks: the bit (1 << channel) of each trace channel it blocks.
#[derive(Clone, Debug)]
pub struct TraceBody {
    pub ax: Real,
    pub ay: Real,
    pub bx: Real,
    pub by: Real,
    pub hx: Real,
    pub hy: Real,
    pub r: Real,
    pub z: Real,
    pub object_type: i32,
    pub profile: Name,
    pub component: Name,
    pub actor: Option<ActorRef>,
    pub platform: i32,
    pub blocks: i32,
    /// A stage solid's polygon section: its corners x y after one another, counter-clockwise, at (ax, ay).
    pub poly: Option<Vec<f64>>,
    /// The floor the body is (a stage platform, a stage solid, a spawned box, a ship's piece).
    pub floor: Option<PlatformRef>,
}

/// The queries' scratch (POOL / used / QUERY, the sweep's hull and the poses buffer): written by a
/// query before it is read, never state.
#[derive(Clone, Debug, Default)]
pub struct TraceScratch {
    /// The gathered bodies of the query.
    pool: Vec<TraceBody>,
    /// What a class's `bodies` hook gave, before the query's filter.
    sink: Vec<TraceBody>,
    /// The single queries' hits.
    hits: Vec<TraceHit>,
    tracer: Tracer,
    /// A fighter's sampled box pose.
    pose: Vec<f32>,
    /// The KnockBack shapes already gathered (SEEN).
    seen: Vec<u32>,
    /// The actors of a family, taken before its pass.
    ids: Vec<ActorRef>,
    /// A component template's key.
    key: String,
    /// A HitBoxTag lower-cased (the boxes' tag table's key).
    tag: String,
    /// A spawned actor's SpawnedPlatform parts: each one's collision on and component name.
    parts: Vec<(bool, Name)>,
}

// ---- object types, profiles, channels

/// `OBJ`: the object type query indices.
pub const OBJ_WORLD_STATIC: i32 = 0;
pub const OBJ_WORLD_DYNAMIC: i32 = 1;
pub const OBJ_PAWN: i32 = 2;
pub const OBJ_PHYSICS_BODY: i32 = 3;
pub const OBJ_DAMAGE_DESTRUCTIBLE: i32 = 8;
pub const OBJ_KNOCK_BACK: i32 = 9;
pub const OBJ_PLATFORM: i32 = 10;
pub const OBJ_PROJECTILE_DESTRUCTIBLE: i32 = 11;
pub const OBJ_ITEM: i32 = 14;
pub const OBJ_PORTAL: i32 = 15;
pub const OBJ_ITEM_DESTRUCTIBLE: i32 = 16;
pub const OBJ_HURT_BOX: i32 = 18;
pub const OBJ_BLOCK_TRACE: i32 = 19;

/// `TRACE`: the trace channels' indices.
pub const TRACE_WALL_JUMP: i32 = 2;
/// `TRACE.ProjectileGroundTrace`.
pub const TRACE_PROJECTILE_GROUND: i32 = 3;
pub const TRACE_PROJECTILE_EXPLODE: i32 = 4;
pub const TRACE_PROJECTILE_GROUND_NO_PLATFORM: i32 = 5;

const CHANNEL_NAMES: [&str; 6] =
    ["Visibility", "Camera", "WallJump", "ProjectileGroundTrace", "ProjectileExplode", "ProjectileGroundNoPlatform"];

const GATHERED: i32 = (1 << OBJ_WORLD_STATIC)
    | (1 << OBJ_WORLD_DYNAMIC)
    | (1 << OBJ_PAWN)
    | (1 << OBJ_PHYSICS_BODY)
    | (1 << OBJ_DAMAGE_DESTRUCTIBLE)
    | (1 << OBJ_PLATFORM)
    | (1 << OBJ_PROJECTILE_DESTRUCTIBLE)
    | (1 << OBJ_ITEM)
    | (1 << OBJ_ITEM_DESTRUCTIBLE)
    | (1 << OBJ_HURT_BOX)
    | (1 << OBJ_PORTAL)
    | (1 << OBJ_BLOCK_TRACE);

/// `ECC`: the serialized ObjectType as a query index (DefaultEngine.ini DefaultChannelResponses 595-612: the object
/// channels; the trace channels 2, 6, 10, 16 take no index).
pub fn ecc(object_type: &str) -> Option<i32> {
    Some(match object_type {
        "ECC_WorldStatic" => 0,
        "ECC_WorldDynamic" => 1,
        "ECC_Pawn" => 2,
        "ECC_PhysicsBody" => 3,
        "ECC_Vehicle" => 4,
        "ECC_Destructible" => 5,
        "ECC_GameTraceChannel1" => 6,
        "ECC_GameTraceChannel3" => 7,
        "ECC_GameTraceChannel4" => 8,
        "ECC_GameTraceChannel5" => 9,
        "ECC_GameTraceChannel7" => 10,
        "ECC_GameTraceChannel8" => 11,
        "ECC_GameTraceChannel9" => 12,
        "ECC_GameTraceChannel11" => 13,
        "ECC_GameTraceChannel12" => 14,
        "ECC_GameTraceChannel13" => 15,
        "ECC_GameTraceChannel14" => 16,
        "ECC_GameTraceChannel15" => 17,
        "ECC_GameTraceChannel17" => 18,
        "ECC_GameTraceChannel18" => 19,
        _ => return None,
    })
}

/// `PROFILE_BLOCKS`: the profiles that Block one of the trace channels 2-5 (the same lines, +EditProfiles 613-630
/// applied; an entry without a Response is Block), as bits (1 << channel). Every other profile takes the channels'
/// defaults: Ignore for all four (DefaultChannelResponses 596, 600, 604, 610).
fn profile_blocks(profile: &str) -> i32 {
    match profile {
        "BlockAll" | "Floor" => 0b11_1100,
        "BlockAllDynamic" => 0b10_1100,
        "IgnoreOnlyPawn" | "Destructible" | "InvisibleWall" | "InvisibleWallDynamic" | "Vehicle" => 0b00_0100,
        "PhysicsActor" | "InvisibleWallPawn" | "CharacterPhysicsActor" => 0b00_1100,
        "Platform" => 0b00_1000,
        _ => 0,
    }
}

/// The object types' names in query index order (`OBJ_NAMES`).
const OBJ_NAMES: [&str; 20] = [
    "WorldStatic",
    "WorldDynamic",
    "Pawn",
    "PhysicsBody",
    "Vehicle",
    "Destructible",
    "PlatformDetection",
    "HitBox",
    "DamageDestructible",
    "KnockBack",
    "Platform",
    "ProjectileDestructible",
    "Cleanse",
    "FadeCheck",
    "Item",
    "Portal",
    "ItemDestructible",
    "WorldStaticDetection",
    "HurtBox",
    "BlockTrace",
];

/// `CHANNEL_DEFAULT`: DefaultEngine.ini DefaultChannelResponses 595-612 for the object channels past the six engine
/// ones (Block).
fn channel_default(name: &str) -> Name {
    match name {
        "PlatformDetection"
        | "HitBox"
        | "KnockBack"
        | "ProjectileDestructible"
        | "Cleanse"
        | "FadeCheck"
        | "Portal"
        | "ItemDestructible"
        | "WorldStaticDetection"
        | "HurtBox"
        | "BlockTrace" => name!("ECR_Ignore"),
        "Item" => name!("ECR_Overlap"),
        _ => name!("ECR_Block"),
    }
}

/// A response's string as its name (`ECR_Ignore`, `ECR_Overlap`, `ECR_Block`).
fn response_name(s: &str) -> Name {
    match s {
        "ECR_Ignore" => name!("ECR_Ignore"),
        "ECR_Overlap" => name!("ECR_Overlap"),
        _ => name!("ECR_Block"),
    }
}

/// A collision profile's name: the profiles of the port's table and Custom.
// Deviation: a profile outside the table (warned where it is read) is `Name::NONE`, not its string: what reads a
// hit's or a body's profile compares it with a profile of the table.
pub fn profile_name(s: &str) -> Name {
    match s {
        "Custom" => name!("Custom"),
        "NoCollision" => name!("NoCollision"),
        "BlockAll" => name!("BlockAll"),
        "OverlapAll" => name!("OverlapAll"),
        "BlockAllDynamic" => name!("BlockAllDynamic"),
        "OverlapAllDynamic" => name!("OverlapAllDynamic"),
        "IgnoreOnlyPawn" => name!("IgnoreOnlyPawn"),
        "OverlapOnlyPawn" => name!("OverlapOnlyPawn"),
        "Pawn" => name!("Pawn"),
        "Spectator" => name!("Spectator"),
        "CharacterMesh" => name!("CharacterMesh"),
        "PhysicsActor" => name!("PhysicsActor"),
        "Destructible" => name!("Destructible"),
        "InvisibleWall" => name!("InvisibleWall"),
        "InvisibleWallDynamic" => name!("InvisibleWallDynamic"),
        "Trigger" => name!("Trigger"),
        "Ragdoll" => name!("Ragdoll"),
        "Vehicle" => name!("Vehicle"),
        "UI" => name!("UI"),
        "HitBox" => name!("HitBox"),
        "PlatformDetection" => name!("PlatformDetection"),
        "DamageDestructible" => name!("DamageDestructible"),
        "InvisibleWallPawn" => name!("InvisibleWallPawn"),
        "KnockBack" => name!("KnockBack"),
        "Platform" => name!("Platform"),
        "CharacterPhysicsActor" => name!("CharacterPhysicsActor"),
        "DetectPawn" => name!("DetectPawn"),
        "Ghost" => name!("Ghost"),
        "Tether" => name!("Tether"),
        "Projectile" => name!("Projectile"),
        "ProjectileDestructible" => name!("ProjectileDestructible"),
        "IgnorePlatform" => name!("IgnorePlatform"),
        "Floor" => name!("Floor"),
        "Fog" => name!("Fog"),
        "NonDamageDestructibleHitBox" => name!("NonDamageDestructibleHitBox"),
        "Item" => name!("Item"),
        "PortalOverlap" => name!("PortalOverlap"),
        "PortalProjectile" => name!("PortalProjectile"),
        "BlockPortal" => name!("BlockPortal"),
        "ProjectileBreaking" => name!("ProjectileBreaking"),
        "Physmat" => name!("Physmat"),
        "ItemDestructible" => name!("ItemDestructible"),
        "ProjectileNavNoPlatform" => name!("ProjectileNavNoPlatform"),
        "ProjectileNavPlatform" => name!("ProjectileNavPlatform"),
        "HurtBox" => name!("HurtBox"),
        _ => Name::NONE,
    }
}

/// The ResponseArray of a component template's body.
fn response_array(comp: &Props) -> &[Value] {
    comp.get("BodyInstance")
        .and_then(Value::as_obj)
        .and_then(|b| b.get("CollisionResponses"))
        .and_then(Value::as_obj)
        .and_then(|r| r.get("ResponseArray"))
        .and_then(Value::as_arr)
        .unwrap_or(&[])
}

/// An entry's member as a string.
fn str_of<'a>(o: &'a Value, key: &str) -> Option<&'a str> {
    o.as_obj().and_then(|o| o.get(key)).and_then(Value::as_str)
}

/// The named profile the build applies over a component template's body, if any: the template's own CollisionProfileName
/// (a shape's unsaved one is the UShapeComponent ctor's OverlapAllDynamic) when it is not Custom and the table has it.
/// FBodyInstance::FixupData (142c714f0) / LoadProfileData (142c77210) rewrite the object type, CollisionEnabled and
/// ResponseArray from it; only a Custom (or no) profile keeps the saved ones.
fn applied_profile(comp: &Props) -> Option<&str> {
    let saved = comp.get("BodyInstance").and_then(Value::as_obj)?.get("CollisionProfileName").and_then(Value::as_str);
    let name = saved.or(is_shape_template(comp).then_some("OverlapAllDynamic"))?;
    (name != "Custom" && crate::world::profiles::profile(name).is_some()).then_some(name)
}

/// `responseTo`: a component template's response to an object type (its index in OBJ):
/// the applied named profile's (`applied_profile`), else the template's ResponseArray entry (an entry without a
/// Response is Block), else the channel's default: Block for the engine channels, DefaultEngine.ini's for the
/// project's (a Custom profile's rule).
pub fn response_to(comp: Option<&Props>, kind: Real) -> Name {
    let index = usize::try_from(crate::math::to_int32(kind)).ok();
    let name = index.and_then(|i| OBJ_NAMES.get(i)).copied().unwrap_or("");
    if let (Some(c), Some(i)) = (comp, index)
        && let Some(p) = applied_profile(c)
        && let Some(r) = crate::world::profiles::profile_response(p, i)
    {
        return match r {
            b'I' => name!("ECR_Ignore"),
            b'O' => name!("ECR_Overlap"),
            _ => name!("ECR_Block"),
        };
    }
    let list = comp.map_or(&[][..], response_array);
    match list.iter().find(|x| str_of(x, "Channel") == Some(name)) {
        Some(r) => str_of(r, "Response").map_or(name!("ECR_Block"), response_name),
        None => channel_default(name),
    }
}

/// A component template of a UShapeComponent class (a Box, Sphere or Capsule): one that saves a size, or the
/// navigation AreaClass, a UShapeComponent property every shape template of the data saves and no other component's
/// does (`isShapeTemplate`).
pub fn is_shape_template(comp: &Props) -> bool {
    ["AreaClass", "BoxExtent", "SphereRadius", "CapsuleRadius", "CapsuleHalfHeight"]
        .iter()
        .any(|k| comp.contains_key(*k))
}

/// `profileInfo`: a profile's body as the table has it (warned when the table lacks it).
fn profile_info(w: &World, profile: &str) -> BodyInfo {
    let b = profile_body(profile);
    if b.kind < 0 {
        crate::fighter::common::warn_once(w, name!("traces: a profile is not in the port's table"));
    }
    b
}

/// A profile's body as the table has it, `kind` -1 for one it lacks.
fn profile_body(profile: &str) -> BodyInfo {
    let pr = crate::world::profiles::profile(profile);
    BodyInfo {
        profile: profile_name(profile),
        kind: pr.map_or(-1, |p| p.0),
        on: pr.is_some_and(|p| p.1),
        blocks: profile_blocks(profile),
    }
}

/// `profileType`: a profile's object type (query index; -1 for one the table lacks,
/// warned).
pub fn profile_type(w: &World, profile: &str) -> i32 {
    profile_info(w, profile).kind
}

/// `bodyInfo`: a component template's body; `None` for a component without a body in the
/// data (no BodyInstance: the class's native default holds) unless a fallback profile is given. A body of no object
/// type the port reads (`kind` -1) is warned by the gather that meets it (`body`).
// Deviation: computed per call, not kept per template object (INFO); the same value.
pub fn body_info(comp: Option<&Props>, fallback_profile: Option<Name>) -> Option<BodyInfo> {
    let fallback = fallback_profile.map(fallback_str);
    let Some(comp) = comp else { return fallback.map(profile_body) };
    let Some(b) = comp.get("BodyInstance").and_then(Value::as_obj) else { return fallback.map(profile_body) };
    // A UWidgetComponent makes a body in World space only (14261bfe0).
    if comp.get("Space").and_then(Value::as_str) == Some("EWidgetSpace::Screen") {
        return None;
    }
    // An unserialized field is the archetype's: a Custom body without ObjectType keeps the class default's (the
    // fallback profile: Pawn_C017's CollisionCylinder is Custom over ACharacter's Pawn). A shape component's is the
    // UShapeComponent ctor's OverlapAllDynamic, applied in PostInitProperties on the CDO too (ctor 142984280 ->
    // 142974170): no saved profile name is that profile, a Custom one with no saved ObjectType is WorldDynamic. Not
    // ported: a body of another component class with neither is warned and dropped.
    let shape = is_shape_template(comp);
    let profile = b.get("CollisionProfileName").and_then(Value::as_str).or(fallback).unwrap_or(if shape {
        "OverlapAllDynamic"
    } else {
        "Custom"
    });
    let profiles = crate::world::profiles::profile;
    let pr = profiles(profile)
        .or_else(|| fallback.and_then(profiles))
        .or_else(|| if shape { profiles("OverlapAllDynamic") } else { None });
    // A named profile (not Custom, in the table) is applied over the saved fields (`applied_profile`).
    if profile != "Custom"
        && let Some(p) = profiles(profile)
    {
        let blocks = profile_blocks(profile);
        return Some(BodyInfo { profile: profile_name(profile), kind: p.0, on: p.1, blocks });
    }
    let kind = match b.get("ObjectType").and_then(Value::as_str) {
        Some(t) => ecc(t),
        None => pr.map(|p| p.0),
    };
    let en = b.get("CollisionEnabled").and_then(Value::as_str).unwrap_or("");
    let on = if en.is_empty() {
        pr.is_none_or(|p| p.1)
    } else {
        !en.ends_with("NoCollision") && !en.ends_with("PhysicsOnly")
    };
    let mut blocks = profile_blocks(profile);
    for r in response_array(comp) {
        let Some(ch) = CHANNEL_NAMES.iter().position(|c| Some(*c) == str_of(r, "Channel")) else { continue };
        if ch < 2 {
            continue;
        }
        if str_of(r, "Response").unwrap_or("ECR_Block") == "ECR_Block" {
            blocks |= 1 << ch;
        } else {
            blocks &= !(1 << ch);
        }
    }
    Some(BodyInfo { profile: profile_name(profile), kind: kind.unwrap_or(-1), on, blocks })
}

/// `bodyInfo` in a gather: warned when it has no object type the port reads.
fn body(w: &World, comp: Option<&Props>, fallback_profile: Option<Name>) -> Option<BodyInfo> {
    let b = body_info(comp, fallback_profile);
    if b.is_some_and(|b| b.kind < 0) {
        crate::fighter::common::warn_once(w, name!("traces: a body with no object type the port reads"));
    }
    b
}

/// A fallback profile's string (the callers pass a profile of the table: the fighters' Pawn, HurtBox, KnockBack, a
/// body's own profile read back, as `shield_actor` shieldBodies does).
fn fallback_str(n: Name) -> &'static str {
    match crate::world::profiles::PROFILES.iter().map(|p| p.0).find(|s| n != Name::NONE && profile_name(s) == n) {
        Some(s) => s,
        None => panic!("traces: a fallback profile the port's table does not name"),
    }
}

/// Whether a query meets a projectile's root Capsule (`rootQueryOn`): its template's query collision (a root with no
/// saved profile is the shape's OverlapAllDynamic, QueryOnly), or what a graph's SetCollisionEnabled wrote on it
/// (`capsuleOn`); never on a dead actor or one with its collision off.
pub fn root_query_on(pr: &crate::actors::Projectile, root: &BodyInfo) -> bool {
    if pr.cap_written {
        return pr.cap_collides && !pr.dead && !pr.no_collision;
    }
    root.on && !pr.dead && !pr.no_collision
}

// ---- the gather

/// The query (`Filter`): which bodies it takes (an object mask, or a channel), and what it ignores.
struct Filter<'a> {
    /// Object types (bits); 0 for a channel query.
    mask: i32,
    /// -1 for an object query.
    channel: i32,
    ignore: &'a [Option<ActorRef>],
    own: Option<ActorRef>,
    /// A query that asks after one actor's bodies alone (boxOverlapsActor): what it would hit of that actor is what
    /// the whole gather's hits hold of it (each body is swept on its own). The sections of other actors are left out;
    /// a class's `bodies` hook is still called for every actor, its bodies of another actor dropped.
    only: Option<ActorRef>,
    /// The querying component's object type for its overlap events (actorsOf's responder), -1 for a query.
    responder: i32,
}

fn wants(q: &Filter, kind: i32, blocks: i32) -> bool {
    if q.channel < 0 {
        q.mask.wrapping_shr(kind.cast_unsigned()) & 1 == 1
    } else {
        blocks.wrapping_shr(q.channel.cast_unsigned()) & 1 == 1
    }
}

fn ignored(q: &Filter, a: Option<ActorRef>) -> bool {
    a.is_some() && (a == q.own || q.ignore.contains(&a))
}

/// A body of this actor (`None`: the level's) is not asked after (`Filter.only`).
fn not_only(q: &Filter, a: Option<ActorRef>) -> bool {
    q.only.is_some() && a != q.only
}

#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn add_box(
    pool: &mut Vec<TraceBody>,
    left: Real,
    right: Real,
    bottom: Real,
    top: Real,
    info: &BodyInfo,
    component: Name,
    actor: Option<ActorRef>,
    platform: i32,
    floor: Option<PlatformRef>,
) {
    let (x, y) = ((left + right) / 2.0, (bottom + top) / 2.0);
    pool.push(TraceBody {
        ax: x,
        ay: y,
        bx: x,
        by: y,
        hx: (right - left) / 2.0,
        hy: (top - bottom) / 2.0,
        r: 0.0,
        z: Real::NAN,
        object_type: info.kind,
        profile: info.profile,
        component,
        actor,
        platform,
        blocks: info.blocks,
        poly: None,
        floor,
    });
}

#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn add_round(
    pool: &mut Vec<TraceBody>,
    ax: Real,
    ay: Real,
    bx: Real,
    by: Real,
    r: Real,
    z: Real,
    info: &BodyInfo,
    component: Name,
    actor: ActorRef,
) {
    pool.push(TraceBody {
        ax,
        ay,
        bx,
        by,
        hx: 0.0,
        hy: 0.0,
        r,
        z,
        object_type: info.kind,
        profile: info.profile,
        component,
        actor: Some(actor),
        platform: -1,
        blocks: info.blocks,
        poly: None,
        floor: None,
    });
}

/// An upright capsule (centre, radius, half height).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn add_capsule(
    pool: &mut Vec<TraceBody>,
    x: Real,
    y: Real,
    r: Real,
    hh: Real,
    info: &BodyInfo,
    component: Name,
    actor: ActorRef,
) {
    let s = crate::math::max(hh - r, 0.0);
    add_round(pool, x, y - s, x, y + s, r, Real::NAN, info, component, actor);
}

/// A class's `bodies` hook's sink (`add`): the bodies of the query's types kept.
fn add_sunk(q: &Filter, s: &mut TraceScratch) {
    for b in s.sink.drain(..) {
        if !wants(q, b.object_type, b.blocks) || ignored(q, b.actor) || not_only(q, b.actor) {
            continue;
        }
        s.pool.push(b);
    }
}

/// The component names each family gathers itself (`PROJ_GATHERED` ...).
const PROJ_GATHERED: [&str; 4] = ["Capsule", "HurtBox", "HitBox", "HitBox2D"];
const ITEM_GATHERED: [&str; 4] = ["Capsule", "Sphere", "ThrowHitBox", "PickupArea"];
const SOLIDS_GATHERED: [&str; 2] = ["Collision", "WallbounceCollision"];

/// `checkClass`: the class data's bodies of the asked types that the port gathers for no component of it, warned.
fn check_class(w: &World, q: &Filter, has_bodies: bool, comps: Option<&Props>, gathered: &[&str]) {
    let Some(comps) = comps else { return };
    if has_bodies {
        return;
    }
    for (name, c) in comps {
        if gathered.contains(&name.as_str()) {
            continue;
        }
        let Some(c) = c.as_obj() else { continue };
        if !c.contains_key("BodyInstance") {
            continue;
        }
        let Some(info) = body(w, Some(c), None) else { continue };
        if !info.on || info.kind < 0 || !wants(q, info.kind, info.blocks) {
            continue;
        }
        if q.responder >= 0 && response_to(Some(c), crate::real(q.responder)) == name!("ECR_Ignore") {
            continue;
        }
        crate::fighter::common::warn_once(w, name!("traces: a class's body is not entered (ActorKind.bodies)"));
    }
}

/// A component template of a class's components by name.
fn comp_of<'a>(comps: Option<&'a Props>, name: &str) -> Option<&'a Props> {
    comps.and_then(|c| c.get(name)).and_then(Value::as_obj)
}

const FIGHTER_ROOT: &str = "CollisionCylinder";
const FLOORBOUNCE: &str = "HurtBoxFloorBounce";
const HURTBOX_INFO: BodyInfo = BodyInfo { profile: name!("HurtBox"), kind: OBJ_HURT_BOX, on: true, blocks: 0 };

/// A fighter component's template (the pawn's over BaseFighter's), by its name without _GEN_VARIABLE.
fn fighter_comp<'a>(d: &'a mvs_data::fighter::FighterData, key: &mut String, name: &str) -> Option<&'a Props> {
    key.clear();
    key.push_str(name);
    if name != FIGHTER_ROOT {
        key.push_str("_GEN_VARIABLE");
    }
    d.components.pawn.get(key.as_str()).or_else(|| d.components.base.get(key.as_str())).and_then(|c| c.p.as_obj())
}

/// A notify's properties (its montage's record in the fighter's data).
fn notify_props<'a>(
    d: &'a mvs_data::fighter::FighterData,
    names: &crate::names::Names,
    n: &crate::events::NotifyRef,
) -> Option<&'a Props> {
    d.montages.get(names.str(n.montage)).and_then(|m| m.notifies.get(n.index as usize)).map(|x| &x.p)
}

fn gather_fighter(w: &mut World, f: FighterId, q: &Filter, s: &mut TraceScratch) {
    let me = ActorRef::Fighter(f);
    {
        let fi = w.fighter(f);
        let actor_collision_off = fi.moves.pull.is_some() || crate::combat::buffs::buff_collision_off(w, f);
        if fi.life.dead != 0.0 || actor_collision_off || ignored(q, Some(me)) {
            return;
        }
    }
    let data = std::sync::Arc::clone(&w.data);
    let ch = w.fighter(f).ch;
    let cd = &data.chars[w.names.str(ch)];
    let d = &cd.fighter;
    let root = body(w, fighter_comp(d, &mut s.key, FIGHTER_ROOT), Some(name!("Pawn"))).expect("the Pawn profile");
    if root.on && !w.fighter(f).capsule.capsule_off && wants(q, root.kind, root.blocks) {
        let (x, y) = (w.fighter(f).body.x, crate::fighter::body::actor_y(w, f));
        let (r, hh) = (crate::fighter::body::cap_radius(w, f), crate::fighter::body::capsule_half(w, f));
        add_capsule(&mut s.pool, x, y, r, hh, &root, name!("CollisionCylinder"), me);
    }
    // HurtBoxFloorBounce (BaseFighter: a HurtBox capsule on the root, query on whatever UpdateHurtBoxes did): the
    // construction script puts it at Z -(CapsuleComponent.CapsuleHalfHeight - its CapsuleHalfHeight) (1407-1586), the
    // template's half height then, at the actor's scale.
    if let Some(fb) = fighter_comp(d, &mut s.key, FLOORBOUNCE) {
        let info = body(w, Some(fb), Some(name!("HurtBox"))).unwrap_or(HURTBOX_INFO);
        if info.on
            && wants(q, info.kind, info.blocks)
            && let Some(c) = crate::combat::hit::floor_bounce_capsule(w, f)
        {
            let (x, y) = (w.fighter(f).body.x, crate::fighter::body::actor_y(w, f));
            add_capsule(&mut s.pool, x, y - c.dz, c.r, c.half, &info, name!("HurtBoxFloorBounce"), me);
        }
    }
    let kb = crate::fighter::common::knock_back_shapes(cd);
    let fi = w.fighter(f);
    let fixed = std::sync::Arc::clone(&w.fixed);
    let Some(boxes) = fixed.samplers.get(&ch) else { return };
    // KnockBack (the profile) blocks no trace channel: a channel query skips them.
    let want_kb = !kb.is_empty() && !fi.montages.active_hits.is_empty() && q.channel < 0 && (q.mask & 1) == 1;
    let want_hurt = q.channel < 0 && (q.mask >> OBJ_HURT_BOX) & 1 == 1 && !boxes.hurt.is_empty();
    if !want_kb && !want_hurt {
        return;
    }
    let mut pose = std::mem::take(&mut s.pose);
    pose.clear();
    pose.resize(cd.boxes.stride as usize, 0.0);
    if !crate::fighter::anim::sample_boxes(w, f, &mut pose) {
        s.pose = pose;
        return;
    }
    let fi = w.fighter(f);
    let fc = fi.body.facing_dir;
    let (bx, by) = (fi.body.x + fc * fi.hit.mesh_x, fi.body.y + fi.hit.mesh_z);
    let shapes = &cd.boxes.shapes;
    let at = |o: usize| crate::real(pose[o]);
    if want_kb {
        // The KnockBack spheres a Hitframe has on: its HitBoxTags as GetHitBoxes looks them up (tag + "2D" for a pawn
        // with Use 2D Hitboxes and a tag other than FullBody), each component once.
        s.seen.clear();
        let use_2d = cd.def.use_2d;
        for hit in fi.montages.active_hits.values() {
            let Some(p) = notify_props(d, &w.names, &hit.n) else { continue };
            let scale = p.get("HitBoxScale").and_then(|v| str_num(v, "X")).unwrap_or(1.0);
            for tag in p.get("HitBoxTags").and_then(Value::as_arr).unwrap_or(&[]) {
                let Some(tag) = tag.as_str() else { continue };
                s.tag.clear();
                s.tag.extend(tag.chars().flat_map(char::to_lowercase));
                if use_2d && tag != "FullBody" {
                    s.tag.push_str("2d");
                }
                for &i in boxes.by_tag.get(s.tag.as_str()).map_or(&[][..], Vec::as_slice) {
                    let sh = &shapes[i as usize];
                    let named = w.names.get(&sh.name);
                    if sh.kind != mvs_data::boxes::BoxKind::Hit || !kb.contains(&sh.name) || s.seen.contains(&i) {
                        continue;
                    }
                    s.seen.push(i);
                    if sh.is_box == Some(true) {
                        crate::fighter::common::warn_once(w, name!("traces: a KnockBack box is not handled"));
                        continue;
                    }
                    let info =
                        body(w, fighter_comp(d, &mut s.key, &sh.name), Some(name!("KnockBack"))).expect("KnockBack");
                    if !info.on || !wants(q, info.kind, info.blocks) {
                        continue;
                    }
                    let o = boxes.offsets[i as usize] as usize;
                    let (x, y) = (bx + at(o) * fc, by + at(o + 1));
                    let r = sh.radius * f64::from(scale) * f64::from(at(o + 3));
                    add_round(
                        &mut s.pool,
                        x,
                        y,
                        x,
                        y,
                        crate::math::narrow(r),
                        at(o + 2),
                        &info,
                        named.unwrap_or(Name::NONE),
                        me,
                    );
                }
            }
        }
    }
    if want_hurt {
        for &i in &boxes.hurt {
            let sh = &shapes[i as usize];
            let info = body(w, fighter_comp(d, &mut s.key, &sh.name), Some(name!("HurtBox"))).unwrap_or(HURTBOX_INFO);
            if !info.on || !wants(q, info.kind, info.blocks) {
                continue;
            }
            let ii = i.cast_signed();
            let o = boxes.offsets[i as usize] as usize;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "hurtEnd is 0 or 3")]
            let e = crate::combat::boxes::hurt_end(boxes, ii) as usize;
            let r = crate::combat::boxes::hurt_r(boxes, ii, &pose, o as Real);
            let component = w.names.get(&sh.name).unwrap_or(Name::NONE);
            let (ax, ay) = (bx + at(o) * fc, by + at(o + 1));
            let (ex, ey) = (bx + at(o + e) * fc, by + at(o + e + 1));
            add_round(&mut s.pool, ax, ay, ex, ey, r, Real::NAN, &info, component, me);
        }
    }
    s.pose = pose;
}

/// A member of an object value as a number.
fn str_num(v: &Value, key: &str) -> Option<Real> {
    v.as_obj().and_then(|o| o.get(key)).and_then(Value::as_f64).map(crate::math::narrow)
}

/// A buff child's own sphere on its spawned actor (Buff_C021_SlowReflectProjectile's AdditionalHurtBox, AddComponent
/// of a sphere attached to the root at no offset, object type ProjectileDestructible).
const BUFF_SPHERE: BodyInfo =
    BodyInfo { profile: name!("Custom"), kind: OBJ_PROJECTILE_DESTRUCTIBLE, on: true, blocks: 0 };

fn buff_sphere(w: &mut World, q: &Filter, a: ActorRef, x: Real, y: Real, s: &mut TraceScratch) {
    let r = crate::combat::elem::actor_hurt_sphere(w, a);
    if r > 0.0 && wants(q, BUFF_SPHERE.kind, BUFF_SPHERE.blocks) {
        add_round(&mut s.pool, x, y, x, y, r, Real::NAN, &BUFF_SPHERE, name!("AdditionalHurtBox"), a);
    }
}

/// A world actor's class's `bodies` hook.
fn world_bodies(w: &World, cls: Name) -> Option<fn(&World, crate::actors::ActorId, &mut Vec<TraceBody>)> {
    match w.registry.actors.get(w.names.str(cls)).map(|e| &e.hooks) {
        Some(ActorHooks::World(h)) => h.bodies,
        _ => None,
    }
}

/// `gather`: the bodies the query takes, into the scratch's pool.
fn gather(w: &mut World, q: &Filter, s: &mut TraceScratch) {
    s.pool.clear();
    if q.channel < 0 {
        if q.mask & !GATHERED != 0 {
            crate::fighter::common::warn_once(w, name!("traces: an object type is not gathered"));
        }
    } else if q.channel < 2 {
        crate::fighter::common::warn_once(w, name!("traces: a trace channel is not handled"));
    }
    let data = std::sync::Arc::clone(&w.data);
    // The level: the stage block and its pass-through platforms.
    if !not_only(q, None) {
        let g = w.stage.ground;
        let floor = profile_info(w, data.stage.floor.profile.as_deref().unwrap_or("Floor"));
        if wants(q, floor.kind, floor.blocks) {
            add_box(
                &mut s.pool,
                crate::math::narrow(g.left),
                crate::math::narrow(g.right),
                crate::math::narrow(g.bottom),
                crate::math::narrow(g.top),
                &floor,
                name!("Floor"),
                None,
                -1,
                None,
            );
        }
        let plat = profile_info(w, "Platform");
        if wants(q, plat.kind, plat.blocks) {
            for (i, p) in w.stage.stage_platforms.iter().enumerate() {
                let floor = Some(PlatformRef::Stage(u32::try_from(i).expect("a platform index")));
                add_box(&mut s.pool, p.left, p.right, p.bottom, p.top, &plat, name!("Box"), None, p.stage_index, floor);
            }
        }
        if w.stage_run.as_ref().is_some_and(|r| !r.solids.is_empty()) {
            gather_solids(w, q, s);
        }
        if q.channel < 0 && data.stage.query_bodies.is_some() {
            gather_query_bodies(w, q, s);
        }
        // The run-time Floor boxes (the Platform ones are the stage platforms, above).
        if w.runtime_solids.is_some() {
            let rf = profile_info(w, "Floor");
            if wants(q, rf.kind, rf.blocks) {
                for rs in crate::world::stage::runtime_floor_boxes(w) {
                    if !rs.state.enabled {
                        continue;
                    }
                    let gb = rs.ground();
                    let component = crate::world::stage::runtime_solid_component(w, rs.spec);
                    let fl = Some(PlatformRef::Runtime(rs.spec));
                    add_box(
                        &mut s.pool,
                        crate::math::narrow(gb.left),
                        crate::math::narrow(gb.right),
                        crate::math::narrow(gb.bottom),
                        crate::math::narrow(gb.top),
                        &rf,
                        component,
                        None,
                        -1,
                        fl,
                    );
                }
            }
        }
    }
    // The ships' pieces and the SpawnedPlatformParent boxes with their collision on.
    s.ids.clear();
    s.ids.extend(w.actors.ships.ids().map(ActorRef::Ship));
    for k in 0..s.ids.len() {
        let a = s.ids[k];
        let ActorRef::Ship(id) = a else { continue };
        let sh = w.actors.ships.get(id).expect("a ship of the list");
        if sh.dead || ignored(q, Some(a)) || not_only(q, Some(a)) {
            continue;
        }
        let comps = crate::fighter::common::actor_components(w, ActorRef::Fighter(sh.owner), sh.cls);
        for (j, pc) in sh.pieces.iter().enumerate() {
            if !pc.on {
                continue;
            }
            let Some(info) = body(w, comp_of(comps, w.names.str(pc.name)), None) else { continue };
            if !wants(q, info.kind, info.blocks) {
                continue;
            }
            let (x, y) = (sh.x + pc.dx, sh.y + pc.dy);
            // Deviation: the floor is the ship's piece here, not the ship itself.
            let fl = Some(PlatformRef::Spawned(a, u32::try_from(j).expect("a piece index")));
            add_box(&mut s.pool, x - pc.hw, x + pc.hw, y - pc.hh, y + pc.hh, &info, pc.name, Some(a), -1, fl);
        }
    }
    s.ids.clear();
    s.ids.extend(w.actors.actors.ids().map(ActorRef::Actor));
    for k in 0..s.ids.len() {
        let a = s.ids[k];
        let ActorRef::Actor(id) = a else { continue };
        let wa = w.actors.actors.get(id).expect("an actor of the list");
        if wa.dead || ignored(q, Some(a)) || not_only(q, Some(a)) {
            continue;
        }
        let (owner, cls) = (wa.owner, wa.cls);
        if !crate::world::spawned_platform::spawned_solids(w, a, &mut s.parts) {
            continue;
        }
        let comps = crate::fighter::common::actor_components(w, ActorRef::Fighter(owner), cls);
        for j in 0..s.parts.len() {
            let (on, name) = s.parts[j];
            if !on {
                continue;
            }
            let Some(info) = body(w, comp_of(comps, w.names.str(name)), None) else { continue };
            if !wants(q, info.kind, info.blocks) {
                continue;
            }
            let j = u32::try_from(j).expect("a part index");
            let p = crate::world::spawned_platform::spawned_plat(w, a, j);
            let fl = Some(PlatformRef::Spawned(a, j));
            add_box(&mut s.pool, p.left, p.right, p.bottom, p.top, &info, name, Some(a), -1, fl);
        }
    }
    // The fighters, the minions too.
    s.ids.clear();
    s.ids.extend(w.pawns.as_ref().unwrap_or(&w.players).iter().map(|&f| ActorRef::Fighter(f)));
    for k in 0..s.ids.len() {
        let a = s.ids[k];
        let ActorRef::Fighter(f) = a else { continue };
        if !not_only(q, Some(a)) {
            gather_fighter(w, f, q, s);
        }
    }
    s.ids.clear();
    s.ids.extend(w.actors.projectiles.ids().map(ActorRef::Projectile));
    for k in 0..s.ids.len() {
        let a = s.ids[k];
        let ActorRef::Projectile(id) = a else { continue };
        gather_projectile(w, q, s, id);
    }
    s.ids.clear();
    s.ids.extend(w.actors.items.ids().map(ActorRef::Item));
    for k in 0..s.ids.len() {
        let a = s.ids[k];
        let ActorRef::Item(id) = a else { continue };
        gather_item(w, q, s, id);
    }
    // C019_MortarAim's Hurtbox (DamageDestructible, QueryAndPhysics, on for the actor's life: MortarAim's capsule under
    // the Aim). Its other components are a root Capsule with no collision data and the Aim (NoCollision).
    s.ids.clear();
    s.ids.extend(w.actors.mortars.ids().map(ActorRef::Mortar));
    for k in 0..s.ids.len() {
        let a = s.ids[k];
        let ActorRef::Mortar(id) = a else { continue };
        let mo = w.actors.mortars.get(id).expect("a mortar of the list");
        if mo.dead || mo.hurt_r <= 0.0 || ignored(q, Some(a)) || not_only(q, Some(a)) {
            continue;
        }
        let comps = crate::fighter::common::actor_components(w, ActorRef::Fighter(mo.owner), mo.cls);
        let Some(hb) = body(w, comp_of(comps, "Hurtbox"), None) else { continue };
        if hb.on && wants(q, hb.kind, hb.blocks) {
            let cy = mo.y + mo.hurt_up;
            let aim_x = mo.x + mo.offset * mo.facing;
            let (seg, r) = (mo.hurt_seg, mo.hurt_r);
            add_round(&mut s.pool, aim_x, cy - seg, aim_x, cy + seg, r, Real::NAN, &hb, name!("Hurtbox"), a);
        }
    }
    s.ids.clear();
    s.ids.extend(w.actors.actors.ids().map(ActorRef::Actor));
    for k in 0..s.ids.len() {
        let a = s.ids[k];
        let ActorRef::Actor(id) = a else { continue };
        let wa = w.actors.actors.get(id).expect("an actor of the list");
        if wa.dead || ignored(q, Some(a)) {
            continue;
        }
        let (owner, cls) = (wa.owner, wa.cls);
        if let Some(hook) = world_bodies(w, cls) {
            hook(w, id, &mut s.sink);
            add_sunk(q, s);
        } else if !not_only(q, Some(a)) {
            let gathered: &[&str] =
                if crate::world::spawned_platform::spawned_solids(w, a, &mut s.parts) { &SOLIDS_GATHERED } else { &[] };
            let comps = crate::fighter::common::actor_components(w, ActorRef::Fighter(owner), cls);
            check_class(w, q, false, comps, gathered);
        }
    }
}

fn gather_projectile(w: &mut World, q: &Filter, s: &mut TraceScratch, id: ProjectileId) {
    let a = ActorRef::Projectile(id);
    let pr = w.actors.projectiles.get(id).expect("a projectile of the list");
    if pr.dead || ignored(q, Some(a)) {
        return;
    }
    let hook = crate::registry::entries::spawned_bodies_hook(w, a);
    if not_only(q, Some(a)) {
        if let Some(h) = hook {
            h(w, a, &mut s.sink);
            add_sunk(q, s);
        }
        return;
    }
    let owner = crate::fighter::common::lookup_owner(w, a);
    let pr = w.actors.projectiles.get(id).expect("a projectile of the list");
    let comps = crate::fighter::common::actor_components(w, ActorRef::Fighter(owner), pr.cls);
    if let Some(root) = body(w, comp_of(comps, "Capsule"), None)
        && root_query_on(pr, &root)
        && wants(q, root.kind, root.blocks)
    {
        let (r, hh) = (pr.cap_r * pr.scale, pr.cap_h * pr.scale);
        add_capsule(&mut s.pool, pr.x, pr.y, r, hh, &root, name!("Capsule"), a);
    }
    // The HurtBox is on while hurtRadius > 0, as the world's hit tests take it.
    let hurt = if pr.hurt_radius > 0.0 { body(w, comp_of(comps, "HurtBox"), None) } else { None };
    if let Some(hurt) = hurt
        && hurt.on
        && wants(q, hurt.kind, hurt.blocks)
    {
        let r = pr.hurt_radius;
        // A class with a HurtBox of another shape enters its bodies itself.
        match crate::actors::projectile::hurt_core(w, id) {
            Some((hx, hy)) => add_round(&mut s.pool, hx, hy, hx, hy, r, Real::NAN, &hurt, name!("HurtBox"), a),
            None if hook.is_none() => {
                crate::fighter::common::warn_once(w, name!("traces: a HurtBox of its own shape is not entered"));
            }
            None => {}
        }
    }
    if let Some(h) = hook {
        h(w, a, &mut s.sink);
        add_sunk(q, s);
    }
    // (checkClass only warns: ahead of the buff sphere, which borrows the World)
    check_class(w, q, hook.is_some(), comps, &PROJ_GATHERED);
    let pr = w.actors.projectiles.get(id).expect("a projectile of the list");
    let (x, y, has_elem) = (pr.x, pr.y, pr.elem.is_some());
    if has_elem {
        buff_sphere(w, q, a, x, y, s);
    }
}

fn gather_item(w: &mut World, q: &Filter, s: &mut TraceScratch, id: ItemId) {
    let a = ActorRef::Item(id);
    let it = w.actors.items.get(id).expect("an item of the list");
    if it.dead || it.holder >= 0 || ignored(q, Some(a)) {
        return;
    }
    let hook = crate::registry::entries::spawned_bodies_hook(w, a);
    if not_only(q, Some(a)) {
        if let Some(h) = hook {
            h(w, a, &mut s.sink);
            add_sunk(q, s);
        }
        return;
    }
    let comps = crate::world::items::item_components(w, id);
    let it = w.actors.items.get(id).expect("an item of the list");
    let no_collision = it.no_collision == Some(true);
    if let Some(root) = body(w, comp_of(comps, "Capsule"), None)
        && root.on
        && wants(q, root.kind, root.blocks)
    {
        add_capsule(&mut s.pool, it.x, it.y, it.capsule_radius(), it.capsule_half_height(), &root, name!("Capsule"), a);
    }
    // The Sphere a hitbox can overlap (`hurtRadius`: 0 while it is off or the item is in a hand).
    let hurt_radius = it.hurt_radius();
    let sp = if hurt_radius > 0.0 { body(w, comp_of(comps, "Sphere"), None) } else { None };
    if let Some(sp) = sp
        && wants(q, sp.kind, sp.blocks)
    {
        add_round(&mut s.pool, it.x, it.y, it.x, it.y, hurt_radius, Real::NAN, &sp, name!("Sphere"), a);
    }
    if !no_collision
        && let Some(pa) = body(w, comp_of(comps, "PickupArea"), None)
        && pa.on
        && wants(q, pa.kind, pa.blocks)
        && let Some((px, py, pr)) = crate::actors::item::pickup_area(w, id)
    {
        add_round(&mut s.pool, px, py, px, py, pr, Real::NAN, &pa, name!("PickupArea"), a);
    }
    if let Some(h) = hook {
        h(w, a, &mut s.sink);
        add_sunk(q, s);
    }
    check_class(w, q, hook.is_some(), comps, &ITEM_GATHERED);
    let it = w.actors.items.get(id).expect("an item of the list");
    let (x, y, has_elem) = (it.x, it.y, it.elem.is_some());
    if has_elem && crate::actors::item::is_owned(w, id) {
        buff_sphere(w, q, a, x, y, s);
    }
}

/// The stage's solids (`gatherSolids`) by their own collision: the profile's object type, the channels the data's
/// flags block (WallJump 2, ProjectileGroundTrace 3, ProjectileExplode 4, ProjectileGroundNoPlatform 5).
fn gather_solids(w: &mut World, q: &Filter, s: &mut TraceScratch) {
    let data = std::sync::Arc::clone(&w.data);
    let sd = data.stage.solids.as_deref().unwrap_or(&[]);
    let all = stage_solids(w);
    let run = w.stage_run.as_ref().expect("the stage's run");
    for x in &all.all {
        let st = &run.solids[x.solid() as usize];
        if !st.enabled {
            continue;
        }
        let d = &sd[usize::try_from(st.id).expect("a solid's data id")];
        let info = solid_info(w, d);
        let skip = if q.channel < 0 { info.kind < 0 || !wants(q, info.kind, 0) } else { !wants(q, 0, info.blocks) };
        if skip {
            continue;
        }
        let comp = w.names.get(&d.name[d.name.rfind('.').map_or(0, |i| i + 1)..]).unwrap_or(Name::NONE);
        match x {
            Solid::Box(i, b) => {
                let g = solid_box_ground(w, b);
                let fl = i.map(PlatformRef::Solid);
                add_box(
                    &mut s.pool,
                    crate::math::narrow(g.left),
                    crate::math::narrow(g.right),
                    crate::math::narrow(g.bottom),
                    crate::math::narrow(g.top),
                    &info,
                    comp,
                    None,
                    -1,
                    fl,
                );
            }
            Solid::Poly(p) => {
                let mut corners = Vec::with_capacity(p.poly.xs.len() * 2);
                for (cx, cy) in p.poly.xs.iter().zip(&p.poly.ys) {
                    corners.push(*cx);
                    corners.push(*cy);
                }
                s.pool.push(TraceBody {
                    ax: st.m.dx,
                    ay: st.m.dy,
                    bx: st.m.dx,
                    by: st.m.dy,
                    hx: 0.0,
                    hy: 0.0,
                    r: 0.0,
                    z: Real::NAN,
                    object_type: info.kind,
                    profile: info.profile,
                    component: comp,
                    actor: None,
                    platform: -1,
                    blocks: info.blocks,
                    poly: Some(corners),
                    floor: Some(PlatformRef::Poly(p.solid)),
                });
            }
        }
    }
}

/// `solidInfo`: a stage solid's body.
fn solid_info(w: &World, d: &mvs_data::stage::StageSolidData) -> BodyInfo {
    use crate::consts::{SOLID_EXPLODE, SOLID_NOPLATFORM, SOLID_ONE_WAY, SOLID_TRACE, SOLID_WALLJUMP};
    let mut blocks = 0;
    for (bit, ch) in [(SOLID_WALLJUMP, 2), (SOLID_TRACE, 3), (SOLID_EXPLODE, 4), (SOLID_NOPLATFORM, 5)] {
        if d.flags & bit != 0 {
            blocks |= 1 << ch;
        }
    }
    // (a one-way solid is object type Platform: the stage data sets ONE_WAY from it)
    let kind = if d.flags & SOLID_ONE_WAY != 0 {
        OBJ_PLATFORM
    } else if d.profile == "Custom" {
        -1
    } else {
        profile_info(w, &d.profile).kind
    };
    if kind < 0 {
        crate::fighter::common::warn_once(w, name!("traces: a stage solid with no object type in the data"));
    }
    BodyInfo { profile: profile_name(&d.profile), kind, on: true, blocks }
}

/// The level's query-only boxes (`gatherQueryBodies`: StageData.queryBodies) by their object type; they block no
/// channel. Sim: x = -UE Y, y = UE Z, on the world's axes (the bind refuses a turned one).
fn gather_query_bodies(w: &World, q: &Filter, s: &mut TraceScratch) {
    for d in w.data.stage.query_bodies.as_deref().unwrap_or(&[]) {
        let kind = OBJ_NAMES.iter().position(|n| *n == d.object_type).map_or(-1, |i| i32::try_from(i).unwrap_or(-1));
        if kind < 0 {
            crate::fighter::common::warn_once(w, name!("traces: a stage query body of an object type the port lacks"));
            continue;
        }
        if !wants(q, kind, 0) {
            continue;
        }
        let info = BodyInfo { profile: profile_name(&d.profile), kind, on: true, blocks: 0 };
        let (x, y, hx, hy) = (-d.center[1], d.center[2], d.half_extent[1], d.half_extent[2]);
        let comp = w.names.get(&d.name[d.name.rfind('.').map_or(0, |i| i + 1)..]).unwrap_or(Name::NONE);
        add_box(
            &mut s.pool,
            crate::math::narrow(x - hx),
            crate::math::narrow(x + hx),
            crate::math::narrow(y - hy),
            crate::math::narrow(y + hy),
            &info,
            comp,
            None,
            -1,
            None,
        );
    }
}

// ---- the sweeps and the calls

/// SweepSingleByChannel 14294e050: a sphere of radius 1e-4 or less is a line trace (`LINE_RADIUS`); the shape
/// adapter's box: KINDA_SMALL_NUMBER as float (DAT_143b05c98 = 9.9999997e-5, `MIN_EXTENT`).
const MIN_EXTENT: Real = 1e-4_f32 as Real;

/// The adapter's half extent: max(c, 1e-4), a negative one too (FPhysicsShapeAdapter 142122420).
fn box_half(c: Real) -> Real {
    crate::math::max(c, MIN_EXTENT)
}

/// SweepMultiByObjectType 14294dca0: a box with all three half extents <= 1e-4 (IsNearlyZero, signed) is a line.
fn nearly_zero_box(hx: Real, hy: Real, depth: Real) -> bool {
    hx <= MIN_EXTENT && hy <= MIN_EXTENT && depth <= MIN_EXTENT
}

#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn query(x0: Real, y0: Real, x1: Real, y1: Real, qx: Real, qy: Real, qr: Real, qd: Real) -> Query {
    Query {
        x0: f64::from(x0),
        y0: f64::from(y0),
        dx: f64::from(x1) - f64::from(x0),
        dy: f64::from(y1) - f64::from(y0),
        qx: f64::from(qx),
        qy: f64::from(qy),
        qr: f64::from(qr),
        qd: f64::from(qd),
    }
}

/// The box query of a trace (`boxTraceSingleForObjects`' and `boxTraceSingle`'s).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
fn box_query(x0: Real, y0: Real, x1: Real, y1: Real, hx: Real, hy: Real, depth: Real) -> Query {
    if nearly_zero_box(hx, hy, depth) {
        query(x0, y0, x1, y1, 0.0, 0.0, 0.0, 0.0)
    } else {
        query(x0, y0, x1, y1, box_half(hx), box_half(hy), 0.0, box_half(depth))
    }
}

/// `boxOverlapQuery`: a sphere of radius 1e-4 when all three are <= 1e-4.
fn box_overlap_query(x: Real, y: Real, hx: Real, hy: Real, depth: Real) -> Query {
    if nearly_zero_box(hx, hy, depth) {
        query(x, y, x, y, 0.0, 0.0, MIN_EXTENT, 0.0)
    } else {
        query(x, y, x, y, box_half(hx), box_half(hy), 0.0, box_half(depth))
    }
}

/// `sweepAll`: the hits of the gathered bodies, sorted by Time (stable; a non-blocking hit ahead of a blocking one at
/// equal Time).
fn sweep_all(q: &Query, s: &mut TraceScratch, out: &mut Vec<TraceHit>) {
    out.clear();
    for b in &s.pool {
        let shape = Shape {
            ax: f64::from(b.ax),
            ay: f64::from(b.ay),
            bx: f64::from(b.bx),
            by: f64::from(b.by),
            hx: f64::from(b.hx),
            hy: f64::from(b.hy),
            r: f64::from(b.r),
            z: f64::from(b.z),
            poly: b.poly.as_deref(),
        };
        let Some(h) = s.tracer.sweep(q, &shape) else { continue };
        let h = TraceHit {
            time: crate::math::narrow(h.time),
            distance: crate::math::narrow(h.distance),
            blocking: h.blocking,
            initial: h.initial,
            x: crate::math::narrow(h.x),
            y: crate::math::narrow(h.y),
            ix: crate::math::narrow(h.ix),
            iy: crate::math::narrow(h.iy),
            nx: crate::math::narrow(h.nx),
            ny: crate::math::narrow(h.ny),
            depth: crate::math::narrow(h.depth),
            object_type: b.object_type,
            profile: b.profile,
            component: b.component,
            actor: b.actor,
            platform: b.platform,
            floor: b.floor,
        };
        let mut k = out.len();
        out.push(h);
        while k > 0 && (out[k - 1].time > h.time || (out[k - 1].time == h.time && out[k - 1].blocking && !h.blocking)) {
            out[k] = out[k - 1];
            k -= 1;
        }
        out[k] = h;
    }
}

/// `maskOf`: the object types as bits.
fn mask_of(types: &[Real]) -> i32 {
    types.iter().fold(0, |m, &t| m | 1i32.wrapping_shl(crate::math::to_int32(t).cast_unsigned()))
}

/// Runs a query with the World's trace scratch: gathers by `filter`, sweeps `q`, and hands the hits to `then`.
fn run<T>(
    w: &mut World,
    filter: &Filter,
    q: &Query,
    out: Option<&mut Vec<TraceHit>>,
    then: impl FnOnce(&World, &[TraceHit]) -> T,
) -> T {
    let mut s = w.scratch.traces.take().unwrap_or_default();
    gather(w, filter, &mut s);
    let mut hits = std::mem::take(&mut s.hits);
    let list = match out {
        Some(o) => {
            sweep_all(q, &mut s, o);
            let r = then(w, o);
            s.hits = hits;
            w.scratch.traces = Some(s);
            return r;
        }
        None => {
            sweep_all(q, &mut s, &mut hits);
            &hits
        }
    };
    let r = then(w, list);
    s.hits = hits;
    w.scratch.traces = Some(s);
    r
}

fn objects<'a>(types: &[Real], ignore: &'a [Option<ActorRef>], own: Option<ActorRef>, responder: i32) -> Filter<'a> {
    Filter { mask: mask_of(types), channel: -1, ignore, own, only: None, responder }
}

fn channel(ch: i32, ignore: &[Option<ActorRef>], own: Option<ActorRef>) -> Filter<'_> {
    Filter { mask: 0, channel: ch, ignore, own, only: None, responder: -1 }
}

/// `capsuleTraceMultiForObjects`: CapsuleTraceMultiForObjects(WorldContext, Start, End,
/// Radius, HalfHeight, ObjectTypes, bTraceComplex, ActorsToIgnore, ..., OutHits, bIgnoreSelf): an upright capsule
/// (the scaled size) from start to end; `own`: the bIgnoreSelf actor. The hits sorted by Time, written to `out` (its
/// allocation reused).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn capsule_trace_multi_for_objects(
    w: &mut World,
    x0: Real,
    y0: Real,
    x1: Real,
    y1: Real,
    radius: Real,
    half_height: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
    own: Option<ActorRef>,
    out: &mut Vec<TraceHit>,
) {
    let q = query(x0, y0, x1, y1, 0.0, crate::math::max(half_height - radius, 0.0), radius, 0.0);
    run(w, &objects(types, ignore, own, -1), &q, Some(out), |_, _| ());
}

/// `sphereTraceMultiForObjects`: the same with a sphere (Start = End: every body it
/// starts in, Time 0).
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn sphere_trace_multi_for_objects(
    w: &mut World,
    x0: Real,
    y0: Real,
    x1: Real,
    y1: Real,
    radius: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
    own: Option<ActorRef>,
    out: &mut Vec<TraceHit>,
) {
    let q = query(x0, y0, x1, y1, 0.0, 0.0, radius, 0.0);
    run(w, &objects(types, ignore, own, -1), &q, Some(out), |_, _| ());
}

/// `boxTraceSingleForObjects`: BoxTraceSingleForObjects(..., HalfSize, Orientation,
/// ObjectTypes, ...): a box of half extents hx (along sim x, UE Y), hy (UE Z) and depth (UE X) with the identity
/// orientation; the first hit, `None` for none.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn box_trace_single_for_objects(
    w: &mut World,
    x0: Real,
    y0: Real,
    x1: Real,
    y1: Real,
    hx: Real,
    hy: Real,
    depth: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
    own: Option<ActorRef>,
) -> Option<TraceHit> {
    let q = box_query(x0, y0, x1, y1, hx, hy, depth);
    run(w, &objects(types, ignore, own, -1), &q, None, |_, h| h.first().copied())
}

/// `sphereTraceSingle`: SphereTraceSingle(..., Radius, TraceChannel, ...): the first body
/// that blocks the channel (2 WallJump, 3 ProjectileGroundTrace, 4 ProjectileExplode, 5 ProjectileGroundNoPlatform;
/// Visibility and Camera are not handled), `None` for none.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn sphere_trace_single(
    w: &mut World,
    x0: Real,
    y0: Real,
    x1: Real,
    y1: Real,
    radius: Real,
    ch: i32,
    ignore: &[Option<ActorRef>],
    own: Option<ActorRef>,
) -> Option<TraceHit> {
    let q = query(x0, y0, x1, y1, 0.0, 0.0, radius, 0.0);
    run(w, &channel(ch, ignore, own), &q, None, |_, h| h.first().copied())
}

/// `capsuleTraceSingle`: CapsuleTraceSingle(..., Radius, HalfHeight, TraceChannel, ...):
/// an upright capsule against the bodies that block the channel, as `sphere_trace_single`.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn capsule_trace_single(
    w: &mut World,
    x0: Real,
    y0: Real,
    x1: Real,
    y1: Real,
    radius: Real,
    half_height: Real,
    ch: i32,
    ignore: &[Option<ActorRef>],
    own: Option<ActorRef>,
) -> Option<TraceHit> {
    let q = query(x0, y0, x1, y1, 0.0, crate::math::max(half_height - radius, 0.0), radius, 0.0);
    run(w, &channel(ch, ignore, own), &q, None, |_, h| h.first().copied())
}

/// `boxTraceSingle`: BoxTraceSingle(..., HalfSize, Orientation, TraceChannel, ...): the
/// box of `box_trace_single_for_objects` against the bodies that block the channel.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn box_trace_single(
    w: &mut World,
    x0: Real,
    y0: Real,
    x1: Real,
    y1: Real,
    hx: Real,
    hy: Real,
    depth: Real,
    ch: i32,
    ignore: &[Option<ActorRef>],
    own: Option<ActorRef>,
) -> Option<TraceHit> {
    let q = box_query(x0, y0, x1, y1, hx, hy, depth);
    run(w, &channel(ch, ignore, own), &q, None, |_, h| h.first().copied())
}

/// `sphereOverlapActors`: SphereOverlapActors (ObjectTypes, ActorClassFilter null,
/// ActorsToIgnore): the actors with a body of the types at the shape, each once, in the order of their first body
/// (unverified: PhysX's order); the level's own bodies have no actor and are not listed. `responder`: -1 for none.
pub fn sphere_overlap_actors(
    w: &mut World,
    x: Real,
    y: Real,
    radius: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
    responder: Real,
) -> Vec<ActorRef> {
    let responder = crate::math::to_int32(responder);
    let q = query(x, y, x, y, 0.0, 0.0, radius, 0.0);
    run(w, &objects(types, ignore, None, responder), &q, None, |w, h| actors_of(w, h, responder))
}

/// `boxOverlapActors`: the box world-axis aligned (BoxOverlapComponents 142b0f650 passes
/// FQuat::Identity), each half extent clamped to >= 1e-4 by the shape adapter, a sphere of radius 1e-4 when all three
/// are <= 1e-4.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn box_overlap_actors(
    w: &mut World,
    x: Real,
    y: Real,
    hx: Real,
    hy: Real,
    depth: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
    responder: Real,
) -> Vec<ActorRef> {
    let responder = crate::math::to_int32(responder);
    let q = box_overlap_query(x, y, hx, hy, depth);
    run(w, &objects(types, ignore, None, responder), &q, None, |w, h| actors_of(w, h, responder))
}

/// `boxOverlapsActor`: whether `box_overlap_actors` lists this actor, with only its own
/// bodies gathered.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn box_overlaps_actor(
    w: &mut World,
    x: Real,
    y: Real,
    hx: Real,
    hy: Real,
    depth: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
    responder: Real,
    actor: ActorRef,
) -> bool {
    let responder = crate::math::to_int32(responder);
    let f = Filter { mask: mask_of(types), channel: -1, ignore, own: None, only: Some(actor), responder };
    let q = box_overlap_query(x, y, hx, hy, depth);
    run(w, &f, &q, None, |w, h| actors_of(w, h, responder).contains(&actor))
}

/// `sphereOverlapsActor`: the same for `sphere_overlap_actors`.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn sphere_overlaps_actor(
    w: &mut World,
    x: Real,
    y: Real,
    radius: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
    responder: Real,
    actor: ActorRef,
) -> bool {
    let responder = crate::math::to_int32(responder);
    let f = Filter { mask: mask_of(types), channel: -1, ignore, own: None, only: Some(actor), responder };
    let q = query(x, y, x, y, 0.0, 0.0, radius, 0.0);
    run(w, &f, &q, None, |w, h| actors_of(w, h, responder).contains(&actor))
}

/// `capsuleXOverlapActors`: the overlap of a capsule lying along sim x (a
/// CapsuleComponent pitched 90: BugsBunnyTunnelParent's ProjectileOverlap): a sphere of radius r over the segment
/// x +- half, at y; the actors as `sphere_overlap_actors`.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn capsule_x_overlap_actors(
    w: &mut World,
    x: Real,
    y: Real,
    half: Real,
    radius: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
) -> Vec<ActorRef> {
    let q = query(x, y, x, y, half, 0.0, radius, 0.0);
    run(w, &objects(types, ignore, None, -1), &q, None, |w, h| actors_of(w, h, -1))
}

/// `capsuleOverlapActors`: CapsuleOverlapActors(WorldContext, CapsulePos, Radius,
/// HalfHeight, ObjectTypes, ActorClassFilter None, ActorsToIgnore): an upright capsule (half height measured to the
/// cap's end), the actors as `sphere_overlap_actors`.
#[allow(clippy::too_many_arguments, reason = "the call's parameters")]
pub fn capsule_overlap_actors(
    w: &mut World,
    x: Real,
    y: Real,
    radius: Real,
    half_height: Real,
    types: &[Real],
    ignore: &[Option<ActorRef>],
) -> Vec<ActorRef> {
    let q = query(x, y, x, y, 0.0, crate::math::max(half_height - radius, 0.0), radius, 0.0);
    run(w, &objects(types, ignore, None, -1), &q, None, |w, h| actors_of(w, h, -1))
}

/// `actorsOf`: the hits' actors, each once. responder: for a component's overlap events in place of the query (a
/// begin overlap needs both sides at Overlap or more, GetCollisionResponseToComponent's minimum), the object type of
/// the querying component: a body whose template answers it with Ignore is dropped (its actor stays when another of
/// its bodies answers). The template is the spawned actor's class component of the hit's name; a fighter's bodies and
/// the level's are kept (unverified: their responses are not read; the Pawn and HurtBox profiles overlap
/// WorldDynamic).
fn actors_of(w: &World, hits: &[TraceHit], responder: i32) -> Vec<ActorRef> {
    let mut out: Vec<ActorRef> = Vec::new();
    for h in hits {
        let Some(a) = h.actor else { continue };
        if out.contains(&a) {
            continue;
        }
        if responder >= 0
            && let Some(t) = template_of(w, h)
            && response_to(Some(t), crate::real(responder)) == name!("ECR_Ignore")
        {
            continue;
        }
        out.push(a);
    }
    out
}

/// `templateOf`: the class component of a hit's body: a spawned actor's (world actors, projectiles, items, ships,
/// mortars) with an owner, else `None` (a fighter's, the level's).
fn template_of<'a>(w: &'a World, h: &TraceHit) -> Option<&'a Props> {
    let (owner, cls) = match h.actor? {
        ActorRef::Projectile(id) => {
            let p = w.actors.projectiles.get(id)?;
            (p.owner?, p.cls)
        }
        ActorRef::Item(id) => {
            let it = w.actors.items.get(id)?;
            (it.owner?, it.cls)
        }
        ActorRef::Actor(id) => {
            let a = w.actors.actors.get(id)?;
            (a.owner, a.cls)
        }
        ActorRef::Ship(id) => {
            let a = w.actors.ships.get(id)?;
            (a.owner, a.cls)
        }
        ActorRef::Mortar(id) => {
            let a = w.actors.mortars.get(id)?;
            (a.owner, a.cls)
        }
        _ => return None,
    };
    comp_of(crate::fighter::common::actor_components(w, ActorRef::Fighter(owner), cls), w.names.str(h.component))
}

// ---- standing edge

/// The character's anim sets use the edge idle (`readsEdge`): the one listener of EdgeDetection, Base_AnimBP's
/// OnEdgeDetection (bound @27608), reads OnEdge && UseEdgeIdle.
fn reads_edge(w: &World, f: FighterId) -> bool {
    let d = crate::fighter::common::data_of(w, f);
    d.def_extra.anim.edge_idle.is_some()
        || d.def_extra.anim_sets.as_ref().is_some_and(|m| m.values().any(|a| a.edge_idle.is_some()))
}

/// `standingOnEdge`: BaseFighter StandingOnEdge (ReceiveTick @35508 -> @36489; ubergraph @164148-164287, then
/// @162726-164143): while walking, two LineTraceSingle (ProjectileGroundTrace) 20 straight down from the actor
/// location - (0, 0, CapsuleHalfHeight) +/- the forward x GetScaledCapsuleRadius / 2: in front, then behind on a hit;
/// OnEdge = no floor in front or none behind. CapsuleHalfHeight is the root capsule's unscaled property (the crouched
/// one while crouched).
pub fn standing_on_edge(w: &mut World, f: FighterId) {
    if !w.fighter(f).body.on_floor || !reads_edge(w, f) {
        return;
    }
    let fi = w.fighter(f);
    let half = if fi.capsule.cmc_crouched {
        crate::math::max(crate::math::narrow(crate::fighter::body::def(w, f).crouched_half), fi.capsule.cap_radius_u)
    } else {
        fi.capsule.cap_half_u
    };
    let y0 = crate::fighter::body::actor_y(w, f) - half;
    let y1 = y0 - 20.0;
    let dx = fi.body.facing_dir * (crate::fighter::body::cap_radius(w, f) / 2.0);
    let x = fi.body.x;
    let ch = TRACE_PROJECTILE_GROUND;
    let me = Some(ActorRef::Fighter(f));
    let front = sphere_trace_single(w, x + dx, y0, x + dx, y1, 0.0, ch, &[], me).is_some();
    let edge = !front || sphere_trace_single(w, x - dx, y0, x - dx, y1, 0.0, ch, &[], me).is_none();
    let fi = w.fighter_mut(f);
    if edge != fi.platforms.on_edge.unwrap_or(false) {
        fi.platforms.on_edge = Some(edge);
    }
}

#[cfg(test)]
mod tests;
