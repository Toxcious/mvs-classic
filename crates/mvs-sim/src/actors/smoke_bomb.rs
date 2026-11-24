//! Batman's smoke bomb (down special) and the occlusion family it brings: SmokeGrenade,
//! SmokeGrenadeOcclusionBP (SmokeOcclusionParent's child) and the buffs they spawn, with the occlusion buff the build
//! shares (Buff_Invisibility and its child Buff_Invisibility_Fog).
//!
//! What the pawn adds (Pawn_BatmanV2's OccludedEvent / NotOccludedEvent, BPI_IsOccluded / BPI_GetSpeed) is entered
//! with the character (`CharHooks`); this module calls it through the table and does nothing for a pawn without it.
//! The drawing (Comp_FighterVisibilityManager AddInvisibility, ShowOrHideUI, DetermineLocalVisibility) is the view's:
//! it reads `hidden` of the live buff ([`occlusion_hidden`]). The sim never calls the visibility manager.

use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_elem};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActorClass};
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{BuffExt, BuffRef, buff, buff_mut, buff_of, end_buff, inst_view, remove_stack, spawn_buff};
use crate::combat::elem::{
    ElemExt, actor_remove_stack, elem_buff_mut, end_actor_buff, instigator_charge, spawn_actor_buff,
};
use crate::combat::{BuffClass, HitPacket};
use crate::consts::{DELTA_SECONDS, SOLID_NAV};
use crate::events::{Event, SoundKey};
use crate::fighter::common::{actor_data, latent_frames, warn_once, warn_text};
use crate::fighter::match_side::SpawnRequest;
use crate::fighter::traces::solids::{PolyBlock, sphere_static, static_solids_and_polys};
use crate::fighter::traces::{OBJ_PLATFORM, body_info, response_to};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half};
use crate::math::ue::size2d;
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::body_settings::{body_mass, box_mass, hull_mass, sphere_settings, sphyl_mass, welded_mass};
use crate::registry::entries::{
    ActorBuffHooks, ActorEntry, ActorHooks, BuffEntry, BuffHooks, HazardHooks, SizeHooks, WorldActorHooks,
};
use crate::registry::props::{flag, num, obj, text};
use crate::registry::signals::{ActorBuffArg, ActorBuffSignal, BuffSignal, BuffSignalArg};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::pawns::{all_list, put_fighters};
use mvs_collision::Ground;
use mvs_collision::rigid_sphere::{Body, Settings, Solver, Static, WeldedBox, WeldedMass};
use mvs_data::fighter::ActorDef;
use mvs_data::value::{Props, Value};

pub const GRENADE: &str = "SmokeGrenade_C";
pub const ZONE: &str = "SmokeGrenadeOcclusionBP_C";
const GRENADE_N: Name = name!("SmokeGrenade_C");
const ZONE_N: Name = name!("SmokeGrenadeOcclusionBP_C");
const FOG: Name = name!("Buff_Invisibility_Fog_C");
/// Batman's Chimney perk (`chars::batman::perks`).
const CHIMNEY: Name = name!("Buff_Batman_Chimney_C");
const INVISIBILITY: Name = name!("Buff_Invisibility_C");
const SPEED_UP: Name = name!("Buff_SpeedUp_C");
const INVULNERABLE: Name = name!("Buff_Invulnerable_C");
const EVADE: Name = name!("Buff_DecrementEvadeCount_C");
/// Buff_SmokeGrenadeProjectileBlock: the buff the smoke gives a fighter inside it.
pub const PROJECTILE_BLOCK: &str = "Buff_SmokeGrenadeProjectileBlock_C";
const PROJECTILE_BLOCK_N: Name = name!("Buff_SmokeGrenadeProjectileBlock_C");
const SLOW_DOWN: Name = name!("Buff_SlowDown_C");
const WEAKENED: Name = name!("Buff_Weakened_C");
/// The zone's loop key (`'smoke' + id`).
const SMOKE_KEY: Name = name!("smoke");

/// BPI_GetFighterCharacter == 2 (EFighter: Batman).
pub fn is_batman(w: &World, f: FighterId) -> bool {
    w.names.str(w.fighter(f).ch) == "batman"
}

// ---- the occlusion buff: Buff_Invisibility and its child Buff_Invisibility_Fog (no graph of its own) ----
// ReceiveBeginPlay @319 (authority): binds the owner's Comp_Pawn_HitstunHandler HitStunStart -> UnHidePlayer @637
// and HitStunEnded -> AttemptToHidePlayer (@837 -> @7323), Comp_Pawn_BranchManager IsAttackingStart -> UnHidePlayer
// @1216 and IsAttackingEnd -> AttemptToHidePlayer @1416, Comp_Pawn_Giant GiantCrouchChanged -> OnGiantCrouch @1795;
// ControlCharacterOutline = !IsLocalOnlinePlayer @3491; AttemptToHidePlayer @3510.
// AttemptToHidePlayer @7338: blocked = (CheckHitstunActive && a valid handler) || (BranchManager IsAttacking && a
// valid manager) || (BPI_C017 && !GiantCrouched) @8031-8337. Blocked: the outline back on (@8466-8572), else
// HidePlayer @8619.
// HidePlayer @3525: once (!CurrentlyInvisible @3624): CurrentlyInvisible = true @3663, the owner's OccludedEvent
// @3759 (BaseFighter @147675: Occluded = true, then the pawn's override), then, off a dedicated server,
// DetermineLocalVisibility @3916 and, for a viewer that does not see him, AddInvisibility @4345,
// ShowOrHideUI(false) @4466 and the outline off @4551-4657.
// UnHidePlayer @5101: once (CurrentlyInvisible @5200): CurrentlyInvisible = false @5210; for that viewer
// RemoveInvisibility @5635, ShowOrHideUI(true) @5756 and the outline on @5870-5976. No NotOccludedEvent: the
// fighter stays Occluded until the buff ends.
// ReceiveEndPlay @6512: UnHidePlayer @6531, the owner's NotOccludedEvent @6729 (BaseFighter @147687: Occluded =
// false, then the pawn's override), the outline off @6973.
// Deviation: the delegates but GiantCrouchChanged are polled: the buff's tick tests the blocked state each tick and
// hides or shows on its change (one tick late against a delegate that fires inside another graph's tick, and a
// hitstun that starts and ends inside one tick shows nothing). The followers' branch (@2357, @4704, @6023, @8650) is
// the actor entry below.
// GiantCrouched (Comp_Pawn_Giant): set by the notify state GiantIsCrouched, which the AnimSequence C017_Crouch carries
// over its whole length: a standing Giant is blocked (@8143), a crouching one is hidden as anyone else. OnGiantCrouch
// @9597 (the GiantCrouchChanged broadcast): crouched -> AttemptToHidePlayer @9611, else UnHidePlayer @9626; the
// tick's poll finds the same state after it.
// Not ported: the outline calls (ControlCharacterOutline / OutlineOn, BPI_SetRenderCustomDepthOnMeshes): each hide
// takes the outline off and each show puts it back, both inside the same call as the invisibility, so for a viewer
// that sees the fighter the outline is never touched and for one that does not, the hidden mesh draws none.

/// Fighter.isAttacking with no list made (read each tick per fighter in the smoke): the same test over
/// `any_active` in place of the attacking flags' list.
fn attacking(w: &mut World, f: FighterId) -> bool {
    let fi = w.fighter(f);
    if fi.montages.montage.is_none() && fi.montages.grouped.is_none() {
        return false;
    }
    if fi.montages.montage.is_some() && fi.charge.begin_attack {
        return true;
    }
    let branch = |n: &mvs_data::fighter::Notify| n.p.get("NotifyBranchManager") != Some(&Value::Bool(false));
    crate::fighter::anim::montages::any_active(w, f, name!("AnimNotify_isAttackingFlag_C"), &branch)
        || crate::fighter::control::cond_attacking(w, f)
}

fn occ_blocked(w: &mut World, f: FighterId) -> bool {
    use crate::chars::iron_giant::{giant_crouched, giant_of};
    (giant_of(w, f).is_some() && !giant_crouched(w, f)) || crate::fighter::in_hitstun(w, f) || attacking(w, f)
}

/// The occlusion buff's (hidden, occ), to write.
fn occ_mut(w: &mut World, b: BuffRef) -> Option<(&mut bool, &mut bool)> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::Occlusion { hidden, occ } => Some((hidden, occ)),
        _ => None,
    }
}

fn set_hidden(w: &mut World, b: BuffRef, v: bool) {
    if let Some((hidden, _)) = occ_mut(w, b) {
        *hidden = v;
    }
}

fn hide_player(w: &mut World, f: FighterId, b: BuffRef) {
    let Some((hidden, occ)) = occ_mut(w, b) else { return };
    if *hidden {
        return;
    }
    *hidden = true;
    *occ = true;
    let reg: &'static Registry = w.registry;
    if let Some(h) = reg.chars.get(w.names.str(w.fighter(f).ch)).and_then(|c| c.occluded_event) {
        h(w, f);
    }
}

/// AttemptToHidePlayer @7338: HidePlayer unless blocked (the outline calls of the blocked branch are not ported).
fn attempt_to_hide(w: &mut World, f: FighterId, b: BuffRef) {
    if !occ_blocked(w, f) {
        hide_player(w, f, b);
    }
}

fn occlusion_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Occlusion { hidden: false, occ: false };
    }
    attempt_to_hide(w, f, b);
}

fn occlusion_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if occ_blocked(w, f) {
        set_hidden(w, b, false);
    } else {
        hide_player(w, f, b);
    }
    false
}

/// OnGiantCrouch @9597 (bound at BeginPlay @1795 on a pawn with Comp_Pawn_Giant: only his broadcast sends it).
fn occlusion_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::GiantCrouchChanged {
        return false;
    }
    if a.crouched == Some(true) {
        attempt_to_hide(w, f, b);
    } else {
        set_hidden(w, b, false); // UnHidePlayer @5101 (CurrentlyInvisible = false; Occluded stays)
    }
    false
}

fn occlusion_end(w: &mut World, f: FighterId, b: BuffRef) {
    if let Some((hidden, occ)) = occ_mut(w, b) {
        *hidden = false;
        *occ = false;
    }
    let reg: &'static Registry = w.registry;
    if let Some(h) = reg.chars.get(w.names.str(w.fighter(f).ch)).and_then(|c| c.not_occluded_event) {
        h(w, f);
    }
}

// The same buff on a follower (`actors::follower`: a spawned actor with BPI_Follower; its owner is no BaseFighter).
// ReceiveBeginPlay @319 (authority) -> @2357: an owner without BPI_Follower stops there (@2415: the buff does
// nothing); with a valid Comp_Follower its FollowerHitReactStart -> UnHidePlayer @2610, FollowerHitReactEnd ->
// AttemptToHidePlayer @2745, FollowerIsAttackingStart -> UnHidePlayer @2880 and FollowerIsAttackingEnd ->
// AttemptToHidePlayer @3015 are bound; then (off a dedicated server) ControlCharacterOutline from the Instigator's
// skin manager @3143-3491 and AttemptToHidePlayer @3510.
// AttemptToHidePlayer @8650: BPI_GetIsFalling || BPI_GetIsFlying || BPI_GetIsAttacking @8962-9336 -> the outline on
// (@9388-9476, not ported), else HidePlayer @9518. No poll: the follower's own broadcasts drive it.
// HidePlayer @4704: once (!CurrentlyInvisible @4772): CurrentlyInvisible = true @4811; off a dedicated server, the
// owner's Instigator cast to BaseFighter @4920 (a follower's is its pawn), IsVisibleLocally =
// DetermineLocalVisibility(Instigator) @4995, Delay(0.2) @5046 -> @15: BPI_GetFollowerMesh.SetVisibility(
// IsVisibleLocally) @160 (FollowerMeshVis Seers: the view decides IsVisibleLocally), the outline off @202-261.
// UnHidePlayer @6023: once (CurrentlyInvisible @6091): CurrentlyInvisible = false @6101; for a viewer that did not
// see it (!IsVisibleLocally @6148) SetVisibility(true) @6332 (All: the same mesh for a viewer that did), the
// outline on @6366-6454. No OccludedEvent on a follower.
// The Delay is one action per node: a HidePlayer while it runs starts none, and an UnHidePlayer does not stop it, so
// a hide, a show and the Delay's end inside 0.2 s leave the mesh at Seers with CurrentlyInvisible false (the
// build's order). The buff's end (EndPlay @6512: UnHidePlayer @6531, the outline off @7080-7273) takes the Delay
// with it.
// Unverified: the Delay counts in the buff's tick behind its owner's (`actor_buff_tick`), from the tick after the
// call.
// Unverified: two guards are taken as passing: HidePlayer @4920-4985 leaves before IsVisibleLocally and the Delay
// when the follower's Instigator cast to BaseFighter fails (CurrentlyInvisible already set: the mesh is never turned
// to Seers), and BeginPlay @3285 leaves before AttemptToHidePlayer @3510 without a valid Comp_Pawn_SkinManager on
// the Instigator (the first hide then waits for a follower broadcast). A follower's Instigator is taken as its pawn,
// with its skin manager.
// Not ported: the outline (ControlCharacterOutline / OutlineOn, SetRenderCustomDepth), as on a fighter.

fn follower_ext(w: &mut World, a: ActorRef, i: u32) -> Option<(&mut bool, &mut bool, &mut Real)> {
    match &mut elem_buff_mut(w, a, i)?.ext {
        ElemExt::Follower { hidden, bound, delay } => Some((hidden, bound, delay)),
        _ => None,
    }
}

fn follower_begin(w: &mut World, a: ActorRef, i: u32, _from: Option<FighterId>) -> bool {
    let Some(k) = crate::actors::follower::follower_of(w, a) else { return false };
    if let Some(b) = elem_buff_mut(w, a, i) {
        b.ext = ElemExt::Follower { hidden: false, bound: k.comp, delay: 0.0 };
    }
    follower_attempt_to_hide(w, a, i);
    false
}

fn follower_attempt_to_hide(w: &mut World, a: ActorRef, i: u32) {
    let Some(k) = crate::actors::follower::follower_of(w, a) else { return };
    if follower_ext(w, a, i).is_none() {
        return;
    }
    if (k.is_falling)(w, a) || (k.is_flying)(w, a) || (k.is_attacking)(w, a) {
        return;
    }
    let Some((hidden, _, delay)) = follower_ext(w, a, i) else { return };
    if *hidden {
        return;
    }
    *hidden = true;
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the test is `!(delay > 0)`")]
    if !(*delay > 0.0) {
        *delay = latent_frames(0.2);
    }
}

fn follower_unhide(w: &mut World, a: ActorRef, i: u32) {
    let Some((hidden, _, _)) = follower_ext(w, a, i) else { return };
    if !*hidden {
        return;
    }
    *hidden = false;
    if let Some(k) = crate::actors::follower::follower_of(w, a) {
        (k.mesh)(w, a, crate::actors::follower::FollowerMeshVis::All);
    }
}

fn follower_tick(w: &mut World, a: ActorRef, i: u32) -> bool {
    let Some((_, _, delay)) = follower_ext(w, a, i) else { return false };
    if *delay <= 0.0 || delay.is_nan() {
        return false;
    }
    *delay -= 1.0;
    if *delay == 0.0
        && let Some(k) = crate::actors::follower::follower_of(w, a)
    {
        (k.mesh)(w, a, crate::actors::follower::FollowerMeshVis::Seers); // @15 -> @160
    }
    false
}

fn follower_on(w: &mut World, a: ActorRef, i: u32, s: ActorBuffSignal, _o: Option<&ActorBuffArg>) -> bool {
    if !follower_ext(w, a, i).is_some_and(|(_, bound, _)| *bound) {
        return false;
    }
    match s {
        ActorBuffSignal::FollowerHitReactStart | ActorBuffSignal::FollowerIsAttackingStart => {
            follower_unhide(w, a, i);
        }
        ActorBuffSignal::FollowerHitReactEnd | ActorBuffSignal::FollowerIsAttackingEnd => {
            follower_attempt_to_hide(w, a, i);
        }
        _ => {}
    }
    false
}

fn follower_end(w: &mut World, a: ActorRef, i: u32) {
    follower_unhide(w, a, i);
}

/// `followerOcclusionHidden`: CurrentlyInvisible of a follower's live occlusion buff (what the view reads beside the
/// mesh's visibility).
pub fn follower_occlusion_hidden(w: &World, a: ActorRef) -> bool {
    let Some(Some(st)) = actor_elem(w, a) else { return false };
    for b in &st.buffs {
        if matches!(b.class, BuffClass::BuffInvisibilityFog | BuffClass::BuffInvisibility)
            && let ElemExt::Follower { hidden, .. } = b.ext
        {
            return hidden;
        }
    }
    false
}

/// `occlusionOf`: the extension of the fighter's first live occlusion buff.
fn occlusion_of(w: &World, f: FighterId) -> Option<(bool, bool)> {
    let b = w.fighter(f).buffs.buffs.iter().find(|b| (b.cls == FOG || b.cls == INVISIBILITY) && b.frames > 0.0)?;
    match b.ext {
        BuffExt::Occlusion { hidden, occ } => Some((hidden, occ)),
        _ => None,
    }
}

/// `occluded`: BaseFighter.Occluded (read by Pawn_BatmanV2 BPI_IsOccluded, Item_Batarang and BatarangProjectile).
pub fn occluded(w: &World, f: FighterId) -> bool {
    occlusion_of(w, f).is_some_and(|(_, occ)| occ)
}

/// `occlusionHidden`: CurrentlyInvisible of the fighter's live occlusion buff: what HidePlayer / UnHidePlayer last
/// did (the view's rule decides who sees it).
pub fn occlusion_hidden(w: &World, f: FighterId) -> bool {
    occlusion_of(w, f).is_some_and(|(hidden, _)| hidden)
}

// ---- the buffs the zone gives

// Buff_Invulnerable: ReceiveBeginPlay @29 (authority) EnableInvulnerability(true, false) @172; ReceiveDestroyed
// @938 -> EnableInvulnerability(false, false) @895. (SpawnInvulnerabilityVisuals has no caller in the class.)
fn invulnerable_begin(w: &mut World, f: FighterId, _b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    crate::fighter::control::enable_invulnerability(w, f, true, false);
}

fn invulnerable_end(w: &mut World, f: FighterId, _b: BuffRef) {
    crate::fighter::control::enable_invulnerability(w, f, false, false);
}

// Buff_SpeedUp (SpeedMod 1.25, Multiply: the parent's speed mod). Its own graph is DelayDestroy: Delay(1.0) @46 ->
// K2_DestroyActor @15 (a second call while the Delay runs starts nothing).
// Unverified: a buff's Delay counts from the buff's first tick, as Buff_PieHeal's.
fn speed_up_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::SpeedUp { left: None };
    }
}

fn speed_up_tick(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::SpeedUp { left: Some(left) } = &mut x.ext else { return false };
    *left -= 1.0;
    *left <= 0.0
}

/// DelayDestroy @46 on a live Buff_SpeedUp.
fn delay_destroy(w: &mut World, b: BuffRef) {
    let Some(x) = buff_mut(w, b) else { return };
    if !matches!(x.ext, BuffExt::SpeedUp { left: Some(_) }) {
        x.ext = BuffExt::SpeedUp { left: Some(latent_frames(1.0)) };
    }
}

// Buff_DecrementEvadeCount (DecrementEvadeOnInterval 1, NumberOfDecrements 8, MaxLifetime 15). ReceiveBeginPlay
// @584 -> @271: authority, a BPI_Base_Fighter owner that is no NotPlayerCharacter (else K2_DestroyActor @569);
// Delay(MaxLifetime) @494 -> @256 K2_DestroyActor; then @184: CurrentDecrements < NumberOfDecrements ->
// MulticastDecrementEvadeCount @15 (-> @694 the owner's BPI_DecrementEvadeCount(8.0): ReplenishDodgeMeter),
// CurrentDecrements + 1, Delay(DecrementEvadeOnInterval) @125 -> @184 again; else K2_DestroyActor @241.
// The life timer of the spawn (6 s from the zone) ends it first: the refills are at 0, 1, ... s of its life.
// A NotPlayerCharacter owner (a minion) is destroyed in the BeginPlay, ahead of any refill (@475 -> @569).
fn evade_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    if w.fighter(f).life.not_player_character {
        end_buff(w, f, b);
        return;
    }
    let life = latent_frames(inst_view(w, b).num("MaxLifetime").unwrap_or(0.0));
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::Evade { n: 0.0, next: 0.0, life };
    }
    evade_step(w, f, b);
}

fn evade_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::Evade { next, life, .. } = &mut x.ext else { return false };
    *life -= 1.0;
    if *life <= 0.0 {
        return true;
    }
    *next -= 1.0;
    if *next > 0.0 {
        return false;
    }
    evade_step(w, f, b)
}

fn evade_step(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let d = inst_view(w, b);
    let (most, every) = (d.num("NumberOfDecrements").unwrap_or(0.0), d.num("DecrementEvadeOnInterval").unwrap_or(0.0));
    let Some(BuffExt::Evade { n, .. }) = buff(w, b).map(|x| x.ext.clone()) else { return false };
    if n >= most {
        return true;
    }
    crate::fighter::control::replenish_evade(w, f, 8.0);
    if let Some(x) = buff_mut(w, b)
        && let BuffExt::Evade { n, next, .. } = &mut x.ext
    {
        *n += 1.0;
        *next = latent_frames(every);
    }
    false
}

// Buff_PassiveApplyBuffonHit_C (WeakenedStacksPerHit 2): Batman's passive (his Comp_Pawn_PassiveBuffManager row,
// self, spawned by the pawn's code). DealtDamageModCondition @0-207: Active = Damage > 0 && BaseKnockBack > 0 @188,
// the parent's answer (true) returned. On Fighter Owner Hit Enemy Player @777 -> @10: Active, an owner with
// BPI_GetFighterCharacter == 2 @169, then BPI_IsOccluded @313 and BPI_GetSpeed @458 (both called), either -> the
// Instigator's state manager SpawnBuffActor(Buff_Weakened_C, HitPlayer, Instigator, WeakenedStacksPerHit, 0.0)
// @627. The Instigator of a self passive is the owner.
// Deviation: Active is the last of the three dealt passes of the hit (dealt_cond); the taken pass's calls on the
// victim's buffs are not ported.
fn passive_begin(w: &mut World, _f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::PassiveApply { active: false };
    }
}

fn passive_dealt(
    w: &mut World,
    _v: ActorRef,
    _o: FighterId,
    _dl: FighterId,
    hit: &HitPacket,
    b: BuffRef,
    _hc: bool,
) -> bool {
    let active = hit.damage.unwrap_or(0.0) > 0.0 && hit.base_knock_back.unwrap_or(0.0) > 0.0;
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::PassiveApply { active };
    }
    true
}

fn passive_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::HitEnemy {
        return false;
    }
    let Some(victim) = a.victim else { return false };
    if !matches!(buff(w, b).map(|x| &x.ext), Some(BuffExt::PassiveApply { active: true })) || !is_batman(w, f) {
        return false;
    }
    let reg: &'static Registry = w.registry;
    let h = reg.chars.get(w.names.str(w.fighter(f).ch));
    let occ = h.and_then(|h| h.is_occluded).is_some_and(|g| g(w, f));
    let speed = h.and_then(|h| h.get_speed).is_some_and(|g| g(w, f));
    if occ || speed {
        let n = inst_view(w, b).num("WeakenedStacksPerHit").unwrap_or(0.0);
        spawn_buff(w, victim, WEAKENED, 0.0, Some(crate::math::to_int32(n)), Some(f), true, None);
    }
    false
}

/// `projectileBlocked`: Comp_Actor_Destructible.IsInvulnerableToProjectiles (Buff_SmokeGrenadeProjectileBlock
/// sets it at its BeginPlay @205 and clears it at its EndPlay @314, the flag's one writer: read as the live buff).
/// pfgDoDamage @4300-4598 pops the call for a dealer that is a projectile on a victim with the flag.
pub fn projectile_blocked(w: &World, f: FighterId) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| b.cls == PROJECTILE_BLOCK_N && b.frames > 0.0)
}

// ---- the actors

fn sound(fighter: i32, ev: Name, x: Real, y: Real) -> Event {
    Event::Sound { fighter, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None }
}

fn emitter(cls: Name, fighter: i32, vfx: Name, x: Real, y: Real, scale: Real) -> Event {
    Event::ProjectileDeath { cls, fighter, vfx: Some(vfx), x, y, ox: 0.0, rot: [0.0; 3], scale, tint: None, to: None }
}

// SmokeGrenade (SpawnedActorParent child). Spawned by Batman_Special_D's Spawner (socket R_Hand,
// InitialMovementDirection (0, 0.1, -1), InitialSpeed 1400). ReceiveBeginPlay @1033: the three shapes PhysicsOnly,
// the team colour (presentation), Delay(0.0667) @2093 -> @503: Box QueryAndPhysics; a valid Comp_Spawner of the
// Instigator -> @15 CompSpawnActor(SmokeGrenadeOcclusionBP_C, ..., the grenade's transform, no speed, AttackID) @137;
// then DetonateClient @275 (-> @2530: SpawnEmitterAttached(PS_BatmanSmokeBombImpact, the OWNER's Mesh, Root) @2615
// with Team = its colour index @2931; sfx_cha_batman_smokebomb_cast at the grenade @3102), K2_DestroyActor @289 and
// sfx_cha_batman_smokebomb_end at the grenade @416 (the call goes on behind the destroy).
// The spawn place: Comp_Spawner spawns with AdjustIfPossibleButAlwaysSpawn, so PostActorConstruction's
// FindTeleportSpot moves the sphere out of the floor or platform it was spawned in before BeginPlay (in the Lab the
// centre goes from 17.8 to 53.0, floor top + 35). The Capsule's template keeps the UShapeComponent profile
// OverlapAllDynamic (WorldDynamic) with WorldStatic and Platform left at Block: the stage block and its platforms.
// Not ported: spawned platforms and Sky Arena's arm donut are not among the boxes (each warns once where the body
// meets the world); Morty's ship pieces are, as kinematic boxes (`grenade_statics`).
// The body: the Capsule (35 / 35: a sphere) simulates with gravity, the X axis locked; Comp_Spawner's launch (the
// direction, normalised, x InitialSpeed, Y along the facing, plus half the owner's velocity) is a velocity change
// of its first step. The ProjectileMovement does nothing to it (TickComponent skips a simulating root: read in the
// exe): the body is PhysX's, the welded actor of the three shapes below (`mvs_collision::rigid_sphere`
// compound_step; its mass under `grenade_body`) with the Capsule's own FBodyInstance: LinearDamping unset (0.01),
// AngularDamping 20, SatchelPhysicalMaterial against the stage's default material (restitution (1.0 + 0.3) / 2 =
// 0.65 over an approach of 200, friction (20 + 0.7) / 2 = 10.35), the spin about X. The Capsule blocks WorldStatic
// and Platform from every side and in every frame. No graph reads the mass (no OnComponentHit); PhysX does, through
// the welded body. The tick order: SmokeGrenade ticks in TG_PrePhysics (SpawnedActorParent ReceiveTick @8771), its
// latent actions run in that actor tick ahead of the frame's PhysX step, and the spawn frame is the Delay's first:
// with latent_frames(0.0667) = 5 the Delay runs out in the fifth frame before that frame's step, after 4 steps. From
// the floor (centre at top + 35) the launch of about -1400 bounces in its first step: up at about 916, so the zone
// is spawned at floor top + 94.4.
// The SCS children MineMesh (on the Capsule) and Box (on MineMesh) simulate with the Capsule: InitBody welds a
// child with collision into its simulating parent's PhysX actor (read in the exe), and BeginPlay's
// SetCollisionEnabled(PhysicsOnly) keeps them in it. The Box (BoxExtent (42, 42, 12) x its scale (0.75, 0.75, 1) x
// MineMesh's 3: half 94.5 x 36 in the plane, its centre 2.5 x 3 = 7.5 over the body's) is a rectangle of the body's
// contacts: SatchelPhysicalMaterial, Platform unlisted (the channel's default, Block).
// MineMesh's convex (SM_BatmanBomb01's one hull, 32 points, within +-29.8 in the plane at scale 3: inside the sphere
// there) is in the welded mass but not a contact shape of the port.
// Deviation: the hull is left out as a contact shape: PhysX's rows with and without it are the same in every case
// tried (floor, air, high, two deep landings, three ledges); a case where it meets a face before the sphere (its
// points out of the plane's projection, +-36.3 in 3D) is not known.
// The impact emitter's event carries the owner's mesh origin; the view attaches it to his Root bone, the mine mesh
// and the aura component are the view's too.
// Not ported: the hit box (HitBoxOn? with the all-zero packet: the Box is query-enabled only in the tick it is
// destroyed), DestroyOnExitedArena (4 ticks of life), PlayerAttachCollision's end overlap (AttachedTo is never
// written: its test OtherActor == AttachedTo @2306 never passes, so @2354-2424 never run and the Box stays welded).

/// `SmokeGrenade`.
#[derive(State, Debug, Default)]
pub struct SmokeGrenade {
    /// The Capsule's, at the root's scale (the size hook's).
    pub radius: Real,
    /// The Delay(0.0667)'s ticks left.
    pub left: Real,
    /// The launch, pending until the first step.
    pub pvx: Real,
    pub pvy: Real,
    pub vx: Real,
    pub vy: Real,
    pub spin: Real,
    /// The angular velocity about the plane's x and y.
    pub wa: Real,
    pub wb: Real,
    /// About the normal (spin = wc x radius).
    pub wc: Real,
    /// The welded Box's turn.
    pub angle: Real,
    pub x: Real,
    pub y: Real,
    /// SpawnedActorParent's stored bounds. Its CheckBlastBoxTimer (0.1 s, the spawn frame not counted) never comes:
    /// the Delay(0.0667) ends the grenade first.
    pub blast_bounds: BlastBounds,
    pub attack_id: i32,
    /// The welded body and the step's buffers. Not state.
    #[state(skip)]
    pub cache: GrenadeCache,
}

/// A grenade's welded body at a root scale (`grenadeBodyAt`'s cache: `None` for one refused, warned) and the
/// buffers its step fills.
#[derive(Clone, Debug, Default)]
pub struct GrenadeCache {
    pub body: Option<(Real, Option<GrenadeBody>)>,
    pub grounds: Vec<Ground>,
    pub statics: Vec<Static>,
}

/// `grenadeBody`'s result: the Capsule's settings and the welded Box and mass.
#[derive(Clone, Copy, Debug)]
pub struct GrenadeBody {
    pub set: Settings,
    pub bx: WeldedBox,
    pub mass: WeldedMass,
}

/// `grenadeBody`: the Capsule's settings and the welded Box per class data; `None` for a material not entered or no
/// Box. `cls`: the class the warnings name (Rick's PolymorphGrenade has the same three shapes). `k`: the root's
/// scale (SetActorScale3D: every shape and offset under the Capsule, its own radius included).
/// The welded mass (UpdateMassProperties, `welded_mass`): the Capsule (bOverrideMass, the default 100; a sphere of
/// 35), MineMesh (MassInKgOverride 46.94; its hull at scale 3) and the Box (no override: (0.001 x 189 x 189 x
/// 72)^0.75 = 361.15 at Z 7.5): 508.09 kg, the centre of mass 5.331 over the Capsule's centre, Ixx 1304972.8 kg cm^2
/// (k = m R^2 / I 0.47696 at R 35).
pub fn grenade_body(w: &World, a: &ActorDef, cls: &str, k: Real) -> Option<GrenadeBody> {
    let c = &a.components;
    let cap = obj(c, "Capsule");
    let set = sphere_settings(w, cap.and_then(|c| obj(c, "BodyInstance")), cls);
    // mm: the mesh between the Box and the Capsule (MineMesh); none when the Box hangs on the Capsule itself
    // (Steven's HealGrenade).
    let bx = obj(c, "Box");
    let mm_name = bx.and_then(|b| text(b, "AttachParent")).filter(|p| !p.is_empty() && *p != "Capsule");
    let mm = mm_name.and_then(|n| obj(c, n));
    let bset = bx.and_then(|b| sphere_settings(w, obj(b, "BodyInstance"), &format!("{cls} Box")));
    let (Some(set), Some(bx), Some(bset)) = (set, bx, bset) else {
        if bx.is_none() {
            warn_text(w, &format!("{cls}: no Box component: the body is not stepped"));
        }
        return None;
    };
    // the Box's place under its parent's scale (MineMesh on the Capsule, neither turned nor moved) (an unset
    // BoxExtent is UBoxComponent's default, 32 on each axis)
    let xyz = |o: Option<&Props>, d: Real| {
        let g = |k: &str| o.and_then(|o| num(o, k)).unwrap_or(d);
        [g("X"), g("Y"), g("Z")]
    };
    let ps = match mm.and_then(|m| obj(m, "RelativeScale3D")) {
        Some(s) => xyz(Some(s), Real::NAN),
        None => [1.0, 1.0, 1.0],
    };
    let s = match obj(bx, "RelativeScale3D") {
        Some(s) => xyz(Some(s), Real::NAN),
        None => [1.0, 1.0, 1.0],
    };
    let e = xyz(obj(bx, "BoxExtent"), 32.0);
    let (ex, ey, ez) = (e[0] * s[0] * ps[0] * k, e[1] * s[1] * ps[1] * k, e[2] * s[2] * ps[2] * k);
    if mm.is_some_and(|m| m.contains_key("RelativeLocation") || m.contains_key("RelativeRotation"))
        || bx.contains_key("RelativeRotation")
        || mm.is_some_and(|m| text(m, "AttachParent") != Some("Capsule"))
    {
        warn_text(w, &format!("{cls}: the Box's parent chain is moved or turned (not read)"));
    }
    let bbi = obj(bx, "BodyInstance");
    let plat = bbi
        .and_then(|b| obj(b, "CollisionResponses"))
        .and_then(|r| r.get("ResponseArray"))
        .and_then(Value::as_arr)
        .and_then(|l| l.iter().filter_map(Value::as_obj).find(|r| text(r, "Channel") == Some("Platform")))
        .and_then(|r| text(r, "Response"))
        .unwrap_or("ECR_Block");
    let oz = obj(bx, "RelativeLocation").and_then(|l| num(l, "Z")).unwrap_or(0.0) * ps[2] * k;
    let cr = cap.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(35.0) * k;
    let hh = cap.and_then(|c| num(c, "CapsuleHalfHeight")).unwrap_or(35.0) * k;
    let mut parts = vec![
        body_mass(w, cap.and_then(|c| obj(c, "BodyInstance")), &[sphyl_mass(cr, hh, 0.0)], cls),
        body_mass(w, bbi, &[box_mass(ex, ey, ez, [0.0, 0.0, oz])], &format!("{cls} Box")),
    ];
    if let Some(mm) = mm {
        match hull_mass(mm, [ps[0] * k, ps[1] * k, ps[2] * k], [0.0; 3]) {
            Some(h) => parts.push(body_mass(
                w,
                obj(mm, "BodyInstance"),
                &[h],
                &format!("{cls} {}", mm_name.unwrap_or_default()),
            )),
            None => warn_text(
                w,
                &format!(
                    "{cls}: the Box's parent has no hull mass in the data (the import's welded mesh mass): left out of the welded mass"
                ),
            ),
        }
    }
    let parts: Option<Vec<_>> = parts.into_iter().collect();
    let parts = parts?;
    Some(GrenadeBody {
        set,
        mass: welded_mass(w, &parts, cr, cls),
        bx: WeldedBox {
            hx: f64::from(ey),
            hy: f64::from(ez),
            hd: f64::from(ex),
            oy: f64::from(oz),
            offset: f64::from(min(max(0.01 * min(min(ex, ey), ez), 0.0001), 1.0)),
            platforms: plat == "ECR_Block",
            restitution: bset.restitution,
            friction: bset.friction,
        },
    })
}

// The statics the welded body steps against besides the stage block: the stage's platforms, then the spawned
// actors' solid boxes whose pair with the body blocks (the world actors' SpawnedPlatformParent parts, then the
// projectiles' boxes, in the lists' order; on and alive). A pair blocks when both sides answer Block to the other's
// object type (GetCollisionResponseToComponent: the minimum): the Capsule (Custom, no ObjectType serialized:
// WorldDynamic) against the part's type, and the part against WorldDynamic; the welded Box (PhysicsBody) the same
// with PhysicsBody. A response is the template's ResponseArray entry, else the channel's default (`response_to`).
// compound_step gives every platform the Box's one Platform rule (WeldedBox.platforms): a part the Capsule blocks and
// the Box does not (or the other way) is warned and left out. Of the ported parts, WW_ShieldActor_U_Special's
// Collision (profile Platform) blocks both; the shields' WallbounceCollision ignores WorldDynamic and PhysicsBody;
// BugsBunnyRocketFProjectile's Collision (Platform, WorldDynamic Ignore, PhysicsBody Block) and RocketUp's Box
// (WorldDynamic: both sides Overlap) are warned or left out by the rule. Morty's ship pieces (object type Platform:
// the five Platform-profile boxes, and `Collision` by its SpawnedPlatformParent archetype; WorldDynamic and
// PhysicsBody left at Block) block both for SmokeGrenade and PolymorphGrenade, whose Capsule and Box leave Platform at
// Block and no graph turns it: they come first, as kinematic boxes rising with the ship (`ship_statics`).
// Deviation: a part is met where it stands at the step, as a static: a part that moves (the shields on a socket
// follow their owner each tick) gives the body no velocity of its own (PhysX's kinematic target velocity, or a
// teleport, is not read for an attached box).

/// Whether the pair of the grenade's components `c` and a part's template blocks the Capsule (bit 1) and the Box
/// (bit 2).
fn pair_of(c: &Props, part: &Props) -> u8 {
    pair_typed(c, body_info(Some(part), None).map_or(-1, |b| b.kind), part)
}

/// [`pair_of`] with the part's object type `pt` given.
fn pair_typed(c: &Props, pt: i32, part: &Props) -> u8 {
    let kind = |p: Option<&Props>| body_info(p, None).map_or(-1, |b| b.kind);
    let (ct, bt) = (kind(obj(c, "Capsule")), kind(obj(c, "Box")));
    let block = |x: Option<&Props>, xt: i32| {
        pt >= 0
            && xt >= 0
            && response_to(x, crate::real(pt)) == name!("ECR_Block")
            && response_to(Some(part), crate::real(xt)) == name!("ECR_Block")
    };
    u8::from(block(obj(c, "Capsule"), ct)) | (u8::from(block(obj(c, "Box"), bt)) << 1)
}

/// One solid part: taken when its pair blocks both the Capsule and the Box (the Capsule alone without the Box's
/// Platform rule).
fn take_part(w: &World, c: &Props, bx: &WeldedBox, cls: &str, of: Name, t: Option<&Props>) -> bool {
    let Some(t) = t else { return false };
    take_pair(w, bx, cls, of, pair_of(c, t))
}

/// [`take_part`] on the pair's answer `v` (`pair_of`).
fn take_pair(w: &World, bx: &WeldedBox, cls: &str, of: Name, v: u8) -> bool {
    if v == 0 {
        return false;
    }
    if v != (if bx.platforms { 3 } else { 1 }) {
        warn_text(
            w,
            &format!(
                "{cls}: {}'s solid box blocks one of the welded Capsule and Box only (not ported: left out)",
                w.names.str(of)
            ),
        );
        return false;
    }
    true
}

/// `grenadeStatics`'s spawned parts (the stage platforms are the caller's), in the lists' order, pushed onto `out`
/// (a ship's piece moves as a kinematic box; another spawned part is no marked stage piece: no kinematic move, no
/// WorldStatic solid). `owner`, `cls`: the
/// grenade's (its class data's components).
pub fn grenade_parts(w: &World, owner: FighterId, cls: Name, bx: &WeldedBox, out: &mut Vec<Static>) {
    let data = w.data.clone();
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(owner), cls) else { return };
    let c = &def.components;
    let cs = w.names.str(cls);
    // The ships' pieces, as `World::blockers` lists them ahead of the spawned parts. Their template's own object type
    // is not read (`Collision` saves none over its SpawnedPlatformParent archetype's Platform): Platform for each.
    let mut i = 0;
    while let Some(sh) = w.actors.ships.nth(i) {
        i += 1;
        let of = w.actors.ships.get(sh).expect("a live ship").cls;
        crate::chars::morty::ship_statics(
            w,
            sh,
            |_, t| t.is_some_and(|t| take_pair(w, bx, cs, of, pair_typed(c, OBJ_PLATFORM, t))),
            out,
        );
    }
    for (_, x) in w.actors.actors.iter() {
        let Some(pl) = &x.plat else { continue };
        if x.dead {
            continue;
        }
        for s in pl.solids() {
            if s.on {
                let t = crate::world::spawned_platform::part_template(w, x.owner, x.cls, s.comp);
                if take_part(w, c, bx, cs, x.cls, t) {
                    let g = Ground {
                        left: f64::from(s.left),
                        right: f64::from(s.right),
                        bottom: f64::from(s.bottom),
                        top: f64::from(s.top),
                    };
                    out.push(Static { g, kinematic: None, static_solid: false, poly: None });
                }
            }
        }
    }
    for (_, pr) in w.actors.projectiles.iter() {
        let Some(solids) = &pr.solids else { continue };
        if pr.dead || pr.no_collision {
            continue;
        }
        for s in solids {
            if s.on {
                let t = crate::world::spawned_platform::box_template(w, pr, s);
                if take_part(w, c, bx, cs, pr.cls, t) {
                    let g = Ground {
                        left: f64::from(s.left),
                        right: f64::from(s.right),
                        bottom: f64::from(s.bottom),
                        top: f64::from(s.top),
                    };
                    out.push(Static { g, kinematic: None, static_solid: false, poly: None });
                }
            }
        }
    }
}

/// `withStatics(grenadeStatics)`: the statics the welded body steps against into `out` (cleared first): the stage's
/// solids (`static_solids`) and platforms, the spawned parts (`grenade_parts`), then the polygon solids. `why`: the body the stage's
/// warnings name; `grounds`: a buffer.
pub fn grenade_statics(
    w: &mut World,
    owner: FighterId,
    cls: Name,
    bx: &WeldedBox,
    why: Name,
    grounds: &mut Vec<Ground>,
    out: &mut Vec<Static>,
) {
    grounds.clear();
    let mut polys = Vec::new();
    static_solids_and_polys(w, SOLID_NAV, why, grounds, Some(&mut polys));
    grounds.extend(w.stage.stage_platforms.iter().map(|p| Ground {
        left: f64::from(p.left),
        right: f64::from(p.right),
        bottom: f64::from(p.bottom),
        top: f64::from(p.top),
    }));
    out.clear();
    out.extend(grounds.iter().map(|&g| sphere_static(w, g)));
    grenade_parts(w, owner, cls, bx, out);
    // (the polygon solids after the boxes)
    out.extend(polys.iter().map(PolyBlock::rigid));
}

fn grenade_mut(w: &mut World, id: ActorId) -> Option<&mut SmokeGrenade> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::SmokeGrenade(g) => Some(g),
        _ => None,
    }
}

fn set_dead(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

fn stage_platform_grounds(w: &World) -> Vec<Ground> {
    w.stage
        .stage_platforms
        .iter()
        .map(|p| Ground {
            left: f64::from(p.left),
            right: f64::from(p.right),
            bottom: f64::from(p.bottom),
            top: f64::from(p.top),
        })
        .collect()
}

/// One physics step: the pending launch as a velocity change, then the body against the stage block and its
/// platforms and the spawned actors' solid boxes. What the body would meet besides them warns.
fn grenade_move(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, cls, k) = (a.owner, a.cls, a.size_scale.unwrap_or(1.0));
    let data = w.data.clone();
    let Some(def) = actor_data(w, &data, ActorRef::Fighter(owner), cls) else { return };
    let Some(g) = grenade_mut(w, id) else { return };
    let mut c = std::mem::take(&mut g.cache);
    if c.body.is_none_or(|(at, _)| at != k) {
        c.body = Some((k, grenade_body(w, def, w.names.str(cls), k)));
    }
    let Some(body) = c.body.and_then(|(_, b)| b) else {
        // its material was refused (warned)
        if let Some(g) = grenade_mut(w, id) {
            g.cache = c;
        }
        return;
    };
    if w.data.stage.name.starts_with("Map_M011") {
        warn_once(w, name!("SmokeGrenade: Sky Arena's arm donut collider is not ported (profile and place unread)"));
    }
    grenade_statics(w, owner, cls, &body.bx, name!("SmokeGrenade_C body"), &mut c.grounds, &mut c.statics);
    let floor = sphere_static(w, w.stage.ground);
    let Some(g) = grenade_mut(w, id) else { return };
    g.vx += g.pvx;
    g.vy += g.pvy;
    g.pvx = 0.0;
    g.pvy = 0.0;
    let mut b = Body {
        x: f64::from(g.x),
        y: f64::from(g.y),
        vx: f64::from(g.vx),
        vy: f64::from(g.vy),
        spin: f64::from(g.spin),
        radius: f64::from(g.radius),
        angle: f64::from(g.angle),
        wa: f64::from(g.wa),
        wb: f64::from(g.wb),
        wc: f64::from(g.wc),
        ..Body::default()
    };
    Solver::default().compound_step(&mut b, &floor, Some(&c.statics), DELTA_SECONDS, &body.set, &body.bx, &body.mass);
    g.cache = c;
    (g.x, g.y, g.vx, g.vy, g.spin, g.angle, g.wa, g.wb, g.wc) = (
        crate::math::narrow(b.x),
        crate::math::narrow(b.y),
        crate::math::narrow(b.vx),
        crate::math::narrow(b.vy),
        crate::math::narrow(b.spin),
        crate::math::narrow(b.angle),
        crate::math::narrow(b.wa),
        crate::math::narrow(b.wb),
        crate::math::narrow(b.wc),
    );
}

fn grenade_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let np = crate::world::spawning::request_props(w, f, r);
    let s = crate::world::spawn_spot::spawn_place(w, f, &np);
    let data = w.data.clone();
    let def = actor_data(w, &data, ActorRef::Fighter(f), r.cls);
    let comps = def.map(|d| &d.components);
    let cap_r = comps.and_then(|c| obj(c, "Capsule")).and_then(|c| num(c, "CapsuleRadius")).unwrap_or(35.0);
    let mut boxes = crate::world::stage_query::static_shapes(w, SOLID_NAV, name!("smoke grenade spawn"));
    boxes.boxes.extend(stage_platform_grounds(w));
    let at = crate::world::spawn_spot::find_teleport_spot(boxes.view(), s.x, s.y, cap_r);
    let d = obj(&np, "InitialMovementDirection");
    let (dy, dz) = (d.and_then(|d| num(d, "Y")).unwrap_or(0.0), d.and_then(|d| num(d, "Z")).unwrap_or(0.0));
    let sp = num(&np, "InitialSpeed").unwrap_or(0.0);
    // the direction's Normal (tolerance 0.0001)
    let u = crate::math::ue::normal([0.0, dy, dz], crate::math::ue::KINDA_SMALL_NUMBER);
    let facing = crate::fighter::facing(w, f);
    let (fvx, fvy) = (w.fighter(f).body.vx, w.fighter(f).body.vy);
    let mut g = SmokeGrenade {
        radius: cap_r,
        left: latent_frames(0.0667),
        x: at.x,
        y: at.y,
        pvx: ((u[1] * sp) * facing + fvx / 2.0),
        pvy: ((u[2] * sp) + fvy / 2.0),
        blast_bounds: new_blast_bounds(),
        attack_id: r.attack_id,
        ..SmokeGrenade::default()
    };
    // the body read once at the spawn (its warnings), kept for the steps at scale 1
    if let Some(def) = def {
        g.cache.body = Some((1.0, grenade_body(w, def, GRENADE, 1.0)));
    }
    let p = def.map(|d| &d.p);
    let doe = p.is_some_and(|p| flag(p, "DestroyOnExitedArena?"));
    let force = p.is_some_and(|p| flag(p, "ForceSpawnPositionInsideBlastBox"));
    let sb = spawn_box(w);
    // the parent's BeginPlay loop
    let dead = blast_box_loop(&sb, &mut g.x, &mut g.y, None, force, &mut g.blast_bounds, doe);
    (WorldActorState::SmokeGrenade(g), dead)
}

/// The actor's tick (TG_PrePhysics): the Delay first, then the frame's physics step.
fn grenade_step(w: &mut World, id: ActorId) {
    let Some(g) = grenade_mut(w, id) else { return };
    g.left -= 1.0;
    if g.left > 0.0 {
        grenade_move(w, id);
        return;
    }
    let (x, y, attack_id) = (g.x, g.y, g.attack_id);
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    spawn_zone(w, f, x, y, attack_id);
    let fid = w.fighter(f).id;
    let (mx, my) = (crate::fighter::mesh_wx(w, f), crate::fighter::mesh_wy(w, f));
    w.events.push(emitter(GRENADE_N, fid, name!("PS_BatmanSmokeBombImpact"), mx, my, 1.0));
    w.events.push(sound(fid, name!("sfx_cha_batman_smokebomb_cast"), x, y));
    set_dead(w, id);
    w.events.push(sound(fid, name!("sfx_cha_batman_smokebomb_end"), x, y));
}

/// The welded body (the Capsule, its Box and their mass) at the root's scale (`grenade_body`); the view too.
fn grenade_scale(w: &mut World, id: ActorId, s: Real, old: Real) {
    if let Some(g) = grenade_mut(w, id) {
        g.radius *= s / old;
    }
}

// SmokeGrenadeOcclusionBP (FogDuration 6, Die With Instigator).
// ReceiveBeginPlay @3610: the Instigator (a BaseFighter) CheckHasBuff(Buff_Batman_Chimney) @3715 -> ChimneyPerkActive
// @3899 (the Chimney perk); with it @3918 PlayerCollision.SetCapsuleHalfHeight(ChimneyHalfHeight 0, true) @3932,
// SetCapsuleRadius(ChimneyRadius 0, true) @3974, K2_SetRelativeLocation(ChimneyCapsuleRelativeLocation (0, 0, 0))
// @4016. The native read: the half height stays 225, the radius becomes 0 (a physics capsule of radius 0.1): a
// vertical line through the centre, 225 up and down. The two UpdateOverlaps find FogActivated false (no buff given
// or taken); from then on PlayerCollision overlaps only a fighter touching that line. PlayerAttachCollision (the 225
// sphere) is not touched: the Clean hazard keeps it.
// Then ActivateFog @4068, then sfx_cha_batman_smokebomb_loop on the actor @4082.
// ActivateFog @2149, off a dedicated server @2287: FogVisuals @2218, Delay(0.5) @2232 -> @764 (FogActivated = true,
// StartFogBuff, Delay(FogDuration) @789 -> @654: FogActivated = false, EndFogBuff) and Delay(0.5) @2297 -> @680
// (StartFogBuff, FogActivated = true, Delay(FogDuration) @705 -> @15: FogActivated = false, EndFogBuff,
// sfx_cha_batman_smokebomb_loop_stop @40 and _end @105 on the actor). The two pairs fire in the same ticks; the
// second StartFogBuff and EndFogBuff find nothing left to do (CanActorReceiveBuffs' Set_Contains, BuffedActors
// emptied), so their order does not matter.
// FogVisuals @2488: PS_BatmanSmoke at the OWNER's Mesh location + (0, 0, 100), scale 0.6 @2809 (ChimneyPerkActive:
// SelectVector @2645 / @2704, ChimneyVFXOffset (0, 0, 0) and ChimneyVFXScale 0.5), Team @3144 (presentation: the
// emitter event at the spawn); Delay(FogDuration + 0.25) @3342 -> @494: the particle's Dissolve 1 and Deactivate,
// Delay(0.5) @584 -> @639 K2_DestroyActor. So the zone lives 6.75 s; its buffs run from 0.5 s to 6.5 s.
// The particle's Dissolve / Deactivate at 6.25 s and its K2_DestroyComponent at an end with the fog on (@460) are the
// view's: it reads `visuals` and `fog_activated`.
// StartFogBuff @4148: PlayerCollision.GetOverlappingActors(BaseFighter) -> OnBeginOverlap each. EndFogBuff @1662:
// Set_ToArray(BuffedActors) -> OnEndOverlap each.
// PlayerCollision (a capsule 225 / 225 on the root: a sphere of 225; Pawn and HurtBox overlap) begin overlap @1986:
// authority, FogActivated, !Set_Contains(BuffedActors) -> OnBeginOverlap; end overlap @4817: FogActivated,
// Set_Contains, !PlayerCollision.IsOverlappingActor -> OnEndOverlap (and the invulnerable array's RemoveItem).
// Deviation: the overlap is tested once a tick in the zone's step, ahead of its latent actions, against the fighters'
// root capsules and hurt shapes at their pose; a pass through inside one tick is missed.
// ReceiveEndPlay @5111: FogActivated -> @266: FogActivated = false, EndFogBuff, loop_stop and end, the particle
// destroyed.
// Unverified: the loop of a zone ended before 0.5 s (no loop_stop) is stopped at its end (the actor's AkComponent
// going away: stock bStopWhenOwnerDestroyed, not read).
// Not ported: the Tick @5145 -> @986 pruning OverlappingInvulnerableFighters and SpawnBuffsOnActor's AddUnique @2088
// (the array has no reader); IncrementCoopActions @1881 (the coop count of the player state: no port);
// SmokeOcclusionParent's destructibles (its follower branch: `follower_overlaps`).
// The parent's BeginPlay blast-box loop and CheckBlastBoxTimer run as for any SpawnedActorParent (`spawn_zone`, the
// step): the zone does not move (ProjectileGravityScale 0, no speed), so only a spawn outside the area ends it.

/// `SmokeZone`.
#[derive(State, Debug, Default)]
pub struct SmokeZone {
    pub x: Real,
    pub y: Real,
    pub attack_id: i32,
    /// PlayerCollision.CapsuleRadius.
    pub radius: Real,
    pub fog_activated: bool,
    /// The two Delay(0.5).
    pub activate: Real,
    /// Their Delay(FogDuration).
    pub fog_end: Real,
    /// FogVisuals' Delay(FogDuration + 0.25).
    pub visuals: Real,
    /// Its Delay(0.5).
    pub destroy: Real,
    /// The loop posted at BeginPlay, not yet stopped.
    pub looping: bool,
    /// The fighters PlayerCollision overlaps (ids; a follower: `follower_key`).
    pub inside: Vec<i32>,
    /// BuffedActors (ids, followers' keys), in insertion order.
    pub buffed: Vec<i32>,
    /// The zone's ticks.
    pub age: Real,
    /// CheckBlastBoxTimer's next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
    /// BPI Clean's Blocked: its Delay(0.0) is pending, taken at the head of the zone's next step. `None` until then.
    pub blocked_destroy: Option<bool>,
    /// ChimneyPerkActive (BeginPlay @3899): `None` when false.
    pub chimney: Option<bool>,
}

fn zone_of(w: &World, id: ActorId) -> Option<&SmokeZone> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::SmokeZone(z) => Some(z),
        _ => None,
    }
}

fn zone_mut(w: &mut World, id: ActorId) -> Option<&mut SmokeZone> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::SmokeZone(z) => Some(z),
        _ => None,
    }
}

/// PlayerCollision's (and PlayerAttachCollision's) radius and the Chimney line's half height at the root's scale.
fn zone_k(w: &World, id: ActorId) -> Real {
    w.actors.actors.get(id).and_then(|a| a.size_scale).unwrap_or(1.0)
}

fn zone_owner(w: &World, id: ActorId) -> Option<FighterId> {
    w.actors.actors.get(id).map(|a| a.owner)
}

fn zone_key(w: &World, id: ActorId) -> SoundKey {
    SoundKey { tag: SMOKE_KEY, n: w.actors.actors.get(id).map_or(-1, |a| a.seq) }
}

/// `spawnZone`: the grenade's CompSpawnActor of the zone at (x, y).
pub fn spawn_zone(w: &mut World, f: FighterId, x: Real, y: Real, attack_id: i32) {
    let data = w.data.clone();
    let Some(a) = actor_data(w, &data, ActorRef::Fighter(f), ZONE_N) else {
        warn_text(w, &format!("spawned actor not exported ({ZONE})"));
        return;
    };
    let pc = obj(&a.components, "PlayerCollision");
    let mut z = SmokeZone {
        x,
        y,
        attack_id,
        radius: pc.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(225.0),
        activate: latent_frames(0.5),
        visuals: latent_frames(num(&a.p, "FogDuration").unwrap_or(0.0) + 0.25),
        looping: true,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        ..SmokeZone::default()
    };
    // Unverified: the zone's Instigator taken as the grenade's owner, who spawned it (as every other read of the
    // zone's owner here).
    if buff_of(w, f, CHIMNEY).is_some() {
        z.chimney = Some(true);
    }
    // The parent's BeginPlay loop: a zone it destroys runs the class's BeginPlay whole (the actor goes once BeginPlay
    // returns) and no latent action of it.
    let doe = flag(&a.p, "DestroyOnExitedArena?");
    let force = flag(&a.p, "ForceSpawnPositionInsideBlastBox");
    let sb = spawn_box(w);
    let dead = blast_box_loop(&sb, &mut z.x, &mut z.y, None, force, &mut z.blast_bounds, doe);
    let chimney = z.chimney.is_some();
    let (sc, off) = (
        obj(&a.p, "ChimneyVFXScale").and_then(|v| num(v, "X")).unwrap_or(0.5),
        obj(&a.p, "ChimneyVFXOffset").and_then(|v| num(v, "Z")).unwrap_or(0.0),
    );
    let x = crate::world::spawning::new_world_actor(
        w,
        ZONE_N,
        WorldActorClass::SmokeGrenadeOcclusionBP,
        f,
        dead,
        WorldActorState::SmokeZone(z),
    );
    let id = w.actors.actors.insert(x);
    let charge = instigator_charge(w, f);
    crate::world::spawning::comp_spawn_actor(w, id, charge, f);
    let fid = w.fighter(f).id;
    let (mx, my) = (crate::fighter::mesh_wx(w, f), crate::fighter::mesh_wy(w, f));
    let vfx = name!("PS_BatmanSmoke");
    if chimney {
        w.events.push(emitter(ZONE_N, fid, vfx, mx, my + off, sc)); // FogVisuals @2645-2809
    } else {
        w.events.push(emitter(ZONE_N, fid, vfx, mx, my + 100.0, 0.6)); // FogVisuals @2809
    }
    let key = zone_key(w, id);
    w.events.push(Event::SoundStart { fighter: fid, ev: name!("sfx_cha_batman_smokebomb_loop"), key });
}

/// SmokeOcclusionParent CanActorReceiveBuffs: a BaseFighter (or a follower), not AlreadyRungOut, not in BuffedActors,
/// and a BPI_C017 pawn only when GiantCrouched (@491-668).
fn can_receive(w: &World, id: ActorId, o: FighterId) -> bool {
    use crate::chars::iron_giant::{giant_crouched, giant_of};
    let oid = w.fighter(o).id;
    w.fighter(o).life.dead <= 0.0
        && zone_of(w, id).is_some_and(|z| !z.buffed.contains(&oid))
        && (giant_of(w, o).is_none() || giant_crouched(w, o))
}

/// AttemptToApplyBuffsOnActor: CanActorReceiveBuffs -> SpawnBuffsOnActor (Success true for a fighter),
/// Set_Add(BuffedActors).
fn attempt_to_apply(w: &mut World, id: ActorId, o: FighterId) {
    if !can_receive(w, id, o) {
        return;
    }
    spawn_buffs_on(w, id, o);
    let oid = w.fighter(o).id;
    if let Some(z) = zone_mut(w, id) {
        z.buffed.push(oid);
    }
}

/// AttemptToRemoveBuffsOnActor: Set_Contains -> RemoveBuffsOnActor, Set_Remove(BuffedActors).
fn attempt_to_remove(w: &mut World, id: ActorId, o: FighterId) {
    let oid = w.fighter(o).id;
    let Some(k) = zone_of(w, id).and_then(|z| z.buffed.iter().position(|&x| x == oid)) else { return };
    remove_buffs_on(w, o);
    if let Some(z) = zone_mut(w, id) {
        z.buffed.remove(k);
    }
}

/// SmokeOcclusionParent OnBeginOverlap: an actor with Comp_Pawn_Giant has the zone's HandleGiantCrouch bound to its
/// GiantCrouchChanged (@94-168), then AttemptToApplyBuffsOnActor @209.
fn begin_overlap(w: &mut World, id: ActorId, o: FighterId) {
    crate::chars::iron_giant::bind_giant_crouch(w, id, o);
    attempt_to_apply(w, id, o);
}

/// OnEndOverlap: HandleGiantCrouch unbound (@94-168), then AttemptToRemoveBuffsOnActor @209. Its callers (the end
/// overlap @4817 and EndFogBuff) pass only actors in BuffedActors: a Giant refused standing, or one who stood up
/// inside (HandleGiantCrouch's removal), stays bound until the zone is destroyed, wherever he goes, and a crouch then
/// buffs him (HandleGiantCrouch tests neither FogActivated nor the overlap).
fn end_overlap(w: &mut World, id: ActorId, key: i32) {
    if key < 0 {
        follower_end_overlap(w, id, key);
        return;
    }
    let Some(o) = w.by_id(key) else { return }; // a minion destroyed since (`drop_zone_gone`)
    crate::chars::iron_giant::unbind_giant_crouch(w, id, o);
    attempt_to_remove(w, id, o);
}

/// HandleGiantCrouch(IsCrouched, Actor): crouched -> AttemptToApplyBuffsOnActor @14, else
/// AttemptToRemoveBuffsOnActor @51.
fn handle_giant_crouch(w: &mut World, id: ActorId, o: FighterId, crouched: bool) {
    if crouched {
        attempt_to_apply(w, id, o);
    } else {
        attempt_to_remove(w, id, o);
    }
}

/// SpawnBuffsOnActor: SpawnBuffActor(Buff_Invisibility_Fog_C, Actor, Actor, 1, 0.0) @145 on anyone; then for a
/// BP_Fighter_Character of the Instigator's team (Comp_Actor_Destructible TeamIndex @638) that is not Batman (@1050)
/// and not the Instigator (@1130): Buff_SpeedUp (0 stacks, 6.0) @1271, Buff_Invulnerable (0, 0.75) @1406,
/// Buff_DecrementEvadeCount (0, 6.0) @1541, Buff_SmokeGrenadeProjectileBlock (0, 6.0) @1676; a Batman of the team or
/// the Instigator: Buff_SmokeGrenadeProjectileBlock (0, 6.0) @1974; another team: the Fog alone (and the invulnerable
/// array, @2088).
fn spawn_buffs_on(w: &mut World, id: ActorId, o: FighterId) {
    spawn_buff(w, o, FOG, 0.0, Some(1), Some(o), true, None);
    let Some(owner) = zone_owner(w, id) else { return };
    if w.fighter(o).team.team != w.fighter(owner).team.team {
        return;
    }
    if !is_batman(w, o) && o != owner {
        spawn_buff(w, o, SPEED_UP, 6.0, Some(0), Some(o), true, None);
        spawn_buff(w, o, INVULNERABLE, 0.75, Some(0), Some(o), true, None);
        spawn_buff(w, o, EVADE, 6.0, Some(0), Some(o), true, None);
    }
    spawn_buff(w, o, PROJECTILE_BLOCK_N, 6.0, Some(0), Some(o), true, None);
}

/// RemoveBuffsOnActor: over the keys of the actor's Comp_Actor_Parent.Children (its buff actors), each cast:
/// Buff_Invisibility_Fog RemoveStack(1) @560, Buff_SlowDown K2_DestroyActor @739, Buff_DecrementEvadeCount @913 and
/// Buff_SmokeGrenadeProjectileBlock @1087 K2_DestroyActor, Buff_SpeedUp DelayDestroy @1257. Every such buff of the
/// fighter, whoever spawned it.
// Unverified: the children map's key order is taken as the buff list's (spawn order).
fn remove_buffs_on(w: &mut World, o: FighterId) {
    let list: Vec<BuffRef> =
        w.fighter(o).buffs.buffs.iter().map(|b| BuffRef { fighter: o, serial: b.serial }).collect();
    for r in list {
        let Some(b) = buff(w, r) else { continue };
        if b.frames <= 0.0 {
            continue;
        }
        let cls = b.cls;
        if cls == FOG {
            remove_stack(w, o, r, 1);
        } else if cls == SLOW_DOWN || cls == EVADE || cls == PROJECTILE_BLOCK_N {
            end_buff(w, o, r);
        } else if cls == SPEED_UP {
            delay_destroy(w, r);
        }
    }
}

// ---- the zone and a follower (`actors::follower`; the list key is `follower_key`)

/// OnBeginOverlap(follower): no Comp_Pawn_Giant (nothing bound), AttemptToApplyBuffsOnActor @209.
/// CanActorReceiveBuffs @0-141: BPI_Follower passes the first test; the BP_FighterCharacter cast fails, so the
/// BPI_BaseFighter cast fails and AlreadyRungOut is taken false (@285 -> @714); then !Set_Contains(BuffedActors) and
/// no BPI_C017 @491 -> CanReceiveBuffs. SpawnBuffsOnActor @0-294: its Comp_State_Manager SpawnBuffActor(
/// Buff_Invisibility_Fog_C, Actor, Actor, 1, 0.0, true) @145; no BP_FighterCharacter @294 -> Success true @893,
/// Set_Add.
fn follower_apply(w: &mut World, id: ActorId, a: ActorId) {
    let key = crate::math::to_int32(crate::actors::follower::follower_key(w, a));
    if zone_of(w, id).is_none_or(|z| z.buffed.contains(&key)) {
        return;
    }
    spawn_actor_buff(w, ActorRef::Actor(a), FOG, 1, 0.0, true, None);
    if let Some(z) = zone_mut(w, id) {
        z.buffed.push(key);
    }
}

/// OnEndOverlap(follower) -> AttemptToRemoveBuffsOnActor: Set_Contains -> RemoveBuffsOnActor over its buff children
/// (Fog RemoveStack(1) @560; Buff_DecrementEvadeCount @913 and Buff_SmokeGrenadeProjectileBlock @1087 destroyed;
/// Buff_SpeedUp's DelayDestroy @1257: no such buff on an actor, none has an actor-side entry), Set_Remove.
// Unverified: a follower destroyed while in the lists leaves them with no end overlap (its buffs end with it).
// Not ported: Buff_SlowDown's K2_DestroyActor @739 on a follower: no buff class of the port is Buff_SlowDown with an
// actor-side entry, so no follower carries one yet (the fighter branch, `remove_buffs_on`, matches it by name).
fn follower_end_overlap(w: &mut World, id: ActorId, key: i32) {
    let Some(k) = zone_of(w, id).and_then(|z| z.buffed.iter().position(|&x| x == key)) else { return };
    let a = crate::actors::follower::follower_by_key(w, crate::real(key));
    if let Some(z) = zone_mut(w, id) {
        z.buffed.remove(k);
    }
    let Some(a) = a else { return };
    let Some(Some(st)) = actor_elem(w, a) else { return };
    let list: Vec<(u32, BuffClass)> = st.buffs.iter().map(|b| (b.serial, b.class)).collect();
    for (s, c) in list {
        match c {
            BuffClass::BuffInvisibilityFog => actor_remove_stack(w, a, s, 1, false),
            BuffClass::BuffDecrementEvadeCount | BuffClass::BuffSmokeGrenadeProjectileBlock => {
                end_actor_buff(w, a, s);
            }
            BuffClass::BuffSpeedUp => warn_once(
                w,
                name!("SmokeGrenadeOcclusionBP: Buff_SpeedUp on a follower (DelayDestroy on an actor: not ported)"),
            ),
            _ => {}
        }
    }
}

/// The Chimney's PlayerCollision: a capsule of radius 0.1 about the upright segment of half length 225 (half height
/// 225 less radius 0), as a box shape of that segment (the two other half extents 1e-6: the box test takes no zero
/// axis) with r 0.1, measured as the spheres are against a hurt capsule (the segments within the sum of radii).
const CHIMNEY_R: Real = 0.1;
const CHIMNEY_HALF: Real = 225.0;

/// The followers' overlap events of the zone's step (behind the fighters'). The begin overlap @1986: FogActivated,
/// !Set_Contains -> OnBeginOverlap; the end overlap @4817: FogActivated, Set_Contains, no longer overlapping ->
/// OnEndOverlap. StartFogBuff @4148 asks GetOverlappingActors(BaseFighter): a follower inside when the fog comes on
/// is not buffed until it leaves and enters again.
// Unverified: the build's event order between a fighter and a follower in one tick is not read.
fn follower_overlaps(w: &mut World, id: ActorId) {
    use crate::actors::follower::{follower_by_key, follower_key, follower_of, follower_touches};
    retain_keys(w, id, |w, k| k >= 0 || follower_by_key(w, crate::real(k)).is_some());
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        if w.actors.actors.get(x).is_none_or(|a| a.dead) || follower_of(w, ActorRef::Actor(x)).is_none() {
            continue;
        }
        let key = crate::math::to_int32(follower_key(w, x));
        let k = zone_k(w, id);
        let Some(z) = zone_of(w, id) else { return };
        let on = if z.chimney.is_some() {
            follower_touches(w, x, z.x, z.y, 0.0, CHIMNEY_R, CHIMNEY_HALF * k)
        } else {
            follower_touches(w, x, z.x, z.y, 0.0, z.radius * k, 0.0)
        };
        let at = z.inside.iter().position(|&v| v == key);
        let fog = z.fog_activated;
        match (on, at) {
            (true, None) => {
                if let Some(z) = zone_mut(w, id) {
                    z.inside.push(key);
                }
                if fog {
                    follower_apply(w, id, x);
                }
            }
            (false, Some(at)) => {
                if let Some(z) = zone_mut(w, id) {
                    z.inside.remove(at);
                }
                if fog {
                    follower_end_overlap(w, id, key);
                }
            }
            _ => {}
        }
    }
}

fn start_fog_buff(w: &mut World, id: ActorId) {
    let Some(z) = zone_of(w, id) else { return };
    for k in z.inside.clone() {
        if let Some(o) = w.by_id(k) {
            begin_overlap(w, id, o);
        }
    }
}

/// A minion destroyed while in the zone's lists (it leaves the World's fighters) leaves them, so its id is not met
/// again by a later minion's.
// Not ported: the destroyed pawn's end overlap (and its HandleGiantCrouch unbind) is not run; a Watermelon has no
// Comp_Pawn_Giant, and its buffs go with it.
fn drop_zone_gone(w: &mut World, id: ActorId) {
    // (a follower's key: `follower_overlaps`)
    retain_keys(w, id, |w, k| k < 0 || w.by_id(k).is_some());
}

/// The zone's `inside` and `buffed` kept to the keys `keep` passes, in place.
fn retain_keys(w: &mut World, id: ActorId, keep: impl Fn(&World, i32) -> bool) {
    let Some(z) = zone_mut(w, id) else { return };
    let (mut inside, mut buffed) = (std::mem::take(&mut z.inside), std::mem::take(&mut z.buffed));
    inside.retain(|&k| keep(w, k));
    buffed.retain(|&k| keep(w, k));
    if let Some(z) = zone_mut(w, id) {
        (z.inside, z.buffed) = (inside, buffed);
    }
}

fn end_fog_buff(w: &mut World, id: ActorId) {
    let Some(z) = zone_of(w, id) else { return };
    for k in z.buffed.clone() {
        end_overlap(w, id, k);
    }
}

fn fog_off(w: &mut World, id: ActorId) {
    if let Some(z) = zone_mut(w, id) {
        z.fog_activated = false;
    }
    if !crate::world::flow::reset_pass(w) {
        end_fog_buff(w, id); // (not in the Lab reset's pass: an old pawn there)
    }
    let Some(z) = zone_mut(w, id) else { return };
    z.looping = false;
    let (x, y) = (z.x, z.y);
    let Some(owner) = zone_owner(w, id) else { return };
    let fid = w.fighter(owner).id;
    let key = zone_key(w, id);
    w.events.push(Event::SoundStop {
        fighter: fid,
        key,
        fade: None,
        ev: Some(name!("sfx_cha_batman_smokebomb_loop_stop")),
        curve: None,
        cut: false,
    });
    w.events.push(sound(fid, name!("sfx_cha_batman_smokebomb_end"), x, y));
}

/// The zone's sphere against a fighter: his root capsule (Pawn) and his hurt shapes at the current pose (HurtBox),
/// in the play plane. False for an opponent the port's own spare switch spares what the zone's owner deals (no Fog).
fn touches(w: &mut World, o: FighterId, id: ActorId) -> bool {
    if w.fighter(o).life.dead > 0.0 || zone_owner(w, id).is_some_and(|f| crate::fighter::spared_by(w, f, o)) {
        return false;
    }
    let k = zone_k(w, id);
    let Some(z) = zone_of(w, id) else { return false };
    let (zx, zy, radius, chimney) = (z.x, z.y, z.radius, z.chimney.is_some());
    let (ox, cy) = (w.fighter(o).body.x, actor_y(w, o));
    let line = if chimney {
        let half = CHIMNEY_HALF * k;
        if (ox - zx).abs() > 600.0 || (cy - zy).abs() > half + 600.0 {
            return false;
        }
        Some(half)
    } else {
        if (ox - zx).abs() > radius * k + 600.0 || (cy - zy).abs() > radius * k + 600.0 {
            return false;
        }
        None
    };
    // (the World's capsules-in-sphere test for the one fighter)
    if !crate::fighter::no_collision(w, o) {
        let cr = cap_radius(w, o);
        let s = max(capsule_half(w, o) - cr, 0.0);
        let hit = match line {
            // The Chimney's line: the root capsule's segment against it, their gap within the sum of radii.
            Some(half) => {
                let gap = max(max(cy - s - (zy + half), 0.0), zy - half - (cy + s));
                size2d([ox - zx, gap]) <= CHIMNEY_R + cr
            }
            None => size2d([ox - zx, max(max(cy - s - zy, 0.0), zy - (cy + s))]) <= radius * k + cr,
        };
        if hit {
            return true;
        }
    }
    if crate::fighter::no_hit(w, o) {
        return false;
    }
    let Some(pose) = crate::combat::phases::victim_pose(w, o) else { return false };
    let shape = match line {
        // Unverified: PhysX's radius 0.1 for a capsule set to 0 is taken with the half height 225 kept whole; the 0.1
        // it may take off the segment's ends is not read.
        Some(half) => WorldSphere {
            x: zx,
            y: zy,
            z: Some(0.0),
            r: CHIMNEY_R,
            bx: Some([1e-6, 0.0, 0.0, 0.0, half, 0.0, 0.0, 0.0, 1e-6]),
            ..WorldSphere::default()
        },
        None => WorldSphere { x: zx, y: zy, z: Some(0.0), r: radius * k, ..WorldSphere::default() },
    };
    let hit = crate::combat::hit::hurt_hit(w, o, std::slice::from_ref(&shape), &pose).is_some();
    crate::combat::phases::give_vbuf(w, pose);
    hit
}

// BPI_EnvironmentalHazard (C019ScrubShield). What its Sphere overlaps: PlayerCollision (a capsule of radius and half
// height 225: a sphere) and PlayerAttachCollision (a sphere of 225), both of object type HitBox with Overlap to
// ProjectileDestructible, at the actor.
// Unverified: their collision taken as on for the zone's life.
// BPI Clean @0-15: Blocked(null), the parent's (no override): DestroyOnBlocked (true in the class defaults) ->
// Delay(0.0) -> DestroySpawnedActor(false) (SpawnedActorParent @18642 -> 3122): K2_DestroyActor at the head of the
// zone's next step, the same tick when the Clean came ahead of it (the projectiles' model; the zone's hit pause not
// read); Success = true.
fn clean_overlaps(w: &World, a: ActorRef, x: Real, y: Real, r: Real) -> bool {
    let ActorRef::Actor(id) = a else { return false };
    let Some(z) = zone_of(w, id) else { return false };
    let dead = w.actors.actors.get(id).is_none_or(|a| a.dead);
    !dead && size2d([x - z.x, y - z.y]) <= z.radius * zone_k(w, id) + r
}

fn clean(w: &mut World, a: ActorRef) -> bool {
    let ActorRef::Actor(id) = a else { return true };
    let Some(x) = w.actors.actors.get(id) else { return true };
    let (owner, cls) = (x.owner, x.cls);
    let data = w.data.clone();
    let on = actor_data(w, &data, ActorRef::Fighter(owner), cls).is_some_and(|d| flag(&d.p, "DestroyOnBlocked"));
    if on && let Some(z) = zone_mut(w, id) {
        z.blocked_destroy = Some(true);
    }
    true
}

fn zone_step(w: &mut World, id: ActorId) {
    let Some(a) = w.actors.actors.get(id) else { return };
    let (owner, cls) = (a.owner, a.cls);
    let data = w.data.clone();
    let p = actor_data(w, &data, ActorRef::Fighter(owner), cls).map(|d| &d.p);
    let doe = p.is_some_and(|p| flag(p, "DestroyOnExitedArena?"));
    let fog_duration = p.and_then(|p| num(p, "FogDuration")).unwrap_or(0.0);
    let Some(z) = zone_mut(w, id) else { return };
    let gone = z.blocked_destroy.is_some() || {
        let frames = z.age;
        z.age += 1.0;
        blast_timer_exit(doe, &mut z.blast_at, &z.blast_bounds, frames, z.x, z.y)
    };
    if gone {
        set_dead(w, id);
        return;
    }
    if w.pawns.is_some() {
        drop_zone_gone(w, id);
    }
    let all = all_list(w);
    for &o in &all {
        let on = touches(w, o, id);
        let oid = w.fighter(o).id;
        let Some(z) = zone_of(w, id) else { break };
        let (at, fog, buffed) = (z.inside.iter().position(|&v| v == oid), z.fog_activated, z.buffed.contains(&oid));
        match (on, at) {
            (true, None) => {
                if let Some(z) = zone_mut(w, id) {
                    z.inside.push(oid);
                }
                if fog {
                    begin_overlap(w, id, o); // @1986 (authority)
                }
            }
            (false, Some(at)) => {
                if let Some(z) = zone_mut(w, id) {
                    z.inside.remove(at);
                }
                if fog && buffed {
                    end_overlap(w, id, oid); // @4817-4887
                }
            }
            _ => {}
        }
    }
    put_fighters(w, all);
    follower_overlaps(w, id);
    // The latent actions, those started in an earlier tick first (a Delay made in a link counts from the next).
    let Some(z) = zone_mut(w, id) else { return };
    let count = |v: &mut Real| {
        if *v > 0.0 {
            *v -= 1.0;
            *v == 0.0
        } else {
            false
        }
    };
    let fog_end = count(&mut z.fog_end);
    let destroy = count(&mut z.destroy);
    let activate = count(&mut z.activate);
    let visuals = count(&mut z.visuals);
    if fog_end {
        fog_off(w, id);
    }
    if destroy {
        set_dead(w, id);
    }
    if activate {
        if let Some(z) = zone_mut(w, id) {
            z.fog_activated = true;
        }
        start_fog_buff(w, id);
        if let Some(z) = zone_mut(w, id) {
            z.fog_end = latent_frames(fog_duration);
        }
    }
    if visuals && let Some(z) = zone_mut(w, id) {
        z.destroy = latent_frames(0.5);
    }
}

fn zone_end(w: &mut World, id: ActorId) {
    let Some(z) = zone_of(w, id) else { return };
    if z.fog_activated {
        fog_off(w, id);
    } else if z.looping
        && let Some(owner) = zone_owner(w, id)
    {
        let fid = w.fighter(owner).id;
        let key = zone_key(w, id);
        w.events.push(Event::SoundStop { fighter: fid, key, fade: None, ev: None, curve: None, cut: false });
    }
    // A destroyed actor's binding is called no more (the broadcast skips it): dropped here.
    let all = all_list(w);
    for &o in &all {
        crate::chars::iron_giant::unbind_giant_crouch(w, id, o);
    }
    put_fighters(w, all);
}

fn zone_giant_crouch(w: &mut World, id: ActorId, f: FighterId, crouched: bool) {
    handle_giant_crouch(w, id, f, crouched);
}

/// The classes of the smoke bomb: its buffs (`buffs({...})`) and its two actors (`actors({...})`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use BuffClass as C;
    let no = ActorBuffHooks::default();
    let occlusion = BuffHooks {
        begin: Some(occlusion_begin),
        tick: Some(occlusion_tick),
        on: Some(occlusion_on),
        end: Some(occlusion_end),
        ..BuffHooks::default()
    };
    let follower = ActorBuffHooks {
        begin: Some(follower_begin),
        tick: Some(follower_tick),
        on: Some(follower_on),
        end: Some(follower_end),
        ..no
    };
    let list = [
        BuffEntry { actor: Some(follower), ..BuffEntry::new(C::BuffInvisibility, occlusion) },
        BuffEntry { actor: Some(follower), ..BuffEntry::new(C::BuffInvisibilityFog, occlusion) },
        BuffEntry::new(
            C::BuffInvulnerable,
            BuffHooks { begin: Some(invulnerable_begin), end: Some(invulnerable_end), ..BuffHooks::default() },
        ),
        BuffEntry::new(
            C::BuffSpeedUp,
            BuffHooks { begin: Some(speed_up_begin), tick: Some(speed_up_tick), ..BuffHooks::default() },
        ),
        // On a spawned actor (VelmaBookProjectile's own ally row): the BPI_BaseFighter cast fails @386 -> @553
        // (NotPlayerCharacter false), so the Delay(MaxLifetime) and the loop run, and each
        // MulticastDecrementEvadeCount's BPI_DecrementEvadeCount on the actor reaches no implementation: nothing but
        // its life (the spawn's life timer, or the actor's EndPlay).
        // Not ported: its own K2_DestroyActor after NumberOfDecrements x DecrementEvadeOnInterval (8 s) or
        // MaxLifetime (15 s); the one row that gives it to an actor has a BuffDuration of 8 s.
        BuffEntry {
            actor: Some(no),
            ..BuffEntry::new(
                C::BuffDecrementEvadeCount,
                BuffHooks { begin: Some(evade_begin), tick: Some(evade_tick), ..BuffHooks::default() },
            )
        },
        BuffEntry::new(C::BuffSmokeGrenadeProjectileBlock, BuffHooks::default()),
        BuffEntry::new(
            C::BuffPassiveApplyBuffonHit,
            BuffHooks {
                begin: Some(passive_begin),
                dealt_cond: Some(passive_dealt),
                on: Some(passive_on),
                ..BuffHooks::default()
            },
        ),
    ];
    for e in list {
        let n = e.class.registered_name().expect("a registered class");
        r.buffs.enter(n, e)?;
    }
    let grenade = WorldActorHooks {
        spawn: Some(grenade_spawn),
        step: Some(grenade_step),
        size: Some(SizeHooks { scale: Some(grenade_scale), ..SizeHooks::default() }),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        GRENADE,
        ActorEntry::new(ActorClass::World(WorldActorClass::SmokeGrenade), ActorHooks::World(grenade)),
    )?;
    let zone = WorldActorHooks {
        // its shapes read the root's scale; the fog is an emitter at a location (FogVisuals), not on the root
        size: Some(SizeHooks::default()),
        step: Some(zone_step),
        end: Some(zone_end),
        giant_crouch_changed: Some(zone_giant_crouch),
        ..WorldActorHooks::default()
    };
    r.actors.enter(
        ZONE,
        ActorEntry {
            hazard: Some(HazardHooks { overlaps: clean_overlaps, clean }),
            ..ActorEntry::new(ActorClass::World(WorldActorClass::SmokeGrenadeOcclusionBP), ActorHooks::World(zone))
        },
    )?;
    Ok(())
}
