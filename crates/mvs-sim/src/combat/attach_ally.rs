//! Buff_AttachToAlly and its children: the owner grappled to the Instigator's
//! root and kept at its held place (`attachFollow`). Its children: Taz's Buff_C015AttachToDogpile
//! (`chars::taz::perks_cloud`) and Velma's Buff_VelmaAttachToAlly (`chars::velma::capture`).
//!
//! Blueprints/Buffs/Navigation/Buff_AttachToAlly, offsets of its ubergraph: a fighter (the owner) carried on its
//! Instigator's root, until it presses anything. A child's graph calls the parent's ahead of its own nodes, so a
//! child's hooks call [`begin`] / [`end`] first, then do their own.
//! - ReceiveBeginPlay @3131 -> @1664 (BindInput pushed @1649): not being destroyed, the owner a BaseFighter -> @39:
//!   the Instigator a BaseFighter (else straight to BindInput), the owner's FMC.OverrideGravity(0) @231, the socket
//!   (AttachSocket, mirrored L_ / R_ for an Instigator facing left when the mirrored one exists on his Mesh,
//!   @382-1051), FMC.InitiateGrapple(the Instigator's ROOT component, that socket) @1070, AdjustHeldLocation(owner,
//!   Instigator) @1300 (the owner's K2_SetActorRelativeLocation((0, 0, -1.5 x its Capsule's scaled half height)),
//!   teleport, @0-161), CarriedMontage = null @1332 and HoldVisualMulticast(null) @15 (@2386:
//!   CreateProxyObjectForPlayMontage with no montage, which plays nothing). Then BindInput @1607: a locally
//!   controlled owner's InputTriggered -> InputTriggered.
//! - InputTriggered @3891: an InputType other than 6 (None) -> DestroyOnServer @1819 -> K2_DestroyActor @1804.
//! - ReceiveTick @1839: with the owner in hitstun, or its active montage the CarriedMontage (null: no montage),
//!   nothing; else HoldVisualMulticast(null) again, which plays nothing. So nothing of the sim.
//! - ReceiveEndPlay @3867 -> @3767 -> @3152: the Instigator a BaseFighter (else nothing more), the owner's
//!   K2_SetActorLocation((0, its own Y, the Instigator's Z), teleport) @3490, ClearGravityOverride @3694, RemoveGrapple
//!   @3726, then HoldVisualStopMulticast(null) (@2924: Montage_Stop(0.0, null) @3083) and Montage_Stop(0.0, null)
//!   @2339: every montage of the owner stopped with no blend.
//! - TakenDamageModCondition @0-450: the owner's team other than the Instigator's and the Attacker's Instigator not
//!   the buff's Instigator.
//! - MovementStarted -> DestroyOnServer has no binder (xref: no caller).
//!
//! The parent class alone (`buffs({ Buff_AttachToAlly_C: attachToAlly() })`) is not entered: nothing the port runs
//! spawns it (SupermanPickUpAlly @1771 is not ported), and its own state would need a buff extension of its own.
//!
//! Unverified: the grapple is to the Instigator's root capsule, a primitive with no sockets: the socket name is taken
//! to give the component's own transform (USceneComponent::GetSocketTransform for a name it does not have; not read
//! in the exe), so SnapToTarget puts the owner at the Instigator's actor location and AdjustHeldLocation's relative
//! offset is under it, in the root's space (times the Instigator's scale). The native grapple is the port's stand-in
//! (`combat::holds::grapple_to_root`): the owner's movement off; it is put there again after the Instigator's moves
//! of every frame by the Instigator's own pass ([`attach_follow`], called from the character hooks of the one
//! Instigator class that spawns a child: Taz's worldTick), not by the attachment.
//! OverrideGravity(0) / ClearGravityOverride are called around it, as for the holds.

use crate::Real;
use crate::State;
use crate::combat::buffs::BuffExt;
use crate::combat::holds::{GrappleRoot, grapple_to_root, remove_root_grapple};
use crate::fighter::anim::grouped::stop_grouped;
use crate::fighter::anim::montages::{stop_dash_montage, stop_montage};
use crate::fighter::body::def;
use crate::fighter::common::peers;
use crate::fighter::moves::recall_move;
use crate::fighter::moves::{clear_gravity_override, override_gravity};
use crate::fighter::{FighterId, actor_y, facing};
use crate::names::Name;
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::world::World;

/// A child's AdjustHeldLocation (`rel`): where it put the owner, from the Instigator's actor location in the root's
/// frame (`dx` along the Instigator's facing).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct AttachRel {
    pub dx: Real,
    pub dy: Real,
}

/// The parent's variables (`AttachState`): the Instigator (InitiateGrapple ran on it) and BindInput's binding.
#[derive(State, Debug, Default)]
pub struct AttachState {
    pub by: Option<FighterId>,
    /// InputTriggered bound.
    pub bound: bool,
    /// A child's AdjustHeldLocation's place. `None`: the parent's place (`held_y`).
    pub rel: Option<AttachRel>,
}

/// A child's BindInput: whether the parent's runs, for the owner and the Instigator.
pub type BindInput = fn(&mut World, FighterId, Option<FighterId>) -> bool;

/// A child's overrides of the parent's functions (its own graph's AdjustHeldLocation and BindInput, which the
/// parent's ReceiveBeginPlay calls): `adjust` puts the owner (the follow keeps that place), `bind_input` says whether
/// the parent's BindInput runs.
#[derive(Clone, Copy, Debug, Default)]
pub struct AttachOverrides {
    pub adjust: Option<fn(&mut World, FighterId, FighterId)>,
    pub bind_input: Option<BindInput>,
}

/// AdjustHeldLocation's offset in the world: -1.5 x the owner's BaseFighter Capsule (half height 96, scaled by the
/// owner's size) along the root's Z, scaled by the Instigator's.
fn held_y(w: &World, f: FighterId, by: FighterId) -> Real {
    let half = crate::math::narrow(def(w, f).inner_cap_half * f64::from(w.fighter(f).body.size_scale));
    let k = w.fighter(by).body.size_scale;
    actor_y(w, by) + ((half * -1.5) * k)
}

fn snap(w: &mut World, f: FighterId, by: FighterId, rel: Option<AttachRel>) {
    let bx = w.fighter(by).body.x;
    let by_y = actor_y(w, by);
    match rel {
        Some(r) => {
            let x = bx + r.dx * facing(w, by);
            recall_move(w, f, x, by_y + r.dy);
        }
        None => {
            let y = held_y(w, f, by);
            recall_move(w, f, bx, y);
        }
    }
}

/// `keepHeldPlace`: a child's AdjustHeldLocation has put the owner where it is: the place kept for the follow.
/// Unverified (as above): the owner stays where the K2_SetActorLocation put it relative to the Instigator's root,
/// turned with the root's yaw when the Instigator turns (the facing).
pub fn keep_held_place(w: &World, f: FighterId, by: FighterId) -> AttachRel {
    let dx = (w.fighter(f).body.x - w.fighter(by).body.x) * facing(w, by);
    AttachRel { dx, dy: (actor_y(w, f) - actor_y(w, by)) }
}

/// The parent's ReceiveBeginPlay: the state the child keeps in its buff's extension.
pub fn begin(w: &mut World, f: FighterId, from: Option<FighterId>, o: AttachOverrides) -> AttachState {
    let mut e = AttachState::default();
    if let Some(from) = from {
        override_gravity(w, f); // @231
        // InitiateGrapple(root) with SnapToTarget, then AdjustHeldLocation.
        let root = GrappleRoot { x: w.fighter(from).body.x, y: actor_y(w, from) };
        grapple_to_root(w, f, root);
        e.by = Some(from);
        match o.adjust {
            Some(adjust) => {
                adjust(w, f, from);
                e.rel = Some(keep_held_place(w, f, from));
            }
            None => snap(w, f, from, None),
        }
    }
    // BindInput: IsLocallyControlled is true for every fighter of the port (a player's pawn on its own page, a bot's
    // on the authority).
    // Deviation: online, a remote player's pawn is not locally controlled on the host, whose buff then binds
    // nothing; the port's peers all run the binding. A child's override decides whether the parent's runs.
    e.bound = match o.bind_input {
        Some(b) => b(w, f, from),
        None => true,
    };
    e
}

/// The parent's `on`: InputTriggered @3891 on the owner's own presses (true: the buff ends).
pub fn on(f: FighterId, e: &AttachState, s: BuffSignal, a: &BuffSignalArg) -> bool {
    s == BuffSignal::FighterInputTriggered && a.presser == Some(f) && e.bound && a.input != Some(6)
}

/// The parent's ReceiveEndPlay.
pub fn end(w: &mut World, f: FighterId, e: &mut AttachState) {
    let Some(by) = e.by else { return }; // @3237: no Instigator, nothing more
    let (x, y) = (w.fighter(f).body.x, actor_y(w, by));
    recall_move(w, f, x, y); // K2_SetActorLocation((0, Y, Instigator Z), teleport) @3490
    clear_gravity_override(w, f); // @3694
    remove_root_grapple(w, f); // RemoveGrapple @3726
    e.by = None;
    // Montage_Stop(0.0, null) @3083 and @2339.
    stop_montage(w, f, Some(0.0), None);
    stop_dash_montage(w, f, 0.0);
    stop_grouped(w, f, None, 0.0);
}

/// The parent's TakenDamageModCondition @0-450: the owner's team other than the Instigator's and the attacker's
/// fighter not the Instigator.
pub fn taken_cond(w: &World, owner: FighterId, e: &AttachState, by: Option<FighterId>) -> bool {
    e.by.is_some_and(|ins| w.fighter(owner).team.team != w.fighter(ins).team.team && by != Some(ins))
}

/// GetAttachParentActor: InitiateGrapple is AActor::AttachToComponent (FMC
/// InitiateGrapple 1410d0dc0) and RemoveGrapple the root's detach, so the owner's attach parent is the Instigator from
/// begin to end (`by`).
pub fn attach_parent(e: &AttachState) -> Option<FighterId> {
    e.by
}

/// The `AttachState` of a buff extension of this family (the children entered: Buff_C015AttachToDogpile,
/// Buff_VelmaAttachToAlly).
fn state_of(x: &BuffExt) -> Option<&AttachState> {
    match x {
        BuffExt::AttachToDogpile(e) => Some(&e.attach),
        BuffExt::VelmaAttachToAlly(e) => Some(&e.attach),
        _ => None,
    }
}

/// `attachFollow`: every fighter carried by `by` through a buff of these classes put at its held place again. Called
/// once a frame by the Instigator's character pass, after the fighters' moves.
pub fn attach_follow(w: &mut World, by: FighterId, classes: &[Name]) {
    let list = peers(w).to_vec();
    for f in list {
        if f == by {
            continue;
        }
        let n = w.fighter(f).buffs.buffs.len();
        for i in 0..n {
            let Some(b) = w.fighter(f).buffs.buffs.get(i) else { break };
            if b.frames <= 0.0 || !classes.contains(&b.cls) {
                continue;
            }
            let Some(e) = state_of(&b.ext) else { continue };
            if e.by == Some(by) {
                let rel = e.rel;
                snap(w, f, by, rel);
            }
        }
    }
}
