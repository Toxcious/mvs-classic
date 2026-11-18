//! The Fall blend-space player (`fighter::anim::blend_spaces`) against a tape of the oracle's
//! (fallAnim / fallRate of Batman's FallBlendSpace over a range of vertical speeds): the heaviest row's clip and the
//! rate, bit for bit. The tape is `.scratch/fall_tape.json` of the tree (written by an oracle probe, never
//! committed); without it, or without the dev store, the test says so and passes.

use mvs_data::GameData;
use mvs_sim::fighter::anim::blend_spaces::{Space, rows_rate_by, select_1d};
use mvs_sim::math::{max, min};
use std::path::PathBuf;

#[test]
fn the_fall_blend_space_matches_the_ts_tape() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(text) = std::fs::read_to_string(root.join(".scratch/fall_tape.json")) else {
        eprintln!("no .scratch/fall_tape.json: skipped");
        return;
    };
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no dev store: skipped");
        return;
    };
    let tape: Vec<(String, String, String)> = serde_json::from_str(&text).expect("the tape's json");
    let data = GameData::load(&store, &["batman"], "trainingroomsmall").expect("the gamedata kind loads");
    let cd = &data.chars["batman"];
    let set = &cd.def.anim;
    let sp = Space::from(set.fall_space.as_ref().expect("Batman's FallBlendSpace"));
    let len_of = |a: &str| {
        if let Some(l) = set.composites.get(a).and_then(|c| c.last()) {
            return mvs_sim::math::narrow(l.start + l.len);
        }
        cd.boxes.anims.get(a).map_or(1.0, |x| mvs_sim::math::narrow(x.length))
    };
    let scale = |a: &str| cd.boxes.anims.get(a).map_or(1.0, |x| mvs_sim::math::narrow(x.rate_scale));
    let hx = |v: f64| format!("{:x}", v.to_bits());
    for (vy, anim, rate) in &tape {
        let vy = f64::from_bits(u64::from_str_radix(vy, 16).expect("hex"));
        let rows = select_1d(sp, min(max(mvs_sim::math::narrow(-vy), 0.0), 10000.0));
        let head = rows.head(sp).or(set.fall.as_deref()).unwrap_or("");
        assert_eq!(head, anim, "vy {vy}: the clip");
        assert_eq!(&hx(f64::from(rows_rate_by(sp, &rows, &len_of, &scale))), rate, "vy {vy}: the rate");
    }
    eprintln!("{} speeds checked", tape.len());
}
