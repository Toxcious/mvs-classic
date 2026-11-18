//! Item_Meeseek_C: one of Bugs Bunny's ACME box pool ([`MeeseekItem`]), and the
//! runner it spawns, SelfActivate_Meeseeks_Run_C ([`MeeseeksRun`], data only: SelfActivate true) on Meeseeks_Run on
//! Meeseeks_Base, a ParentProjectilePhysics child. Offsets are the ubergraph's unless named.
//!
//! The box. CDO: Durability 999, CanUseItem, FragileAfterSpent, MaxUseCount 3, BasketballBounceMultiplier .3,
//! CanBasketballBounce true, OnHitBounce 500; Capsule 50 / 50 at scale 1.25.
//! - HitEnemyPlayer @10: the parent's (`Item::spend`), then a fighter of another team than the box's Destructible
//!   team: CurrentUseCount + 1, greater than MaxUseCount -> K2_DestroyActor @439.
//! - Thrown @1010: the parent's, CanBasketballBounce = true @1029, MeeseekBox.SetWorldScale3D(1.75).
//! - ReceiveHit @1630: the parent's, then MeeseekBox world scale 1.75. Then the Capsule's OnComponentHit @458 (after
//!   the actor's ReceiveHit, once per body contact of the step: ItemHooks::component_hit; not when the parent's tail
//!   destroyed the box): authority,
//!   CanBasketballBounce, the other actor not a fighter: |SpawnedActorParentVelocity| > 1100 ->
//!   AddImpulse(MirrorVectorByNormal(v x BasketballBounceMultiplier, Hit.Normal), velocity change) @930 (added to
//!   the parent's ItemHitGroundImpulse: both pending on the body that the parent has just made dynamic); else
//!   CanBasketballBounce = false @994.
//! - ActivateObject @1090: ItemActorParent's (Durability 999 -> 1: the next throw hit breaks it), then the
//!   Instigator's Comp_Spawner.CompSpawnActor(SelfActivate_Meeseeks_Run_C, no attach, WorldTransforms false,
//!   translation (0, 68, 180), direction (1, 0, 0), 1200, 1200, the Instigator) @1337; CurrentUseCount + 1, equal to
//!   MaxUseCount -> K2_DestroyActor @1611. One counter for the uses and the enemy hits.
//!
//! The runner (the Capsule a simulating sphere, r = hh 154, ProjectileNavPlatform, X translation and X / Y / Z
//! rotation locked, mass 150, LinearDamping 0; EnableProjectileMovement false, so SAP's BeginPlay deactivates the
//! movement).
//! - Meeseeks_Base BeginPlay @3594: SAP's (SetTeamColor @13150 -> Base @3094 / @2464: PS_MeeseeksSpawn and
//!   sfx_cha_tutorial_meeseeks_cast at the actor), ClearExtraMeeseeks @862 (every other actor of its class with the
//!   same Instigator, MaxMeeseeksOfClass 1), the Instigator's facing, team; authority: the Instigator in hitstun ->
//!   K2_DestroyActor @1839, else its DamageManager KnockedBack bound @2051 -> destroy @2754.
//! - Meeseeks_Run BeginPlay @1745: Base's, CreateProxyObjectForPlayMontage(Meeseeks_SkelMesh,
//!   Meeseeks_Attack_Run_Montage, 1.0, 0.0, None) @1755, authority @918: vo_meeseeks_sys_spawn at the actor,
//!   Direction = the Instigator's forward vector @1138; GetFighterCharacter != 7 OR SelfActivate (always for this
//!   class) -> Delay(SelfActivationDelay .25) @1389 -> ActivateObject @294.
//! - ActivateObject: Base's @296 (Activated, CanBeReflected; HitTheGym @510 is empty; SetChargeValues @533: the
//!   owner's GetChargeValue 1 / 0 / 2 / 4, unclamped; KnockedBack unbound @755; SetActorEnableCollision(false) @796,
//!   Delay(0) @807 -> true @15), then Run's DoOnce @377: AdvanceMontage @458, Running @5198, HitBoxOn? and
//!   UpdateHitbox @503-514, Delay(Lerp(MinTime 1, MaxTime 5, clamp(the Instigator's GetChargeValue(4)))) @859 ->
//!   K2_DestroyActor @15 and vo_meeseeks_sys_despawn at the actor @71.
//! - ReceiveTick @2227 (ParentProjectilePhysics' first), while Running, authority: SphereTraceSingle(loc + (0, +-50,
//!   0), 75 further, radius 50, ProjectileGroundTrace, ignore self) @2536; a blocking hit whose normal is not zero and
//!   |N.Y| > |N.Z| -> the gate @1567: SetFacing(!FacingLeft) @1667, Delay(.5) @1690 -> reopen @150. Then
//!   Capsule.SetPhysicsLinearVelocity((V.X, Direction.Y < 0 ? -PM.MaxSpeed : PM.MaxSpeed, min(V.Z, 10))) @3563 (set
//!   ahead of the frame's simulate, gravity added by the step) and the rotation by FacingLeft @3685.
//! - SetFacing @4344: Base's @2162 (FacingLeft, the hit box's facing and the rotation), Direction.Y = left ? |Y| :
//!   -|Y|.
//! - Reflected @5127: SAP's, then (authority, Running) the gated turn @1527.
//! - ReceiveDestroyed @3609 -> @2774 -> EndVisuals @2353: sfx_cha_tutorial_meeseeks_despawn at (0, 0, 0).
//! - AdvanceMontage @4672-5032 (ActivateObject's @458, with ActivationConditions[0]: the class default false, as no
//!   Comp_Spawner call of this class sets a condition): Montage_GetCurrentSection == 'Charging' ->
//!   Montage_JumpToSection(EarlyRelease ? 'DelayedRun' : 'Charge Release').
//! - The montage's proxy (bindings @1859-2138): OnCompleted @4626 -> K2_DestroyActor and vo_meeseeks_sys_despawn at
//!   the actor (@173); OnBlendOut @5142, OnInterrupted @5170, OnNotifyBegin @309 and OnNotifyEnd @2184 are empty.
//!   Its sections loop (Charging -> Charging, Charge Release -> Charge Release): it never completes. Its notifies
//!   (all Queued) run on the actor: Ak_OneShot sfx_cha_C020_meeseeks_attack, Ak_SurfaceEffect footsteps,
//!   AnimNotify_PlayVFX PS_MeeseeksNAF2 and AnimNotify_DestroyUnactivatedMeeseeks (at 1.0 s, in Charge Release,
//!   which only AdvanceMontage reaches, after Activated is set: nothing).
//! - Not ported, by the build: SetVelocity @3716 (no caller).
//!
//! Die With Instigator: the world's respawn (the SpawnedActors pass reads the class data's flag).

use crate::Real;
use crate::State;
use crate::actors::actor_montage::{
    ActorAnim, MontageEvent, montage_current_section, montage_jump_to_section, montage_play,
};
use crate::actors::item::{ItemContact, ItemExt, item_activate_parent, item_destructible_team};
use crate::actors::projectile::{self, ProjectileExt, class_p};
use crate::actors::tunnel_carry;
use crate::actors::{ActorClass, ActorRef, ItemClass, ItemId, Projectile, ProjectileClass, ProjectileId};
use crate::combat::elem;
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::{actor_def, instigator_of, latent_frames, sound_at_for, warn_parts, warn_text};
use crate::fighter::moves::V2;
use crate::fighter::traces::solids::sphere_static;
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND, sphere_trace_single};
use crate::fighter::{self, FighterId};
use crate::math::ue::size2d;
use crate::math::{max, min};
use crate::name;
use crate::registry::body_settings::sphere_settings;
use crate::registry::entries::{CarryHooks, ItemHooks, MeshHooks, ProjectileHooks, ReflectHooks};
use crate::registry::props::{num, obj};
use crate::registry::{ActorEntry, ActorHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::items::destroy_item;
use crate::world::spawned_platform::BlockKind;
use crate::world::spawning::{comp_spawn_projectile, destroy_projectile};
use crate::world::stage_query::rigid_statics;
use mvs_collision::Ground;
use mvs_collision::rigid_sphere::{Body, DEFAULT_K, Settings, Solver, Static};
use mvs_data::value::Props;

const BOX: &str = "Item_Meeseek_C";
const RUN: &str = "SelfActivate_Meeseeks_Run_C";

/// A number of the class data the graph reads (no default: a missing one warns and reads 0).
fn data_num(w: &World, p: Option<&Props>, key: &str, cls: &str) -> Real {
    if let Some(v) = p.and_then(|p| num(p, key)) {
        return v;
    }
    warn_parts(w, &[cls, " data missing: ", key]);
    0.0
}

// ---------------------------------------------------------------- Item_Meeseek_C

/// `MeeseekItem extends Item` (Item_Meeseek_C). Ported for a spawn with no owner (the FFA area's): the box reads its
/// owner's team in HitEnemyPlayer (`item_destructible_team`) and its user in ActivateObject.
#[derive(State, Debug)]
pub struct MeeseekItem {
    /// CurrentUseCount.
    pub use_count: i32,
    /// CanBasketballBounce.
    pub can_bounce: bool,
    /// MeeseekBox at world scale 1.75 (Thrown, ReceiveHit), never set back: the view's.
    pub big_box: bool,
    /// SpawnedActorParentVelocity: GetVelocity at the parent's ReceiveTick, the actor's last tick before a hit.
    pub pvx: Real,
    pub pvy: Real,
}

impl Default for MeeseekItem {
    fn default() -> Self {
        MeeseekItem { use_count: 0, can_bounce: true, big_box: false, pvx: 0.0, pvy: 0.0 }
    }
}

fn box_state(w: &mut World, it: ItemId) -> &mut MeeseekItem {
    match &mut w.actors.items.get_mut(it).expect("a live item").ext {
        ItemExt::MeeseekItem(m) => m,
        _ => unreachable!("Item_Meeseek_C holds a MeeseekItem"),
    }
}

/// A number of the box's class data.
fn box_num(w: &World, it: ItemId, key: &str) -> Real {
    let i = w.actors.items.get(it).expect("a live item");
    data_num(w, Some(i.def(&w.data).p), key, BOX)
}

/// `thrown`: CanBasketballBounce = true @1029, MeeseekBox.SetWorldScale3D(1.75).
fn thrown(w: &mut World, it: ItemId) {
    let m = box_state(w, it);
    m.can_bounce = true;
    m.big_box = true;
}

/// `hit_enemy` @10: the Destructible's TeamIndex (the last grabber's) against the fighter's; true is the
/// K2_DestroyActor @439.
fn hit_enemy(w: &mut World, it: ItemId, vic: FighterId) -> bool {
    if w.fighter(vic).team.team == item_destructible_team(w, it) {
        return false;
    }
    let max = box_num(w, it, "MaxUseCount");
    let m = box_state(w, it);
    m.use_count += 1;
    crate::real(m.use_count) > max
}

/// `receive_hit` @1630, behind the parent's (script after its K2_DestroyActor runs on): MeeseekBox at world scale
/// 1.75.
fn receive_hit(w: &mut World, it: ItemId, _destroyed: bool) -> bool {
    box_state(w, it).big_box = true;
    false
}

/// The Capsule's OnComponentHit @458, for each of the step's body contacts behind ReceiveHit (not on a pending-kill
/// component): authority, CanBasketballBounce, the other actor not a fighter (the contacts are stage solids alone):
/// |SpawnedActorParentVelocity| > 1100 -> AddImpulse(MirrorVectorByNormal(v x BasketballBounceMultiplier,
/// Hit.Normal), velocity change) @930, on top of the parent's impulse; else CanBasketballBounce = false @994.
fn component_hit(w: &mut World, it: ItemId, c: ItemContact) -> bool {
    let (can, pvx, pvy) = {
        let m = box_state(w, it);
        (m.can_bounce, m.pvx, m.pvy)
    };
    if !can {
        return false;
    }
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated comparison !(a > b): NaN reads as slow")]
    if !(size2d([pvx, pvy]) > 1100.0) {
        box_state(w, it).can_bounce = false; // @994
        return false;
    }
    let (nx, ny) = (c.nx, c.ny);
    // MirrorVectorByNormal(v x k, N) = v k - 2 ((v k) . N) N, a velocity change on top of the parent's impulse.
    let k = box_num(w, it, "BasketballBounceMultiplier");
    let (vx, vy) = (pvx * k, pvy * k);
    let d = vx * nx + vy * ny;
    let i = w.actors.items.get_mut(it).expect("a live item");
    i.vx += vx - 2.0 * d * nx;
    i.vy += vy - 2.0 * d * ny;
    false
}

/// `tick`: SpawnedActorParent ReceiveTick @8803 (the box has no tick of its own): SpawnedActorParentVelocity =
/// GetVelocity.
// Unverified: taken while the box is not in a hand (ItemHooks::tick); in a hand the value of its last free tick
// stays, so a throw hit in the throw's first frame reads it (the attached root's velocity is not read).
fn tick(w: &mut World, it: ItemId) -> bool {
    let (vx, vy) = {
        let i = w.actors.items.get(it).expect("a live item");
        (i.vx, i.vy)
    };
    let m = box_state(w, it);
    m.pvx = vx;
    m.pvy = vy;
    false
}

/// `activate` @1090: ItemActorParent's (no DestroyWhenSpent: the box stays), then the runner @1337.
fn activate(w: &mut World, it: ItemId, user: FighterId) {
    item_activate_parent(w, it);
    // The Instigator's Comp_Spawner, translation (0, 68, 180) relative to the Instigator (WorldTransforms false), as
    // the notify path places a socket-None spawn (the item branch: the actor's location - (0, 0, 100) plus the
    // translation, mirrored by facing).
    // Unverified: HandleActorPostSpawn's relative branch for a graph's call is taken as the notify path's rule.
    let facing = fighter::facing(w, user);
    let x = w.fighter(user).body.x + 68.0 * facing;
    let y = fighter::actor_y(w, user) - 100.0 + 180.0;
    comp_spawn_projectile(
        w,
        user,
        name!("SelfActivate_Meeseeks_Run_C"),
        x,
        y,
        [Some(1.0), Some(0.0), Some(0.0)],
        1200.0,
        1200.0,
        None,
    );
    let max = box_num(w, it, "MaxUseCount");
    let m = box_state(w, it);
    m.use_count += 1;
    if crate::real(m.use_count) == max {
        destroy_item(w, it); // @1611
    }
}

fn box_sub() -> ItemExt {
    ItemExt::MeeseekItem(MeeseekItem::default())
}

// ---------------------------------------------------------------- SelfActivate_Meeseeks_Run_C

/// The class's body and hit box, from its template.
struct RunStatic {
    /// The Capsule's FBodyInstance (`None`: not stepped, warned by `sphere_settings`).
    set: Option<Settings>,
    /// The Capsule's radius.
    radius: Real,
    /// The Body sphere (HitBoxTags [Body]): SphereRadius x the smallest axis of its world scale.
    body_r: Real,
    /// Its centre below the Capsule's (UE Z).
    body_off: Real,
}

/// `staticOf`: the class data of `cls` through the fighter `own` (the spawner).
fn static_of(w: &World, own: FighterId, cls: &str) -> Option<RunStatic> {
    let c = w.names.get(cls)?;
    let a = actor_def(w, ActorRef::Fighter(own), c)?;
    let cs = &a.components;
    let cap = obj(cs, "Capsule");
    let r = data_num(w, cap, "CapsuleRadius", cls);
    let hh = data_num(w, cap, "CapsuleHalfHeight", cls);
    if hh != r {
        warn_text(w, &format!("{cls}: the Capsule is not a sphere (half height {hh}, radius {r}): stepped as one"));
    }
    // Body (tags Body, HitBox) under Meeseeks_SkelMesh (relative Z -153, scale 1.155) under the Capsule
    // (bAbsoluteScale, 1): radius 120 x 1.155, its centre at -153 + 33.333 x 1.155.
    let body = obj(cs, "Body");
    let mesh = body.and_then(|b| b.get("AttachParent")).and_then(|v| v.as_str()).and_then(|k| obj(cs, k));
    let under = mesh.is_some_and(|m| m.get("AttachParent").and_then(|v| v.as_str()) == Some("Capsule"));
    if body.is_none() || mesh.is_none() || !under {
        warn_text(w, &format!("{cls}: the Body sphere is not under a mesh under the Capsule: its place not read"));
    }
    let scale = |p: Option<&Props>| -> [Real; 3] {
        let s = p.and_then(|p| obj(p, "RelativeScale3D"));
        let g = |k: &str| s.and_then(|s| num(s, k)).unwrap_or(1.0);
        [g("X"), g("Y"), g("Z")]
    };
    let (ms, bs) = (scale(mesh), scale(body));
    let wscale = min(min((ms[0] * bs[0]).abs(), (ms[1] * bs[1]).abs()), (ms[2] * bs[2]).abs());
    let z = |p: Option<&Props>| p.and_then(|p| obj(p, "RelativeLocation")).and_then(|l| num(l, "Z")).unwrap_or(0.0);
    Some(RunStatic {
        set: sphere_settings(w, cap.and_then(|c| obj(c, "BodyInstance")), cls),
        radius: r,
        body_r: data_num(w, body, "SphereRadius", cls) * wscale,
        body_off: z(mesh) + z(body) * ms[2],
    })
}

/// `MeeseeksRun extends Projectile` (SelfActivate_Meeseeks_Run_C).
#[derive(State, Debug)]
pub struct MeeseeksRun {
    /// The Capsule body's linear velocity (sim plane).
    pub bvx: Real,
    pub bvy: Real,
    /// Its spin (0 under the X rotation lock).
    pub bspin: Real,
    /// FacingLeft.
    pub facing_left: bool,
    /// Direction.Y's side in the sim's x: +1 toward +x (UE Direction.Y < 0).
    pub dir: Real,
    /// Activated.
    pub activated: bool,
    /// CanBeReflected (the class default false; Base ActivateObject sets it).
    pub can_reflect: bool,
    /// Running.
    pub running: bool,
    /// Meeseeks_SkelMesh's montages (`anim`, `actors::actor_montage::anim_of`).
    pub anim: ActorAnim,
    /// The Instigator's KnockedBack bound to OnInstigaatorKnockedBack (Base BeginPlay, authority).
    pub kb_bound: bool,
    /// The Instigator's knockedBackCount at the bind.
    pub kb_at: i32,
    /// GetInstigator(): the spawning fighter's id. A reflect changes the owner alone (SAP @23895 /
    /// Comp_ReflectProjectile @1385 SetOwner); the class's data stays the spawner's too.
    pub inst: i32,
    /// The age the SelfActivationDelay fires at (-1: none).
    pub activate_at: Real,
    /// The age ActivateObject's Delay(0) fires at.
    pub collision_at: Real,
    /// The gate @1567 closed.
    pub turn_closed: bool,
    /// The age its Delay(.5) fires at.
    pub turn_at: Real,
    /// The age the run's Delay fires at.
    pub end_at: Real,
    /// The Body sphere's centre below the root (the hit's y offset).
    pub off_y: Real,
}

impl Default for MeeseeksRun {
    /// The field initialisers (a loaded record's own values replace them).
    fn default() -> Self {
        MeeseeksRun {
            bvx: 0.0,
            bvy: 0.0,
            bspin: 0.0,
            facing_left: false,
            dir: 0.0,
            activated: false,
            can_reflect: false,
            running: false,
            anim: ActorAnim::default(),
            kb_bound: true,
            kb_at: 0,
            inst: 0,
            activate_at: -1.0,
            collision_at: -1.0,
            turn_closed: false,
            turn_at: -1.0,
            end_at: -1.0,
            off_y: 0.0,
        }
    }
}

fn run_ref(p: &Projectile) -> &MeeseeksRun {
    match &p.ext {
        ProjectileExt::MeeseeksRun(r) => r,
        e => panic!("SelfActivate_Meeseeks_Run_C without its state: {e:?}"),
    }
}

fn st(w: &World, id: ProjectileId) -> &MeeseeksRun {
    run_ref(w.actors.projectiles.get(id).expect("a live projectile"))
}

fn st_mut(w: &mut World, id: ProjectileId) -> &mut MeeseeksRun {
    match &mut w.actors.projectiles.get_mut(id).expect("a live projectile").ext {
        ProjectileExt::MeeseeksRun(r) => r,
        e => panic!("SelfActivate_Meeseeks_Run_C without its state: {e:?}"),
    }
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pr_mut(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

/// `hit_xy`'s part for the runner: the Body sphere, under the mesh below the root.
pub fn hit_xy(p: &Projectile) -> (Real, Real) {
    (p.x, p.y + run_ref(p).off_y)
}

/// `hit_velocity`'s part for the runner: GetVelocity of the simulating root: its body's.
pub fn hit_velocity(p: &Projectile) -> (Real, Real) {
    let r = run_ref(p);
    (r.bvx, r.bvy)
}

/// The constructor (`new MeeseeksRun`): the movement stopped, the body's start, the Instigator's facing and the
/// Delay's end.
fn construct(w: &World, p: &mut Projectile) {
    let f = p.owner.expect("a runner a fighter's Comp_Spawner made");
    let cls = w.names.str(p.cls);
    // SAP BeginPlay with EnableProjectileMovement false: ProjectileMovement.Deactivate (SAP @3777-3792). The root
    // simulates from its registration (the template's bSimulatePhysics).
    p.stopped = true;
    let fm = w.fighter(f);
    // Comp_Spawner HandleActorPostSpawn @11329, the impulse branch: direction x speed ((1200, 0, 0): the X lock takes
    // it) + the owner's velocity / 2, as a velocity change on the body.
    let (bvx, bvy) = (fm.body.vx / 2.0, fm.body.vy / 2.0);
    // Base BeginPlay: the Instigator's facing (SetFacing: FacingLeft, the hit box's facing and the rotation); Run
    // BeginPlay @1138: Direction = the Instigator's forward vector.
    let facing = fm.body.facing_dir;
    p.facing = facing;
    let st = static_of(w, f, cls);
    p.hit_radius = st.as_ref().map_or(0.0, |s| s.body_r * p.scale);
    let off_y = st.as_ref().map_or(0.0, |s| s.body_off * p.scale);
    // @1341: BPI_GetFighterCharacter != 7 (Pawn_C020's) OR SelfActivate -> Delay(SelfActivationDelay). The actor ticks
    // in its spawn frame and the BeginPlay Delay counts it. No ported pawn is Pawn_C020 (EFighter 7), so the test
    // holds for every instigator, whatever SelfActivate says.
    let activate_at = latent_frames(data_num(w, Some(class_p(w, p)), "SelfActivationDelay", cls));
    p.ext = ProjectileExt::MeeseeksRun(MeeseeksRun {
        bvx,
        bvy,
        bspin: 0.0,
        facing_left: facing < 0.0,
        dir: facing,
        activated: false,
        can_reflect: false,
        running: false,
        anim: ActorAnim::new(name!("Meeseeks_SkelMesh")),
        kb_bound: true,
        kb_at: fm.attackers.knocked_back_count,
        inst: fm.id,
        activate_at,
        collision_at: -1.0,
        turn_closed: false,
        turn_at: -1.0,
        end_at: -1.0,
        off_y,
    });
}

/// SetFacing (Run @4344 -> Base @2162 -> @139): FacingLeft, the hit box's facing and the actor's rotation; then
/// Direction.Y = left ? |Y| : -|Y| (the sim's x is -UE Y).
fn set_facing(w: &mut World, id: ProjectileId, left: bool) {
    pr_mut(w, id).facing = if left { -1.0 } else { 1.0 };
    let s = st_mut(w, id);
    s.facing_left = left;
    s.dir = if left { -1.0 } else { 1.0 };
}

/// The gate @1567 (the tick's wall and Reflected): closed, SetFacing(!FacingLeft) @1667, Delay(.5) @1690 -> reopen
/// @150. `in_tick`: made inside the actor's ReceiveTick, ahead of that tick's latent actions, which count this frame;
/// from Reflected (a hit, after the tick) the next frame's tick is the first.
fn gated_turn(w: &mut World, id: ProjectileId, in_tick: bool) {
    if st(w, id).turn_closed {
        return;
    }
    st_mut(w, id).turn_closed = true;
    let left = st(w, id).facing_left;
    set_facing(w, id, !left);
    let age = crate::real(pr(w, id).age);
    st_mut(w, id).turn_at = age + latent_frames(0.5) - if in_tick { 1.0 } else { 0.0 };
}

/// ActivateObject: Meeseeks_Base's @296, then Run's DoOnce @377. Run from the SelfActivationDelay's link, inside the
/// actor's latent actions: its own Delays count from the next frame.
fn activate_run(w: &mut World, id: ProjectileId) {
    let a = ActorRef::Projectile(id);
    let age = crate::real(pr(w, id).age);
    let owner = pr(w, id).owner.expect("a runner a fighter's Comp_Spawner made");
    let cls = w.names.str(pr(w, id).cls).to_owned();
    {
        let s = st_mut(w, id);
        s.activated = true;
        s.can_reflect = true;
    }
    // HitTheGym @510 (GetOwner's GetChargeValue(4) >= .75): an empty event.
    // SetChargeValues @533: the owner's GetChargeValue 1 / 0 / 2 / 4 into the actor's own component, unclamped and
    // with no ReceiveChargeValuesFromInstigator test (neither `receive_charge` nor `set_charge_variables`).
    let c = elem::instigator_charge(w, owner);
    {
        let p = pr_mut(w, id);
        p.charge = c;
        p.no_collision = true; // SetActorEnableCollision(false) @796
    }
    {
        let s = st_mut(w, id);
        s.kb_bound = false; // @755
        s.collision_at = age + latent_frames(0.0); // Delay(0) @807 -> @15
        if s.running {
            return; // the DoOnce @377 (Running is set behind it and never cleared)
        }
    }
    // AdvanceMontage @458 (EarlyRelease: ActivationConditions[0], false)
    let cur = montage_current_section(w, a);
    if w.names.str(cur).eq_ignore_ascii_case("Charging") {
        montage_jump_to_section(w, a, name!("Charge Release"));
    }
    st_mut(w, id).running = true; // StartRunning @5198
    projectile::set_hitbox(pr_mut(w, id), true); // HitBoxOn? = true, UpdateHitbox @503-514
    // @710-859: the Instigator's GetChargeValue(4) (its live charge in its charged montage, else 0).
    // @629: no Comp_Pawn_ChargeAttack on None: the flow ends with no Delay (the run has no end of its own).
    let Some(inst) = instigator_of(w, st(w, id).inst) else { return };
    let pct = elem::instigator_charge(w, inst).map_or(0.0, |c| c.pct);
    let (lo, hi) = {
        let p = Some(class_p(w, pr(w, id)));
        (data_num(w, p, "MinTime", &cls), data_num(w, p, "MaxTime", &cls))
    };
    let t = lo + (hi - lo) * min(max(pct, 0.0), 1.0);
    st_mut(w, id).end_at = age + latent_frames(t);
}

/// Meeseeks_Base SetTeamColor @3094 -> @2464 (not on a dedicated server): the mesh's team material (the view's),
/// SpawnEmitterAtLocation(PS_MeeseeksSpawn, the actor, scale 1) @2492 and sfx_cha_tutorial_meeseeks_cast at the actor
/// @2609.
fn spawn_visuals(w: &mut World, id: ProjectileId) {
    let (cls, fighter, x, y) = {
        let p = pr(w, id);
        (p.cls, p.owner_id, p.x, p.y)
    };
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(name!("PS_MeeseeksSpawn")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    });
    let inst = instigator_of(w, st(w, id).inst); // (GetInstigator: the RTPC's)
    sound_at_for(w, inst, name!("sfx_cha_tutorial_meeseeks_cast"), x, y);
}

/// `begin`: Meeseeks_Run BeginPlay @1745 behind Base's @3594.
fn begin(w: &mut World, id: ProjectileId) {
    let (owner, cls, x, y) = {
        let p = pr(w, id);
        (p.owner.expect("a runner a fighter's Comp_Spawner made"), p.cls, p.x, p.y)
    };
    // SAP BeginPlay's SetTeamColor @13150 (Base @3094 -> @2464): PS_MeeseeksSpawn and the cast sound at the actor.
    spawn_visuals(w, id);
    // ClearExtraMeeseeks @862: the other actors of its class with its Instigator (Max 1: all of them).
    let max_of_class = data_num(w, Some(class_p(w, pr(w, id))), "MaxMeeseeksOfClass", w.names.str(cls));
    let inst = st(w, id).inst;
    let others: Vec<ProjectileId> = w
        .actors
        .projectiles
        .iter()
        .filter(|(o, p)| {
            *o != id && !p.dead && p.cls == cls && matches!(&p.ext, ProjectileExt::MeeseeksRun(r) if r.inst == inst)
        })
        .map(|(o, _)| o)
        .collect();
    let n = crate::real(u32::try_from(others.len()).expect("a short list"));
    if n >= max_of_class {
        let mut i = 0.0;
        while i <= n - max_of_class {
            let Some(&o) = usize::try_from(crate::math::to_int32(i)).ok().and_then(|k| others.get(k)) else { break };
            destroy_projectile(w, o, false);
            i += 1.0;
        }
    }
    // @1839: the Instigator's Comp_Pawn_HitstunManager.CheckHitstunActive (`in_hitstun`) -> K2_DestroyActor; the
    // script runs on: Run's spawn VO below still plays and its Delay never fires.
    if fighter::in_hitstun(w, owner) {
        st_mut(w, id).kb_bound = false;
        destroy_projectile(w, id, false);
    }
    montage_play(w, ActorRef::Projectile(id), name!("Meeseeks_Attack_Run_Montage"), 1.0, 0.0, true); // @1755
    sound_at_for(w, None, name!("vo_meeseeks_sys_spawn"), x, y); // @990 (Instigator null)
}

/// `step`: the actor's ReceiveTick @2227 and its latent actions; true is its K2_DestroyActor.
fn step(w: &mut World, id: ProjectileId) -> bool {
    // OnInstigaatorKnockedBack @2769 -> K2_DestroyActor @2754.
    // Deviation: the Instigator's KnockedBack broadcast is taken at the actor's next tick (`knocked_back_count`
    // polled, as Morty's whip), not inside the hit. A pawn that is gone broadcasts nothing.
    let (inst, kb_bound, kb_at) = {
        let s = st(w, id);
        (s.inst, s.kb_bound, s.kb_at)
    };
    if kb_bound && instigator_of(w, inst).map_or(kb_at, |f| w.fighter(f).attackers.knocked_back_count) != kb_at {
        return true;
    }
    // ReceiveTick @2227 (ParentProjectilePhysics' first: `projectile::step`), while Running, authority.
    if st(w, id).running {
        let left = st(w, id).facing_left;
        let (px, py) = (pr(w, id).x, pr(w, id).y);
        let x0 = px + if left { -50.0 } else { 50.0 };
        let x1 = x0 + if left { -75.0 } else { 75.0 };
        let h =
            sphere_trace_single(w, x0, py, x1, py, 50.0, TRACE_PROJECTILE_GROUND, &[], Some(ActorRef::Projectile(id)));
        if let Some(h) = h
            && h.blocking
            && (h.nx.abs() > 1e-4 || h.ny.abs() > 1e-4)
            && h.nx.abs() > h.ny.abs()
        {
            gated_turn(w, id, true);
        }
        // @3563: (V.X, +-PM.MaxSpeed, min(V.Z, 10)); @3685 the rotation by FacingLeft.
        let max_speed = pr(w, id).max_speed;
        let s = st_mut(w, id);
        s.bvx = s.dir * max_speed;
        s.bvy = min(s.bvy, 10.0);
        let left = s.facing_left;
        pr_mut(w, id).facing = if left { -1.0 } else { 1.0 };
    }
    // The latent actions, in the order of their links' age.
    let age = crate::real(pr(w, id).age);
    let s = st(w, id);
    let (act, col, turn, end) = (s.activate_at, s.collision_at, s.turn_at, s.end_at);
    if age == act {
        activate_run(w, id); // @294
    }
    if age == col {
        pr_mut(w, id).no_collision = false; // @15
    }
    if age == turn {
        st_mut(w, id).turn_closed = false; // @150
    }
    if age == end {
        // @15 K2_DestroyActor, then @71 vo_meeseeks_sys_despawn at the actor.
        destroy_projectile(w, id, true);
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        sound_at_for(w, None, name!("vo_meeseeks_sys_despawn"), x, y); // (Instigator null)
    }
    false
}

/// The body's physics step, after the actor's tick (TG_PrePhysics).
fn physics(w: &mut World, id: ProjectileId) {
    let (own, cls) = {
        let p = pr(w, id);
        // (the class's data: the spawner's)
        (instigator_of(w, st(w, id).inst).or(p.owner), p.cls)
    };
    let Some(own) = own else { return };
    let Some(set) = static_of(w, own, w.names.str(cls)).and_then(|s| s.set.map(|set| (set, s.radius))) else {
        return;
    };
    let (set, radius) = set;
    let p = pr(w, id);
    // Unverified: a reflect's SetMobility(Stationary) holds the body still (as the safe's).
    if p.mobility.is_some() {
        return;
    }
    let (no_collision, scale, x, y) = (p.no_collision, p.scale, p.x, p.y);
    let s = st(w, id);
    let mut body = Body {
        x: f64::from(x),
        y: f64::from(y),
        vx: f64::from(s.bvx),
        vy: f64::from(s.bvy),
        spin: f64::from(s.bspin),
        radius: f64::from(radius * scale),
        ..Body::default()
    };
    // ProjectileNavPlatform blocks WorldStatic and Platform (the stage's platforms, `World::blockers`); with the
    // actor's collision off (ActivateObject's frame) the body meets nothing.
    let (floor, list): (Static, Option<Vec<Static>>) = if no_collision {
        (nowhere(), None)
    } else {
        let shapes = w.blocker_shapes(BlockKind::Nav);
        (sphere_static(w, w.stage.ground), Some(rigid_statics(w, &shapes)))
    };
    Solver::default().sphere_step(&mut body, &floor, list.as_deref(), DELTA_SECONDS, &set, DEFAULT_K);
    let p = pr_mut(w, id);
    (p.x, p.y) = (crate::math::narrow(body.x), crate::math::narrow(body.y));
    let s = st_mut(w, id);
    (s.bvx, s.bvy, s.bspin) =
        (crate::math::narrow(body.vx), crate::math::narrow(body.vy), crate::math::narrow(body.spin));
}

/// The NOWHERE floor of a body with its collision off: a box far below everything.
fn nowhere() -> Static {
    Static {
        g: Ground { left: 0.0, right: 0.0, top: -1e9, bottom: -1e9 - 1.0 },
        kinematic: None,
        static_solid: false,
        poly: None,
    }
}

/// `destroyed`: ReceiveDestroyed @3609 -> @2774 (authority, !Activated: the unbind) -> EndVisuals @2353: the despawn
/// sound at the world origin (PF_Post_Event_At_Location's Location (0, 0, 0); the Instigator is only the RTPC's).
fn destroyed(w: &mut World, id: ProjectileId) {
    st_mut(w, id).kb_bound = false;
    let inst = instigator_of(w, st(w, id).inst);
    sound_at_for(w, inst, name!("sfx_cha_tutorial_meeseeks_despawn"), 0.0, 0.0);
}

fn run_id(a: ActorRef) -> ProjectileId {
    match a {
        ActorRef::Projectile(id) => id,
        _ => unreachable!("the runner is a projectile"),
    }
}

/// Reflected @5127: SAP's, then @1527 (authority, Running) the gated turn.
fn reflected(w: &mut World, a: ActorRef) {
    let id = run_id(a);
    if st(w, id).running {
        gated_turn(w, id, false);
    }
}

/// SAP MulticastReflect @23937 SetTeamColor: Base's, the spawn visuals again.
fn multicast(w: &mut World, a: ActorRef, _new_owner: FighterId, _pause: bool) {
    spawn_visuals(w, run_id(a));
}

/// BPI_CanBeReflected (SAP: CanBeReflected), which Base's ActivateObject sets.
fn can_be_reflected(w: &World, a: ActorRef) -> bool {
    st(w, run_id(a)).can_reflect
}

/// The proxy's OnCompleted @4626: K2_DestroyActor, then (the script runs on) vo_meeseeks_sys_despawn at the actor
/// @173. OnBlendOut @5142 and OnInterrupted @5170 are empty.
fn montage_event(w: &mut World, a: ActorRef, ev: MontageEvent, _montage: crate::names::Name) {
    if ev != MontageEvent::Completed {
        return;
    }
    let id = run_id(a);
    destroy_projectile(w, id, true);
    let (x, y) = (pr(w, id).x, pr(w, id).y);
    sound_at_for(w, None, name!("vo_meeseeks_sys_despawn"), x, y); // (Instigator null)
}

// Buff_ProjectileThroughTunnel's hooks: the shared Projectile ones but for the root, which simulates physics from its
// registration (the template's bSimulatePhysics) while its ProjectileMovement is deactivated (SAP BeginPlay
// @3777-3792): IsSimulatingPhysics passes the tunnel's test @10913-11036; the movement's Velocity reads 0
// (UMovementComponent::Deactivate -> StopMovementImmediately), so the SelectVector falls to GetVelocity, the body's;
// MulticastProjectileVelocity with the movement inactive is SetPhysicsLinearVelocity on the root: the body's.
fn carry_moving(_w: &World, _a: ActorRef) -> bool {
    true
}

fn carry_velocity(w: &World, a: ActorRef) -> V2 {
    let s = st(w, run_id(a));
    V2 { x: s.bvx, y: s.bvy }
}

fn carry_set_velocity(w: &mut World, a: ActorRef, vx: Real, vy: Real) {
    let s = st_mut(w, run_id(a));
    s.bvx = vx;
    s.bvy = vy;
}

/// This file's registry entries (Item_Meeseek_C, SelfActivate_Meeseeks_Run_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let item = ItemHooks {
        sub: Some(box_sub),
        thrown: Some(thrown),
        hit_enemy: Some(hit_enemy),
        receive_hit: Some(receive_hit),
        component_hit: Some(component_hit),
        tick: Some(tick),
        activate: Some(activate),
        ..ItemHooks::default()
    };
    r.actors.enter(
        BOX,
        ActorEntry {
            ownerless: true,
            ..ActorEntry::new(ActorClass::Item(ItemClass::ItemMeeseek), ActorHooks::Item(item))
        },
    )?;
    let run = ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        physics: Some(physics),
        destroyed: Some(destroyed),
        reflect: ReflectHooks {
            reflected: Some(reflected),
            multicast: Some(multicast),
            can_be_reflected: Some(can_be_reflected),
            ..ReflectHooks::default()
        },
        ..ProjectileHooks::default()
    };
    let carry = CarryHooks {
        moving: carry_moving,
        velocity: carry_velocity,
        set_velocity: carry_set_velocity,
        ..tunnel_carry::PROJECTILE
    };
    r.actors.enter(
        RUN,
        ActorEntry {
            carry: Some(carry),
            mesh: Some(MeshHooks { montage_event: Some(montage_event), ..MeshHooks::default() }),
            ..ActorEntry::new(
                ActorClass::Projectile(ProjectileClass::SelfActivateMeeseeksRun),
                ActorHooks::Projectile(run),
            )
        },
    )
}
