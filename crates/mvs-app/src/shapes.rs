//! (the port's own) The sim's shapes as it tests them, drawn over the match above the HUD:
//! the Lab Tools' Display page (lab_tools.rs). Nothing here runs unless a switch is on; the sim is read (the box
//! sampler and the hit shapes fill buffers of this file's own or new lists).
//!
//! The shapes are those of the latest tick. A fighter's are moved by the distance between its drawn place of this
//! frame (the HUD's point: between the last two ticks) and the tick's, so they sit on the drawn mesh; the pose is the
//! tick's.
//! Not drawn: the clone shapes of a Disguise's second mesh, the points a pawn with its normal hitboxes off leaves (its
//! shapes show at their full size).
// Deviation: the shapes are drawn as `mvs_ui::canvas` draws (its own note). A World actor's radius, hit box and hit
// spheres are read by class (`radius`, the AirFlap's, the Dogpile's and the Banana Guard's), not from any object's
// fields and methods of those names.

use mvs_sim::World;
use mvs_sim::combat::boxes::{CH_ALL, WorldSphere};
use mvs_sim::combat::packet::HitPacket;
use mvs_sim::fighter::FighterId;
use mvs_ui::canvas::{Canvas, Hex, LABEL_STEP};
use mvs_view::HudPoint;

const HIT: Hex = [0xff, 0x3b, 0x30];
const HURT: Hex = [0xff, 0xd6, 0x0a];
const HURT_OFF: Hex = [0x8e, 0x8e, 0x93];
const INVULN: Hex = [0x5a, 0xc8, 0xfa];
const CAPSULE: Hex = [0xff, 0xff, 0xff];
const ACTOR_HIT: Hex = [0xff, 0x9f, 0x0a];
const ACTOR_HURT: Hex = [0x30, 0xd1, 0x58];
const PICKUP: Hex = [0x64, 0xd2, 0xff];
const SOLID: Hex = [0xbf, 0x5a, 0xf2];
const GROUND: Hex = [0x30, 0xd1, 0x58];
const PLATFORM: Hex = [0x34, 0xc7, 0xb9];
const BLAST: Hex = [0xff, 0x45, 0x3a];
/// The outer blast box: BLAST at 0x80 alpha ('#ff453a80').
const BLAST_OUTER_ALPHA: f64 = 128.0 / 255.0;
const SPAWN: Hex = [0x0a, 0x84, 0xff];
const WHITE: Hex = [0xff, 0xff, 0xff];

/// The Display screen's shape switches (ShowKey), held by the Lab's tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LabShow {
    pub hit: bool,
    pub hurt: bool,
    pub capsule: bool,
    pub actors: bool,
    pub stage: bool,
    pub labels: bool,
}

/// The switches of the Display page, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowKey {
    Hit,
    Hurt,
    Capsule,
    Actors,
    Stage,
    Labels,
}

impl ShowKey {
    pub const ALL: [ShowKey; 6] =
        [ShowKey::Hit, ShowKey::Hurt, ShowKey::Capsule, ShowKey::Actors, ShowKey::Stage, ShowKey::Labels];

    /// The colour the switch's shapes are drawn in (the Display page shows it beside the switch): the stage's ground
    /// for its collision, the labels' white.
    pub fn color(self) -> Hex {
        match self {
            ShowKey::Hit => HIT,
            ShowKey::Hurt => HURT,
            ShowKey::Capsule => CAPSULE,
            ShowKey::Actors => ACTOR_HIT,
            ShowKey::Stage => GROUND,
            ShowKey::Labels => WHITE,
        }
    }

    pub fn get(self, s: &LabShow) -> bool {
        match self {
            ShowKey::Hit => s.hit,
            ShowKey::Hurt => s.hurt,
            ShowKey::Capsule => s.capsule,
            ShowKey::Actors => s.actors,
            ShowKey::Stage => s.stage,
            ShowKey::Labels => s.labels,
        }
    }

    pub fn set(self, s: &mut LabShow, v: bool) {
        *match self {
            ShowKey::Hit => &mut s.hit,
            ShowKey::Hurt => &mut s.hurt,
            ShowKey::Capsule => &mut s.capsule,
            ShowKey::Actors => &mut s.actors,
            ShowKey::Stage => &mut s.stage,
            ShowKey::Labels => &mut s.labels,
        } = v;
    }
}

/// What a frame's overlay draws on: the canvas, and where the frame's fighters are drawn.
pub struct Overlay<'a> {
    pub canvas: Canvas<'a>,
    /// Each player's HUD point of this frame (None: not shown).
    pub at: &'a [Option<HudPoint>],
    /// Where the frame is between the last two ticks.
    pub alpha: f64,
}

impl<'a> std::ops::Deref for Overlay<'a> {
    type Target = Canvas<'a>;
    fn deref(&self) -> &Canvas<'a> {
        &self.canvas
    }
}

impl std::ops::DerefMut for Overlay<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.canvas
    }
}

/// The pose buffer of the box sampler (kept from frame to frame).
#[derive(Debug, Default)]
pub struct ShapesState {
    pose: Vec<f32>,
    spheres: Vec<WorldSphere>,
    ids: Vec<mvs_sim::actors::ActorId>,
    /// The labels' places of this frame (pixels: x, top).
    label_at: Vec<[f64; 3]>,
}

/// `n` as the labels show it: one decimal, JavaScript's shortest number text.
fn n1(x: f64) -> String {
    let r = (x * 10.0 + 0.5).floor() / 10.0 + 0.0;
    format!("{r}")
}

fn fighter_shapes(o: &mut Overlay<'_>, st: &mut ShapesState, w: &mut World, f: FighterId, i: usize, show: &LabShow) {
    use mvs_sim::fighter as fi;
    let Some(at) = o.at.get(i).copied().flatten() else { return };
    if w.fighter(f).life.dead != 0.0 {
        return;
    }
    // the drawn mesh's place against the tick's (the HUD's point is the mesh's: meshWX / meshWY)
    let (ox, oy) = (at.x - f64::from(fi::mesh_wx(w, f)), at.y - f64::from(fi::mesh_wy(w, f)));
    let x = w.fighter(f).body.x;
    if show.capsule {
        let r = fi::cap_radius(w, f);
        let s = (fi::capsule_half(w, f) - r).max(0.0);
        let ay = fi::actor_y(w, f);
        o.capsule(
            [f64::from(x) + ox, f64::from(ay) - f64::from(s) + oy, 0.0],
            [f64::from(x) + ox, f64::from(ay) + f64::from(s) + oy, 0.0],
            f64::from(r),
            CAPSULE,
            true,
        );
    }
    if !show.hit && !show.hurt {
        return;
    }
    let ch = w.fighter(f).ch;
    let key = w.names.str(ch).to_owned();
    let data = w.data.clone();
    let Some(d) = data.chars.get(&key) else { return };
    st.pose.clear();
    st.pose.resize(d.boxes.stride as usize, 0.0);
    if !fi::anim::sample_boxes(w, f, &mut st.pose) {
        return;
    }
    let (xx, yy) = (at.x, at.y);
    let facing = w.fighter(f).body.facing_dir;
    if show.hurt {
        let col = if fi::invulnerable(w, f) { INVULN } else { HURT };
        let Some(sampler) = w.fixed.samplers.get(&ch) else { return };
        let pose = &st.pose;
        let p = |k: usize| f64::from(pose[k]);
        for &k in &sampler.hurt {
            let sh = &d.boxes.shapes[k as usize];
            let o0 = sampler.offsets[k as usize] as usize;
            let off = w.fighter(f).hit.hurt_off.contains(&(k as i32));
            let c = if off { HURT_OFF } else { col };
            if sh.capsule {
                o.capsule(
                    [xx + p(o0) * f64::from(facing), yy + p(o0 + 1), p(o0 + 2)],
                    [xx + p(o0 + 3) * f64::from(facing), yy + p(o0 + 4), p(o0 + 5)],
                    sh.radius * p(o0 + 6),
                    c,
                    off,
                );
            } else {
                o.circle(xx + p(o0) * f64::from(facing), yy + p(o0 + 1), p(o0 + 2), sh.radius * p(o0 + 3), c, off);
            }
        }
        // (the floor bounce's hurt capsule on the root capsule)
        if let Some(e) = fi::extra_hurt(w, f) {
            o.capsule(
                [f64::from(e.x) + ox, f64::from(e.y0) + oy, 0.0],
                [f64::from(e.x) + ox, f64::from(e.y1) + oy, 0.0],
                f64::from(e.r),
                col,
                false,
            );
        }
    }
    if show.hit {
        let hits: Vec<_> = w.fighter(f).montages.active_hits.values().cloned().collect();
        let pose = std::mem::take(&mut st.pose);
        for h in &hits {
            let props = mvs_sim::combat::hit::hit_props(w, f, h);
            let packet = HitPacket::from_props(&w.names, &props, Some(h.n));
            // (the call writes only the World's warnings, `warned`: a tag with no shape;
            // its readers, the Disguise's clone shapes included, take the World unchanged)
            for s in mvs_sim::combat::phases::hit_spheres(
                w,
                f,
                &packet,
                &pose,
                CH_ALL,
                0.0,
                mvs_sim::math::narrow(f64::INFINITY),
                None,
            ) {
                let z = s.z.unwrap_or(0.0);
                if let Some(b) = &s.bx {
                    o.box3([f64::from(s.x) + ox, f64::from(s.y) + oy, f64::from(z)], &b.map(f64::from), HIT);
                } else if let Some(g) = s.seg {
                    o.capsule(
                        [f64::from(s.x) + ox, f64::from(s.y) + oy, f64::from(z)],
                        [f64::from(g[0]) + ox, f64::from(g[1]) + oy, f64::from(g[2])],
                        f64::from(s.r),
                        HIT,
                        false,
                    );
                } else {
                    o.circle(f64::from(s.x) + ox, f64::from(s.y) + oy, f64::from(z), f64::from(s.r), HIT, false);
                }
            }
        }
        st.pose = pose;
    }
}

fn actor_shapes(o: &mut Overlay<'_>, st: &mut ShapesState, w: &mut World) {
    use mvs_sim::actors::projectile as pj;
    let a = o.alpha;
    let lerp = |p: f64, x: f64| p + (x - p) * a;
    for (_, pr) in w.actors.projectiles.iter() {
        if pr.dead {
            continue;
        }
        let (x, y) = (lerp(f64::from(pr.prev_x), f64::from(pr.x)), lerp(f64::from(pr.prev_y), f64::from(pr.y)));
        // The shapes where the sim tests them (hurt_x / hurt_y, hit_shapes: off the root for the Bugs rockets).
        if pr.hurt_radius > 0.0 {
            o.circle(
                x + f64::from(pj::hurt_x(pr) - pr.x),
                y + f64::from(pj::hurt_y(pr) - pr.y),
                0.0,
                f64::from(pr.hurt_radius),
                ACTOR_HURT,
                false,
            );
        }
        if pr.hit_radius > 0.0 {
            st.spheres.clear();
            pj::hit_shapes(w, pr, &mut st.spheres);
            for s in std::mem::take(&mut st.spheres) {
                o.circle(x + f64::from(s.x - pr.x), y + f64::from(s.y - pr.y), 0.0, f64::from(s.r), ACTOR_HIT, false);
            }
        } else {
            o.cross(x, y, ACTOR_HIT);
        }
        // (its solid boxes with their collision on, where the frame's head placed them)
        for s in pr.solids.iter().flatten() {
            if s.on && !pr.no_collision {
                o.rect(
                    f64::from(s.left),
                    f64::from(s.right),
                    f64::from(s.bottom),
                    f64::from(s.top),
                    SOLID,
                    1.0,
                    false,
                    false,
                );
            }
        }
        o.label(x, y, &[format!("{} #{}", w.names.str(pr.cls), pr.seq)], ACTOR_HIT, 10.0);
    }
    for (_, it) in w.actors.items.iter() {
        if it.dead {
            continue;
        }
        let (x, y) = (lerp(f64::from(it.prev_x), f64::from(it.x)), lerp(f64::from(it.prev_y), f64::from(it.y)));
        let def = it.def(&w.data);
        o.circle(
            x + f64::from(it.pickup_x(def)) - f64::from(it.x),
            y + f64::from(it.pickup_y(def)) - f64::from(it.y),
            0.0,
            f64::from(it.pickup_radius(def)),
            PICKUP,
            true,
        );
        let hurt = it.hurt_radius();
        if hurt > 0.0 {
            o.circle(x, y, 0.0, f64::from(hurt), ACTOR_HURT, false);
        }
        let hit = it.hit_radius(def.p);
        if hit > 0.0 {
            o.circle(x, y, 0.0, f64::from(hit), ACTOR_HIT, false);
        }
        o.label(x, y, &[format!("{} #{}", w.names.str(it.cls), it.seq)], PICKUP, 10.0);
    }
    let mark = |o: &mut Overlay<'_>, cls: &str, seq: i32, x: f64, y: f64| {
        o.cross(x, y, ACTOR_HIT);
        o.label(x, y, &[format!("{cls} #{seq}")], ACTOR_HIT, 10.0);
    };
    for (_, m) in w.actors.markers.iter().filter(|(_, m)| !m.dead) {
        mark(o, w.names.str(m.cls), m.seq, f64::from(m.x), f64::from(m.y));
    }
    for (_, m) in w.actors.ships.iter().filter(|(_, m)| !m.dead) {
        mark(o, w.names.str(m.cls), m.seq, f64::from(m.x), f64::from(m.y));
    }
    for (_, m) in w.actors.mortars.iter().filter(|(_, m)| !m.dead) {
        mark(o, w.names.str(m.cls), m.seq, f64::from(m.x), f64::from(m.y));
    }
    world_actor_shapes(o, st, w);
    for (_, m) in w.actors.aim_punches.iter().filter(|(_, m)| !m.dead) {
        mark(
            o,
            w.names.str(m.cls),
            m.seq,
            lerp(f64::from(m.prev_x), f64::from(m.x)),
            lerp(f64::from(m.prev_y), f64::from(m.y)),
        );
    }
    for (_, m) in w.actors.accelerators.iter().filter(|(_, m)| !m.dead) {
        mark(o, w.names.str(m.cls), m.seq, f64::from(m.x), f64::from(m.y));
    }
}

/// The World's actors: a SpawnedPlatformParent child's solid boxes, then for one with a
/// place its root capsule as a pawn (`pawnBody`) or else the circle of its radius, its oriented hit box
/// (`hitBoxShape`: the Banana Guard's spear) and its hit spheres (`hitSpheres`: the AirFlap's, the Dogpile's), each
/// grey while its hit boxes are off, the cross and the label.
fn world_actor_shapes(o: &mut Overlay<'_>, st: &mut ShapesState, w: &mut World) {
    use mvs_sim::actors::WorldActorState as S;
    use mvs_sim::registry::entries::ActorHooks;
    let a = o.alpha;
    st.ids.clear();
    st.ids.extend(w.actors.actors.iter().filter(|(_, p)| !p.dead).map(|(id, _)| id));
    for i in 0..st.ids.len() {
        let id = st.ids[i];
        let Some(p) = w.actors.actors.get(id) else { continue };
        // (a SpawnedPlatformParent child's solid boxes, where they carry their sides)
        for s in p.plat.iter().flat_map(|pl| pl.solids()) {
            o.rect(
                f64::from(s.left),
                f64::from(s.right),
                f64::from(s.bottom),
                f64::from(s.top),
                SOLID,
                1.0,
                false,
                false,
            );
        }
        let Some((ox, oy, _)) = mvs_sim::actors::world_actor::world_actor_place_of(w, id) else { continue };
        let (x, y) = match &p.state {
            S::AimMarker(m) => (
                f64::from(m.prev_x) + f64::from(m.x - m.prev_x) * a,
                f64::from(m.prev_y) + f64::from(m.y - m.prev_y) * a,
            ),
            _ => (f64::from(ox), f64::from(oy)),
        };
        let (dx, dy) = (x - f64::from(ox), y - f64::from(oy));
        let hooks = match w.registry.actors.get(w.names.str(p.cls)).map(|e| e.hooks) {
            Some(ActorHooks::World(h)) => Some(h),
            _ => None,
        };
        if let Some(pb) = hooks.and_then(|h| h.pawn_body).and_then(|f| f(w, id)) {
            o.capsule(
                [(f64::from(pb.x) + dx), (f64::from(pb.y0) + dy), 0.0],
                [(f64::from(pb.x) + dx), (f64::from(pb.y1) + dy), 0.0],
                f64::from(pb.r),
                ACTOR_HURT,
                false,
            );
        } else if let Some(r) = radius(&p.state).filter(|&r| r > 0.0) {
            o.circle(x, y, 0.0, r, ACTOR_HIT, false);
        }
        let (cls, seq) = (p.cls, p.seq);
        let off = |on: bool| if on { ACTOR_HIT } else { HURT_OFF };
        st.spheres.clear();
        let mut sphere_col = ACTOR_HIT;
        let mut spear = None;
        match &p.state {
            S::AirFlap(s) => {
                sphere_col = off(s.hit_on);
                if let Some(f) = hooks.and_then(|h| h.hit_shapes) {
                    f(w, id, &mut st.spheres);
                }
            }
            S::Dogpile(s) => {
                sphere_col = off(s.hit_on);
                let r = s.radius * mvs_sim::chars::taz::dogpile::world_scale(w, id);
                st.spheres.push(WorldSphere { x: s.x, y: s.y, z: None, r, ..WorldSphere::default() });
            }
            S::BananaGuard(g) => spear = Some(off(g.hit_on)),
            _ => {}
        }
        // (the spear's pose fills the guard's clip cache on its first call: static data, not state)
        if let Some(col) = spear
            && let Some(hb) = mvs_sim::items::banana_guard::spear_pose(w, id)
            && let Some(b) = &hb.bx
        {
            o.box3(
                [(f64::from(hb.x) + dx), (f64::from(hb.y) + dy), f64::from(hb.z.unwrap_or(0.0))],
                &b.map(f64::from),
                col,
            );
        }
        for s in &st.spheres {
            o.circle(
                f64::from(s.x) + dx,
                f64::from(s.y) + dy,
                f64::from(s.z.unwrap_or(0.0)),
                f64::from(s.r),
                sphere_col,
                false,
            );
        }
        o.cross(x, y, ACTOR_HIT);
        o.label(x, y, &[format!("{} #{}", w.names.str(cls), seq)], ACTOR_HIT, 10.0);
    }
}

/// The first of `hitRadius`, `radius` and `r` a World actor's class holds.
// Unverified: the Rust classes' fields keep their names (`radius`, `r`; TurretBomb's `hitRadius` getter is its
// sphere's radius at its scale); a field renamed in a port draws no circle here.
fn radius(s: &mvs_sim::actors::WorldActorState) -> Option<f64> {
    use mvs_sim::actors::WorldActorState as S;
    Some(f64::from(match s {
        S::TurretBomb(b) => b.sphere_r * b.scale,
        S::ProjectileBarrier(s) => s.radius,
        S::SmokeGrenade(s) => s.radius,
        S::SmokeZone(s) => s.radius,
        S::SnackBox(s) => s.radius,
        S::ScrubShield(s) => s.radius,
        S::AntenaBolts(s) => s.radius,
        S::BracerShield(s) => s.radius,
        S::HarleyBomb(s) => s.radius,
        S::DisguiseShock(s) => s.radius,
        S::KillPie(s) => s.radius,
        S::ForceField(s) => s.radius,
        S::PortalAimer(s) => s.radius,
        S::PolymorphGrenade(s) => s.radius,
        S::Dogpile(s) => s.radius,
        S::HealZone(s) => s.radius,
        S::ElectricZone(s) => s.radius,
        S::JakeSpitAllyHitBox(s) => s.radius,
        S::SpitHitBox(s) => s.radius,
        S::Evidence(s) => s.radius,
        S::Ray(s) => s.radius,
        S::AllySaw(s) => s.radius,
        S::BananaGuard(s) => s.radius,
        S::Nepter(s) => s.radius,
        S::HighFiveShockwave(s) => s.r,
        S::GemBoostBlast(s) => s.r,
        S::RocketExplosion(s) => s.r,
        _ => return None,
    }))
}

fn stage_shapes(o: &mut Overlay<'_>, w: &World) {
    let s = &w.stage;
    let g = &s.ground;
    o.rect(g.left, g.right, g.bottom, g.top, GROUND, 1.0, false, true);
    for p in &s.stage_platforms {
        o.rect(
            f64::from(p.left),
            f64::from(p.right),
            f64::from(p.bottom),
            f64::from(p.top),
            PLATFORM,
            1.0,
            false,
            true,
        );
    }
    let (b, bo) = (&s.blast, &s.blast_outer);
    o.rect(f64::from(b.left), f64::from(b.right), f64::from(b.bottom), f64::from(b.top), BLAST, 1.0, false, false);
    o.rect(
        f64::from(bo.left),
        f64::from(bo.right),
        f64::from(bo.bottom),
        f64::from(bo.top),
        BLAST,
        BLAST_OUTER_ALPHA,
        true,
        false,
    );
    for (i, p) in s.spawns.iter().enumerate() {
        o.cross(f64::from(p.x), f64::from(p.y), SPAWN);
        o.label(f64::from(p.x), f64::from(p.y), &[format!("spawn {}", i + 1)], SPAWN, 8.0);
    }
    for (i, p) in s.respawns.iter().enumerate() {
        o.cross(f64::from(p.x), f64::from(p.y), SPAWN);
        o.label(f64::from(p.x), f64::from(p.y), &[format!("respawn {}", i + 1)], SPAWN, 8.0);
    }
}

/// A fighter's state label over its head, moved up past an earlier one's it would overlap.
fn state_label(o: &mut Overlay<'_>, st: &mut ShapesState, w: &World, f: FighterId, i: usize) {
    use mvs_sim::fighter as fi;
    let Some(at) = o.at.get(i).copied().flatten() else { return };
    let fm = w.fighter(f);
    if fm.life.dead != 0.0 {
        return;
    }
    let invuln = fi::invulnerable(w, f);
    let mo = fm.montages.montage.as_ref();
    let mut v = format!("v {}, {}", n1(f64::from(fm.body.vx)), n1(f64::from(fm.body.vy)));
    if fm.hit.hitstun > 0.0 {
        v += &format!(" · hitstun {}", fm.hit.hitstun);
    }
    if fm.hit.hitpause > 0.0 {
        v += &format!(" · hitpause {}", fm.hit.hitpause);
    }
    if invuln {
        v += " · invulnerable";
    }
    let lines = [
        format!(
            "P{} {}{} · {} dmg",
            i + 1,
            fi::anim::update::state_name(w, f),
            if fi::grounded(w, f) { "" } else { " (air)" },
            n1(f64::from(fm.stats.damage))
        ),
        match mo {
            Some(m) => format!("{} {:.3} s (f {})", w.names.str(m.montage.name), m.pos, (m.pos * 60.0 + 1e-4).floor()),
            None => w.names.str(fm.anim.loco_anim).to_owned(),
        },
        v,
    ];
    let x = at.x;
    let y = f64::from(fi::actor_y(w, f)) + f64::from(fi::capsule_half(w, f)) + (at.y - f64::from(fi::mesh_wy(w, f)));
    let Some(p) = o.to(x, y, 0.0) else { return };
    let n = lines.len() as f64;
    let half = o.label_width(&lines) / 2.0;
    let mut top = p[1] - 8.0 - n * LABEL_STEP;
    for l in &st.label_at {
        if (l[0] - p[0]).abs() < l[2] + half + 8.0 && (l[1] - top).abs() < n * LABEL_STEP + 2.0 {
            top = l[1] - n * LABEL_STEP - 4.0;
        }
    }
    st.label_at.push([p[0], top, half]);
    o.label(x, y, &lines, WHITE, top - p[1]);
}

/// One frame's overlay, after the HUD's draw.
pub fn draw(o: &mut Overlay<'_>, st: &mut ShapesState, w: &mut World, show: &LabShow) {
    if !ShowKey::ALL.iter().any(|k| k.get(show)) {
        return;
    }
    if show.stage {
        stage_shapes(o, w);
    }
    if show.actors {
        actor_shapes(o, st, w);
    }
    for i in 0..w.players.len() {
        let f = w.players[i];
        fighter_shapes(o, st, w, f, i, show);
    }
    st.label_at.clear();
    if show.labels {
        for i in 0..w.players.len() {
            let f = w.players[i];
            state_label(o, st, w, f, i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_show_as_the_ts_labels() {
        assert_eq!(n1(1.25), "1.3");
        assert_eq!(n1(2.0), "2");
        assert_eq!(n1(-0.04), "0");
        assert_eq!(n1(-1.25), "-1.2");
    }

    #[test]
    fn every_switch_reads_and_writes_its_field() {
        let mut s = LabShow::default();
        for k in ShowKey::ALL {
            k.set(&mut s, true);
            assert!(k.get(&s));
        }
        assert_eq!(s, LabShow { hit: true, hurt: true, capsule: true, actors: true, stage: true, labels: true });
    }
}
