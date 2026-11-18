//! The registry's rules: the tables build, a name entered twice is an error at build, a name no module entered is an
//! error at bind that names the asset.

use mvs_data::GameData;
use mvs_sim::registry::{CharHooks, Registry, registry};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use mvs_sim::{BindError, RegistryError, World};
use std::sync::Arc;

/// The match's data for `keys` on the Lab, from the dev store; none (the test says so and passes) without one.
fn data(keys: &[&str]) -> Option<Arc<GameData>> {
    let dir = mvs_store::dev_store_dir();
    let Ok(store) = mvs_store::Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return None;
    };
    Some(Arc::new(GameData::load(&store, keys, "trainingroomsmall").expect("the gamedata kind loads")))
}

fn setup(chars: &[&str]) -> MatchSetup {
    let players = chars.iter().map(|c| MatchPlayer { character: Some(c.to_string()), ..MatchPlayer::default() });
    MatchSetup { players: players.collect(), ..MatchSetup::default() }
}

#[test]
fn the_tables_build() {
    let r = Registry::build().expect("no name is registered twice");
    let shaggy = r.chars.get("shaggy").expect("Shaggy's entry");
    assert!(shaggy.state.is_some());
    assert!(registry().chars.contains("shaggy"));
}

#[test]
fn a_name_entered_twice_is_an_error() {
    let mut r = Registry::empty();
    r.chars.enter("shaggy", CharHooks::default()).unwrap();
    let e = r.chars.enter("shaggy", CharHooks::default()).unwrap_err();
    assert_eq!(e, RegistryError { family: "character", name: "shaggy" });
    assert_eq!(e.to_string(), "character shaggy is registered twice");
}

#[test]
fn an_unknown_name_is_an_error_at_bind() {
    let r = Registry::empty();
    let e = r.buffs.resolve("Buff_Nowhere_C", "HitDamage row 12").unwrap_err();
    assert_eq!(e.asset, "HitDamage row 12");
    assert_eq!(e.to_string(), "buff Buff_Nowhere_C is not registered (named by HitDamage row 12)");

    // A character no module entered: every character of the roster is entered, so the table's own lookup.
    let e = r.chars.resolve("nobody", "MatchSetup.players[1].character").unwrap_err();
    assert_eq!(
        e,
        BindError { family: "character", name: "nobody".into(), asset: "MatchSetup.players[1].character".into() }
    );
    // A character the data lacks.
    let Some(shaggy) = data(&["shaggy"]) else { return };
    let e = World::new(shaggy, 1, &setup(&["arya"])).unwrap_err();
    assert_eq!(e.family, "character data");
}

#[test]
fn a_known_cast_binds() {
    let Some(d) = data(&["shaggy"]) else { return };
    let w = World::new(d, 1, &setup(&["shaggy", "shaggy"])).expect("binds");
    assert_eq!(w.players.len(), 2);
    let f = w.fighter(w.players[1]);
    assert_eq!(f.id, 1);
    assert_eq!(w.names.str(f.ch), "shaggy");
    assert!(f.hooks.char.state.is_some());
}
