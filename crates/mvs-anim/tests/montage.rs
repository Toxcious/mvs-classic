//! The montage player over mvs-data's montage records: queued and branching-point notifies, section links, and a
//! state's field-wise `clone_from`.

use mvs_anim::montage::{MontagePlayer, MontageState, NotifySink};
use mvs_data::fighter::{Montage, Notify, NotifyTick, Section};

const DT: f64 = 1.0 / 60.0;

fn notify(cls: &str, t: f64, dur: f64, state: bool, tick: NotifyTick) -> Notify {
    Notify {
        cls: cls.to_string(),
        inst: None,
        t,
        dur,
        state,
        tick,
        track: 0,
        p: Default::default(),
        seq: None,
        slot: None,
    }
}

fn montage(length: f64, sections: Vec<Section>, notifies: Vec<Notify>) -> Montage {
    Montage {
        name: "Test".to_string(),
        length,
        blend_in: 0.0,
        blend_out: 0.25,
        auto_blend_out: true,
        blend_out_trigger_time: -1.0,
        blend_out_option: None,
        rate_scale: 1.0,
        sections,
        slots: Vec::new(),
        notifies,
        group: None,
        holds_source: None,
        keeps_nav: None,
        skin_slot_notifies: Vec::new(),
    }
}

fn section(name: &str, t: f64, next: Option<&str>) -> Section {
    Section { name: name.to_string(), next: next.map(str::to_string), t }
}

/// What the player called, in order, with the position at the call.
#[derive(Default)]
struct Log {
    calls: Vec<(&'static str, usize, f64)>,
}

impl NotifySink<u32> for Log {
    fn begin(&mut self, p: &mut MontagePlayer<'_, u32>, idx: usize) {
        self.calls.push(("begin", idx, p.s.pos));
    }
    fn end(&mut self, p: &mut MontagePlayer<'_, u32>, idx: usize) {
        self.calls.push(("end", idx, p.s.pos));
    }
    fn instant(&mut self, p: &mut MontagePlayer<'_, u32>, idx: usize) {
        self.calls.push(("instant", idx, p.s.pos));
    }
}

fn kinds(log: &Log) -> Vec<(&'static str, usize)> {
    log.calls.iter().map(|&(k, i, _)| (k, i)).collect()
}

#[test]
fn a_queued_state_begins_and_ends_once_over_its_window() {
    let m =
        montage(1.0, vec![section("Default", 0.0, None)], vec![notify("State", 0.1, 0.1, true, NotifyTick::Queued)]);
    let mut s = MontageState::new(0u32, true);
    let mut log = Log::default();
    let mut on_at = None;
    for frame in 0..20 {
        assert!(MontagePlayer { m: &m, s: &mut s }.advance(DT, &mut log, false, false));
        if s.has(0) && on_at.is_none() {
            on_at = Some(frame);
        }
    }
    assert_eq!(kinds(&log), vec![("begin", 0), ("end", 0)]);
    // The window [0.1, 0.2] at 60 Hz: on from the step that passes 0.1 until the one that passes 0.2.
    // (the clock's EPS: six frames of 1/60 reach 0.1 within it; the float32 position runs up to 2e-9 past a frame)
    let begin_pos = log.calls[0].2;
    let end_pos = log.calls[1].2;
    // the positions of the two steps that pass 0.1 and 0.2 (float32 sums of 1/60, widened): 6 and 13 frames
    assert_eq!((begin_pos, end_pos), (0.10000000149011612, 0.21666666865348816));
    assert_eq!(on_at, Some(5));
    assert!(!s.has(0));
}

#[test]
fn a_branching_point_fires_inside_the_step_a_queued_one_after_it() {
    let m = montage(
        1.0,
        vec![section("Default", 0.0, None)],
        vec![
            notify("Queued", 0.05, 0.0, false, NotifyTick::Queued),
            notify("Branch", 0.05, 0.0, false, NotifyTick::BranchingPoint),
        ],
    );
    let mut s = MontageState::new(0u32, true);
    let mut log = Log::default();
    let mut fired = None;
    for _ in 0..10 {
        let mut p = MontagePlayer { m: &m, s: &mut s };
        let h = p.advance_only(DT, &mut log, false, false).expect("still playing");
        if !log.calls.is_empty() && fired.is_none() {
            // The branching point is in, the queued instant not yet: it waits for the dispatch.
            fired = Some(kinds(&log));
        }
        assert!(p.dispatch_advanced(&h, &mut log));
    }
    assert_eq!(fired, Some(vec![("instant", 1)]));
    assert_eq!(kinds(&log), vec![("instant", 1), ("instant", 0)]);
}

#[test]
fn a_notify_at_zero_fires_on_the_first_tick() {
    let m =
        montage(1.0, vec![section("Default", 0.0, None)], vec![notify("Start", 0.0, 0.0, false, NotifyTick::Queued)]);
    let mut s = MontageState::new(0u32, true);
    let mut log = Log::default();
    MontagePlayer { m: &m, s: &mut s }.advance(DT, &mut log, false, false);
    assert_eq!(kinds(&log), vec![("instant", 0)]);
}

#[test]
fn a_section_that_links_to_itself_loops() {
    let m = montage(
        0.5,
        vec![section("Loop", 0.0, Some("Loop"))],
        vec![notify("Mark", 0.25, 0.0, false, NotifyTick::Queued)],
    );
    let mut s = MontageState::new(0u32, true);
    let mut log = Log::default();
    for _ in 0..40 {
        assert!(MontagePlayer { m: &m, s: &mut s }.advance(DT, &mut log, false, false));
    }
    // 40 frames: 0.667 s over a 0.5 s loop.
    // 40 float32 frames of 1/60 less the 0.5 s loop
    assert_eq!(s.pos, 0.16666685044765472);
    assert!(!s.at_end);
    assert_eq!(kinds(&log), vec![("instant", 0)]);
    for _ in 0..30 {
        MontagePlayer { m: &m, s: &mut s }.advance(DT, &mut log, false, false);
    }
    assert_eq!(kinds(&log), vec![("instant", 0), ("instant", 0)]);
}

#[test]
fn the_last_section_end_blends_the_montage_out() {
    let m = montage(0.25, vec![section("Default", 0.0, None)], Vec::new());
    let mut s = MontageState::new(0u32, true);
    let mut log = Log::default();
    let mut frames = 0;
    while (MontagePlayer { m: &m, s: &mut s }).advance(DT, &mut log, false, false) {
        frames += 1;
        assert!(frames < 120, "the montage never ended");
    }
    assert!(s.ended);
    assert!(s.blend_time == m.blend_out);
}

#[test]
fn clone_from_copies_every_field() {
    let m =
        montage(1.0, vec![section("Default", 0.0, None)], vec![notify("State", 0.0, 0.5, true, NotifyTick::Queued)]);
    let mut a = MontageState::new(7u32, true);
    let mut log = Log::default();
    for _ in 0..5 {
        MontagePlayer { m: &m, s: &mut a }.advance(DT, &mut log, false, false);
    }
    a.play_rate = 1.5;
    let mut b = MontageState::new(0u32, false);
    b.active.reserve(4);
    let cap = b.active.capacity();
    b.clone_from(&a);
    assert_eq!(a, b);
    assert_eq!(b.active.capacity(), cap, "the save reuses the list");
    assert_eq!(a.clone(), a);
}
