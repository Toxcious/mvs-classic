//! The marker: GarnetMarker2_C ([`GarnetMarker2`], a projectile).
//!
//! The down special's marker, a SpawnedActorParent child (offsets of its ubergraph). Spawned by Special_D_Montage2 /
//! Air_D_Montage2's AnimNotify_Spawner (offset (0, 0, 200), socket None, InitialSpeed 2000 forward) and by the gauntlet
//! perk route (GarnetGauntletsProjectile's EndPlay @3868: `comp_spawn_projectile`, which brings the place); activated
//! by MoveToMarker_Montage4's four AnimNotify_ActivateSpawned. Ability slot 1 (Cooldown -1, AlternateCooldowns [25]):
//! TriggerAbilityOnSpawnIndex triggers it at the spawn, EndPlay starts the alternate cooldown.
//!
//! The class's graph, by entry:
//! - ReceiveBeginPlay @6452 -> @5325: the parent's, then the queued flows in the order they pop: @5355 the Instigator's
//!   BPI_SetIdleGauntlet(self) @5518 (behind DoesImplementInterface(BPI_Garnet)); @6397 Delay(0.2) -> @15
//!   Capsule.SetCollisionEnabled(QueryOnly); @6218 InstigatingGarnet = GetInstigator, its OnBFDefeated bound to
//!   OnAttachedFighterRunOut; @6170 EnableProjectileMovement = false and ProjectileMovement.Deactivate(); @5557
//!   (authority, no attach parent) the location clamped per axis into [BlastBoxBoundsMin + BlastBoxBuffer,
//!   BlastBoxBoundsMax - BlastBoxBuffer] (FClamp @5909-6019, no sweep, teleport @6129).
//! - The Capsule's begin overlap @6457: push 6467, then @2995 (authority) InstigatingGarnet's GetAllies contains
//!   OtherActor: AttachedFighter = OtherActor @3281, Delay(0) @3745 -> @678 UpdateMaxLifetimeWithAuthority(MaxLifetime);
//!   whatever that test gave, the pop runs @6467 with the AttachedFighter as it stands: a fighter of the marker's
//!   TeamIndex @6683 -> StopMovementImmediately, Deactivate, Capsule.K2_AttachToComponent(its root, SnapToTarget)
//!   @6927, its OnBFDefeated bound to OnAttachedFighterRunOut @7046; then AddMarkerUIState @7454.
//! - AddMarkerUIState @7516 -> @54: no attach parent -> Delay(0) @623 -> @54 again; else StarVFX hidden @117 and
//!   AddStateForPlayer(GarnetMarker, type 1, display 0, values -1, timer -1, MI_State_GarnetStar, active, text '',
//!   auto-remove -1, no subicon) @151-568 on the attach parent's player.
//! - ActivateObject @4318: ActivationConditions[0] -> MulticastActivatedEffects (@7521) and Deactivate @4411; else [1]
//!   -> a DoOnce (open at the start) @4517: Activated = true, MulticastDetach @4599; neither -> ReleaseToPool @4614.
//!   The conditions are the actor's (BPI_SetActivationConditions: the notify's rows ahead of the call; class default
//!   [false, false]).
//! - MulticastDetach @8432: RemoveStateForPlayer(GarnetMarker) on the attach parent, else on AttachedFighter @8143;
//!   K2_DetachFromActor(KeepWorld) @8318, Capsule NoCollision, StarVFX shown, DestroyOnExitedArena? = false,
//!   CheckForBlastBox() @8417.
//! - OnAttachedFighterRunOut @7471 (authority): ReleaseToPool (the port pools nothing: K2_DestroyActor).
//! - ReceiveEndPlay @2144: InstigatingGarnet's BeginAlternateCooldownOnAuthority(TriggerAbilityIndex, 0, true) @847;
//!   RemoveStateForPlayer on the attach parent @1054, else on AttachedFighter @2080; (authority) AttachedFighter's
//!   EnableInvulnerability(false, false) @1601; sfx_cha_garnet_special_d_end at the Capsule @1689; DestroyChildren
//!   @1767; InstigatingGarnet's BPI_ResetGauntlets @1926.
//! - ReceiveTick @3839: BeamParticleSystem's source points 1 and 2 at InstigatingGarnet's location: `beam_x` / `beam_y`,
//!   the render's.
//! - Dormant: MulticastAttachMarker (no caller), StartShaking, RecallGauntlets, SetTeamColor, ReceiveAnyDamage and the
//!   hit handlers. HitBoxOn? false and never set: no hit, no clank.
//!
//! Model. A projectile of the world's list whose movement is off from its BeginPlay on: it stands where it was placed,
//! or rides its attach parent's actor location. Its Delays count on the actor's age in the kind's step: one made in the
//! BeginPlay or an overlap event counts the frame it is made in, one made in a Delay's link counts from the next frame.
// Deviation: the Capsule's begin overlaps (a sphere of radius 200: half height = radius, absolute scale 1) are tested
// once a frame in the kind's step against the fighters' root capsules (a fighter waiting for its respawn left out),
// and at the spawn behind the BeginPlay (as the build: begin overlaps right after ReceiveBeginPlay); the build
// dispatches the later ones inside whichever move made them.
// Deviation: OnBFDefeated of InstigatingGarnet and of each fighter the marker attached to is read at the marker's next
// step (the fighter's `dead`), not inside the defeat tail's broadcast (no signal reaches a projectile).
// Unverified: the EndPlay runs in `destroy_projectile`'s `destroyed` hook, ahead of the parent's EndPlay part the port
// runs there (the buff children's end); the build runs the parent's first. (DeathVFX / Ak Death Location are
// DestroySpawnedActor's, ahead of K2_DestroyActor, and null for the marker.)

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, StepEnd, class_p, expiry, set_instance_vars, update_max_lifetime};
use crate::actors::world_actor::actor_p;
use crate::actors::{ActorClass, ActorRef, Projectile, ProjectileClass, ProjectileId};
use crate::chars::garnet::pawn::{reset_gauntlets, set_idle_gauntlet};
use crate::chars::garnet::state::garnet_of;
use crate::events::{Event, UiStateOp, UiStateParams};
use crate::fighter::common::latent_frames;
use crate::fighter::control::{begin_alternate_cooldown, enable_invulnerability};
use crate::fighter::{FighterId, actor_y};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::{flag, num, truthy};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{blast_box_loop, spawn_box};
use crate::world::flow::reset_pass;
use crate::world::spawning::destroy_projectile;
use indexmap::IndexSet;
use mvs_data::value::Value;

pub const MARKER2: Name = name!("GarnetMarker2_C");
const UI_ID: Name = name!("GarnetMarker");
const END_SOUND: Name = name!("sfx_cha_garnet_special_d_end");

/// `GarnetMarker2 extends Projectile` (GarnetMarker2_C): the fields the view reads (`attached_fighter`,
/// `attach_parent`) and the class's own.
#[derive(State, Debug)]
pub struct GarnetMarker2 {
    /// AttachedFighter (BP_FighterCharacter, CDO None): the ally the marker sits on (the Capsule overlap @2995, kept
    /// after the detach), `None` for none.
    pub attached_fighter: Option<FighterId>,
    /// `attachParent`: GetAttachParentActor, the fighter the marker is attached to now (HasAllyMarked, Detach,
    /// AddMarkerUIState: the marker's own star hidden while it has one), `None` for none. Not
    /// Projectile.attached (a spawn's AttachToActor).
    pub attach_parent: Option<FighterId>,
    /// InstigatingGarnet (BeginPlay @6238).
    pub instigating: Option<FighterId>,
    /// The fighters whose OnBFDefeated is bound to OnAttachedFighterRunOut (InstigatingGarnet @6355, each attach @7046;
    /// never unbound: the EndPlay's unbind @2325-2412 goes through GetParentActor, None for a spawned actor).
    pub run_out_on: Vec<FighterId>,
    /// ActivationConditions (BPI_SetActivationConditions).
    pub conds: Vec<bool>,
    /// Activated (MulticastActivatedEffects toggles it, ActivateObject's DoOnce sets it).
    pub is_activated: bool,
    /// ActivateObject's DoOnce @4517 closed.
    pub detach_once: bool,
    /// The Capsule's collision on (QueryAndPhysics in the template, QueryOnly from the 0.2 s Delay) or NoCollision
    /// (MulticastDetach @8334): its overlaps with the fighters.
    pub collide: bool,
    /// StarVFX hidden (AddMarkerUIState @117; shown again by MulticastDetach @8372).
    pub star_hidden: bool,
    /// ActivatedPosition (@8040): written, read by nothing (the Tick's SelectVector takes the other pin).
    pub activated_x: Real,
    pub activated_y: Real,
    /// The beam's source points (ReceiveTick @3955 / @4107): InstigatingGarnet's location at the last tick.
    pub beam_x: Real,
    pub beam_y: Real,
    /// The ages its Delays fire at (-1: none): 0.2 -> @15 (QueryOnly), the overlap's Delay(0) -> @678 (the lifetime),
    /// AddMarkerUIState's Delay(0) -> @54 (its retry).
    pub query_at: Real,
    pub life_at: Real,
    pub ui_retry_at: Real,
    /// The fighters (ids) the Capsule overlaps.
    pub over: IndexSet<i32>,
}

impl Default for GarnetMarker2 {
    fn default() -> Self {
        GarnetMarker2 {
            attached_fighter: None,
            attach_parent: None,
            instigating: None,
            run_out_on: Vec::new(),
            conds: Vec::new(),
            is_activated: false,
            detach_once: false,
            collide: true,
            star_hidden: false,
            activated_x: 0.0,
            activated_y: 0.0,
            beam_x: 0.0,
            beam_y: 0.0,
            query_at: -1.0,
            life_at: -1.0,
            ui_retry_at: -1.0,
            over: IndexSet::new(),
        }
    }
}

/// `attachParent !== null`: the marker is attached to a fighter. Called by pawn.rs's has_ally_marked
/// (the condition GarnetMarkerAttachedToAlly_C).
pub fn has_attach_parent(m: &GarnetMarker2) -> bool {
    m.attach_parent.is_some()
}

fn pr(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pm(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn mk(w: &World, id: ProjectileId) -> &GarnetMarker2 {
    match &pr(w, id).ext {
        ProjectileExt::GarnetMarker2(m) => m,
        e => panic!("GarnetMarker2_C without its state: {e:?}"),
    }
}

fn mk_w(w: &mut World, id: ProjectileId) -> &mut GarnetMarker2 {
    match &mut pm(w, id).ext {
        ProjectileExt::GarnetMarker2(m) => m,
        e => panic!("GarnetMarker2_C without its state: {e:?}"),
    }
}

fn owner(w: &World, id: ProjectileId) -> FighterId {
    pr(w, id).owner.expect("a marker has an owner")
}

/// The Projectile's step override: the movement does not tick; the actor stands, or rides its attach parent's root;
/// only its timers run (MaxLifetime, the blast-box exit while DestroyOnExitedArena? holds). Called by
/// `projectile::step` for a marker, in place of the parent's.
pub fn step_override(w: &mut World, id: ProjectileId) -> Option<StepEnd> {
    if pr(w, id).dead {
        return None;
    }
    let t = mk(w, id).attach_parent;
    let at = t.map(|t| (w.fighter(t).body.x, actor_y(w, t)));
    let p = pm(w, id);
    p.prev_x = p.x;
    p.prev_y = p.y;
    p.bounced = false;
    if let Some((x, y)) = at {
        p.x = x;
        p.y = y;
    }
    p.age += 1;
    expiry(w, id)
}

/// GetAllies of InstigatingGarnet's Comp_Actor_Destructible: the players of its team but itself, one waiting for its
/// respawn left out (the order unverified).
fn is_ally(w: &World, inst: FighterId, o: FighterId) -> bool {
    o != inst
        && w.fighter(o).team.team == w.fighter(inst).team.team
        && w.fighter(o).life.dead <= 0.0
        && !w.fighter(o).life.is_minion
}

/// The begin overlap's flow with `o` the event's OtherActor. `base`: the age a Delay made here counts from.
fn begin_overlap(w: &mut World, id: ProjectileId, o: FighterId, base: Real) {
    // @2995 (authority).
    if let Some(inst) = mk(w, id).instigating
        && is_ally(w, inst, o)
    {
        let m = mk_w(w, id);
        m.attached_fighter = Some(o); // @3281 (PS_Garnet_Victory_Sparkles on its root: the render's)
        if m.life_at < 0.0 {
            m.life_at = base + latent_frames(0.0); // Delay(0) @3745 (a pending Delay ignores the call)
        }
    }
    // @6467: the queued attach, with AttachedFighter as it stands.
    let Some(a) = mk(w, id).attached_fighter else { return };
    if w.fighter(a).team.team != w.fighter(owner(w, id)).team.team {
        return; // @6683: the marker's TeamIndex (its owner's, SetTeamIndexOnSpawn)
    }
    let (ax, ay) = (w.fighter(a).body.x, actor_y(w, a));
    let p = pm(w, id);
    p.vx = 0.0; // StopMovementImmediately @6775
    p.vy = 0.0;
    p.x = ax; // K2_AttachToComponent(its root, SnapToTarget) @6927
    p.y = ay;
    let m = mk_w(w, id);
    m.attach_parent = Some(a);
    if !m.run_out_on.contains(&a) {
        m.run_out_on.push(a); // OnBFDefeated += OnAttachedFighterRunOut @7046
    }
    // @7087: no HomingTargetComponent, so the Capsule keeps its collision; AddMarkerUIState @7454.
    add_marker_ui_state(w, id, base);
}

/// AddMarkerUIState @54.
fn add_marker_ui_state(w: &mut World, id: ProjectileId, base: Real) {
    let Some(p) = mk(w, id).attach_parent else {
        let m = mk_w(w, id);
        if m.ui_retry_at < 0.0 {
            m.ui_retry_at = base + latent_frames(0.0); // Delay(0) @623 -> @54
        }
        return;
    };
    mk_w(w, id).star_hidden = true; // StarVFX.SetVisibility(false) @117
    w.fighter_mut(p).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: UI_ID,
        state_type: 1,
        display_type: 0,
        current_value: -1.0,
        max_value: -1.0,
        timer: -1.0,
        icon_material: name!("MI_State_GarnetStar"),
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    })); // GetUIBrokerForFighter(attach parent).AddStateForPlayer @568
}

/// RemoveStateForPlayer(GarnetMarker) on the attach parent, else on AttachedFighter (MulticastDetach @8432 / @8143,
/// EndPlay @898 / @1964).
fn remove_marker_ui_state(w: &mut World, id: ProjectileId) {
    let m = mk(w, id);
    if let Some(h) = m.attach_parent.or(m.attached_fighter) {
        w.fighter_mut(h).controls.ui_state_ops.push(UiStateOp::Remove(UI_ID));
    }
}

/// The Capsule's begin edges since the last test. `base`: as `begin_overlap`'s.
fn overlaps(w: &mut World, id: ProjectileId, base: Real) {
    if !mk(w, id).collide {
        mk_w(w, id).over.clear();
        return;
    }
    let r = pr(w, id).cap_h; // CapsuleHalfHeight = CapsuleRadius = 200 (a sphere), absolute scale 1
    for v in crate::registry::props::all_fighters(w) {
        // the Pawn channel: a minion too
        let (x, y) = (pr(w, id).x, pr(w, id).y);
        let on = w.fighter(v).life.dead <= 0.0 && crate::actors::tether_shape::sphere_hits(w, x, y, r, v);
        let vid = w.fighter(v).id;
        if !on {
            mk_w(w, id).over.shift_remove(&vid);
            continue;
        }
        if mk(w, id).over.contains(&vid) {
            continue;
        }
        mk_w(w, id).over.insert(vid);
        begin_overlap(w, id, v, base);
    }
}

/// FClamp(v, lo, hi) (FMath::Clamp: lo when v < lo, else the lesser of v and hi).
fn fclamp(v: Real, lo: Real, hi: Real) -> Real {
    if v < lo {
        lo
    } else if v < hi {
        v
    } else {
        hi
    }
}

/// @5651-6129: the location clamped into the stored blast-box bounds less BlastBoxBuffer (UE axes: Y = -sim x).
fn clamp_into_blast_box(w: &mut World, id: ProjectileId) {
    let p = pr(w, id);
    let s = p.blast_bounds;
    let buf = crate::registry::props::obj(class_p(w, p), "BlastBoxBuffer");
    let by = buf.and_then(|b| num(b, "Y")).unwrap_or(0.0);
    let bz = buf.and_then(|b| num(b, "Z")).unwrap_or(0.0);
    // BlastBoxBoundsMin.Y = -s.r, Max.Y = -s.l; Min.Z = s.b, Max.Z = s.t.
    let ue_y = fclamp(-p.x, -s.right + by, -s.left - by);
    let ue_z = fclamp(p.y, s.bottom + bz, s.top - bz);
    let p = pm(w, id);
    p.x = -ue_y;
    p.prev_x = p.x;
    p.y = ue_z;
    p.prev_y = p.y;
}

/// MulticastActivatedEffects @7521.
fn activated_effects(w: &mut World, id: ProjectileId) {
    let m = mk_w(w, id);
    m.is_activated = !m.is_activated; // (and the beam's Color: the render's)
    let Some(inst) = m.instigating else { return };
    let (x, y) = (w.fighter(inst).body.x, actor_y(w, inst));
    let m = mk_w(w, id);
    m.activated_x = x;
    m.activated_y = y;
}

/// MulticastDetach @8432.
fn detach(w: &mut World, id: ProjectileId) {
    remove_marker_ui_state(w, id);
    let m = mk_w(w, id);
    m.attach_parent = None; // K2_DetachFromActor(KeepWorld) @8318: it stays where it is
    m.collide = false; // @8334
    m.over.clear();
    m.star_hidden = false; // @8372
    set_instance_vars(w, id, vec![("DestroyOnExitedArena?".to_owned(), Value::Bool(false))]); // @8406 (the 0.1 s exit check reads it at each fire)
    // CheckForBlastBox @8417 (ForceSpawnPositionInsideBlastBox: a clamp, no destroy).
    let sb = spawn_box(w);
    let force = flag(actor_p(w, ActorRef::Projectile(id)), "ForceSpawnPositionInsideBlastBox");
    let crate::actors::Projectile { x, y, prev_x, prev_y, blast_bounds, .. } = pm(w, id);
    blast_box_loop(&sb, x, y, Some((prev_x, prev_y)), force, blast_bounds, false);
}

fn construct(w: &World, p: &mut Projectile) {
    let conds = class_p(w, p)
        .get("ActivationConditions")
        .and_then(Value::as_arr)
        .map_or_else(Vec::new, |a| a.iter().map(truthy).collect());
    p.ext = ProjectileExt::GarnetMarker2(GarnetMarker2 { conds, ..GarnetMarker2::default() });
}

fn begin(w: &mut World, id: ProjectileId) {
    let inst = owner(w, id); // GetInstigator (the spawner's pawn)
    // @5355: BPI_SetIdleGauntlet(self) on an Instigator that implements BPI_Garnet.
    if garnet_of(w, inst).is_some() {
        set_idle_gauntlet(w, inst, Some(id));
    }
    // @6218: InstigatingGarnet; its OnBFDefeated bound to OnAttachedFighterRunOut (a BP_FighterCharacter).
    let m = mk_w(w, id);
    m.query_at = latent_frames(0.2); // Delay(0.2) @6397 -> @15
    m.instigating = Some(inst);
    m.run_out_on.push(inst);
    // @6170: EnableProjectileMovement = false, ProjectileMovement.Deactivate() (the spawner's velocity is stored on
    // the deactivated component and never moves it).
    let p = pm(w, id);
    p.vx = 0.0;
    p.vy = 0.0;
    p.stopped = true;
    // @5557 (authority, no attach parent): the clamp.
    if mk(w, id).attach_parent.is_none() {
        clamp_into_blast_box(w, id);
    }
    // The overlaps at the spawn place (see the header's Deviation note).
    overlaps(w, id, 0.0);
}

fn step(w: &mut World, id: ProjectileId) -> bool {
    let a = crate::real(pr(w, id).age);
    // OnAttachedFighterRunOut @7471 (authority): ReleaseToPool (see the header's Deviation note).
    if mk(w, id).run_out_on.iter().any(|&f| w.fighter(f).life.dead > 0.0) {
        return true;
    }
    // The overlaps behind the frame's moves (an event ahead of this frame's latent pass).
    overlaps(w, id, a - 1.0);
    // ReceiveTick @3839: the beam's source points.
    if let Some(inst) = mk(w, id).instigating {
        let (x, y) = (w.fighter(inst).body.x, actor_y(w, inst));
        let m = mk_w(w, id);
        m.beam_x = x;
        m.beam_y = y;
    }
    // The latent actions.
    if a == mk(w, id).query_at {
        let m = mk_w(w, id);
        m.query_at = -1.0;
        m.collide = true; // Capsule.SetCollisionEnabled(QueryOnly) @15: overlaps on (again, after a detach)
    }
    if a == mk(w, id).life_at {
        mk_w(w, id).life_at = -1.0;
        let life = num(class_p(w, pr(w, id)), "MaxLifetime").unwrap_or(0.0);
        update_max_lifetime(pm(w, id), life, 0); // UpdateMaxLifetimeWithAuthority(MaxLifetime) @678, behind the pass
    }
    if a == mk(w, id).ui_retry_at {
        mk_w(w, id).ui_retry_at = -1.0;
        add_marker_ui_state(w, id, a);
    }
    false
}

/// ActivateObject @4318.
fn activate(w: &mut World, id: ProjectileId) {
    if mk(w, id).conds.first().copied().unwrap_or(false) {
        activated_effects(w, id); // MulticastActivatedEffects @4397, then ProjectileMovement.Deactivate() @4411
        return;
    }
    if mk(w, id).conds.get(1).copied().unwrap_or(false) {
        if mk(w, id).detach_once {
            return;
        }
        let m = mk_w(w, id);
        m.detach_once = true;
        m.is_activated = true; // @4588
        detach(w, id); // MulticastDetach @4599
        return;
    }
    destroy_projectile(w, id, false); // ReleaseToPool @4614
}

/// ReceiveEndPlay @2144 (see the header's Unverified note). Inside the Lab reset's pass the calls into a pawn are left
/// out (the build's reach the old pawns); the HUD's state and the sound stay.
fn destroyed(w: &mut World, id: ProjectileId) {
    let reset = reset_pass(w);
    let inst = mk(w, id).instigating;
    if let Some(inst) = inst
        && !reset
    {
        let slot = num(class_p(w, pr(w, id)), "TriggerAbilityIndex").unwrap_or(0.0);
        begin_alternate_cooldown(w, inst, crate::math::to_int32(slot), 0, true); // @847
    }
    remove_marker_ui_state(w, id); // @1054 / @2080
    if let Some(a) = mk(w, id).attached_fighter
        && !reset
    {
        enable_invulnerability(w, a, false, false); // @1601
    }
    let p = pr(w, id);
    let ev = Event::Sound {
        fighter: p.owner_id,
        ev: END_SOUND,
        x: Some(p.x),
        y: Some(p.y),
        hit: None,
        pooled: false,
        key: None,
    }; // @1689
    w.events.push(ev);
    // DestroyChildren @1767: none left (the parent's EndPlay part ends them: `destroy_projectile`).
    if let Some(inst) = inst
        && !reset
        && garnet_of(w, inst).is_some()
    {
        reset_gauntlets(w, inst); // BPI_ResetGauntlets @1926
    }
}

/// BPI_SetActivationConditions (SpawnedActorParent @4520 Array_Set, no resize). Called by `world::spawning`'s
/// ActivateActors for a marker.
pub fn set_condition(w: &mut World, id: ProjectileId, n: i32, v: bool) {
    let m = mk_w(w, id);
    if let Ok(i) = usize::try_from(n)
        && i < m.conds.len()
    {
        m.conds[i] = v;
    }
}

/// This file's registry entries (GarnetMarker2_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.actors.enter(
        "GarnetMarker2_C",
        ActorEntry::new(
            ActorClass::Projectile(ProjectileClass::GarnetMarker2),
            ActorHooks::Projectile(ProjectileHooks {
                construct: Some(construct),
                begin: Some(begin),
                step: Some(step),
                activate: Some(activate),
                destroyed: Some(destroyed),
                ..ProjectileHooks::default()
            }),
        ),
    )
}
