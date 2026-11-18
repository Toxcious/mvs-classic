//! Wonder Woman's perks: Buff_Perk_WonderWoman_FastFallSlam (the landing's slam, its
//! buff actor's own hit handler as the hits' dealer: `BuffExt::FastFallSlam`), Buff_Perk_WonderWoman_ProjectileBlock
//! (`BuffExt::ProjectileBlock`, its evade's spawn) and WW_EvadeProjectileBlock_C ([`EvadeShield`], a ShieldActor
//! child with its own meets / meetsComp / blockVisuals: [`EVADE`]).
//!
//! Sources: Buff_Perk_WonderWoman_FastFallSlam, Buff_Perk_WonderWoman_ProjectileBlock, WW_EvadeProjectileBlock
//! (offsets are their ubergraphs'); class defaults from the wonderwoman perk part (the buffs, and the evade shield's
//! whole class data in its "actors", which `actor_def` falls back to).
//!
//! The shared code reaches the slam's buff actor as a dealer (`combat::hits` dealer, `BuffExt::dodge_mut` /
//! `actor_owner`: [`slam_dodge_mut`], [`slam_owner`]) and the evade shield's place (`actors::world_actor`:
//! [`evade_place`]).

use std::sync::Arc;

use indexmap::IndexSet;

use crate::Real;
use crate::State;
use crate::actors::projectile::{capsule_half_height, capsule_radius};
use crate::actors::shield_actor::{self, Placed, ShieldActor, ShieldHooks};
use crate::actors::world_actor::{WorldActor, WorldActorState};
use crate::actors::{ActorId, ActorRef, ProjectileId, WorldActorClass};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{BuffExt, buff_of, inst_view};
use crate::combat::elem::instigator_charge;
use crate::combat::hits::{HitShape, apply_projectile_hit, dealer, hit_box_actor_hits};
use crate::combat::phases::{alive, give_vbuf, los_blocked, no_hit, victim_pose};
use crate::combat::{BuffClass, BuffRef, HitPacket};
use crate::events::{Event, NotifyRef};
use crate::fighter::common::{actor_def, latent_frames, timer_frames, warn_once};
use crate::fighter::control::{
    DodgeState, attack_dodged, dodge_over, dodge_tick, trigger_ability, trigger_ability_check,
};
use crate::fighter::match_side::{Foreign, SpawnRequest};
use crate::fighter::moves::V2;
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_ITEM, OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PROJECTILE_DESTRUCTIBLE,
    OBJ_WORLD_DYNAMIC, OBJ_WORLD_STATIC, body_info, response_to, root_query_on,
};
use crate::fighter::{FighterId, actor_y, facing, invulnerable};
use crate::math::ue::rot_from_x;
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffHooks};
use crate::registry::props::{buff, buff_mut, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::match_fighters;
use mvs_data::value::{Props, Value};

const EVADE_SHIELD: &str = "WW_EvadeProjectileBlock_C";

/// `HITBOX_PROFILE_TYPES`: the object types a hit box of the HitBox profile overlaps
/// (Pawn, DamageDestructible, ProjectileDestructible, ItemDestructible), as `hit_box_actor_hits` takes them.
pub const HITBOX_PROFILE_TYPES: [Real; 4] = [
    OBJ_PAWN as Real,
    OBJ_DAMAGE_DESTRUCTIBLE as Real,
    OBJ_PROJECTILE_DESTRUCTIBLE as Real,
    OBJ_ITEM_DESTRUCTIBLE as Real,
];

// ---- Buff_Perk_WonderWoman_FastFallSlam
// ReceiveBeginPlay @495 -> @85 (authority, the owner a BP_FighterCharacter): binds the owner's FastFallLanded @233
// (BaseFighter's broadcast on the landing of a fast fall: the BuffSignal FastFallLanded with the height
// FastFallStartLocation.Z - the actor's Z; `fighter/moves/jump.rs` fast_fall_landed_binder lists the class), copies the
// owner's TeamIndex into the buff's own Comp_Actor_Destructible @325 and attaches the buff to the owner's
// CapsuleComponent @433 (location and rotation snapped: the buff's root is the owner's actor location and facing).
// Fast Fall Slam(height) @510: height > MinHeight (200) @548, else nothing. scale = Lerp(0.6, 1.0, (FMax(FMin(height,
// MaxHeight 1000), MinHeight) - MinHeight) / (MaxHeight - MinHeight)) @558-788, float32, computed twice alike.
//   Fast Fall Slam Client(scale) @835 (multicast; not on a dedicated server @1370): SlamVFX (Template
//     PS_CreatureCrystal_Impact, at the buff's root) SetWorldScale3D(scale) @1443, Activate(true) @1484.
//   The buff's own Comp_Pawn_HitBoxHandler1.HitBoxesOn([HitBox], HitBoxInfo, (scale, scale, scale)) @1196: the
//     component tagged HitBox is DamageSphere (SphereRadius 400 at the root, class data), so a sphere of 400 x scale
//     around her actor location; HitBoxInfo Damage 4, BaseKnockBack 1000, KnockBackScaling 12, direction (0.5,
//     0.15), DirectionalKnockback, VelocityInsteadOfFacing, DoLOSCheck, HitEachActorOnlyOnce, HitReaction 2, no hit
//     pause.
//   Delay(0.066) @1315 -> @15 HitBoxesOff([HitBox]) @39 (the hit list cleared). A slam while the Delay is pending
//     turns the boxes on again (the new scale) and does not restart it.
// The hits are the buff actor's own handler: pfgDoDamage with the buff as AttackingActor (`combat::hits` dealer of a
// buff actor), the victim's DamageAndLaunch through apply_projectile_hit as Buff_DelayKnockUp's (chars/batman/bomb.rs).
// DirectionalKnockback: the knockback points from the sphere's centre (her actor location) to the victim's capsule,
// whatever her facing. VelocityInsteadOfFacing reaches only the projectile hit direction's BaseKnockBack Lerp, which a
// buff actor's dealer (no velocity) skips.
// Deviation: the overlaps are tested in the frame's timer pass (the buffs' `timer` hook), from the landing's frame
// through the frame before the Delay's tick turns the boxes off, not inside her movement tick where the build's
// HitBoxesOn makes them (UpdateOverlaps): a fighter that ticks after her in the landing's frame is hit after its own
// tick, and the boxes' later overlaps are tested once a frame at her place of that frame.
// The handler's hits on the actors that are no fighter (DamageSphere's HitBox profile overlaps the projectiles'
// HurtBox, the items' Sphere, the mortar aim's Hurtbox and the actors' bodies of those types): hit_box_actor_hits,
// behind the fighters, the begins of an overlap kept per buff (over) and the hit list (once) cleared with the hit list.
// Not ported: the handler's ally pass (HitBoxInfo has no On Hit Buffs) and NotifyAllyOfHit for a team mate.
// Unverified: VelocityInsteadOfFacing takes the attached buff actor's velocity as zero (AActor::GetVelocity of a root
// with no movement component; not read in the exe): no BaseKnockBack Lerp by a vertical velocity.
// Deviation: SlamVFX is drawn as a one-shot emitter at her actor location of the landing (projectileDeath), not a
// component that follows her for the system's life. The perk is IsUnavailableOffline (in no menu list), so no player
// of the port wears it and no VFX part holds PS_CreatureCrystal_Impact for a Wonder Woman (Creature's folder alone).
// The DODGED text of a slam over an invulnerable fighter: the buff actor's handler's ReceiveTick (U@8263, the dodge
// pass of Comp_Pawn_HitBoxHandler: `fighter::control::dodge_tick`) on the buff's state, in the same timer pass behind
// the hits.
// Unverified: the handler's tick is taken there, once a frame while the buff lives, as the World's dodge pass is behind
// the frame's overlap tests.
// Deviation: SLAM_RADIUS is DamageSphere's SphereRadius as the class data states it (400), written here: the buff
// class's components are not in the wonderwoman perk part (the perk tables keep a buff's defaults alone).

/// DamageSphere's SphereRadius (Buff_Perk_WonderWoman_FastFallSlam's class data).
const SLAM_RADIUS: Real = 400.0;

/// The buff actor's handler as the hits' dealer, made at the landing's HitBoxesOn (`DealerActor`): its place (her
/// actor location) and facing of the last test, and its owner. The rest is the class's data.
#[derive(State, Copy, Debug)]
pub struct SlamDealer {
    pub x: Real,
    pub y: Real,
    pub facing: Real,
    pub owner: FighterId,
}

/// `SlamState`: Buff_Perk_WonderWoman_FastFallSlam's extension.
#[derive(State, Debug, Default)]
pub struct Slam {
    /// The HitBox component's collision (HitBoxesOn / HitBoxesOff).
    pub on: bool,
    /// Its world scale (HitBoxesOn's).
    pub scale: Real,
    /// The pending Delay(0.066): buff ticks left, 0 none.
    pub delay: Real,
    /// The handler's hit list (HitEachActorOnlyOnce), by fighter id.
    pub hits: Vec<i32>,
    /// The actors that are no fighter the sphere overlaps now.
    pub over: IndexSet<ActorRef>,
    /// The hit list's actors that are no fighter.
    pub once: IndexSet<ActorRef>,
    pub dealer: Option<SlamDealer>,
    /// The buff actor's Comp_Pawn_HitBoxHandler dodge variables.
    pub dodge: Option<DodgeState>,
}

/// `BlockState`: Buff_Perk_WonderWoman_ProjectileBlock's extension.
#[derive(State, Debug, Default)]
pub struct Block {
    /// The Timer's frames left, 0 not active.
    pub timer: Real,
}

/// `EvadeShield extends ShieldActor` (WW_EvadeProjectileBlock_C): the shield's state (the child adds no field).
#[derive(State, Debug, Default)]
pub struct EvadeShield {
    pub base: ShieldActor,
}

/// The dodge variables of the slam's buff actor handler (`SlamState.dodge`, the evade code DodgeState).
pub fn slam_dodge_mut(e: &mut Slam) -> Option<&mut Option<DodgeState>> {
    Some(&mut e.dodge)
}

/// The fighter the slam's buff actor credits its hits to (`SlamState.dealer.owner`: the Wonder Woman holding it).
pub fn slam_owner(e: &Slam) -> Option<FighterId> {
    e.dealer.map(|d| d.owner)
}

/// The evade shield's place (its `x`, `y`), `None` for none.
pub fn evade_place(w: &World, a: &WorldActor, s: &EvadeShield) -> Option<(Real, Real)> {
    shield_actor::place(w, a, &s.base)
}

fn slam(w: &World, b: BuffRef) -> Option<&Slam> {
    match &buff(w, b)?.ext {
        BuffExt::FastFallSlam(e) => Some(e),
        _ => None,
    }
}

fn slam_mut(w: &mut World, b: BuffRef) -> Option<&mut Slam> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::FastFallSlam(e) => Some(e),
        _ => None,
    }
}

fn block_mut(w: &mut World, b: BuffRef) -> Option<&mut Block> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::ProjectileBlock(e) => Some(e),
        _ => None,
    }
}

/// `slamHits`: the sphere's hits of the frame on the fighters, then on the actors that are no fighter.
fn slam_hits(w: &mut World, f: FighterId, b: BuffRef) {
    let h =
        inst_view(w, b).obj("HitBoxInfo").map_or_else(HitPacket::default, |p| HitPacket::from_props(&w.names, p, None));
    let Some(e) = slam(w, b) else { return };
    let r = SLAM_RADIUS * e.scale;
    let (x, y, fc) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    let Some(e) = slam_mut(w, b) else { return };
    e.dealer = Some(SlamDealer { x, y, facing: fc, owner: f });
    let attack_id = h.attack_id.unwrap_or(0);
    let bkb = h.base_knock_back.unwrap_or(0.0);
    let me = ActorRef::Buff(b);
    let mut d = dealer(w, me);
    d.attack_id = attack_id;
    d.facing = fc;
    let sphere = WorldSphere { x, y, z: Some(0.0), r, ..WorldSphere::default() };
    let team = w.fighter(f).team.team;
    for o in match_fighters(w) {
        let oid = w.fighter(o).id;
        let listed = slam(w, b).is_some_and(|e| e.hits.contains(&oid));
        if o == f || w.fighter(o).team.team == team || !alive(w, o) || no_hit(w, o) || listed {
            continue;
        }
        let Some(vp) = victim_pose(w, o) else { continue };
        let met = crate::combat::hit::hurt_hit(w, o, std::slice::from_ref(&sphere), &vp).is_some();
        give_vbuf(w, vp);
        if !met {
            continue;
        }
        let (ox, oy) = (w.fighter(o).body.x, w.fighter(o).body.y);
        if h.do_los_check.unwrap_or(false) && los_blocked(w, x, y, ox, oy) {
            continue;
        }
        if invulnerable(w, o) {
            // AttackingActor: the buff actor
            let tick = w.flow.tick;
            attack_dodged(w, me, o, attack_id, bkb, tick, None, false);
            continue;
        }
        dodge_over(w, me, o);
        if h.hit_each_actor_only_once.unwrap_or(false)
            && let Some(e) = slam_mut(w, b)
        {
            e.hits.push(oid);
        }
        apply_projectile_hit(w, &d, o, &h, &[], V2 { x, y }, None, &[], 0.0);
    }
    let Some(e) = slam_mut(w, b) else { return };
    let (mut over, mut once) = (std::mem::take(&mut e.over), std::mem::take(&mut e.once));
    hit_box_actor_hits(w, me, &h, &HITBOX_PROFILE_TYPES, Some(HitShape::Sphere { x, y, r }), &mut over, &mut once);
    if let Some(e) = slam_mut(w, b) {
        e.over = over;
        e.once = once;
    }
}

fn slam_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::FastFallSlam(Slam { scale: 1.0, ..Slam::default() });
    }
}

/// Fast Fall Slam(height) @510 on FastFallLanded.
fn slam_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::FastFallLanded {
        return false;
    }
    let d = inst_view(w, b);
    let h = a.height.unwrap_or(0.0);
    let (lo_h, hi_h) = (d.num("MinHeight").unwrap_or(0.0), d.num("MaxHeight").unwrap_or(0.0));
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(h > min): a NaN height returns")]
    if !(h > lo_h) {
        return false;
    }
    let t = (max(min(h, hi_h), lo_h) - lo_h) / (hi_h - lo_h);
    let lo: Real = 0.6;
    let scale = lo + (t * (1.0 - lo));
    let fm = w.fighter(f);
    let ev = Event::ProjectileDeath {
        cls: name!("Buff_Perk_WonderWoman_FastFallSlam_C"),
        fighter: fm.id,
        vfx: Some(name!("PS_CreatureCrystal_Impact")),
        x: fm.body.x,
        y: actor_y(w, f),
        ox: 0.0,
        rot: [0.0, if facing(w, f) > 0.0 { -90.0 } else { 90.0 }, 0.0],
        scale,
        tint: None,
        to: None,
    };
    w.fighter_mut(f).events.push(ev);
    let Some(e) = slam_mut(w, b) else { return false };
    e.on = true;
    e.scale = scale;
    if e.delay <= 0.0 {
        e.delay = latent_frames(0.066);
    }
    false
}

/// The Delay(0.066)'s tick: HitBoxesOff, the hit list cleared.
fn slam_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    let Some(e) = slam_mut(w, b) else { return false };
    if e.delay <= 0.0 {
        return false;
    }
    e.delay -= 1.0;
    if e.delay > 0.0 {
        return false;
    }
    e.on = false; // HitBoxesOff
    e.hits.clear();
    e.over.clear();
    e.once.clear();
    e.dealer = None;
    false
}

/// The frame's timer pass: the sphere's hits while on, then the handler's dodge pass.
fn slam_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if slam(w, b).is_some_and(|e| e.on) {
        slam_hits(w, f, b);
    }
    let tick = w.flow.tick;
    let mut events = std::mem::take(&mut w.events);
    dodge_tick(w, ActorRef::Buff(b), tick, &mut events);
    w.events = events;
    false
}

// ---- Buff_Perk_WonderWoman_ProjectileBlock and WW_EvadeProjectileBlock
// ReceiveBeginPlay @10 (authority, a BP_FighterCharacter owner with a Comp_Spawner): binds the owner's EvadeTriggered
// @340 (the BuffSignal EvadeTriggered, `duration` the evade's invulnerable time).
// OnEvadeTriggered(InvincibilityDuration) @386: the owner's Comp_Pawn_AbilityManager.TriggerAbilityCheck(AbilityIndex
// 2) @571 (the cooldown's VO and sound when it fails); not ready: nothing. Ready: with the Timer active @669 it is
// cleared @683 and no shield is spawned; else CompSpawnActor(WW_EvadeProjectileBlock_C, attached to the owner, socket
// Shield, Transform (location 0, rotation 0, scale 2)) @948. Both then Timer = K2_SetTimerDelegate(ClearShield,
// FMax(InvincibilityDuration, MinimumBlockDuration 0.1)) @703-829 (not looping). The dodge spends nothing.
// ClearShield @1079: CompSpawner.DestroySpawnedActors(WW_EvadeProjectileBlock_C): every shield of the class she
// spawned.
// ServerAbilityCall @1376 -> MulticastAbilityCall @1132: the AbilityManager's TriggerAbilityActual(AbilityIndex) @1317
// (`fighter::control::trigger_ability`: the cooldown, the ammo).
// The timer is set outside the timer tick (the evade, in the pawn's tick): timer_frames, counted in the frame's timer
// pass.

fn block_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::ProjectileBlock(Block::default());
    }
}

/// `EVADE_P`: CompSpawnActor(WW_EvadeProjectileBlock_C) @948's properties.
fn evade_props() -> Props {
    let v3 = |x: Real| {
        let mut o = Props::new();
        for k in ["X", "Y", "Z"] {
            o.insert(k.into(), Value::Num(f64::from(x)));
        }
        Value::Obj(o)
    };
    let mut t = Props::new();
    t.insert("Translation".into(), v3(0.0));
    t.insert("Scale3D".into(), v3(2.0));
    let mut p = Props::new();
    p.insert("AttachToActor".into(), Value::Bool(true));
    p.insert("Socket".into(), Value::Str("Shield".into()));
    p.insert("Transforms".into(), Value::Obj(t));
    p
}

/// OnEvadeTriggered(InvincibilityDuration) @386.
fn block_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::EvadeTriggered {
        return false;
    }
    let d = inst_view(w, b);
    let (slot, least) = (d.num("AbilityIndex").map_or(0, to_int32), d.num("MinimumBlockDuration").unwrap_or(0.0));
    if !trigger_ability_check(w, f, slot) {
        return false;
    }
    let Some(e) = block_mut(w, b) else { return false };
    let spawn = e.timer <= 0.0;
    e.timer = timer_frames(Some(max(a.duration.unwrap_or(0.0), least)));
    if spawn {
        let r = SpawnRequest {
            cls: name!("WW_EvadeProjectileBlock_C"),
            n: NotifyRef { montage: Name::NONE, index: 0 },
            attack_id: w.fighter(f).decay.attack_count,
            foreign: Foreign::None,
            copy: None,
            charge: instigator_charge(w, f),
            aim: None,
            props: Some(Arc::new(evade_props())),
            instigator: None,
            origin: None,
        };
        w.fighter_mut(f).requests.spawn_requests.push(r);
    }
    false
}

/// The Timer's run out: ClearShield @1079.
fn block_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = block_mut(w, b) else { return false };
    if e.timer <= 0.0 {
        return false;
    }
    e.timer -= 1.0;
    if e.timer > 0.0 {
        return false;
    }
    let cls = name!("WW_EvadeProjectileBlock_C");
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        if let Some(a) = w.actors.actors.get_mut(x)
            && a.owner == f
            && a.cls == cls
        {
            a.dead = true;
        }
    }
    false
}

// WW_EvadeProjectileBlock (child of WW_ShieldActor: actors/shield_actor.rs, whose step, bodies and the parent's
// OnProjectileBlocked do-once it keeps). Class data: BlockCollision BoxExtent 32, RelativeScale3D (0.5, 1, 1.25) at
// the root, profile OverlapAllDynamic (WorldDynamic, QueryOnly); Collision and WallbounceCollision NoCollision (no
// platform, no wall); BlockHeavyProjectiles; no mesh and no particle component (the actor shows nothing).
// With the spawn's scale 2 the box is 32 half thick and 80 half high at her actor location.
// Block Projectile Hit Visuals(Location), its override (@595): sfx_cha_wonderwoman_bracerblock at the Location @659,
// PS_WonderWoman_Perk_Evade at the Location turned FindLookAtRotation(actor location + (0, 0, 200), Location) @765-861
// (scale 1, the Team parameter: the view's), then (authority, a valid emitter @955) the instigator's
// BPI_CheckIfHasBuff(Buff_Perk_WonderWoman_ProjectileBlock_C) -> its ServerAbilityCall @504: a blocked projectile
// spends the ability (once per the parent's 0.2 s do-once, which calls this function).
// The class is not in her character data (its def would renumber her static ids) but in her perk table, which
// `actor_def` reads after the character's: the world's generic reads of a spawned actor's class (grant_actor_buffs:
// SpawnedActorParent's BeginPlay GrantBuffsFromInstigator @9233; the parent checks, the class casts) take it as any
// other.
// BlockCollision's profile is OverlapAllDynamic (object type WorldDynamic, DefaultEngine.ini 555; Item added at 615),
// not WW_ShieldActor's ProjectileDestructible: it overlaps the engine's six object types and Item, and a projectile's
// component overlaps it when that component's response to WorldDynamic is not Ignore. `evade_meets` takes the
// projectile's three bodies, each while on: the root Capsule (upright, the traces' capsule), HitBox (the hit circle) and
// HurtBox (the hurt sphere at the actor). By the profiles (592-610): a HitBox of profile HitBox (object type HitBox,
// WorldDynamic Ignore) and a HurtBox of ProjectileDestructible (WorldDynamic Ignore) are never met; a root of
// ProjectileNavNoPlatform / ProjectileNavPlatform (PhysicsBody, WorldDynamic left at Block) is, hit box on or off,
// whatever its cooked ResponseArray says (ShaggySandwichProjectile's, PieProjectile's, C019BlasterChargeProjectile's and
// the blaster and missile shots' cook WorldDynamic Ignore; the profile overrides it). A HitBox of
// profile OverlapAllDynamic (ShadowClone_Special_F_Projectile, Meeseeks_ChargePunch, the portals) is met.
// The component's response is its named profile's (LoadProfileData 142c77210 applies it over the cooked ResponseArray,
// ShaggySandwichProjectile's root Capsule, profile ProjectileNavNoPlatform, blocks WorldDynamic
// where its cooked array says Ignore, so the shield meets the sandwich.
// A body whose object type is a custom channel past Item is taken as not met (the profile leaves those at their
// channel default). Items (ItemActorParent: no SpawnedActorParent for the overlap's cast @2157) are not tested:
// shield_step goes through the World's projectiles alone.

/// `EVADE_MEETS`: the object types OverlapAllDynamic overlaps (WorldStatic, WorldDynamic, Pawn, PhysicsBody, Vehicle,
/// Destructible, Item).
const EVADE_MEETS: [i32; 7] = [OBJ_WORLD_STATIC, OBJ_WORLD_DYNAMIC, OBJ_PAWN, OBJ_PHYSICS_BODY, 4, 5, OBJ_ITEM];

/// `evadeMeetsBody`: whether BlockCollision's OverlapAllDynamic meets a body of the component template `comp` (whether
/// the body is on is the caller's: the template's is not the run's): its object type is one OverlapAllDynamic overlaps
/// and its response to WorldDynamic is not Ignore, both as the build applies the body's named profile (`body_info`,
/// `response_to`).
// Computed per call (the value is the same for every call).
pub fn evade_meets_body(comp: Option<&Props>) -> bool {
    let Some(comp) = comp else { return false };
    body_info(Some(comp), None).is_some_and(|i| EVADE_MEETS.contains(&i.kind))
        && response_to(Some(comp), crate::real(OBJ_WORLD_DYNAMIC)) != name!("ECR_Ignore")
}

/// `EvadeShield.meets`: the projectile's root capsule, its hurt sphere and its hit circle, each where the profile
/// meets it.
fn evade_meets(w: &World, s: &ShieldActor, pr: ProjectileId, b: &Placed) -> bool {
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    // (a projectile with no owner has no class data to read: `actorDef` of no owner throws)
    let Some(c) = p.owner.and_then(|o| actor_def(w, ActorRef::Fighter(o), p.cls)).map(|d| &d.components) else {
        return false;
    };
    let comp = |k: &str| obj(c, k);
    let ax = max((p.x - s.x).abs() - b.hx, 0.0);
    let ay = (p.y - s.y).abs() - b.hy;
    // The root Capsule as the traces' queries meet it (`root_query_on`): a root with no saved profile is query-on
    // until a graph writes it (C023B_SpawnedGunShot_C).
    let cap = comp("Capsule");
    if body_info(cap, None).is_some_and(|i| root_query_on(p, &i)) && evade_meets_body(cap) {
        let r = capsule_radius(p);
        let dy = max(ay - max(capsule_half_height(p) - r, 0.0), 0.0);
        if ax * ax + dy * dy <= r * r {
            return true;
        }
    }
    if p.hurt_radius > 0.0 && evade_meets_body(comp("HurtBox")) {
        let dy = max(ay, 0.0);
        if ax * ax + dy * dy <= p.hurt_radius * p.hurt_radius {
            return true;
        }
    }
    // The hit circle (ShieldActor's meets: hit_collision, set at run time by UpdateHitbox) for a HitBox the profile
    // meets.
    evade_meets_body(comp("HitBox")) && shield_actor::meets(w, s, pr, b)
}

/// `EvadeShield.meetsComp`: a hit box that one call turns on, by the same profile rule (C016_SassBubble's Box, of
/// profile HitBox, is never met).
fn evade_meets_comp(w: &World, pr: ProjectileId, comp: Name) -> bool {
    let Some(p) = w.actors.projectiles.get(pr) else { return false };
    p.owner.is_some_and(|o| {
        evade_meets_body(actor_def(w, ActorRef::Fighter(o), p.cls).and_then(|d| obj(&d.components, w.names.str(comp))))
    })
}

/// `EvadeShield.blockVisuals`: the override @595.
fn evade_block_visuals(w: &mut World, id: ActorId, x: Real, y: Real) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (f, cls) = (a.owner, a.cls);
    let fid = w.fighter(f).id;
    let ev = name!("sfx_cha_wonderwoman_bracerblock");
    w.events.push(Event::Sound { fighter: fid, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    let dx = x - w.fighter(f).body.x;
    let dy = y - (actor_y(w, f) + 200.0);
    // FindLookAtRotation(the actor + (0, 0, 200), Location) @815: MakeFromX, FMatrix::Rotator (the sim's +x is UE -Y)
    let [pitch, yaw, roll] = rot_from_x([0.0, -dx, dy]);
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: fid,
        vfx: Some(name!("PS_WonderWoman_Perk_Evade")),
        x,
        y,
        ox: 0.0,
        rot: [pitch, yaw, roll],
        scale: 1.0,
        tint: None,
        to: None,
    });
    if let Some(b) = buff_of(w, f, name!("Buff_Perk_WonderWoman_ProjectileBlock_C")) {
        let slot = inst_view(w, b).num("AbilityIndex").map_or(0, to_int32);
        trigger_ability(w, f, slot);
    }
}

/// The evade shield's [`ShieldHooks`].
pub const EVADE: ShieldHooks =
    ShieldHooks { meets: evade_meets, meets_comp: evade_meets_comp, block_visuals: evade_block_visuals };

/// CompSpawnActor(WW_EvadeProjectileBlock_C) @948: a spawn request the world's spawn pass takes in the same frame as a
/// notify's (the kind's spawn below, add_actor with the owner's charge values and grant_actor_buffs). AttachToActor and
/// socket Shield, which her mesh has not (as WW_ShieldRamForward's: shield_actor.rs): attached to the actor itself, the
/// Transform its relative one (Scale3D the root's world scale).
fn evade_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let ch = w.fighter(f).ch;
    let has = w
        .names
        .get("socket:Shield")
        .is_some_and(|n| w.fixed.samplers.get(&ch).is_some_and(|s| s.by_name.contains_key(&n)));
    if has {
        warn_once(
            w,
            name!("WW_EvadeProjectileBlock_C: the owner has a Shield socket: placed at the actor all the same"),
        );
    }
    let np = crate::world::spawning::request_props(w, f, r);
    let t = obj(&np, "Transforms").cloned().unwrap_or_default();
    let def = actor_def(w, ActorRef::Fighter(f), r.cls);
    let c = |k: &str| def.and_then(|d| obj(&d.components, k));
    let (b, col, wl) = (
        shield_actor::place_box(c("BlockCollision"), &t),
        shield_actor::place_box(c("Collision"), &t),
        shield_actor::place_box(c("WallbounceCollision"), &t),
    );
    let mut s = shield_actor::new_shield(w, f, b, col, wl);
    s.spawn_t = Some(Arc::new(t));
    let (ox, oy, fc) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    s.x = ox + s.box_.map_or(0.0, |b| b.fwd * fc);
    s.y = oy + s.box_.map_or(0.0, |b| b.up);
    let dead = shield_actor::shield_blast_loop(w, f, r.cls, &mut s);
    (WorldActorState::EvadeShield(EvadeShield { base: s }), dead)
}

/// This file's registry entries (the two perk buffs, WW_EvadeProjectileBlock_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let slam = BuffHooks {
        begin: Some(slam_begin),
        on: Some(slam_on),
        tick: Some(slam_tick),
        timer: Some(slam_timer),
        ..BuffHooks::default()
    };
    r.buffs.enter(
        "Buff_Perk_WonderWoman_FastFallSlam_C",
        BuffEntry::new(BuffClass::BuffPerkWonderWomanFastFallSlam, slam),
    )?;
    let block =
        BuffHooks { begin: Some(block_begin), on: Some(block_on), timer: Some(block_timer), ..BuffHooks::default() };
    r.buffs.enter(
        "Buff_Perk_WonderWoman_ProjectileBlock_C",
        BuffEntry::new(BuffClass::BuffPerkWonderWomanProjectileBlock, block),
    )?;
    // Spawned by the buff alone. The parent's step, bodies, end and BPI calls.
    let mut e: ActorEntry =
        shield_actor::entry(WorldActorClass::WWEvadeProjectileBlock, &["WW_ShieldActor_C", "SpawnedPlatformParent_C"]);
    if let ActorHooks::World(h) = &mut e.hooks {
        h.spawn = Some(evade_spawn);
    }
    r.actors.enter(EVADE_SHIELD, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(v: &str) -> Props {
        match serde_json::from_str::<Value>(v).expect("a JSON object") {
            Value::Obj(o) => o,
            _ => unreachable!(),
        }
    }

    // The profile rule of the evade shield's box against a projectile's bodies (the head note).
    #[test]
    fn evade_meets_body_profiles() {
        // a HitBox of profile HitBox: never met
        assert!(!evade_meets_body(Some(&props(
            r#"{"SphereRadius":10,"BodyInstance":{"CollisionProfileName":"HitBox"}}"#
        ))));
        // a root of ProjectileNavNoPlatform (PhysicsBody, WorldDynamic Block): met
        let root = r#"{"CapsuleRadius":10,"BodyInstance":{"CollisionProfileName":"ProjectileNavNoPlatform"}}"#;
        assert!(evade_meets_body(Some(&props(root))));
        // the same root with a cooked WorldDynamic Ignore (the sandwich's): the profile overrides it, met
        let ignored = r#"{"CapsuleRadius":10,"BodyInstance":{"CollisionProfileName":"ProjectileNavNoPlatform",
            "CollisionResponses":{"ResponseArray":[{"Channel":"WorldDynamic","Response":"ECR_Ignore"}]}}}"#;
        assert!(evade_meets_body(Some(&props(ignored))));
        // a Custom body keeps its saved array: WorldDynamic Ignore is not met
        let custom = r#"{"CapsuleRadius":10,"BodyInstance":{"CollisionProfileName":"Custom","ObjectType":"ECC_PhysicsBody",
            "CollisionResponses":{"ResponseArray":[{"Channel":"WorldDynamic","Response":"ECR_Ignore"}]}}}"#;
        assert!(!evade_meets_body(Some(&props(custom))));
        // a HitBox of profile OverlapAllDynamic: met
        assert!(evade_meets_body(Some(&props(
            r#"{"SphereRadius":10,"BodyInstance":{"CollisionProfileName":"OverlapAllDynamic"}}"#
        ))));
        assert!(!evade_meets_body(None));
    }

    // The slam's scale (Fast Fall Slam @558-788) at the heights' ends, float32.
    #[test]
    fn slam_scale() {
        let scale = |h: Real| {
            let (lo_h, hi_h) = (200.0, 1000.0);
            let t = (max(min(h, hi_h), lo_h) - lo_h) / (hi_h - lo_h);
            let lo: Real = 0.6;
            lo + (t * (1.0 - lo))
        };
        assert_eq!(scale(1000.0), 1.0);
        assert_eq!(scale(5000.0), 1.0);
        assert_eq!(scale(600.0), 0.8);
    }
}
