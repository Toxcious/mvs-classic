//! Batcave's level script (Map_Batcave_C: the vent cycle, the warning state) and its two fog actors
//! (FogOcclusionBP_C).
//!
//! The level script (offsets: ExecuteUbergraph_Map_Batcave):
//! - ReceiveBeginPlay @1303: the UI and the bindings (@1323-2192), then @447: GetAllowHazards @502 and HasAuthority
//!   @554 (the port runs every World as the authority), then the two nonlooping timers TurnOnLeftFog @653 and
//!   TurnOnRightFog @779, each RandomFloatInRange(VentOnMinTime 70, VentOnMaxTime 120) (left drawn first); then
//!   SetBatcaveWarningState(0) @2320. The bindings: left fog FogEnabled -> LeftVentOpen @2000, FogDisabled ->
//!   LeftVentClosed @2064, right fog FogEnabled -> RightVentOpen @2128, FogDisabled -> RightVentClose @2192.
//!   Unverified: the level script's draws against the bats' (`bats`) and any other BeginPlay's are in the order
//!   the port's create runs them (the level's actor BeginPlay order is not read).
//! - TurnOnLeftFog @3502 -> @1137: SetBatcaveWarningState(SteamLeftOn 3), Batcave_FanBlades_2.StartSpeedup @1190,
//!   Delay(WarningTimeToVent 1.25) @1244 -> @987: FocOcclusionBP_Left.ActivateFogForDuration(VentFogDuration 30),
//!   Delay(30 + WarningTimeAfterVent 2) @1078 -> @15: SetBatcaveWarningState(SteamLeftOff 7), StopSpeedup @68, the
//!   TurnOnLeftFog timer again with RandomFloatInRange(VentRestartMinTime 45, VentRestartMaxTime 75) @173.
//! - TurnOnRightFog @5053: the same with SteamRightOn 4, Batcave_FanBlades2 @5106, Delay @3443 -> @837 (the right
//!   fog), Delay @928 -> @231 (SteamRightOff 8, StopSpeedup @284, the TurnOnRightFog timer @389).
//! - The vent callbacks cross the sides (@2751-3409): the left fog's FogEnabled / FogDisabled set the AnimBP's
//!   RightVent true / false, the right fog's set LeftVent. BatCenterStage_2 is hidden: no visible pose.
//! - ResetLevelObjects @2337 (bound to the parent game mode's dispatcher, broadcast once by StartNewMatch):
//!   [`vents_reset_level_objects`]. OnTrainingLevelChange @5165: [`vents_training_level_change`].
//!
//! The level constants (Default__Map_Batcave_C) are the build's values, not read from the stage data. The fans'
//! speed-up and the vent bools are presentation: state the drawing reads, no sim effect.

use super::{own, own_mut, slot_of};
use crate::Real;
use crate::State;
use crate::actors::{ActorRef, StageActorClass};
use crate::combat::BuffClass;
use crate::combat::boxes::WorldSphere;
use crate::combat::buffs::{buff_of, remove_stack, spawn_buff};
use crate::combat::elem::{actor_remove_stack, spawn_actor_buff};
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, no_collision, no_hit};
use crate::math::{max, to_int32};
use crate::name;
use crate::names::Name;
use crate::world::World;
use crate::world::pawns::{all_list, put_fighters};
use crate::world::stage::{stage_delay, stage_random_range, stage_set_timer};
use crate::world::stage_actors::StageActorOwn;

/// The level script's owner number for its Delays and timers (`StageTimer::owner`; a placed actor's is its
/// `StageData.placed` index, never negative).
pub const LEVEL_OWNER: i32 = -1;
/// This file's event numbers (`StageEntry::timer` `ev`) are `VENT_EV` and up, below `CAR_EV` (`car`).
pub const VENT_EV: i32 = 100;
const EV_LEFT_ON: i32 = VENT_EV; // the TurnOnLeftFog timer
const EV_RIGHT_ON: i32 = VENT_EV + 1; // the TurnOnRightFog timer
const EV_LEFT_FOG: i32 = VENT_EV + 2; // Delay @1244 -> @987
const EV_LEFT_OFF: i32 = VENT_EV + 3; // Delay @1078 -> @15
const EV_RIGHT_FOG: i32 = VENT_EV + 4; // Delay @3443 -> @837
const EV_RIGHT_OFF: i32 = VENT_EV + 5; // Delay @928 -> @231
const EV_FOG_ON: i32 = VENT_EV + 10; // a fog's Delay(DelayUntilInvisibility) @6458 -> @895
const EV_FOG_END: i32 = VENT_EV + 11; // a fog's Delay(FogDuration) @7652 -> @879
const EV_FOG_DESTROY: i32 = VENT_EV + 12; // a fog's Delay(1.5) @6926 -> @15
// Timer keys (one handle per object and delegate) and Delay keys (one action per object and node): the offsets.
const KEY_LEFT_TIMER: i32 = 3502;
const KEY_RIGHT_TIMER: i32 = 5053;

// Default__Map_Batcave_C
const WARNING_TIME_TO_VENT: Real = 1.25;
const WARNING_TIME_AFTER_VENT: Real = 2.0;
const VENT_FOG_DURATION: Real = 30.0;
const VENT_ON_MIN: Real = 70.0;
const VENT_ON_MAX: Real = 120.0;
const VENT_RESTART_MIN: Real = 45.0;
const VENT_RESTART_MAX: Real = 75.0;
/// FogOcclusionBP: PlayerAttachCollision_GEN_VARIABLE.SphereRadius (no instance overrides it), the sphere on the
/// root; Default__FogOcclusionBP_C.DelayUntilInvisibility (an instance value of the stage data first).
const FOG_RADIUS: Real = 300.0;
const DELAY_UNTIL_INVISIBILITY: Real = 1.25;

const FOG_BUFF: Name = name!("Buff_Invisibility_Fog_C");
const JERRY: Name = name!("Jerry_SpawnedActor_C");
const JERRY_TEMP: Name = name!("Jerry_SpawnedActor_TempInvincibility_C");

/// A FogOcclusionBP_C (FocOcclusionBP_Left / _Right): its state in the match.
/// - `fog_activated`: FogActivated (true from the activation Delay's link @895 to DeactivateFog @6540);
/// - `buffed`: BuffedFighters (fighter ids, a follower's key (`follower_key`) below 0, insertion order);
/// - `inside`: the fighters PlayerAttachCollision overlaps (ids), kept while FogActivated (the begin and end overlap
///   events test FogActivated first, and StartFogBuff asks the overlap afresh);
/// - `particle`: FogParticle, the drawing's (ActivateFogVisuals @6992: PS_SmokeNew at the root + VisualOffset (-30,
///   0, 0), scale (1.25, 1.15, 1); sfx_map_batcave_steam_occlusion): 0 none (or destroyed), 1 active, 2 deactivated
///   (DeactivateFogVisuals @6876), `dissolve` its Dissolve parameter 1 (@6822).
#[derive(State, Debug, Default)]
pub struct BatcaveFog {
    /// The root's depth (UE X).
    pub z: Real,
    /// FocOcclusionBP_Left 0, _Right 1, another name -1 (the level script names neither).
    pub side: i32,
    /// DelayUntilInvisibility (s).
    pub delay_on: Real,
    pub fog_activated: bool,
    pub buffed: Vec<i32>,
    pub inside: Vec<i32>,
    pub particle: i32,
    pub dissolve: bool,
}

/// `fogKind.create`: the fog at its placed place.
pub fn fog_create(w: &mut World, slot: u32) {
    let data = std::sync::Arc::clone(&w.data);
    let r = w.stage_run.as_mut().expect("a stage run");
    let a = &mut r.actors[slot as usize];
    let d = &data.stage.placed.as_ref().expect("placed actors")[a.index as usize];
    let side = if d.name == "FocOcclusionBP_Left" {
        0
    } else if d.name == "FocOcclusionBP_Right" {
        1
    } else {
        -1
    };
    a.own = StageActorOwn::BatcaveFog(BatcaveFog {
        z: crate::math::narrow(d.loc.first().copied().unwrap_or(0.0)),
        side,
        delay_on: crate::registry::props::num(&d.p, "DelayUntilInvisibility").unwrap_or(DELAY_UNTIL_INVISIBILITY),
        ..BatcaveFog::default()
    });
}

// SetBatcaveWarningState's enum (WarningState: Names): Idle 0, BatsLeftOn 1, BatsRightOn 2, SteamLeftOn 3,
// SteamRightOn 4, BatsLeftOff 5, BatsRightOff 6, SteamLeftOff 7, SteamRightOff 8.
pub const WARN_IDLE: i32 = 0;
pub const WARN_BATS_LEFT_ON: i32 = 1;
pub const WARN_BATS_RIGHT_ON: i32 = 2;
pub const WARN_STEAM_LEFT_ON: i32 = 3;
pub const WARN_STEAM_RIGHT_ON: i32 = 4;
pub const WARN_BATS_LEFT_OFF: i32 = 5;
pub const WARN_BATS_RIGHT_OFF: i32 = 6;
pub const WARN_STEAM_LEFT_OFF: i32 = 7;
pub const WARN_STEAM_RIGHT_OFF: i32 = 8;

// The six warning screen actors' bHidden: the drawing reads them. Screen order: bats global, left, right; steam
// global, left, right.
const SCREEN_BATS: usize = 0;
const SCREEN_BATS_LEFT: usize = 1;
const SCREEN_BATS_RIGHT: usize = 2;
const SCREEN_STEAM: usize = 3;
const SCREEN_STEAM_LEFT: usize = 4;
const SCREEN_STEAM_RIGHT: usize = 5;

/// `VentsState`.
#[derive(State, Debug, Default)]
pub struct VentsState {
    /// The last SetBatcaveWarningState value.
    pub warning: i32,
    /// The six screens' bHidden, `SCREEN_*` order.
    pub hidden: [bool; 6],
    /// The two fogs' `StageData.placed` indices (FocOcclusionBP_Left, _Right), -1 where the level has none (IsValid
    /// false).
    pub fogs: [i32; 2],
    /// Batcave_FanBlades_2 (the left cycle's) and Batcave_FanBlades2 (the right's) sped up: StartSpeedup to
    /// StopSpeedup.
    pub fans: [bool; 2],
    /// BatCenterStage_Skeleton_AnimBlueprint's LeftVent / RightVent (crossed: the left fog sets RightVent).
    pub left_vent: bool,
    pub right_vent: bool,
}

/// `ventsCreate`: the level script's BeginPlay.
pub fn vents_create(w: &mut World) -> VentsState {
    let fog_of = |w: &World, side: i32| -> i32 {
        w.stage_run
            .as_ref()
            .into_iter()
            .flat_map(|r| r.actors.iter())
            .find(|a| {
                a.cls == StageActorClass::FogOcclusionBP
                    && matches!(&a.own, StageActorOwn::BatcaveFog(f) if f.side == side)
            })
            .map_or(-1, |a| i32::try_from(a.index).expect("a placed index"))
    };
    let mut s = VentsState {
        warning: WARN_IDLE,
        hidden: [true; 6],
        fogs: [fog_of(w, 0), fog_of(w, 1)],
        fans: [false, false],
        left_vent: false,
        right_vent: false,
    };
    // BeginPlay @447-779: AllowHazards and authority, then the two first waits (left drawn first).
    if w.setup.allow_hazards {
        let left = stage_random_range(w, VENT_ON_MIN, VENT_ON_MAX);
        stage_set_timer(w, LEVEL_OWNER, KEY_LEFT_TIMER, left, false, EV_LEFT_ON);
        let right = stage_random_range(w, VENT_ON_MIN, VENT_ON_MAX);
        stage_set_timer(w, LEVEL_OWNER, KEY_RIGHT_TIMER, right, false, EV_RIGHT_ON);
    }
    set_warning_state(&mut s, WARN_IDLE); // @2320
    s
}

/// SetBatcaveWarningState (reliable multicast; Map_Batcave:138-189): the screens it shows and hides. Called by the
/// level script's vent cycle and, through the parent game mode's bats dispatchers, by the bats (`bats`).
pub fn set_warning_state(s: &mut VentsState, state: i32) {
    let h = &mut s.hidden;
    s.warning = state;
    match state {
        WARN_IDLE => h.fill(true),
        WARN_BATS_LEFT_ON | WARN_BATS_RIGHT_ON => {
            h[SCREEN_BATS] = false;
            h[if state == WARN_BATS_LEFT_ON { SCREEN_BATS_LEFT } else { SCREEN_BATS_RIGHT }] = false;
            h[SCREEN_STEAM] = true;
        }
        WARN_STEAM_LEFT_ON | WARN_STEAM_RIGHT_ON => {
            h[SCREEN_STEAM] = false;
            h[if state == WARN_STEAM_LEFT_ON { SCREEN_STEAM_LEFT } else { SCREEN_STEAM_RIGHT }] = false;
            h[SCREEN_BATS] = true;
        }
        WARN_BATS_LEFT_OFF | WARN_BATS_RIGHT_OFF | WARN_STEAM_LEFT_OFF | WARN_STEAM_RIGHT_OFF => {
            let side = [SCREEN_BATS_LEFT, SCREEN_BATS_RIGHT, SCREEN_STEAM_LEFT, SCREEN_STEAM_RIGHT]
                [usize::try_from(state - WARN_BATS_LEFT_OFF).expect("a warning state")];
            h[side] = true;
            h[SCREEN_BATS] = h[SCREEN_BATS_LEFT] && h[SCREEN_BATS_RIGHT];
            h[SCREEN_STEAM] = h[SCREEN_STEAM_LEFT] && h[SCREEN_STEAM_RIGHT];
        }
        // other bytes only print their name
        _ => {}
    }
}

fn fog_ref(w: &World, slot: u32) -> &BatcaveFog {
    match &w.stage_run.as_ref().expect("a stage run").actors[slot as usize].own {
        StageActorOwn::BatcaveFog(f) => f,
        o => panic!("stage actor {slot} is not a BatcaveFog: {o:?}"),
    }
}

fn fog_mut(w: &mut World, slot: u32) -> &mut BatcaveFog {
    match &mut w.stage_run.as_mut().expect("a stage run").actors[slot as usize].own {
        StageActorOwn::BatcaveFog(f) => f,
        o => panic!("stage actor {slot} is not a BatcaveFog: {o:?}"),
    }
}

fn fog_xy(w: &World, slot: u32) -> (Real, Real) {
    let a = &w.stage_run.as_ref().expect("a stage run").actors[slot as usize];
    (a.x, a.y)
}

/// `fogAt`: the fog of a `StageData.placed` index (its slot), none for a negative one.
fn fog_at(w: &World, index: i32) -> Option<u32> {
    if index < 0 {
        return None;
    }
    let slot = slot_of(w, index)?;
    (w.stage_run.as_ref()?.actors[slot as usize].cls == StageActorClass::FogOcclusionBP).then_some(slot)
}

/// A due Delay or timer of the level script or a fog (`ev` `VENT_EV` and up).
pub fn vents_timer(w: &mut World, ev: i32, owner: i32) {
    if owner != LEVEL_OWNER {
        let Some(f) = fog_at(w, owner) else { return };
        if ev == EV_FOG_ON {
            // @895: FogActivated = true, StartFogBuff, broadcast FogEnabled
            fog_mut(w, f).fog_activated = true;
            start_fog_buff(w, f);
            fog_enabled(w, f, true);
        } else if ev == EV_FOG_END {
            deactivate_fog(w, f, true); // @879
        } else if ev == EV_FOG_DESTROY {
            fog_mut(w, f).particle = 0; // @15 FogParticle.K2_DestroyComponent (the current one)
        }
        return;
    }
    match ev {
        // TurnOnLeftFog @3502 -> @1137, TurnOnRightFog @5053
        EV_LEFT_ON | EV_RIGHT_ON => {
            let left = ev == EV_LEFT_ON;
            let o = own_mut(w);
            set_warning_state(&mut o.vents, if left { WARN_STEAM_LEFT_ON } else { WARN_STEAM_RIGHT_ON });
            o.vents.fans[usize::from(!left)] = true; // StartSpeedup @1190 / @5106
            stage_delay(
                w,
                LEVEL_OWNER,
                if left { 1244 } else { 3443 },
                WARNING_TIME_TO_VENT,
                if left { EV_LEFT_FOG } else { EV_RIGHT_FOG },
            );
        }
        // @987, @837
        EV_LEFT_FOG | EV_RIGHT_FOG => {
            let left = ev == EV_LEFT_FOG;
            let fogs = own(w).vents.fogs;
            if let Some(f) = fog_at(w, fogs[usize::from(!left)]) {
                activate_fog_for_duration(w, f, VENT_FOG_DURATION);
            }
            // Add_FloatFloat(VentFogDuration, WarningTimeAfterVent) @1032 / @882
            stage_delay(
                w,
                LEVEL_OWNER,
                if left { 1078 } else { 928 },
                VENT_FOG_DURATION + WARNING_TIME_AFTER_VENT,
                if left { EV_LEFT_OFF } else { EV_RIGHT_OFF },
            );
        }
        // @15, @231
        EV_LEFT_OFF | EV_RIGHT_OFF => {
            let left = ev == EV_LEFT_OFF;
            let o = own_mut(w);
            set_warning_state(&mut o.vents, if left { WARN_STEAM_LEFT_OFF } else { WARN_STEAM_RIGHT_OFF });
            o.vents.fans[usize::from(!left)] = false; // StopSpeedup @68 / @284
            let wait = stage_random_range(w, VENT_RESTART_MIN, VENT_RESTART_MAX);
            stage_set_timer(
                w,
                LEVEL_OWNER,
                if left { KEY_LEFT_TIMER } else { KEY_RIGHT_TIMER },
                wait,
                false,
                if left { EV_LEFT_ON } else { EV_RIGHT_ON },
            );
        }
        _ => {}
    }
}

/// The fog's FogEnabled / FogDisabled broadcast and the level script's callbacks bound to it (@2000-2192): the left
/// fog drives RightVent, the right fog LeftVent (@2890-3409; each after a valid AnimBP cast).
fn fog_enabled(w: &mut World, f: u32, on: bool) {
    let index = i32::try_from(w.stage_run.as_ref().expect("a stage run").actors[f as usize].index).expect("an index");
    let o = own_mut(w);
    if index == o.vents.fogs[0] {
        o.vents.right_vent = on;
    } else if index == o.vents.fogs[1] {
        o.vents.left_vent = on;
    }
}

/// ActivateFogForDuration(FogDuration) @7638: ActivateFog @6444 (ActivateFogVisuals, then Delay(DelayUntilInvisibility)
/// @6458 -> @895), then Delay(FogDuration) @7652 -> @879. Both Delays start in this call; each is one action of its
/// node per fog, so a call while one runs starts nothing of that one (and neither a reset nor a deactivation cancels
/// them).
pub fn activate_fog_for_duration(w: &mut World, f: u32, duration: Real) {
    // ActivateFogVisuals @6992 (off a dedicated server): a new particle (the old one, still valid, Deactivate @7507)
    let fog = fog_mut(w, f);
    fog.particle = 1;
    fog.dissolve = false;
    let (delay_on, index) = (fog.delay_on, fog_index(w, f));
    stage_delay(w, index, 6458, delay_on, EV_FOG_ON);
    stage_delay(w, index, 7652, duration, EV_FOG_END);
}

fn fog_index(w: &World, slot: u32) -> i32 {
    i32::try_from(w.stage_run.as_ref().expect("a stage run").actors[slot as usize].index).expect("an index")
}

/// DeactivateFog(DissolveFog) @6517: DeactivateFogVisuals @6586, FogActivated = false @6540, EndFogBuff @6551,
/// broadcast FogDisabled @6565.
fn deactivate_fog(w: &mut World, f: u32, dissolve: bool) {
    // DeactivateFogVisuals (off a dedicated server): mix_stop_all_fade_onesecond on the fog @6704 (the drawing's, on
    // the particle's leaving 1); a valid particle: Dissolve 1 when dissolving @6822, Deactivate @6876, then destroyed
    // at once @6912 -> @15, or after Delay(1.5) @6926 when dissolving.
    if fog_ref(w, f).particle != 0 {
        if dissolve {
            let fog = fog_mut(w, f);
            fog.dissolve = true;
            fog.particle = 2;
            let index = fog_index(w, f);
            stage_delay(w, index, 6926, 1.5, EV_FOG_DESTROY);
        } else {
            fog_mut(w, f).particle = 0;
        }
    }
    fog_mut(w, f).fog_activated = false;
    end_fog_buff(w, f);
    fog_mut(w, f).inside.clear();
    fog_enabled(w, f, false);
}

/// ResetLevelObjects @2337 (the parent game mode's dispatcher; StartNewMatch broadcasts it once, 0.2 s after
/// BeginPlay): IsValid(RightFog) @2337, then DeactivateFog(false) on Left @2380 and Right @2417; the cars' BPI_Reset
/// @2454-2645 are `car`'s. It clears no timer or Delay, stops no fan and leaves the warning state. At StartNewMatch
/// no fog is on (the first vent is 70 s or more later): it changes no state there.
pub fn vents_reset_level_objects(w: &mut World) {
    let fogs = own(w).vents.fogs;
    if fog_at(w, fogs[1]).is_none() {
        return;
    }
    for i in fogs {
        if let Some(f) = fog_at(w, i) {
            deactivate_fog(w, f, false);
        }
    }
}

/// OnTrainingLevelChange @5165 (PandaGameState's dispatcher): each valid fog, Left then Right, DeactivateFog(false).
/// The port has no training level change: nothing calls it.
#[allow(dead_code, reason = "nothing calls it: the port has no training level change")]
pub fn vents_training_level_change(w: &mut World) {
    let fogs = own(w).vents.fogs;
    for i in fogs {
        if let Some(f) = fog_at(w, i) {
            deactivate_fog(w, f, false);
        }
    }
}

// ---- FogOcclusionBP's membership (authority)

/// A BaseFighter (or a BPI_Follower: the follower functions below) that is not AlreadyRungOut (a defeated fighter:
/// `life.dead`) and has a Comp_State_Manager (every fighter).
fn eligible(w: &World, o: FighterId) -> bool {
    w.fighter(o).life.dead <= 0.0
}

/// SpawnBuffActor(Buff_Invisibility_Fog_C, Actor, Actor, 1, 0.0, true, false) and Set_Add(BuffedFighters) (@5319 /
/// @5523 in StartFogBuff, @6304 / @6388 at a begin overlap).
fn give_buff(w: &mut World, f: u32, o: FighterId) {
    spawn_buff(w, o, FOG_BUFF, 0.0, Some(1), Some(o), true, None);
    let id = w.fighter(o).id;
    fog_mut(w, f).buffed.push(id);
}

/// CheckHasBuff(Buff_Invisibility_Fog) -> RemoveStack(1) and Set_Remove (@611-818, @1543-1809). Without the buff the
/// actor stays in BuffedFighters (the Set_Remove is behind the CheckHasBuff branch).
fn take_buff(w: &mut World, f: u32, o: FighterId) {
    let Some(b) = buff_of(w, o, FOG_BUFF) else { return };
    remove_stack(w, o, b, 1);
    let id = w.fighter(o).id;
    let fog = fog_mut(w, f);
    if let Some(k) = fog.buffed.iter().position(|&x| x == id) {
        fog.buffed.remove(k);
    }
}

/// StartFogBuff @3922: PlayerAttachCollision.GetOverlappingActors(BaseFighter) @4009, each eligible one not in
/// BuffedFighters buffed; then the same over GetOverlappingActors(Jerry_SpawnedActor_C) @2134-3821 (an IsA filter:
/// the class and its children, Jerry_SpawnedActor_TempInvincibility_C among them; a follower of another class is met
/// by the begin overlap alone).
// Unverified: the overlapping actors are taken in fighter order, then in World.actors order (the build's overlap
// array order is not read).
fn start_fog_buff(w: &mut World, f: u32) {
    use crate::actors::follower::{follower_key, follower_of, follower_touches};
    fog_mut(w, f).inside.clear();
    let all = all_list(w);
    for &o in &all {
        if !touches(w, o, f) {
            continue;
        }
        let id = w.fighter(o).id;
        fog_mut(w, f).inside.push(id);
        if eligible(w, o) && !fog_ref(w, f).buffed.contains(&id) {
            give_buff(w, f, o);
        }
    }
    put_fighters(w, all);
    let (x, y) = fog_xy(w, f);
    let z = fog_ref(w, f).z;
    let mut i = 0;
    while let Some(a) = w.actors.actors.nth(i) {
        i += 1;
        if w.actors.actors.get(a).is_none_or(|x| x.dead)
            || follower_of(w, ActorRef::Actor(a)).is_none()
            || !follower_touches(w, a, x, y, z, FOG_RADIUS, 0.0)
        {
            continue;
        }
        let key = to_int32(follower_key(w, a));
        fog_mut(w, f).inside.push(key);
        if is_jerry(w, a) {
            follower_give(w, f, a);
        }
    }
}

/// EndFogBuff @3826: Set_ToArray(BuffedFighters), each one's stack removed (and it leaves the set) when it has the
/// buff.
fn end_fog_buff(w: &mut World, f: u32) {
    for id in fog_ref(w, f).buffed.clone() {
        if id < 0 {
            follower_take(w, f, id);
            continue;
        }
        if let Some(o) = w.by_id(id) {
            take_buff(w, f, o);
        }
    }
}

// ---- a follower (`actors::follower`; its list key is `follower_key`)

/// Jerry's actor, the class StartFogBuff asks for (@2191), and its children: GetOverlappingActors filters with IsA, so
/// a child class (Jerry_SpawnedActor_TempInvincibility_C, INDEX: SuperStruct Jerry_SpawnedActor_C) counts.
fn is_jerry(w: &World, a: crate::actors::ActorId) -> bool {
    w.actors.actors.get(a).is_some_and(|x| x.cls == JERRY || x.cls == JERRY_TEMP)
}

/// The begin overlap @5648 and StartFogBuff's second loop on a follower: BaseFighter || BPI_Follower @5787-5928; the
/// AlreadyRungOut read @6003 / @2836 is on the failed BaseFighter cast (None: read as false, the Accessed None
/// default); !Set_Contains; its Comp_State_Manager (SpawnedActorParent's) SpawnBuffActor(Buff_Invisibility_Fog_C,
/// Actor, Actor, 1, 0.0, true) @6304 / @3497, Set_Add @6388 / @3701.
fn follower_give(w: &mut World, f: u32, a: crate::actors::ActorId) {
    let key = to_int32(crate::actors::follower::follower_key(w, a));
    if fog_ref(w, f).buffed.contains(&key) {
        return;
    }
    spawn_actor_buff(w, ActorRef::Actor(a), FOG_BUFF, 1, 0.0, true, None);
    fog_mut(w, f).buffed.push(key);
}

/// The end overlap @49 and EndFogBuff @940-1869 on a follower: in the set, CheckHasBuff(Fog) on its
/// Comp_State_Manager -> RemoveStack(1) and Set_Remove (none without the buff, as `take_buff`). A follower destroyed
/// since leaves the set.
// Deviation: its end overlap at the destroy is not run; its buffs end with it.
fn follower_take(w: &mut World, f: u32, key: i32) {
    let Some(k) = fog_ref(w, f).buffed.iter().position(|&x| x == key) else { return };
    let Some(ActorRef::Actor(a)) = crate::actors::follower::follower_by_key(w, crate::real(key)) else {
        fog_mut(w, f).buffed.remove(k);
        return;
    };
    let Some(Some(st)) = crate::actors::world_actor::actor_elem(w, ActorRef::Actor(a)) else { return };
    let Some(serial) = st.buffs.iter().find(|b| b.class == BuffClass::BuffInvisibilityFog).map(|b| b.serial) else {
        return;
    };
    actor_remove_stack(w, ActorRef::Actor(a), serial, 1, false);
    fog_mut(w, f).buffed.remove(k);
}

/// The followers' overlap events in `vents_tick`, behind the fighters'.
// Unverified: the order of the two in one tick is not read.
fn follower_overlaps(w: &mut World, f: u32) {
    use crate::actors::follower::{follower_by_key, follower_key, follower_of, follower_touches};
    let live: Vec<i32> = fog_ref(w, f)
        .inside
        .iter()
        .copied()
        .filter(|&k| !(k < 0 && follower_by_key(w, crate::real(k)).is_none()))
        .collect();
    fog_mut(w, f).inside = live;
    let (x, y) = fog_xy(w, f);
    let z = fog_ref(w, f).z;
    let mut i = 0;
    while let Some(a) = w.actors.actors.nth(i) {
        i += 1;
        if w.actors.actors.get(a).is_none_or(|x| x.dead) || follower_of(w, ActorRef::Actor(a)).is_none() {
            continue;
        }
        let key = to_int32(follower_key(w, a));
        let on = follower_touches(w, a, x, y, z, FOG_RADIUS, 0.0);
        let k = fog_ref(w, f).inside.iter().position(|&v| v == key);
        match (on, k) {
            (true, None) => {
                fog_mut(w, f).inside.push(key);
                follower_give(w, f, a);
            }
            (false, Some(k)) => {
                fog_mut(w, f).inside.remove(k);
                follower_take(w, f, key);
            }
            _ => {}
        }
    }
}

/// The overlap events of PlayerAttachCollision, tested once a tick in the leftover pass (`StageEntry::tick`), after
/// the frame's Delays: begin overlap @5648 (FogActivated, not in the set, eligible -> the buff), end overlap @49
/// (FogActivated, in the set, no longer overlapping -> `take_buff`). Only while FogActivated: both events test it
/// first, and StartFogBuff asks the overlap afresh.
// Deviation: the build's events come inside the fighters' movement (ahead of the frame's latent pass); the port's
// test follows the latent pass, so a fighter entering in the frame the fog turns on or off is met by StartFogBuff /
// EndFogBuff alone (the same buffs at the frame's end). A pass through the sphere inside one tick is missed.
pub fn vents_tick(w: &mut World) {
    for i in own(w).vents.fogs {
        let Some(f) = fog_at(w, i) else { continue };
        if !fog_ref(w, f).fog_activated {
            continue;
        }
        let all = all_list(w);
        for &o in &all {
            let on = touches(w, o, f);
            let id = w.fighter(o).id;
            let k = fog_ref(w, f).inside.iter().position(|&v| v == id);
            if on && k.is_none() {
                fog_mut(w, f).inside.push(id);
                if !fog_ref(w, f).buffed.contains(&id) && eligible(w, o) {
                    give_buff(w, f, o);
                }
            } else if !on && let Some(k) = k {
                fog_mut(w, f).inside.remove(k);
                if fog_ref(w, f).buffed.contains(&id) {
                    take_buff(w, f, o);
                }
            }
        }
        put_fighters(w, all);
        follower_overlaps(w, f);
    }
}

/// The Lab reset (`StageEntry::reset`: TrainGameMode destroys the pawns, not the level's actors): the fogs keep their
/// state and Delays; the old pawns leave BuffedFighters and the overlaps (the new pawns are other actors; a new pawn
/// inside a fog on begins its overlap at the next test).
pub fn vents_reset(w: &mut World) {
    for i in own(w).vents.fogs {
        let Some(f) = fog_at(w, i) else { continue };
        let fog = fog_mut(w, f);
        fog.buffed.clear();
        fog.inside.clear();
    }
}

/// PlayerAttachCollision (a sphere of `FOG_RADIUS` at the root; Pawn and HurtBox overlap) against a fighter: his root
/// capsule (Pawn) and his hurt shapes at the current pose (HurtBox), in three dimensions: the fog's root lies 100 cm
/// off the play plane (UE X 100), the fighters on it.
// Unverified: the overlap is the solids' exact one at the pose of the test (as the smoke zone's).
fn touches(w: &mut World, o: FighterId, f: u32) -> bool {
    if w.fighter(o).life.dead > 0.0 {
        return false;
    }
    let (fx, fy) = fog_xy(w, f);
    let fz = fog_ref(w, f).z;
    let r = FOG_RADIUS;
    let (ox, cy) = (w.fighter(o).body.x, actor_y(w, o));
    if (ox - fx).abs() > r + 600.0 || (cy - fy).abs() > r + 600.0 {
        return false;
    }
    if !no_collision(w, o) {
        let cr = cap_radius(w, o);
        let h = max(capsule_half(w, o) - cr, 0.0);
        let dy = max(max(cy - h - fy, 0.0), fy - (cy + h));
        if crate::math::ue::size([fz, ox - fx, dy]) <= r + cr {
            return true;
        }
    }
    if no_hit(w, o) {
        return false;
    }
    let Some(pose) = crate::combat::phases::victim_pose(w, o) else { return false };
    let shape = WorldSphere { x: fx, y: fy, z: Some(fz), r, ..WorldSphere::default() };
    let hit = crate::combat::hit::hurt_hit(w, o, std::slice::from_ref(&shape), &pose).is_some();
    crate::combat::phases::give_vbuf(w, pose);
    hit
}
