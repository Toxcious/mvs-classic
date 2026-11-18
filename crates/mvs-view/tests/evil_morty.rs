//! Evil Morty (C019_S02) on the podium, on the dev store: his defeat montage is the skin's own (not among Morty's
//! montages), and its SetSubmeshVisibility notify hides EyepatchMesh, a part the default skin lacks. Without the dev
//! store, or with one imported before the skin's podium montages were kept, the test says so and passes.

use mvs_view::PreviewView;

const DEFEAT: &str = "C019_S02_Emote_Defeat_Montage";

#[test]
fn the_defeat_montage_hides_the_eyepatch() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no dev store: skipped");
        return;
    };
    let view = PreviewView::new(&store, "morty", "C019_S02").expect("Morty's C019_S02 preview");
    let notifies = view.notifies(DEFEAT);
    if notifies.is_empty() {
        eprintln!("the store's C019_S02 has no {DEFEAT} of its own (an older import): skipped");
        return;
    }
    let hide = notifies
        .iter()
        .find(|n| {
            n.cls == "AnimNotify_SetSubmeshVisibility_C"
                && n.p.get("SubmeshTag").and_then(mvs_data::Value::as_str) == Some("EyepatchMesh")
        })
        .expect("the defeat montage's EyepatchMesh notify");
    assert!(hide.state, "a notify state");
    assert!(hide.p.get("Visible").and_then(mvs_data::Value::as_bool) != Some(true), "it hides the part");
    assert_eq!(view.parts_of("EyepatchMesh").len(), 1, "the eyepatch is a part of the view");
}
