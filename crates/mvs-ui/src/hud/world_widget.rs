//! The scenery's world-space widgets painted into the pictures the renderer draws on their quads (mvs-render
//! scenery/widget.rs): each widget class instantiated
//! from the hud bundle once, laid out at its draw size (no DPI scale), its animations set from the block's state,
//! painted through the element batches into its target, and only in a frame whose picture changed.

use mvs_render::Renderer;
use mvs_render::scenery::{WidgetPaint, WorldWidget};

use super::common::{Anim, grouped};
use crate::bundle::Bundle;
use crate::element::{ElementList, Ui};
use crate::slate::{Geometry, Tree, Vis, WidgetId, layout, paint};
use crate::text::Fonts;
use crate::{Error, Result};

/// UI_SpaceJam_Scoreboard's sides by TeamIndex.
const SIDES: [&str; 2] = ["UI_SpaceJam_ScoreboardTeam_Right", "UI_SpaceJam_ScoreboardTeam_Left"];

/// UI_SpaceJam_Scoreboard's tree and animations.
#[derive(Debug)]
struct Scoreboard {
    tree: Tree,
    enter: Anim,
    /// Each side's widget and its labelScore.
    sides: [(WidgetId, WidgetId); 2],
    change: Anim,
}

impl Scoreboard {
    fn new(b: &Bundle) -> Scoreboard {
        let class = "UI_SpaceJam_Scoreboard";
        let mut tree = crate::slate::instantiate(b, class, class);
        let sides = SIDES.map(|n| {
            let w = tree.find(n);
            // PreConstruct: contentOverlay's render shear X = SelectFloat(-12, 12, Flip)
            let flip = tree.props(w).get("Flip").and_then(serde_json::Value::as_bool).unwrap_or(false);
            let overlay = tree.find_in(w, "contentOverlay");
            tree.widget_mut(overlay).transform.shear[0] = if flip { -12.0 } else { 12.0 };
            (w, tree.find_in(w, "labelScore"))
        });
        Scoreboard {
            tree,
            enter: Anim::of(b, class, "EnterAnim"),
            sides,
            change: Anim::of(b, "UI_SpaceJam_ScoreboardTeam", "ScoreChangeAnim"),
        }
    }

    /// The picture's state: the animations' times held at their last frame (UUMGSequencePlayer's end), the scores.
    fn state(&self, enter: f64, scores: [i32; 2], changed: [Option<f64>; 2]) -> Painted {
        let end = self.change.end();
        Painted::Scoreboard {
            enter: enter.min(self.enter.end()),
            scores,
            sides: changed.map(|c| c.map_or(end, |t| t.min(end))),
        }
    }

    fn paint(&mut self, fonts: &mut Fonts, size: [u32; 2], st: &Painted, out: &mut ElementList) {
        let Painted::Scoreboard { enter, scores, sides } = *st else { return };
        let t = &mut self.tree;
        let root = t.root();
        t.widget_mut(root).vis = Vis::Visible;
        self.enter.at(t, root, enter);
        for (k, &(w, label)) in self.sides.iter().enumerate() {
            // SetScore: labelScore = Conv_IntToText(score, grouping)
            t.widget_mut(label).text = grouped(i64::from(scores[k]));
            self.change.at(t, w, sides[k]);
        }
        let area = Geometry { pos: [0.0, 0.0], size: [f64::from(size[0]), f64::from(size[1])] };
        layout(t, fonts, area, 1.0);
        paint(t, fonts, out, 1.0);
    }
}

/// What a widget's picture last showed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Painted {
    Hidden,
    Scoreboard { enter: f64, scores: [i32; 2], sides: [f64; 2] },
}

/// The match's world-space widgets: made with the HUD, painted after the scenery's frame and before the renderer
/// draws.
#[derive(Debug)]
pub struct WorldWidgets {
    scoreboard: Scoreboard,
    /// Each widget's key and what its picture shows.
    painted: Vec<(&'static str, Painted)>,
    list: ElementList,
}

impl WorldWidgets {
    /// From the hud bundle.
    pub fn new(b: &Bundle) -> WorldWidgets {
        WorldWidgets { scoreboard: Scoreboard::new(b), painted: Vec::new(), list: ElementList::default() }
    }

    /// The warm-up, after the scenery's begin and before the match's first frame: each widget's target made and
    /// painted once with every glyph its numbers can show (the font atlas filled, the batches' shaders bound), then
    /// cleared, so the first show makes nothing mid-match.
    pub fn warm(&mut self, renderer: &mut Renderer, ui: &mut Ui, fonts: &mut Fonts) -> Result<()> {
        let mut k = 0;
        while let Some(w) = renderer.scenery_widget(k) {
            k += 1;
            let st = match w.class {
                // (every digit and the group separator; the animations at their ends)
                "UI_SpaceJam_Scoreboard" => self.scoreboard.state(f64::MAX, [1_234_567_890, 0], [None, None]),
                class => return Err(unknown(&w, class)),
            };
            self.draw(renderer, ui, fonts, &w, st)?;
            renderer.scenery_widget_target(w.key)?;
            self.set_painted(w.key, Painted::Hidden);
        }
        Ok(())
    }

    /// The scenery's widgets as its last frame left them painted into their targets where their picture changed.
    pub fn paint(&mut self, renderer: &mut Renderer, ui: &mut Ui, fonts: &mut Fonts) -> Result<()> {
        let mut k = 0;
        while let Some(w) = renderer.scenery_widget(k) {
            k += 1;
            let st = self.state(&w)?;
            let was = self.set_painted(w.key, st);
            // (a hidden widget's quad is not drawn: its picture stays as it was until it shows again)
            if st == was || st == Painted::Hidden {
                continue;
            }
            self.draw(renderer, ui, fonts, &w, st)?;
        }
        Ok(())
    }

    /// What the widget `key` shows from now on; what it showed before.
    fn set_painted(&mut self, key: &'static str, st: Painted) -> Painted {
        match self.painted.iter_mut().find(|p| p.0 == key) {
            Some(p) => std::mem::replace(&mut p.1, st),
            None => {
                self.painted.push((key, st));
                Painted::Hidden
            }
        }
    }

    /// The widget's target cleared and `st` painted into it.
    fn draw(
        &mut self,
        renderer: &mut Renderer,
        ui: &mut Ui,
        fonts: &mut Fonts,
        w: &WorldWidget,
        st: Painted,
    ) -> Result<()> {
        let Some(target) = renderer.scenery_widget_target(w.key)? else { return Ok(()) };
        self.list.clear();
        match st {
            Painted::Scoreboard { .. } => self.scoreboard.paint(fonts, w.size, &st, &mut self.list),
            Painted::Hidden => {}
        }
        // (painted once and kept: every picture made before the draw)
        let stream = std::mem::replace(&mut ui.stream_pictures, false);
        let drawn = ui.draw(renderer.gpu(), renderer.assets(), fonts, &self.list, &target);
        ui.stream_pictures = stream;
        drawn
    }

    fn state(&self, w: &WorldWidget) -> Result<Painted> {
        Ok(match (&w.paint, w.class) {
            (None, _) => Painted::Hidden,
            (Some(WidgetPaint::Scoreboard { enter, scores, changed }), "UI_SpaceJam_Scoreboard") => {
                self.scoreboard.state(*enter, *scores, *changed)
            }
            (Some(_), class) => return Err(unknown(w, class)),
        })
    }
}

fn unknown(w: &WorldWidget, class: &str) -> Error {
    Error::Format { what: format!("world widget {}", w.key), why: format!("{class} is not a widget this paints") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_render::gpu::{Gpu, Subresource, TexDesc, TexKind, TexViews};
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

    /// The scoreboard painted at 950 x 950 with the EnterAnim and both ScoreChangeAnims at their ends: the picture
    /// has the box and the numbers (a part of it covered, not all). `MVS_WIDGET_PNG=<file>` writes it over grey.
    /// Needs the dev store and a GPU; without them it says so and passes.
    #[test]
    fn scoreboard_paints() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
            eprintln!("no dev store: skipped");
            return;
        };
        let Ok(b) = Bundle::load(&store, "hud") else {
            eprintln!("no hud bundle: skipped");
            return;
        };
        let mut fonts = Fonts::new(&store, &b).unwrap();
        let Ok(gpu) = Gpu::new(None, false) else {
            eprintln!("no GPU: skipped");
            return;
        };
        let assets =
            mvs_render::Assets::open(mvs_store::Store::open(mvs_store::dev_store_dir()).unwrap(), &gpu).unwrap();
        let mut ui = Ui::new(&gpu, &assets).unwrap();
        ui.time = Some(0.0);
        let mut board = Scoreboard::new(&b);
        let st = board.state(10.0, [3, 1250], [None, Some(10.0)]);
        assert_eq!(st, board.state(20.0, [3, 1250], [None, None]), "the times are held at the animations' ends");
        let mut list = ElementList::default();
        board.paint(&mut fonts, [950, 950], &st, &mut list);
        let texts: Vec<String> = board.sides.iter().map(|&(_, l)| board.tree.widget(l).text.clone()).collect();
        assert_eq!(texts, ["3", "1,250"]);
        let zero = vec![0u8; 950 * 950 * 4];
        let desc = TexDesc {
            kind: TexKind::D2,
            size: [950, 950, 1],
            mips: 1,
            format: DXGI_FORMAT_R8G8B8A8_UNORM,
            views: TexViews { srv: true, rtv: true, ..Default::default() },
        };
        let target = gpu.texture(&desc, &[Subresource { data: &zero, row_pitch: 950 * 4, slice_pitch: 0 }]).unwrap();
        ui.draw(&gpu, &assets, &mut fonts, &list, &target).unwrap();
        let px = gpu.read_texture(&target).unwrap();
        let covered = px.chunks(4).filter(|p| p[3] > 0).count();
        assert!(covered > 950 * 950 / 50 && covered < 950 * 950, "covered {covered}");
        if let Ok(out) = std::env::var("MVS_WIDGET_PNG") {
            // premultiplied colour over grey
            let over: Vec<u8> = px
                .chunks(4)
                .flat_map(|p| {
                    let a = f32::from(p[3]) / 255.0;
                    let c = |x: u8| (f32::from(x) + 64.0 * (1.0 - a)).round().min(255.0) as u8;
                    [c(p[0]), c(p[1]), c(p[2]), 255]
                })
                .collect();
            mvs_render::image::write_png(std::path::Path::new(&out), 950, 950, &over).unwrap();
        }
    }
}
