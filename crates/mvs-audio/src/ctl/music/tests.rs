//! The music against what the oracle's music did on the same hierarchies (run in node on a fake audio context:
//! its source starts, stops and fade events, in seconds; the numbers below are its).

use std::collections::BTreeMap;

use mvs_data::sound::{
    AutoType, Bus, Clip, ClipAuto, Curve, DecisionNode, EntryType, Id, MediaRef, Meter, MusicFade, MusicItem,
    MusicRanSeq, MusicRule, MusicSegment, MusicSwitch, MusicTrack, Node, Obj, Props, Pt, Rtpc, RuleDst, RuleSrc,
    SyncType,
};

use super::{Music, World, iter};
use crate::ctl::samples;
use crate::mix::auto::Auto;
use crate::spec::{Cancel, Control, Param, ParamChange, Ramp, Target, VoiceId};

struct Tw {
    objs: BTreeMap<Id, Obj>,
    media: BTreeMap<Id, MediaRef>,
    switch: BTreeMap<Id, Id>,
    buses: BTreeMap<Id, Bus>,
    rand: Vec<f64>,
    drawn: usize,
    controls: Vec<Control>,
    /// The audio clock (samples) each control was emitted at, and the clock now.
    sent: Vec<u64>,
    now: u64,
    lines: Vec<(f64, Id, String)>,
    /// The decisions are not logged (a log line allocates).
    quiet: bool,
    warns: Vec<&'static str>,
    voices: u32,
}

impl World for Tw {
    fn obj(&self, id: Id) -> Option<&Obj> {
        self.objs.get(&id)
    }
    fn media(&self, id: Id) -> Option<&MediaRef> {
        self.media.get(&id)
    }
    fn switch(&self, group: Id) -> Option<Id> {
        self.switch.get(&group).copied()
    }
    fn bus(&self, id: Id) -> Option<&Bus> {
        self.buses.get(&id)
    }
    fn master(&self) -> Option<Id> {
        None
    }
    fn rtpc_db(&self, _: &[Rtpc]) -> f64 {
        0.0
    }
    fn touch(&mut self, _: Id) {}
    fn emit(&mut self, c: Control) {
        self.controls.push(c);
        self.sent.push(self.now);
    }
    fn voice(&mut self) -> VoiceId {
        self.voices += 1;
        VoiceId(self.voices)
    }
    fn draw(&mut self) -> f64 {
        let v = self.rand[self.drawn % self.rand.len()];
        self.drawn += 1;
        v
    }
    fn line(&mut self, t: f64, node: Id, what: &str) {
        if !self.quiet {
            self.lines.push((t, node, what.to_owned()));
        }
    }
    fn warn(&mut self, what: &'static str, _: u64) {
        self.warns.push(what);
    }
}

fn meter() -> Meter {
    Meter { tempo: 120.0, beats: 4, beat_value: 4, grid_period: 0.0, grid_offset: 0.0 }
}

fn seg(dur: f64, entry: f64, exit: f64, tracks: Vec<Id>) -> Obj {
    Obj::MusicSegment(MusicSegment { node: Node::default(), children: tracks, meter: Some(meter()), dur, entry, exit })
}

fn track(src: Id, dur: f64, auto: Vec<ClipAuto>, loops: Option<f64>) -> Obj {
    let mut node = Node::default();
    if let Some(l) = loops {
        node.p = Props(vec![(58, l)]);
    }
    let clips = vec![Clip { src, play_at: 0.0, begin_trim: 0.0, end_trim: 0.0, src_dur: dur, held: 1 }];
    Obj::MusicTrack(MusicTrack { node, clips, auto, look: 0.0 })
}

fn item(seg: Id) -> MusicItem {
    MusicItem { seg, id: seg * 10, loop_count: 1, weight: 50_000, ..MusicItem::default() }
}

fn group(rs: u8, children: Vec<MusicItem>, shuffle: bool) -> MusicItem {
    MusicItem { seg: 0, id: 77, loop_count: 0, weight: 50_000, rs, shuffle, children, ..MusicItem::default() }
}

fn playlist(root: MusicItem) -> Obj {
    let children = leaves(&root);
    let node = Node { parent: Some(1), ..Node::default() };
    Obj::MusicRandomSequenceContainer(MusicRanSeq { node, children, meter: None, list: vec![root], rules: vec![] })
}

fn fade(t: f64, off: f64, curve: Curve) -> MusicFade {
    MusicFade { t, off, curve }
}

fn rule(s: RuleSrc, d: RuleDst) -> MusicRule {
    MusicRule { src: vec![-1], dst: vec![-1], s, d, trans: None }
}

fn src_side(t: f64, off: f64, curve: Curve, sync: SyncType) -> RuleSrc {
    RuleSrc { fade: fade(t, off, curve), sync, post: false }
}

fn dst_side(t: f64, off: f64, curve: Curve, pre: bool) -> RuleDst {
    RuleDst { fade: fade(t, off, curve), entry: EntryType::EntryMarker, pre }
}

/// The hierarchy of the probe: a switch on group 100 (10 -> playlist 2 of segments 4, 5; 20 -> playlist 3 of segment 5).
fn hierarchy(rules: Vec<MusicRule>, t6: Obj) -> BTreeMap<Id, Obj> {
    let mut o = BTreeMap::new();
    let tree = |key, node| DecisionNode { key, node, children: vec![] };
    let root = MusicSwitch { args: vec![100], tree: vec![tree(10, 2), tree(20, 3)], rules, ..MusicSwitch::default() };
    o.insert(1, Obj::MusicSwitchContainer(root));
    o.insert(2, playlist(group(0, vec![item(4), item(5)], false)));
    o.insert(3, playlist(group(0, vec![item(5)], false)));
    o.insert(4, seg(8000.0, 500.0, 7500.0, vec![6]));
    o.insert(5, seg(6000.0, 0.0, 5500.0, vec![7]));
    o.insert(6, t6);
    o.insert(7, track(901, 6000.0, vec![], None));
    o
}

fn world(objs: BTreeMap<Id, Obj>, rand: Vec<f64>) -> Tw {
    let mut media = BTreeMap::new();
    for id in [900, 901, 902] {
        media.insert(id, MediaRef { frames: 100 * 48_000, rate: 48_000, channels: 2, ..MediaRef::default() });
    }
    let switch = BTreeMap::from([(100, 10)]);
    Tw {
        objs,
        media,
        switch,
        buses: BTreeMap::new(),
        rand,
        drawn: 0,
        controls: vec![],
        sent: vec![],
        now: 0,
        lines: vec![],
        quiet: false,
        warns: vec![],
        voices: 0,
    }
}

/// The probe's clock: every 0.1 s from `from` to `until`, rounded as it rounds.
fn drive(m: &mut Music, w: &mut Tw, from: f64, until: f64) {
    let mut t = from;
    while t <= until + 1e-9 {
        w.now = samples(t);
        m.tick(w, t);
        t = ((t + 0.1) * 1e6).round() / 1e6;
    }
}

fn switch_to(m: &mut Music, w: &mut Tw, group: Id, value: Id, now: f64) {
    w.switch.insert(group, value);
    w.now = samples(now);
    m.switch_changed(w, now);
}

/// What the controls say of one voice.
struct V {
    id: VoiceId,
    at: u64,
    offset: u32,
    loops: u16,
    loop_points: Option<[u32; 2]>,
    dist: f64,
    stop: Option<u64>,
    param: Vec<(Param, ParamChange)>,
}

impl V {
    fn fades(&self) -> Vec<(u64, f64, Ramp)> {
        self.of(Param::Fade)
    }
    fn of(&self, p: Param) -> Vec<(u64, f64, Ramp)> {
        self.param.iter().filter(|x| x.0 == p).map(|x| (x.1.at, x.1.value, x.1.ramp)).collect()
    }
}

fn voices(w: &Tw) -> Vec<V> {
    let mut out: Vec<(VoiceId, V)> = Vec::new();
    for c in &w.controls {
        match c {
            Control::Start(s) => out.push((
                s.voice,
                V {
                    id: s.voice,
                    at: s.at,
                    offset: s.offset,
                    loops: s.loops,
                    loop_points: s.loop_points,
                    dist: s.dist,
                    stop: None,
                    param: vec![],
                },
            )),
            Control::Cancel(_) => {}
            Control::Change(ch) => {
                let Target::Voice(id) = ch.target else { panic!("a change on {:?}", ch.target) };
                let v = &mut out.iter_mut().find(|x| x.0 == id).expect("a started voice").1;
                if ch.param == Param::Stop {
                    v.stop = Some(v.stop.map_or(ch.at, |s| s.min(ch.at)));
                } else {
                    v.param.push((ch.param, *ch));
                }
            }
        }
    }
    out.into_iter().map(|x| x.1).collect()
}

fn s(t: f64) -> u64 {
    samples(t)
}

/// A voice parameter's value at `t` as the mixer has it from the controls: each taken when it was emitted (a cancel
/// drops what waits from its sample on), each change at its sample, or at once when that has passed.
fn level(w: &Tw, voice: VoiceId, param: Param, init: f64, t: f64) -> f64 {
    let q = s(t);
    let target = Target::Voice(voice);
    let mut auto = Auto::new(init);
    // the changes waiting, by sample, with the sample they were emitted at
    let mut pending: Vec<(ParamChange, u64)> = Vec::new();
    let take = |pending: &mut Vec<(ParamChange, u64)>, auto: &mut Auto, until: u64| {
        while pending.first().is_some_and(|p| p.0.at < until) {
            let (c, sent) = pending.remove(0);
            auto.change(c.at.max(sent), c.value, c.ramp);
        }
    };
    for (c, &sent) in w.controls.iter().zip(&w.sent) {
        if sent > q {
            break;
        }
        take(&mut pending, &mut auto, sent);
        match c {
            Control::Change(ch) if ch.target == target && ch.param == param => {
                let k = pending.partition_point(|p| p.0.at <= ch.at);
                pending.insert(k, (*ch, sent));
            }
            Control::Cancel(x) if x.target == target && x.param == param => pending.retain(|p| p.0.at < x.from),
            _ => {}
        }
    }
    take(&mut pending, &mut auto, q + 1);
    auto.value(q)
}

fn fade_at(w: &Tw, v: &V, t: f64) -> f64 {
    level(w, v.id, Param::Fade, 1.0, t)
}

fn near(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() < 1e-9, "{what}: {got} vs {want}");
}

/// A curve ramp's value at time `t` as the oracle's per-frame steps have it (their end points lie on the curve).
fn on_curve(curve: Curve, t0: f64, secs: f64, from: f64, to: f64, t: f64, v: f64) {
    let want = crate::ctl::curves::interp(curve, (t - t0) / secs, from, to);
    assert!((want - v).abs() < 1e-12, "{curve:?} at {t}: {want} vs {v}");
}

fn play(m: &mut Music, w: &mut Tw) {
    m.play(w, 0.0, 1);
}

#[test]
fn exit_marker_switch() {
    let rules = vec![rule(
        src_side(500.0, 0.0, Curve::Linear, SyncType::ExitMarker),
        dst_side(700.0, 100.0, Curve::Sine, false),
    )];
    let mut w = world(hierarchy(rules, track(900, 8000.0, vec![], None)), vec![0.5]);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 3.0);
    switch_to(&mut m, &mut w, 100, 20, 3.0);
    drive(&mut m, &mut w, 3.0, 12.0);
    assert!(w.warns.is_empty(), "{:?}", w.warns);
    let v = voices(&w);
    // the oracle: n3 start 0.5 offset 0.5 stop 7.5; n5 (the chained segment 5, stopped at once); n8 start 7.5; n10 13
    assert_eq!((v[0].at, v[0].offset, v[0].stop), (s(0.5), s(0.5) as u32, Some(s(7.5))));
    assert_eq!((v[1].at, v[1].offset), (s(7.5), 0));
    assert!(v[1].stop.unwrap() < v[1].at, "the chained segment the transition cuts never plays");
    assert_eq!((v[2].at, v[2].offset, v[2].stop), (s(7.5), 0, Some(s(13.5))));
    assert_eq!((v[3].at, v[3].offset), (s(13.0), 0));
    // the first run's fade: in at 0.6 over 0.7 s (Sine), then out from 7.0 over 0.5 s (Linear) from 1 to 0
    let f = v[0].fades();
    assert!(f.contains(&(s(0.6), 0.0, Ramp::Set)));
    assert!(f.contains(&(s(0.6), 1.0, Ramp::Curve { secs: 0.7, curve: Curve::Sine })));
    on_curve(Curve::Sine, 0.6, 0.7, 0.0, 1.0, 0.6212121212121212, 0.04758191582374229);
    on_curve(Curve::Linear, 7.0, 0.5, 1.0, 0.0, 7.020833333333333, 0.9583333333333334);
    // (programmed at the commit, 7.2: the voice joins the fade-out there)
    for (t, want) in [(7.2, 0.6), (7.25, 0.5), (7.4, 0.2), (7.5, 0.0)] {
        near(fade_at(&w, &v[0], t), want, "the first run's fade-out");
    }
    // the second run's: in at 7.6 over 0.7 s
    let f = v[2].fades();
    assert!(f.contains(&(s(7.6), 1.0, Ramp::Curve { secs: 0.7, curve: Curve::Sine })));
    assert!(w.lines.iter().any(|l| l.2 == "segment" && l.1 == 5 && (l.0 - 7.5).abs() < 1e-9));
}

#[test]
fn next_bar_switch() {
    let rules =
        vec![rule(src_side(0.0, 0.0, Curve::Linear, SyncType::NextBar), dst_side(0.0, 0.0, Curve::Linear, false))];
    let mut w = world(hierarchy(rules, track(900, 8000.0, vec![], None)), vec![0.5]);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 2.6);
    switch_to(&mut m, &mut w, 100, 20, 2.6);
    drive(&mut m, &mut w, 2.6, 8.0);
    let v = voices(&w);
    // the oracle: n3 start 0 offset 0.5; n5 start 7; the stop at the bar line 4.0 (120 bpm 4/4 from the entry cue 0.5
    // s into a segment that starts at -0.5 s); n8 start 4; n10 start 9.5 after it
    assert_eq!((v[0].at, v[0].offset, v[0].stop), (s(0.0), s(0.5) as u32, Some(s(4.0))));
    assert_eq!(v[1].at, s(7.0));
    assert!(v[1].stop.unwrap() < v[1].at);
    assert_eq!((v[2].at, v[2].offset), (s(4.0), 0));
    assert_eq!(v[3].at, s(9.5));
    assert!(v[0].fades().contains(&(s(4.0), 0.0, Ramp::Set)));
}

#[test]
fn immediate_switch_with_fades() {
    let rules = vec![rule(
        src_side(1000.0, -200.0, Curve::SCurve, SyncType::Immediate),
        dst_side(300.0, 0.0, Curve::Linear, true),
    )];
    let mut w = world(hierarchy(rules, track(900, 8000.0, vec![], None)), vec![0.5]);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 1.0);
    switch_to(&mut m, &mut w, 100, 20, 1.0);
    drive(&mut m, &mut w, 1.0, 5.0);
    let v = voices(&w);
    // the oracle: n3 start 0.7 offset 0 (a pre-entry playlist from its start); fade in at 1.2 over 0.3 s; stop at 2.0
    // after the 1.0 s fade-out from 1.0 s, which starts from the fade-in's level there (0); n6 start 2.2; n8 start 7.7
    assert_eq!((v[0].at, v[0].offset, v[0].stop), (s(0.7), 0, Some(s(2.0))));
    assert!(v[0].fades().contains(&(s(1.2), 1.0, Ramp::Curve { secs: 0.3, curve: Curve::Linear })));
    // the oracle's fades from the commit at 2.0 on: the first run's fade-in at 1.2 is cancelled by its fade-out from
    // 1.0 (0 throughout), the second run's fade-in runs from 2.2 to 2.5
    for t in [2.0, 2.1, 2.2, 2.3] {
        near(fade_at(&w, &v[0], t), 0.0, "the first run, cancelled");
    }
    near(fade_at(&w, &v[1], 2.3), 0.3333333333333318, "the second run's fade-in");
    near(fade_at(&w, &v[1], 2.5), 1.0, "the second run's fade-in");
    assert_eq!(v[1].at, s(2.2));
    assert_eq!(v[2].at, s(7.7));
}

#[test]
fn cancelled_fade_in() {
    // a fade-in scheduled at 3.9 (sync 3.0 + offset 0.9) is cancelled by the fade-out from 3.0 of the switch back
    let rules = vec![rule(
        src_side(1000.0, 0.0, Curve::Linear, SyncType::Immediate),
        dst_side(500.0, 900.0, Curve::Linear, false),
    )];
    let mut w = world(hierarchy(rules, track(900, 8000.0, vec![], None)), vec![0.5]);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 2.0);
    switch_to(&mut m, &mut w, 100, 20, 2.0);
    drive(&mut m, &mut w, 2.0, 3.0);
    switch_to(&mut m, &mut w, 100, 10, 3.0);
    drive(&mut m, &mut w, 3.0, 7.0);
    let v = voices(&w);
    // the oracle: sources (start, offset, stop) (1, 0.5, 3), (3, 0, 4), (4, 0.5); the fades from each commit on:
    // run 0 (committed 2.7) 0.02 at 2.9, 0 from 3.0; run 1 0 throughout; run 2 0.2 at 5.0, 0.6 at 5.2
    assert_eq!((v[0].at, v[0].offset, v[0].stop), (s(1.0), s(0.5) as u32, Some(s(3.0))));
    assert_eq!((v[1].at, v[1].offset, v[1].stop), (s(3.0), 0, Some(s(4.0))));
    assert_eq!((v[2].at, v[2].offset), (s(4.0), s(0.5) as u32));
    near(fade_at(&w, &v[0], 2.9), 0.020000000000000004, "run 0");
    near(fade_at(&w, &v[0], 3.0), 0.0, "run 0");
    for t in [3.0, 3.5, 3.7, 3.9, 3.95, 3.99] {
        near(fade_at(&w, &v[1], t), 0.0, "run 1, its fade-in cancelled");
    }
    let from = s(3.0);
    assert!(w.controls.iter().any(|c| matches!(c, Control::Cancel(Cancel { from: f, target, param: Param::Fade })
        if *f == from && *target == Target::Voice(v[1].id))));
    near(fade_at(&w, &v[2], 5.0), 0.19999999999999896, "run 2");
    near(fade_at(&w, &v[2], 5.2), 0.5999999999999993, "run 2");
}

#[test]
fn voice_joins_a_fade() {
    // an 8 s fade-in from 0; the playlist's second segment starts at 5.5, part-way through it
    let rules = vec![rule(
        src_side(0.0, 0.0, Curve::Linear, SyncType::ExitMarker),
        dst_side(8000.0, 0.0, Curve::Linear, false),
    )];
    let mut w = world(hierarchy(rules, track(900, 8000.0, vec![], None)), vec![0.5]);
    w.switch.insert(100, 20);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 12.0);
    let v = voices(&w);
    // the oracle: sources start at 0, 5.5, 11; the run's fade 0.125 at 1, 0.5 at 4, 0.6875 at 5.5, 0.75 at 6, 0.9875
    // at 7.9, 1 from 8
    assert_eq!([v[0].at, v[1].at, v[2].at], [s(0.0), s(5.5), s(11.0)]);
    near(fade_at(&w, &v[0], 1.0), 0.125, "the first voice");
    near(fade_at(&w, &v[0], 4.0), 0.5, "the first voice");
    for (t, want) in [(5.5, 0.6875), (6.0, 0.75), (7.9, 0.9875), (8.0, 1.0), (9.0, 1.0)] {
        near(fade_at(&w, &v[1], t), want, "the voice that joins");
    }
}

#[test]
fn late_clip_automation() {
    // a clip whose voice starts 0.8 s after its time: both automations run from the clip's start, the voice joins them
    let pt = |x, y, curve| Pt { x, y, curve };
    let auto = vec![
        ClipAuto {
            clip: 0,
            kind: AutoType::Volume,
            pts: vec![pt(0.0, 0.0, Curve::Linear), pt(1.0, 0.5, Curve::Linear)],
        },
        ClipAuto {
            clip: 0,
            kind: AutoType::FadeIn,
            pts: vec![pt(0.0, 0.0, Curve::Linear), pt(2.0, 1.0, Curve::Linear)],
        },
    ];
    let rules =
        vec![rule(src_side(0.0, 0.0, Curve::Linear, SyncType::ExitMarker), dst_side(0.0, 0.0, Curve::Linear, false))];
    let mut o = hierarchy(rules, track(900, 8000.0, vec![], None));
    o.insert(7, track(901, 6000.0, auto, None));
    let mut w = world(o, vec![0.5]);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 3.0);
    switch_to(&mut m, &mut w, 100, 20, 3.0);
    // the music's next tick comes late: the switch commits at 7.8, after its sync at 7.0
    drive(&mut m, &mut w, 7.8, 9.5);
    let v = voices(&w);
    let b = v.iter().find(|x| x.at == s(7.8)).expect("the late voice");
    assert_eq!(b.offset, s(0.8) as u32);
    // the gain nodes' timelines: Volume 1 at 7.0 to 1.5 at 8.0, FadeIn 0 at 7.0 to 1 at 9.0
    for (t, vol, fade) in [(7.8, 1.4, 0.4), (8.0, 1.5, 0.5), (8.5, 1.5, 0.75), (9.0, 1.5, 1.0)] {
        near(level(&w, b.id, Param::Dist, b.dist, t), vol, "Volume");
        near(level(&w, b.id, Param::Mute, 1.0, t), fade, "FadeIn");
    }
}

#[test]
fn clip_automation() {
    let pt = |x, y, curve| Pt { x, y, curve };
    let auto = vec![
        ClipAuto {
            clip: 0,
            kind: AutoType::Volume,
            pts: vec![
                pt(0.0, 0.0, Curve::Linear),
                pt(1.0, 0.5, Curve::Sine),
                pt(3.0, -0.25, Curve::Constant),
                pt(4.0, 0.0, Curve::Linear),
            ],
        },
        ClipAuto { clip: 0, kind: AutoType::FadeIn, pts: vec![pt(0.0, 0.0, Curve::Exp3), pt(2.0, 1.0, Curve::Linear)] },
    ];
    let rules =
        vec![rule(src_side(0.0, 0.0, Curve::Linear, SyncType::ExitMarker), dst_side(0.0, 0.0, Curve::Linear, false))];
    let mut w = world(hierarchy(rules, track(900, 8000.0, auto, Some(0.0))), vec![0.5]);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 1.0);
    assert!(w.warns.is_empty(), "{:?}", w.warns);
    let v = voices(&w);
    // the oracle: n5 start 0 offset 0.5 loop to 8 s; the Volume gain starts at 1.25 (0.25 + 1), runs linearly to 1.5
    // at 0.5 s, along the Sine span to 0.75 at 2.5 s (95 steps in all), then steps to 1 at 3.5 s; the FadeIn gain
    // starts at 0.015625 and reaches 1 at 1.5 s in 71 per-frame steps
    assert_eq!((v[0].at, v[0].offset, v[0].loops, v[0].loop_points), (0, s(0.5) as u32, 0, Some([0, 8 * 48_000])));
    assert_eq!(v[0].dist, 1.25);
    let d = v[0].of(Param::Dist);
    assert_eq!(d[0], (s(0.0), 1.5, Ramp::Linear(0.5)));
    assert_eq!(d[1], (s(0.5), 0.75, Ramp::Curve { secs: 2.0, curve: Curve::Sine }));
    assert_eq!(d[2], (s(3.5), 1.0, Ramp::Set));
    assert_eq!(d.len(), 3);
    let mute = v[0].of(Param::Mute);
    assert_eq!(mute[0], (0, 0.015625, Ramp::Set));
    assert_eq!(mute.len(), 1 + 71);
    // the first and last step: (0.0211267..., 0.01769...) and (1.5, 1)
    let first = v[0].param.iter().filter(|x| x.0 == Param::Mute).nth(1).unwrap().1;
    assert_eq!(first.at, 0);
    let (t1, ramp) = (0.021126760563380254, first.ramp);
    let Ramp::Linear(len) = ramp else { panic!("{ramp:?}") };
    assert!((len - t1).abs() < 1e-12 && (first.value - 0.017690501269868764).abs() < 1e-12);
    let last = v[0].param.iter().rev().find(|x| x.0 == Param::Mute).unwrap().1;
    assert_eq!(last.value, 1.0);
    assert!((last.at as f64 / 48_000.0 + if let Ramp::Linear(l) = last.ramp { l } else { 0.0 } - 1.5).abs() < 1e-3);
}

#[test]
fn random_group_chain() {
    // a shuffle group of three segments, the draws 0.9 0.1 0.5 0.3 0.7 in turn
    let mut o = hierarchy(
        vec![rule(src_side(0.0, 0.0, Curve::Linear, SyncType::ExitMarker), dst_side(0.0, 0.0, Curve::Linear, false))],
        track(900, 8000.0, vec![], None),
    );
    o.insert(2, playlist(group(2, vec![item(4), item(5), item(8)], true)));
    o.insert(8, seg(4000.0, 0.0, 3500.0, vec![9]));
    o.insert(9, track(902, 4000.0, vec![], None));
    let mut w = world(o, vec![0.9, 0.1, 0.5, 0.3, 0.7]);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 30.0);
    let v = voices(&w);
    // the oracle: sources start at 0 (media 902), 3 (900, from 3.5 s on), 10.5 (901), 15.5 (900), 23 (902), 26.5
    // (901), 31.5 (900)
    let at: Vec<u64> = v.iter().map(|x| x.at).collect();
    assert_eq!(at, [0.0, 3.0, 10.5, 15.5, 23.0, 26.5, 31.5].map(s));
    let medium: Vec<Id> =
        w.controls.iter().filter_map(|c| if let Control::Start(x) = c { Some(x.medium) } else { None }).collect();
    assert_eq!(medium, [902, 900, 901, 900, 902, 901, 900]);
    assert_eq!(w.drawn, 8);
}

#[test]
fn plays_on_without_allocating() {
    // a random playlist of three segments, one with two clip automations: once its lists have grown, chaining its
    // segments and starting their voices allocates nothing
    let pt = |x, y, curve| Pt { x, y, curve };
    let auto = vec![
        ClipAuto { clip: 0, kind: AutoType::Volume, pts: vec![pt(0.0, 0.0, Curve::Linear), pt(1.0, 0.5, Curve::Sine)] },
        ClipAuto { clip: 0, kind: AutoType::FadeIn, pts: vec![pt(0.0, 0.0, Curve::Exp3), pt(2.0, 1.0, Curve::Linear)] },
    ];
    let mut o = hierarchy(
        vec![rule(src_side(0.0, 0.0, Curve::Linear, SyncType::ExitMarker), dst_side(0.0, 0.0, Curve::Linear, false))],
        track(900, 8000.0, auto, None),
    );
    o.insert(2, playlist(group(2, vec![item(4), item(5), item(8)], true)));
    o.insert(8, seg(4000.0, 0.0, 3500.0, vec![9]));
    o.insert(9, track(902, 4000.0, vec![], None));
    let mut w = world(o, vec![0.9, 0.1, 0.5, 0.3, 0.7, 0.2, 0.6]);
    w.quiet = true;
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 60.0);
    w.controls.reserve(100_000);
    w.sent.reserve(100_000);
    let started = w.controls.len();
    let ((), n) = crate::mix::tests::allocations(|| drive(&mut m, &mut w, 60.0, 180.0));
    assert!(w.controls.len() > started + 50, "{} controls", w.controls.len() - started);
    assert_eq!(n, 0, "allocations and frees while the music played on");
}

#[test]
fn sources_follow_the_values() {
    let o = hierarchy(vec![], track(900, 8000.0, vec![], None));
    assert_eq!(super::load::sources(&o, 1, &[10], false), vec![900, 901]);
    assert_eq!(super::load::sources(&o, 1, &[20], false), vec![901]);
    // a value the container has no branch for: every branch
    assert_eq!(super::load::sources(&o, 1, &[], false), vec![900, 901]);
    assert_eq!(super::load::sources(&o, 1, &[999], false), vec![900, 901]);
}

#[test]
fn flow_holds_every_state() {
    let flow = crate::MusicFlow { group: 1, start: 5, next: vec![(5, vec![6]), (6, vec![7, 5])] };
    assert_eq!(super::load::flow_values(&[2, 5, 6, 3], &flow), vec![2, 3, 5, 6, 7]);
}

#[test]
fn step_group_keeps_its_place() {
    // a step sequence gives one item per visit: two playlist activations play the segments in turn
    let mut o = hierarchy(vec![], track(900, 8000.0, vec![], None));
    o.insert(2, playlist(group(1, vec![item(4), item(5)], false)));
    let mut w = world(o, vec![0.5]);
    let mut step = iter::StepInfo::new();
    let root = group(1, vec![item(4), item(5)], false);
    let mut order = Vec::new();
    for _ in 0..3 {
        let mut it = iter::Iter::new(Some(root.clone()));
        order.push(it.next(&step, &mut w));
        it.played(&mut step);
    }
    assert_eq!(order, [Some(4), Some(5), Some(4)]);
}

fn leaves(i: &MusicItem) -> Vec<Id> {
    let mut v: Vec<Id> = if i.seg != 0 { vec![i.seg] } else { vec![] };
    for c in &i.children {
        for s in leaves(c) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
    }
    v
}

#[test]
fn a_waiting_transition_holds_its_first_segment() {
    // playlist 3 plays segment 8 (medium 902), which no run has until the switch commits at the exit marker (7.5)
    let rules =
        vec![rule(src_side(500.0, 0.0, Curve::Linear, SyncType::ExitMarker), dst_side(0.0, 0.0, Curve::Linear, false))];
    let mut o = hierarchy(rules, track(900, 8000.0, vec![], None));
    o.insert(3, playlist(group(0, vec![item(8)], false)));
    o.insert(8, seg(6000.0, 0.0, 5500.0, vec![9]));
    o.insert(9, track(902, 6000.0, vec![], None));
    let mut w = world(o, vec![0.5]);
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 3.0);
    switch_to(&mut m, &mut w, 100, 20, 3.0);
    drive(&mut m, &mut w, 3.0, 4.0);
    let objs = &w.objs;
    let obj = |id| objs.get(&id);
    assert!(m.pending.is_some(), "the transition waits for its sync point");
    assert!(m.runs.values().all(|r| !r.holds(&obj, 902)), "no run plays the waiting segment yet");
    assert!(m.uses(&obj, 902), "a release now keeps the waiting run's medium");
    assert!(!m.uses(&obj, 999));
}

#[test]
fn clips_send_to_the_aux_buses_of_the_music_node() {
    let rules =
        vec![rule(src_side(0.0, 0.0, Curve::Linear, SyncType::ExitMarker), dst_side(0.0, 0.0, Curve::Linear, false))];
    let mut objs = hierarchy(rules, track(900, 8000.0, vec![], None));
    let Some(Obj::MusicSwitchContainer(root)) = objs.get_mut(&1) else { panic!("the root") };
    root.node.aux = vec![64_502_571, 0, 64_502_573];
    root.node.p = Props(vec![(19, -6.0), (21, 0.0)]);
    let mut w = world(objs, vec![0.5]);
    w.buses.insert(64_502_571, Bus::default());
    w.buses.insert(64_502_573, Bus::default());
    let mut m = Music::default();
    play(&mut m, &mut w);
    drive(&mut m, &mut w, 0.0, 3.0);
    let sent: Vec<_> = w
        .controls
        .iter()
        .filter_map(|c| if let Control::Start(v) = c { Some((v.sends_n, v.sends[0], v.sends[1])) } else { None })
        .collect();
    assert!(!sent.is_empty());
    for (n, a, b) in sent {
        assert_eq!(n, 2);
        assert_eq!(a.bus, 64_502_571);
        near(a.gain, 10f64.powf(-6.0 / 20.0), "send 0");
        assert_eq!((b.bus, b.gain), (64_502_573, 1.0));
    }
}
