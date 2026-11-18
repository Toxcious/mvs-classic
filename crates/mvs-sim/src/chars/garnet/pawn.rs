//! Pawn_Garnet's own logic: BeginPlay, BPI_Garnet (the idle and the flying
//! gauntlets, the marker's place, the electric charge), the conditions, the notifies (GauntletGlow, AimFlag,
//! SetAimFromMovement, ConsumeElectric), the Tick's aim and ground probes and the pawn hooks (tick, slot_input,
//! slot_mix, death). The aim itself is aim.rs's.
//!
//! BeginPlay @12806 (after the parent's): SingCollisionMaxScale = Sing Sphere's RelativeScale3D @12826 (sing.rs),
//! OnExitedArena bound to HandleExitedArena on authority @4854-4911, SkinApplied (the view), the Garnet_AnimBP cast
//! @13005-13122. Nothing of it is sim state here: mod.rs's match_reset made the CDO state. HandleExitedArena @19916 ->
//! @18814 (and EndPlay @19518, AbductPlayer @18804) writes MulticastElectricityStatus(false, false) and
//! FullChargedInput(false) (the view's: a widget), both charge flags false, destroys SingingVFX and stops
//! ElectricityTimeline (no state): [`exited_arena`], from the death hook (World.checkBlast runs OnExitedArena and the
//! death in one pass, with nothing between that reads these). AbductPlayer's is not reached: the world has no pawn hook
//! at an abduct (deviation).
//!
//! The BPI_Garnet functions other files call take the World: the port is the authority (HasAuthority true; a multicast
//! runs here).

use super::aim;
use super::dash::{charge_to_gauntlet_body, garnet_at_marker};
use super::hold::{set_is_holding_enemy_body, throw_fighters_body};
use super::shockwave::{ground_tick, set_ground_positions_body};
use super::state::{garnet, garnet_mut, garnet_of, garnet_w};
use crate::Real;
use crate::actors::ProjectileId;
use crate::events::NotifyRef;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::fighter::match_side::{notify_props, sfx};
use crate::name;
use crate::names::Name;
use crate::registry::props::flag;
use crate::registry::{CharHooks, Condition, NotifyHandler, Registry, RegistryError};
use crate::world::World;
use mvs_data::value::Props;

/// IsValid on an actor reference: held, and not destroyed (no Garnet class sets CanBePooled: every ReleaseToPool is a
/// K2_DestroyActor).
fn valid(w: &World, p: Option<ProjectileId>) -> bool {
    p.and_then(|id| w.actors.projectiles.get(id)).is_some_and(|p| !p.dead)
}

// ---------------------------------------------------------------- ReceiveTick @13597

/// The parent's tick runs first. The graph pushes 13641, 13705, 13946 and 14544 and goes to 7927; each part ends in a
/// pop, so the order is: the ground probes @7927 (shockwave.rs), the pointer arrow @14544 (the view's), the aim @13946,
/// the charging-to-marker aim @13705 (GetMarkerLocation @13715, SetAimDirection @13920), the heart's opacity @13641 (the
/// view's). Then AimResetTimeline's own tick (aim.rs reset_timeline_tick: its order against ReceiveTick is unverified
/// there).
fn tick(w: &mut World, f: FighterId) {
    ground_tick(w, f); // @7927 (shockwave.rs)
    aim::aim_tick(w, f); // @13946
    marker_aim_tick(w, f); // @13705
    aim::reset_timeline_tick(w, f);
}

/// The Tick's charging-to-marker aim @13705-13920: ChargingToMarker, GetMarkerLocation(false) @13715, then
/// SetAimDirection(Z of the normalized marker - self, false, true) @13920 (aim.rs marker_aim).
fn marker_aim_tick(w: &mut World, f: FighterId) {
    if !garnet(w, f).charging_to_marker {
        return;
    }
    let (ok, x, y) = get_marker_location(w, f, false);
    if ok {
        aim::marker_aim(w, f, x, y);
    }
}

/// `slotMix` (Garnet_AnimBP's slot graph from the data: `fighter::anim::slot_mix::graph_slots`): the mix it writes, which
/// the shapes and the drawn pose read.
fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&garnet(w, f).pawn.slot_mix)
}

/// `slotMix`, written.
fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    garnet_mut(w, f).map(|s| &mut s.pawn.slot_mix)
}

/// `death`: the exit's flags, and the port does not update a dead pawn's AnimBP (its mix is emptied).
fn death(w: &mut World, f: FighterId) {
    exited_arena(w, f);
    garnet_w(w, f).pawn.slot_mix.slot_mix.clear();
}

/// The hooks of this file in Garnet's CharHooks (the pawn hooks).
pub fn hooks(h: &mut CharHooks) {
    h.tick = Some(tick);
    h.slot_input = Some(aim::slot_input);
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.death = Some(death);
}

// ---------------------------------------------------------------- BPI_Garnet
// BPI_Garnet (an interface, empty in BPI_Garnet; implemented by the pawn), called by the other files on a fighter whose
// cast to BPI_Garnet succeeds (`garnet_of` is `Some`).

/// BPI_SetAimDirection(Direction, Reset, Fast) @9777 (SetAimDirection, aim.rs): GarnetGauntletsProjectile's
/// MulticastVelocity @4777 (gauntlets.rs), the Tick's charging-to-marker aim @13920, ChargeToGauntlet(false)'s reset
/// (dash.rs).
pub fn set_aim_direction(w: &mut World, f: FighterId, direction: Real, reset: bool, fast: bool) {
    aim::set_aim_direction(w, f, direction, reset, fast);
}

/// BPI_SetIdleGauntlet(Gauntlet) @4054: IdleGauntlet = Gauntlet, then on authority MulticastSetIdleGauntlets(Gauntlet)
/// @12533: both the incoming one and IdleGauntlet valid (@12533-12605) -> MarkerLocationBeforeDestroy = IdleGauntlet's
/// location @12619-12669 (IdleGauntlet is the incoming one already: its own location); IdleGauntlet = it @12696.
/// Caller: GarnetMarker2's BeginPlay (marker.rs).
pub fn set_idle_gauntlet(w: &mut World, f: FighterId, gauntlet: Option<ProjectileId>) {
    garnet_w(w, f).idle_gauntlet = gauntlet; // @4054
    multicast_set_idle_gauntlets(w, f, gauntlet); // @4103
}

fn multicast_set_idle_gauntlets(w: &mut World, f: FighterId, gauntlet: Option<ProjectileId>) {
    let idle = garnet(w, f).idle_gauntlet;
    if valid(w, gauntlet) && valid(w, idle) {
        let m = w.actors.projectiles.get(idle.expect("valid")).expect("valid");
        let (x, y) = (m.x, m.y);
        let p = &mut garnet_w(w, f).pawn;
        p.marker_location_before_destroy_x = x;
        p.marker_location_before_destroy_y = y;
    }
    garnet_w(w, f).idle_gauntlet = gauntlet; // @12696
}

/// BPI_AreGauntletsCast (out Cast: IsValid(IdleGauntlet)): GarnetHasGauntletsCast / GarnetNoGauntletsCast, the dash
/// (dash.rs).
pub fn are_gauntlets_cast(w: &World, f: FighterId) -> bool {
    valid(w, garnet(w, f).idle_gauntlet)
}

/// BPI_SetProjectileGauntlets(Gauntlets) @9582 -> @3894 (authority) -> MulticastSetProjectileGauntlet @15214:
/// ProjectileGauntlets = Gauntlets. Caller: GarnetGauntletsProjectile (gauntlets.rs).
pub fn set_projectile_gauntlets(w: &mut World, f: FighterId, gauntlets: Option<ProjectileId>) {
    garnet_w(w, f).projectile_gauntlets = gauntlets;
}

/// BPI_GarnetHasGauntletProjectileOut (out ProjectileOut: IsValid(ProjectileGauntlets)): GarnetGauntletProjectileOut.
pub fn garnet_has_gauntlet_projectile_out(w: &World, f: FighterId) -> bool {
    valid(w, garnet(w, f).projectile_gauntlets)
}

/// BPI_ChargeToGauntlet(Charging) @6469-7764: AnimNotify_GarnetMoveToGauntlets (dash.rs); its body is dash.rs's, on the
/// shared Buff_SafeMoveToPosition: this forwards to it.
pub fn charge_to_gauntlet(w: &mut World, f: FighterId, charging: bool) {
    charge_to_gauntlet_body(w, f, charging);
}

/// BPI_IsMovingToMarker (out IsMoving: ChargingToMarker): GarnetAtMarker.
pub fn is_moving_to_marker(w: &World, f: FighterId) -> bool {
    garnet(w, f).charging_to_marker
}

/// BPI_SetIsHoldingEnemy(True) @12061-12230: Buff_HeldByEnemyGarnet's Begin @934 / End (hold.rs).
pub fn set_is_holding_enemy(w: &mut World, f: FighterId, holding: bool) {
    set_is_holding_enemy_body(w, f, holding);
}

/// BPI_ThrowFighters(HitStruct, Drop) @11876 -> @5477-6279: AnimNotify_GarnetThrowFighters (hold.rs; its body, ThrowFighter
/// and the pfgDoDamage, is hold.rs's).
pub fn throw_fighters(w: &mut World, f: FighterId, hit: &Props, drop: bool) {
    throw_fighters_body(w, f, hit, drop);
}

/// BPI_SetGroundPositions @7917 -> @4316 (FindFloor, the arrays cleared, both probes on): AnimNotify_GarnetStartGroundChecking
/// (the body is shockwave.rs's).
pub fn set_ground_positions(w: &mut World, f: FighterId) {
    set_ground_positions_body(w, f);
}

/// BPI_ResetGauntlets @4128 (authority): MulticastSetIdleGauntlets(None) @4158 -> IdleGauntlet = None (no location
/// saved: the incoming one is not valid). It neither destroys the marker nor waits. Callers: GarnetMarker2's EndPlay
/// @1926 (marker.rs), GarnetGauntletsIdle (dormant).
pub fn reset_gauntlets(w: &mut World, f: FighterId) {
    multicast_set_idle_gauntlets(w, f, None);
}

/// BPI_ConsumeElectricity @9772 -> @6424 (authority) -> ConsumeElectricityServer @15981: ElectricityTimeline.Stop @8592
/// (never played in the live kit: no state), PF_Post_Event(sfx_cha_garnet_attack_special_u_charge_loop_stop) on herself
/// @15991, then @15902-15955: ChargingElectricity and ElectricityFullyCharged false, MulticastElectricityStatus (false,
/// false) (the charge VFX, the view's: it reads the two flags), FullChargedInput(false) (the owning client's UI),
/// ElectricityFullChargedTimer cleared (never set in the live kit). Callers: AnimNotify_GarnetConsumeElectric (the
/// cooldown and charged montages of the up and down specials), the up special (zone.rs).
pub fn consume_electricity(w: &mut World, f: FighterId) {
    sfx(w, f, Some(name!("sfx_cha_garnet_attack_special_u_charge_loop_stop"))); // @15991
    let z = &mut garnet_w(w, f).zone;
    z.charging_electricity = false; // @15902
    z.electricity_fully_charged = false; // @15913
}

/// HandleExitedArena @19916 -> @18814-18943 (the header's note): the charge flags and the singing VFX (sing.rs's flag)
/// off.
fn exited_arena(w: &mut World, f: FighterId) {
    let s = garnet_w(w, f);
    s.zone.charging_electricity = false; // @18845
    s.zone.electricity_fully_charged = false; // @18856
    s.sing.singing_vfx = false; // @18910 SingingVFX.K2_DestroyComponent
}

/// BPI_GetMarkerLocation(GetPositionBeforeWarp; out Success, Location) @0-231: GetPositionBeforeWarp -> (true,
/// LocationBeforeMove) @14-25; else a valid IdleGauntlet -> (true, its location) @100-161; else (false,
/// LocationBeforeMove) @193-204. Callers: the Tick's charging-to-marker aim @13715, the dash (dash.rs). Returns (Success,
/// x, y) (sim).
pub fn get_marker_location(w: &World, f: FighterId, before_warp: bool) -> (bool, Real, Real) {
    let s = garnet(w, f);
    if !before_warp
        && valid(w, s.idle_gauntlet)
        && let Some(m) = s.idle_gauntlet.and_then(|id| w.actors.projectiles.get(id))
    {
        return (true, m.x, m.y);
    }
    (before_warp, s.location_before_move_x, s.location_before_move_y)
}

/// BPI_HasAllyMarked (out Ally Marked) @0-149: a valid IdleGauntlet whose GetAttachParentActor is valid @43-114 (no team
/// or fighter test); else false @138. Its one caller: GarnetMarkerAttachedToAlly (below). The attach parent is marker.rs's
/// GarnetMarker2 field (absent on an IdleGauntlet of another class).
pub fn has_ally_marked(w: &World, f: FighterId) -> bool {
    let Some(id) = garnet(w, f).idle_gauntlet else { return false };
    match w.actors.projectiles.get(id) {
        Some(p) if !p.dead => marker_has_attach_parent(p),
        _ => false,
    }
}

/// `(attachParent ?? null) !== null`: a GarnetMarker2's attach parent; an IdleGauntlet of another class has none.
fn marker_has_attach_parent(p: &crate::actors::projectile::Projectile) -> bool {
    match &p.ext {
        crate::actors::projectile::ProjectileExt::GarnetMarker2(m) => {
            crate::chars::garnet::marker::has_attach_parent(m)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------- the conditions
// Each a Condition2 child whose Check(owner) reads no variable of the object (Condition2's empty Reset). An owner that is
// not a Garnet fails DoesImplementInterface / the cast to BPI_Garnet.
// - GarnetHasGauntletsCast @0-233: BPI_AreGauntletsCast; not a Garnet -> false @201.
// - GarnetNoGauntletsCast (its child) @0-77: Not(the parent's Check): not a Garnet -> true.
// - GarnetGauntletProjectileOut @0-233: BPI_GarnetHasGauntletProjectileOut; not a Garnet -> false.
// - GarnetAtMarker @0-506: BPI_IsMovingToMarker @79, then Not(BPI_CheckIfHasBuff(Buff_MoveToPosition_C)) @218-400 (a child
//   class counts: her Buff_SafeMoveToPosition is the force move of the ally route, `team.ally.buff > 0`); not moving or
//   not a Garnet -> false.
// - GarnetIsNotSinging @0-262: Not(BPI_IsSinging), which is literal false (Pawn_Garnet BPI_IsSinging @0): a Garnet ->
//   true, not a Garnet -> false @230.
// - GarnetAttackBuffReady @0-291: Ready AND AttackBuff of BPI_IsSingReady, literal (false, false) (@0-11): false.
// - GarnetDefenseBuffReady @0-320: Ready AND Not(AttackBuff): false.
// - GarnetSingBuffNotReady @0-282: Not(Ready): a Garnet -> true, not a Garnet -> false @239.
// - GarnetMarkerAttachedToAlly @0-233: BPI_HasAllyMarked; not a Garnet -> false @201 (MoveToMarker_Montage2's
//   ConditionSectionJump to ThrowAlly, f0.13).
// (the neutral special's release always takes the Cancel row.)

fn has_gauntlets_cast(w: &mut World, f: FighterId) -> bool {
    garnet_of(w, f).is_some() && are_gauntlets_cast(w, f)
}

fn no_gauntlets_cast(w: &mut World, f: FighterId) -> bool {
    !has_gauntlets_cast(w, f)
}

fn gauntlet_projectile_out(w: &mut World, f: FighterId) -> bool {
    garnet_of(w, f).is_some() && garnet_has_gauntlet_projectile_out(w, f)
}

fn at_marker(w: &mut World, f: FighterId) -> bool {
    garnet_of(w, f).is_some() && garnet_at_marker(w, f)
}

fn is_garnet(w: &mut World, f: FighterId) -> bool {
    garnet_of(w, f).is_some()
}

fn never(_w: &mut World, _f: FighterId) -> bool {
    false
}

fn marker_attached_to_ally(w: &mut World, f: FighterId) -> bool {
    garnet_of(w, f).is_some() && has_ally_marked(w, f)
}

// ---------------------------------------------------------------- the notifies
// AnimNotify_Garnet*: MeshComp's owner cast to BPI_Garnet (a Disguise's copy of her montage plays on the thief: the cast
// fails, nothing).
// - AnimNotify_GarnetAimFlag: Begin BPI_SetGarnetAimFlag(true) @121, End (false) @121 (Special_U_Montage3).
// - AnimNotify_GarnetSetAimFromMovement: Begin BPI_SetReadAimFromMovement(true, Is Horizontal Movement) @215 (the notify's
//   field, CDO false; Air_D's true), End (false, false) @215 (MoveToMarker3, Air_MoveToMarker).
// - AnimNotify_GarnetConsumeElectric: Begin BPI_ConsumeElectricity @215; no End (the six cooldown and charged montages of
//   the up and down specials).
// - AnimNotify_GarnetGauntletGlow (Garnet_Emote_DefaultTaunt_Montage): Begin casts the owner to BPI_CharacterViewer @42
//   (the menu's viewer actor, never a fighter) for BPI_GarnetGauntletFadeOut @121 and returns false. In a match it does
//   nothing.

fn aim_flag_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if garnet_of(w, f).is_some() {
        aim::set_garnet_aim_flag(w, f, true);
    }
}

fn aim_flag_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if garnet_of(w, f).is_some() {
        aim::set_garnet_aim_flag(w, f, false);
    }
}

fn aim_from_movement_begin(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    if garnet_of(w, f).is_some() {
        let horizontal = flag(notify_props(w, f, n), "Is Horizontal Movement");
        aim::set_read_aim_from_movement(w, f, true, horizontal);
    }
}

fn aim_from_movement_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if garnet_of(w, f).is_some() {
        aim::set_read_aim_from_movement(w, f, false, false);
    }
}

fn consume_electric_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if garnet_of(w, f).is_some() {
        consume_electricity(w, f);
    }
}

/// This file's registry entries (pawn.rs's conditions and notifies).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let conditions: [(&str, crate::registry::entries::CondFn); 9] = [
        ("GarnetHasGauntletsCast_C", has_gauntlets_cast),
        ("GarnetNoGauntletsCast_C", no_gauntlets_cast),
        ("GarnetGauntletProjectileOut_C", gauntlet_projectile_out),
        ("GarnetAtMarker_C", at_marker),
        ("GarnetIsNotSinging_C", is_garnet),
        ("GarnetAttackBuffReady_C", never),
        ("GarnetDefenseBuffReady_C", never),
        ("GarnetSingBuffNotReady_C", is_garnet),
        ("GarnetMarkerAttachedToAlly_C", marker_attached_to_ally),
    ];
    for (n, c) in conditions {
        r.conditions.enter(n, Condition::Fn(c))?;
    }
    let notifies: [(&str, NotifyHandler); 4] = [
        ("AnimNotify_GarnetGauntletGlow_C", NotifyHandler::default()),
        (
            "AnimNotify_GarnetAimFlag_C",
            NotifyHandler { begin: Some(aim_flag_begin), end: Some(aim_flag_end), ..NotifyHandler::default() },
        ),
        (
            "AnimNotify_GarnetSetAimFromMovement_C",
            NotifyHandler {
                begin: Some(aim_from_movement_begin),
                end: Some(aim_from_movement_end),
                ..NotifyHandler::default()
            },
        ),
        (
            "AnimNotify_GarnetConsumeElectric_C",
            NotifyHandler { begin: Some(consume_electric_begin), ..NotifyHandler::default() },
        ),
    ];
    for (n, h) in notifies {
        r.notifies.enter(n, h)?;
    }
    Ok(())
}

#[allow(dead_code, reason = "names kept for the declarations above")]
const _N: Option<Name> = None;
