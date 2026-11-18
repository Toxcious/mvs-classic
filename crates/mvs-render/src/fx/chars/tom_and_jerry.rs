//! Tom and Jerry's effects: the particle systems of Jerry and the
//! actors, the classes' projectile rules). The meshes, and the bones these effects ride, are mvs-view's
//! (the Jerry view enters "tj:Trail", "tj:Charge" and "tj:Neck", the dynamite "tj:Wick", the
//! trap "tj:Aura").
//!
//! Jerry_SpawnedActor:
//!   - Trail_Effect (PS_TomJerry_Tennisball_Ribbon on JerryMesh, scale 0.8333, Team @13344): shown from the spawn,
//!     hidden by JerryFlyingMulticast @27016 (the flight's end).
//!     Unverified: that end is seen as JerryActor::flying going false after it was true, or the follow.
//!   - PS_TomJerry_ChampagnePop: LaunchCork @1792 SpawnEmitterAttached(Champagne, bone Neck, Location (0, 0, 50),
//!     rot (0, 0, 0), scale 1, KeepRelativeOffset, bAutoDestroy) with Team (SelectFloat(0.6, the colour value,
//!     IsLocalOnline) @2093), at the sim's tick of the launch (the champagne state's pop_tick), following the bone.
//!   - PS_AttackChargeUP (on Champagne at Z 21, scale 0.4): Activate(true) @26723 with the bottle, Deactivate @24999 /
//!     @1756 (JerryActor::charge_up).
//!
//! MouseTrap_SpawnedActor: PS_TomJerryMouseTrapAura (no socket, Z 10 on MouseTrapMesh, scale 0.75), Team;
//! K2_DestroyComponent at Detonate @5135 (MouseTrap::aura going false; a rollback to before it makes it anew).
//!
//! Dynamite_SpawnedActor_C, CantSplitDynamite_SpawnedActor_C: the wick, BeginPlay @436
//! SpawnEmitterAttached(PS_TomJerry_Wick, DynamiteMesh, socket Wick, (0, 0, 0), rot 0, scale 1, KeepRelativeOffset),
//! destroyed @5129 with the mesh's hiding (killed when Dynamite::hidden goes true).
//!
//! The tennis balls' particle components (the shared path's, with the owner's Team): ParticleSystem shown while
//! IsParticleSystemVisible (@1809 on, @1900 off; TennisBall::particle_visible), ParticleTrail
//! (PS_TomJerry_Tennisball_Ribbon under StaticMesh) always.
//! JerryCork_C: ParticleSystem (PS_TomJerry_Tennisball_Ribbon) is the shared path's, shown while JerryCork::trail
//! (hidden at the pop @560 / @2028); ParticleSystem2 (PS_ShaggySandwichProjectile at Z 21.4, scale 0.75, bVisible
//! false; Team by SetTeamColor @2870) shown by ReflectedMulticast(true) @1937, hidden at the pop @594
//! (JerryCork::reflect_fx).
//!
//! PS_TomJerryPoof00: on the sim's TjPoof (Pawn_TomAndJerry ResetJerry complete @5646: at jerry_un_C_spine_a,
//! scale 2, no Team). Jerry's other comebacks spawn none on Tom (FishingLure EndPlay @10787 only attaches;
//! Jerry_SpawnedActor's EndPlay poof @7871 is the sim's). Its sound is the sim's.
//!
//! FishingLure_C's Cable (UCableComponent under Lure, under JerryMesh): a component mesh with
//! TomJerryFishLine_MAT and Team, see [`lure_tick`].

use std::collections::{BTreeSet, HashMap};

use mvs_data::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt, attach_parent};
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::chars::tom_and_jerry::champagne::CORK;
use mvs_sim::chars::tom_and_jerry::dynamite::{CANT_SPLIT_DYNAMITE, DYNAMITE};
use mvs_sim::chars::tom_and_jerry::fishing::LURE;
use mvs_sim::chars::tom_and_jerry::state::tj_of;
use mvs_sim::chars::tom_and_jerry::tennis::{TENNIS_BALL, TENNIS_BALL_REFLECTED};
use mvs_sim::events::Event;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows};
use mvs_view::actor_view::{rel_rows, xyz};
use mvs_view::meshes::{Cable, CableDef};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule, component_warm, fighter_of};
use crate::cmesh::{CMeshGeometry, CMeshId};
use crate::fx::{Follow, Fx};

const POOF: &str = "PS_TomJerryPoof00";
const RIBBON: &str = "PS_TomJerry_Tennisball_Ribbon";
const POP: &str = "PS_TomJerry_ChampagnePop";
const CHARGE: &str = "PS_AttackChargeUP";
const TRAP_AURA: &str = "PS_TomJerryMouseTrapAura";
const WICK: &str = "PS_TomJerry_Wick";

/// The bones the views enter (mvs-view chars/tom_and_jerry/jerry.rs TRAIL, CHARGE, NECK; actors.rs WICK, AURA).
const TRAIL_BONE: &str = "tj:Trail";
const CHARGE_BONE: &str = "tj:Charge";
const NECK_BONE: &str = "tj:Neck";
const WICK_BONE: &str = "tj:Wick";
const AURA_BONE: &str = "tj:Aura";
/// The fish line's material, the pole bone its end hangs on (the lure view enters the owner's Tom_FishingPole bone
/// as the lure's) and the Tip socket's offset along it.
const FISH_LINE: &str = "Panda_Main/Characters/TomAndJerry/VFX/TomJerryFishLine_MAT#local";
const ROD_BONE: &str = "Rod5";
const TIP_X: f64 = 49.692963;
/// One component tick.
const STEP: f64 = 1.0 / 60.0;

/// One lure's fish line.
#[derive(Debug)]
struct Line {
    c: Cable,
    id: CMeshId,
    /// The component's place in the actor (under Lure and JerryMesh), its rotation rows there and its scale.
    rel: V3,
    rot: Rows,
    scale: V3,
    /// SetAttachEndToComponent found the pole (decided at the first place).
    attached: Option<bool>,
    /// The world tick of the last component tick (-1: none yet).
    last: i32,
}

/// One Jerry's systems.
#[derive(Debug)]
struct JerryFx {
    team: f64,
    trail: Option<SystemId>,
    /// The trail was asked for (made at the view's first sight, as a view is made).
    trail_made: bool,
    pop: Option<SystemId>,
    charge: Option<SystemId>,
    /// The launch tick of the last pop drawn.
    pop_seen: i32,
    flew: bool,
    trail_off: bool,
}

/// What Tom and Jerry's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct TomAndJerry {
    jerries: HashMap<ActorId, JerryFx>,
    /// The traps' auras (None: none now).
    traps: HashMap<ActorId, Option<SystemId>>,
    /// The dynamites' wicks by the actor's sequence number (None: none now).
    wicks: HashMap<i32, Option<SystemId>>,
    /// The lures' fish lines by the actor's sequence number.
    lines: HashMap<i32, Line>,
    warned: BTreeSet<String>,
}

fn me(c: &mut CharFx) -> &mut TomAndJerry {
    match c {
        CharFx::TomAndJerry(s) => s,
        _ => unreachable!("a Tom and Jerry rule entered by another module"),
    }
}

/// A bone a view entered for actor `a`, as last drawn.
fn bone_scale(fx: &Fx, a: ActorRef, name: &str) -> Option<f64> {
    let (_, x) = fx.actor_bones.get(&a)?.iter().find(|(n, _)| &**n == name)?;
    Some(x.scale[0])
}

fn bone(a: ActorRef, name: &str, loc: [f64; 3], scale: f64) -> Follow {
    Follow::Bone { a, bone: name.to_owned(), loc, rel: IDENTITY, scale: [scale; 3] }
}

/// The system when it is loaded, else a warning once.
fn has(warned: &mut BTreeSet<String>, fx: &mut Fx, ps: &str) -> bool {
    if fx.player.emitter_count(ps).is_some() {
        return true;
    }
    if warned.insert(ps.to_owned()) {
        eprintln!("warning: Tom and Jerry: {ps} is not loaded");
    }
    false
}

fn warn(warned: &mut BTreeSet<String>, s: String) {
    if !warned.contains(&s) {
        eprintln!("warning: Tom and Jerry: {s}");
        warned.insert(s);
    }
}

impl TomAndJerry {
    fn kill_jerry(fx: &mut Fx, j: &JerryFx) {
        for h in [j.charge, j.pop, j.trail].into_iter().flatten() {
            fx.player.kill(h);
        }
    }

    fn clear(&mut self, fx: &mut Fx) {
        for (_, j) in self.jerries.drain() {
            Self::kill_jerry(fx, &j);
        }
        for h in self.traps.drain().filter_map(|(_, h)| h) {
            fx.player.kill(h);
        }
        for h in self.wicks.drain().filter_map(|(_, h)| h) {
            fx.player.kill(h);
        }
        for (_, l) in self.lines.drain() {
            fx.components.remove(l.id);
        }
    }

    /// Jerry's systems, once a sim step.
    fn jerry(&mut self, fx: &mut Fx, w: &World, id: ActorId) {
        let x = w.actors.actors.get(id).expect("a live actor");
        let WorldActorState::JerryActor(a) = &x.state else { return };
        let ar = ActorRef::Actor(id);
        let pt = tj_of(w, x.owner).map_or(-1, |s| s.champagne.pop_tick);
        let Self { jerries, warned, .. } = self;
        let j = jerries.entry(id).or_insert_with(|| JerryFx {
            team: fx.team(w.fighter(x.owner).id),
            trail: None,
            trail_made: false,
            pop: None,
            charge: None,
            pop_seen: pt,
            flew: false,
            trail_off: false,
        });
        let team = [("Team", j.team)];
        // Trail_Effect from the spawn (its place: the view's, from its first draw)
        if !j.trail_made && !j.trail_off {
            if has(warned, fx, RIBBON) {
                j.trail = fx.spawn_follow(w, RIBBON, bone(ar, TRAIL_BONE, [0.0; 3], 1.0), 0.8333333, &team);
                j.trail_made = j.trail.is_some();
            } else {
                j.trail_made = true;
            }
        }
        // PS_TomJerry_ChampagnePop at the launch
        if pt >= 0 && pt != j.pop_seen {
            j.pop_seen = pt;
            let neck = fx.actor_bones.get(&ar).is_some_and(|b| b.iter().any(|(n, _)| &**n == NECK_BONE));
            if !neck {
                warn(warned, format!("{POP} is not drawn (no Champagne_Mesh view or no bone Neck on it)"));
            } else if has(warned, fx, POP) {
                if let Some(h) = j.pop.take() {
                    fx.player.kill(h);
                }
                j.pop = fx.spawn_follow(w, POP, bone(ar, NECK_BONE, [0.0, 0.0, 50.0], 1.0), 1.0, &team);
            }
        }
        if j.pop.is_some_and(|h| !fx.player.alive(h)) {
            j.pop = None;
        }
        // PS_AttackChargeUP: Activate(true) with the bottle, Deactivate at its end
        if a.charge_up && j.charge.is_none() {
            if has(warned, fx, CHARGE) {
                j.charge = fx.spawn_follow(w, CHARGE, bone(ar, CHARGE_BONE, [0.0; 3], 0.4), 0.4, &[]);
            }
        } else if !a.charge_up
            && let Some(h) = j.charge.take()
        {
            fx.player.deactivate(h);
        }
        // Trail_Effect: hidden by JerryFlyingMulticast
        if a.flying {
            j.flew = true;
        }
        if !j.trail_off && ((j.flew && !a.flying) || a.following.is_some()) {
            j.trail_off = true;
            if let Some(h) = j.trail {
                fx.player.set_visible(h, false);
            }
        }
    }

    /// trapViews' aura, once a sim step.
    fn trap(&mut self, fx: &mut Fx, w: &World, id: ActorId) {
        let x = w.actors.actors.get(id).expect("a live actor");
        let WorldActorState::MouseTrap(t) = &x.state else { return };
        let h = self.traps.entry(id).or_default();
        match (*h, t.aura) {
            // Detonate @5135: PS_TomJerryMouseTrapAura.K2_DestroyComponent
            (Some(k), false) => {
                fx.player.kill(k);
                *h = None;
            }
            (None, true) => {
                let a = ActorRef::Actor(id);
                let Some(s) = bone_scale(fx, a, AURA_BONE) else { return };
                if !has(&mut self.warned, fx, TRAP_AURA) {
                    return;
                }
                let team = fx.team(w.fighter(x.owner).id);
                let k = fx.spawn_follow(w, TRAP_AURA, bone(a, AURA_BONE, [0.0; 3], 1.0), s, &[("Team", team)]);
                self.traps.insert(id, k);
            }
            _ => {}
        }
    }
}

impl CharFxHooks for TomAndJerry {
    /// The fishing line's cable (the warm-up, `Fx::warm`).
    fn warm(&mut self, _fx: &mut Fx, w: &World, out: &mut Vec<mvs_view::WarmMesh>) {
        if fighter_of(w, mvs_sim::chars::tom_and_jerry::KEY).is_some() {
            out.push(component_warm(None, FISH_LINE));
        }
    }

    /// The actors: Jerry views' and trap views' steps.
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        let live = |id: &ActorId| w.actors.actors.get(*id).is_some_and(|x| !x.dead);
        let gone: Vec<ActorId> = self.jerries.keys().filter(|id| !live(id)).copied().collect();
        for id in gone {
            let j = self.jerries.remove(&id).expect("listed");
            Self::kill_jerry(fx, &j);
        }
        let gone: Vec<ActorId> = self.traps.keys().filter(|id| !live(id)).copied().collect();
        for id in gone {
            if let Some(Some(h)) = self.traps.remove(&id) {
                fx.player.kill(h);
            }
        }
        let ids: Vec<(ActorId, bool)> = w
            .actors
            .actors
            .iter()
            .filter(|(_, x)| !x.dead)
            .filter_map(|(id, x)| match &x.state {
                WorldActorState::JerryActor(_) => Some((id, true)),
                WorldActorState::MouseTrap(_) => Some((id, false)),
                _ => None,
            })
            .collect();
        for (id, jerry) in ids {
            if jerry {
                self.jerry(fx, w, id);
            } else {
                self.trap(fx, w, id);
            }
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn event(&mut self, fx: &mut Fx, _w: &World, e: &Event) -> bool {
        match e {
            Event::TjPoof { x, y, scale, .. } => {
                if has(&mut self.warned, fx, POOF) {
                    fx.player.spawn(POOF, [0.0, f64::from(-x), f64::from(*y)], [0.0; 3], [f64::from(*scale); 3], &[]);
                }
                true
            }
            _ => false,
        }
    }
}

// ---- the projectile rules -----------------------------------------------------------------------------------------

fn ball_shown(_w: &World, pr: &Projectile) -> bool {
    matches!(&pr.ext, ProjectileExt::TennisBall(b) if b.particle_visible)
}

const BALL: ProjRule = ProjRule { shown: Some(ball_shown), more: &["ParticleTrail"], ..ProjRule::NONE };

fn cork_shown(_w: &World, pr: &Projectile) -> bool {
    match &pr.ext {
        ProjectileExt::JerryCork(k) => k.trail,
        _ => true,
    }
}

fn cork_tick(_c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let on =
        w.actors.projectiles.get(st.id).is_some_and(|p| matches!(&p.ext, ProjectileExt::JerryCork(k) if k.reflect_fx));
    if let Some(Some(h)) = st.more.first() {
        fx.player.set_visible(*h, on);
    }
}

const CORK_RULE: ProjRule =
    ProjRule { shown: Some(cork_shown), more: &["ParticleSystem2"], tick: Some(cork_tick), ..ProjRule::NONE };

/// The wick (a SocketFx): spawned on the socket while the mesh shows, killed at its hiding.
fn wick_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let s = me(c);
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let off = matches!(&pr.ext, ProjectileExt::Dynamite(d) if d.hidden);
    let h = s.wicks.entry(st.seq).or_default();
    if off && let Some(k) = h.take() {
        fx.player.kill(k);
    }
    if let Some(k) = *h {
        fx.player.set_visible(k, !st.hid);
        return;
    }
    if off || st.hid {
        return;
    }
    let a = ActorRef::Projectile(st.id);
    let Some(scale) = bone_scale(fx, a, WICK_BONE) else {
        warn(&mut s.warned, format!("DynamiteMesh has no bone Root ({WICK} not drawn)"));
        return;
    };
    if !has(&mut s.warned, fx, WICK) {
        return;
    }
    let k = fx.spawn_follow(w, WICK, bone(a, WICK_BONE, [0.0; 3], 1.0), scale, &[]);
    s.wicks.insert(st.seq, k);
}

fn wick_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(Some(h)) = me(c).wicks.remove(&st.seq) {
        fx.player.kill(h);
    }
}

const DYNAMITE_RULE: ProjRule = ProjRule { tick: Some(wick_tick), end: Some(wick_end), ..ProjRule::NONE };

/// FishingLure_C's Cable: BeginPlay @3334 -> @2517-3248 attaches its end to the first of the
/// Instigator's SkeletalMeshComponents whose SkeletalMesh is Tom_FishingPole, at Tip (Tom_FishingPole_Skeleton's
/// SkeletalMeshSocket_0 on Rod5 at (49.692963, 0, 0), no rotation, scale 1); with no pole then, or once it is gone,
/// the end is the component itself (EndLocation (0, 0, 0)). @6432 CreateDynamicMaterialInstance(0,
/// TomJerryFishLine_MAT), @6718 Team = SelectFloat(0.6, the Instigator's colour index, IsLocalOnline). Nothing hides
/// the Cable (SetVisualsMulticast @13146-13251 hides JerryMesh, Lure and LureNoRope only, without children); facing
/// left, JerryMesh turns by BeginPlay's rotation (-90, -180, 0) @5636 (Lure::jerry_left).
// Unverified: one component tick of 1/60 s per world tick from the first place (the spawn tick, 8 at most a step),
// its ends at the drawn frame's places (Cable::pin between ticks); the Tip from the pole view's last pose.
fn lure_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let a = ActorRef::Projectile(st.id);
    let s = me(c);
    if let std::collections::hash_map::Entry::Vacant(e) = s.lines.entry(st.seq) {
        let Some(def) = mvs_view::actors::class_def(w, a) else { return };
        let left = matches!(&pr.ext, ProjectileExt::Lure(l) if l.jerry_left);
        let cs: std::borrow::Cow<'_, Props> = if left && def.components.contains_key("JerryMesh") {
            let mut r = def.components.clone();
            if let Some(Value::Obj(j)) = r.get_mut("JerryMesh") {
                let mut rot = Props::new();
                rot.insert("Pitch".to_owned(), Value::Num(-90.0));
                rot.insert("Yaw".to_owned(), Value::Num(-180.0));
                rot.insert("Roll".to_owned(), Value::Num(0.0));
                j.insert("RelativeRotation".to_owned(), Value::Obj(rot));
            }
            std::borrow::Cow::Owned(r)
        } else {
            std::borrow::Cow::Borrowed(&def.components)
        };
        let Some(cd) = cs.get("Cable") else { return };
        let o = cd.as_obj();
        let par = attach_parent(&cs, "Cable");
        let prot: Rows = std::array::from_fn(|k| f64::from(par.rot[k]));
        let rl = xyz(o.and_then(|o| o.get("RelativeLocation")), 0.0);
        let rs = xyz(o.and_then(|o| o.get("RelativeScale3D")), 1.0);
        let crot = rel_rows(o.and_then(|o| o.get("RelativeRotation")));
        let cl = [0, 1, 2].map(|k| f64::from(par.scale[k]) * rl[k]);
        let rel =
            [0, 1, 2].map(|k| f64::from(par.loc[k]) + cl[0] * prot[k] + cl[1] * prot[3 + k] + cl[2] * prot[6 + k]);
        let team = pr.owner.map_or(0.0, |f| fx.team(w.fighter(f).id));
        let id = fx.components.add(FISH_LINE, &[("Team", team)], CMeshGeometry::empty());
        e.insert(Line {
            c: Cable::new(&CableDef::of(Some(cd))),
            id,
            rel: rel.map(f64::from),
            rot: compose_rows(&crot, &prot),
            scale: [0, 1, 2].map(|k| f64::from(par.scale[k]) * rs[k]),
            attached: None,
            last: -1,
        });
    }
    let l = s.lines.get_mut(&st.seq).expect("made above");
    // the Cable's world transform under Lure, JerryMesh and the actor (as placeUnder)
    let arot = mvs_view::actors::actor_rot(w, pr);
    let mut start: V3 = [0.0, f64::from(-pr.x), f64::from(pr.y)];
    for (k, v) in start.iter_mut().enumerate() {
        *v += f64::from(pr.scale) * (l.rel[0] * arot[k] + l.rel[1] * arot[3 + k] + l.rel[2] * arot[6 + k]);
    }
    let rot = compose_rows(&l.rot, &arot);
    let tip = fx.actor_bones.get(&a).and_then(|b| b.iter().find(|(n, _)| &**n == ROD_BONE)).map(|(_, x)| {
        let r = &x.rot;
        [0, 1, 2].map(|k| x.loc[k] + TIP_X * x.scale[0] * r[k])
    });
    let attached = *l.attached.get_or_insert(tip.is_some());
    let end = match tip {
        Some(e) if attached => e,
        _ => start,
    };
    let t = w.flow.tick;
    if l.last < 0 || t < l.last {
        l.last = t - 1; // (the first place, or a rollback)
    }
    let n = (t - l.last).min(8);
    l.last = t;
    if n > 0 {
        for _ in 0..n {
            l.c.tick(STEP, start, end);
        }
    } else {
        l.c.pin(start, end);
    }
    let shown = !st.hid;
    let Some(m) = fx.components.get_mut(l.id) else { return };
    m.shown = shown;
    if shown {
        l.c.build(start, &rot, l.scale.map(|v| v * f64::from(pr.scale)));
        m.geometry = CMeshGeometry::own(l.c.positions().to_vec(), l.c.uvs().to_vec(), l.c.indices().to_vec());
    }
}

fn lure_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(l) = me(c).lines.remove(&st.seq) {
        fx.components.remove(l.id);
    }
}

const LURE_RULE: ProjRule = ProjRule { tick: Some(lure_tick), end: Some(lure_end), ..ProjRule::NONE };

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (LURE, LURE_RULE),
    (TENNIS_BALL, BALL),
    (TENNIS_BALL_REFLECTED, BALL),
    (CORK, CORK_RULE),
    (DYNAMITE, DYNAMITE_RULE),
    (CANT_SPLIT_DYNAMITE, DYNAMITE_RULE),
];
