//! The shared stage wall classes (`world::wall_bp`) on Cromulons' speakers, on the World of the stage-m009 trace:
//! the create, the damage and its stages, the destroy and its Delay's link, the binds and the two broadcasts, and the
//! ground trace's cast to DestructibleObjectBP, a proximity mine stuck to a speaker. Without a store or the trace the tests check nothing and say so.
//! A speaker the stage's own create has not made (its module not merged yet) is made here from its placed entry.

use mvs_sim::actors::ActorRef;
use mvs_sim::actors::StageActorClass;
use mvs_sim::events::Event;
use mvs_sim::fighter::traces::{TRACE_PROJECTILE_GROUND, sphere_trace_single};
use mvs_sim::stages::cromulons::Speaker;
use mvs_sim::world::World;
use mvs_sim::world::stage_actors::{StageActor, StageActorOwn};
use mvs_sim::world::wall_bp::{
    WALL_DELAY, WallActor, WallDelegate, ground_destructible, is_destructible_object, is_wall_bp, wall,
    wall_any_damage, wall_bind, wall_create, wall_delayed, wall_destroy_destructible, wall_hurt, wall_of_owner,
    wall_unbind,
};
use mvs_sim_trace::harness::{self, Harness};

const FLAG_FN: &str = "C018Flag_C.OnStructureDestroyed";
const PIE_FN: &str = "AryaKillPie_C.Destroy";

/// The DamageAudio child's RelativeLocation of M009_DestructiblePlatformBP (only the place of the sound reads it).
const AUDIO_REL: [mvs_sim::Real; 3] = [0.0, 0.0, 0.0];

/// The World of stage-m009 with its two speakers as wall actors (the stage's own, or made here).
fn speakers() -> Option<(Harness, Vec<WallActor>)> {
    let mut h = harness::at("stage-m009", 1)?;
    let w = &mut h.w;
    let mut out: Vec<WallActor> = (0..w.stage_run.as_ref()?.actors.len())
        .filter_map(|i| is_destructible_object(w, ActorRef::Stage(u32::try_from(i).unwrap())))
        .collect();
    if out.is_empty() {
        let placed = w.data.stage.placed.clone().unwrap_or_default();
        for (index, p) in placed.iter().enumerate() {
            let cls = match p.cls.as_str() {
                "M009_DestructiblePlatformBP_C" => StageActorClass::M009DestructiblePlatformBP,
                "M009_DestructiblePlatformBP_Child_C" => StageActorClass::M009DestructiblePlatformBPChild,
                _ => continue,
            };
            let run = w.stage_run.as_mut().expect("Cromulons has a stage run");
            let slot = u32::try_from(run.actors.len()).unwrap();
            run.actors.push(StageActor {
                cls,
                index: u32::try_from(index).unwrap(),
                team: -1,
                dead: false,
                x: 0.0,
                y: 0.0,
                own: StageActorOwn::None,
            });
            let a = WallActor(slot);
            let part = wall_create(w, a, AUDIO_REL);
            #[allow(clippy::needless_update, reason = "the stage's track adds the speaker's own fields")]
            let s = Speaker { wall: part, ..Speaker::default() };
            w.stage_run.as_mut().unwrap().actors[slot as usize].own = StageActorOwn::Speaker(s);
            out.push(a);
        }
    }
    assert_eq!(out.len(), 2, "Cromulons has two speakers");
    Some((h, out))
}

fn hit(w: &mut World, a: WallActor) {
    wall_any_damage(w, a, 1000.0, wall_destroy_destructible, None);
}

fn warned(w: &World) -> Vec<String> {
    let mut v = w.warned.borrow_mut();
    let out = v.iter().map(|s| s.to_string()).collect();
    v.clear();
    out
}

/// wallCreate: HP from the first stage, the DamageBox as the hurt shape, the montages by object name, the root from
/// the placed transform; both speakers pass the cast to WallBP.
#[test]
fn a_speaker_is_made_from_its_placed_data() {
    let Some((h, sp)) = speakers() else { return };
    let w = &h.w;
    for &a in &sp {
        assert!(is_wall_bp(w, ActorRef::Stage(a.0)));
        let p = wall(w, a);
        let stages = &p.geo.stages;
        assert!(!stages.is_empty() && p.hp == stages[0] && p.stage == 0 && !p.destroyed, "{p:?}");
        assert!(!p.damage_montage.is_empty() && !p.damage_montage.contains('.'), "{}", p.damage_montage);
        assert!(!p.geo.hurt.is_empty(), "a DamageBox");
        let mut out = Vec::new();
        assert!(wall_hurt(w, a, &mut out) && out.len() == p.geo.hurt.len());
        let i = w.stage_run.as_ref().unwrap().actors[a.0 as usize].index;
        assert_eq!(wall_of_owner(w, i32::try_from(i).unwrap()), Some(a));
    }
}

/// wallAnyDamage: a hit takes the HP down and plays the damage visuals; HP at 0 goes to the next stage's HP; past the
/// last stage DestroyDestructible runs once (Destroyed_0, the Delay(0.1)), a later hit only counts the stage on. The
/// Delay's link turns the solids and the hurt box off.
#[test]
fn a_speaker_breaks_by_stages() {
    let Some((mut h, sp)) = speakers() else { return };
    let w = &mut h.w;
    let a = sp[0];
    let n = wall(w, a).geo.stages.len();
    let hp0 = wall(w, a).hp;
    w.events.clear();
    wall_any_damage(w, a, 0.25, wall_destroy_destructible, None); // (Stages[0].HP is 1 on the speakers)
    let p = wall(w, a);
    assert!(p.hp < hp0 && p.stage == 0 && p.hit_at == w.flow.tick && p.hit_montage == p.damage_montage);
    if p.geo.damage_audio.is_some() {
        assert!(w.events.iter().any(|e| matches!(e, Event::Sound { fighter: -1, pooled: true, .. })));
    }
    for k in 1..=n {
        hit(w, a);
        assert_eq!(wall(w, a).stage, i32::try_from(k).unwrap());
    }
    let p = wall(w, a);
    assert!(p.destroyed && p.broken_at == w.flow.tick && p.off_at < 0);
    let owner = i32::try_from(w.stage_run.as_ref().unwrap().actors[a.0 as usize].index).unwrap();
    let delays = |w: &World| {
        w.stage_run
            .as_ref()
            .unwrap()
            .timers
            .iter()
            .filter(|t| t.delay && t.owner == owner && t.key == WALL_DELAY)
            .count()
    };
    assert_eq!(delays(w), 1, "the Delay(0.1)");
    hit(w, a);
    assert_eq!(wall(w, a).stage, i32::try_from(n + 1).unwrap());
    assert_eq!(delays(w), 1, "no second Delay");
    wall_delayed(w, a);
    let p = wall(w, a);
    assert_eq!(p.off_at, w.flow.tick);
    let mut out = Vec::new();
    assert!(!wall_hurt(w, a, &mut out));
    let run = w.stage_run.as_ref().unwrap();
    for &id in &p.geo.solids {
        assert!(!run.solids[usize::try_from(id).unwrap()].enabled, "solid {id} off");
    }
    assert!(!wall(w, sp[1]).destroyed, "the other speaker is untouched");
}

/// The binds: AddUnique, an unentered function warned and not bound, unbind; StructureDestroyed and ObjectDestroyed
/// call a live listener's bound function (Marvin's flag's OnStructureDestroyed destroys the projectile) and drop a
/// gone one.
#[test]
fn the_wall_delegates_bind_and_broadcast() {
    let Some((mut h, sp)) = speakers() else { return };
    let a = sp[0];
    let gone = ActorRef::Projectile(mvs_sim::actors::ProjectileId::new(u32::MAX, u32::MAX));
    {
        let w = &mut h.w;
        assert!(wall(w, a).listeners.is_none(), "no list before the first bind");
        wall_bind(w, a, WallDelegate::ObjectDestroyed, gone, PIE_FN);
        wall_bind(w, a, WallDelegate::ObjectDestroyed, gone, PIE_FN);
        wall_bind(w, a, WallDelegate::ObjectDestroyed, gone, "Test_C.NotEntered");
        assert_eq!(wall(w, a).listeners.as_ref().map(Vec::len), Some(1), "AddUnique; the unentered one not bound");
        assert!(warned(w).iter().any(|m| m.contains("Test_C.NotEntered")));
        wall_unbind(w, a, WallDelegate::ObjectDestroyed, gone, PIE_FN);
        assert_eq!(wall(w, a).listeners.as_ref().map(Vec::len), Some(0));
        wall_bind(w, a, WallDelegate::ObjectDestroyed, gone, PIE_FN);
    }
    // a live projectile of the match to listen
    let mut live = None;
    for _ in 0..3000 {
        if let Some((id, _)) = h.w.actors.projectiles.iter().find(|(_, p)| !p.dead) {
            live = Some(id);
            break;
        }
        h.step_recorded();
    }
    let w = &mut h.w;
    let n = wall(w, a).geo.stages.len();
    let Some(pid) = live else {
        eprintln!("note: stage-m009 has no projectile: the call of a bound function is not checked");
        for _ in 0..=n {
            hit(w, a);
        }
        assert_eq!(wall(w, a).listeners.as_ref().map(Vec::len), Some(0), "a gone listener is dropped");
        return;
    };
    let me = ActorRef::Projectile(pid);
    wall_bind(w, a, WallDelegate::StructureDestroyed, me, FLAG_FN);
    assert_eq!(wall(w, a).listeners.as_ref().map(Vec::len), Some(2));
    for _ in 0..=n {
        hit(w, a);
    }
    assert!(wall(w, a).destroyed);
    assert!(w.actors.projectiles.get(pid).is_none_or(|p| p.dead), "StructureDestroyed called the flag's function");
    let ls = wall(w, a).listeners.clone().unwrap();
    assert!(ls.iter().all(|l| l.listener == me), "the gone listener is dropped: {ls:?}");
}

/// groundDestructible: a ProjectileGroundTrace down onto a speaker meets its solid and casts to it; one onto the stage
/// block casts to nothing.
#[test]
fn a_ground_trace_finds_the_speaker_under_it() {
    let Some((mut h, sp)) = speakers() else { return };
    let w = &mut h.w;
    for &a in &sp {
        let b = &wall(w, a).geo.hurt[0];
        let mvs_sim::world::stage_actors::StageHurt::Box(s) = b else { panic!("a box") };
        let (x, top) = (s.x, s.y + s.bx.unwrap()[4]);
        let hit = sphere_trace_single(w, x, top + 200.0, x, top - 175.0, 70.0, TRACE_PROJECTILE_GROUND, &[], None);
        assert_eq!(ground_destructible(w, hit.as_ref()), Some(a), "{hit:?}");
    }
    let hit = sphere_trace_single(w, 0.0, 500.0, 0.0, -1000.0, 70.0, TRACE_PROJECTILE_GROUND, &[], None);
    assert!(hit.is_some(), "the stage block");
    assert_eq!(ground_destructible(w, hit.as_ref()), None);
}

/// Item_ProximityMine's ReceiveHit on a DestructibleObjectBP (@1724-1822): a dropped mine that lands on a speaker arms
/// and binds the speaker's ObjectDestroyed to its DestroyWithAttached; the speaker's break detonates it (the blast's
/// event) and releases it.
#[test]
fn a_mine_on_a_speaker_detonates_when_it_breaks() {
    use mvs_sim::input::NO_INPUT;
    use mvs_sim::world::items::{drop_held_item, spawn_and_equip_item};
    let Some((mut h, sp)) = speakers() else { return };
    let a = sp[0];
    let f = h.fighter(0);
    // (the items' data is read with random items on)
    std::sync::Arc::make_mut(&mut h.w.setup).mutators.push("Mutator_RandomItemSpawns".to_owned());
    let cls = h.w.lit("Item_ProximityMine_C");
    let it = spawn_and_equip_item(&mut h.w, f, cls).expect("a mine spawns");
    let inputs = vec![NO_INPUT; h.w.players.len()];
    h.step(&inputs); // (held for a step: the drop below is its Drop)
    drop_held_item(&mut h.w, f);
    let b = &wall(&h.w, a).geo.hurt[0];
    let mvs_sim::world::stage_actors::StageHurt::Box(s) = b else { panic!("a box") };
    let (x, top) = (s.x, s.y + s.bx.unwrap()[4]);
    {
        let m = h.w.actors.items.get_mut(it).expect("the mine lives");
        (m.x, m.y, m.vx, m.vy) = (x, top + 60.0, 0.0, 0.0);
    }
    let bound = |h: &Harness| {
        wall(&h.w, a).listeners.as_ref().is_some_and(|ls| {
            ls.iter().any(|l| l.listener == ActorRef::Item(it) && l.f == "Item_ProximityMine_C.DestroyWithAttached")
        })
    };
    for _ in 0..60 {
        h.step(&inputs);
        if bound(&h) {
            break;
        }
    }
    assert!(bound(&h), "the mine landed on the speaker and bound its ObjectDestroyed");
    let w = &mut h.w;
    w.events.clear();
    let n = wall(w, a).geo.stages.len();
    for _ in 0..=n {
        hit(w, a);
    }
    assert!(w.actors.items.get(it).is_none_or(|m| m.dead), "the mine is released");
    assert!(
        w.events.iter().any(|e| matches!(e, Event::ProjectileDeath { cls: c, .. } if *c == cls)),
        "the blast's event"
    );
}
