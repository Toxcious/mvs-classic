//! A match's end stops every voice: its stage ambience, its fighters' and items' loops and its music. Each stage
//! below begins as the exe does (`MatchAudio`'s begin lines, the base scope under them), plays two seconds with a
//! few of its fighters' and items' looping events posted on top, ends as `run.rs` finishes a match
//! (`MatchAudio::end`), and must be silent after the stop's fade and the output's tail, every post ended.
//!
//! Throne Room's ambience events are not in this build (ledger "Throne Room audio"): its begin posts none and
//! logs no dropped post. The dev store and the stage traces' headers are the inputs; without them it says so and
//! passes.

use std::sync::Arc;

use mvs_app::audio::{BeginOpts, Index, MatchAudio, base_events, hud_named};
use mvs_audio::tape::post::{Call, Line};
use mvs_audio::{Audio, Backend, Note, QUANTUM, RATE};

/// One tick of sound mixed through `a`, its notes counted: `(warnings by text, ended posts)`.
struct Run {
    a: Audio,
    owed: usize,
    tick: u32,
    peak: f32,
    warns: Vec<&'static str>,
    ended: usize,
}

impl Run {
    fn call(&mut self, l: &Line) {
        self.a.call(l).unwrap_or_else(|e| panic!("{l:?}: {e}"));
    }

    fn ticks(&mut self, n: u32) {
        self.peak = 0.0;
        for _ in 0..n {
            self.owed += RATE as usize / 60;
            let frames = self.owed / QUANTUM * QUANTUM;
            self.owed -= frames;
            self.a.pump(self.tick, frames);
            self.peak = self.a.take_output().iter().fold(self.peak, |p, s| p.max(s.abs()));
            for n in self.a.take_notes() {
                match n {
                    Note::Warn { what, .. } => self.warns.push(what),
                    Note::EventEnd { .. } => self.ended += 1,
                    _ => {}
                }
            }
            self.tick += 1;
        }
    }
}

fn looping(known: &Index, names: &[String]) -> Vec<String> {
    names.iter().filter(|e| e.contains("loop") && known.has(e)).take(6).cloned().collect()
}

#[test]
fn match_end_stops_every_loop() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("note: no store; nothing checked");
        return;
    };
    let init = mvs_data::sound::load_init(&store).expect("the InitBank");
    let known = Index::new(init.events.into_keys());
    let store = Arc::new(store);
    for name in ["stage-m008", "stage-m011", "stage-treehouse", "stage-m010"] {
        let trace_path = mvs_sim_trace::run::default_traces().join(format!("{name}.trace"));
        let Ok(trace) = mvs_trace::Trace::read(&trace_path) else {
            eprintln!("note: no trace {name}; skipped");
            continue;
        };
        let (mut w, _) = mvs_sim_trace::world_for(&trace.header, &store).expect("the World of the header");
        std::sync::Arc::make_mut(&mut w.setup).flow = true;
        let mut lines = Vec::new();
        let mut m = MatchAudio::with_opts(
            &w,
            known.clone(),
            &mut lines,
            BeginOpts { music_skin: Some("mus_base".into()), ..BeginOpts::default() },
        );
        let audio = Audio::new(store.clone(), Backend::Offline).expect("the offline audio");
        let mut r = Run { a: audio, owed: 0, tick: 0, peak: 0.0, warns: Vec::new(), ended: 0 };
        let hud = hud_named(r.a.store(), &known);
        r.call(&Line {
            tick: 0,
            call: Call::Scope { name: "base".into(), keep: base_events(&known, &hud), lazy: Vec::new() },
        });
        for l in &lines {
            r.call(l);
        }
        // (the fighters' and the items' looping events, posted on game objects as the sim's sounds are)
        let mut loops = Vec::new();
        for l in &lines {
            if let Call::Scope { name, keep, .. } = &l.call
                && ["match", "items", "stage"].contains(&name.as_str())
            {
                loops.extend(looping(&known, keep));
            }
        }
        for owner in 0..2 {
            r.call(&Line { tick: 0, call: Call::SetEmitter { owner, pos: [0.0, 0.0, 100.0], front: [1.0, 0.0, 0.0] } });
        }
        for (i, ev) in loops.iter().enumerate() {
            let owner = i32::try_from(i % 2).unwrap_or(0);
            r.call(&Line {
                tick: 0,
                call: Call::Post {
                    ev: ev.clone(),
                    owner: Some(owner),
                    key: Some(format!("{owner}:loop{i}")),
                    stop_key: None,
                    at: None,
                },
            });
        }
        r.ticks(120);
        let live = r.peak;
        let posts_before = r.ended;
        let mut end = Vec::new();
        m.end(120, false, &mut end);
        for l in &end {
            r.call(l);
        }
        r.ticks(30);
        r.ticks(240);
        r.ticks(60);
        eprintln!(
            "{name}: {} begin lines, {} loops posted; peak playing {live:.4}, after the end {:.6}; ended {posts_before} -> {}; warns {:?}",
            lines.len(),
            loops.len(),
            r.peak,
            r.ended,
            r.warns
        );
        assert!(!r.warns.contains(&"event not loaded: post dropped"), "{name}: a post was dropped: {:?}", r.warns);
        assert!(r.peak < 1e-4, "{name}: a voice plays on after the match's end (peak {})", r.peak);
    }
}
