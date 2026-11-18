//! The particle components of the items part's actors that no class rule draws
//! (the meshes: mvs_view's items class view).
//!
//! Each particle component that starts shown and active (bAutoActivate, bVisible, not bHiddenInGame: Item_Snowball's
//! PS_Creature_SnowBall_Projectile, Item_CarWheel's ParticleSystem1) is made at its component's place under the
//! actor's root, follows the actor, is hidden while the actor is and is killed with it. Left to others: an item's
//! ParticleSystem (ItemActorParent's thrown trail, which BeginPlay hides @157: Fx::item_trail) and an owned
//! projectile's (Fx::place_projectiles). Only ItemActorParent's SetTeamColor gives a Team (to ParticleSystem @11480);
//! these have none.
//! A projectile's are made ahead of the step's events and the particle tick (a
//! new projectile's view is placed at once); an item's after the tick (items are left to the frame).
//! Unverified: an item's are made at the first step that sees it shown, not at the actor's spawn tick.
//! Deviation: a held item's components follow the holder's mesh place under the item's rotation, not the holder's hold
//! socket as the item's meshes do.
//!
//! The FFA cars' rules ([`RULES`]; the cars: mvs-sim items/car.rs, their meshes mvs_view
//! items/car.rs), for Parent_Car_SpawnedActor_C, BatMobile_SpawnedActor_Child_C and MysteryMachine_SpawnedCar_C:
//! - ParticleSystem (PS_C023A_OverheatCar, bVisible false, on the SkeletalMesh's socket corvette_front, which neither
//!   mesh has: the component's own place): the shared component draw (Fx::place_projectiles), shown by
//!   OverheatVisuals @13200 (SetOverheatState @137: the sim's has_overheat), as Gizmo's car's (chars/gizmo.rs).
//! - BatMobile's ThrusterEffect (PS_SchwiftyThruster on the bone Body of the car's mesh at (0.20, 371.07, 36.48), roll
//!   -90, scale (0.5, 0.5, 1.25), bVisible false): made at the first step the car is shown and its view has the bone,
//!   no Team; shown while the sim's thruster is (SpecialInputMulticast @999 SetVisibility(true), CheckToStopBoost @1210
//!   SetVisibility(false)) and the car is, killed with the car. Tried once per car: a mesh without Body or class data
//!   without ThrusterEffect warns and draws none.
//!   Unverified: checked once a World step, from the bones of the step's poses: the system starts a step after the
//!   car's view (the step that makes the view has no bones yet).
//!   Unverified: it simulates while hidden (bAutoActivate; the template's bStartWithTickEnabled is false, and whether a
//!   hidden particle component ticks is a native rule not read).

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::projectile::Projectile;
use mvs_sim::actors::{ActorRef, ProjectileExt};
use mvs_sim::chars::gizmo::car::car_of;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::items::car::{BATMOBILE, MYSTERY_MACHINE, PARENT_CAR};
use mvs_vfx::SystemId;
use mvs_vfx::math::IDENTITY;
use mvs_view::actor_view::{comp_of, place_under};
use mvs_view::actors::{actor_place, class_def, class_view_draws, item_shown};
use mvs_view::item_particle_components;

use super::attach::{rot_rows, text, warn_once, xyz};
use super::chars::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use super::{Follow, Fx};

// ---- the FFA cars ------------------------------------------------------------------------------------------------

const THRUSTER: &str = "ThrusterEffect";
const THRUSTER_BONE: &str = "Body";

/// The cars' rules, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (PARENT_CAR, ProjRule { shown: Some(overheated), ..ProjRule::NONE }),
    (
        BATMOBILE,
        ProjRule { shown: Some(overheated), tick: Some(thruster_tick), end: Some(thruster_end), ..ProjRule::NONE },
    ),
    (MYSTERY_MACHINE, ProjRule { shown: Some(overheated), ..ProjRule::NONE }),
];

/// What the cars' effects keep between steps: BatMobile's ThrusterEffect, by car projectile seq (None: tried and not
/// drawn).
#[derive(Debug, Default)]
pub(crate) struct ItemCars {
    thrusters: IndexMap<i32, Option<SystemId>>,
}

impl CharFxHooks for ItemCars {
    /// The Lab's reset has ended the systems: the handles are dropped.
    fn reset(&mut self, _fx: &mut Fx) {
        self.thrusters.clear();
    }

    fn end(&mut self, fx: &mut Fx) {
        for h in self.thrusters.drain(..).filter_map(|(_, h)| h) {
            fx.player.kill(h);
        }
    }
}

fn me(c: &mut CharFx) -> &mut ItemCars {
    match c {
        CharFx::ItemCars(s) => s,
        _ => unreachable!("an item car rule entered by another module"),
    }
}

fn overheated(_w: &World, pr: &Projectile) -> bool {
    !pr.dead && car_of(&pr.ext).is_some_and(|c| c.has_overheat)
}

/// The car view's `place` for the ThrusterEffect: hidden while the car is; made at the first shown step whose pose
/// has the bone; shown while the sim's thruster is.
fn thruster_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let a = ActorRef::Projectile(st.id);
    let shown = actor_place(w, a).is_some_and(|p| !p.hidden);
    let s = me(c);
    // (once the view has posed: the step that makes it has no bones yet)
    if shown && !s.thrusters.contains_key(&st.seq) && fx.actor_bones.contains_key(&a) {
        let h = thruster_spawn(fx, w, pr, a);
        s.thrusters.insert(st.seq, h);
    }
    if let Some(&Some(h)) = s.thrusters.get(&st.seq) {
        let on = shown && matches!(&pr.ext, ProjectileExt::BatMobileCar(b) if b.thruster);
        fx.player.set_visible(h, on);
    }
}

/// The ThrusterEffect on the car's bone Body, no Team; None (warned) where the class data has no
/// ThrusterEffect or the pose no Body.
fn thruster_spawn(fx: &mut Fx, w: &World, pr: &Projectile, a: ActorRef) -> Option<SystemId> {
    let cls = w.names.str(pr.cls);
    let comp = actor_def(w, a, pr.cls).and_then(|d| d.components.get(THRUSTER)).and_then(|v| v.as_obj());
    let Some((template, comp)) = comp.and_then(|c| Some((text(c, "Template")?, c))) else {
        warn_once(format!("{cls}: no {THRUSTER} component in the class data"));
        return None;
    };
    if !fx.actor_bones.get(&a).is_some_and(|b| b.iter().any(|(n, _)| n.eq_ignore_ascii_case(THRUSTER_BONE))) {
        warn_once(format!("{cls}: {THRUSTER}'s bone {THRUSTER_BONE} is not in the mesh"));
        return None;
    }
    let f = Follow::Bone {
        a,
        bone: THRUSTER_BONE.to_owned(),
        loc: xyz(comp.get("RelativeLocation"), 0.0),
        rel: rot_rows(comp.get("RelativeRotation")),
        scale: xyz(comp.get("RelativeScale3D"), 1.0),
    };
    fx.spawn_follow(w, template, f, 1.0, &[])
}

fn thruster_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(Some(h)) = me(c).thrusters.shift_remove(&st.seq) {
        fx.player.kill(h);
    }
}

// ---- the class view's components --------------------------------------------------------------------------------

/// One actor's components: the system of each (None until it is first shown).
#[derive(Debug)]
struct ClassFx {
    comps: Vec<(String, Follow, Option<SystemId>)>,
    seen: bool,
}

/// The components of every actor the class view draws, by actor, and the steps' scratch lists.
#[derive(Debug, Default)]
pub(crate) struct ClassFxs {
    by: IndexMap<ActorRef, ClassFx>,
    /// The step's live actors of the pass's kind, each with whether it is shown.
    live: Vec<(ActorRef, bool)>,
    gone: Vec<ActorRef>,
}

/// The components of actor `a` that start shown and active (mvs_view's list), with their follows.
fn components(w: &World, a: ActorRef) -> Vec<(String, Follow, Option<SystemId>)> {
    let Some(d) = class_def(w, a) else { return Vec::new() };
    let mut out = Vec::new();
    for (name, template) in item_particle_components(w, a) {
        let Some(comp) = comp_of(&d.components, &name) else { continue };
        // (the component's transform under the actor's root: Follow::Actor puts it under the actor's place)
        let xf = place_under(&comp, &IDENTITY, 1.0, 0.0, 0.0);
        out.push((template, Follow::Actor { turned: true, a, rel: xf.loc, rel_rot: xf.rot, scale: xf.scale }, None));
    }
    out
}

impl Fx {
    /// classView's `place` for the particle components of the projectiles (`projectiles`, ahead of the step's
    /// events) or of the items (after the particle tick): made at the first step the actor is shown, hidden with
    /// it, killed when it goes.
    pub(super) fn place_class_items(&mut self, w: &World, projectiles: bool) {
        let mut s = std::mem::take(&mut self.class_fx);
        let kind = |r: &ActorRef| matches!(r, ActorRef::Projectile(_)) == projectiles;
        for (_, r) in s.by.iter_mut().filter(|(k, _)| kind(k)) {
            r.seen = false;
        }
        s.live.clear();
        let a = &w.actors;
        if projectiles {
            for (id, _) in a.projectiles.iter().filter(|(_, x)| !x.dead) {
                let r = ActorRef::Projectile(id);
                if class_view_draws(w, r) {
                    s.live.push((r, actor_place(w, r).is_some_and(|p| !p.hidden)));
                }
            }
        } else {
            for (id, it) in a.items.iter().filter(|(_, x)| !x.dead) {
                let r = ActorRef::Item(id);
                if class_view_draws(w, r) {
                    s.live.push((r, item_shown(w, it)));
                }
            }
        }
        for &(r, shown) in &s.live {
            let st = s.by.entry(r).or_insert_with(|| ClassFx { comps: components(w, r), seen: false });
            st.seen = true;
            for (template, f, h) in &mut st.comps {
                if h.is_none() && shown {
                    *h = self.spawn_follow(w, template, f.clone(), 1.0, &[]);
                }
                if let Some(h) = *h {
                    self.player.set_visible(h, shown);
                }
            }
        }
        s.gone.clear();
        s.gone.extend(s.by.iter().filter(|(k, r)| kind(k) && !r.seen).map(|(k, _)| *k));
        for r in &s.gone {
            let Some(st) = s.by.shift_remove(r) else { continue };
            for h in st.comps.into_iter().filter_map(|(_, _, h)| h) {
                self.player.kill(h);
            }
        }
        self.class_fx = s;
    }

    /// The match's end (Fx::end): every component goes.
    pub(super) fn end_class_items(&mut self) {
        for (_, st) in self.class_fx.by.drain(..) {
            for h in st.comps.into_iter().filter_map(|(_, _, h)| h) {
                self.player.kill(h);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! The cars' rules over ffa-items-b's first Mystery Machine (the Rust baseline), set overheated, then made a
    //! boosting BatMobile: no trace match overheats a car, and none of the Rust baseline has a BatMobile.
    use mvs_sim::actors::ProjectileExt;
    use mvs_sim::items::car::{BATMOBILE, BatMobileCar};
    use mvs_view::{FighterPose, ViewPoser};

    use super::super::Fx;
    use super::super::chars::CharFx;

    fn hidden(fx: &Fx, h: mvs_vfx::SystemId) -> bool {
        fx.player.system(h).expect("a live system").hidden
    }

    /// One presented tick: the views placed, the effects stepped on their poses.
    fn present(poser: &mut ViewPoser, w: &mvs_sim::World, fx: &mut Fx) {
        let (mut fighters, mut actors) = (Vec::<FighterPose>::new(), Vec::new());
        poser.place(w);
        poser.present(w, 1.0, &mut fighters, &mut actors);
        fx.step(w, &fighters, &actors).expect("the step");
    }

    #[test]
    fn the_overheat_and_the_thruster_show() {
        let car = |w: &mvs_sim::World| {
            w.actors.projectiles.iter().any(|(_, p)| !p.dead && matches!(p.ext, ProjectileExt::ItemCar(_)))
        };
        let Some(mut hs) = mvs_sim_trace::harness::at_baseline_where("ffa-items-b", 0, car) else { return };
        let store = mvs_store::Store::open(mvs_store::dev_store_dir()).expect("the dev store");
        let w = &mut hs.w;
        let (id, seq) = w
            .actors
            .projectiles
            .iter()
            .find(|(_, p)| !p.dead && matches!(p.ext, ProjectileExt::ItemCar(_)))
            .map(|(id, p)| (id, p.seq))
            .expect("the Mystery Machine");
        let pr = w.actors.projectiles.get_mut(id).expect("the car");
        let ProjectileExt::ItemCar(c) = &mut pr.ext else { unreachable!() };
        c.car.has_overheat = true;
        let mut poser = ViewPoser::new(&store, w).expect("the views");
        let mut fx = Fx::new(&store, 1).expect("the player");
        present(&mut poser, w, &mut fx);
        let h = fx.projectiles.get(&seq).and_then(|st| st.h).expect("the car's ParticleSystem");
        assert_eq!(&*fx.player.system(h).expect("live").t.name, "PS_C023A_OverheatCar");
        assert!(!hidden(&fx, h), "shown once overheated");

        // the same car as a boosting BatMobile, a new actor (seq) for the effects, new views (theirs are by actor)
        let bat_cls = w.lit(BATMOBILE);
        let pr = w.actors.projectiles.get_mut(id).expect("the car");
        let ProjectileExt::ItemCar(c) = std::mem::take(&mut pr.ext) else { unreachable!() };
        let mut bat = BatMobileCar { thruster: true, ..BatMobileCar::default() };
        bat.item.car = c.car;
        bat.item.car.has_overheat = false;
        pr.ext = ProjectileExt::BatMobileCar(bat);
        pr.cls = bat_cls;
        pr.seq += 1000;
        let seq = pr.seq;
        // (a step for the view of the new actor, a step for the bone its first pose gives)
        let mut poser = ViewPoser::new(&store, w).expect("the views");
        present(&mut poser, w, &mut fx);
        present(&mut poser, w, &mut fx);
        let thruster = |fx: &mut Fx| {
            fx.chars.iter().find_map(|c| match c {
                CharFx::ItemCars(s) => s.thrusters.get(&seq).copied().flatten(),
                _ => None,
            })
        };
        let t = thruster(&mut fx).expect("the ThrusterEffect on the bone Body");
        assert_eq!(&*fx.player.system(t).expect("live").t.name, "PS_SchwiftyThruster");
        assert!(!hidden(&fx, t), "shown while the thruster is");
        let h = fx.projectiles.get(&seq).and_then(|st| st.h).expect("the BatMobile's ParticleSystem");
        assert!(hidden(&fx, h), "the overheat hidden");
        let pr = w.actors.projectiles.get_mut(id).expect("the car");
        let ProjectileExt::BatMobileCar(b) = &mut pr.ext else { unreachable!() };
        b.thruster = false;
        present(&mut poser, w, &mut fx);
        assert!(hidden(&fx, t), "hidden once the boost ends");
    }
}
