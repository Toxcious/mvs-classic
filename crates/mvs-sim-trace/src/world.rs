//! The World's own record (`WorldRecord`): the match state's rows, the timers by kind,
//! the items in reach, the bonus trackers and the `rest` of the World's other own keys.

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::world::blast::{BlastBounds, KoBox, Shrunk};
use mvs_sim::world::items::Reach;
use mvs_sim::world::{BonusTracker, SpawnPoint, Timer, TimerCmd};
use mvs_trace::{BonusRecord, ReachRecord, RefRecord, TimerRecord, Value, WorldRecord};

use crate::conv::{Plain, Rec, as_obj, check_keys, obj, plain_struct, rec_rows, rec_struct};
use crate::cx::{Lx, Wx};

pub fn put(w: &World, x: &Wx, r: &mut WorldRecord) {
    let f = &w.flow;
    r.tick = f64::from(f.tick);
    r.intro_ticks = f64::from(f.intro_ticks);
    r.rng = w.rng.put(x);
    r.actor_seq = f64::from(w.actors.actor_seq);
    r.item_seq = f64::from(w.actors.item_seq);
    rec_rows!(put f, x, r;
        match_started: match_started, match_over: match_over, winning_team: winning_team,
        match_start_tick: match_start_tick, match_over_tick: match_over_tick, ko_point_multiplier: ko_point_multiplier,
        team_knockouts: team_knockouts, team_damage: team_damage, stocks: stocks, ring_out_anchors: step_ring_outs,
        blast_shrink: blast_shrink, announcer_until: announcer_until, victory_vo: victory_vo,
    );
    r.vo_speakers = f64::from(f.vo_shared.speakers);
    // (a frame hit is its attack's key, a number)
    r.frame_hits = w.frame_hits.iter().map(|&k| Value::Num(k as f64)).collect();
    r.reach = w.actors.reach.iter().map(|e| e.put(x)).collect();
    r.bonus_trackers = f.bonus_trackers.put(x);
    r.timers = f.timers.iter().map(timer_put).collect();
    r.rest = rest_put(w, x);
}

/// The serial a buff-actor PreviousAttacker waits with, plus its index in the wearer's buffs, until
/// [`resolve_buff_attackers`] (never handed out: serials count up from 0).
const PENDING_ATTACKER: u32 = u32::MAX - 3000;

/// A buff actor written as PreviousAttacker (`cx.rs`: -2 - (its wearer's oracle id x 1000 + its index in the wearer's
/// buffs)): a marker on the wearer, resolved once every fighter has its buffs.
pub fn buff_attacker_load(v: f64, l: &mut Lx) -> Option<ActorRef> {
    let k = -2.0 - v;
    #[allow(clippy::cast_possible_truncation, reason = "a whole key checked below")]
    let (tsid, i) = ((k / 1000.0).floor() as i32, (k % 1000.0) as u32);
    if k.fract() != 0.0 {
        l.miss(format_args!("a buff actor's key {v} that is no whole number"));
        return None;
    }
    let f = l.fighter(f64::from(tsid))?;
    Some(ActorRef::Buff(mvs_sim::combat::BuffRef { fighter: f, serial: PENDING_ATTACKER + i }))
}

/// Turns [`buff_attacker_load`]'s markers into the wearers' entries.
fn resolve_buff_attackers(w: &mut World, l: &mut Lx) {
    let ids: Vec<_> = w.fighters.iter().map(|(id, _)| id).collect();
    for id in ids {
        let Some(ActorRef::Buff(b)) = w.fighter(id).attackers.previous_attacker else { continue };
        if b.serial < PENDING_ATTACKER {
            continue;
        }
        let i = (b.serial - PENDING_ATTACKER) as usize;
        let r = w
            .fighter(b.fighter)
            .buffs
            .buffs
            .get(i)
            .map(|e| mvs_sim::combat::BuffRef { fighter: b.fighter, serial: e.serial });
        if r.is_none() {
            l.miss(format_args!("a buff actor as previous attacker at index {i}, past its wearer's list"));
        }
        w.fighter_mut(id).attackers.previous_attacker = r.map(ActorRef::Buff);
    }
}

pub fn load(w: &mut World, r: &WorldRecord, l: &mut Lx) {
    // The fighters have their buff lists by now (`load_frame` loads them after the actors): a chain's stun buff
    // resolves.
    l.at("projectiles", |l| crate::black_adam::chain::resolve_stuns(w, l));
    // A plasma shot's held buff is on its victim's list.
    l.at("projectiles", |l| crate::actors::resolve_hold_buffs(w, l));
    // A buff actor as PreviousAttacker: its wearer's entry at the written index.
    l.at("fighters", |l| resolve_buff_attackers(w, l));
    // A fighter's platform on a world actor's box: the actor's part of the written flags.
    l.at("fighters", |l| crate::fighter::resolve_actor_plats(w, l));
    // An Iron Giant's PilotBuff is on his pilot: resolved once every fighter has its buffs.
    crate::iron_giant::resolve_pilot_buffs(w, r, l);
    // A Gizmo's attach buffs: his own and his carried ally's, once every fighter has its buffs.
    crate::gizmo::resolve_gizmo_buffs(w, r, l);
    crate::gizmo::perks::resolve_backpack_spawned(w, r, l);
    // A car's held buff entry is on its driver's list.
    crate::gizmo::car::resolve_car_buffs(w, l);
    // The bolts' ProtectiveBuff is on their owner.
    l.at("actors", |l| crate::iron_giant::actor_state::resolve_bolt_buffs(w, l));
    // An Iron Giant's DamageReflect holds its Buff_Thorns_C entry: resolved with the lists.
    crate::iron_giant::perks::resolve_thorns(w, l);
    // A Velma carry run's PickUpBuffs and watched buffs are on the held fighters.
    crate::velma::carry::resolve_run_buffs(w, l);
    // A polymorph's `from` is written by its kind alone: found again as its begin found it.
    crate::polymorph::resolve_from(w);
    let f = &mut w.flow;
    l.at("tick", |l| f.tick.load(&r.tick, l));
    // TimeSeconds is not in the record: the running float32 sum of the steps the tick counts, as World::step adds it.
    w.time_seconds = (0..f.tick.max(0)).fold(0.0, |t, _| t + mvs_sim::consts::DELTA_SECONDS);
    l.at("intro_ticks", |l| f.intro_ticks.load(&r.intro_ticks, l));
    l.at("rng", |l| w.rng.load(&r.rng, l));
    l.at("actor_seq", |l| w.actors.actor_seq.load(&r.actor_seq, l));
    l.at("item_seq", |l| w.actors.item_seq.load(&r.item_seq, l));
    rec_rows!(load f, r, l;
        match_started: match_started, match_over: match_over, winning_team: winning_team,
        match_start_tick: match_start_tick, match_over_tick: match_over_tick, ko_point_multiplier: ko_point_multiplier,
        team_knockouts: team_knockouts, team_damage: team_damage, stocks: stocks, ring_out_anchors: step_ring_outs,
        blast_shrink: blast_shrink, announcer_until: announcer_until, victory_vo: victory_vo,
    );
    l.at("vo_speakers", |l| f.vo_shared.speakers.load(&r.vo_speakers, l));
    l.at("frame_hits", |l| {
        w.frame_hits.clear();
        for v in &r.frame_hits {
            match v {
                // (an attack key is a whole number well inside an i64)
                #[allow(clippy::cast_possible_truncation)]
                Value::Num(n) if n.fract() == 0.0 => {
                    w.frame_hits.insert(*n as i64);
                }
                v => l.miss(format_args!("{v:?} is not an attack key")),
            }
        }
    });
    l.at("reach", |l| w.actors.reach.load(&r.reach, l));
    l.at("bonus_trackers", |l| f.bonus_trackers.load(&r.bonus_trackers, l));
    l.at("timers", |l| timers_load(&mut f.timers, &r.timers, l));
    l.at("rest", |l| rest_load(w, &r.rest, l));
}

// ------------------------------------------------------------------ rows

rec_struct!(BonusTracker => BonusRecord { alive: alive, spree: spree, spree_done: spree_done });

impl Rec<ReachRecord> for Reach {
    fn put(&self, x: &Wx) -> ReachRecord {
        ReachRecord {
            items: self.items.iter().map(|a| Rec::<RefRecord>::put(a, x)).collect(),
            closest: self.closest.map(|a| Rec::<RefRecord>::put(&a, x)),
        }
    }
    fn load(&mut self, r: &ReachRecord, l: &mut Lx) {
        self.items.clear();
        for (i, e) in r.items.iter().enumerate() {
            if let Some(a) = l.at(&format!("items[{i}]"), |l| <ActorRef as Rec<RefRecord>>::make(e, l)) {
                self.items.push(a);
            }
        }
        self.closest = r.closest.as_ref().and_then(|c| l.at("closest", |l| <ActorRef as Rec<RefRecord>>::make(c, l)));
    }
    fn make(r: &ReachRecord, l: &mut Lx) -> Option<Self> {
        let mut e = Reach::default();
        e.load(r, l);
        Some(e)
    }
}

/// A timer's kind: what its oracle closure runs (the oracle's `TIMER_KINDS`), from the Rust command.
fn timer_kind(c: &TimerCmd) -> &'static str {
    match c {
        TimerCmd::RingOutAnchorEnd => "ringOutAnchor",
        TimerCmd::RemoveCameraFocus { .. } => "removeCameraFocus",
        TimerCmd::AbductTail { .. } => "abductTail",
        TimerCmd::BonusSurvived { .. } => "bonusSurvived",
        TimerCmd::EndWarning { .. } => "endWarning",
        TimerCmd::StartBlastShrink => "blastShrink",
        TimerCmd::IntroReveal { .. } => "introReveal",
        TimerCmd::IntroMatchStartVo => "matchStartVo",
        TimerCmd::IntroPlay { .. } => "introEnd",
        TimerCmd::IntroCameraAll => "onCamera",
        TimerCmd::IntroVisibleAll => "visible",
        TimerCmd::VictoryVo { .. } => "victoryVo",
        TimerCmd::MatchPointShow { .. } => "matchPointShow",
    }
}

fn timer_put(t: &Timer) -> TimerRecord {
    TimerRecord { at: f64::from(t.at), key: t.key.map(f64::from), kind: timer_kind(&t.cmd).to_owned() }
}

/// A timer's command from its record. The record holds the kind and the key, not what the oracle closure captured: the
/// World's own timer of the same kind, key and due tick is kept; else the command is made, its payload from the key
/// where the key is it (`removeCameraFocus`: the oracle keeps `f.id` as the key), else a stand-in (the first fighter,
/// step 0), named, so the list keeps its length and order.
fn timer_cmd(t: &TimerRecord, keep: Option<&TimerCmd>, l: &mut Lx) -> Option<TimerCmd> {
    let kind = t.kind.as_str();
    if let Some(k) = keep {
        return Some(*k);
    }
    if kind == "removeCameraFocus" {
        let by_key = t.key.and_then(|k| l.fighters.iter().find(|f| f64::from(f.0) == k)).map(|f| f.1);
        match by_key {
            Some(f) => return Some(TimerCmd::RemoveCameraFocus { fighter: f }),
            None => l.miss(format_args!("a removeCameraFocus timer whose key {:?} is no fighter's id", t.key)),
        }
    }
    let fighter = l.fighters.first().map(|f| f.1);
    let stand_in =
        |l: &mut Lx| l.miss(format_args!("a {kind} timer made with a stand-in payload (the record holds no captures)"));
    let c = match kind {
        "ringOutAnchor" => TimerCmd::RingOutAnchorEnd,
        "blastShrink" => TimerCmd::StartBlastShrink,
        "matchStartVo" => TimerCmd::IntroMatchStartVo,
        "onCamera" => TimerCmd::IntroCameraAll,
        "visible" => TimerCmd::IntroVisibleAll,
        k => {
            stand_in(l);
            match (k, fighter) {
                ("endWarning", _) => TimerCmd::EndWarning { step: 0 },
                ("introEnd", _) => TimerCmd::IntroPlay { index: 0 },
                ("victoryVo", _) => TimerCmd::VictoryVo { winner: -1 },
                ("matchPointShow", _) => TimerCmd::MatchPointShow { team: 0 },
                ("removeCameraFocus", Some(f)) => TimerCmd::RemoveCameraFocus { fighter: f },
                ("abductTail", Some(f)) => TimerCmd::AbductTail { fighter: f },
                ("bonusSurvived", Some(f)) => TimerCmd::BonusSurvived { fighter: f },
                ("introReveal", Some(f)) => TimerCmd::IntroReveal { fighter: f },
                (k, _) => {
                    l.miss(format_args!("a timer of no known kind {k}"));
                    return None;
                }
            }
        }
    };
    Some(c)
}

fn timers_load(ts: &mut Vec<Timer>, r: &[TimerRecord], l: &mut Lx) {
    let mut old: Vec<Option<Timer>> = std::mem::take(ts).into_iter().map(Some).collect();
    for (i, t) in r.iter().enumerate() {
        l.at(&format!("[{i}]"), |l| {
            // (the World's own timer by what identifies it: its kind, key and due tick, each kept once)
            let same =
                |o: &Timer| timer_kind(&o.cmd) == t.kind && o.key.map(f64::from) == t.key && f64::from(o.at) == t.at;
            let keep = old.iter_mut().find(|o| o.as_ref().is_some_and(same)).and_then(Option::take).map(|o| o.cmd);
            let Some(cmd) = timer_cmd(t, keep.as_ref(), l) else { return };
            let mut timer = Timer { at: 0, cmd, key: None };
            l.at("at", |l| timer.at.load(&t.at, l));
            l.at("key", |l| timer.key.load(&t.key, l));
            ts.push(timer);
        });
    }
}

// ------------------------------------------------------------------ rest

plain_struct!(SpawnPoint { "facing" = facing, "x" = x, "y" = y });

/// The World's blast bounds as the World writes them (`{left, right, bottom, top}`; an actor's are `{l, r, b, t}`).
fn bounds_put(b: &BlastBounds) -> Value {
    obj(vec![
        ("bottom", Value::Num(f64::from(b.bottom))),
        ("left", Value::Num(f64::from(b.left))),
        ("right", Value::Num(f64::from(b.right))),
        ("top", Value::Num(f64::from(b.top))),
    ])
}

fn bounds_load(b: &mut BlastBounds, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    check_keys(o, &["bottom", "left", "right", "top"], l);
    for (k, f) in [("bottom", &mut b.bottom), ("left", &mut b.left), ("right", &mut b.right), ("top", &mut b.top)] {
        match o.get(k) {
            Some(v) => l.at(k, |l| f.load_value(v, l)),
            None => l.miss(format_args!("key {k} absent")),
        }
    }
}

/// `koBox` (no `halfExtent`: the shrunk box's) and `koBox0` (with it).
fn ko_box_put(k: &KoBox, half: bool, x: &Wx) -> Value {
    let mut e = vec![("center", k.center.to_value(x)), ("y", k.y.to_value(x)), ("z", k.z.to_value(x))];
    if half {
        e.push(("halfExtent", k.half_extent.to_value(x)));
    }
    obj(e)
}

fn ko_box_load(k: &mut KoBox, half: bool, v: &Value, l: &mut Lx) {
    let Some(o) = as_obj(v, l) else { return };
    let keys: &[&str] = if half { &["center", "halfExtent", "y", "z"] } else { &["center", "y", "z"] };
    check_keys(o, keys, l);
    let key = |n: &str, l: &mut Lx, set: &mut dyn FnMut(&Value, &mut Lx)| match o.get(n) {
        Some(v) => l.at(n, |l| set(v, l)),
        None => l.miss(format_args!("key {n} absent")),
    };
    key("center", l, &mut |v, l| k.center.load_value(v, l));
    key("y", l, &mut |v, l| k.y.load_value(v, l));
    key("z", l, &mut |v, l| k.z.load_value(v, l));
    if half {
        key("halfExtent", l, &mut |v, l| k.half_extent.load_value(v, l));
    }
}

plain_struct!(Shrunk { "center" = center, "halfExtent" = half_extent, "jumpOutBufferTop" = jump_out_buffer_top });

plain_struct!(mvs_sim::world::items::ItemArea {
    "items" = items, "newMatchAt" = new_match_at, "pendingAt" = pending_at, "pendingY" = pending_y, "pendingZ" = pending_z, "start" = start, "timerAt" = timer_at,
});
plain_struct!(mvs_sim::world::mutators::WorldMutator { "bound" = bound, "chosen" = chosen, "cls" = cls, "delayAt" = delay_at });
plain_struct!(mvs_sim::world::flow::WorldMutatorHandler { "buffs" = buffs });

const REST_KEYS: [&str; 27] = [
    "blast",
    "blastOuter",
    "blastShrinkBy",
    "blastShrinkTick",
    "blastShrinkTime",
    "comboReset",
    "dodgeball",
    "ground",
    "itemArea",
    "koBox",
    "koBox0",
    "newPawns",
    "overlapMidFrame",
    "pawnCtl",
    "pawnSeq",
    "respawns",
    "runtimeSolids",
    "sameAttackPenalty",
    "shrunk",
    "spawns",
    "stagePlatforms",
    "stageRun",
    "tiedFFATeams",
    "tutorial",
    "waves",
    "wheel",
    "worldMutators",
];

fn rest_put(w: &World, x: &Wx) -> Value {
    let s = &w.stage;
    let f = &w.flow;
    let mut o: Vec<(&str, Value)> = vec![
        ("blast", bounds_put(&s.blast)),
        ("blastOuter", bounds_put(&s.blast_outer)),
        ("blastShrinkTick", f.blast_shrink_tick.to_value(x)),
        ("comboReset", w.combo_reset.to_value(x)),
        ("ground", s.ground.to_value(x)),
        ("koBox", ko_box_put(&s.ko_box, false, x)),
        ("koBox0", ko_box_put(&s.ko_box0, true, x)),
        ("overlapMidFrame", w.overlap_mid_frame.to_value(x)),
        ("respawns", s.respawns.to_value(x)),
        ("sameAttackPenalty", f.same_attack_penalty.to_value(x)),
        ("shrunk", s.shrunk.to_value(x)),
        ("spawns", s.spawns.to_value(x)),
        ("stagePlatforms", crate::stages::platforms_put(w, x)),
        ("tiedFFATeams", f.tied_ffa_teams.to_value(x)),
    ];
    // `pawnSeq`: set from the first pawn spawn, unset again by dropPawns.
    if let Some(n) = w.pawn_seq {
        o.push(("pawnSeq", n.to_value(x)));
    }
    // The World's keys the oracle sets only in some matches (a shrink, items, world mutators).
    if let Some(v) = f.blast_shrink_time {
        o.push(("blastShrinkTime", v.to_value(x)));
    }
    if let Some(v) = f.blast_shrink_by {
        o.push(("blastShrinkBy", v.to_value(x)));
    }
    if let Some(v) = &w.item_area {
        o.push(("itemArea", v.to_value(x)));
    }
    if let Some(v) = &w.world_mutators {
        o.push(("worldMutators", v.to_value(x)));
    }
    // A mode's keys: the tutorial's, its run-time solids and the pawns' controllers.
    crate::tutorial::rest_put(w, x, &mut o);
    crate::waves::rest_put(w, x, &mut o);
    crate::wheel::rest_put(w, x, &mut o);
    crate::dodgeball::rest_put(w, x, &mut o);
    // (`pawns`, a mode's list of every pawn, is the frame's fighters: `frame_of`.)
    if let Some(r) = &w.stage_run {
        o.push(("stageRun", crate::stages::run_put(r, x)));
    }
    o.sort_by(|a, b| a.0.cmp(b.0));
    obj(o)
}

fn rest_load(w: &mut World, v: &Value, l: &mut Lx) {
    let Some(ob) = as_obj(v, l) else { return };
    check_keys(ob, &REST_KEYS, l);
    let get = |k: &str, l: &mut Lx| {
        let v = ob.get(k);
        if v.is_none() {
            l.miss(format_args!("key {k} absent"));
        }
        v
    };
    let s = &mut w.stage;
    let f = &mut w.flow;
    if let Some(v) = get("blast", l) {
        l.at("blast", |l| bounds_load(&mut s.blast, v, l));
    }
    if let Some(v) = get("blastOuter", l) {
        l.at("blastOuter", |l| bounds_load(&mut s.blast_outer, v, l));
    }
    if let Some(v) = get("blastShrinkTick", l) {
        l.at("blastShrinkTick", |l| f.blast_shrink_tick.load_value(v, l));
    }
    if let Some(v) = get("comboReset", l) {
        l.at("comboReset", |l| w.combo_reset.load_value(v, l));
    }
    if let Some(v) = get("ground", l) {
        l.at("ground", |l| s.ground.load_value(v, l));
    }
    if let Some(v) = get("koBox", l) {
        l.at("koBox", |l| ko_box_load(&mut s.ko_box, false, v, l));
    }
    if let Some(v) = get("koBox0", l) {
        l.at("koBox0", |l| ko_box_load(&mut s.ko_box0, true, v, l));
    }
    if let Some(v) = get("overlapMidFrame", l) {
        l.at("overlapMidFrame", |l| w.overlap_mid_frame.load_value(v, l));
    }
    if let Some(v) = get("respawns", l) {
        l.at("respawns", |l| s.respawns.load_value(v, l));
    }
    if let Some(v) = get("sameAttackPenalty", l) {
        l.at("sameAttackPenalty", |l| f.same_attack_penalty.load_value(v, l));
    }
    if let Some(v) = get("shrunk", l) {
        l.at("shrunk", |l| s.shrunk.load_value(v, l));
    }
    if let Some(v) = get("spawns", l) {
        l.at("spawns", |l| s.spawns.load_value(v, l));
    }
    if let Some(v) = get("tiedFFATeams", l) {
        l.at("tiedFFATeams", |l| f.tied_ffa_teams.load_value(v, l));
    }
    if let Some(v) = get("stagePlatforms", l) {
        l.at("stagePlatforms", |l| crate::stages::platforms_load(w, v, l));
    }
    match (ob.get("stageRun"), w.stage_run.is_some()) {
        (Some(v), _) => l.at("stageRun", |l| crate::stages::run_load(w, v, l)),
        (None, true) => l.miss("key stageRun absent"),
        (None, false) => {}
    }
    w.flow.blast_shrink_time =
        ob.get("blastShrinkTime").and_then(|v| l.at("blastShrinkTime", |l| mvs_sim::Real::from_value(v, l)));
    w.flow.blast_shrink_by =
        ob.get("blastShrinkBy").and_then(|v| l.at("blastShrinkBy", |l| mvs_sim::names::Name::from_value(v, l)));
    w.item_area =
        ob.get("itemArea").and_then(|v| l.at("itemArea", |l| mvs_sim::world::items::ItemArea::from_value(v, l)));
    w.world_mutators = ob
        .get("worldMutators")
        .and_then(|v| l.at("worldMutators", |l| mvs_sim::world::flow::WorldMutatorHandler::from_value(v, l)));
    w.pawn_seq = ob.get("pawnSeq").and_then(|v| l.at("pawnSeq", |l| i32::from_value(v, l)));
    crate::tutorial::rest_load(w, ob, l);
    crate::waves::rest_load(w, ob, l);
    crate::wheel::rest_load(w, ob, l);
    crate::dodgeball::rest_load(w, ob, l);
}
