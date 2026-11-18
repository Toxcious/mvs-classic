//! A bot's tree reading the velocity of an actor that is neither a fighter, a projectile nor an item (`ai/util.rs`
//! `vel` velOf), which no match runs: no bot match has an enemy Harley. BotDetectEnemy's
//! ConsiderProjectile writes an enemy HarleyBomb (a world actor) into EnemyProjectile, which Base_BehaviorTree's
//! ProjectileETALessThan and BotCrouchUnderProjectile read with velOf. On a fresh World of Harley against a Shaggy
//! bot, Harley's HarleyQuinn_Bomb_ThrowU_Montage throws a bomb that is then kept flying at the bot; the bot thinks
//! every tick: about 160 ticks in, once its first special ends, the tree's search reaches ProjectileETALessThan
//! (Selector_47's EnemyProjectile branch), where the port panicked before every family had its velOf. Without a store
//! it checks nothing.

use mvs_anim::montage::PROXY_NONE;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::ai::bt::BtBot;
use mvs_sim::ai::util::vel;
use mvs_sim::fighter::anim::montages::start_montage;
use mvs_sim::input::{InputState, NO_INPUT};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::world::World;
use mvs_sim_trace::harness;
use std::sync::Arc;

const NONE: [InputState; 2] = [NO_INPUT, NO_INPUT];

fn bomb(w: &World) -> Option<ActorId> {
    w.actors.actors.iter().find(|(_, a)| matches!(a.state, WorldActorState::HarleyBomb(_)) && !a.dead).map(|(id, _)| id)
}

#[test]
fn a_bot_reads_the_velocity_of_an_enemy_bomb() {
    let player = |c: &str, team, bot| MatchPlayer {
        character: Some(c.to_owned()),
        team: Some(team),
        bot,
        difficulty: Some(3.0),
        ..MatchPlayer::default()
    };
    let setup =
        MatchSetup { players: vec![player("harley", 0, false), player("shaggy", 1, true)], ..MatchSetup::default() };
    let Some(mut h) = harness::fresh(&setup, 1) else { return };
    let trees = Arc::new(h.w.data.bot_trees.clone());
    h.w.bot_trees = Some(trees.clone());
    let mut bot = BtBot::match_bot(12345, &trees, 3);
    for _ in 0..20 {
        h.step(&NONE);
    }
    let (harley, shaggy) = (h.fighter(0), h.fighter(1));
    let m = h.w.lit("HarleyQuinn_Bomb_ThrowU_Montage");
    start_montage(&mut h.w, harley, m, &PROXY_NONE, None);
    let mut id = None;
    for _ in 0..30 {
        h.step(&NONE);
        id = bomb(&h.w);
        if id.is_some() {
            break;
        }
    }
    let id = id.expect("Harley's throw spawns a HarleyBomb");
    let a = ActorRef::Actor(id);
    let key = bot.bb.key("EnemyProjectile");
    let mut seen = 0;
    for _ in 0..300 {
        if h.w.actors.actors.get(id).is_none_or(|x| x.dead) {
            break;
        }
        // the bomb kept 500 behind the bot, flying at it level with its capsule
        let (bx, by) = {
            let b = &h.w.fighter(shaggy).body;
            (b.x, b.y)
        };
        if let WorldActorState::HarleyBomb(b) = &mut h.w.actors.actors.get_mut(id).expect("a live bomb").state {
            (b.x, b.y, b.vx, b.vy, b.pvx, b.pvy) = (bx - 500.0, by, 1500.0, 0.0, 1500.0, 0.0);
            b.grav_at = 1e9;
        }
        let input = bot.think(&mut h.w, shaggy);
        if bot.bb.obj(key) == Some(a) {
            seen += 1;
        }
        h.step(&[NO_INPUT, input]);
    }
    assert!(seen > 0, "BotDetectEnemy never wrote the bomb into EnemyProjectile");
    // velOf of the world actor: its class's vx / vy, world Y = -x
    if let Some(WorldActorState::HarleyBomb(b)) = h.w.actors.actors.get(id).map(|x| &x.state) {
        assert_eq!(vel(&h.w, Some(a)), (-b.vx, b.vy));
    }
}
