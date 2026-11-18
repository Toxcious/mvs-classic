//! Velma's capture: Buff_VelmaAttachToAlly ([`CaptureExt`], a child of
//! Buff_AttachToAlly: `combat::attach_ally`, with its AdjustHeldLocation and BindInput overrides; an ally carried on
//! her back, an enemy held by the carry run) and VelmaNotHoldingFighterCondition; joined to worldTick (the held
//! fighters follow her) and knockedBack (a held ally lets go). Its export `start_capture` is called by carry.rs.
//!
//! Buff_VelmaAttachToAlly (offsets of its ubergraph; latent resumes 4325 -> 954, 5411 -> 1019, 5664 -> 15), a
//! Buff_AttachToAlly child (`combat::attach_ally`: the parent's graph). Class data: BuffDuration 5, Stun 10000 (no
//! struggle), StateInfo Group 12 (BlockerGroups [12], BlockerBuffs [Buff_StevenBubbleEnemy_C], BreakGroups [13]),
//! AttachSocket Spine_Socket, BreakOutDelay 0.5, AttachActorOffsets Pawn_Jake -50, Pawn_C015 -50, Pawn_Superman 100,
//! Pawn_TomAndJerry -40 (UserConstructionScript's AttachOffsetCharList). VelmaCarryRun_Actor spawns it on the enemy it
//! runs into (@17466, [`start_capture`]).
//! - ReceiveBeginPlay @6354 -> @5895: the parent's (its InitiateGrapple, then this class's AdjustHeldLocation and
//!   BindInput overrides, below). Then the owner's main mesh playing Velma_Special_Pickup_HoldPose_Montage (a Velma
//!   carrying): (authority) the Instigator's JumpToSectionServer(Fall, HoldPose) @6258 and K2_DestroyActor @6323. Else
//!   @5719: the owner's BPI_SetBeingCarried(true, true) (Base_AnimBP's carried pose), then @4675: the
//!   owner's BPI_SetFacingLogic(the Instigator's BPI_IsFacingLeft) @4924; the Instigator's FacingChanged ->
//!   OnFacingChanged @5088; (authority) HeldBaseFighter = the owner @5254, its BPI_SetHitRotaion(false) @5352;
//!   @3858: the Instigator's and the owner's DamageManager.KnockedBack -> KnockedBack (@3976, @4133);
//!   HoldVisualMulticast(null) @4174 (the owner's Montage_Stop(0.0, null) @5625); the owner of the Instigator's team:
//!   Delay(BreakOutDelay) @4325 -> @954 HeldBaseFighter.InputPreTriggered -> ActivatedLaunch @977. Then (both teams)
//!   Delay(0.5) @5411 -> @1019: the Instigator's main mesh not playing HoldPose -> K2_DestroyActor @1288.
//! - ActivatedLaunch -> ActivatedLaunchServer -> @6359: InputType 0, 1, 2 or 3 -> K2_DestroyActor @6607.
//! - KnockedBack -> @6622: the owner of the Instigator's team -> K2_DestroyActor @6902 (an enemy's stays).
//! - OnFacingChanged(FacingLeft) -> Delay(0.033) @5664 -> @15: HeldBaseFighter.BPI_SetFacingLogic(the event's
//!   FacingLeft, the latest one's) @129.
//! - The parent's ReceiveTick @1839: the owner in hitstun or its active montage the parent's CarriedMontage (null: no
//!   montage) -> nothing; else HoldVisualMulticast(null): Montage_Stop(0.0, null) on the owner, every tick.
//! - AdjustHeldLocation (override): the owner's K2_SetActorLocation(the Instigator's Mesh.GetSocketLocation(
//!   AttachSocket) + (0, (Instigator.FacingLeft ? 1 : -1) x (2 x R + 30), 2 x H + AttachOffsetCharList[the owner's
//!   class] or 0), no sweep, teleport) @1026, R / H: the owner's BaseFighter Capsule (innerCapRadius / innerCapHalf:
//!   42 / 96), unscaled.
//! - BindInput (override) @4384: the owner of the Instigator's team -> the parent's BindInput @4664 (InputTriggered).
//! - ReceiveEndPlay @1303: the parent's; the owner's BPI_SetBeingCarried(false, true) @1421; the FacingChanged
//!   bindings removed; then, both BaseFighters: of other teams @1873 -> the release place below, of one team @3616 ->
//!   (authority) the owner's LaunchCharacter((0, 0, 2500), false, true) @3685 and PF_Post_Event(
//!   sfx_cha_velma_holdpose_release, the owner) @3756. The release place (@2013-3611, @176-925): H = the owner's
//!   unscaled root capsule half height (`raw_half`), R = its Capsule's scaled radius; CapsuleTraceSingle(the
//!   Instigator's location -> the owner's, R, H, channel 5, ignoring both) @2476; PreCheck = its hit's Location, else
//!   the owner's @2939; CapsuleOverlapActors(PreCheck, R, H, [WorldStatic, DamageDestructible], ignoring both) @3113.
//!   Either one: LineTraceSingle(the Instigator's location -> + (0, 0, H + 25), channel 5, ignoring both) @3475, then
//!   K2_SetActorLocation(that line hit ? the Instigator's location + (0, 0, her own half height - (H + 25)) : the
//!   overlap ? the Instigator's Capsule location + (0, 0, H) : PreCheck, no sweep, teleport) @807 and
//!   K2_SetActorRotation(0) @869; neither: the rotation alone.
//! - VelmaNotHoldingFighterCondition: BPI_GetFighterCharacter == 19 (another fighter: false), then Not(IsHoldingEnemy
//!   or IsHoldingAlly). Velma_Special_Pickup_HoldPose_Montage's branch windows read it.
//!
//! Buff_Flatten is shared code (`combat::flatten`); LeBron's slams give it too.
// Deviation (as `combat::attach_ally`'s): the owner is put at its held place again after the fighters' moves of every
// frame (`attach_follow` from this file's `world_tick`), kept where AdjustHeldLocation put it relative to the
// Instigator's root and turned with her facing.
// Deviation: the FacingChanged binding is a poll of the Instigator's facing at the buff's tick (the port has no facing
// signal); the owner's K2_SetActorRotation(0) at the release is not ported (the port's fighter has no actor rotation
// off its facing).
// Deviation: the release's K2_SetActorLocation without a sweep is resolved out of the walking boxes at once
// (`teleport_move`).

use super::state::velma_of;
use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::combat::attach_ally::{self, AttachOverrides, AttachState};
use crate::combat::buffs::{buff, buff_mut, end_buff, spawn_buff};
use crate::combat::{BuffClass, BuffExt, BuffRef, HitPacket};
use crate::fighter::anim::MontageSlot;
use crate::fighter::anim::grouped::{active_instance, stop_grouped};
use crate::fighter::anim::montages::{montage_is_active, stop_dash_montage, stop_montage};
use crate::fighter::body::def;
use crate::fighter::common::{buff_def, latent_frames, peers, warn_text};
use crate::fighter::match_side::sfx;
use crate::fighter::moves::{Impulse, nav_jump, recall_move, teleport_move};
use crate::fighter::traces::{
    OBJ_DAMAGE_DESTRUCTIBLE, OBJ_WORLD_STATIC, TRACE_PROJECTILE_GROUND_NO_PLATFORM, capsule_overlap_actors,
    capsule_trace_single, sphere_trace_single,
};
use crate::fighter::{FighterId, actor_y, facing, in_hitstun, mesh_wx, mesh_wy, set_facing};
use crate::name;
use crate::names::Name;
use crate::registry::entries::BuffHooks;
use crate::registry::props::{num, obj, text};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{BuffEntry, Condition, Registry, RegistryError};
use crate::world::World;

pub const ATTACH_TO_ALLY: &str = "Buff_VelmaAttachToAlly_C";
/// [`ATTACH_TO_ALLY`], as a name.
pub const ATTACH_TO_ALLY_N: Name = name!("Buff_VelmaAttachToAlly_C");
const HOLD_POSE: Name = name!("Velma_Special_Pickup_HoldPose_Montage");
const SFX_RELEASE: Name = name!("sfx_cha_velma_holdpose_release");

/// Buff_VelmaAttachToAlly's extension: the parent's variables (`combat::attach_ally`), then its own
/// (`CaptureExt`: `{ ins, held, faceIn, launchIn, checkIn, launchBound, faceLeft, insFacing }`).
#[derive(State, Debug, Default)]
pub struct CaptureExt {
    /// The parent's.
    pub attach: AttachState,
    /// The Instigator (the parent's `by` is cleared by its EndPlay, ahead of this class's).
    pub ins: Option<FighterId>,
    /// HeldBaseFighter.
    pub held: Option<FighterId>,
    /// Delay(BreakOutDelay) -> the ActivatedLaunch binding: frames left, 0 none.
    pub launch_in: Real,
    /// ActivatedLaunch bound to HeldBaseFighter.InputPreTriggered.
    pub launch_bound: bool,
    /// Delay(0.5) -> the HoldPose check @1019: frames left, 0 none.
    pub check_in: Real,
    /// Delay(0.033) of OnFacingChanged: frames left, 0 none.
    pub face_in: Real,
    /// The last OnFacingChanged's FacingLeft.
    pub face_left: bool,
    /// The Instigator's facing at the last poll (the FacingChanged stand-in).
    pub ins_facing: Real,
}

fn ext(w: &World, b: BuffRef) -> Option<&CaptureExt> {
    match &buff(w, b)?.ext {
        BuffExt::VelmaAttachToAlly(e) => Some(e),
        _ => None,
    }
}

fn ext_mut(w: &mut World, b: BuffRef) -> Option<&mut CaptureExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::VelmaAttachToAlly(e) => Some(e),
        _ => None,
    }
}

/// `sameTeam`.
fn same_team(w: &World, f: FighterId, by: Option<FighterId>) -> bool {
    by.is_some_and(|b| w.fighter(f).team.team == w.fighter(b).team.team)
}

/// Mesh.GetSocketLocation(AttachSocket) on the Instigator: sim x, y.
fn socket_at(w: &World, by: FighterId, sock: &str) -> (Real, Real) {
    let ch = w.fighter(by).ch;
    let index = crate::chars::iron_giant::actor_state::socket_index(w, by, sock);
    let pose = index.and_then(|_| crate::fighter::anim::sampled_boxes(w, by));
    let (Some(i), Some(pose)) = (index, pose) else {
        let msg = format!("{ATTACH_TO_ALLY}: no socket track {sock} on {}", w.names.str(ch));
        warn_text(w, &msg);
        let mesh_z = crate::combat::hits::char_data(w, w.fighter(by)).boxes.mesh_z;
        return (mesh_wx(w, by), mesh_wy(w, by) + crate::math::narrow(mesh_z));
    };
    let o = w.fixed.samplers.get(&ch).and_then(|b| b.offsets.get(usize::try_from(i).expect("a box index")).copied());
    let o = usize::try_from(o.expect("a socket offset")).expect("a box offset");
    let at = (mesh_wx(w, by) + crate::real(pose[o]) * facing(w, by), mesh_wy(w, by) + crate::real(pose[o + 1]));
    crate::fighter::anim::put_pose(w, pose);
    at
}

/// AttachOffsetCharList[GetObjectClass(owner)] (Map_Find; 0 when absent): the entries' pawn classes and the port's
/// keys.
fn char_offset(w: &World, f: FighterId) -> Real {
    let Some(d) = buff_def(w, ActorRef::Fighter(f), ATTACH_TO_ALLY_N) else { return 0.0 };
    let key = w.names.str(w.fighter(f).ch);
    for e in crate::registry::props::arr(d, "AttachActorOffsets") {
        let Some(e) = e.as_obj() else { continue };
        let cls = obj(e, "Actor").and_then(|a| text(a, "AssetPathName")).unwrap_or("");
        let cls = cls.rsplit('.').next().unwrap_or("");
        let k = match cls {
            "Pawn_Jake_C" => "jake",
            "Pawn_C015_C" => "taz",
            "Pawn_Superman_C" => "superman",
            "Pawn_TomAndJerry_C" => "tomandjerry",
            other => {
                warn_text(w, &format!("{ATTACH_TO_ALLY}: AttachActorOffsets names {other}, which no port key has"));
                continue;
            }
        };
        if k == key {
            return num(e, "Float").unwrap_or(0.0);
        }
    }
    0.0
}

/// AdjustHeldLocation (override).
fn adjust(w: &mut World, f: FighterId, by: FighterId) {
    let sock = buff_def(w, ActorRef::Fighter(f), ATTACH_TO_ALLY_N)
        .and_then(|d| text(d, "AttachSocket"))
        .unwrap_or("None")
        .to_owned();
    let (sx, sy) = socket_at(w, by, &sock);
    let dd = def(w, f);
    let (radius, half) = (dd.inner_cap_radius, dd.inner_cap_half);
    // (FacingLeft ? 1 : -1) x (2R + 30) on UE Y, sim x = -UE Y: on the Instigator's facing side.
    let side = (crate::math::narrow(radius * 2.0) + 30.0) * if facing(w, by) < 0.0 { 1.0 } else { -1.0 };
    let up = crate::math::narrow(half * 2.0) + char_offset(w, f);
    recall_move(w, f, sx - side, sy + up);
}

/// Montage_Stop(0.0, null) on the owner: every montage, no blend.
fn stop_all(w: &mut World, f: FighterId) {
    stop_montage(w, f, Some(0.0), None);
    stop_dash_montage(w, f, 0.0);
    stop_grouped(w, f, None, 0.0);
}

/// `startCapture` (VelmaCarryRun_Actor @17466): Buff_VelmaAttachToAlly spawned on `victim` by `f`; the buff, `None`
/// when none lives. Called by carry.rs.
pub fn start_capture(w: &mut World, f: FighterId, victim: FighterId, _run: ActorRef) -> Option<BuffRef> {
    let b = spawn_buff(w, victim, ATTACH_TO_ALLY_N, 0.0, Some(0), Some(f), true, None)?;
    buff(w, b).filter(|x| x.frames > 0.0).map(|_| b)
}

/// capture.rs's part of the joined worldTick: the fighters she holds follow her (`attach_ally::attach_follow`), after
/// the fighters' moves.
pub fn world_tick(w: &mut World, f: FighterId) {
    attach_ally::attach_follow(w, f, &[ATTACH_TO_ALLY_N]);
}

/// capture.rs's part of the joined knockedBack (DamageManager.KnockedBack -> @6622): a held ally lets go.
pub fn knocked_back(w: &mut World, f: FighterId, _x: Real, _y: Real, _z: Real, _by: Option<ActorRef>) {
    let team = w.fighter(f).team.team;
    for o in peers(w).to_vec() {
        if o == f || w.fighter(o).team.team != team {
            continue;
        }
        let refs: Vec<BuffRef> = w
            .fighter(o)
            .buffs
            .buffs
            .iter()
            .filter(|b| b.cls == ATTACH_TO_ALLY_N && b.frames > 0.0)
            .map(|b| BuffRef { fighter: o, serial: b.serial })
            .collect();
        for b in refs {
            let held_by_f = ext(w, b).is_some_and(|e| e.ins == Some(f));
            if held_by_f {
                end_buff(w, o, b);
            }
        }
    }
}

/// The buff's BeginPlay: the parent's, then this class's @6354.
fn begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::VelmaAttachToAlly(CaptureExt::default());
    }
    let attach = attach_ally::begin(
        w,
        f,
        from,
        AttachOverrides { adjust: Some(adjust), bind_input: Some(|w, f, by| same_team(w, f, by)) },
    );
    let ins_facing = from.map_or(0.0, |s| facing(w, s));
    if let Some(e) = ext_mut(w, b) {
        e.attach = attach;
        e.ins = from;
        e.held = None;
        e.launch_in = 0.0;
        e.check_in = 0.0;
        e.face_in = 0.0;
        e.launch_bound = false;
        e.face_left = false;
        e.ins_facing = ins_facing;
    }
    // @6092: the owner a Velma in her HoldPose.
    if montage_is_active(w, f, HOLD_POSE) {
        if let Some(from) = from {
            // JumpToSectionServer(Fall, HoldPose) on the Instigator's MontageNavigation (`nav_jump`'s rule): that
            // montage while it plays, else the active one.
            let plays = w
                .fighter(from)
                .montages
                .montage
                .as_ref()
                .is_some_and(|m| m.montage.name == HOLD_POSE && !m.ended && !m.terminated);
            let m = if plays { Some(MontageSlot::Main) } else { active_instance(w, from) };
            if let Some(m) = m {
                nav_jump(w, from, m, name!("Fall"));
            }
        }
        end_buff(w, f, b);
        return;
    }
    crate::fighter::anim::set_being_carried(w, f, true, true); // @5719: Base_AnimBP's carried pose
    if let Some(from) = from {
        let v = facing(w, from);
        set_facing(w, f, v); // BPI_SetFacingLogic(the Instigator's FacingLeft) @4924
    }
    if let Some(e) = ext_mut(w, b) {
        e.held = Some(f); // @5254
    }
    w.fighter_mut(f).react.hit_rotation_off = Some(true); // BPI_SetHitRotaion(false) @5352
    stop_all(w, f); // HoldVisualMulticast(null) @4174
    if same_team(w, f, from) {
        let delay = buff_def(w, ActorRef::Fighter(f), ATTACH_TO_ALLY_N).and_then(|d| num(d, "BreakOutDelay"));
        let n = latent_frames(delay.unwrap_or(0.0));
        if let Some(e) = ext_mut(w, b) {
            e.launch_in = n; // @4325
        }
    }
    if let Some(e) = ext_mut(w, b) {
        e.check_in = latent_frames(0.5); // @5411
    }
}

/// The parent's ReceiveTick, then the latent actions (AActor::Tick: ReceiveTick ahead of ProcessLatentActions).
fn tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    if !(in_hitstun(w, f) || active_instance(w, f).is_none()) {
        stop_all(w, f);
    }
    let Some(by) = ext(w, b).and_then(|e| e.ins) else {
        return tick_latent(w, b, None);
    };
    // FacingChanged (polled): OnFacingChanged(FacingLeft) -> Delay(0.033), a running Delay left as it is.
    let by_facing = facing(w, by);
    if let Some(e) = ext_mut(w, b)
        && by_facing != e.ins_facing
    {
        e.ins_facing = by_facing;
        e.face_left = by_facing < 0.0;
        if e.face_in == 0.0 {
            e.face_in = latent_frames(0.033);
        }
    }
    tick_latent(w, b, Some(by))
}

/// The buff's three countdowns.
fn tick_latent(w: &mut World, b: BuffRef, by: Option<FighterId>) -> bool {
    let mut face = None;
    let mut check = false;
    if let Some(e) = ext_mut(w, b) {
        if e.face_in > 0.0 {
            e.face_in -= 1.0;
            if e.face_in == 0.0
                && let Some(h) = e.held
            {
                face = Some((h, if e.face_left { -1.0 } else { 1.0 })); // @129
            }
        }
        if e.launch_in > 0.0 {
            e.launch_in -= 1.0;
            if e.launch_in == 0.0 {
                e.launch_bound = true; // @977
            }
        }
        if e.check_in > 0.0 {
            e.check_in -= 1.0;
            check = e.check_in == 0.0;
        }
    }
    if let Some((h, v)) = face {
        set_facing(w, h, v);
    }
    check && !by.is_some_and(|by| montage_is_active(w, by, HOLD_POSE)) // @1019 -> @1288
}

fn on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    let Some(e) = ext(w, b) else { return false };
    // The parent's: InputTriggered @3891 on the owner's own presses.
    if attach_ally::on(f, &e.attach, s, a) {
        return true;
    }
    // ActivatedLaunch: HeldBaseFighter's own InputPreTriggered (the buff's owner).
    if s == BuffSignal::InputPreTriggered {
        return e.launch_bound && e.held == Some(f) && a.input.is_some_and(|i| (0..=3).contains(&i));
    }
    // The owner's KnockedBack -> @6622.
    if s == BuffSignal::KnockedBack {
        return same_team(w, f, e.ins);
    }
    false
}

fn end(w: &mut World, f: FighterId, b: BuffRef) {
    // The parent's EndPlay first.
    let mut at = ext_mut(w, b).map(|x| std::mem::take(&mut x.attach)).unwrap_or_default();
    attach_ally::end(w, f, &mut at);
    crate::fighter::anim::set_being_carried(w, f, false, true); // @1421
    let ins = ext_mut(w, b).map(|x| {
        x.attach = at;
        // The FacingChanged bindings go with the actor.
        x.face_in = 0.0;
        x.launch_in = 0.0;
        x.check_in = 0.0;
        x.ins
    });
    let Some(Some(by)) = ins else { return };
    if same_team(w, f, Some(by)) {
        // @3685 LaunchCharacter((0, 0, 2500), false, true): Z replaced, XY added to.
        if w.fighter(f).life.dead <= 0.0 {
            let m = &mut w.fighter_mut(f).moves;
            m.impulse = Some(Impulse { vx: 0.0, vy: 2500.0, xy: false, z: true, kind: 0, d: None });
            m.hit_launch = true;
        }
        sfx(w, f, Some(SFX_RELEASE)); // @3756
        return;
    }
    let h = crate::combat::holds::raw_half(w, f);
    let r = crate::math::narrow(def(w, f).inner_cap_radius * f64::from(w.fighter(f).body.size_scale));
    let ignore = [Some(ActorRef::Fighter(by)), Some(ActorRef::Fighter(f))];
    let (bx, by_y) = (w.fighter(by).body.x, actor_y(w, by));
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let tr = capsule_trace_single(w, bx, by_y, fx, fy, r, h, TRACE_PROJECTILE_GROUND_NO_PLATFORM, &ignore, None);
    let (px, py) = tr.map_or((fx, fy), |t| (t.x, t.y)); // ReleaseLocationPreCheck @2939
    let types = [crate::real(OBJ_WORLD_STATIC), crate::real(OBJ_DAMAGE_DESTRUCTIBLE)]; // [0, 8] @3038
    let over = !capsule_overlap_actors(w, px, py, r, h, &types, &ignore).is_empty();
    if !(over || tr.is_some()) {
        return; // @3227 -> @869: the rotation alone
    }
    let top = by_y + (h + 25.0);
    let line = sphere_trace_single(w, bx, by_y, bx, top, 0.0, TRACE_PROJECTILE_GROUND_NO_PLATFORM, &ignore, None);
    if line.is_some() {
        let half = crate::combat::holds::raw_half(w, by) - (h + 25.0);
        teleport_move(w, f, bx, by_y + half);
    } else if over {
        teleport_move(w, f, bx, by_y + h);
    } else {
        teleport_move(w, f, px, py);
    }
}

#[allow(clippy::too_many_arguments, reason = "the hook's signature")]
fn taken_cond(
    w: &mut World,
    owner: FighterId,
    b: BuffRef,
    _attacker: Option<ActorRef>,
    _boxed: bool,
    _hit: &HitPacket,
    by: Option<FighterId>,
) -> bool {
    ext(w, b).is_some_and(|e| attach_ally::taken_cond(w, owner, &e.attach, by))
}

fn attach_parent(w: &World, _f: FighterId, b: BuffRef) -> Option<FighterId> {
    ext(w, b).and_then(|e| attach_ally::attach_parent(&e.attach))
}

/// VelmaNotHoldingFighterCondition_C: Velma, holding neither an enemy nor an ally.
fn not_holding(w: &mut World, f: FighterId) -> bool {
    velma_of(w, f).is_some_and(|s| !(w.fighter(f).holds.holding_enemy || s.carry.holding_ally))
}

/// This file's registry entries: Buff_VelmaAttachToAlly_C (the parent's hooks around its own: `attachToAlly`),
/// VelmaNotHoldingFighterCondition_C.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        ATTACH_TO_ALLY,
        BuffEntry::new(
            BuffClass::BuffVelmaAttachToAlly,
            BuffHooks {
                begin: Some(begin),
                tick: Some(tick),
                on: Some(on),
                end: Some(end),
                taken_cond: Some(taken_cond),
                attach_parent: Some(attach_parent),
                ..BuffHooks::default()
            },
        ),
    )?;
    r.conditions.enter("VelmaNotHoldingFighterCondition_C", Condition::Fn(not_holding))
}
