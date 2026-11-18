//! The buff visuals no trace match shows on its own: Static Electricity's charged hands (electric.rs), the haste
//! (speed.rs) and Buff_PieHeal's heal (chars/arya.rs), each made from a Lab World whose buffs a test sets. Without
//! the store each test says so and passes.

use std::sync::Arc;

use mvs_data::GameData;
use mvs_sim::World;
use mvs_sim::combat::buffs::{BuffExt, spawn_buff};
use mvs_sim::combat::perks::PerkExt;
use mvs_sim::input::InputState;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_view::{FighterPose, ViewPoser};

use crate::fx::{Follow, Fx};

fn store() -> Option<mvs_store::Store> {
    match mvs_store::Store::open(mvs_store::dev_store_dir()) {
        Ok(s) => Some(s),
        Err(_) => {
            eprintln!("no dev store: skipped");
            None
        }
    }
}

/// A Lab of two fighters of `ch`, the first with `perks`.
fn lab(store: &mvs_store::Store, ch: &str, perks: &[&str]) -> World {
    let data = Arc::new(GameData::load(store, &[ch], "trainingroomsmall").expect("the gamedata kind loads"));
    let p = MatchPlayer { character: Some(ch.into()), ..MatchPlayer::default() };
    let first = MatchPlayer { perks: perks.iter().map(|s| (*s).to_owned()).collect(), ..p.clone() };
    let setup =
        MatchSetup { players: vec![first, p], rules: None, flow: true, allow_hazards: true, ..MatchSetup::default() };
    World::new(data, 1, &setup).expect("binds")
}

struct Run {
    w: World,
    poser: ViewPoser,
    fx: Fx,
    fighters: Vec<FighterPose>,
    actors: Vec<mvs_view::ActorPose>,
}

impl Run {
    fn new(store: &mvs_store::Store, w: World) -> Run {
        let poser = ViewPoser::new(store, &w).expect("the views");
        let fx = Fx::new(store, 1).expect("the player");
        Run { w, poser, fx, fighters: Vec::new(), actors: Vec::new() }
    }

    fn step(&mut self, n: usize) {
        for _ in 0..n {
            self.w.step(&[InputState::default(), InputState::default()]);
            self.present();
        }
    }

    /// The effects' step of the World as it is (a test's write included).
    fn present(&mut self) {
        self.poser.place(&self.w);
        self.poser.present(&self.w, 1.0, &mut self.fighters, &mut self.actors);
        self.fx.step(&self.w, &self.fighters, &[]).expect("the step");
        self.w.events.clear();
    }

    /// The live followed systems of template `ps`, with their follows.
    fn live(&self, ps: &str) -> Vec<&Follow> {
        let p = &self.fx.player;
        self.fx
            .followers
            .iter()
            .filter(|(h, _)| p.system(*h).is_some_and(|s| &*s.t.name == ps && !s.killed))
            .map(|(_, f)| f)
            .collect()
    }
}

#[test]
fn static_electricity_charged_hands_come_and_go() {
    let Some(store) = store() else { return };
    let mut r = Run::new(&store, lab(&store, "finn", &["Perk_General_ElectricProjectile"]));
    r.step(30);
    let f = r.w.all_ids()[0];
    let set = |w: &mut World, charged: bool| {
        let b =
            w.fighter_mut(f).buffs.buffs.iter_mut().find(|b| matches!(b.ext, BuffExt::Perk(PerkExt::Grounded { .. })));
        b.expect("the perk's buff").ext = BuffExt::Perk(PerkExt::Grounded { left: 0.0, charged });
    };
    assert!(r.live("PS_CreatureCrystal_Buff").is_empty());
    set(&mut r.w, true);
    r.present();
    let hands = r.live("PS_CreatureCrystal_Buff");
    assert_eq!(hands.len(), 2, "both hands charged");
    assert!(hands.iter().all(|f| matches!(f, Follow::Socket { .. })));
    set(&mut r.w, false);
    r.present();
    assert!(r.live("PS_CreatureCrystal_Buff").is_empty(), "the charge given away removes them");
}

#[test]
fn haste_shows_over_the_default_speed() {
    let Some(store) = store() else { return };
    let mut r = Run::new(&store, lab(&store, "finn", &[]));
    r.step(30);
    let f = r.w.all_ids()[0];
    assert!(r.live("PS_Buff_Haste_Speed").is_empty());
    let cls = r.w.names.get("Buff_SpeedUp_C").expect("Finn's data has Buff_SpeedUp");
    spawn_buff(&mut r.w, f, cls, 2.0, Some(0), Some(f), true, None);
    assert!(r.w.fighter(f).buffs.speed_mod > 1.0);
    r.present();
    let on = r.live("PS_Buff_Haste_Speed");
    assert_eq!(on.len(), 1, "the haste of Buff_SpeedUp");
    let a = r.fx.place(&r.w, on[0]).expect("a place");
    let k = f64::from(r.w.fighter(f).body.size_scale) * 1.4;
    assert!(a.scale.is_some_and(|s| s.iter().all(|v| (v - k).abs() < 1e-6)), "1.4 x the actor scale");
    // the buff's end brings the speed back to the default: destroyed
    r.step(200);
    assert!((r.w.fighter(f).buffs.speed_mod - 1.0).abs() < 1e-6);
    assert!(r.live("PS_Buff_Haste_Speed").is_empty(), "gone at the default speed");
}

#[test]
fn a_pie_heal_spawns_its_heal_once() {
    let Some(store) = store() else { return };
    let mut r = Run::new(&store, lab(&store, "arya", &[]));
    r.step(30);
    let f = r.w.all_ids()[1];
    let before = r.live("PS_Heal").len();
    let cls = r.w.names.get("Buff_PieHeal_C").expect("Arya's data has Buff_PieHeal");
    let by = r.w.all_ids()[0];
    spawn_buff(&mut r.w, f, cls, 0.0, Some(0), Some(by), true, None);
    r.present();
    let heals = r.live("PS_Heal");
    assert_eq!(heals.len(), before + 1, "one PS_Heal at the buff's first sight");
    let heal = heals[heals.len() - 1].clone();
    let a = r.fx.place(&r.w, &heal).expect("a place");
    assert!(a.scale.is_some_and(|s| s.iter().all(|v| (v - 1.3).abs() < 1e-6)), "world scale 1.3 (SnapToTarget)");
    // the relative scale is fixed at the spawn: a later actor scale scales the component with it
    let fm = r.w.fighter_mut(f);
    fm.body.actor_scale = None;
    let s0 = fm.body.size_scale;
    fm.body.size_scale = s0 * 0.5;
    let a = r.fx.place(&r.w, &heal).expect("a place");
    assert!(a.scale.is_some_and(|s| s.iter().all(|v| (v - 0.65).abs() < 1e-6)), "world scale 1.3 x 0.5 after");
    r.w.fighter_mut(f).body.size_scale = s0;
    r.present();
    assert!(r.live("PS_Heal").len() <= before + 1, "not again while the buff lasts");
}

#[test]
fn a_freeze_takes_the_actor_scale() {
    let Some(store) = store() else { return };
    let mut r = Run::new(&store, lab(&store, "superman", &[]));
    r.step(30);
    let f = r.w.all_ids()[1];
    let id = r.w.fighter(f).id;
    r.w.fighter_mut(f).body.size_scale = 0.6;
    r.fx.ice_vfx(&r.w, id, "sfx_cha_superman_icebreath_freeze");
    let (hit, cube) = r.fx.pawns[&id].ice;
    let at = |h: Option<mvs_vfx::SystemId>| {
        let h = h.expect("a kept emitter");
        let (_, f) = r.fx.followers.iter().find(|(x, _)| *x == h).expect("followed");
        r.fx.place(&r.w, f).expect("a place")
    };
    let (a, b) = (at(hit), at(cube));
    let fm = r.w.fighter(f);
    let origin = [0.0, -f64::from(fm.body.x), f64::from(mvs_sim::fighter::actor_y(&r.w, f))];
    let dist =
        |p: [f64; 3]| ((p[0] - origin[0]).powi(2) + (p[1] - origin[1]).powi(2) + (p[2] - origin[2]).powi(2)).sqrt();
    for x in [&a, &b] {
        assert!(x.scale.is_some_and(|s| s.iter().all(|v| (v - 0.6).abs() < 1e-6)), "the actor's scale");
    }
    assert!((dist(a.loc) - 50.0 * 0.6).abs() < 1e-3, "P_Impact_Ice's (0, -50, 0) under the scale");
    let half = mvs_sim::fighter::body::def(&r.w, f).inner_cap_half;
    assert!((dist(b.loc) - half * 0.6).abs() < 1e-3, "the cube's raw half height under the scale");
}
