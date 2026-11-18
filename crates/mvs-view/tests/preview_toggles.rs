//! The menu preview runs the AnimBP's sub-mesh toggles (BP_CharacterPreview has a VisibilityManager): on the dev store,
//! each character's preview stepped through its clips (no notify runs here, so only the hook's preview_step changes
//! what is drawn) shows or hides a part at least once. Without the dev store the test says so and passes.

use mvs_view::{FighterPose, PreviewAt, PreviewView};

const IDENT: [[f64; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
const DT: f64 = 1.0 / 30.0;
/// Seconds of each clip stepped through.
const SPAN: f64 = 3.0;

/// What is drawn: each mesh and the sections shown.
fn drawn(p: &FighterPose) -> Vec<(String, Vec<bool>)> {
    let mut v: Vec<_> = p.meshes.iter().map(|m| (m.mesh.to_string(), m.shown.clone())).collect();
    v.sort();
    v
}

/// The first clip whose stepping changes what the preview draws, with what was drawn before and after.
fn first_toggle(store: &mvs_store::Store, key: &str) -> Option<(String, String)> {
    let cd = mvs_data::char_data(store, key).expect("the character's data");
    let mut clips: Vec<&String> = cd.fighter.paths.clips.keys().collect();
    clips.sort();
    let mut view = PreviewView::new(store, key, "").expect("the preview");
    let mut seen = None;
    for clip in clips {
        let mut t = 0.0;
        while t < SPAN {
            let at = PreviewAt { clip, time: t, facing_left: false, montage: "Preview", blend_in: 0.0 };
            view.step(&at, DT);
            let now = drawn(&view.pose(&IDENT));
            match &seen {
                None => seen = Some(now),
                Some(s) if *s != now => {
                    let gone: Vec<_> = s.iter().filter(|m| !now.contains(m)).map(|m| m.0.clone()).collect();
                    let came: Vec<_> = now.iter().filter(|m| !s.contains(m)).map(|m| m.0.clone()).collect();
                    return Some((clip.clone(), format!("{t:.2} s: changed {gone:?} -> {came:?}")));
                }
                _ => {}
            }
            t += DT;
        }
    }
    None
}

#[test]
fn the_previews_toggle_their_parts() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no dev store: skipped");
        return;
    };
    let mut none = Vec::new();
    for key in ["rick", "batman", "marvin", "garnet", "steven", "jake", "bugs"] {
        match first_toggle(&store, key) {
            Some((clip, what)) => eprintln!("{key}: {clip} {what}"),
            None => none.push(key),
        }
    }
    assert!(none.is_empty(), "no part toggled on the preview of {none:?}");
}
