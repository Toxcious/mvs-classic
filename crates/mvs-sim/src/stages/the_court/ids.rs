//! The Court's owner and event numbers (`StageTimer::owner`, `StageEntry::timer`'s event). A module of its own that
//! imports nothing.

/// The level script's owner number (`StageTimer::owner`): a placed actor's is its `StageData.placed` index, never
/// negative.
pub const LEVEL_OWNER: i32 = -1;
/// The shot clock's (M010_ShotClock_C, a VFX-level actor the stage bind does not place: `score`).
pub const SHOTCLOCK_OWNER: i32 = -5;
/// The event numbers, by file: `ball` `BALL_EV`.., `pickups` `POWERUP_EV`.., `score` `SCORE_EV`.., `wall` `WALL_EV`.. (a
/// wall's own owner number).
pub const BALL_EV: i32 = 100;
pub const POWERUP_EV: i32 = 200;
pub const SCORE_EV: i32 = 300;
pub const WALL_EV: i32 = 400;
