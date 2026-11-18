//! Throne Room's level script (Map_M008_C).
//! The gameplay level serializes three Map_M008_C actors (Map_M008_C_2, _32, _4) and each runs BeginPlay
//! (AWorldSettings::NotifyBeginPlay has no level script filter): three schedules of Delays, each its own object (owner
//! number `SCRIPT_OWNERS[i]`), whose move calls retarget the same movers. None ticks: every Delay runs in the leftover
//! pass. BeginPlay runs at the load, before tick 1 (`StageEntry::create`), so its Delays count from tick 1 (f180,
//! f2461, f2701); a Delay made in a link counts from the next frame. The schedule runs from BeginPlay over the rounds;
//! only a new World (a new match) restarts it: no ring-out, respawn or reset handler.
//!
//! The level CDO values (Default__Map_M008_C): TimeInOriginalForm 45, AnimLeadTime 4, ElevatorMoveDownTime 65,
//! ElevatorMoveUpTime 18, MainElevatorMovement 6000, PillarMovementDistance 15000; TimeInTransformation is not stored
//! (0). Every sum below is exact in float32.
//!
//! Left out: every PF_Post_Event and the Delays that only lead to one (the move sounds @182-383, @714-1044,
//! @1537-2014, @2440-4158, @4742-5095, @5664-5921, the 38 s rattles @7420 -> @5952): the stage is silent (its events are
//! absent from the paks). CreateUIGlobalWidget(2) (InToCountdown) @7112 and the EnvironmentGameplaySettings.DepthOffset
//! write @7475 are the presentation's. HasAuthority @7326: every peer runs the sim (as BasePlatformBP's), so true.

use crate::Real;
use crate::State;
use crate::fighter::common::warn_text;
use crate::world::World;
use crate::world::stage::{StageOwn, stage_delay};
use crate::world::stage_actors::StageActorOwn;

use super::ThroneRoomOwn;
use super::elevator::{FOREGROUND, PILLAR_LEFT, PILLAR_RIGHT, SIDE_LEFT, SIDE_RIGHT, elevator_move};

/// The three script actors' owner numbers (`StageTimer::owner`), in the level's PersistentLevel.Actors order (_2, _32,
/// _4). Negative and not the game mode's (-2, batcave) nor a platform's (`PLATFORM_OWNER` range).
pub const SCRIPT_OWNERS: [i32; 3] = [-1, -3, -4];
/// This file's event numbers (`StageEntry::timer` ev with a script owner) are `SCRIPT_EV` + the Delay's resume offset in
/// ExecuteUbergraph_Map_M008.
pub const SCRIPT_EV: i32 = 100;
const fn ev(link: i32) -> i32 {
    SCRIPT_EV + link
}

// The level CDO values (above), seconds / cm.
const ORIGINAL_FORM: Real = 45.0;
const ANIM_LEAD: Real = 4.0;
const DOWN_TIME: Real = 65.0;
const UP_TIME: Real = 18.0;
const MAIN_DIST: Real = 6000.0;
const PILLAR_DIST: Real = 15000.0;
const TRANSFORMATION: Real = 0.0;

/// What the drawing reads of one script instance's presentation calls (`M008Presentation`): the world tick of each
/// call, -1 before the first (a later call overwrites it). The ten cached components (CacheOriginalAnimPlayRate @8194:
/// the five mover SkeletalMeshes, the two wall gears M008_Gears_Walls_01_SkelMesh_5 / SkelMesh2,
/// M008_Stage_01_BP_2.Skelmesh and the side alcoves M008_Alcove_01_SkelMesh_2 / SkelMesh2) keep their original play
/// rates, read at BeginPlay. The three instances call at the same ticks (one schedule each): the drawing reads any one.
#[derive(State, Debug)]
pub struct M008Presentation {
    // PlayIntroAnim (@7574, the 3 s Delay @7166 -> @5937), each a nonlooping Play(false) of the placed asset:
    /// M008_Gears_BehindThrone_01_SkelMesh_5 @7644 (f180).
    pub intro_gears: i32,
    /// M008_Center_Alcove_SkelMesh_2 @4558 (Delay 0.5 @7589: f210).
    pub intro_centre: i32,
    /// M008_Alcove_01_SkelMesh_2 @4502 and M008_Alcove_01_SkelMesh2 @4446 (Delay 1.0 @7519: f240).
    pub intro_sides: i32,
    /// M008_Throne_01_SkelMesh_2 @4390 (Delay 0.5 @4335 from there: f270).
    pub intro_throne: i32,
    // PlayTransformAnim (@7700; BeginPlay's Delay 41 @7267 -> @5922 at f2461, then the up request's Delay 59 @4251 ->
    // @4310): ResetAnims (@8105-6524: the ten to position 0 without notifies, SetPlayRate to the cached rate; it stops
    // nothing), then its Delays:
    /// ResetAnims.
    pub transform: i32,
    /// Delay 0.2 @7734 -> @5096: the foreground's SkeletalMesh PlayAnimation(M008_FloorFG_Fold).
    pub floor_fold: i32,
    /// Delay 0.2 @8050 -> @4614: both pillars' PlayAnimation(M008_Tower_01_Fold).
    pub tower_fold: i32,
    /// Delay AnimLeadTime 4 @7991 -> @5290: both wall gears Play(false) (their placed asset).
    pub wall_gears: i32,
    /// Delay 5 @7932 -> @5161: both side elevators' PlayAnimation(M008_Elevator_01_Fold).
    pub elevator_fold: i32,
    /// Delay 7 @7831 -> @5401: Stage Skelmesh SetPosition(0) then PlayAnimation(M008_Stage_01_Fold), both side alcoves
    /// PlayAnimation(M008_Alcove_01_Tower_Fold).
    pub stage_fold: i32,
    /// PlayAnimsInReverse (@9357-7087, at the up request @2015): each of the ten SetPlayRate(-cached rate), Play(false)
    /// from its current position (no reset to an end).
    pub reverse: i32,
}

impl Default for M008Presentation {
    /// `shownNone`.
    fn default() -> Self {
        M008Presentation {
            intro_gears: -1,
            intro_centre: -1,
            intro_sides: -1,
            intro_throne: -1,
            transform: -1,
            floor_fold: -1,
            tower_fold: -1,
            wall_gears: -1,
            elevator_fold: -1,
            stage_fold: -1,
            reverse: -1,
        }
    }
}

/// One Map_M008_C instance's state (`M008Script`).
#[derive(State, Debug, Default)]
pub struct M008Script {
    pub owner: i32,
    pub shown: M008Presentation,
}

fn own(w: &World) -> &ThroneRoomOwn {
    match &w.stage_run.as_ref().expect("a stage state").s {
        StageOwn::ThroneRoom(s) => s,
        _ => panic!("Throne Room: the stage's own state is another stage's"),
    }
}

fn own_mut(w: &mut World) -> &mut ThroneRoomOwn {
    match &mut w.stage_run.as_mut().expect("a stage state").s {
        StageOwn::ThroneRoom(s) => s,
        _ => panic!("Throne Room: the stage's own state is another stage's"),
    }
}

/// `scriptCreate`: the three instances' ReceiveBeginPlay (@7092), in level order: CacheOriginalAnimPlayRate (the
/// drawing's), the 3 s intro Delay @7166 -> @5937, the 41 s Delay @7267 -> @5922 (PlayTransformAnim), and (authority)
/// the 45 s Delay @7361 -> @15 (ThroneRoomMoveLoop).
pub fn script_create(w: &mut World, movers: Vec<i32>) {
    let allow_hazards = w.setup.allow_hazards;
    let scripts = SCRIPT_OWNERS.iter().map(|&owner| M008Script { owner, shown: M008Presentation::default() }).collect();
    w.stage_run.as_mut().expect("a stage state").s =
        StageOwn::ThroneRoom(ThroneRoomOwn { scripts, movers, allow_hazards });
    for owner in SCRIPT_OWNERS {
        stage_delay(w, owner, 7166, 3.0, ev(5937));
        stage_delay(w, owner, 7267, ORIGINAL_FORM - ANIM_LEAD, ev(5922));
        stage_delay(w, owner, 7361, ORIGINAL_FORM, ev(15));
    }
}

/// `move`: ThroneRoomElevatorMove on the mover of a role.
fn move_role(w: &mut World, role: usize, distance: Real, time: Real) {
    let i = own(w).movers[role];
    let slot = u32::try_from(i).ok().and_then(|i| {
        let run = w.stage_run.as_ref()?;
        let k = run.actors.iter().position(|a| a.index == i && matches!(a.own, StageActorOwn::M008Elevator(_)))?;
        u32::try_from(k).ok()
    });
    match slot {
        Some(slot) => elevator_move(w, slot, distance, time),
        None => warn_text(w, &format!("Map_M008: no mover for role {role} in the stage data (its move is not made)")),
    }
}

/// ThroneRoomMoveLoop's body @30: the foreground down, Delay 3 @127 -> @457.
fn move_loop(w: &mut World, owner: i32) {
    move_role(w, FOREGROUND, -PILLAR_DIST, 30.0);
    stage_delay(w, owner, 127, 3.0, ev(457));
}

/// PlayTransformAnim @7700: ResetAnims, then its Delays in their push order (@7734 first, then the pops 8050, 7991,
/// 7932, 7831).
fn play_transform(w: &mut World, owner: i32) {
    let t = w.flow.tick;
    shown_mut(w, owner).transform = t;
    stage_delay(w, owner, 7734, 0.2, ev(5096));
    stage_delay(w, owner, 8050, 0.2, ev(4614));
    stage_delay(w, owner, 7991, ANIM_LEAD, ev(5290));
    stage_delay(w, owner, 7932, ANIM_LEAD + 1.0, ev(5161));
    stage_delay(w, owner, 7831, ANIM_LEAD + 3.0, ev(5401));
}

fn shown_mut(w: &mut World, owner: i32) -> &mut M008Presentation {
    let s = own_mut(w).scripts.iter_mut().find(|s| s.owner == owner).expect("a Throne Room script owner");
    &mut s.shown
}

/// Whether `owner` is one of the level script instances.
pub fn is_script(w: &World, owner: i32) -> bool {
    own(w).scripts.iter().any(|s| s.owner == owner)
}

/// `scriptTimer`: a due Delay of one script instance (ev `SCRIPT_EV` + its resume offset).
pub fn script_timer(w: &mut World, owner: i32, e: i32) {
    let t = w.flow.tick;
    match e - SCRIPT_EV {
        // BeginPlay's 45 s Delay -> @15 ThroneRoomMoveLoop(); the loop's own Delay 63 @2381 -> @30
        15 | 30 => move_loop(w, owner),
        // @457: both pillars down, Delay 2 @659 -> @1118
        457 => {
            move_role(w, PILLAR_LEFT, -PILLAR_DIST, DOWN_TIME);
            move_role(w, PILLAR_RIGHT, -PILLAR_DIST, DOWN_TIME);
            stage_delay(w, owner, 659, 2.0, ev(1118));
        }
        // @1118: both sides down; GetActiveMatchConfigForContext(...).GetAllowHazards() @1325-1422 (Success not tested):
        // false ends the branch (no up, no next loop, no second PlayTransformAnim); true: Delay 65 + 0 @1478 -> @2015
        1118 => {
            move_role(w, SIDE_LEFT, -MAIN_DIST, DOWN_TIME);
            move_role(w, SIDE_RIGHT, -MAIN_DIST, DOWN_TIME);
            if own(w).allow_hazards {
                stage_delay(w, owner, 1478, DOWN_TIME + TRANSFORMATION, ev(2015));
            }
        }
        // @2015: PlayAnimsInReverse, the four up requests and the foreground's, Delay 18 + 45 @2381 -> @30 (the next
        // loop), Delay 18 + 45 - 4 @4251 -> @4310 (PlayTransformAnim)
        // Unverified: this link runs in the frame the side down moves (made at the same @1118 link, 3901 updates each)
        // make their last update; the port updates every move ahead of the due Delays (`stage_latent`), so the sides
        // have reached -6000 and the up requests start from there (back to 0). The other order would retarget each
        // side's move one update short: the up move ending 1.538 cm high, +1.54 cm a cycle.
        2015 => {
            shown_mut(w, owner).reverse = t;
            move_role(w, SIDE_LEFT, MAIN_DIST, UP_TIME);
            move_role(w, SIDE_RIGHT, MAIN_DIST, UP_TIME);
            move_role(w, PILLAR_LEFT, PILLAR_DIST, UP_TIME);
            move_role(w, PILLAR_RIGHT, PILLAR_DIST, UP_TIME);
            move_role(w, FOREGROUND, PILLAR_DIST, 25.0);
            stage_delay(w, owner, 2381, UP_TIME + ORIGINAL_FORM, ev(30));
            stage_delay(w, owner, 4251, UP_TIME + ORIGINAL_FORM - ANIM_LEAD, ev(4310));
        }
        // PlayTransformAnim: BeginPlay's 41 s Delay -> @5922, the up request's 59 s Delay -> @4310
        5922 | 4310 => play_transform(w, owner),
        5096 => shown_mut(w, owner).floor_fold = t,
        4614 => shown_mut(w, owner).tower_fold = t,
        5290 => shown_mut(w, owner).wall_gears = t,
        5161 => shown_mut(w, owner).elevator_fold = t,
        5401 => shown_mut(w, owner).stage_fold = t,
        // PlayIntroAnim @7574 (BeginPlay's 3 s Delay -> @5937): Delay 1.0 @7519 -> @4325, the behind-throne gears now,
        // Delay 0.5 @7589 -> @4558
        5937 => {
            stage_delay(w, owner, 7519, 1.0, ev(4325));
            shown_mut(w, owner).intro_gears = t;
            stage_delay(w, owner, 7589, 0.5, ev(4558));
        }
        // @4325: Delay 0.5 @4335 -> @4390, both side alcoves now
        4325 => {
            stage_delay(w, owner, 4335, 0.5, ev(4390));
            shown_mut(w, owner).intro_sides = t;
        }
        4390 => shown_mut(w, owner).intro_throne = t,
        4558 => shown_mut(w, owner).intro_centre = t,
        _ => warn_text(w, &format!("Map_M008: a Delay link it does not have ({e})")),
    }
}
