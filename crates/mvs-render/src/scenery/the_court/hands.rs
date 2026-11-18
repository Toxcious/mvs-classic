//! The Court's clock (M010_BombFactory_ClockHands_C), drawn from the block's
//! `clockHands`. Offsets are the ubergraph's.
//! - BeginPlay @950 (PandaGameState cast: the Lab's TrainingGameState is one too): SetHandsRandomRotation @1627 /
//!   @1764, each hand's relative roll RandomFloatInRange(0, 360) (the scenery's unseeded stream, SceneryHost::rand: no
//!   part of the simulation), then UpdateLifeCount bound @358. HandAInitialRotX / HandBInitialRotX stay 0.
//! - ReceiveTick @1534: while not IsRotatingHands, DeltaTime += DeltaSeconds and UpdateMatchTimer @439: with
//!   DeltaTime at 0.4 or more the A saved roll + 12 and the B one - 8 are set as the rolls (so the first step after BeginPlay goes from
//!   the random rolls to 12 / -8, as the build's) and DeltaTime = 0 (one step, the rest lost). Float32 throughout.
//! - OnUpdateLifeCount @1912 -> RotateHands(770, -770) @1099: GetInitialHandsRotation (the saved rolls := the rolls),
//!   IsRotatingHands, both done flags false, Rotate Hand A.PlayFromStart @325, Delay(0.5) @885 (one pending: a second
//!   call while it waits adds none) -> @100 Rotate Hand B.PlayFromStart. Each update @133 / @1151: roll = the
//!   argument x EaseInOut + the saved roll; each Finished @423 / @15 sets its done flag, and both done capture the
//!   rolls again and clear IsRotatingHands.
//! - The timelines (FTimeline::TickTimeline 142e46330): position += dt (float32), the update each tick, finished
//!   once the position goes strictly past the length (A 4 s, B 5 s: its length unstored, the component default
//!   142e34d70), clamped there.
//! - What broadcasts UpdateLifeCount: PandaGameState's UpdateScoreUI @16191, which PandaGameMode calls in
//!   StartNewMatch for every team (@2099, PandaGameMode BeginPlay's Delay(0.2) -> StartNewMatch: the Lab's too) and
//!   in UpdateTeamScore (@1049, FreeForAll every team @2148). Here: a RotateHands at the scenery's 0.2 s Delay from its begin (the level's
//!   BeginPlay), after the tick groups, as a Delay's.
//! - The hands' relative rolls under the actor's root: each drawn as its instance (placed at the template's roll
//!   `roll`) x a roll by the difference (the hand has no other relative rotation, scale 1), the latest two ticks'
//!   blended by the frame's alpha.
// Unverified (as batcave's motion): PlayFromStart setting 0 and firing the update at once is 4.26 source; the
// timelines tick after the actor's own tick (both TG_PrePhysics, no prerequisite).
// Deviation: UpdateTeamScore's RotateHands is one in a tick whose World's team knockouts differ from the last tick's
// (a tick with no World forgets them): an UpdateTeamScore that leaves every score as it was
// (FreeForAll's tie re-pick, updateTeamScore(team, 0)) turns nothing; a rollback that undoes a score turns the hands
// again; the first World tick seen (a scenery joining a match under way) turns nothing.

use mvs_sim::fighter::common::latent_frames;
use mvs_view::M4;
use serde::Deserialize;

use crate::Result;
use crate::scenery::host::{InstanceId, SceneryHost};
use crate::scenery::{CurveKey, rich_curve};
use crate::stage::{bad, transform};
use crate::view::mul;

/// PandaGameMode ReceiveBeginPlay's Delay before StartNewMatch.
const START_NEW_MATCH_DELAY: f64 = 0.2;

#[derive(Debug, Deserialize)]
pub(super) struct Timeline {
    length: f64,
    keys: Vec<CurveKey>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Pair {
    a: f64,
    b: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct Idle {
    every: f64,
    a: f64,
    b: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Rotate {
    a: f64,
    b: f64,
    delay_b: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct Timelines {
    a: Timeline,
    b: Timeline,
}

#[derive(Debug, Deserialize)]
pub(super) struct HandsBlock {
    a: String,
    b: String,
    roll: Pair,
    random: [f64; 2],
    idle: Idle,
    rotate: Rotate,
    timelines: Timelines,
}

/// Math.fround.
fn f(x: f64) -> f64 {
    f64::from(x as f32)
}

#[derive(Debug)]
struct Hand {
    id: InstanceId,
    /// The instance's roll (degrees).
    placed: f64,
    /// The instance's UE transform.
    base: M4,
    /// The relative roll after the latest tick, and the one before.
    roll: f64,
    prev: f64,
    /// HandAInitialRotX / HandBInitialRotX.
    saved: f64,
    /// RotateHands' degrees.
    arg: f64,
    length: f64,
    keys: Vec<CurveKey>,
    /// The timeline's position.
    pos: f64,
    playing: bool,
    done: bool,
}

impl Hand {
    fn update(&mut self) {
        self.roll = f(f(self.arg * f(rich_curve(&self.keys, self.pos))) + self.saved);
    }

    fn play_from_start(&mut self) {
        self.pos = 0.0;
        self.playing = true;
        self.update();
    }

    fn draw(&self, host: &mut SceneryHost, deg: f64) {
        let turn = transform([0.0; 3], [0.0, 0.0, deg - self.placed], [1.0; 3]);
        host.place(self.id, Some(mul(&turn, &self.base)));
    }
}

#[derive(Debug)]
pub(super) struct Hands {
    a: Hand,
    b: Hand,
    random: [f64; 2],
    every: f64,
    idle: [f64; 2],
    rotate: [f64; 2],
    b_delay: u32,
    start_at: u32,
    /// DeltaTime.
    delta: f64,
    /// IsRotatingHands.
    rotating: bool,
    /// The Delay's frames left; None: none pending.
    wait_b: Option<u32>,
    /// Ticks since begin.
    ticks: u32,
    /// The World's team knockouts last seen; None: no World in the last tick.
    scores: Option<Vec<f64>>,
}

impl Hands {
    pub(super) fn make(d: HandsBlock, host: &SceneryHost) -> Result<Hands> {
        let hand = |name: &str, placed: f64, tl: Timeline| -> Result<Hand> {
            let id = host
                .instance(name)
                .ok_or_else(|| bad("scenery: the court", format!("the clock hand {name} is not in the file")))?;
            let i = &host.instances()[id.0 as usize];
            Ok(Hand {
                id,
                placed,
                base: transform(i.loc, i.rot, i.scale),
                roll: placed,
                prev: placed,
                saved: 0.0,
                arg: 0.0,
                length: tl.length,
                keys: tl.keys,
                pos: 0.0,
                playing: false,
                done: false,
            })
        };
        let frames = |s: f64| latent_frames(mvs_sim::math::narrow(s)) as u32;
        Ok(Hands {
            a: hand(&d.a, d.roll.a, d.timelines.a)?,
            b: hand(&d.b, d.roll.b, d.timelines.b)?,
            random: d.random,
            every: f(d.idle.every),
            idle: [d.idle.a, d.idle.b],
            rotate: [d.rotate.a, d.rotate.b],
            b_delay: frames(d.rotate.delay_b),
            start_at: frames(START_NEW_MATCH_DELAY),
            delta: 0.0,
            rotating: false,
            wait_b: None,
            ticks: 0,
            scores: None,
        })
    }

    /// The World's team knockouts against the last tick's (a length change counts); them kept.
    fn scores_changed(&mut self, k: &[f64]) -> bool {
        let Some(scores) = &mut self.scores else {
            self.scores = Some(k.to_vec());
            return false;
        };
        let changed = scores.len() != k.len() || scores.iter().zip(k).any(|(a, b)| a != b);
        scores.clear();
        scores.extend_from_slice(k);
        changed
    }

    fn capture(&mut self) {
        self.a.saved = self.a.roll;
        self.b.saved = self.b.roll;
    }

    fn finished(&mut self) {
        if self.a.done && self.b.done {
            self.capture();
            self.rotating = false;
        }
    }

    fn rotate_hands(&mut self) {
        self.a.arg = self.rotate[0];
        self.b.arg = self.rotate[1];
        self.capture();
        self.rotating = true;
        self.a.done = false;
        self.b.done = false;
        self.a.play_from_start();
        if self.wait_b.is_none() {
            self.wait_b = Some(self.b_delay);
        }
    }

    pub(super) fn begin(&mut self, host: &mut SceneryHost) {
        for h in [&mut self.a, &mut self.b] {
            // SetHandsRandomRotation
            h.roll = f(host.rand(self.random));
            h.prev = h.roll;
            h.saved = 0.0;
            h.arg = 0.0;
            h.pos = 0.0;
            h.playing = false;
            h.done = false;
            h.draw(host, h.roll);
        }
        self.delta = 0.0;
        self.rotating = false;
        self.wait_b = None;
        self.ticks = 0;
        self.scores = None;
    }

    /// One tick of `dt` seconds; `knockouts`: the World's team knockouts (None outside a match of the stage).
    pub(super) fn tick(&mut self, dt: f64, knockouts: Option<&[f64]>) {
        self.a.prev = self.a.roll;
        self.b.prev = self.b.roll;
        self.ticks += 1;
        // ReceiveTick @1534
        if !self.rotating {
            self.delta = f(self.delta + f(dt));
            if self.delta >= self.every {
                self.a.saved = f(self.a.saved + self.idle[0]);
                self.a.roll = self.a.saved;
                self.b.saved = f(self.b.saved + self.idle[1]);
                self.b.roll = self.b.saved;
                self.delta = 0.0;
            }
        }
        // the timelines
        for k in 0..2 {
            let h = if k == 0 { &mut self.a } else { &mut self.b };
            if !h.playing {
                continue;
            }
            h.pos = f(h.pos + f(dt));
            if h.pos > h.length {
                h.pos = h.length;
                h.playing = false;
                h.update();
                h.done = true;
                self.finished();
            } else {
                h.update();
            }
        }
        // after the tick groups: the Delay before B, StartNewMatch's UpdateLifeCount
        if let Some(w) = self.wait_b {
            let w = w.saturating_sub(1);
            if w == 0 {
                self.wait_b = None;
                self.b.play_from_start();
            } else {
                self.wait_b = Some(w);
            }
        }
        if self.ticks == self.start_at {
            self.rotate_hands();
        }
        match knockouts {
            None => self.scores = None,
            Some(k) => {
                if self.scores_changed(k) {
                    self.rotate_hands();
                }
            }
        }
    }

    pub(super) fn frame(&self, host: &mut SceneryHost, alpha: f64) {
        for h in [&self.a, &self.b] {
            let mut dl = h.roll - h.prev;
            // (Math.round)
            dl -= 360.0 * (dl / 360.0 + 0.5).floor();
            h.draw(host, h.prev + dl * alpha);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> HandsBlock {
        serde_json::from_value(serde_json::json!({
            "a": "A", "b": "B",
            "roll": { "a": -60, "b": -160 },
            "random": [0, 360],
            "idle": { "every": 0.4, "a": 12, "b": -8 },
            "rotate": { "a": 770, "b": -770, "delayB": 0.5 },
            "timelines": {
                "a": { "length": 4, "keys": [[0, 0, 0, 0, 1], [4, 1, 0, 0, 1]] },
                "b": { "length": 5, "keys": [[0, 0, 0, 0, 1], [5, 1, 0, 0, 1]] }
            }
        }))
        .unwrap()
    }

    fn hand(roll: f64) -> Hand {
        Hand {
            id: InstanceId(0),
            placed: roll,
            base: crate::view::identity(),
            roll,
            prev: roll,
            saved: 0.0,
            arg: 0.0,
            length: 0.0,
            keys: Vec::new(),
            pos: 0.0,
            playing: false,
            done: false,
        }
    }

    /// The idle steps, StartNewMatch's RotateHands at its Delay, B half a second after A, both done at their
    /// timelines' ends with the saved rolls captured.
    #[test]
    fn hands_step_and_rotate() {
        let d = block();
        let mut h = Hands {
            a: Hand { length: 4.0, keys: d.timelines.a.keys.clone(), ..hand(-60.0) },
            b: Hand { length: 5.0, keys: d.timelines.b.keys.clone(), ..hand(-160.0) },
            random: d.random,
            every: f(0.4),
            idle: [12.0, -8.0],
            rotate: [770.0, -770.0],
            b_delay: latent_frames(0.5) as u32,
            start_at: latent_frames(mvs_sim::math::narrow(START_NEW_MATCH_DELAY)) as u32,
            delta: 0.0,
            rotating: false,
            wait_b: None,
            ticks: 0,
            scores: None,
        };
        assert_eq!((h.start_at, h.b_delay), (12, 30));
        let dt = 1.0 / 60.0;
        for _ in 0..11 {
            h.tick(dt, None);
        }
        assert!(!h.rotating);
        h.tick(dt, None);
        assert!(h.rotating && h.a.playing && !h.b.playing);
        for _ in 0..30 {
            h.tick(dt, None);
        }
        assert!(h.b.playing);
        // A ends after 4 s of its timeline, B 0.5 s after it
        for _ in 0..(5 * 60 + 30) {
            h.tick(dt, None);
        }
        assert!(!h.rotating && h.a.done && h.b.done);
        assert_eq!((h.a.saved, h.b.saved), (h.a.roll, h.b.roll));
    }

    /// A change of the team knockouts (not the first look, not a repeat) is what turns the hands.
    #[test]
    fn score_change_turns_the_hands() {
        let d = block();
        let mut h = Hands {
            a: Hand { length: 4.0, keys: d.timelines.a.keys.clone(), ..hand(-60.0) },
            b: Hand { length: 5.0, keys: d.timelines.b.keys.clone(), ..hand(-160.0) },
            random: d.random,
            every: f(0.4),
            idle: [12.0, -8.0],
            rotate: [770.0, -770.0],
            b_delay: 30,
            start_at: 1000,
            delta: 0.0,
            rotating: false,
            wait_b: None,
            ticks: 0,
            scores: None,
        };
        assert!(!h.scores_changed(&[0.0, 0.0]));
        assert!(!h.scores_changed(&[0.0, 0.0]));
        assert!(h.scores_changed(&[1.0, 0.0]));
        assert!(!h.scores_changed(&[1.0, 0.0]));
        assert!(h.scores_changed(&[1.0]));
        h.tick(1.0 / 60.0, Some(&[2.0]));
        assert!(h.rotating && h.a.playing);
    }
}
