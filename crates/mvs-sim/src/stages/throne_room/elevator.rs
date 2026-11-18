//! Throne Room's movers (M008_Elevator_BP_C: the left / right side elevators, the left /
//! right pillars and the foreground elevator; decoded M008_Elevator_BP). One local event, ThroneRoomElevatorMove(distance,
//! time): UpdateIntersectionsEveryFrame on, MoveComponentTo of the Scene root to its current location + (0, 0, distance)
//! over abs(time) (linear, relative, unswept: `stage_move_to` on its Box solid), the link @336 -> @15 turns the updating
//! off. No tick, no BeginPlay, no hit, no reset: the level script (`script.rs`) calls the moves. The class's
//! ElevatorTravelDistance / ElevatorTravelTime (1605 / 15, the sides' 3500 and the pillars' 6500) are read nowhere: the
//! level passes its own.

use std::sync::Arc;

use crate::Real;
use crate::State;
use crate::actors::StageActorClass;
use crate::fighter::common::warn_text;
use crate::registry::entries::StageActorEntry;
use crate::world::World;
use crate::world::stage::{StagePiece, stage_move_to};
use crate::world::stage_actors::StageActorOwn;

pub const ELEVATOR: &str = "M008_Elevator_BP_C";
/// The move's link (MoveComponentTo @336 -> @15), the `StageEntry::timer` ev with the mover's owner number.
pub const MOVE_LINK: i32 = 15;

// Which of the five the level names (Default__Map_M008_C's actor references, by the placed actor's name).
/// ThroneRoomLeftElevator_7.
pub const SIDE_LEFT: usize = 0;
/// ThroneRoomRightElevator_9.
pub const SIDE_RIGHT: usize = 1;
/// ThroneRoomLeftPillar_8.
pub const PILLAR_LEFT: usize = 2;
/// ThroneRoomRightPillar_10.
pub const PILLAR_RIGHT: usize = 3;
/// ThroneRoomForegroundElevator.
pub const FOREGROUND: usize = 4;
pub const ROLE_NAMES: [&str; 5] = [
    "ThroneRoomLeftElevator_7",
    "ThroneRoomRightElevator_9",
    "ThroneRoomLeftPillar_8",
    "ThroneRoomRightPillar_10",
    "ThroneRoomForegroundElevator",
];

/// An M008_Elevator_BP_C in the match (`M008Elevator`): its own state, its `StageActorOwn` variant.
#[derive(State, Debug, Default)]
pub struct M008Elevator {
    /// Its ROLE_NAMES index, -1 for a name the level script does not reference.
    pub role: i32,
    /// Its Box's StageData.solids index (-1 when the stage data has none).
    pub floor: i32,
    /// Its DefaultSceneRoot's level Z (UE), float32: the root's place is `root_z` + the Box solid's dy (the Box is
    /// attached to the root with no other motion).
    pub root_z: Real,
    /// Comp_EdgeTrace.UpdateIntersectionsEveryFrame (on from a move call to its link; read by nothing: the edges take
    /// an elevator's crossings at its place now, unverified, as Scooby's elevator's).
    pub edge_update: bool,
}

/// M008_Elevator_BP_C's entry.
pub fn entry() -> StageActorEntry {
    StageActorEntry {
        class: StageActorClass::M008ElevatorBP,
        hurt_type: None,
        friendly_fire: false,
        can_take_damage: true,
        hit_pause_multiplier: None,
        surface: None,
        create: Some(create),
        any_damage: None,
        hit_by_enemy: None,
        hit_by_ally: None,
        knocked_back: None,
        hurt: None,
        sounds: &[],
        damage_disabled: None,
        invulnerable: None,
        current_damage: None,
        victim_pause: None,
        mesh: None,
        reset: None,
        solid_hit: None,
    }
}

/// The placed actor's Box (the one solid named `<actor>.Box`): its StageData.solids index, -1 none.
fn box_of(w: &World, d: &mvs_data::stage::StagePlacedData) -> i32 {
    let all = w.data.stage.solids.as_deref().unwrap_or(&[]);
    let want = format!("{}.Box", d.name);
    let id = d
        .solids
        .iter()
        .flatten()
        .copied()
        .find(|&i| usize::try_from(i).ok().and_then(|i| all.get(i)).is_some_and(|s| s.name == want));
    match id {
        Some(id) => id,
        None => {
            warn_text(
                w,
                &format!("M008_Elevator_BP_C {}: no Box solid in the stage data (its moves are not made)", d.name),
            );
            -1
        }
    }
}

fn create(w: &mut World, slot: u32) {
    let data = Arc::clone(&w.data);
    let a = &w.stage_run.as_ref().expect("a stage state").actors[slot as usize];
    let d = &data.stage.placed.as_deref().unwrap_or(&[])[a.index as usize];
    let own = M008Elevator {
        role: ROLE_NAMES.iter().position(|n| *n == d.name).map_or(-1, |i| i32::try_from(i).expect("a role")),
        floor: box_of(w, d),
        root_z: crate::math::narrow(d.loc[2]),
        edge_update: false,
    };
    let a = &mut w.stage_run.as_mut().expect("a stage state").actors[slot as usize];
    a.x = crate::math::narrow(-d.loc[1]);
    a.y = crate::math::narrow(d.loc[2]);
    a.own = StageActorOwn::M008Elevator(own);
}

/// `elevatorMove`: ThroneRoomElevatorMove(distance, time) (@49-336): Comp_EdgeTrace.UpdateIntersectionsEveryFrame =
/// true, then MoveComponentTo(DefaultSceneRoot, (X, Y, GetComponentLocation.Z + distance), rotation zero, no eases,
/// Abs(time), Move) with the mover as the callback target (owner: its placed index). A call while its action runs
/// retargets it from where it is (`ComponentMove::call`: the three level script instances' calls in one pass give the
/// same target). The Z sum is the Blueprint's float32 Add_FloatFloat.
pub fn elevator_move(w: &mut World, slot: u32, distance: Real, time: Real) {
    let run = w.stage_run.as_mut().expect("a stage state");
    let a = &mut run.actors[slot as usize];
    let index = a.index;
    let StageActorOwn::M008Elevator(e) = &mut a.own else { panic!("stage actor {slot} is no M008Elevator") };
    e.edge_update = true;
    let (floor, root_z) = (e.floor, e.root_z);
    let Some(s) = usize::try_from(floor).ok().and_then(|i| run.solids.get(i)) else { return };
    let (dx, dy) = (s.m.dx, s.m.dy);
    let z = (root_z + dy) + distance;
    let over = time.abs();
    let piece = StagePiece::Solid(u32::try_from(floor).expect("a solid id"));
    stage_move_to(
        w,
        piece,
        dx,
        z - root_z,
        over,
        i32::try_from(index).expect("a placed index"),
        MOVE_LINK,
        0,
        false,
        false,
    );
}

/// `elevatorMoveDone`: the move's link @15: Comp_EdgeTrace.UpdateIntersectionsEveryFrame = false.
pub fn elevator_move_done(w: &mut World, slot: u32) {
    let a = &mut w.stage_run.as_mut().expect("a stage state").actors[slot as usize];
    if let StageActorOwn::M008Elevator(e) = &mut a.own {
        e.edge_update = false;
    }
}
