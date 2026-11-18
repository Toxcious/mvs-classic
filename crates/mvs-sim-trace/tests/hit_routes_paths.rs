//! pfgDoDamage's routes for a dealer or a victim no match runs (`combat::spawned_hits`): the ally rows of a team
//! mate's projectile on an item (@5931-6835), the FastFallSlam buff's handler as the dealer on an item no fighter
//! holds, Item_Dynamite's blast on a World.actors actor (HarleyJackInTheBox, whose ReceiveAnyDamage sets it off) and
//! VelmaHealBeam's clank (@20004) of its final blast on an enemy grenade. On fresh Worlds; without a store they check
//! nothing.

use mvs_sim::Real;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef, ItemId};
use mvs_sim::chars::wonder_woman::perks::SlamDealer;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::buffs::{BuffExt, buff_mut, spawn_buff};
use mvs_sim::combat::elem::elem_of;
use mvs_sim::combat::hits::{DamageCall, do_damage};
use mvs_sim::combat::packet::{HitPacket, OnHitBuffRow};
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::notify_of;
use mvs_sim::fighter::match_side::{Foreign, SpawnRequest};
use mvs_sim::input::NO_INPUT;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::World;
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

/// A pfgDoDamage call with no HitBox and these rows.
fn call(rows: Option<&[OnHitBuffRow]>) -> DamageCall<'_> {
    DamageCall { hit_box: false, comp: None, clank: false, rows }
}

/// A shot of `f`'s at `(x, y)`.
fn grenade(w: &mut World, f: FighterId, x: Real, y: Real) -> mvs_sim::actors::ProjectileId {
    let cls = w.lit("C019_CookGrenade_C");
    mvs_sim::world::spawning::comp_spawn_projectile(w, f, cls, x, y, [Some(1.0), Some(0.0), Some(0.0)], 0.0, 0.0, None)
        .expect("the grenade spawns")
}

/// A sandwich of `f`'s, 300 above it.
fn sandwich(w: &mut World, f: FighterId) -> ItemId {
    let cls = w.lit("Item_Sandwich_C");
    let (x, y) = (w.fighter(f).body.x, mvs_sim::fighter::actor_y(w, f));
    mvs_sim::world::items::spawn_item(w, cls, f, x, y + 300.0, 0.0, 0.0, false, None).expect("the sandwich")
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

/// Morty's grenade on a sandwich of Shaggy's (his team mate), with one row of Buff_DecrementEvadeCount (Velma's data):
/// whether the sandwich holds the buff after.
fn ally_row(apply_ally: bool) -> Option<bool> {
    let mut h = fresh(&[("morty", 0), ("shaggy", 0), ("velma", 1)])?;
    let (m, s) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    let it = sandwich(w, s);
    let (x, y) = (w.fighter(m).body.x, mvs_sim::fighter::actor_y(w, m));
    let g = grenade(w, m, x, y + 300.0);
    let row = OnHitBuffRow {
        applied_buffs: w.lit("Buff_DecrementEvadeCount_C"),
        apply_buff_self: false,
        apply_buff_hit: true,
        apply_buff_ally: apply_ally,
        apply_buff_enemy: false,
        require_hit_to_apply_buff_to_self: false,
        stack_count: Some(1),
        buff_duration: Some(1.0),
    };
    let p = HitPacket { damage: Some(5.0), ..HitPacket::default() };
    do_damage(w, ActorRef::Projectile(g), ActorRef::Item(it), &p, &call(Some(&[row])));
    Some(elem_of(w, ActorRef::Item(it), BuffClass::BuffDecrementEvadeCount).is_some())
}

#[test]
fn a_team_mates_ally_row_lands_on_his_item() {
    let Some(on) = ally_row(true) else { return };
    let Some(off) = ally_row(false) else { return };
    assert!(on, "ApplyBuffHit and ApplyBuffAlly: the row's buff on the team mate's item (@5931-6835)");
    assert!(!off, "without ApplyBuffAlly: nothing");
}

#[test]
fn the_slam_buff_deals_on_an_item_no_fighter_holds() {
    let Some(mut h) = fresh(&[("wonderwoman", 0), ("shaggy", 1)]) else { return };
    let (ww, s) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    let cls = w.lit("Buff_Perk_WonderWoman_FastFallSlam_C");
    let b = spawn_buff(w, ww, cls, 0.0, None, Some(ww), false, Some(ww)).expect("the slam buff spawns");
    // (the landing's HitBoxesOn, as `slam_hits` makes it)
    let (x, y) = (w.fighter(ww).body.x, mvs_sim::fighter::actor_y(w, ww));
    match &mut buff_mut(w, b).expect("the slam buff").ext {
        BuffExt::FastFallSlam(e) => e.dealer = Some(SlamDealer { x, y, facing: 1.0, owner: ww }),
        _ => unreachable!("the slam buff holds a Slam"),
    }
    let it = sandwich(w, s);
    // (as the FFA spawner's: no owner, its class looked up through a fighter of the match)
    let i = w.actors.items.get_mut(it).expect("the sandwich");
    i.owner = None;
    i.lookup_fighter = Some(s);
    let p = HitPacket { damage: Some(5.0), destroy_projectiles: Some(true), ..HitPacket::default() };
    let hit = do_damage(w, ActorRef::Buff(b), ActorRef::Item(it), &p, &call(None));
    assert!(hit, "CanBeHitBy: the buff's team (Wonder Woman's) against the item's -1");
}

#[test]
fn the_dynamite_blast_sets_off_a_jack_in_the_box() {
    let Some(mut h) = fresh(&[("bugs", 0), ("harley", 1)]) else { return };
    let (bugs, harley) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    let jack = world_actor(w, harley, "HarleyJackInTheBox_C", Some("HarleyQuinn_Special_JackInTheBox_Montage"));
    let jack_at = |w: &World| match &w.actors.actors.get(jack).expect("the jack").state {
        WorldActorState::JackBox(j) => (j.x, j.y, j.exploded),
        _ => unreachable!("HarleyJackInTheBox_C holds a JackBox"),
    };
    let (x, y, exploded) = jack_at(w);
    assert!(!exploded);
    let cls = w.lit("Item_Dynamite_C");
    let it = mvs_sim::world::items::spawn_item(w, cls, bugs, x, y + 50.0, 0.0, 0.0, false, None).expect("the dynamite");
    mvs_sim::actors::item::item_throw_hit(w, it, false, false);
    mvs_sim::world::items::item_throw_hits(w);
    assert!(jack_at(w).2, "the blast's pfgDoDamage on the jack: its ReceiveAnyDamage @8011");
}

/// VelmaHealBeam's final blast packet on an enemy C019_CookGrenade, the beam's ProjectileAttackType `t`: (Hit, the
/// grenade's flag IsProjectileClanking after).
fn beam_on_grenade(t: i32) -> Option<(bool, bool)> {
    let mut h = fresh(&[("velma", 0), ("morty", 1)])?;
    let (v, m) = (h.fighter(0), h.fighter(1));
    let w = &mut h.w;
    let beam = world_actor(w, v, "VelmaHealBeam_C", None);
    let packet = match &mut w.actors.actors.get_mut(beam).expect("the beam").state {
        WorldActorState::Beam(b) => {
            assert_eq!(b.attack_type, 2, "the class default ProjectileAttackType");
            b.attack_type = t;
            b.final_hit.clone()
        }
        _ => unreachable!("VelmaHealBeam_C holds a Beam"),
    };
    let (x, y) = (w.fighter(m).body.x, mvs_sim::fighter::actor_y(w, m));
    let g = grenade(w, m, x, y + 300.0);
    let hit = do_damage(w, ActorRef::Actor(beam), ActorRef::Projectile(g), &packet, &call(Some(&[])));
    let clanking = w.actors.projectiles.get(g).is_some_and(|p| p.clanking);
    Some((hit, clanking))
}

#[test]
fn the_final_blast_alone_clanks_an_enemy_grenade() {
    let Some(charging) = beam_on_grenade(2) else { return };
    let Some(blast) = beam_on_grenade(0) else { return };
    assert_eq!(charging, (false, false), "type 2: BPI_CanProjectileClank false, no DestroyProjectiles: HitBy alone");
    assert!(blast.0, "type 0: the clank @20004, then DamageAndLaunch @15514");
    assert!(!blast.1, "SetIsProjectileClanking(false) at the call's end @21484");
}
