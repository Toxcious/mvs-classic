//! The hit rotation's pitch (`fighter::anim::box_pose` hit_pitch / layer_pitch) against a tape of the oracle's
//! (a Shaggy in HitStunned over branches, facings, knockbacks and DirectionSoft values), bit
//! for bit. The tape is `.scratch/pitch_tape.json` of the tree (written by an oracle probe, never committed); without
//! it, or without the dev store, the test says so and passes.

use mvs_data::GameData;
use mvs_sim::World;
use mvs_sim::fighter::anim::GroundState;
use mvs_sim::fighter::anim::box_pose::{hit_pitch, layer_pitch};
use mvs_sim::names::Name;
use mvs_sim::setup::{MatchPlayer, MatchSetup};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

fn bits(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().expect("hex"), 16).expect("hex"))
}

#[test]
fn the_hit_pitch_matches_the_ts_tape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(text) = std::fs::read_to_string(root.join(".scratch/pitch_tape.json")) else {
        eprintln!("no .scratch/pitch_tape.json: skipped");
        return;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no dev store: skipped");
        return;
    };
    let tape: Vec<Value> = serde_json::from_str(&text).expect("the tape's json");
    let data = Arc::new(GameData::load(&store, &["shaggy"], "trainingroomsmall").expect("the gamedata kind loads"));
    let player = MatchPlayer { character: Some("shaggy".into()), ..MatchPlayer::default() };
    let setup = MatchSetup { players: vec![player.clone(), player], ..MatchSetup::default() };
    let mut w = World::new(data, 7, &setup).expect("binds");
    let f = w.players[0];
    let hx = |v: Option<f64>| v.map(|v| format!("{:x}", v.to_bits()));
    for r in &tape {
        let br = r[0].as_str().unwrap();
        let br = w.names.get(br).filter(|_| !br.is_empty()).unwrap_or(Name::NONE);
        let fi = w.fighter_mut(f);
        fi.sm.gs = GroundState::HitStunned;
        fi.react.hr_branch = br;
        fi.body.facing_dir = mvs_sim::math::narrow(r[1].as_f64().unwrap());
        fi.react.hit_stun_kb_vx = mvs_sim::math::narrow(bits(&r[2]));
        fi.react.direction_soft = mvs_sim::math::narrow(bits(&r[3]));
        let want = |v: &Value| v.as_str().map(str::to_owned);
        assert_eq!(hx(hit_pitch(&w, f).map(f64::from)), want(&r[4]), "{r}: hitPitch");
        assert_eq!(hx(layer_pitch(&w, f).map(f64::from)), want(&r[5]), "{r}: layerPitch");
    }
    eprintln!("{} cases checked", tape.len());
}
