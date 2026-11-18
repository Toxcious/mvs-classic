//! Finn_Shop's world-space widgets: its four WidgetComponents, drawn by the
//! HUD before the screen layer's widgets.

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::finn::state::finn_of;

use crate::bundle::{Bundle, Rgba};
use crate::element::ElementList;
use crate::hud::input_label::InputLabels;
use crate::slate::{Geometry, Tree, WidgetId, instantiate, layout_widget, paint_widget};
use crate::text::Fonts;

const WIDGETS: [&str; 4] = ["UI_Finn_Shop_Cancel", "UI_Finn_Shop_Boots", "UI_Finn_Shop_Armor_Buns", "UI_Finn_Shop_BMO"];
const REL_X: [f64; 4] = [-150.0, -50.0, 50.0, 150.0];
const DRAW_SIZE: f64 = 500.0;
/// Finn_Shop CDO CantBuyTint.
const CANT_BUY_TINT: f64 = 0.75;
/// Finn_Shop CDO UnselectedOpacity.
const UNSELECTED_OPACITY: f64 = 0.5;
const CAN_BUY: Rgba = [1.0, 1.0, 1.0, 1.0];
const CANT_BUY: Rgba = [CANT_BUY_TINT, CANT_BUY_TINT, CANT_BUY_TINT, 1.0];

/// One shop as the last World step left it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    x: f64,
    y: f64,
    /// The root's scale (the sim's size scale): the widgets' places and scales.
    k: f64,
    selection: i32,
    affordable: [bool; 4],
}

/// One of the four widgets.
#[derive(Debug)]
struct Slot {
    t: Tree,
    root: WidgetId,
    backing: WidgetId,
}

/// FinnShopWidgets: CancelWidget, BootsWidget, ArmorBunsWidget and BmoWidget:
/// Space World (the class default; DrawSize the class default 500 x 500, pivot 0.5), under the Capsule at relative X
/// -150, -50, 50, 150 (Y within 0.001, Z 0), yaw 90, scale 0.5, bVisible false; the actor's rotation is yaw -90 (@15,
/// @109, and again on a facing change @13819), so each quad faces +X with its slots along the screen, Cancel on the
/// left. Each holds a UI_Finn_Shop_* tree: a CanvasPanel whose centred Overlay (offset -128, -128) holds a 256 x 256
/// Backing image (Finn_Shop_Cancel / Finn_Shop_Boot / Finn_Shop_Armor_Buns / Finn_Shop_BMO). Their IntroFlash and
/// LoopingRotate animations are never played (no graph plays them; LoopingRotate names a Rotator the trees lack).
/// Shown with the shop's widgets (@211-313: FinnShopFields.visible). The per-slot pass (@1196-2494, from BeginPlay and
/// UpdateSelectionMulticast): SetTintColorAndOpacity(RGB = affordable ? 1 : CantBuyTint 0.75, A = selected ? 1 :
/// UnselectedOpacity 0.5) and SetRelativeScale3D(selected ? 0.5 : 0.25).
/// Drawn before the screen layer's widgets: each widget painted through the affine map the camera's projection gives
/// its quad (its centre and two edge points).
// Deviation: the quads are drawn over the scene rather than as translucent world quads (a fighter in front of them
// does not cover them); the DrawSize 500 x 500 is the class default; the component tint is taken as a multiply of
// the widget's picture (the Backing's colour here; the build's Widget3DPassThrough material is not read).
#[derive(Debug, Default)]
pub(crate) struct FinnShops {
    slots: Vec<Slot>,
    laid: bool,
    seen: Vec<Seen>,
}

impl FinnShops {
    /// None: the bundle lacks the widgets.
    pub(crate) fn new(b: &Bundle) -> Option<FinnShops> {
        if !WIDGETS.iter().all(|n| b.widgets.contains_key(*n)) {
            return None;
        }
        let slots = WIDGETS
            .iter()
            .map(|n| {
                let t = instantiate(b, n, n);
                let (root, backing) = (t.root(), t.find("Backing"));
                Slot { t, root, backing }
            })
            .collect();
        Some(FinnShops { slots, laid: false, seen: Vec::new() })
    }

    /// After each World step: the shown shops.
    pub(crate) fn observe(&mut self, w: &World, _now: f64, _real: f64, _inputs: &mut InputLabels, _order: &mut u64) {
        self.seen.clear();
        for f in w.all_ids() {
            let Some(ActorRef::Actor(id)) = finn_of(w, f).and_then(|s| s.shop) else { continue };
            let Some(a) = w.actors.actors.get(id) else { continue };
            let WorldActorState::FinnShop(s) = &a.state else { continue };
            if a.dead || !s.visible {
                continue;
            }
            self.seen.push(Seen {
                x: f64::from(s.x),
                y: f64::from(s.y),
                k: f64::from(a.size_scale.unwrap_or(1.0)),
                selection: s.selection,
                affordable: s.affordable,
            });
        }
    }

    /// The world-space widgets, beneath the screen layer: the roots painted, by their place in `roots`.
    pub(crate) fn draw_world(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        _dpi: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        let mut drawn = Vec::new();
        if self.seen.is_empty() || self.slots.is_empty() {
            return drawn;
        }
        if !self.laid {
            self.laid = true;
            for s in &mut self.slots {
                let root = s.root;
                layout_widget(&mut s.t, fonts, root, Geometry { pos: [0.0, 0.0], size: [DRAW_SIZE; 2] }, 1.0);
            }
        }
        for s in &self.seen {
            for (i, rel) in REL_X.iter().enumerate() {
                let sel = s.selection == i as i32;
                let scale = if sel { 0.5 } else { 0.25 } * s.k;
                let h = (DRAW_SIZE / 2.0) * scale;
                // the actor at (0, -x, y) in UE; rel X along the actor's yaw -90: world -Y
                let cy = -s.x - rel * s.k;
                let cz = s.y;
                let (Some(c), Some(r), Some(b)) =
                    (project([0.0, cy, cz]), project([0.0, cy - h, cz]), project([0.0, cy, cz - h]))
                else {
                    continue;
                };
                let half = DRAW_SIZE / 2.0;
                let (ax, ay) = ((r[0] - c[0]) / half, (r[1] - c[1]) / half);
                let (bx, by) = ((b[0] - c[0]) / half, (b[1] - c[1]) / half);
                let w = &mut self.slots[i];
                let backing = w.backing;
                w.t.widget_mut(backing).color = if s.affordable[i] { CAN_BUY } else { CANT_BUY };
                let m = [ax, ay, bx, by, c[0] - half * ax - half * bx, c[1] - half * ay - half * by];
                paint_widget(&w.t, fonts, out, w.root, m, if sel { 1.0 } else { UNSELECTED_OPACITY });
                drawn.push(i);
            }
        }
        drawn
    }

    /// The screen layer's widgets: none.
    pub(crate) fn draw_screen(
        &mut self,
        _fonts: &mut Fonts,
        _out: &mut ElementList,
        _dpi: f64,
        _real: f64,
        _project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        Vec::new()
    }

    /// Each root the tape names, with its owner there.
    pub(crate) fn roots(&self) -> Vec<(&'static str, &Tree)> {
        self.slots.iter().map(|s| ("FinnShopWidgets", &s.t)).collect()
    }
}
