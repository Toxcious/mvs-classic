//! The perk tables' lookups (`combat::perks`) on the dev store, against the oracle's answers for the trace match
//! m5-combat's perks (two Shaggys, each with two perks): the PerkData entries, their buff classes and SpawnOnAllies,
//! and the classes' defaults found in the general table or a character's part.

use mvs_data::GameData;
use mvs_sim::World;
use mvs_sim::combat::perks::{match_perk_info, perk_buff_classes, perk_buff_def, perk_info, perk_part_actor};
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use std::sync::Arc;

/// Shaggy against Shaggy on the Lab; none (the test says so and passes) without a dev store.
fn world() -> Option<World> {
    let dir = mvs_store::dev_store_dir();
    let Ok(store) = mvs_store::Store::open(&dir) else {
        eprintln!("no store at {}: skipped", dir.display());
        return None;
    };
    let data = Arc::new(GameData::load(&store, &["shaggy"], "trainingroomsmall").expect("the gamedata kind loads"));
    let player = MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player.clone(), player], ..MatchSetup::default() };
    Some(World::new(data, 21, &setup).expect("binds"))
}

#[test]
fn m5_combat_perks() {
    let Some(w) = world() else { return };
    let peers = w.players.clone();
    // (player, perk, its one buff class, SpawnOnAllies, the class defaults' key count): the oracle's
    // matchPerkInfo / perkBuffDef for the same set-up.
    let want = [
        (0, "Perk_Shaggy_DyingRage", "Buff_Shaggy_DyingRage_C", false, 36),
        (0, "Perk_General_ArmorBoost_Medium", "Buff_Perk_ArmorBoost_Medium_C", false, 35),
        (1, "Perk_Shaggy_ExtraHungry", "Buff_Shaggy_ExtraHungry_C", false, 35),
        (1, "Perk_General_EvadeSpeed_Small", "Buff_Perk_EvadeSpeed_Small_C", true, 36),
    ];
    for (i, id, cls, allies, keys) in want {
        let f = peers[i];
        let p = match_perk_info(&w, f, &peers, id).unwrap_or_else(|| panic!("{id}: found"));
        assert_eq!(p.buffs, [cls], "{id}");
        assert_eq!(p.spawn_on_allies, allies, "{id}");
        assert!(perk_info(&w, id).is_some(), "{id}");
        let d = perk_buff_def(&w, f, Some(&peers), cls).unwrap_or_else(|| panic!("{cls}: a def"));
        assert_eq!(d.len(), keys, "{cls}");
        assert!(perk_buff_classes(&w, f, Some(&peers)).contains(&cls), "{cls}");
    }
    assert!(perk_part_actor(&w, peers[0], Some(&peers), "FireWallHazard_C").is_none());
}
