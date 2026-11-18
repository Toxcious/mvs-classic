//! Pawn_TomAndJerry's own logic (offsets of its ubergraph): BeginPlay's
//! bindings, the JerryPointer and the wall traces of ReceiveTick, ResetJerry, the BPI_TomAndJerry setters his other
//! files call, the conditions JerryOutCondition / JerryNotOutCondition and the notifies AnimNotify_JerryIgnoreWall /
//! AnimNotify_JerryOffsetOverride; its hooks (`pawnHooks`). The shared AnimNotify_OverrideMirror is the shared notifies';
//! Comp_JerryTracker / Comp_JerryControler are data only.
//!
//! BeginPlay @5024-5480: the four bindings (EX_BindDelegate targets):
//!   OnBFDefeated -> HideJakeModel @5286: CharHooks.death (below). Deviation: the port's abduct defeat (World's abduct
//!     path) broadcasts OnBFDefeated to the buffs alone and calls no `death` hook, so Jerry is not put back there.
//!   AbilityCooldownUpdated -> ResetJerry @5329: CharHooks.ability_cooldown_updated.
//!   FacingChanged -> OnFacingChange @5394: ubergraph @5479, an empty pop_flow (nothing).
//!   UpdatedSubMesh -> UpdatedVisibility @5437: CharHooks.updated_sub_mesh.
//! then HulkAnimBP = the mesh's anim instance @5096 (read nowhere), OriginalGravityScale @5115 (read by no Tom and Jerry
//! graph), OriginalCapsuleHalfHeight / Radius @5164 / @5213 (BaseFighter's, the template's in the port), and @5480 ->
//! @286: HasAuthority -> SetJerryAttachedMulticast(true) ([`begin_play`]: `init` and `match_reset`).
//! UserConstructionScript: BodyB.SetMasterPoseComponent(Mesh): drawing (the view).
//! BPI_Cleanup @6260 (ResetFighterLogic: CharHooks.reset): isGliding = false (no Tom and Jerry graph sets it true),
//! VisibleCoolDown = false.
//! BPI_GetFighterCharacter returns EFighter TomAndJerry (15). BPI_Use2DHitboxes returns "Use 2D Hitboxes" (class default
//! true: CharDef.use2D); its one writer, BPI_Toggle2DHitboxes @15, is Comp_Pawn_HitBoxHandler's debug path, as for the
//! other pawns. HeadScale @6093 (BPI_SetHeadScale -> the AnimBP's HeadScale): the view's. The AI (BPI_ReadyForAI
//! @6619-7174): bots.rs (no track).
//!
//! Tick order: ReceiveTick @352 runs the JerryPointer @376-1203 first, then the wall traces @1312-4996
//! (BPI_JerryWallOffset @3647). Its place against the AnimBP's update is a pawn Tick's against its AnimBP: the pawn's
//! tick first, the mesh's anim update after it (Fighter's mesh tick), so the WallAlpha the tick writes is read by the
//! same frame's update; the sockets the tick reads are the pose evaluated last frame.
//! The pawn's hit data is the shared code's: KnockBackMirror on PaddleBall's first hit of each section, AllowGroundControl
//! in isAttackingFlag, ReflectProjectiles on the Tennis hitframes, AnimNotify_UpdateHitframeDamage in
//! Air_Attack_TrashLids; Rocket_Kick / Rocket_Kick_NoJerry(_Ground) name Control as Default's next section and have
//! none: the montage plays to its end (no such section, INDEX_NONE).

use super::aim;
use super::arms;
use super::state::{tj, tj_of, tj_w};
use crate::Real;
use crate::actors::world_actor::{actor_cls, actor_dead};
use crate::actors::{ActorRef, item::truthy};
use crate::events::{Event, NotifyRef};
use crate::fighter::anim::overrides::{add_override_pack, remove_override_pack};
use crate::fighter::anim::slot_blend::slot_blend_reset;
use crate::fighter::anim::{OverridePack, OverrideRow, SlotMix, set_sub_mesh};
use crate::fighter::control::{char_data_in, trigger_ability};
use crate::fighter::traces::{TRACE_PROJECTILE_GROUND, TRACE_WALL_JUMP, box_trace_single, sphere_trace_single};
use crate::fighter::{FighterId, actor_y, cap_half, capsule_half, facing, mesh_wx, mesh_wy};
use crate::math::max;
use crate::math::ue::size;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{Condition, NotifyHandler};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use mvs_data::fighter::AttackEntry;
use mvs_data::value::Value;

/// ResetJerry's Index == 4 @5485, TriggerAbilityActual(4) @6568.
const JERRY_SLOT: i32 = 4;
/// The pointer's EqualEqual_ClassClass @1055 (not the TempInvincibility child).
const JERRY_CLASS: Name = name!("Jerry_SpawnedActor_C");

// ---------------------------------------------------------------- BPI_TomAndJerry
// The pawn's interface (BPI_TomAndJerry; Pawn_TomAndJerry's implementations at the offsets named) that the other tracks
// call on a Tom and Jerry (`tj_of` is `Some`).

/// BPI_SetIsJerryAttached(IsAttached) @6441: HasAuthority (offline: true) -> SetJerryAttachedMulticast(IsAttached).
/// Callers: Jerry's EndPlay @7704 and the pickup (jerry.rs), fishing.rs, kicked.rs and rocket.rs.
pub fn set_is_jerry_attached(w: &mut World, f: FighterId, attached: bool) {
    set_jerry_attached_multicast(w, f, attached);
}

/// BPI_TriggerJerryCooldown -> TriggerAbilityMulticast @6553 -> TriggerAbilityActual(4) @6568 (`trigger_ability`: the
/// game mode's instant cooldowns and BlockCooldownCount start nothing). Caller: Jerry's EndPlay @8271 (jerry.rs). The
/// cooldown's start and end come back through AbilityCooldownUpdated ([`reset_jerry`]).
pub fn trigger_jerry_cooldown(w: &mut World, f: FighterId) {
    trigger_ability(w, f, JERRY_SLOT);
}

/// BPI_SetJerryOffsetTrace(Value) @332: JerryOffsetOverrideTrace. Caller: AnimNotify_JerryOffsetOverride (below).
fn set_jerry_offset_trace(w: &mut World, f: FighterId, value: bool) {
    tj_w(w, f).jerry_offset_override_trace = value;
}

// ---------------------------------------------------------------- Tom_and_Jerry_AnimBP's setters
// The BPI_Anim_BP / Tom_and_Jerry_AnimBP events the other tracks call on Tom's anim instance (the cast to BPI_Anim_BP of
// the mesh's GetAnimInstance): the AnimBP events' own assignments (`TomAndJerryState::anim`), read by aim.rs.

/// BPI_SetAimDirection(AimDirection) @16057 (-> @992, AimDirectionInverted: aim.rs derives it). Callers:
/// TomAndJerry_SlingShot_Aim_02 (slingshot.rs), TomAndJerry_NoJerry_SlingShot_Aim (tennis.rs).
pub fn set_aim_direction(w: &mut World, f: FighterId, dir: Real) {
    tj_w(w, f).anim.aim_direction = dir;
}

/// BPI_SetControlBool(CrontrolBool) @16129 (RocketControlBool). Caller: JerryRocket (rocket.rs).
pub fn set_control_bool(w: &mut World, f: FighterId, on: bool) {
    tj_w(w, f).anim.rocket_control = on;
}

/// BPI_SetFacingOverride(ShouldOverrideFacing?) @16089: OverrideMirror?. Caller: AnimNotify_OverrideMirror's begin
/// (true) and end (false) (through CharHooks.set_facing_override).
fn set_facing_override(w: &mut World, f: FighterId, should: bool) {
    tj_w(w, f).anim.override_mirror = should;
}

/// BPI_JerryWallOffset(Alpha) @16029 (WallAlpha). Caller: the pawn's tick @3647 ([`wall_tick`]).
fn jerry_wall_offset(w: &mut World, f: FighterId, alpha: Real) {
    tj_w(w, f).anim.wall_alpha = alpha;
}

// ---------------------------------------------------------------- the pawn's own events

/// SetJerryAttachedMulticast(JerryAttached) @6465-6484: JerryAttached, then SetSubMeshVisibility(JerryMesh,
/// JerryAttached, true) (`set_sub_mesh`: its UpdatedSubMesh broadcast reaches [`updated_visibility`]).
fn set_jerry_attached_multicast(w: &mut World, f: FighterId, attached: bool) {
    tj_w(w, f).jerry_attached = attached;
    set_sub_mesh(w, f, name!("JerryMesh"), attached);
}

/// The pack of the class defaults the NoJerry taunt adds (`f.data.pawn.NoJerryTauntOverride`): the taunt button's
/// Tom_and_Jerry_Emote_NoJerry_Taunt_Montage, Priority 0.
pub fn no_jerry_taunt_pack(w: &World, f: FighterId) -> OverridePack {
    let v = char_data_in(&w.data, w, f).fighter.pawn.get("NoJerryTauntOverride").and_then(Value::as_obj);
    let Some(v) = v else { panic_data(w, f, "the pawn data has no NoJerryTauntOverride") };
    let priority = v.get("Priority").and_then(Value::as_f64).unwrap_or_else(|| panic_data(w, f, "no Priority"));
    let rows = v.get("Overrides").and_then(Value::as_arr).unwrap_or_else(|| panic_data(w, f, "no Overrides"));
    let overrides = rows
        .iter()
        .map(|r| {
            let r = r.as_obj().unwrap_or_else(|| panic_data(w, f, "an Overrides row is no object"));
            let attack = r.get("Attack").cloned().unwrap_or(Value::Null);
            let attack: AttackEntry = mvs_data::value::from_value(attack)
                .unwrap_or_else(|e| panic_data(w, f, &format!("NoJerryTauntOverride's Attack: {e}")));
            OverrideRow { aerial_move: truthy(r, "AerialMove"), attack }
        })
        .collect();
    OverridePack { priority: crate::math::narrow(priority), overrides, from_data: true }
}

/// UpdatedVisibility(Tag, Visible) @6283-6440, bound to Comp_FighterVisibilityManager.UpdatedSubMesh: for the JerryMesh
/// tag (EqualEqual_NameName: no case), shown -> Comp_AbilityOverrideManager.RemoveOverridePack(NoJerryTauntOverride)
/// @6349, hidden -> AddOverridePack(NoJerryTauntOverride) @6395.
fn updated_visibility(w: &mut World, f: FighterId, tag: Name, visible: bool) {
    if w.names.str(tag).to_lowercase() != "jerrymesh" {
        return;
    }
    let pack = no_jerry_taunt_pack(w, f);
    if visible {
        remove_override_pack(w, f, &pack);
    } else {
        add_override_pack(w, f, pack);
    }
}

/// The world location of a mesh socket on Jerry's bones (sim space), as GetSocketLocation reads the pose last
/// evaluated: the sampled pose (`sampled_boxes`) with ModifyBone_6's pull of jerry_un_COM toward the component origin by
/// its alpha (aim.rs `jerry_offset_alpha`; every jerry_* bone rides jerry_un_COM, whose socket Jerry_Location_Socket is
/// sampled). ModifyBone_7's 100 cm is along the depth. The two bones are sampled as sockets for this. None without a
/// pose.
fn jerry_socket(w: &World, f: FighterId, bone: &str) -> Option<(Real, Real)> {
    let b = w.fixed.samplers.get(&w.fighter(f).ch)?;
    let i = *b.by_lower.get(&format!("socket:{bone}"))?;
    let c = *b.by_lower.get("socket:jerry_location_socket")?;
    let s = crate::fighter::anim::sampled_boxes(w, f)?;
    let o = usize::try_from(b.offsets[i as usize]).expect("a box offset");
    let oc = usize::try_from(b.offsets[c as usize]).expect("a box offset");
    let a = aim::jerry_offset_alpha(w, f);
    let g = |k: usize| crate::real(s[k]);
    let x = mesh_wx(w, f) + (g(o) - a * g(oc)) * facing(w, f);
    let y = mesh_wy(w, f)
        + g(o + 1)
        + a * crate::math::narrow(char_data_in(&w.data, w, f).boxes.mesh_z - f64::from(g(oc + 1)));
    crate::fighter::anim::put_pose(w, s);
    Some((x, y))
}

/// ResetJerry(Index, CooldownComplete) @5485 (bound to Comp_Pawn_AbilityManager.AbilityCooldownUpdated): slot 4 alone ->
/// JerryCoolDownMulticast(CooldownComplete) @6074.
///   complete @5553: VisibleCoolDown = true, SetJerryAttachedMulticast(true), SpawnEmitterAtLocation(PS_TomJerryPoof00 at
///     the socket jerry_un_C_spine_a, rotation 0, scale 2) @5646 (event TjPoof), PF_Post_Event_At_Location(
///     sfx_cha_tomandjerry_slingshot_reappear, the socket, the instigator, self) @5802;
///   started @5889: VisibleCoolDown = false, PF_Post_Event_At_Location(sfx_cha_tomandjerry_slingshot_fall, the socket)
///     @5987.
fn reset_jerry(w: &mut World, f: FighterId, slot: i32, complete: bool) {
    if slot != JERRY_SLOT {
        return;
    }
    let at = |w: &World| jerry_socket(w, f, "jerry_un_c_spine_a").unwrap_or((w.fighter(f).body.x, actor_y(w, f)));
    if complete {
        tj_w(w, f).visible_cooldown = true;
        set_jerry_attached_multicast(w, f, true);
        let (x, y) = at(w);
        let id = w.fighter(f).id;
        let reappear = name!("sfx_cha_tomandjerry_slingshot_reappear");
        let events = &mut w.fighter_mut(f).events;
        events.push(Event::TjPoof { fighter: id, x, y, scale: 2.0 });
        events.push(Event::Sound {
            fighter: id,
            ev: reappear,
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
    } else {
        tj_w(w, f).visible_cooldown = false;
        let (x, y) = at(w);
        let id = w.fighter(f).id;
        let ev = name!("sfx_cha_tomandjerry_slingshot_fall");
        w.fighter_mut(f).events.push(Event::Sound {
            fighter: id,
            ev,
            x: Some(x),
            y: Some(y),
            hit: None,
            pooled: false,
            key: None,
        });
    }
}

/// The pawn's BeginPlay end (@5480 -> @286: SetJerryAttachedMulticast(true)) and the anim instance's initialisation
/// ([`aim::anim_init`]), for a new pawn: when the fighter is made (`init`) and at a match reset (mod.rs's `match_reset`
/// made the state new first). The slot graph's lists start at INDEX_NONE (a new SlotBlend).
fn begin_play(w: &mut World, f: FighterId) {
    let s = tj_w(w, f);
    if let Some(b) = s.pawn.slot_mix.slot_blend.as_mut() {
        slot_blend_reset(b);
    }
    slot_blend_reset(&mut s.anim.arms);
    aim::anim_init(w, f);
    set_jerry_attached_multicast(w, f, true);
}

/// pawn.rs's part of the joined matchReset (after mod.rs's new state): its BeginPlay (the slot blends reset,
/// SetJerryAttachedMulticast(true)).
pub fn match_reset(w: &mut World, f: FighterId) {
    begin_play(w, f);
}

/// ReceiveTick @352, the JerryPointer @376-1203 (IsLocallyControlled @400: true for every pawn of an offline match, a
/// player's or a bot's AIController alike): with VisibleCoolDown false or JerryMesh shown (@410 / @424), the DoOnce
/// @35-115 hides JerryPointer once and opens the Gate @140; else the DoOnce is reset @270 and, the Gate open, every
/// Jerry_SpawnedActor_C of Comp_Spawner.SpawnedActors (that class exactly, in list order: the last wins) becomes
/// SpawnedJerryAcor @1184 and shows JerryPointer @1203. The arrow's turn toward it (@480-665) is the view's drawing.
fn pointer_tick(w: &mut World, f: FighterId) {
    if !tj(w, f).visible_cooldown || aim::jerry_mesh_visible(w, f) {
        let p = &mut tj_w(w, f).pawn;
        if p.hide_done {
            return;
        }
        p.hide_done = true;
        p.pointer_visible = false;
        p.gate_open = true;
        return;
    }
    tj_w(w, f).pawn.hide_done = false;
    if !tj(w, f).pawn.gate_open {
        return;
    }
    let list = w.fighter(f).items.spawned_actors.clone();
    for a in list {
        // a destroyed actor has left the build's array
        if actor_dead(w, a) || actor_cls(w, a) != Some(JERRY_CLASS) {
            continue;
        }
        let p = &mut tj_w(w, f).pawn;
        p.spawned_jerry = Some(a);
        p.pointer_visible = true;
    }
}

/// ReceiveTick's wall part @1312-4996, every tick. Sim space (x = -UE Y, y = UE Z); the forward vector is the facing.
///   z = JerryOffsetOverrideTrace ? ActorZ - CapsuleHalfHeight + 50 : the socket jerry_un_C_spine_d's Z (@1340-1725);
///   BoxTraceSingle(actor + fwd x 40 at z, actor + fwd x JerryOffsetMax at z, HalfSize (0, 0, 25), rotation 0,
///     channel 2 WallJump, ignore self) @1927: a hit -> JerryOffsetAlpha = 1 - |Location - start| / (JerryOffsetMax - 40)
///     @2103-3044 (the start taken again, the same point); none -> 0 @4996;
///   the ledge @3071: LineTraceSingle(feet + fwd x 150, 20 lower; channel 3 ProjectileGroundTrace) @3380: ground ahead ->
///     @3530; none -> LineTraceSingle(feet + fwd x 150 - 20 up, feet - fwd x 75 - 20 up) @4141: a hit with
///     |CharacterMovement.Velocity| < 50 -> JerryOffsetAlpha = FMax(it, 1 - |feet - ImpactPoint| / 150) @4403-4964;
///   @3530: BPI_JerryWallOffset(JerryOffsetAlpha) @3647.
fn wall_tick(w: &mut World, f: FighterId) {
    let fwd = facing(w, f);
    let (ax, az) = (w.fighter(f).body.x, actor_y(w, f));
    // the raw CapsuleHalfHeight (@1615 / @3169 / @3834 / @4431): unscaled
    let cap_half_u = w.fighter(f).capsule.cap_half_u;
    let feet =
        az - (if cap_half_u > 0.0 { capsule_half(w, f) * (cap_half_u / cap_half(w, f)) } else { capsule_half(w, f) });
    let jerry_max = char_data_in(&w.data, w, f)
        .fighter
        .pawn
        .get("JerryOffsetMax")
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic_data(w, f, "the pawn data has no JerryOffsetMax"));
    let z = if tj(w, f).jerry_offset_override_trace {
        feet + 50.0
    } else {
        // no pose: the actor's height (never met: the boxes hold the socket)
        jerry_socket(w, f, "jerry_un_c_spine_d").map_or(az, |at| at.1)
    };
    let me = Some(ActorRef::Fighter(f));
    let sx = ax + fwd * 40.0;
    let hit = box_trace_single(
        w,
        sx,
        z,
        ax + fwd * crate::math::narrow(jerry_max),
        z,
        0.0,
        25.0,
        0.0,
        TRACE_WALL_JUMP,
        &[],
        me,
    );
    let mut alpha = match hit {
        Some(h) if h.blocking => 1.0 - size([0.0, h.x - sx, h.y - z]) / crate::math::narrow(jerry_max - 40.0),
        _ => 0.0,
    };
    let lx = ax + fwd * 150.0;
    if sphere_trace_single(w, lx, feet, lx, feet - 20.0, 0.0, TRACE_PROJECTILE_GROUND, &[], me).is_none() {
        let back = sphere_trace_single(
            w,
            lx,
            feet - 20.0,
            ax - fwd * 75.0,
            feet - 20.0,
            0.0,
            TRACE_PROJECTILE_GROUND,
            &[],
            me,
        );
        let b = &w.fighter(f).body;
        let slow = size([0.0, b.vx, b.vy]) < 50.0;
        if let Some(h) = back
            && h.blocking
            && slow
        {
            alpha = max(alpha, 1.0 - size([0.0, ax - h.ix, feet - h.iy]) / 150.0);
        }
    }
    tj_w(w, f).pawn.jerry_offset_alpha = alpha;
    jerry_wall_offset(w, f, alpha);
}

/// pawn.rs's part of the joined tick (before champagne.rs's): the JerryPointer, then the wall traces.
pub fn tick(w: &mut World, f: FighterId) {
    pointer_tick(w, f);
    wall_tick(w, f);
}

/// HideJakeModel (OnBFDefeated @5286 -> ubergraph @5480 -> @286): HasAuthority -> SetJerryAttachedMulticast(true).
/// The slot mix: the port does not update a dead pawn's AnimBP (aim.rs, Finn's rule).
fn death(w: &mut World, f: FighterId) {
    set_jerry_attached_multicast(w, f, true);
    tj_w(w, f).pawn.slot_mix.slot_mix.clear();
}

/// BPI_Cleanup @6271: VisibleCoolDown = false.
fn reset(w: &mut World, f: FighterId) {
    tj_w(w, f).visible_cooldown = false;
}

/// `animUpdate`: aim.rs's, then (after the montage update) arms.rs's.
fn anim_update(w: &mut World, f: FighterId, dilation: Real, pre: bool) {
    aim::anim_update(w, f, dilation, pre);
    if !pre {
        arms::arms_update(w, f, dilation);
    }
}

fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&tj(w, f).pawn.slot_mix)
}

fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    Some(&mut tj_w(w, f).pawn.slot_mix)
}

/// JerryOutCondition_C: Not(BPI_CheckJerryAttached) of the owner cast to BPI_TomAndJerry, a failed cast true (Not of
/// false).
fn jerry_out(w: &mut World, f: FighterId) -> bool {
    !tj_of(w, f).is_some_and(|s| s.jerry_attached)
}

/// JerryNotOutCondition_C: BPI_CheckJerryAttached, a failed cast false.
fn jerry_not_out(w: &mut World, f: FighterId) -> bool {
    tj_of(w, f).is_some_and(|s| s.jerry_attached)
}

/// AnimNotify_JerryIgnoreWall_C begin (BL_TomAndJerry @121): casts MeshComp's anim instance to Tom_and_Jerry_AnimBP_C and
/// sets IgnoreJerryWallOffset. Montages (PaddleBall, SlingShot, Rocket, Fishing, the emotes) and the layer's sequences
/// (Idle_Alt, WallGrab, WallGrab_Idle: seqNotifies). A failed cast (another AnimBP on the mesh): nothing.
fn ignore_wall_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if let Some(s) = crate::chars::tom_and_jerry::state::tj_mut(w, f) {
        s.anim.ignore_wall_offset = true;
    }
}

/// AnimNotify_JerryIgnoreWall_C end: with TurnOffOnEnd? @0, clears it @135.
fn ignore_wall_end(w: &mut World, f: FighterId, n: NotifyRef, _key: i32) {
    if tj_of(w, f).is_none() {
        return;
    }
    if truthy(crate::fighter::match_side::notify_props(w, f, n), "TurnOffOnEnd?") {
        tj_w(w, f).anim.ignore_wall_offset = false;
    }
}

/// AnimNotify_JerryOffsetOverride_C begin / end @121: cast MeshComp's owner to BPI_TomAndJerry and call
/// BPI_SetJerryOffsetTrace(true / false) (Attack_Tennis, _Air, Combo_04).
fn offset_override_begin(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if tj_of(w, f).is_some() {
        set_jerry_offset_trace(w, f, true);
    }
}

fn offset_override_end(w: &mut World, f: FighterId, _n: NotifyRef, _key: i32) {
    if tj_of(w, f).is_some() {
        set_jerry_offset_trace(w, f, false);
    }
}

/// The hooks of this file in Tom and Jerry's CharHooks (`pawnHooks`, in registration order): `init`, `death`, `reset`,
/// `ability_cooldown_updated`, `updated_sub_mesh`, `turned` (aim.rs), `anim_update` (aim.rs, then arms.rs), `pose`
/// (arms.rs), `set_facing_override`, `slot_input` and `anim_set` (aim.rs), `slot_mix` / `slot_mix_mut` (the pawn
/// state's mix). `match_reset` and `tick` are mod.rs's joined ones.
pub fn hooks(h: &mut CharHooks) {
    h.init = Some(begin_play);
    h.death = Some(death);
    h.reset = Some(reset);
    h.ability_cooldown_updated = Some(reset_jerry);
    h.updated_sub_mesh = Some(updated_visibility);
    h.turned = Some(aim::on_facing_changed);
    h.anim_update = Some(anim_update);
    h.pose = Some(arms::arm_shapes);
    h.set_facing_override = Some(set_facing_override);
    h.slot_input = Some(aim::slot_input);
    h.slot_mix = Some(slot_mix);
    h.slot_mix_mut = Some(slot_mix_mut);
    h.anim_set = Some(aim::anim_set);
}

/// This file's registry entries (the two conditions, the two notifies).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.conditions.enter("JerryOutCondition_C", Condition::Fn(jerry_out))?;
    r.conditions.enter("JerryNotOutCondition_C", Condition::Fn(jerry_not_out))?;
    let ignore =
        NotifyHandler { begin: Some(ignore_wall_begin), end: Some(ignore_wall_end), ..NotifyHandler::default() };
    r.notifies.enter("AnimNotify_JerryIgnoreWall_C", ignore)?;
    let offset = NotifyHandler {
        begin: Some(offset_override_begin),
        end: Some(offset_override_end),
        ..NotifyHandler::default()
    };
    r.notifies.enter("AnimNotify_JerryOffsetOverride_C", offset)?;
    Ok(())
}

/// A fault of the pawn data: the character and what is missing.
fn panic_data(w: &World, f: FighterId, what: &str) -> ! {
    panic!("tomandjerry pawn ({}): {what}", w.names.str(w.fighter(f).ch))
}
