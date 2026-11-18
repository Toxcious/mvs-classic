//! The stage's placed actors that take hits: the hit path of a placed stage actor and the
//! non-fighter team test, Comp_Pawn_DamageManager DamageAndLaunch's non-fighter branch. Read from
//! Comp_Pawn_DamageManager, Comp_Pawn_HitBoxHandler, Comp_Actor_Destructible. The hit tests that reach this (a
//! fighter's melee hit shapes, a projectile's or an item's hit body over the actor's hurt shapes) are the World's.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, StageActorClass};
use crate::combat::HitPacket;
use crate::combat::boxes::{ExtraHurt, WorldSphere};
use crate::events::{Event, SoundHit};
use crate::fighter::FighterId;
use crate::fighter::common::warn_text;
use crate::fighter::moves::V2;
use crate::registry::entries::StageActorEntry;
use crate::world::World;
use crate::world::stage::StageState;
use mvs_data::value::Props;

/// `StageActor`: a placed stage actor (or one a mode spawns with a placed one's hit path), by its
/// place in `StageState::actors` (`ActorRef::Stage`). The class's own state comes with the stages' ports.
#[derive(State, Debug)]
pub struct StageActor {
    /// The Blueprint class.
    pub cls: StageActorClass,
    /// Its place in `StageData.placed` (stable); a run-time actor's: its place in `StageState::actors`.
    pub index: u32,
    pub team: i32,
    /// K2_DestroyActor: it leaves the hit tests; it stays in the list (its index is its id).
    pub dead: bool,
    /// Its root now, sim space (x = -UE Y, y = UE Z).
    pub x: Real,
    pub y: Real,
    /// Its class's own state (the fields beyond StageActor's), set by its entry's create.
    pub own: StageActorOwn,
}

/// A stage actor's class's own state, a variant per class family (each struct in its stage's module).
#[derive(State, Debug, Default)]
pub enum StageActorOwn {
    /// None yet (before its create), or a class with no own state.
    #[default]
    None,
    Pillar(crate::stages::tree_fort::Pillar),
    Speaker(crate::stages::cromulons::Speaker),
    ScoobyWall(crate::stages::scoobys_haunted_mansion::ScoobyWall),
    ScoobyElevator(crate::stages::scoobys_haunted_mansion::ScoobyElevator),
    ScoobySwitch(crate::stages::scoobys_haunted_mansion::ScoobySwitch),
    BatcaveFog(crate::stages::batcave::BatcaveFog),
    BatcaveCar(crate::stages::batcave::BatcaveCar),
    BatcaveBats(crate::stages::batcave::BatcaveBats),
    M008Elevator(crate::stages::throne_room::M008Elevator),
    M010Hoop(crate::stages::the_court::M010Hoop),
    M010Wall(crate::stages::the_court::M010Wall),
    Meeseeks(crate::modes::tutorial::advanced::meeseeks::MeeseeksOwn),
}

/// `StageHurt`: a stage actor's hurt shape.
#[derive(Clone, PartialEq, Debug)]
pub enum StageHurt {
    Up(ExtraHurt),
    Box(WorldSphere),
    Depth { x: Real, y: Real, z0: Real, z1: Real, r: Real },
}

fn slot(a: ActorRef) -> u32 {
    match a {
        ActorRef::Stage(i) => i,
        _ => panic!("not a stage actor: {a:?}"),
    }
}

fn actor(w: &World, a: ActorRef) -> &StageActor {
    &w.stage_run.as_ref().expect("a stage state").actors[slot(a) as usize]
}

// ---- the call surface of the stage actors

/// `canBeHitBy`: Comp_Actor_Destructible.CanBeHitBy(the dealer's team) (@0-143): not
/// DamageDisabled and (FriendlyFire or another team). `dealer_team`: DamageAndLaunch's resolved team, the dealer's own
/// Comp_Actor_Destructible TeamIndex (@588-596; -1 when it has none: a level actor); `victim_team`: the victim
/// component's TeamIndex (a spawned actor's is its owner's, SetTeamIndexOnSpawn; a level actor's -1).
pub fn can_be_hit_by(dealer_team: i32, victim_team: i32, d: Option<CanBeHitByD>) -> bool {
    let d = d.unwrap_or(CanBeHitByD { damage_disabled: false, friendly_fire: false });
    !d.damage_disabled && (d.friendly_fire || dealer_team != victim_team)
}

/// `stageMeshTicks`: the stage actors' skeletal mesh ticks (TG_PrePhysics), ahead of
/// the spawned actors'. Unverified: a level actor's mesh is taken to tick ahead of the spawned actors' (it registered
/// first, with the level); the live tick order of a level actor's mesh against the pawns' was not read.
pub fn stage_mesh_ticks(w: &mut World) {
    let n = w.stage_run.as_ref().map_or(0, |r| r.actors.len());
    for i in 0..n {
        let a = ActorRef::Stage(u32::try_from(i).expect("a list index"));
        if !actor(w, a).dead && crate::actors::actor_montage::has_anim(w, a) {
            crate::actors::actor_montage::mesh_tick(w, a);
        }
    }
}

/// `stageDamageAndLaunch`: Comp_Pawn_DamageManager DamageAndLaunch (@0-10985) on a
/// placed stage actor. Returns Hit (CanBeHitBy passed).
/// - @51 CanTakeDamage (the class's DamageManager, default true): false is no hit and nothing else.
/// - @608-659 CanBeHitBy(dealer_team) against TeamIndex -1 (FriendlyFire and DamageDisabled the kind's).
/// - Allowed: no fighter damage clamp, no decay (RegisterHitFromFighter exits for a non-fighter); ApplyDamage(Damage)
///   @2591 -> ReceiveAnyDamage (`any_damage`) for any Damage but 0 (read in the exe: ApplyDamage and
///   AActor::TakeDamage stop at 0 alone, bCanBeDamaged is not read); then the tail from @3222.
/// - Refused (@718 -> @8047): HitByAlly (`hit_by_ally`, force = the hit's ForceKnockback); without ForceKnockback (or
///   AlwaysReceiveKnockback, which no asset sets) pop_flow to @7956. With it @8516 goto 3222: the same tail as an
///   allowed hit, without ApplyDamage. The ApplyBuffDamageMods ahead of it (@8150-8489) needs a Comp_State_Manager on
///   the victim: a stage class has none, so it is left out (not ported).
/// - The tail (@3222-5631): the reaction (light or knockback), the dealer fighter's BPI_ControllerRumble @3668
///   (`by_fighter`), HitByEnemy @3723, the victim's own hit pause @3818-5212 only with a Comp_Actor_Damage and a
///   Comp_HitPause (`current_damage` and `victim_pause`: LocalScalingHitPause, unverified), SetMostRecentAttacker /
///   MostRecentDamagingObjectClass @5302-5631 (read by nothing for a stage actor), then pop_flow to @7956: Hit =
///   CanBeHitBy and not LocalHitFailed.
///
/// The dealer's own hit pause on a Hit is the caller's (pfgDoDamage @16603-17442: `stage_hit_pause_scale`).
/// Not ported: DoLOSCheck (pfgDoDamage @1007-1012) is not tested; its elemental calls are not made.
/// `owner`: the dealer's owning fighter (the fighter itself for its melee); `by_fighter`: the dealer actor is that
/// fighter.
#[allow(clippy::too_many_arguments, reason = "the call keeps the build function's parameters")]
pub fn stage_damage_and_launch(
    w: &mut World,
    a: ActorRef,
    dealer_team: i32,
    hit: &HitPacket,
    dmg: Real,
    causer: ActorRef,
    owner: FighterId,
    at: V2,
    by_fighter: bool,
) -> bool {
    let i = slot(a);
    let Some(st) = w.stage_run.as_ref().and_then(|r| r.actors.get(i as usize)) else { return false };
    let Some(k) = w.registry.stage_actors.get(st.cls.name()).copied() else { return false };
    if st.dead || !k.can_take_damage {
        return false;
    }
    let team = st.team;
    let dd = k.damage_disabled.is_some_and(|d| d(w, i));
    let can =
        can_be_hit_by(dealer_team, team, Some(CanBeHitByD { damage_disabled: dd, friendly_fire: k.friendly_fire }));
    if !can {
        let force = hit.force_knockback == Some(true);
        if let Some(h) = k.hit_by_ally {
            h(w, i, causer, hit, force);
        }
        if !force {
            return false;
        }
    } else if dmg != 0.0
        && let Some(h) = k.any_damage
    {
        h(w, i, dmg, causer);
    }
    if !actor(w, a).dead {
        reaction(w, &k, a, hit, dmg, causer, owner, at, by_fighter);
    }
    if by_fighter {
        crate::fighter::tick::controller_rumble(w, owner, hit.controller_rumble);
    }
    if !actor(w, a).dead
        && let Some(h) = k.hit_by_enemy
    {
        h(w, i, causer, hit, dmg);
    }
    let raw = hit.hit_pause_time.unwrap_or(0.0);
    if !actor(w, a).dead
        && raw > 0.0
        && k.current_damage.is_some()
        && let Some(vp) = k.victim_pause
    {
        let sec = raw * crate::consts::HIT_PAUSE_MULT * stage_hit_pause_scale(w, &k, a);
        vp(w, i, sec);
    }
    can
}

/// The reaction branch (@3222 -> @3346): BaseKnockBack > 0 or HitStunMult > 0 is the knockback path, a notification
/// on a non-fighter (Calculate Knock Back returns for an owner that is no BP_FighterCharacter; StopMovement @10435 on an
/// AIController a stage actor has not; KnockedBack, KnockedBackIncludeSelf, HitDispatch: `knocked_back`); else a
/// positive Damage takes LightHitReactionMulticast @3468 -> PlayHitSound at the hit (OnHitSound / OnHitAudio) with the
/// victim's physical material (`surface`), none when the dealer is a fighter in hitstun (@140-329).
#[allow(clippy::too_many_arguments, reason = "the call's inputs")]
fn reaction(
    w: &mut World,
    k: &StageActorEntry,
    a: ActorRef,
    hit: &HitPacket,
    dmg: Real,
    causer: ActorRef,
    owner: FighterId,
    at: V2,
    by_fighter: bool,
) {
    if hit.base_knock_back.unwrap_or(0.0) > 0.0 || hit.hit_stun_mult.unwrap_or(0.0) > 0.0 {
        if let Some(h) = k.knocked_back {
            h(w, slot(a), causer, hit);
        }
        return;
    }
    let Some(ev) = hit.on_hit_sound.or(hit.on_hit_audio) else { return };
    if dmg.is_nan() || dmg <= 0.0 || (by_fighter && crate::fighter::in_hitstun(w, owner)) {
        return;
    }
    // Deviation: a kind without `surface` posts the sound with SurfaceType 6 (flesh, no PhysMaterialOverride),
    // warned: GetVictimPhysmat of the hit component is the class's to give.
    if k.surface.is_none() {
        warn_text(w, &format!("stage actor {}: no surface for the light hit sound (6 taken)", k.class.name()));
    }
    let charge = if by_fighter { Some(crate::fighter::percent_charged(w, owner)) } else { None };
    let hit = SoundHit { victim: -1, z_floor: None, charge, surface: Some(k.surface.unwrap_or(6)), instigator: None };
    let fighter = w.fighter(owner).id;
    w.events.push(Event::Sound { fighter, ev, x: Some(at.x), y: Some(at.y), hit: Some(hit), pooled: false, key: None });
}

/// `stageHitActorRows`: pfgDoDamage's OnHitBuffs rows for the HitActor on a Hit
/// (@18293-19121: ApplyBuffHit? and ApplyBuffEnemy? -> SpawnBuffActor on it): a stage actor holds no buff list, so
/// such a row is refused with a warning. Not ported.
pub fn stage_hit_actor_rows(w: &World, a: ActorRef, rows: Option<&[Props]>) {
    let Some(rows) = rows else { return };
    let cls = actor(w, a).cls.name();
    for e in rows {
        let on = |k: &str| e.get(k).is_some_and(|v| v.as_bool() == Some(true));
        if on("ApplyBuffHit?") && on("ApplyBuffEnemy?") {
            let b = e.get("AppliedBuffs").and_then(|v| v.as_str()).unwrap_or("undefined");
            warn_text(w, &format!("{b} on {cls} (a stage actor HitActor): OnHitBuffs rows not ported"));
        }
    }
}

/// `stageHitPauseScale`: the dealer's hit pause scale on a Hit (pfgDoDamage
/// @16603-16790): the victim DamageManager's HitPauseMultiplier (the component default 1), times the CurrentDamage
/// tier only with a Comp_Actor_Damage on the victim (<= 50: 1, <= 90: 1.2, <= 110: 1.3, else 1.5).
pub fn stage_hit_pause_scale(w: &mut World, k: &StageActorEntry, a: ActorRef) -> Real {
    let m = k.hit_pause_multiplier.unwrap_or(1.0);
    let Some(cd) = k.current_damage else { return m };
    let d = cd(w, slot(a));
    m * if d <= 50.0 {
        1.0
    } else if d <= 90.0 {
        1.2
    } else if d <= 110.0 {
        1.3
    } else {
        1.5
    }
}

/// `stageHurt`: its hurt shapes now with the channel they answer (`None`: none, or its
/// collision off). A kind with `hurt` and no `hurt_type` is warned and met by nothing (no default object type).
/// `body`: the caller's buffer, cleared, then the shapes (the `body` of the result); the channel is returned.
pub fn stage_hurt(w: &mut World, k: &StageActorEntry, a: ActorRef, body: &mut Vec<StageHurt>) -> Option<Real> {
    body.clear();
    let hurt = k.hurt?;
    if actor(w, a).dead {
        return None;
    }
    let Some(ch) = k.hurt_type else {
        warn_text(w, &format!("stage actor {}: hurt shapes with no hurtType (met by nothing)", k.class.name()));
        return None;
    };
    if !hurt(w, slot(a), body) || body.is_empty() {
        return None;
    }
    Some(crate::real(ch))
}

/// `stageHurtOver`: whether the hit shape `s` (a sphere with a depth, a depth-extruded
/// sphere: `z` `None`, or a box) meets any of the hurt shapes: an upright capsule as a fighter's root capsule
/// (`shape_over_capsule`), a box as the oriented boxes' overlap (`shape_over_box`), a depth capsule by the distance to
/// its segment (a box's to the segment, `seg_box_dist2`; an extruded sphere's in the play plane, where its line meets
/// every depth).
pub fn stage_hurt_over(s: &WorldSphere, body: &[StageHurt]) -> bool {
    use crate::combat::boxes::{shape_over_box, shape_over_capsule};
    body.iter().any(|h| match h {
        StageHurt::Up(c) => shape_over_capsule(s, c),
        StageHurt::Box(b) => shape_over_box(s, b),
        &StageHurt::Depth { x, y, z0, z1, r } => depth_over(s, x, y, z0, z1, r),
    })
}

fn depth_over(s: &WorldSphere, x: Real, y: Real, z0: Real, z1: Real, r: Real) -> bool {
    if s.bx.is_some() {
        return crate::combat::boxes::seg_box_dist2(s, x, y, z0, x, y, z1) <= r * r;
    }
    let (dx, dy) = (s.x - x, s.y - y);
    let dz = s.z.map_or(0.0, |z| z - crate::math::max(z0, crate::math::min(z1, z)));
    let rr = s.r + r;
    dx * dx + dy * dy + dz * dz <= rr * rr
}

/// `stageVulnerable`: pfgDoDamage's IsVulnerableTo gate (@4608, ahead of
/// DamageAndLaunch): InvulnerabilityCount <= 0 (DamageDisabled and the team are CanBeHitBy's, inside
/// DamageAndLaunch).
pub fn stage_vulnerable(w: &mut World, k: &StageActorEntry, a: ActorRef) -> bool {
    !actor(w, a).dead && !k.invulnerable.is_some_and(|i| i(w, slot(a)))
}

/// `stageHurtCentre`: the point a projectile's hit body is taken nearest to: the first
/// shape's centre in the plane.
pub fn stage_hurt_centre(body: &[StageHurt]) -> V2 {
    match &body[0] {
        StageHurt::Up(h) => V2 { x: h.x, y: (h.y0 + h.y1) / 2.0 },
        StageHurt::Box(h) => V2 { x: h.x, y: h.y },
        &StageHurt::Depth { x, y, .. } => V2 { x, y },
    }
}

/// The stage state's actor list (for the callers that walk it).
pub fn stage_actors(run: &StageState) -> &[StageActor] {
    &run.actors
}

// ---- the call surface's argument and return types

/// What `canBeHitBy` takes or gives (a struct literal).
#[derive(Clone, Debug)]
pub struct CanBeHitByD {
    pub damage_disabled: bool,
    pub friendly_fire: bool,
}

/// `stageHitActorRows` over the typed rows of a packet: a row for the HitActor that
/// applies to a hit enemy is refused with a warning (a stage actor holds no buff list).
pub fn stage_hit_actor_rows_typed(w: &World, a: ActorRef, rows: &[crate::combat::OnHitBuffRow]) {
    let cls = actor(w, a).cls.name();
    for e in rows {
        if e.apply_buff_hit && e.apply_buff_enemy {
            warn_text(
                w,
                &format!(
                    "{} on {cls} (a stage actor HitActor): OnHitBuffs rows not ported",
                    w.names.str(e.applied_buffs)
                ),
            );
        }
    }
}

/// `StageActor.dead` of the stage's actor `i`.
pub fn stage_actor_dead(w: &World, i: u32) -> bool {
    w.stage_run.as_ref().and_then(|r| r.actors.get(usize::try_from(i).ok()?)).is_some_and(|a| a.dead)
}

/// `StageActor.index` of the stage's actor `i`: its StageData.placed index (a run-time
/// actor's past every placed one).
pub fn stage_actor_index(w: &World, i: u32) -> i32 {
    let r = w.stage_run.as_ref().expect("a stage run");
    let a = &r.actors[usize::try_from(i).expect("a stage actor index")];
    i32::try_from(a.index).expect("a stage actor index")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_be_hit_by_rules() {
        assert!(can_be_hit_by(0, -1, None));
        assert!(!can_be_hit_by(-1, -1, None));
        assert!(can_be_hit_by(-1, -1, Some(CanBeHitByD { damage_disabled: false, friendly_fire: true })));
        assert!(!can_be_hit_by(0, -1, Some(CanBeHitByD { damage_disabled: true, friendly_fire: true })));
    }

    #[test]
    fn depth_capsule_over_a_sphere() {
        let s = WorldSphere { x: 0.0, y: 0.0, z: Some(50.0), r: 10.0, ..WorldSphere::default() };
        assert!(depth_over(&s, 5.0, 0.0, 0.0, 100.0, 10.0));
        assert!(!depth_over(&s, 25.0, 0.0, 0.0, 100.0, 10.0));
        assert_eq!(
            stage_hurt_centre(&[StageHurt::Depth { x: 1.0, y: 2.0, z0: 0.0, z1: 1.0, r: 1.0 }]),
            V2 { x: 1.0, y: 2.0 }
        );
    }
}
