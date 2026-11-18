//! Paths of Wonder Woman's lasso, Iron Giant's weapon mode, wall and ScorchedEarth, and the air move desaturation that no trace match runs, on
//! fresh Worlds (without a store they check nothing):
//! - the lasso (`chars/wonder_woman/actors.rs`, the tests of its port plan's section 8): its return 300 short of the
//!   blast box's side ahead of age 17 (10), an ally and an enemy on one return both handled (13), the stick's
//!   velocity after she turns (14); the pulled ally's Buff_MoveCompTo_WithStun keeping him alive outside the blast
//!   box (SetCanSurviveBlastboxJumpOutArea, `combat/buffs.rs` survives_blast_jump_out);
//! - Iron Giant (`chars/iron_giant/{cannon_aim,plasma,wall,perks}.rs`): the cannon's aim, shot and burst, the disc
//!   aimer and its discs, the plasma shot and its hold, ScorchedEarth's thruster and fire walls, the wall platform;
//! - HandleAirMoveDesaturation and ResetJump's saturation (`fighter/match_side.rs`): Diffuse_Mult 0.7 once the air
//!   moves are used up, 1.0 at the reset.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::Real;
use mvs_sim::actors::{ProjectileClass, ProjectileId, WorldActorClass};
use mvs_sim::combat::buffs::{buff_of, spawn_buff, start_buff_move};
use mvs_sim::events::Event;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::World;
use mvs_sim_trace::harness::{self, Harness};

const THROW: &str = "WonderWoman_LassoThrowFast";
const PULL: &str = "Buff_MoveCompTo_WithStun_C";

fn player(c: &str, team: u8) -> MatchPlayer {
    MatchPlayer { character: Some(c.to_owned()), team: Some(team), bot: false, ..MatchPlayer::default() }
}

/// A fresh World of these players, 40 ticks in (the oracle test's `make`).
fn fresh(players: &[(&str, u8)]) -> Option<Harness> {
    let setup = MatchSetup { players: players.iter().map(|&(c, t)| player(c, t)).collect(), ..MatchSetup::default() };
    let mut h = harness::fresh(&setup, 1)?;
    let none = vec![NO_INPUT; players.len()];
    for _ in 0..40 {
        h.step(&none);
    }
    Some(h)
}

fn place(h: &mut Harness, f: FighterId, x: Real) {
    let b = &mut h.w.fighter_mut(f).body;
    b.x = x;
    b.prev_x = x;
}

fn facing(h: &Harness, f: FighterId) -> Real {
    h.w.fighter(f).body.facing_dir
}

fn lasso(w: &World) -> Option<ProjectileId> {
    w.actors
        .projectiles
        .iter()
        .find(|(_, p)| p.class == ProjectileClass::LassoProjectileFastV3 && !p.dead)
        .map(|(id, _)| id)
}

/// Steps with `inp` for player 0 until the lasso is gone (at most 120 ticks); `each` sees the World after each tick
/// the lasso lived through.
fn fly(h: &mut Harness, inp: InputState, mut each: impl FnMut(&mut Harness, ProjectileId)) {
    let ww = h.fighter(0);
    let m = h.w.lit(THROW);
    start_montage(&mut h.w, ww, m, &PROXY_NONE, None);
    let n = h.w.players.len();
    let mut seen = false;
    for _ in 0..120 {
        let mut ins = vec![NO_INPUT; n];
        ins[0] = inp;
        h.step(&ins);
        match lasso(&h.w) {
            Some(id) => {
                seen = true;
                each(h, id);
            }
            None if seen => return,
            None => {}
        }
    }
    assert!(seen, "the throw spawns the lasso");
}

/// Test 10: the lasso that reaches 300 short of a side of the level border (SM_LevelBorder01's bounds, 3.875 x the
/// blast box's half width: no fighter stands that far out, so the lasso is moved there at its age 3) starts its return
/// that tick, ahead of its age 17; one left in the middle of the stage, at 17.
#[test]
fn the_lasso_returns_short_of_the_blast_box_side_early() {
    for at_edge in [false, true] {
        let Some(mut h) = fresh(&[("wonderwoman", 0), ("shaggy", 1)]) else { return };
        let (ww, s) = (h.fighter(0), h.fighter(1));
        let f = facing(&h, ww);
        let x0 = h.w.fighter(ww).body.x;
        place(&mut h, s, x0 - 1500.0 * f);
        h.step(&[NO_INPUT, NO_INPUT]);
        let b = h.w.stage.blast;
        let border = (Real::from(b.right) - Real::from(b.left)) * 0.5 * 3.875;
        let side =
            if f > 0.0 { Real::from(b.right).max(border) - 300.0 } else { Real::from(b.left).min(-border) + 300.0 };
        let mut started: Option<i32> = None;
        fly(&mut h, NO_INPUT, |h, id| {
            let p = h.w.actors.projectiles.get_mut(id).expect("the lasso");
            if at_edge && p.age == 3 && !p.return_started {
                p.x = side - 20.0 * f;
                p.prev_x = p.x;
            }
            if started.is_none() && p.return_started {
                started = Some(p.age);
            }
        });
        let age = started.expect("the lasso returns");
        if at_edge {
            assert_eq!(age, 4, "StartReturn at the border's side, the tick after it got there");
        } else {
            assert_eq!(age, 17, "StartReturn at LassoTravelDuration away from the sides");
        }
    }
}

/// Test 13: in a 2v2, an ally and an enemy both on the return's path: the ally is pulled (Buff_ArmorPack,
/// Buff_MoveCompTo_WithStun) and the enemy is hit: the two gates are separate.
#[test]
fn an_ally_and_an_enemy_on_one_return_are_both_handled() {
    let Some(mut h) = fresh(&[("wonderwoman", 0), ("shaggy", 1), ("shaggy", 0), ("shaggy", 1)]) else { return };
    let (ww, e1, ally, e3) = (h.fighter(0), h.fighter(1), h.fighter(2), h.fighter(3));
    let f = facing(&h, ww);
    let x0 = h.w.fighter(ww).body.x;
    place(&mut h, ally, x0 + 800.0 * f);
    place(&mut h, e1, x0 + 760.0 * f);
    place(&mut h, e3, x0 - 1800.0 * f);
    h.step(&[NO_INPUT; 4]);
    let d0 = h.w.fighter(e1).stats.damage;
    let (pull, armor) = (h.w.lit(PULL), h.w.lit("Buff_ArmorPack_C"));
    let (mut pulled, mut armored) = (false, false);
    fly(&mut h, NO_INPUT, |h, _| {
        pulled |= buff_of(&h.w, ally, pull).is_some();
        armored |= buff_of(&h.w, ally, armor).is_some();
    });
    assert!(pulled && armored, "the ally pulled ({pulled}) and armoured ({armored})");
    let hit = h.w.fighter(e1).stats.damage - d0;
    assert!((hit - 6.0).abs() < 1e-4, "the enemy hit for 6: {hit}");
    assert_eq!(h.w.fighter(e3).stats.damage, 0.0, "the enemy behind her untouched");
}

/// Test 14: the stick held up while the lasso flies out under CanControl, and she turns round: the controlled
/// velocity takes her new facing (Normal(0, facing, 0.5) x 4250, the owner's facing read each tick).
#[test]
fn the_controlled_lasso_takes_her_new_facing() {
    let Some(mut h) = fresh(&[("wonderwoman", 0), ("shaggy", 1)]) else { return };
    let (ww, s) = (h.fighter(0), h.fighter(1));
    let f = facing(&h, ww);
    let x0 = h.w.fighter(ww).body.x;
    place(&mut h, s, x0 - 1500.0 * f);
    h.step(&[NO_INPUT, NO_INPUT]);
    let up = InputState { y: 1.0, ..NO_INPUT };
    let mut rows: Vec<(i32, bool, Real, Real)> = Vec::new();
    let mut turned = false;
    fly(&mut h, up, |h, id| {
        let p = h.w.actors.projectiles.get(id).expect("the lasso");
        let (age, ctl, vx, vy) = (p.age, p.can_control, p.vx, p.vy);
        rows.push((age, ctl, vx, vy));
        if age == 6 && !turned {
            turned = true;
            h.w.fighter_mut(ww).body.facing_dir = -f;
        }
    });
    let speed = 4250.0;
    let (ux, uy) = (2.0 / Real::sqrt(5.0), 1.0 / Real::sqrt(5.0));
    let before = rows.iter().find(|r| r.0 == 5).expect("age 5");
    let after = rows.iter().find(|r| r.0 == 8).expect("age 8");
    assert!(before.1 && after.1, "both under CanControl");
    assert!(
        (before.2 - ux * speed * f).abs() < 1.0 && (before.3 - uy * speed).abs() < 1.0,
        "up, her first facing: {before:?}"
    );
    assert!(
        (after.2 + ux * speed * f).abs() < 1.0 && (after.3 - uy * speed).abs() < 1.0,
        "up, the new facing: {after:?}"
    );
}

/// The pulled ally's buff: its move (the World's buffMoveTick pass) carries him, and its StartMove sets
/// SetCanSurviveBlastboxJumpOutArea, so he leaves the blast box under its stun (hitstun to the box) and is not rung
/// out while the buff lives.
#[test]
fn the_pull_buff_keeps_its_fighter_alive_outside_the_blast_box() {
    let Some(mut h) = fresh(&[("wonderwoman", 0), ("shaggy", 1)]) else { return };
    let (ww, s) = (h.fighter(0), h.fighter(1));
    let right = h.w.stage.blast.right;
    place(&mut h, s, right - 150.0);
    h.step(&[NO_INPUT, NO_INPUT]);
    let cls = h.w.lit(PULL);
    let b = spawn_buff(&mut h.w, s, cls, 5.0, Some(0), Some(ww), true, None).expect("the pull buff");
    let ty = mvs_sim::fighter::actor_y(&h.w, s) + 50.0;
    start_buff_move(&mut h.w, s, b, right + 300.0, ty, 0.3);
    let mut out_in_stun = false;
    for _ in 0..30 {
        h.step(&[NO_INPUT, NO_INPUT]);
        if buff_of(&h.w, s, cls).is_none() {
            break;
        }
        assert_eq!(h.w.fighter(s).life.dead, 0.0, "alive while the pull lasts");
        out_in_stun |= h.w.fighter(s).body.x > right && mvs_sim::fighter::in_hitstun(&h.w, s);
    }
    assert!(out_in_stun, "out of the box under the buff's stun");
}

fn diffuse(w: &mut World, f: FighterId) -> Vec<Real> {
    let id = w.fighter(f).id;
    let name = w.lit("Diffuse_Mult");
    let mut out = Vec::new();
    let mut all: Vec<Event> = std::mem::take(&mut w.events);
    all.append(&mut w.fighter_mut(f).events);
    for e in all {
        if let Event::MatScalar { fighter, name: n, value } = e
            && fighter == id
            && n == name
        {
            out.push(value);
        }
    }
    out
}

/// HandleAirMoveDesaturation: the air special that uses up the last air move (jumps and air evades at their max)
/// sets Diffuse_Mult 0.7 once; ResetJump (the landing's reset) sets 1.0 once; a reset with the mesh at 1.0 sends
/// nothing.
#[test]
fn the_air_moves_used_up_desaturate_until_the_reset() {
    let Some(mut h) = fresh(&[("batman", 0), ("shaggy", 1)]) else { return };
    let b = h.fighter(0);
    let w = &mut h.w;
    w.events.clear();
    w.fighter_mut(b).events.clear();
    let max_jumps = mvs_sim::fighter::moves::max_jumps(w, b);
    let max_specials = mvs_sim::fighter::air_special_max(w, b);
    let x = w.fighter_mut(b);
    x.jump.jumps = max_jumps as i32;
    x.air.air_evades = mvs_sim::consts::AIR_EVADE_MAX as i32;
    x.air.air_specials = max_specials as i32 - 1;
    mvs_sim::fighter::match_side::handle_air_move_desaturation(w, b);
    assert!(diffuse(w, b).is_empty(), "nothing before the last move");
    mvs_sim::fighter::control::increment_air_special(w, b);
    assert_eq!(diffuse(w, b), vec![0.7], "0.7 with every air move used");
    mvs_sim::fighter::match_side::handle_air_move_desaturation(w, b);
    assert!(diffuse(w, b).is_empty(), "sent once");
    mvs_sim::fighter::moves::reset_jump_counts(w, b);
    assert_eq!(diffuse(w, b), vec![1.0], "1.0 at ResetJump");
    mvs_sim::fighter::moves::reset_jump_counts(w, b);
    assert!(diffuse(w, b).is_empty(), "a reset at 1.0 sends nothing");
}

// ---------------------------------------------------------------- Iron Giant's weapon mode, wall and ScorchedEarth

/// An Iron Giant against a Shaggy `dx` ahead of him on the ground, facing right; in weapon mode (rage full) unless
/// `buff` is given, which he gets instead.
fn giant(dx: Real, buff: Option<&str>) -> Option<(Harness, FighterId, FighterId)> {
    let mut h = fresh(&[("irongiant", 0), ("shaggy", 1)])?;
    let (g, s) = (h.fighter(0), h.fighter(1));
    mvs_sim::chars::iron_giant::giant_w(&mut h.w, g).hover_rage.as_mut().expect("the hover rage").rage = 100.0;
    match buff {
        None => mvs_sim::chars::iron_giant::pawn::set_alternate_mode(&mut h.w, g, true),
        Some(b) => {
            let c = h.w.lit(b);
            spawn_buff(&mut h.w, g, c, 0.0, Some(0), Some(g), true, None).expect("the buff");
        }
    }
    h.w.fighter_mut(g).body.facing_dir = 1.0;
    let gx = h.w.fighter(g).body.x;
    place(&mut h, s, gx + dx);
    Some((h, g, s))
}

const fn pad(x: Real, y: Real, attack: bool, special: bool) -> InputState {
    InputState { x, y, attack, special, ..NO_INPUT }
}

/// The live world actors and projectiles of these classes.
fn live(h: &Harness, actor: &[WorldActorClass], proj: &[ProjectileClass]) -> usize {
    h.w.actors.actors.iter().filter(|(_, a)| !a.dead && actor.contains(&a.class)).count()
        + h.w.actors.projectiles.iter().filter(|(_, p)| !p.dead && proj.contains(&p.class)).count()
}

/// Presses `press`, then steps `n` ticks of `hold(t)`; `each` sees the World after each.
fn run(
    h: &mut Harness,
    press: InputState,
    hold: impl Fn(i32) -> InputState,
    n: i32,
    mut each: impl FnMut(&Harness, i32),
) {
    h.step(&[press, NO_INPUT]);
    for t in 0..n {
        h.step(&[hold(t), NO_INPUT]);
        each(h, t);
    }
}

/// The cannon (special + down): C017_CannonShotAim lives while Special is held, the release fires C017_CannonShot
/// (ActivateObject), whose death spawns the burst (C017_CannonExplotion, `spawn_burst`): a Shaggy 700 ahead takes the
/// shot's 5 and the burst's 5, and all three are gone 70 ticks on.
#[test]
fn the_cannon_aims_fires_at_the_release_and_bursts() {
    let Some((mut h, _, s)) = giant(700.0, None) else { return };
    let (mut aim_held, mut shot_at, mut burst_at) = (true, None, None);
    run(
        &mut h,
        pad(0.0, -1.0, false, true),
        |t| pad(0.0, 0.0, false, t < 30),
        72,
        |h, t| {
            if t < 29 {
                aim_held &= live(h, &[WorldActorClass::C017CannonShotAim], &[]) == 1;
            }
            if shot_at.is_none() && live(h, &[], &[ProjectileClass::C017CannonShot]) == 1 {
                shot_at = Some(t);
            }
            if burst_at.is_none() && live(h, &[WorldActorClass::C017CannonExplotion], &[]) == 1 {
                burst_at = Some(t);
            }
        },
    );
    assert!(aim_held, "the aim lives while Special is held");
    let (shot, burst) = (shot_at.expect("the shot"), burst_at.expect("the burst"));
    assert!(
        (30..=32).contains(&shot) && burst > shot,
        "the shot at the release ({shot}), the burst after it ({burst})"
    );
    assert_eq!(h.w.fighter(s).stats.damage, 10.0, "the shot's 5 and the burst's 5");
    let all = [WorldActorClass::C017CannonShotAim, WorldActorClass::C017CannonExplotion];
    assert_eq!(live(&h, &all, &[ProjectileClass::C017CannonShot]), 0, "all gone");
}

/// The disc (attack + forward): C017_EnergyDiscAimer and its C017_DiscShot shots; two reach a Shaggy 900 ahead (2
/// each), and the aimer goes.
#[test]
fn the_disc_aimer_fires_its_discs() {
    let Some((mut h, _, s)) = giant(900.0, None) else { return };
    let (mut aimer, mut discs) = (false, 0);
    run(
        &mut h,
        pad(1.0, 0.0, true, false),
        |t| pad(0.0, 0.0, t < 30, false),
        100,
        |h, _| {
            aimer |= live(h, &[WorldActorClass::C017EnergyDiscAimer], &[]) == 1;
            discs = discs.max(live(h, &[], &[ProjectileClass::C017DiscShot]));
        },
    );
    assert!(aimer && discs >= 2, "the aimer ({aimer}) and two discs at once ({discs})");
    assert_eq!(h.w.fighter(s).stats.damage, 4.0, "two discs of 2");
    assert_eq!(live(&h, &[WorldActorClass::C017EnergyDiscAimer], &[ProjectileClass::C017DiscShot]), 0, "all gone");
}

/// The plasma (special + forward, the stick down while it aims): C017_PlasmaAimer, then C017_Plasma_Shot; its hit (5)
/// gives the Shaggy Buff_HeldByPlasmaBall, and once attached he rides with the ball.
#[test]
fn the_plasma_shot_holds_the_fighter_it_hits() {
    let Some((mut h, _, s)) = giant(900.0, None) else { return };
    let held = h.w.lit("Buff_HeldByPlasmaBall_C");
    let (mut aimer, mut hold_at, mut rode) = (false, None, 0);
    run(
        &mut h,
        pad(1.0, 0.0, false, true),
        |t| pad(0.0, -1.0, false, t < 30),
        60,
        |h, t| {
            aimer |= live(h, &[WorldActorClass::C017PlasmaAimer], &[]) == 1;
            if hold_at.is_none() && buff_of(&h.w, s, held).is_some() {
                hold_at = Some(t);
            }
            let ball =
                h.w.actors.projectiles.iter().find(|(_, p)| !p.dead && p.class == ProjectileClass::C017PlasmaShot);
            if let (Some(t0), Some((_, p))) = (hold_at, ball)
                && t > t0 + 3
                && (mvs_sim::fighter::actor_y(&h.w, s) - p.y).abs() < 1.0
            {
                rode += 1;
            }
        },
    );
    assert!(aimer, "the aimer");
    assert!(hold_at.is_some(), "Buff_HeldByPlasmaBall on the Shaggy");
    assert_eq!(h.w.fighter(s).stats.damage, 5.0, "the shot's 5");
    assert!(rode >= 10, "he rides with the ball once attached: {rode} ticks");
}

/// ScorchedEarth: a Down Air with Buff_C017_ScorchedEarth spawns C017_DownThrusterAttack, whose ThrusterHitGround
/// spawns a FireWallHazard per foot (`thruster_hit_ground`); the thruster goes and the two walls stay.
#[test]
fn the_scorched_earth_thruster_lays_two_fire_walls() {
    let Some((mut h, g, _)) = giant(3000.0, Some("Buff_C017_ScorchedEarth_C")) else { return };
    for _ in 0..12 {
        h.step(&[InputState { jump: true, ..NO_INPUT }, NO_INPUT]);
    }
    assert!(!h.w.fighter(g).body.on_floor, "in the air");
    let fire = ProjectileClass::FireWallHazard;
    let (mut thruster, mut walls) = (false, 0);
    run(
        &mut h,
        pad(0.0, -1.0, true, false),
        |_| NO_INPUT,
        45,
        |h, _| {
            thruster |= live(h, &[WorldActorClass::C017DownThrusterAttack], &[]) == 1;
            walls = walls.max(live(h, &[], &[fire]));
        },
    );
    assert!(thruster, "the thruster");
    assert_eq!(walls, 2, "a fire wall per foot");
    assert_eq!(live(&h, &[WorldActorClass::C017DownThrusterAttack], &[fire]), 2, "the thruster gone, the walls on");
}

/// The wall platform (SpawnedPlatform_C017_Wall, C017_Wall_In's notify): spawned while he wall slides, it stands 150
/// behind him and 120 over his actor location as a platform; the slide's end (OnIsWallSliding false) takes it; a
/// spawn while not sliding is refused (BeginPlay's K2_DestroyActor).
#[test]
fn the_wall_platform_stands_while_he_slides() {
    use mvs_data::value::{Props, Value};
    let Some((mut h, g, _)) = giant(3000.0, None) else { return };
    let obj =
        |kv: &[(&str, Value)]| Value::Obj(kv.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect::<Props>());
    let t = obj(&[("X", Value::Num(-150.0)), ("Y", Value::Num(0.0)), ("Z", Value::Num(120.0))]);
    let Value::Obj(props) = obj(&[
        ("Transforms", obj(&[("Translation", t)])),
        ("AttachToActor", Value::Bool(true)),
        ("Socket", Value::Str("None".to_owned())),
    ]) else {
        unreachable!()
    };
    let props = std::sync::Arc::new(props);
    let req = |h: &Harness| mvs_sim::fighter::match_side::SpawnRequest {
        cls: h.w.lit("SpawnedPlatform_C017_Wall_C"),
        n: mvs_sim::events::NotifyRef { montage: mvs_sim::names::Name::NONE, index: 0 },
        attack_id: 0,
        foreign: mvs_sim::fighter::match_side::Foreign::None,
        copy: None,
        charge: None,
        aim: None,
        props: Some(std::sync::Arc::clone(&props)),
        instigator: None,
        origin: None,
    };
    let walls = |h: &Harness| live(h, &[WorldActorClass::SpawnedPlatformC017Wall], &[]);
    // not sliding: refused
    let r = req(&h);
    mvs_sim::world::spawning::spawn_world_actor(&mut h.w, g, &r);
    h.step(&[NO_INPUT, NO_INPUT]);
    assert_eq!(walls(&h), 0, "no wall without the slide");
    // sliding
    mvs_sim::fighter::moves::wall_slide_on(&mut h.w, g);
    let r = req(&h);
    let id = mvs_sim::world::spawning::spawn_world_actor(&mut h.w, g, &r).expect("the wall");
    // (a notify's spawn lists it in his Comp_Spawner's SpawnedActors, which his OnIsWallSliding reaches; this spawn
    // is the graph's that does not)
    h.w.fighter_mut(g).items.spawned_actors.push(mvs_sim::actors::ActorRef::Actor(id));
    let a = h.w.actors.actors.get(id).expect("the wall");
    let (gx, gy) = (h.w.fighter(g).body.x, mvs_sim::fighter::actor_y(&h.w, g));
    let mvs_sim::actors::world_actor::WorldActorState::WallPlatform(st) = &a.state else { panic!("a wall platform") };
    assert!(a.plat.is_some(), "a platform");
    let (dx, dy) = (st.x - gx, st.y - gy);
    assert!((dx + 150.0).abs() < 0.01 && (dy - 120.0).abs() < 0.01, "150 behind, 120 up: {dx} {dy}");
    mvs_sim::fighter::moves::wall_slide_off(&mut h.w, g);
    h.step(&[NO_INPUT, NO_INPUT]);
    assert_eq!(walls(&h), 0, "gone with the slide");
}
