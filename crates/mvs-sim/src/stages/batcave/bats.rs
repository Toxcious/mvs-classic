//! Batcave's two bat swarms (BatcaveBatsBP_C). Offsets are the class's ubergraph's. Their
//! warning reaches the level script's SetBatcaveWarningState through the parent game mode's dispatchers (Map_Batcave
//! @1721-1936 binds BatcaveBatsBP_Left's BatsWarning / BatsWarningOff to BatsWarningLeft / BatsWarningOff_Left: states
//! 1 / 5, the right one's to 2 / 6): `vents::set_warning_state`.
//! - BeginPlay @213: the match config's AllowHazards, then authority: K2_SetTimerDelegate(ActivateBats,
//!   RandomFloatInRange(60, 120), not looping) @488. Without AllowHazards nothing ever runs (the cycle has no other
//!   start).
//! - ActivateBats @2780: BatsWarningVisuals @2074 (broadcast BatsWarning; the Env_Audio_Emit child's ManualStart @2180:
//!   sfx_map_batcave_bats_flying; PS_BatcaveBats_Warning, destroyed by a Delay(4.5) @2313 -> @15: the drawing's from
//!   `warn_at`), Delay(1.5) @2794 -> @846: BatsMainVisuals @2368 (PS_BatcaveBats_Full, deactivated by a
//!   Delay(BatsDuration) @2464 -> @49 and destroyed 3 s later @85 -> @140: the drawing's from `main_at`), Delay(1.5)
//!   @860 -> @569 BatsOn = true, Delay(BatsDuration 5) @580 -> @639 BatsOn = false, Delay(2) @650 -> @705
//!   BatsWarningVisualsOff @2760 (broadcast BatsWarningOff), then K2_SetTimerDelegate(ActivateBats,
//!   RandomFloatInRange(45, 75), not looping) @788.
//! - ReceiveTick @1983 (authority, BatsOn): for each index of BoxOfBatsBottom.GetOverlappingActors(BaseFighter_C)
//!   (refetched at each step; nothing in the loop moves a fighter, so one list): a fighter not IsInvulnerable? takes
//!   ApplyDamage(0.13, no instigator, no causer, DmgTypeBP_Environmental) @1341 (`apply_direct_damage`: the armor's
//!   temp health first), hitVFX @1416 (PlayHitVFX false on both: nothing), then a DoOnce @1439 (its gate shared by
//!   all fighters): sfx_map_batcave_bats_hit on that fighter @1754, Delay(0.2) @1827 -> @546 opens it again. The tick
//!   precedes the actor's latent pass, so BatsOn set by a Delay's link counts from the next tick: the damage window is
//!   [3, 8) s after the warning.
//! - BoxOfBatsBottom's BeginOverlap @1907: a cast, nothing. BoxOfBatsTop: read by nothing. Their collision never
//!   changes.
//! - No reset, ring-out or EndPlay handler stops a cycle.
//!
//! Deviation: the overlap list is the fighters' root capsules against the box on the play plane (the box spans the
//! plane's depth), tested in the swarm's own tick from the places the frame before left (UpdateOverlaps along each
//! move is not modelled; a teleport counts at once); its order is begin order with a removal taking the last one's
//! place (OverlappingComponents RemoveAtSwap, stock 4.26, not read in the exe). A dead fighter (its pawn gone) and one
//! with its collision off overlap nothing.

use super::vents::{WARN_BATS_LEFT_OFF, WARN_BATS_LEFT_ON, WARN_BATS_RIGHT_OFF, WARN_BATS_RIGHT_ON, set_warning_state};
use super::{own_mut, slot_of};
use crate::Real;
use crate::State;
use crate::combat::hit::apply_direct_damage;
use crate::events::{Event, SoundKey};
use crate::fighter::common::warn_text;
use crate::fighter::{FighterId, actor_y, cap_radius, capsule_half, invulnerable, no_collision};
use crate::math::max;
use crate::math::ue::size2d;
use crate::name;
use crate::registry::props::num_or;
use crate::world::World;
use crate::world::pawns::{all_list, put_fighters};
use crate::world::stage::{stage_delay, stage_random_range, stage_set_timer};
use crate::world::stage_actors::StageActorOwn;

/// This file's event numbers (`StageEntry::timer` `ev`) are `BATS_EV` and up.
pub const BATS_EV: i32 = 300;
const EV_ACTIVATE: i32 = BATS_EV; // the ActivateBats timer
const EV_MAIN: i32 = BATS_EV + 1; // Delay @2794 -> @846
const EV_ON: i32 = BATS_EV + 2; // Delay @860 -> @569
const EV_OFF: i32 = BATS_EV + 3; // Delay @580 -> @639
const EV_WARN_OFF: i32 = BATS_EV + 4; // Delay @650 -> @705
const EV_GATE: i32 = BATS_EV + 5; // Delay @1827 -> @546
// The keys: the timer handle (the ActivateBats delegate) and the Delay nodes.
const KEY_ACTIVATE: i32 = 0;
const KEY_MAIN: i32 = 1;
const KEY_ON: i32 = 2;
const KEY_OFF: i32 = 3;
const KEY_WARN_OFF: i32 = 4;
const KEY_GATE: i32 = 5;

/// The ApplyDamage literal (DamagePerTick, also 0.13, is read by nothing).
fn damage() -> Real {
    0.13
}

/// The swarm's static data (not state: the record leaves it out).
#[derive(Clone, Debug, Default)]
pub struct BatsGeo {
    /// Its BatsWarning's SetBatcaveWarningState value (`vents::WARN_*`), -1 unknown.
    on: i32,
    off: i32,
    /// BatsDuration.
    duration: Real,
    /// ActivateBatsMinTime, MaxTime.
    activate: [Real; 2],
    /// ReactivateBatsMinTime, MaxTime.
    reactivate: [Real; 2],
    /// BoxOfBatsBottom, sim: x, y, half x, half y.
    bx: Option<[Real; 4]>,
    /// The Env_Audio_Emit child actor, sim.
    audio: [Real; 2],
}

/// `BatcaveBats`.
#[derive(State, Debug, Default)]
pub struct BatcaveBats {
    /// BatsOn.
    pub on: bool,
    /// The tick of the last BatsWarningVisuals (the warning emitter, the flying sound), -1.
    pub warn_at: i32,
    /// The tick of the last BatsMainVisuals (the full swarm emitter), -1.
    pub main_at: i32,
    /// The tick of the last BatsWarningVisualsOff, -1.
    pub warn_off_at: i32,
    /// The hit sound's DoOnce: Temp_bool_IsClosed_Variable.
    pub gate_closed: bool,
    /// Its Temp_bool_Has_Been_Initd_Variable.
    pub gate_init: bool,
    /// BoxOfBatsBottom's overlapping fighters (ids), in the list's order.
    pub inside: Vec<i32>,
    #[state(skip)]
    geo: BatsGeo,
}

// The level script binds each swarm by its actor reference (Map_Batcave @1721-1936), so the side goes by the placed
// actor's name. Env_Audio_Emit RelativeLocation under the root (Map_Batcave instance values).
fn side_of(name: &str) -> Option<(i32, i32, [Real; 3])> {
    match name {
        "BatcaveBatsBP_Left" => Some((WARN_BATS_LEFT_ON, WARN_BATS_LEFT_OFF, [0.0, -647.4209, 1824.8911])),
        "BatcaveBatsBP_Right" => Some((WARN_BATS_RIGHT_ON, WARN_BATS_RIGHT_OFF, [0.0, 458.34436, 1922.4723])),
        _ => None,
    }
}

/// `batsKind.create`.
pub fn bats_create(w: &mut World, slot: u32) {
    let data = std::sync::Arc::clone(&w.data);
    let index = w.stage_run.as_ref().expect("a stage run").actors[slot as usize].index;
    let d = &data.stage.placed.as_ref().expect("placed actors")[index as usize];
    let side = side_of(&d.name);
    if side.is_none() {
        warn_text(
            w,
            &format!("{} {}: not one of Map_Batcave's two swarms (no warning state is bound to it)", d.cls, d.name),
        );
    }
    let solids = data.stage.solids.as_deref().unwrap_or(&[]);
    let s = d
        .solids
        .iter()
        .flatten()
        .filter_map(|&id| usize::try_from(id).ok().and_then(|i| solids.get(i)))
        .find(|x| x.name.split('.').nth(1) == Some("BoxOfBatsBottom"));
    if s.is_none() {
        warn_text(w, &format!("{} {}: no BoxOfBatsBottom solid in the stage data", d.cls, d.name));
    }
    let rel = side.map_or([0.0; 3], |s| s.2);
    let loc = |i: usize| d.loc.get(i).copied().unwrap_or(0.0);
    let p = &d.p;
    let geo = BatsGeo {
        on: side.map_or(-1, |s| s.0),
        off: side.map_or(-1, |s| s.1),
        duration: num_or(p, "BatsDuration", 5.0),
        activate: [num_or(p, "ActivateBatsMinTime", 60.0), num_or(p, "ActivateBatsMaxTime", 120.0)],
        reactivate: [num_or(p, "ReactivateBatsMinTime", 45.0), num_or(p, "ReactivateBatsMaxTime", 75.0)],
        bx: s.map(|s| [-s.center[1], s.center[2], s.half_extent[1], s.half_extent[2]].map(crate::math::narrow)),
        audio: [crate::math::narrow(-(loc(1) + f64::from(rel[1]))), crate::math::narrow(loc(2) + f64::from(rel[2]))],
    };
    w.stage_run.as_mut().expect("a stage run").actors[slot as usize].own = StageActorOwn::BatcaveBats(BatcaveBats {
        warn_at: -1,
        main_at: -1,
        warn_off_at: -1,
        geo,
        ..BatcaveBats::default()
    });
}

fn bats_ref(w: &World, slot: u32) -> &BatcaveBats {
    match &w.stage_run.as_ref().expect("a stage run").actors[slot as usize].own {
        StageActorOwn::BatcaveBats(b) => b,
        o => panic!("stage actor {slot} is not a BatcaveBats: {o:?}"),
    }
}

fn bats_mut(w: &mut World, slot: u32) -> &mut BatcaveBats {
    match &mut w.stage_run.as_mut().expect("a stage run").actors[slot as usize].own {
        StageActorOwn::BatcaveBats(b) => b,
        o => panic!("stage actor {slot} is not a BatcaveBats: {o:?}"),
    }
}

fn index_of(w: &World, slot: u32) -> i32 {
    i32::try_from(w.stage_run.as_ref().expect("a stage run").actors[slot as usize].index).expect("an index")
}

/// BeginPlay @213 (`create`, AllowHazards given): the first ActivateBats timer.
pub fn bats_begin(w: &mut World, slot: u32) {
    let g = bats_ref(w, slot).geo.clone();
    let wait = stage_random_range(w, g.activate[0], g.activate[1]);
    let index = index_of(w, slot);
    stage_set_timer(w, index, KEY_ACTIVATE, wait, false, EV_ACTIVATE);
}

/// The fighter's root capsule against an axis-aligned box on the play plane (as `chars::bugs::safe`).
fn capsule_in_box(w: &World, v: FighterId, x: Real, y: Real, hx: Real, hy: Real) -> bool {
    let r = cap_radius(w, v);
    let half = capsule_half(w, v) - r;
    let (vx, ay) = (w.fighter(v).body.x, actor_y(w, v));
    let dx = max(max(x - hx - vx, 0.0), vx - (x + hx));
    let dy = max(max(y - hy - (ay + half), 0.0), ay - half - (y + hy));
    size2d([dx, dy]) < r
}

/// The overlap list now (unverified, see the head).
fn overlaps(w: &mut World, slot: u32) {
    let b = bats_ref(w, slot).geo.bx;
    let all = all_list(w);
    for &f in &all {
        let now = b.is_some_and(|b| {
            w.fighter(f).life.dead <= 0.0 && !no_collision(w, f) && capsule_in_box(w, f, b[0], b[1], b[2], b[3])
        });
        let id = w.fighter(f).id;
        let a = bats_mut(w, slot);
        let i = a.inside.iter().position(|&v| v == id);
        if now && i.is_none() {
            a.inside.push(id);
        } else if !now && let Some(i) = i {
            let last = a.inside.pop().expect("a list holding an index");
            if i < a.inside.len() {
                a.inside[i] = last;
            }
        }
    }
    put_fighters(w, all);
}

/// ReceiveTick @1983 (`StageEntry::pre_tick`: the entry lists both swarms in `tickers`).
pub fn bats_tick(w: &mut World, owner: i32) {
    let Some(slot) = slot_of(w, owner) else { return };
    overlaps(w, slot);
    if !bats_ref(w, slot).on {
        return;
    }
    for id in bats_ref(w, slot).inside.clone() {
        let Some(f) = w.by_id(id) else { continue };
        if invulnerable(w, f) {
            continue;
        }
        apply_direct_damage(w, f, damage(), None);
        // the DoOnce @1439: its first pass only marks it initialised (@1882), the gate starts open
        let a = bats_mut(w, slot);
        if !a.gate_init {
            a.gate_init = true;
        }
        if a.gate_closed {
            continue;
        }
        a.gate_closed = true;
        let fid = w.fighter(f).id;
        w.events.push(Event::Sound {
            fighter: fid,
            ev: name!("sfx_map_batcave_bats_hit"),
            x: None,
            y: None,
            hit: None,
            pooled: false,
            key: None,
        });
        stage_delay(w, owner, KEY_GATE, 0.2, EV_GATE);
    }
}

/// A due Delay or timer of a swarm (`ev` `BATS_EV` and up).
pub fn bats_timer(w: &mut World, slot: u32, ev: i32) {
    let g = bats_ref(w, slot).geo.clone();
    let index = index_of(w, slot);
    let tick = w.flow.tick;
    match ev {
        EV_ACTIVATE => {
            // BatsWarningVisuals @2074
            bats_mut(w, slot).warn_at = tick;
            if g.on >= 0 {
                set_warning_state(&mut own_mut(w).vents, g.on);
            }
            w.events.push(Event::Sound {
                fighter: -1,
                ev: name!("sfx_map_batcave_bats_flying"),
                x: Some(g.audio[0]),
                y: Some(g.audio[1]),
                hit: None,
                pooled: false,
                key: Some(ak_key(index)),
            });
            stage_delay(w, index, KEY_MAIN, 1.5, EV_MAIN);
        }
        EV_MAIN => {
            bats_mut(w, slot).main_at = tick;
            stage_delay(w, index, KEY_ON, 1.5, EV_ON);
        }
        EV_ON => {
            bats_mut(w, slot).on = true;
            stage_delay(w, index, KEY_OFF, g.duration, EV_OFF);
        }
        EV_OFF => {
            bats_mut(w, slot).on = false;
            stage_delay(w, index, KEY_WARN_OFF, 2.0, EV_WARN_OFF);
        }
        EV_WARN_OFF => {
            bats_mut(w, slot).warn_off_at = tick;
            if g.off >= 0 {
                set_warning_state(&mut own_mut(w).vents, g.off);
            }
            let wait = stage_random_range(w, g.reactivate[0], g.reactivate[1]);
            stage_set_timer(w, index, KEY_ACTIVATE, wait, false, EV_ACTIVATE);
        }
        EV_GATE => {
            let a = bats_mut(w, slot);
            a.gate_closed = false;
            a.gate_init = true;
        }
        _ => {}
    }
}

/// The Env_Audio_Emit child actor's own game object (`'batcaveBats' + index`).
fn ak_key(index: i32) -> SoundKey {
    SoundKey { tag: name!("batcaveBats"), n: index }
}
