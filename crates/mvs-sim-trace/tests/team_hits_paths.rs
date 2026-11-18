//! pfgDoDamage on a team mate and on a team mate's actor, paths no match runs: SupermanGroundLazerDamage's Box and
//! C015Dogpile's HitBox on a team mate (the entry rows, NotifyAllyOfHit, HitEachActorOnlyOnce's set; no damage), and
//! the sass bubble's Box on a team mate's VelmaEvidence and WW_ShieldActor (their trace bodies: the Buff_ArmorPack row
//! through the ally rows @5931-6835). On fresh Worlds; without a store they check nothing.

use mvs_sim::actors::WorldActorClass;
use mvs_sim::actors::world_actor::{WorldActorState, actor_entry};
use mvs_sim::actors::{ActorId, ActorRef, ProjectileId};
use mvs_sim::chars::lebron::bubble::{Rect, box_hits};
use mvs_sim::chars::superman::laser::spawn_damage;
use mvs_sim::chars::taz::dogpile::dogpile_of;
use mvs_sim::chars::velma::evidence::spawn_evidence_actor;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::elem::elem_of;
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::notify_of;
use mvs_sim::fighter::match_side::{Foreign, SpawnRequest};
use mvs_sim::fighter::traces::TraceBody;
use mvs_sim::input::NO_INPUT;
use mvs_sim::registry::entries::ActorHooks;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::World;
use mvs_sim::{InputState, Real};
use mvs_sim_trace::harness::{self, Harness};

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), bot: false, ..MatchPlayer::default() }
}

/// A fresh World of these players, ten ticks in.
fn fresh(players: &[(&str, u8)]) -> Option<Harness> {
    let setup = MatchSetup { players: players.iter().map(|&(c, t)| player(c, t)).collect(), ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    let none = vec![NO_INPUT; players.len()];
    for _ in 0..10 {
        h.step(&none);
    }
    Some(h)
}

/// The spawn of a world actor class by `f` from `montage`'s notify whose props name the class (`None`: no notify).
fn world_actor(w: &mut World, f: FighterId, cls: &str, montage: Option<&str>) -> ActorId {
    let n = match montage {
        Some(m) => {
            let montage = w.lit(m);
            let short = cls.trim_end_matches("_C");
            (0..64)
                .map(|index| NotifyRef { montage, index })
                .find(|&n| notify_of(w, f, n).is_some_and(|x| format!("{:?}", x.p).contains(short)))
                .unwrap_or_else(|| panic!("{m} has a notify that spawns {cls}"))
        }
        None => NotifyRef { montage: w.lit("None"), index: 0 },
    };
    let r = SpawnRequest {
        cls: w.lit(cls),
        n,
        attack_id: w.fighter(f).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge: None,
        aim: None,
        props: None,
        instigator: None,
        origin: None,
    };
    mvs_sim::world::spawning::spawn_world_actor(w, f, &r).unwrap_or_else(|| panic!("{cls} spawns"))
}

/// The kind's `hits` pass of the world actor `id`.
fn run_hits(w: &mut World, id: ActorId) {
    let e = actor_entry(w, ActorRef::Actor(id)).expect("the actor's entry");
    let ActorHooks::World(k) = &e.hooks else { panic!("a world actor kind") };
    (k.hits.expect("a hits hook"))(w, id);
}

/// The world actor `id`'s trace bodies (its kind's `bodies`).
fn bodies(w: &World, id: ActorId) -> Vec<TraceBody> {
    let e = actor_entry(w, ActorRef::Actor(id)).expect("the actor's entry");
    let ActorHooks::World(k) = &e.hooks else { panic!("a world actor kind") };
    let mut out = Vec::new();
    (k.bodies.expect("a bodies hook"))(w, id, &mut out);
    out
}

/// Puts the fighter `f` at `(x, y)`.
fn put(w: &mut World, f: FighterId, x: Real, y: Real) {
    let b = &mut w.fighter_mut(f).body;
    (b.x, b.prev_x, b.y, b.prev_y) = (x, x, y, y);
}

/// The team mate (1) and the enemy (2) put on the same spot (the team mate's) inside `id`'s hit box, which is put
/// there and turned on by `place`, then its `hits` pass: (whether the team mate is in the actor's HitEachActorOnlyOnce
/// set, the team mate's damage taken, the enemy's damage taken).
fn mate_and_enemy(h: &mut Harness, id: ActorId, place: fn(&mut World, ActorId, Real, Real)) -> (bool, Real, Real) {
    let (mate, foe) = (h.fighter(1), h.fighter(2));
    let w = &mut h.w;
    let (x, y) = (w.fighter(mate).body.x, w.fighter(mate).body.y);
    put(w, foe, x, y);
    place(w, id, x, y + 100.0);
    let (d0, e0) = (w.fighter(mate).stats.damage, w.fighter(foe).stats.damage);
    run_hits(w, id);
    let mid = w.fighter(mate).id;
    let set = match &w.actors.actors.get(id).expect("the actor").state {
        WorldActorState::LaserDamage(d) => d.hit_set.contains(&mid),
        WorldActorState::Dogpile(_) => dogpile_of(w, id).expect("the cloud").hit_set.contains(&mid),
        _ => unreachable!("a laser or a cloud"),
    };
    (set, w.fighter(mate).stats.damage - d0, w.fighter(foe).stats.damage - e0)
}

#[test]
fn the_ground_laser_passes_over_a_team_mate_as_an_ally_hit() {
    let Some(mut h) = fresh(&[("superman", 0), ("shaggy", 0), ("shaggy", 1)]) else { return };
    let s = h.fighter(0);
    let w = &mut h.w;
    let sweep = world_actor(w, s, "SupermanGroundLazer_C", Some("Superman_GroundLazer_Montage"));
    let (x, y) = (w.fighter(s).body.x, w.fighter(s).body.y);
    let id = spawn_damage(w, sweep, x, y).expect("the damage actor spawns");
    let each_once = match &w.actors.actors.get(id).expect("the damage actor").state {
        WorldActorState::LaserDamage(d) => d.hit.hit_each_actor_only_once,
        _ => unreachable!("a LaserDamage"),
    };
    assert_eq!(each_once, Some(true), "SupermanGroundLazerDamage's HitBoxDamage: HitEachActorOnlyOnce");
    let (set, mate, foe) = mate_and_enemy(&mut h, id, |w, id, x, y| {
        if let WorldActorState::LaserDamage(d) = &mut w.actors.actors.get_mut(id).expect("the damage actor").state {
            (d.x, d.y, d.hit_on) = (x, y, true);
        }
    });
    assert!(foe > 0.0, "the enemy on the spot is hit: the Box meets the spot");
    assert!(set, "the team mate went through pfgDoDamage (HitEachActorOnlyOnce's set)");
    assert_eq!(mate, 0.0, "the team mate takes no damage (CanBeHitBy)");
}

#[test]
fn the_dogpile_passes_over_a_team_mate_as_an_ally_hit() {
    let Some(mut h) = fresh(&[("taz", 0), ("shaggy", 0), ("shaggy", 1)]) else { return };
    let t = h.fighter(0);
    let id = world_actor(&mut h.w, t, "C015Dogpile_C", Some("C015_Up_Special_Ground_Montage2"));
    let each_once = dogpile_of(&h.w, id).expect("the cloud").hit.hit_each_actor_only_once;
    assert_eq!(each_once, Some(true), "C015Dogpile's HitBoxDamage: HitEachActorOnlyOnce");
    let (set, mate, foe) = mate_and_enemy(&mut h, id, |w, id, x, y| {
        if let WorldActorState::Dogpile(s) = &mut w.actors.actors.get_mut(id).expect("the cloud").state {
            (s.x, s.y, s.hit_on) = (x, y, true);
        }
    });
    assert!(foe > 0.0, "the enemy on the spot is hit: the HitBox meets the spot");
    assert!(set, "the team mate went through pfgDoDamage (HitEachActorOnlyOnce's set)");
    assert_eq!(mate, 0.0, "the team mate takes no damage (CanBeHitBy)");
}

/// LeBron's (fighter 0) sass bubble at `(x, y)`.
fn bubble(w: &mut World, f: FighterId, cls: &str, x: Real, y: Real) -> ProjectileId {
    let cls = w.lit(cls);
    mvs_sim::world::spawning::comp_spawn_projectile(w, f, cls, x, y, [Some(1.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
        .expect("the bubble spawns")
}

/// The sass bubble's Box (160 each way) on the actor `id` placed at `(x, y)`, twice (the two overlap updates of one
/// activation): whether the actor holds Buff_ArmorPack after (its row's StackCount is 0: no stacks to count), and
/// whether the bubble's HitEachActorOnlyOnce list holds the actor.
fn bubble_on(h: &mut Harness, id: ActorId, x: Real, y: Real) -> (bool, bool) {
    let l = h.fighter(0);
    let w = &mut h.w;
    let pr = bubble(w, l, "C016_SassBubble_C", x, y + 400.0);
    let b = Rect { x, y, hx: 160.0, hy: 160.0, hd: 160.0 };
    box_hits(w, pr, b);
    if w.actors.projectiles.get(pr).is_some_and(|p| !p.dead) {
        box_hits(w, pr, b);
    }
    let listed = w.actors.projectiles.get(pr).is_some_and(|p| p.hit_actors.contains(&ActorRef::Actor(id)));
    (elem_of(w, ActorRef::Actor(id), BuffClass::BuffArmorPack).is_some(), listed)
}

#[test]
fn both_sass_bubbles_hit_each_actor_once() {
    let Some(mut h) = fresh(&[("lebron", 0), ("velma", 1)]) else { return };
    let (l, v) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    let (x, y) = (w.fighter(l).body.x, w.fighter(l).body.y);
    let a = bubble(w, l, "C016_SassBubble_C", x, y + 400.0);
    let b = bubble(w, v, "Velma_SassBubblev2_C", x, y + 400.0);
    for (pr, cls) in [(a, "C016_SassBubble"), (b, "Velma_SassBubblev2")] {
        let p = w.actors.projectiles.get(pr).expect("the bubble");
        assert_eq!(p.hit.hit_each_actor_only_once, Some(true), "{cls}'s HitBoxDamage: HitEachActorOnlyOnce");
    }
}

#[test]
fn the_sass_bubble_arms_a_team_mates_evidence() {
    let Some(mut h) = fresh(&[("lebron", 0), ("velma", 0), ("shaggy", 1)]) else { return };
    let v = h.fighter(1);
    let w = &mut h.w;
    let (x, y) = (w.fighter(v).body.x + 300.0, w.fighter(v).body.y + 200.0);
    let cls = w.lit("VelmaEvidence_C");
    let ev = spawn_evidence_actor(w, v, cls, x, y, 0.0, 0.0, 0.0, 0.0).expect("the evidence spawns");
    let bs = bodies(w, ev);
    assert_eq!(bs.len(), 1, "the evidence's Destructible sphere is a trace body");
    assert!(bs[0].r > 0.0 && bs[0].object_type == 18, "the sphere (r {}), HurtBox ({})", bs[0].r, bs[0].object_type);
    let (ex, ey) = (bs[0].ax, bs[0].ay);
    let (armed, listed) = bubble_on(&mut h, ev, ex, ey);
    assert!(armed, "Buff_ArmorPack (ApplyBuffHit, ApplyBuffAlly) on the team mate's evidence");
    assert!(listed, "the evidence in the bubble's HitEachActorOnlyOnce list");
}

#[test]
fn the_sass_bubble_leaves_an_enemys_evidence_unarmed() {
    let Some(mut h) = fresh(&[("lebron", 0), ("velma", 1)]) else { return };
    let v = h.fighter(1);
    let w = &mut h.w;
    let (x, y) = (w.fighter(v).body.x + 300.0, w.fighter(v).body.y + 200.0);
    let cls = w.lit("VelmaEvidence_C");
    let ev = spawn_evidence_actor(w, v, cls, x, y, 0.0, 0.0, 0.0, 0.0).expect("the evidence spawns");
    let bs = bodies(w, ev);
    let (ex, ey) = (bs[0].ax, bs[0].ay);
    assert!(!bubble_on(&mut h, ev, ex, ey).0, "no ally row on an enemy's actor");
}

#[test]
fn the_sass_bubble_arms_a_team_mates_shield() {
    let Some(mut h) = fresh(&[("lebron", 0), ("wonderwoman", 0), ("shaggy", 1)]) else { return };
    // Wonder Woman's side special (WonderWoman_ShieldRamForward_Montage) until its shield has its BlockCollision.
    let i = InputState::default;
    let side = InputState { x: 1.0, special: true, ..i() };
    let mut found = None;
    for n in 0..90 {
        h.step(&[i(), if n == 0 { side } else { i() }, i()]);
        found = h.w.actors.actors.iter().find_map(|(id, a)| {
            (!a.dead && a.class == WorldActorClass::WWShieldActor && !bodies(&h.w, id).is_empty()).then_some(id)
        });
        if found.is_some() {
            break;
        }
    }
    let id = found.expect("a WW_ShieldActor with its BlockCollision within 90 ticks of the side special");
    let e = actor_entry(&h.w, ActorRef::Actor(id)).expect("the shield's entry");
    assert!(e.spawned_meet_bodies, "a projectile's or an item's hit body meets the shield's BlockCollision");
    let bs = bodies(&h.w, id);
    assert_eq!(bs.len(), 1, "the shield's BlockCollision is a trace body");
    let (sx, sy) = (bs[0].ax, bs[0].ay);
    let (armed, listed) = bubble_on(&mut h, id, sx, sy);
    assert!(armed, "Buff_ArmorPack (ApplyBuffHit, ApplyBuffAlly) on the team mate's shield");
    assert!(listed, "the shield in the bubble's HitEachActorOnlyOnce list");
}

#[test]
fn the_evidence_meets_spawned_hit_bodies() {
    let Some(mut h) = fresh(&[("velma", 0), ("shaggy", 1)]) else { return };
    let v = h.fighter(0);
    let w = &mut h.w;
    let (x, y) = (w.fighter(v).body.x, w.fighter(v).body.y + 200.0);
    let cls = w.lit("VelmaEvidence_C");
    let ev = spawn_evidence_actor(w, v, cls, x, y, 0.0, 0.0, 0.0, 0.0).expect("the evidence spawns");
    let e = actor_entry(w, ActorRef::Actor(ev)).expect("the evidence's entry");
    assert!(e.spawned_meet_bodies, "a projectile's or an item's hit body meets the evidence's Destructible");
}
