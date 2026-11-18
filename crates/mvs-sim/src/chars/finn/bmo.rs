//! Finn's BMO: Bmo_Chop_Buff, BMO_FallowActor ([`BmoActor`], a world actor),
//! AnimNotify_Finn_BMO_Visibility, AnimNotify_Finn_RemoveShopBuff, BmoCondition / NoBmoCondition.
//!
//! Written as Finn's own entry (a copy of Jerry_SpawnedActor's follow part). No pawn hook joined; the pawn keeps
//! nothing of BMO but `FinnState.bmo`.
//!
//! Bmo_Chop_Buff: a BuffParentActor child (its BeginPlay / EndPlay supers are BuffParentActor_C's). Given by
//! Finn_Shop's purchase (shop.rs). ReceiveBeginPlay: the parent's, then on authority the owner's Comp_Spawner
//! CompSpawnActor(BMO_FallowActor_C, not attached, socket None, relative identity, no direction, speed 0) @207, then
//! PF_Post_VO_Event(the owner, ability_bmo, no context, 1.0) @30. ReceiveEndPlay: the parent's alone; its end reaches
//! the BMO through the BMO's own bind (DieWithBuff).
//!
//! BMO_FallowActor (a SpawnedActorParent child; offsets of its ubergraph): ReceiveBeginPlay @4775: the parent's
//! (GrantBuffsFromInstigator, the blast-box loop), UpdateLocality @4682 (drawing), Delay(0.1) @4805 (-> @165), then
//! @4504: FollowingFighter = the Instigator, K2_AttachToActor(Instigator, SnapToTarget, KeepWorld, KeepWorld) @4638,
//! then IsFollowing, Follow_X_Offset = -(HH - 10) @2298, Follow_Z_Offset = -(HH - 80) @2530, Follow_Y_Offset = R - 100
//! @2762 (HH, R: the Instigator's root capsule scaled half height and radius), CrouchHeight = -(scale x
//! CrouchedHalfHeight - 40) @3099, relative location (X, facing left ? -Y : Y, Z) @3697 and
//! RotateServer(IsFacingLeft); the binds OnIsWallSliding -> OnWallSlide, FacingChanged -> UpdateFacing,
//! Comp_FighterVisibilityManager.AdjustedVisibility -> AdjustVisibility, OnExitedArena -> DieWithFollower
//! @3882-4193; then Delay(2.0) @15 (-> @70).
//! Delay 0.1 -> @165: the owner's CheckHasBuff(Bmo_Chop_Buff): found, its OnEndPlay -> DieWithBuff @412; else
//! K2_DestroyActor @454. Delay 2.0 -> @70: PF_Post_VO_Event(the owner, ability_bmo_select, no context, 1.0).
//! ReceiveTick @754: the follower's CMC IsCrouching differs from IsCrouching: IsCrouching = it @1050, CrouchChange
//! @469: relative location (X, FacingLeft ? -Y : Y, crouching ? CrouchHeight : Z).
//! UpdateFacing(FacingLeft) @1620: relative location (X, FacingLeft ? -Y : Y, IsCrouching ? CrouchHeight : Z), the
//! rotation, RotateServer -> RotateMulticast @4251. OnWallSlide(IsWallSliding) @5115: relative location (X + 50,
//! IsWallSliding ? (FacingLeft ? 100 : -100) : (FacingLeft ? -Y : Y), IsWallSliding ? Z + 120 : Z) (X + 50 whether
//! or not it slides: a slide's end leaves it so until the next facing or crouch change).
//! AdjustVisibility(InvisibilityCount) @4875: <= 0 and the mesh hidden -> shown; > 0 and shown -> hidden.
//! DieWithBuff @5539 / DieWithFollower @4860: K2_DestroyActor.
//!
//! The geometry: the relative location is in the Instigator's frame, whose yaw SetRotationLogic sets by its facing:
//! its X is the pawn's forward, so sim x = facing x X; Y is the depth (dropped: not in the play plane); Z is sim y
//! over the Instigator's actor location (`actor_y`). The follow is taken in the actor pass (behind the fighters'
//! moves), as an attached child moves with its parent.
// Unverified: the pawn's local X taken as its facing in sim x.
// Unverified: its two Delays are counted from the spawn frame in the actor's step (`latent_frames`).
// Deviation: AdjustedVisibility's broadcast is taken by reading the Instigator's InvisibilityCount in the step (the
// port's visibility functions broadcast nothing); the rule is the handler's.
// Deviation: DieWithFollower (OnExitedArena) is run from Bmo_Chop_Buff's ExitedArena signal (the buff is alive
// whenever the BMO is past its 0.1 s check); the BMO's own blast-box timer ends it too.
// Not ported: the custom depth stencil (drawing), its Comp_Pawn_PassiveBuffManager (Fireproof / Iceproof on itself:
// nothing in the port hits the BMO), its capsule's Pawn collision. KeepOnCamera? true: `on_camera`.

use super::state::finn_mut;
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::BuffClass;
use crate::combat::buffs::{BuffRef, buff_of, end_buff};
use crate::events::NotifyRef;
use crate::fighter::anim::visibility::invisibility_count;
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::match_side::post_vo;
use crate::fighter::{FighterId, actor_y, cap_half, capsule_half, facing};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{
    ActorEntry, ActorHooks, BuffEntry, BuffHooks, Condition, NotifyHandler, SizeHooks, WorldActorHooks,
};
use crate::registry::props::flag;
use crate::registry::signals::{BuffSignal, BuffSignalArg, OwnerSignal};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit_rounded, new_blast_bounds, spawn_box};
use crate::world::spawning::{add_actor, comp_spawn_actor, new_world_actor};

/// The BMO's class.
pub const BMO: &str = "BMO_FallowActor_C";
/// Bmo_Chop_Buff's class.
pub const BMO_CHOP: &str = "Bmo_Chop_Buff_C";

const BMO_N: Name = name!("BMO_FallowActor_C");
const BMO_CHOP_N: Name = name!("Bmo_Chop_Buff_C");

/// `BmoActor` (BMO_FallowActor_C): what the view reads of it (its place, facing, crouch and visibility) and its follow
/// state.
#[derive(State, Debug)]
pub struct BmoActor {
    /// Comp_Actor_KeepOnCamera (KeepOnCamera? true).
    pub on_camera: bool,
    pub x: Real,
    pub y: Real,
    /// FacingLeft (@4251).
    pub facing_left: bool,
    /// IsCrouching (@1050).
    pub is_crouching: bool,
    /// AnimNotify_Finn_BMO_Visibility's last write (shown by default).
    pub visible: bool,
    /// BMOMesh's visibility as AdjustVisibility (@4875) left it.
    pub mesh_visible: bool,
    /// OnWallSlide's last IsWallSliding (the depth offset it chose).
    pub wall_sliding: bool,
    /// The relative location's X (the Instigator's forward) and Z (up), as the last write left them.
    pub rel_x: Real,
    pub rel_z: Real,
    /// Follow_X_Offset.
    pub follow_x: Real,
    /// Follow_Z_Offset.
    pub follow_z: Real,
    /// CrouchHeight.
    pub crouch_height: Real,
    /// Delay(0.1) @4805: frames left, 0 done.
    pub check_left: Real,
    /// Delay(2.0) @15: frames left, 0 done.
    pub vo_left: Real,
    /// The Bmo_Chop_Buff whose OnEndPlay is bound to DieWithBuff.
    pub bound_to: Option<BuffRef>,
    /// Ticks, the spawn frame counted (the parent's CheckBlastBoxTimer).
    pub age: i32,
    pub blast_at: Real,
    pub blast_bounds: BlastBounds,
}

impl Default for BmoActor {
    /// The field initial values.
    fn default() -> Self {
        BmoActor {
            on_camera: true,
            x: 0.0,
            y: 0.0,
            facing_left: false,
            is_crouching: false,
            visible: true,
            mesh_visible: true,
            wall_sliding: false,
            rel_x: 0.0,
            rel_z: 0.0,
            follow_x: 0.0,
            follow_z: 0.0,
            crouch_height: 0.0,
            check_left: 0.0,
            vo_left: 0.0,
            bound_to: None,
            age: 0,
            blast_at: 0.1,
            blast_bounds: new_blast_bounds(),
        }
    }
}

/// The BMO_FallowActor_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &BmoActor) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

fn bmo_of(w: &World, id: ActorId) -> Option<&BmoActor> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::BmoActor(s) => Some(s),
        _ => None,
    }
}

fn bmo_mut(w: &mut World, id: ActorId) -> Option<&mut BmoActor> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::BmoActor(s) => Some(s),
        _ => None,
    }
}

/// The relative location to the place: the Instigator's actor location plus (facing x X, Z).
fn follow(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let (ox, oy, fc) = (w.fighter(f).body.x, actor_y(w, f), facing(w, f));
    if let Some(a) = bmo_mut(w, id) {
        a.x = ox + fc * a.rel_x;
        a.y = oy + a.rel_z;
    }
}

/// Bmo_Chop_Buff's CompSpawnActor @207 and BMO_FallowActor's BeginPlay.
fn spawn_bmo(w: &mut World, f: FighterId) {
    let data = w.data.clone();
    let Some(d) = actor_data(w, &data, ActorRef::Fighter(f), BMO_N) else {
        crate::fighter::common::warn_text(w, &format!("spawned actor not exported ({BMO})"));
        return;
    };
    // Comp_Spawner's socket None, relative identity: the owner's location less 100 in Z (as the port's notify spawns).
    let hh = capsule_half(w, f);
    let (cap_u, cap) = (w.fighter(f).capsule.cap_half_u, cap_half(w, f));
    let scale = if cap_u > 0.0 { cap / cap_u } else { 1.0 };
    let mut s = BmoActor {
        x: w.fighter(f).body.x,
        y: (actor_y(w, f) - 100.0),
        follow_x: (-(hh - 10.0)),                                               // @2298
        follow_z: (-(hh - 80.0)),                                               // @2530
        crouch_height: (-((scale * w.fighter(f).scaled.crouched_half) - 40.0)), // @3099
        check_left: latent_frames(0.1),
        vo_left: latent_frames(2.0),
        ..BmoActor::default()
    };
    let sb = spawn_box(w);
    let dead = blast_box_loop(
        &sb,
        &mut s.x,
        &mut s.y,
        None,
        flag(&d.p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        flag(&d.p, "DestroyOnExitedArena?"),
    );
    let charge = crate::combat::elem::instigator_charge(w, f);
    let actor = new_world_actor(w, BMO_N, WorldActorClass::BMOFallowActor, f, dead, WorldActorState::BmoActor(s));
    let id = w.actors.actors.insert(actor);
    if dead {
        add_actor(w, id, charge, true);
        return;
    }
    if let Some(st) = finn_mut(w, f) {
        st.bmo = Some(ActorRef::Actor(id));
    }
    let left = facing(w, f) < 0.0;
    if let Some(a) = bmo_mut(w, id) {
        a.rel_x = a.follow_x;
        a.rel_z = a.follow_z;
        a.facing_left = left; // RotateServer(IsFacingLeft)
    }
    follow(w, id);
    comp_spawn_actor(w, id, charge, f); // AfterSpawnedActor: Buff_Enlarge's binding, then the signal
}

fn bmo_step(w: &mut World, id: ActorId) {
    let Some(o) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let crouched = w.fighter(o).capsule.cmc_crouched;
    let n = invisibility_count(w, o);
    let Some(a) = bmo_mut(w, id) else { return };
    a.age += 1;
    // ReceiveTick @754: the crouch.
    if a.is_crouching != crouched {
        a.is_crouching = crouched;
        a.rel_x = a.follow_x;
        a.rel_z = if crouched { a.crouch_height } else { a.follow_z };
    }
    // AdjustVisibility (see above).
    if n <= 0.0 && !a.mesh_visible {
        a.mesh_visible = true;
    } else if n > 0.0 && a.mesh_visible {
        a.mesh_visible = false;
    }
    follow(w, id);
    // The latent actions: Delay 0.1 -> @165, Delay 2.0 -> @70.
    let Some(a) = bmo_mut(w, id) else { return };
    if a.check_left > 0.0 {
        a.check_left -= 1.0;
        if a.check_left == 0.0 {
            match buff_of(w, o, BMO_CHOP_N) {
                None => {
                    if let Some(x) = w.actors.actors.get_mut(id) {
                        x.dead = true; // K2_DestroyActor @454
                    }
                    return;
                }
                Some(b) => {
                    if let Some(a) = bmo_mut(w, id) {
                        a.bound_to = Some(b);
                    }
                }
            }
        }
    }
    let Some(a) = bmo_mut(w, id) else { return };
    let mut vo = false;
    if a.vo_left > 0.0 {
        a.vo_left -= 1.0;
        vo = a.vo_left == 0.0;
    }
    if vo {
        post_vo(w, o, name!("ability_bmo_select"), None, 1.0);
    }
    // The parent's CheckBlastBoxTimer (0.1 s, looping; DestroyOnExitedArena? true).
    let Some(a) = bmo_mut(w, id) else { return };
    let (x, y) = (a.x, a.y);
    let age = a.age;
    let out = blast_timer_exit_rounded(doe, &mut a.blast_at, &a.blast_bounds, crate::real(age - 1), x, y);
    if out && let Some(x) = w.actors.actors.get_mut(id) {
        x.dead = true;
    }
}

fn bmo_owner_signal(w: &mut World, id: ActorId, _f: FighterId, sig: OwnerSignal, v: bool) {
    match sig {
        OwnerSignal::Facing => {
            // UpdateFacing @1620.
            if let Some(a) = bmo_mut(w, id) {
                a.rel_x = a.follow_x;
                a.rel_z = if a.is_crouching { a.crouch_height } else { a.follow_z };
                a.facing_left = v;
            }
            follow(w, id);
        }
        OwnerSignal::WallSliding => {
            // OnWallSlide @5115.
            if let Some(a) = bmo_mut(w, id) {
                a.wall_sliding = v;
                a.rel_x = a.follow_x + 50.0;
                a.rel_z = if v { a.follow_z + 120.0 } else { a.follow_z };
            }
            follow(w, id);
        }
        OwnerSignal::Jumping => {}
    }
}

fn bmo_end(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if let Some(s) = finn_mut(w, f)
        && s.bmo == Some(ActorRef::Actor(id))
    {
        s.bmo = None;
    }
}

/// The owner's live BMOs, in their spawn order.
fn live_bmos(w: &World, f: FighterId) -> Vec<ActorId> {
    w.fighter(f)
        .items
        .spawned_actors
        .iter()
        .filter_map(|a| match a {
            ActorRef::Actor(id) => Some(*id),
            _ => None,
        })
        .filter(|&id| w.actors.actors.get(id).is_some_and(|x| !x.dead && x.cls == BMO_N))
        .collect()
}

/// DieWithBuff (only: the ending buff) / DieWithFollower (None): the owner's live BMOs bound to the call end.
fn kill_bmos(w: &mut World, f: FighterId, only: Option<BuffRef>) {
    for id in live_bmos(w, f) {
        if (only.is_none() || bmo_of(w, id).is_some_and(|a| a.bound_to == only))
            && let Some(x) = w.actors.actors.get_mut(id)
        {
            x.dead = true;
        }
    }
}

fn chop_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    spawn_bmo(w, f);
    post_vo(w, f, name!("ability_bmo"), None, 1.0); // @30
}

fn chop_on(w: &mut World, f: FighterId, _b: BuffRef, s: BuffSignal, _a: &BuffSignalArg) -> bool {
    if s == BuffSignal::ExitedArena {
        kill_bmos(w, f, None); // the BMO's DieWithFollower bind (see above)
    }
    false
}

fn chop_end(w: &mut World, f: FighterId, b: BuffRef) {
    kill_bmos(w, f, Some(b)); // its OnEndPlay -> DieWithBuff
}

/// The owner's first SpawnedActors entry of the class: hidden (Begin) or shown (End); the loop returns there.
fn set_bmo_hidden(w: &mut World, f: FighterId, hidden: bool) {
    if let Some(&id) = live_bmos(w, f).first()
        && let Some(a) = bmo_mut(w, id)
    {
        a.visible = !hidden; // SetActorHiddenInGame
    }
}

fn visibility_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    set_bmo_hidden(w, f, true);
}

fn visibility_end(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    set_bmo_hidden(w, f, false);
}

/// Received_NotifyBegin: the owner's Bmo_Chop_Buff, when there is one, K2_DestroyActor.
fn remove_shop_buff_begin(w: &mut World, f: FighterId, _n: NotifyRef, _idx: i32) {
    if let Some(b) = buff_of(w, f, BMO_CHOP_N) {
        end_buff(w, f, b);
    }
}

fn bmo_condition(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, BMO_CHOP_N).is_some()
}

fn no_bmo_condition(w: &mut World, f: FighterId) -> bool {
    buff_of(w, f, BMO_CHOP_N).is_none()
}

/// This file's registry entries (Bmo_Chop_Buff_C, BMO_FallowActor_C, the notifies and conditions). `size`: no shape
/// of its own; the view draws BMOMesh at the root's scale. The follow offsets are the owner's (world).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        size: Some(SizeHooks::default()),
        step: Some(bmo_step),
        owner_signal: Some(bmo_owner_signal),
        end: Some(bmo_end),
        ..WorldActorHooks::default()
    };
    r.actors
        .enter(BMO, ActorEntry::new(ActorClass::World(WorldActorClass::BMOFallowActor), ActorHooks::World(hooks)))?;
    r.buffs.enter(
        BMO_CHOP,
        BuffEntry::new(
            BuffClass::BmoChopBuff,
            BuffHooks { begin: Some(chop_begin), on: Some(chop_on), end: Some(chop_end), ..BuffHooks::default() },
        ),
    )?;
    r.notifies.enter(
        "AnimNotify_Finn_BMO_Visibility_C",
        NotifyHandler { begin: Some(visibility_begin), end: Some(visibility_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_Finn_RemoveShopBuff_C",
        NotifyHandler { begin: Some(remove_shop_buff_begin), ..NotifyHandler::default() },
    )?;
    r.conditions.enter("BmoCondition_C", Condition::Fn(bmo_condition))?;
    r.conditions.enter("NoBmoCondition_C", Condition::Fn(no_bmo_condition))
}
