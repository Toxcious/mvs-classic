//! The players' start points: ParentGameMode.GetPlayerStart's PlayerStart by tag and team-sorted index, and the port's
//! own starts past the stage's four for the big matches (more than four players).

use crate::consts::SOLID_FLOOR;
use crate::setup::{self as S, MatchSetup, TeamStyle};
use crate::world::SpawnPoint;
use mvs_data::stage::{SolidKind, StageData, StageStart};

/// The starts every stage has: `Player1`..`Player4`, `Respawn1`..`Respawn4`.
const STAGE_STARTS: usize = 4;

/// The room an extra start leaves beside the start it stands by (UE units, toward the stage's centre).
// Deviation: the port's own; the beta has no fifth start.
pub const START_SPACING: f64 = 150.0;

/// Which list a start is read from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartKind {
    /// The stage's PlayerStarts tagged `Player<n>`: the match's first spawn.
    Spawn,
    /// The stage's PlayerStarts tagged `Respawn<n>`.
    Respawn,
}

/// Convert Player Index to Team Sorted Index (ParentGameMode @0-2023): only the 2v2 style sorts (the players of a team
/// before this one plus the sizes of the earlier teams, then 0, 2, 1, 3); the others use the player index (or the
/// set-up's start). `teams`: every player's team.
// Deviation: a match of more than four players (the port's own big matches) has no row in the build's switch (its
// default is 0): there a team's k-th player takes start 2k + team, the switch's own pattern for two teams of two
// (0, 2 for the first team, 1, 3 for the second).
pub fn sorted_index(setup: &MatchSetup, teams: &[i32], i: usize) -> usize {
    if setup.team_style != TeamStyle::TwoVsTwo {
        return setup.players[i].start.map_or(i, usize::from);
    }
    let t = teams[i];
    let before = (0..i).filter(|&o| teams[o] == t).count();
    if teams.len() > STAGE_STARTS {
        return 2 * before + usize::try_from(t).expect("a team index");
    }
    let local = before + teams.iter().filter(|&&o| o < t).count();
    [0, 2, 1, 3][local]
}

/// The starts of team-sorted indexes `ns` for the team style `tag`, one per entry of `ns`: below four the stage's
/// `Player<n + 1>` / `Respawn<n + 1>`, the first one with the style's actor tag (ParentGameMode.GetPlayerStart).
/// From four on (the port's own big matches), taken in index order: the stage's own point of that index when it stands
/// [`MIN_START_GAP`] or more from every start taken before it, else the port's own ([`extra_start`]). None where the
/// stage lacks one of its four or an extra start finds no place.
// Deviation: the build reads four starts per tag. The Player5..8 / Respawn5..8 some stages hold (TrainingRoomSmall's
// Respawn5..8) are data the beta never used, and some of them stand on another start (Batcave's Player5 on Player3,
// TreeHouse's Player7 on Player3, TreeHouseSmall's Player7 / 8 on Player5 / 6, M009 small's Player5 80 from
// Player1): one is used only clear of the others.
pub fn match_starts(st: &StageData, kind: StartKind, tag: &str, ns: &[usize]) -> Option<Vec<SpawnPoint>> {
    let mut order: Vec<usize> = (0..ns.len()).collect();
    order.sort_by_key(|&i| ns[i]);
    let mut out = vec![SpawnPoint::default(); ns.len()];
    let mut taken: Vec<SpawnPoint> = Vec::with_capacity(ns.len());
    for i in order {
        let n = ns[i];
        let own = stage_start(st, kind, tag, n).map(point);
        let p = if n < STAGE_STARTS {
            own?
        } else {
            match own.filter(|p| clear(&taken, *p)) {
                Some(p) => p,
                None => extra_start(st, kind, tag, n, &taken)?,
            }
        };
        taken.push(p);
        out[i] = p;
    }
    Some(out)
}

/// How near a start of the port's own big matches may stand to another (UE units, in x and height).
// Deviation: the port's own.
pub const MIN_START_GAP: f64 = 150.0;

/// The most steps of [`START_SPACING`] an extra start looks for its place, each way.
const MAX_STEPS: u8 = 4;

/// Whether `p` stands [`MIN_START_GAP`] or more from every start of `taken`.
fn clear(taken: &[SpawnPoint], p: SpawnPoint) -> bool {
    taken.iter().all(|t| {
        let (dx, dy) = (f64::from(t.x) - f64::from(p.x), f64::from(t.y) - f64::from(p.y));
        dx * dx + dy * dy >= MIN_START_GAP * MIN_START_GAP
    })
}

/// The stage's own start of index `n`.
fn stage_start<'a>(st: &'a StageData, kind: StartKind, tag: &str, n: usize) -> Option<&'a StageStart> {
    let (list, prefix) = match kind {
        StartKind::Spawn => (&st.spawns, "Player"),
        StartKind::Respawn => (&st.respawns, "Respawn"),
    };
    let name = format!("{prefix}{}", n + 1);
    list.iter().find(|s| s.tag.as_deref() == Some(name.as_str()) && s.tags.iter().any(|t| t == tag))
}

/// A PlayerStart as the sim reads it: UE yaw -90 faces -Y = sim +x.
fn point(e: &StageStart) -> SpawnPoint {
    SpawnPoint {
        x: crate::math::narrow(-e.loc[1]),
        y: crate::math::narrow(e.loc[2]),
        facing: if e.yaw < 0.0 { 1.0 } else { -1.0 },
    }
}

/// The port's own start of team-sorted index `n` from four on: beside the stage's start `n % 4` (in a 2v2 style that
/// is a start of the same team: 2k + team), with its facing, the first place clear of the starts `taken`
/// ([`MIN_START_GAP`]) of 1, 2, .. [`MAX_STEPS`] x [`START_SPACING`] toward the stage's centre (one that would pass
/// the centre is not taken), then of as many steps away from it. A start is set on the floor there, found by a trace
/// down from the stage start's height: a spawn keeps the stage start's height above the floor under it (none: on the
/// floor), a respawn keeps the stage start's own height (a stage's respawns stand in the air at one height) and only
/// needs a floor below. None when no step finds a place.
// Deviation: the port's own starts for a fifth to eighth player.
// Unverified: the floor is the stage's data place of its walking boxes (World.ground's box, the pass-through
// platforms, the enabled box solids a fighter stands on); a stage's run that moves or switches one before the spawn
// is not followed, nor a polygon or rotated solid.
fn extra_start(st: &StageData, kind: StartKind, tag: &str, n: usize, taken: &[SpawnPoint]) -> Option<SpawnPoint> {
    let base = stage_start(st, kind, tag, n % STAGE_STARTS)?;
    let facing = point(base).facing;
    let (bx, by) = (-base.loc[1], base.loc[2]);
    let cx = st.center.as_ref().map_or(0.0, |c| -c.loc[1]);
    let inward = if cx > bx {
        1.0
    } else if cx < bx {
        -1.0
    } else {
        f64::from(facing)
    };
    let above = floor_top(st, bx, by).map_or(0.0, |t| by - t);
    let steps = (1..=MAX_STEPS).map(f64::from);
    let toward = steps.clone().map(|k| bx + inward * START_SPACING * k).filter(|&x| (cx - x) * inward >= 0.0);
    let away = steps.map(|k| bx - inward * START_SPACING * k);
    toward.chain(away).find_map(|x| {
        let top = floor_top(st, x, by)?;
        let y = match kind {
            StartKind::Spawn => top + above,
            StartKind::Respawn => by,
        };
        let p = SpawnPoint { x: crate::math::narrow(x), y: crate::math::narrow(y), facing };
        clear(taken, p).then_some(p)
    })
}
/// The trace down from (x, y): the highest top at or below `y` of the stage's walking boxes whose span holds `x`.
fn floor_top(st: &StageData, x: f64, y: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut meet = |left: f64, right: f64, top: f64| {
        if x >= left && x <= right && top <= y && best.is_none_or(|b| top > b) {
            best = Some(top);
        }
    };
    let f = &st.floor;
    meet(-(f.center[1] + f.half_extent[1]), -(f.center[1] - f.half_extent[1]), f.top);
    for p in st.platforms.iter().flatten() {
        meet(-p.y[1], -p.y[0], p.top);
    }
    for s in st.solids.iter().flatten() {
        if s.kind != SolidKind::Box || s.flags & SOLID_FLOOR == 0 || s.enabled == Some(false) {
            continue;
        }
        meet(-(s.center[1] + s.half_extent[1]), -(s.center[1] - s.half_extent[1]), s.center[2] + s.half_extent[2]);
    }
    best
}

/// Whether stage `st` has a start and a respawn for every player of every big match of team style `style` a lobby
/// can make ([`match_starts`]): the stage's four with the style's tag, and a place clear of the others for each extra
/// one. A lobby leaves a stage without them out of its big matches.
pub fn big_match_starts_ok(st: &StageData, style: TeamStyle) -> bool {
    let tag = S::START_TAGS[style as usize];
    big_match_indexes(style).iter().all(|ns| {
        match_starts(st, StartKind::Spawn, tag, ns).is_some() && match_starts(st, StartKind::Respawn, tag, ns).is_some()
    })
}

/// The team-sorted indexes ([`sorted_index`]) of every match of more than [`STAGE_STARTS`] players of `style`: Free
/// For All's player indexes of 5 to [`S::MAX_PLAYERS`] players; in 2v2, every split of up to four a team (the larger
/// team is the match's team 0: its k-th player takes 2k, the other team's 2k + 1). The extra starts are placed in
/// index order clear of those before, so a split's set is judged as its own, not as a part of the eight.
pub fn big_match_indexes(style: TeamStyle) -> Vec<Vec<usize>> {
    let most = S::MAX_PLAYERS;
    if style != TeamStyle::TwoVsTwo {
        return (STAGE_STARTS + 1..=most).map(|n| (0..n).collect()).collect();
    }
    let side = most / 2;
    let mut out = Vec::new();
    for a in 1..=side {
        for b in 1..=a {
            if a + b > STAGE_STARTS {
                let mut ns: Vec<usize> = (0..a).map(|k| 2 * k).chain((0..b).map(|k| 2 * k + 1)).collect();
                ns.sort_unstable();
                out.push(ns);
            }
        }
    }
    out
}
