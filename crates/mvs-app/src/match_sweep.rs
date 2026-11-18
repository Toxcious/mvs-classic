//! `mvs --match-sweep [--sweep-ticks <n>] [--sweep-only <key>[,..]]`: every fighter in every skin through a custom
//! game's load and its first ticks, one match each: the picked fighter and skin in slot 0 (a human with no input),
//! three bots on other fighters of the roster in a random skin of theirs, free-for-all and 2v2 in turn, each player
//! with perks, taunts, a ring-out and a banner from the menu data, on the offline maps in turn. A match's panic is
//! caught, printed with the case and the run goes on (`mvs: sweep` lines, a summary at the end). Run with
//! `RUST_BACKTRACE=1` for the backtraces.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use mvs_sim::setup::{MatchPlayer, Rules, TeamStyle};
use mvs_sim::{MatchSetup, setup::perk_list};
use mvs_ui::bundle::MenuData;
use mvs_ui::menu::lobby::loadout::asset_name;

use crate::app::{Frame, MatchKind, MatchStart, Shell};
use crate::prepare::{Pending, PrepareJob};
use crate::run::{MatchLoop, MatchOptions, STEP};
use crate::{Error, Result};

/// The sweep's options.
#[derive(Clone, Debug)]
pub struct SweepOptions {
    /// The frames each match runs (one tick each).
    pub ticks: u32,
    /// The fighters to sweep (empty: the whole roster).
    pub only: Vec<String>,
}

impl Default for SweepOptions {
    fn default() -> Self {
        SweepOptions { ticks: 900, only: Vec::new() }
    }
}

/// A small seeded draw (the sweep's own: the same cases every run).
struct Draw(u64);

impl Draw {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        #[allow(clippy::cast_precision_loss)]
        let x = (self.0 >> 11) as f64 / (1u64 << 53) as f64;
        x
    }

    fn pick<'a, T>(&mut self, v: &'a [T]) -> Option<&'a T> {
        if v.is_empty() {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
        let i = ((self.next() * v.len() as f64) as usize).min(v.len() - 1);
        v.get(i)
    }
}

/// A player of `key` in `skin` with a perk of each of its perk lists, its taunts, a ring-out and a banner.
fn player(menu: &MenuData, key: &str, skin: &str, bot: bool, d: &mut Draw) -> MatchPlayer {
    let f = menu.fighters.get(key);
    let mut perks = vec![String::new(); 4];
    if let Some(p) = f.and_then(|f| f.perks.as_ref()) {
        for (slot, list) in p.lists.iter().take(4).enumerate() {
            if let Some(id) = d.pick(list) {
                perks[slot].clone_from(id);
            }
        }
    }
    let cat = &menu.cosmetics;
    let taunts: Vec<String> = cat
        .characters
        .values()
        .find(|c| c.key == key)
        .map(|c| {
            c.taunts
                .iter()
                .take(4)
                .filter_map(|slug| cat.taunt.iter().find(|t| &t.slug == slug))
                .map(|t| asset_name(&t.path).to_owned())
                .collect()
        })
        .unwrap_or_default();
    MatchPlayer {
        bot,
        difficulty: bot.then_some(1.0),
        character: Some(key.to_owned()),
        skin: (!skin.is_empty()).then(|| skin.to_owned()),
        perks: perk_list(&perks),
        taunts,
        ring_out: d.pick(&cat.ringout).map(|r| asset_name(&r.path).to_owned()),
        banner: d.pick(&cat.banner).map(|b| b.slug.clone()),
        ..MatchPlayer::default()
    }
}

/// Runs the sweep on the shell's store; Err only when the menu data is missing.
pub fn run(sh: &mut Shell, o: &SweepOptions) -> Result<()> {
    let menu = sh
        .ui
        .as_ref()
        .and_then(|u| u.menu.menu.clone())
        .ok_or_else(|| Error::Match("the sweep needs the menu data (no --no-ui)".into()))?;
    let roster: Vec<String> = mvs_protocol::roster::CHARACTERS
        .iter()
        .map(|c| (*c).to_owned())
        .filter(|c| menu.fighters.contains_key(c))
        .collect();
    let mut cases: Vec<(String, String)> = Vec::new();
    for key in &roster {
        if !o.only.is_empty() && !o.only.contains(key) {
            continue;
        }
        let f = &menu.fighters[key];
        cases.push((key.clone(), String::new()));
        for s in &f.skins {
            if !s.is_default {
                cases.push((key.clone(), s.name.clone()));
            }
        }
    }
    eprintln!("mvs: sweep {} cases over {} fighters, {} ticks each", cases.len(), roster.len(), o.ticks);
    let mut d = Draw(0x5eed);
    let mut failed = Vec::new();
    for (n, (key, skin)) in cases.iter().enumerate() {
        let at = roster.iter().position(|k| k == key).unwrap_or(0);
        let mut players = vec![player(&menu, key, skin, false, &mut d)];
        let mut bots = Vec::new();
        for b in 1..4 {
            let bk = &roster[(at + b * (n + 1)) % roster.len()];
            let skins = &menu.fighters[bk].skins;
            let bs = d.pick(skins).filter(|s| !s.is_default).map_or_else(String::new, |s| s.name.clone());
            bots.push(format!("{bk}/{bs}"));
            players.push(player(&menu, bk, &bs, true, &mut d));
        }
        let map = if menu.offline_maps.is_empty() {
            None
        } else {
            Some(menu.offline_maps[n % menu.offline_maps.len()].clone())
        };
        let setup = MatchSetup {
            team_style: if n % 2 == 0 { TeamStyle::FreeForAll } else { TeamStyle::TwoVsTwo },
            players,
            rules: Some(Rules { knockouts_to_win: 3.0, duration: 420.0, stocks: true }),
            flow: true,
            allow_hazards: true,
            ..MatchSetup::default()
        };
        let what = format!(
            "case {} {key}/{} vs {} on {} ({})",
            n + 1,
            if skin.is_empty() { "default" } else { skin },
            bots.join(", "),
            map.as_deref().unwrap_or("-"),
            if n % 2 == 0 { "free-for-all" } else { "2v2" }
        );
        let t0 = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| one(sh, setup, map.clone(), o.ticks)));
        let line = match r {
            Ok(Ok(t)) => format!("{what}: ok, {t} frames, {:.1} s", t0.elapsed().as_secs_f64()),
            Ok(Err(e)) => format!("{what}: ERROR {e}"),
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_default();
                format!("{what}: PANIC {msg}")
            }
        };
        eprintln!("mvs: sweep {line}");
        if !line.contains(": ok,") {
            failed.push(line);
        }
    }
    eprintln!("mvs: sweep summary: {} of {} failed", failed.len(), cases.len());
    for l in &failed {
        eprintln!("  {l}");
    }
    Ok(())
}

/// One match: loaded as the loading screen loads it, then `ticks` frames of one step each; the frames it ran.
fn one(sh: &mut Shell, setup: MatchSetup, map: Option<String>, ticks: u32) -> Result<u32> {
    let mut start = MatchStart::new(setup, MatchKind::Default, 7);
    start.map = map;
    let mut pending = PrepareJob::of(sh, &start, false).map(Pending::start);
    start.prepared = match &mut pending {
        Some(p) => loop {
            if let Some(r) = p.take() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        },
        None => None,
    };
    let mut m = MatchLoop::new(sh, start, &MatchOptions::default())?;
    let mut frames = 0;
    while frames < ticks {
        let mut resized = None;
        let cpu_start = Instant::now();
        if !sh.window.pump(&mut sh.devices, &mut resized) {
            break;
        }
        let end = m.frame(sh, &Frame { dt: STEP, resized, cpu_start })?;
        if let Some(a) = sh.audio.as_mut() {
            a.pump(u32::MAX, mvs_audio::QUANTUM * mvs_audio::QUANTA_PER_FRAME);
            let _ = a.take_output();
            let _ = a.take_notes();
        }
        frames += 1;
        if end.is_some() {
            break;
        }
    }
    m.end(sh)?;
    Ok(frames)
}
