//! FishingLure: the lure ([`Lure`], a ParentProjectilePhysics child
//! with a tick of its own): the cast, the sweet-spot hit lerped by distance (UsedDamage @9170-9189), the grab of Jerry
//! (its Cheese branch dormant), MoveComponentTo back to Tom (`ComponentMove`) and the cable; the Bait perk
//! (Buff_TomAndJerry_Bait, a marker the lure tests @7198-7372; its pull: Buff_MoveToPosition_LassoGrapple); joined to
//! montageEnded.
//!
//! The graph (FishingLure ubergraph offsets): ReceiveBeginPlay @5309 (the JerryOutCondition @4926, the bindings @5766-6374,
//! Delay(0.2) -> gravity scale 8, BaitPerkEquipped @7099, Delay(1.0) -> DestroyOnExitedArena?); ActivateObject @8652
//! (the fishing montages' ActivateSpawned rows: Reel f110 {0 T}, Ground End {0 F, 1 T}, Air End {0 T, 1 T});
//! @2210 SweetSpotHit, GrabJerry, the hit box off, ProjectileMovement.Deactivate, Delay(0.075) -> MoveLureToTomMulticast;
//! ProjectileBounce @8388; HitEnemy @12661; ReceiveEndPlay @10669 (no parent call); GrabJerry @11281;
//! ReceiveAnyDamage @7489; ReceiveDestroyed @7769; ReceiveTick @10251 (it does not call ParentProjectilePhysics').
//
// Deviation (fishing a): the owner's KnockedBack, HitStunStart and OnMontageEnded are sampled in the kind's step
// (`attackers.knocked_back_count`; a buff stun that began since the last step, a knockback's stun coming with its
// KnockedBack; `TomAndJerryFishingState::montage_ends`), not inside the broadcasts: the lure dies in the projectile pass
// of the frame, not in the hit or the montage's end.
// Unverified (fishing b): the bait bounce's JumpToSectionServer(Reel, null, AllowNonLocalPlayer false) @4868 reaches the
// multicast @1154, which jumps only for a pawn not locally controlled or with AllowNonLocalPlayer: every pawn of an
// offline match is locally controlled, so the jump is not made (the HitEnemy jump @3897 passes true and is made).
// Deviation (fishing c): the 250 spheres leave out WorldDynamic bodies the port's traces do not hold; GrabJerry's begin
// overlap is a per-tick sphere of the Capsule's radius (80) against the same types, from the bind on, not the engine's
// begin-overlap event of the Capsule's profile (ProjectileNavPlatform) against Jerry's bodies; the attached Jerry is
// placed at the lure in the lure's step.
// Unverified (fishing d): the Delays count from the call as latent_frames (from BeginPlay, from ActivateObject); the
// MoveComponentTo started in the Delay's resume makes its first update in the next frame's step.
// Deviation (fishing e): GetDistanceTo (the sweet spot's lerp) and the launch's direction are taken in the plane (depth 0).
// Deviation (fishing f): the LassoGrapple buff is not put in the owner's buff list; its move is the ally move's (as
// chars/wonder_woman/perks_lasso.rs, which enters Buff_MoveToPosition_LassoGrapple_C as ported).
// Deviation (fishing g): the class's RotatingMovement (yaw 2000 deg/s on the root) is left out: in flight the
// ProjectileMovement sets the rotation from the velocity every pass, at rest the tick's SetWorldRotation and on the way
// back the latent MoveComponentTo write over it; the order of the two movement components' ticks and of the latent
// update against RotatingMovement are not read. Two windows write no rotation, where the build's lure turns (the reel's
// Delay 0.075 after the movement stops, Destroy Visuals' Delay 0.25): the port holds the last flight rotation there
// (looks only: the hit box is off).

use crate::Real;
use crate::State;
use crate::actors::projectile::{
    CollisionResponse, ProjectileExt, class_p, hitbox_active, set_gravity_scale, set_hitbox, set_instance_vars,
    set_platform_response, stop_simulating,
};
use crate::actors::{ActorClass, ActorId, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::combat::buffs::buff_of;
use crate::combat::hits::{actor_hit_enemy, apply_projectile_hit, dealer, projectile_blocks};
use crate::combat::{BuffClass, HitPacket};
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::anim::branches::conds_ok;
use crate::fighter::common::latent_frames;
use crate::fighter::control::{attack_dodged, dodge_over};
use crate::fighter::match_side::end_move_buff;
use crate::fighter::moves::{Impulse, V2, force_move_to_point, is_falling};
use crate::fighter::tick::jump_section_logic;
use crate::fighter::traces::{
    OBJ_ITEM_DESTRUCTIBLE, OBJ_PAWN, OBJ_PHYSICS_BODY, OBJ_PROJECTILE_DESTRUCTIBLE, sphere_overlap_actors,
};
use crate::fighter::{FighterId, actor_y, facing, in_hitstun, invulnerable};
use crate::math::ue::{SMALL_NUMBER, rot_from_x, safe_normal, size};
use crate::math::{max, min, to_int32};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, ProjectileHooks};
use crate::registry::props::{flag_or, num_or, obj};
use crate::registry::spawner_notifies::cond_list;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::pawns::is_match_fighter;
use crate::world::spawning::destroy_projectile;
use crate::world::stage::ComponentMove;
use mvs_data::value::Value;

use super::jerry::{destroy as jerry_destroy, jerry_mut, jerry_of, set_reset_jerry};
use super::pawn::set_is_jerry_attached;
use super::state::{tj_mut, tj_of};

/// The lure's class.
pub const LURE: &str = "FishingLure_C";
/// The Bait perk's marker.
pub const BAIT: &str = "Buff_TomAndJerry_Bait_C";

const REEL: Name = name!("Reel");
const SHOCKWAVE: Name = name!("PS_TomJerryNAUAIR_Shockwave");
const DESTROYED_FX: Name = name!("PS_LureDestroyed");
const HIT_AUDIO: Name = name!("sfx_cha_tomandjerry_fishing_hit");

/// `Lure extends Projectile` (FishingLure_C): the view reads the cable's two ends and the components' visibility.
#[derive(State, Debug)]
pub struct Lure {
    /// The cable's start on Tom (sim x, y: his actor location), drawn to the lure's place.
    pub cable_x: Real,
    pub cable_y: Real,
    /// JerryMesh shown (SetVisualsMulticast(true) hides it).
    pub jerry_mesh: bool,
    /// Lure shown (SetVisualsMulticast(true) hides it).
    pub lure_mesh: bool,
    /// LureNoRope shown (SetVisualsMulticast(false) hides it).
    pub no_rope: bool,
    /// PS_TomJerry_Ribbon shown (hidden at the first bounce and by ReelVisuals).
    pub ribbon: bool,
    /// JerryMesh's relative rotation set at BeginPlay by the Instigator's BPI_IsFacingLeft.
    pub jerry_left: bool,
    /// ActivationConditions.
    pub conds: [bool; 2],
    /// BaitPerkEquipped.
    pub bait: bool,
    pub has_bounced: bool,
    pub refresh_jerry: bool,
    /// JerryAttached (SetVisualsMulticast(false)).
    pub jerry_attached: bool,
    /// ShowImpactVFX.
    pub show_impact: bool,
    /// HP.
    pub lure_hp: Real,
    /// The DoOnce @3350.
    pub activate_closed: bool,
    /// The DoOnce @3800.
    pub reel_closed: bool,
    /// OnActorBeginOverlap += GrabJerry (@2253).
    pub grab_bound: bool,
    /// PullingJerry (IsPullingJerry: some).
    pub pulling_jerry: Option<ActorRef>,
    /// The ages the Delays resume at (-1: none): 0.2 -> gravity 8, 1.0 -> DestroyOnExitedArena?, 0.075 -> the move
    /// back, 0.25 -> destroy (ReceiveAnyDamage).
    pub grav_at: Real,
    pub arena_at: Real,
    pub return_at: Real,
    pub destroy_at: Real,
    /// MoveComponentTo(Capsule).
    pub mv: Option<ComponentMove>,
    /// The owner's delegates' samples (Deviation fishing a).
    pub kb_at: i32,
    pub stun_was: bool,
    pub stun_age: Real,
    pub montage_at: i32,
}

impl Default for Lure {
    fn default() -> Self {
        Lure {
            cable_x: 0.0,
            cable_y: 0.0,
            jerry_mesh: true,
            lure_mesh: true,
            no_rope: true,
            ribbon: true,
            jerry_left: false,
            conds: [false, false],
            bait: false,
            has_bounced: false,
            refresh_jerry: true,
            jerry_attached: false,
            show_impact: true,
            lure_hp: 1.0,
            activate_closed: false,
            reel_closed: false,
            grab_bound: false,
            pulling_jerry: None,
            grav_at: -1.0,
            arena_at: -1.0,
            return_at: -1.0,
            destroy_at: -1.0,
            mv: None,
            kb_at: 0,
            stun_was: false,
            stun_age: 0.0,
            montage_at: 0,
        }
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn lr(w: &World, id: ProjectileId) -> &Lure {
    match &pr(w, id).ext {
        ProjectileExt::Lure(s) => s,
        e => panic!("a FishingLure_C without its state: {e:?}"),
    }
}

fn lx(w: &mut World, id: ProjectileId) -> &mut Lure {
    match &mut pm(w, id).ext {
        ProjectileExt::Lure(s) => s,
        e => panic!("a FishingLure_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a lure has an owner")
}

fn sound(w: &mut World, id: ProjectileId, ev: Name) {
    let fighter = w.fighter(owner(w, id)).id;
    let p = pr(w, id);
    let e = Event::Sound { fighter, ev, x: Some(p.x), y: Some(p.y), hit: None, pooled: false, key: None };
    w.events.push(e);
}

fn emitter(w: &mut World, id: ProjectileId, vfx: Name, scale: Real) {
    let fighter = w.fighter(owner(w, id)).id;
    let p = pr(w, id);
    let e = Event::ProjectileDeath {
        cls: p.cls,
        fighter,
        vfx: Some(vfx),
        x: p.x,
        y: p.y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale,
        tint: None,
        to: None,
    };
    w.events.push(e);
}

/// ProjectileMovement.Deactivate: the component stops ticking, its Velocity kept.
fn deactivate(p: &mut Projectile) {
    let (vx, vy) = (p.vx, p.vy);
    stop_simulating(p);
    p.vx = vx;
    p.vy = vy;
}

/// K2_SetActorRotation / Capsule.K2_SetWorldRotation (the lure's own graph: no parent tick): the pitch in degrees, the
/// side the yaw's (`Projectile::rot_x`: 1 = sim +x, UE yaw < 0; -1 for yaw 0 or +90).
fn set_rotation(p: &mut Projectile, pitch: Real, side: Real) {
    p.pitch = pitch;
    p.rot_x = side;
}

/// The Jerry actor `a` (a live Jerry of this owner), else none.
fn live_jerry_of(w: &World, a: ActorRef, f: FighterId) -> Option<ActorId> {
    let ActorRef::Actor(id) = a else { return None };
    let wa = w.actors.actors.get(id)?;
    jerry_of(w, id)?;
    (!wa.dead && wa.owner == f).then_some(id)
}

/// The owner's HitStunStart since the last step (Deviation fishing a): a buff stun begun since then (a knockback's stun
/// comes with its KnockedBack, which destroys the lure already).
fn stun_started(w: &mut World, id: ProjectileId) -> bool {
    let f = owner(w, id);
    let (stunned, age) = (w.fighter(f).buffs.buff_stunned, w.fighter(f).buffs.buff_stun_age);
    let l = lx(w, id);
    let fresh = stunned && (!l.stun_was || age <= l.stun_age);
    l.stun_was = stunned;
    l.stun_age = age;
    fresh
}

/// Jerry taken by the lure (@1059-1193, GrabJerry @11537-11742): K2_AttachToActor(SnapToTarget) and his
/// ProjectileMovement deactivated.
fn take_jerry(w: &mut World, id: ProjectileId, j: ActorId) {
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    lx(w, id).pulling_jerry = Some(ActorRef::Actor(j));
    if let Some(s) = jerry_mut(w, j) {
        s.moving = false;
        s.aim_down = false;
        s.x = x;
        s.y = y;
    }
}

/// HitEnemy @12661 (HitEnemyPlayer of the lure's handler): with the hit box on and no bounce yet, the DoOnce @3800 ->
/// the owner's JumpToSectionServer(Reel, null, AllowNonLocalPlayer true) @3897.
fn hit_enemy_part(w: &mut World, id: ProjectileId) {
    let l = lr(w, id);
    if !hitbox_active(pr(w, id)) || l.has_bounced || l.reel_closed {
        return;
    }
    lx(w, id).reel_closed = true;
    let f = owner(w, id);
    jump_section_logic(w, f, REEL);
}

/// SweetSpotHit @12622: ReelVisuals (@12743: PS_TomJerryNAUAIR_Shockwave at the lure, scale 1, the ribbon hidden), then
/// nothing with the owner in hitstun (@12155), else SphereOverlapActors(the lure, 250, [PhysicsBody, Pawn,
/// ProjectileDestructible, ItemDestructible], BaseFighter) and for each of another team (the targets'
/// Comp_Actor_Destructible TeamIndex @9078) pfgDoDamage(null, it, UsedDamage, no buffs) @10053, UsedDamage being
/// (@9170-9913): BaitPerkEquipped ? BaitHitDamage : SweetSpotHitDamage, with d = FClamp(NormalizeToRange(GetDistanceTo
/// (owner), 400, 2200), 0, 1): Damage = Lerp(RangeDamageDecay x Damage, Damage, d), BaseKnockBack = Lerp(BaseKnockBack -
/// RangeBaseKnockDecay, BaseKnockBack, d), KnockBackScaling = d >= 0.75 ? 18.5 : 14, OnHitAudio = d >= 0.75 ? its own :
/// sfx_cha_tomandjerry_fishing_hit. Each hit that lands broadcasts the handler's HitEnemyPlayer: HitEnemy, with the
/// hit box still on here.
fn sweet_spot_hit(w: &mut World, id: ProjectileId) {
    emitter(w, id, SHOCKWAVE, 1.0);
    lx(w, id).ribbon = false;
    let f = owner(w, id);
    if in_hitstun(w, f) {
        return;
    }
    let cp = class_p(w, pr(w, id));
    let key = if lr(w, id).bait { "BaitHitDamage" } else { "SweetSpotHitDamage" };
    let empty = mvs_data::value::Props::new();
    let base = HitPacket::from_props(&w.names, obj(cp, key).unwrap_or(&empty), None);
    let decay = num_or(cp, "RangeDamageDecay", 0.0);
    let kb_decay = num_or(cp, "RangeBaseKnockDecay", 0.0);
    let me = ActorRef::Projectile(id);
    let types = [
        crate::real(OBJ_PHYSICS_BODY),
        crate::real(OBJ_PAWN),
        crate::real(OBJ_PROJECTILE_DESTRUCTIBLE),
        crate::real(OBJ_ITEM_DESTRUCTIBLE),
    ];
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    let found = sphere_overlap_actors(w, x, y, 250.0, &types, &[Some(me)], -1.0);
    for a in found {
        let ActorRef::Fighter(vic) = a else { continue };
        if !is_match_fighter(w, vic) {
            continue; // the BaseFighter class filter
        }
        if w.fighter(vic).team.team == w.fighter(f).team.team || w.fighter(vic).life.dead > 0.0 || no_hit_of(w, vic) {
            continue;
        }
        let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
        let d = min(max((size([0.0, pr(w, id).x - fx, pr(w, id).y - fy]) - 400.0) / 1800.0, 0.0), 1.0);
        let far = d >= 0.75;
        let dmg = base.damage.unwrap_or(0.0);
        let bkb = base.base_knock_back.unwrap_or(0.0);
        let near = decay * dmg;
        let bkb_near = bkb - kb_decay;
        let mut p = base.clone();
        p.damage = Some(near + (d * (dmg - near)));
        p.base_knock_back = Some(bkb_near + (d * (bkb - bkb_near)));
        p.knock_back_scaling = Some(if far { 18.5 } else { 14.0 });
        if !far {
            p.on_hit_audio = Some(HIT_AUDIO);
        }
        let dl = dealer(w, me);
        if projectile_blocks(w, &dl, vic) {
            continue;
        }
        if invulnerable(w, vic) {
            let (tick, aid) = (w.flow.tick, pr(w, id).attack_id);
            attack_dodged(w, me, vic, aid, p.base_knock_back.unwrap_or(0.0), tick, None, false);
            continue;
        }
        dodge_over(w, me, vic);
        let at = V2 { x: w.fighter(vic).body.x, y: actor_y(w, vic) };
        let pct = crate::combat::elem::charge_pct(w, me);
        if !apply_projectile_hit(w, &dl, vic, &p, &[], at, None, &[], pct) {
            continue;
        }
        actor_hit_enemy(w, &dl, vic);
        hit_enemy_part(w, id);
    }
}

fn no_hit_of(w: &World, f: FighterId) -> bool {
    crate::combat::phases::no_hit(w, f)
}

/// @2210-2512: the sweet spot, GrabJerry bound, the hit box off, the movement deactivated, Delay(0.075), and the pull of
/// the owner's Jerry within 250.
fn reel(w: &mut World, id: ProjectileId) {
    sweet_spot_hit(w, id);
    lx(w, id).grab_bound = true;
    set_hitbox(pm(w, id), false);
    deactivate(pm(w, id));
    let at = crate::real(pr(w, id).age) + latent_frames(0.075);
    lx(w, id).return_at = at;
    if lr(w, id).pulling_jerry.is_some() {
        return;
    }
    let f = owner(w, id);
    let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN)];
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    let found = sphere_overlap_actors(w, x, y, 250.0, &types, &[Some(ActorRef::Projectile(id))], -1.0);
    for a in found {
        if let Some(j) = live_jerry_of(w, a, f) {
            take_jerry(w, id, j);
            return;
        }
    }
}

/// ActivateObject @8652. True: K2_DestroyActor.
fn activate_object(w: &mut World, id: ProjectileId) -> bool {
    if !lr(w, id).conds[0] {
        return lr(w, id).conds[1];
    }
    if lr(w, id).activate_closed {
        return false;
    }
    lx(w, id).activate_closed = true;
    let f = owner(w, id);
    if is_falling(w, f) {
        // the owner's CharacterMovement IsFalling() @3447; @1608-2163: LaunchCharacter((0, dir.Y x 1800, dir.Z x 1800),
        // true, true), dir = GetDirectionUnitVector((0, owner Y, owner Z), the lure).
        let dx = pr(w, id).x - w.fighter(f).body.x;
        let dy = pr(w, id).y - actor_y(w, f);
        let [_, ux, uy] = safe_normal([0.0, dx, dy], SMALL_NUMBER);
        let (vx, vy) = (ux * 1800.0, uy * 1800.0);
        if w.fighter(f).life.dead <= 0.0 {
            w.fighter_mut(f).moves.impulse = Some(Impulse { vx, vy, xy: true, z: true, kind: 0, d: None });
        }
    } else if in_hitstun(w, f) {
        return true;
    }
    reel(w, id);
    false
}

/// ProjectileBounce @4005: the bait's pull.
fn bait_pull(w: &mut World, id: ProjectileId) {
    if !lr(w, id).bait || lr(w, id).has_bounced {
        return;
    }
    let f = owner(w, id);
    // SpawnBuffActor's cast fails with no pawn to put it on (waiting for the respawn).
    if w.fighter(f).life.dead > 0.0 {
        return;
    }
    // MulticastBeginMove(ImpactResult.Location, false, false, 0.4) (the WW lasso's model of the buff's move: BeginMove's
    // destination with X 0, finish SetVelocity with EndingLaunchVector (0, 0, 100)).
    let t = 0.4;
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    end_move_buff(w, f);
    let mid = force_move_to_point(w, f, x, y, t, 1.0, 0.0, 100.0, 0.0, false);
    w.fighter_mut(f).team.ally.move_id = to_int32(mid);
    w.fighter_mut(f).team.ally.buff = max(latent_frames(t) - 1.0, 1.0);
    // JumpToSectionServer(Reel, null, false, ...) @4868: no jump offline (Unverified fishing b).
}

// ---------------------------------------------------------------- the kind

/// The constructor's tail: the class's own state.
fn construct(_w: &World, p: &mut Projectile) {
    p.ext = ProjectileExt::Lure(Lure::default());
}

fn begin(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    let cp = class_p(w, pr(w, id));
    let hp = num_or(cp, "HP", 1.0);
    let refresh = flag_or(cp, "RefreshJerry", true);
    let show = flag_or(cp, "ShowImpactVFX", true);
    let conds = cond_list(w, cp, "JerryOutCondition");
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let left = facing(w, f) < 0.0;
    {
        let l = lx(w, id);
        l.lure_hp = hp;
        l.refresh_jerry = refresh;
        l.show_impact = show;
        l.cable_x = fx;
        l.cable_y = fy;
        // @5477-5693: JerryMesh's rotation by the Instigator's facing.
        l.jerry_left = left;
    }
    // @4926: the JerryOutCondition.
    if conds_ok(w, f, conds.as_deref(), false) {
        let l = lx(w, id);
        l.jerry_mesh = false;
        l.lure_mesh = false;
        l.refresh_jerry = false;
    } else {
        if tj_of(w, f).is_some() {
            set_is_jerry_attached(w, f, false);
        }
        let l = lx(w, id);
        l.jerry_attached = true;
        l.no_rope = false;
    }
    // @5436: the Capsule's Platform channel to Overlap.
    set_platform_response(pm(w, id), CollisionResponse::Overlap);
    // @5766-6374: the bindings' samples.
    let kb = w.fighter(f).attackers.knocked_back_count;
    let (stunned, stun_age) = (w.fighter(f).buffs.buff_stunned, w.fighter(f).buffs.buff_stun_age);
    let montage = tj_of(w, f).map_or(0, |s| s.fishing.montage_ends);
    let bait = buff_of(w, f, name!("Buff_TomAndJerry_Bait_C")).is_some();
    let age = crate::real(pr(w, id).age);
    let l = lx(w, id);
    l.kb_at = kb;
    l.stun_was = stunned;
    l.stun_age = stun_age;
    l.montage_at = montage;
    l.grav_at = age + latent_frames(0.2);
    l.bait = bait;
    l.arena_at = age + latent_frames(1.0);
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    let f = owner(w, id);
    // OnOwnerKB, DestroyOnOwnerHit, OnMontageEnd (Deviation fishing a).
    if w.fighter(f).attackers.knocked_back_count != lr(w, id).kb_at {
        return true;
    }
    if stun_started(w, id) {
        return true;
    }
    if tj_of(w, f).map_or(0, |s| s.fishing.montage_ends) != lr(w, id).montage_at {
        return true;
    }
    // ReceiveTick @10251-10600: at rest after a bounce, yaw 90 facing left, -90 facing right. On the way back the
    // MoveComponentTo's update comes behind it and puts the look-at back (the rotation set at its start: Deviation
    // fishing g).
    if lr(w, id).mv.is_none() && lr(w, id).has_bounced && size([0.0, pr(w, id).vx, pr(w, id).vy]) < 1.0 {
        let side = if facing(w, f) < 0.0 { -1.0 } else { 1.0 };
        set_rotation(pm(w, id), 0.0, side);
    }
    if let Some(mut m) = lr(w, id).mv {
        let done = m.update(DELTA_SECONDS);
        lx(w, id).mv = Some(m);
        let p = pm(w, id);
        p.x = m.x;
        p.y = m.y;
        if done {
            return true; // MoveComponentTo's completion @12525 -> @42
        }
    }
    let age = crate::real(pr(w, id).age);
    if age == lr(w, id).grav_at {
        set_gravity_scale(pm(w, id), 8.0);
    }
    if age == lr(w, id).arena_at {
        set_instance_vars(w, id, vec![("DestroyOnExitedArena?".to_owned(), Value::Bool(true))]);
    }
    if age == lr(w, id).return_at {
        // @57: MoveLureToTomMulticast(the Instigator + Z 200, Lerp(0.05, 0.3, NormalizeToRange(distance, 300, 2500)),
        // unclamped).
        let tx = w.fighter(f).body.x;
        let ty = actor_y(w, f) + 200.0;
        let (px, py) = (pr(w, id).x, pr(w, id).y);
        let n = (size([0.0, tx - px, ty - py]) - 300.0) / 2200.0;
        let dur = 0.05 + (n * (0.3 - 0.05));
        let cur = lr(w, id).mv;
        lx(w, id).mv = ComponentMove::call(cur, px, py, tx, ty, false, false, dur, 0);
        pm(w, id).returning = true;
        // @12355-12401: K2_SetActorRotation(FindLookAtRotation(the target, the lure)): the direction from Tom's point to
        // the lure, (0, -dx, dy) in UE (the zero vector's rotator is zero: rot_x -1, as aim_down_tick).
        let (dx, dy) = (px - tx, py - ty);
        set_rotation(pm(w, id), rot_from_x([0.0, -dx, dy])[0], if dx > 0.0 { 1.0 } else { -1.0 });
    }
    if age == lr(w, id).destroy_at {
        return true;
    }
    // GrabJerry (Deviation fishing c).
    if lr(w, id).grab_bound && lr(w, id).pulling_jerry.is_none() {
        let types = [crate::real(OBJ_PHYSICS_BODY), crate::real(OBJ_PAWN)];
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        let found = sphere_overlap_actors(w, x, y, 80.0, &types, &[Some(ActorRef::Projectile(id))], -1.0);
        for a in found {
            if let Some(j) = live_jerry_of(w, a, f) {
                take_jerry(w, id, j);
                break;
            }
        }
    }
    if let Some(ActorRef::Actor(j)) = lr(w, id).pulling_jerry
        && w.actors.actors.get(j).is_some_and(|a| !a.dead)
    {
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        if let Some(s) = jerry_mut(w, j) {
            s.x = x;
            s.y = y;
        }
    }
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let l = lx(w, id);
    l.cable_x = fx;
    l.cable_y = fy;
    false
}

fn bounce_impact(w: &mut World, id: ProjectileId, _nx: Real, _ny: Real, ivx: Real, ivy: Real) -> bool {
    bait_pull(w, id);
    {
        let l = lx(w, id);
        l.has_bounced = true;
        l.ribbon = false;
    }
    if size([0.0, ivx, ivy]) >= 1500.0 {
        sound(w, id, name!("sfx_cha_tomandjerry_fishing_impact"));
    }
    if hitbox_active(pr(w, id)) {
        set_hitbox(pm(w, id), false);
    }
    false
}

fn hit_enemy(w: &mut World, id: ProjectileId, _v: FighterId) {
    hit_enemy_part(w, id);
}

fn activate(w: &mut World, id: ProjectileId) {
    if activate_object(w, id) {
        destroy_projectile(w, id, false);
    }
}

fn any_damage(w: &mut World, id: ProjectileId, dmg: Real, _by: ActorRef) -> bool {
    let hp = lr(w, id).lure_hp - dmg;
    lx(w, id).lure_hp = hp;
    if hp > 0.0 {
        return false;
    }
    // Destroy Visuals @8282 and Delay(0.25) @7714.
    let age = crate::real(pr(w, id).age);
    let p = pm(w, id);
    set_gravity_scale(p, 0.0);
    p.vx = 0.0;
    p.vy = 0.0;
    let l = lx(w, id);
    l.show_impact = false;
    l.destroy_at = age + latent_frames(0.25);
    false
}

fn destroyed(w: &mut World, id: ProjectileId) {
    if !lr(w, id).show_impact {
        return;
    }
    emitter(w, id, DESTROYED_FX, 1.3);
    sound(w, id, name!("sfx_cha_tomandjerry_fishing_end"));
}

fn end(w: &mut World, id: ProjectileId) {
    let f = owner(w, id);
    if lr(w, id).refresh_jerry && tj_of(w, f).is_some() {
        set_is_jerry_attached(w, f, true);
    }
    let Some(ActorRef::Actor(j)) = lr(w, id).pulling_jerry else { return };
    if w.actors.actors.get(j).is_some_and(|a| !a.dead) {
        set_reset_jerry(w, j, true);
        jerry_destroy(w, j);
    } else if tj_of(w, f).is_some() {
        set_is_jerry_attached(w, f, true);
    }
}

/// The lure's `setCondition` (BPI_SetActivationConditions row `n`). Called by `world::spawning` for a lure.
pub fn set_condition(w: &mut World, p: ProjectileId, n: i32, v: bool) {
    let Ok(i) = usize::try_from(n) else { return };
    if let Some(c) = lx(w, p).conds.get_mut(i) {
        *c = v;
    }
}

/// fishing.rs's part of the joined montageEnded (its only one): the main mesh's OnMontageEnded, bound by every lure at
/// its BeginPlay (@6310): counted here, the lure compares (Deviation fishing a).
pub fn montage_ended(w: &mut World, f: FighterId, m: Name) {
    let _ = m;
    if let Some(s) = tj_mut(w, f) {
        s.fishing.montage_ends += 1;
    }
}

/// This file's registry entries (Buff_TomAndJerry_Bait_C, FishingLure_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(BAIT, BuffEntry::new(BuffClass::BuffTomAndJerryBait, BuffHooks::default()))?;
    let kind = ProjectileHooks {
        // ReceiveTick @10251 does not call ParentProjectilePhysics'
        own_tick: true,
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        bounce_impact: Some(bounce_impact),
        hit_enemy: Some(hit_enemy),
        activate: Some(activate),
        any_damage: Some(any_damage),
        destroyed: Some(destroyed),
        end: Some(end),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        LURE,
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::FishingLure), ActorHooks::Projectile(kind)),
    )?;
    Ok(())
}
