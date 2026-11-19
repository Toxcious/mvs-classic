//! The network's input: the match from the bot's side, a fixed count of numbers. The frame is mirrored so the target
//! stands on the +x side (the action list's sides are relative to it): every x is multiplied by the target's side.
//! The same function makes the teacher's data (`mvs-sim-trace`'s `expert-data`) and the bot's input.

use super::plan::{self, View};
use crate::Real;
use crate::fighter::FighterId;
use crate::math;
use crate::world::World;

/// The roster, in the order of the fighter one-hots. A fighter not in it has no one-hot.
pub const ROSTER: [&str; 23] = [
    crate::chars::arya::KEY,
    crate::chars::batman::KEY,
    crate::chars::black_adam::KEY,
    crate::chars::bugs::KEY,
    crate::chars::creature::KEY,
    crate::chars::finn::KEY,
    crate::chars::garnet::KEY,
    crate::chars::gizmo::KEY,
    crate::chars::harley::KEY,
    crate::chars::iron_giant::KEY,
    crate::chars::jake::KEY,
    crate::chars::lebron::KEY,
    crate::chars::marvin::KEY,
    crate::chars::morty::KEY,
    crate::chars::rick::KEY,
    crate::chars::shaggy::KEY,
    crate::chars::steven::KEY,
    crate::chars::stripe::KEY,
    crate::chars::superman::KEY,
    crate::chars::taz::KEY,
    crate::chars::tom_and_jerry::KEY,
    crate::chars::velma::KEY,
    crate::chars::wonder_woman::KEY,
];

/// The numbers of one fighter's block.
const PER_FIGHTER: usize = 22;
/// The numbers of the whole: the bot's block and the target's, where the target is, the situation, the one-hots.
pub const COUNT: usize = 2 * PER_FIGHTER + 5 + 6 + 2 * ROSTER.len();

/// A distance's scale: 1 per 1000 units.
const DIST: Real = 0.001;
/// A speed's scale.
const SPEED: Real = 0.001;
/// Damage's scale: 1 per 100 percent.
const DAMAGE: Real = 0.01;
/// Hitstun's scale (its frames).
const STUN: Real = 1.0 / 30.0;

/// Fighter `f`'s block into `out`, `t` the frame's mirror: where it stands on the stage (from the centre, above the
/// floor, to each ground edge and blast zone), its speed, damage, facing, on the floor, hitstun, jumps and air dodges
/// left, a montage playing and how far in, invulnerable, dodging, off stage, knocked out.
fn fighter(w: &World, f: FighterId, t: Real, out: &mut Vec<Real>) {
    let x = w.fighter(f);
    let b = &x.body;
    let g = &w.stage.ground;
    let (left, right, top) = (math::narrow(g.left), math::narrow(g.right), math::narrow(g.top));
    let centre = math::narrow((g.left + g.right) / 2.0);
    let bl = &w.stage.blast;
    let (blo, bhi) = (math::min(bl.left, bl.right), math::max(bl.left, bl.right));
    // ahead: the +x side of the mirrored frame
    let (edge_ahead, edge_behind) = if t > 0.0 { (right - b.x, b.x - left) } else { (b.x - left, right - b.x) };
    let (blast_ahead, blast_behind) = if t > 0.0 { (bhi - b.x, b.x - blo) } else { (b.x - blo, bhi - b.x) };
    let montage = x.montages.montage.as_ref();
    let stun = crate::fighter::in_hitstun(w, f);
    out.extend_from_slice(&[
        (b.x - centre) * t * DIST,
        (b.y - top) * DIST,
        b.vx * t * SPEED,
        b.vy * SPEED,
        x.stats.damage * DAMAGE,
        b.facing_dir * t,
        Real::from(u8::from(b.on_floor)),
        Real::from(u8::from(stun)),
        if stun { math::min(math::max(x.hit.hitstun, 0.0), 120.0) * STUN } else { 0.0 },
        math::min(Real::from(u8::try_from(x.jump.jumps.clamp(0, 9)).expect("clamped")), 3.0),
        math::min(Real::from(u8::try_from(x.air.air_evades.clamp(0, 9)).expect("clamped")), 3.0),
        Real::from(u8::from(montage.is_some())),
        montage.map_or(0.0, |m| math::min(math::narrow(m.pos), 3.0)),
        Real::from(u8::from(x.invuln.invuln > 0.0 || x.invuln.invuln_count > 0)),
        Real::from(u8::from(x.evade.evading)),
        Real::from(u8::from(plan::off_stage(w, f))),
        Real::from(u8::from(x.life.dead != 0.0)),
        edge_ahead * DIST,
        edge_behind * DIST,
        blast_ahead * DIST,
        blast_behind * DIST,
        math::min(b.y - bl.bottom, bl.top - b.y) * DIST,
    ]);
}

/// The index of fighter `f`'s key in [`ROSTER`].
fn roster_index(w: &World, f: FighterId) -> Option<usize> {
    let key = w.names.str(w.fighter(f).ch);
    ROSTER.iter().position(|&k| k == key)
}

/// The input of the bot `me` deciding from `w` with view `v`, into `out` (cleared first, [`COUNT`] numbers).
pub fn features(w: &World, me: FighterId, v: &View, out: &mut Vec<Real>) {
    out.clear();
    let t = v.t;
    fighter(w, me, t, out);
    let m = &w.fighter(me).body;
    match v.target {
        Some(e) => {
            fighter(w, e, t, out);
            let o = &w.fighter(e).body;
            let (dx, dy) = ((o.x - m.x) * t, o.y - m.y);
            out.extend_from_slice(&[dx * DIST, dy * DIST, (dx.abs() + dy.abs()) * DIST, 1.0, v.s * t]);
        }
        None => {
            out.resize(out.len() + PER_FIGHTER, 0.0);
            out.extend_from_slice(&[0.0, 0.0, 0.0, 0.0, v.s * t]);
        }
    }
    let situation = [
        plan::Situation::Hitstun,
        plan::Situation::OffStage,
        plan::Situation::Punish,
        plan::Situation::Near,
        plan::Situation::Far,
        plan::Situation::Air,
    ];
    out.extend(situation.iter().map(|&s| Real::from(u8::from(s == v.situation))));
    for f in [Some(me), v.target] {
        let at = out.len();
        out.resize(at + ROSTER.len(), 0.0);
        if let Some(i) = f.and_then(|f| roster_index(w, f)) {
            out[at + i] = 1.0;
        }
    }
    debug_assert_eq!(out.len(), COUNT, "the feature count");
}
