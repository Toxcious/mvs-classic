//! The match chat's overlay drawn to a PNG for a look (`CHAT_SHOT=<file.png> [CHAT_SHOT_BG=<frame.png>] cargo test
//! --test match_chat_shot -- --ignored`): bubbles at two scoreboard slots (the newest with its tail, emoji in them)
//! and the open box with a log and a line typed past the box's width, over a frame of the game (`mvs --replay ..
//! --shot ..`) or grey. The slots stand where the top scoreboard's portraits are at 1920 x 1080. Needs the dev store
//! and a GPU.

use mvs_app::match_chat::{BoxKey, MatchChat, Overlay};
use mvs_protocol::messages::ChatTo;
use mvs_render::gpu::{Gpu, Subresource, TexDesc, TexKind, TexViews};
use mvs_ui::element::{ElementList, Ui};
use mvs_ui::text::Fonts;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

#[test]
#[ignore]
fn draw_the_match_chat() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("no dev store: skipped");
        return;
    };
    let out = std::env::var("CHAT_SHOT").unwrap_or_else(|_| "match_chat.png".into());
    let bundle = mvs_ui::bundle::Bundle::load(&store, "hud").unwrap();
    let mut fonts = Fonts::new(&store, &bundle).unwrap();
    let [w, h] = [1920u32, 1080];
    let peers = vec!["me".to_string(), "ally".into(), "foe".into(), "foe2".into()];
    let names = vec!["Tester".to_string(), "Velma".into(), "Shaggy".into(), "Taz".into()];
    let mut chat = MatchChat::new(0, peers, names, vec![0, 0, 1, 1], 0.0);
    chat.received("foe", ChatTo::All, "nice one \u{1F600}", true, 0.0);
    chat.received("foe", ChatTo::All, "gg \u{1F44D}\u{1F3FD}\u{1F525}", true, 900.0);
    chat.received("ally", ChatTo::Team, "go left \u{2764}\u{FE0F} \u{1F1FA}\u{1F1F8}", true, 1000.0);
    chat.open_box(false, true, false);
    chat.set_input("hi");
    chat.key(BoxKey::Enter, true, 1100.0);
    chat.open_box(false, true, false);
    chat.set_input("this is a long line typed past the width of the box \u{1F602} and on and on to its end");
    // the top scoreboard's portraits at 1920 x 1080
    let slot_rect = |slot: usize| -> Option<[f64; 4]> { Some([618.0 + 176.0 * slot as f64, 20.0, 96.0, 96.0]) };
    let o = Overlay { size: [w, h], layer: 1000, bottom: false, slot_rect: &slot_rect, caret: true };
    let mut list = ElementList::default();
    chat.draw(&o, 1200.0, &mut fonts, &mut list);
    let bg = match std::env::var("CHAT_SHOT_BG") {
        Ok(p) => {
            let (bw, bh, px) = mvs_render::image::read_png(std::path::Path::new(&p)).unwrap();
            assert_eq!([bw, bh], [w, h], "the background is 1920 x 1080");
            px
        }
        Err(_) => [0x40u8, 0x40, 0x40, 0xff].repeat((w * h) as usize),
    };
    let gpu = Gpu::new(None, false).unwrap();
    let assets = mvs_render::Assets::open(mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap(), &gpu).unwrap();
    let target = gpu
        .texture(
            &TexDesc {
                kind: TexKind::D2,
                size: [w, h, 1],
                mips: 1,
                format: DXGI_FORMAT_R8G8B8A8_UNORM,
                views: TexViews { srv: true, rtv: true, ..Default::default() },
            },
            &[Subresource { data: &bg, row_pitch: w * 4, slice_pitch: 0 }],
        )
        .unwrap();
    let mut ui = Ui::new(&gpu, &assets).unwrap();
    ui.time = Some(1.2);
    ui.draw(&gpu, &assets, &mut fonts, &list, &target).unwrap();
    let px = gpu.read_texture(&target).unwrap();
    mvs_render::image::write_png(std::path::Path::new(&out), w, h, &px).unwrap();
    eprintln!("{} elements, {out}", list.elements.len());
}
