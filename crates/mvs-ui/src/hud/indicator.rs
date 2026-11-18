//! UI_ItemIndicator: BugsBunnyTunnelParent's
//! ItemUiWidget, a WidgetComponent in Space Screen under the actor's Capsule (class UI_ItemIndicator_C, DrawSize
//! 25 x 25, bDrawAtDesiredSize, Pivot (0.5, 1.5)), with the PickUp status its BeginPlay adds
//! (BugsBunnyTunnelParent @3124-3650), in the widget components' screen layer.

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;

use super::common::{Anim, BrokerPlayer, brush_path};
use crate::bundle::{Bundle, Rgba};
use crate::element::ElementList;
use crate::slate::{Geometry, Tree, Vis, WidgetId, instantiate, layout, measure_tree, paint};
use crate::text::Fonts;

const ARROW: &str = "PC-Icon-ARROW-DOWN";
const BLACK: Rgba = [0.0, 0.0, 0.0, 1.0];

/// FLinearColor::ToFColor(true) then FLinearColor(FColor): the sRGB byte round trip of an FColor parameter.
fn via_fcolor(c: Rgba) -> Rgba {
    let enc = |x: f64| {
        let v = x.clamp(0.0, 1.0);
        ((if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.999).floor()
    };
    let dec = |b: f64| {
        let v = b / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    [dec(enc(c[0])), dec(enc(c[1])), dec(enc(c[2])), (c[3].clamp(0.0, 1.0) * 255.999).floor() / 255.0]
}

/// UI_Functions ModifiedColor(colour, lerpColor, amount, modifyAlpha true, newAlpha).
fn modified(c: Rgba, to: Rgba, t: f64, alpha: f64) -> Rgba {
    [c[0] + (to[0] - c[0]) * t, c[1] + (to[1] - c[1]) * t, c[2] + (to[2] - c[2]) * t, alpha]
}

/// A UI_Status of the indicator: the widget and the indicator clock at its Shimmer / Throb calls (-1: not yet).
#[derive(Clone, Debug)]
struct Status {
    w: WidgetId,
    shimmer_at: f64,
    throb_at: f64,
}

/// The class's tree with its PickUp status, made once.
#[derive(Debug)]
struct Proto {
    t: Tree,
    status: WidgetId,
    enter: Anim,
    throb: Anim,
    shimmer: Anim,
}

/// UI_ItemIndicator. Its child UI_Statuses' OnInitialized runs first (RemoveAllStatuses: the tree's tempStatus1 / 2
/// leave the container). OnInitialized @147:
///   SetUIOptionsFromPreferences: the default preferences (mode 0's options: showIndicatorIcon, showStatuses,
///     showUsername on, showPercentageBar and splitLayout off) -> SetUIOptions @0-926: indicatorIcon
///     SelfHitTestInvisible, percentageBar Hidden, statuses SelfHitTestInvisible, middleContainer's brush
///     ModifiedColor(its colour, black, 0, true, 0.35), then UpdateLayout (percentageBar at (0, 0), middleContainer
///     at (0, -40), mainContainer at (0, 0), indicatorIcon put in bottomContainer again with top padding 24: where
///     the tree has it; bottomContainer stays Collapsed, so the chevron is never drawn);
///   @63 SetColor(ColorForIndex(0)): icon tinted, statuses.SetColor (its `color` and each child's outline),
///     middleContainer's brush ModifiedColor(colour, black, 0.85, true, 0.35);
///   @282 SetRenderOpacity(0), Delay(0.25) -> @15 PlayAnimation(EnterAnim, 0, 1 loop, forward, speed 2).
/// The tunnel's BeginPlay, after the component's (which made the widget): statuses.AddStatus(PickUp,
/// PC-Icon-ARROW-DOWN, colour (0, 0, 0, 0): the statuses' own colour, lifetime -1, debuff false), then
/// ThrobStatus(PickUp) and ItemUiWidget.SetVisibility(false).
/// UI_Statuses.AddStatus @1072-1407: a new UI_Status in `container` with left padding 50, then Init with the colour
/// replaced @182-723. ThrobStatus @494: PlayAnimation(Throb, 0, 0 loops = until stopped, forward, 1). UI_Status.Init:
/// the icon, outline and flasher brushes from the icon (bMatchSize false), SetColor (the outline, the colour as an
/// FColor through Conv_ColorToLinearColor), no timer for lifetime -1, no debuff, adjustmentcontainer at scale 1 /
/// offset 0, and a 2.0 s timer to Shimmer (ShimmerAnim, 0 loops: until stopped).
#[derive(Debug)]
struct Indicator {
    t: Tree,
    statuses: Vec<Status>,
    /// Real seconds the component was visible.
    clock: f64,
    last_real: f64,
    /// The clock at PlayAnimation(EnterAnim), -1: not yet.
    enter_at: f64,
    delay_done: bool,
    born: f64,
    /// Game time of the UI_Status timer.
    shimmer_due: f64,
    shown: bool,
    pos: Option<[i32; 2]>,
}

/// The item indicators by tunnel.
#[derive(Debug)]
pub struct ItemIndicators {
    proto: Option<Proto>,
    list: IndexMap<i32, Indicator>,
    at: IndexMap<i32, [f64; 3]>,
}

impl ItemIndicators {
    /// color: the palette's entry 0.
    pub fn new(b: &Bundle, color: Rgba) -> ItemIndicators {
        let ok = b.widgets.contains_key("UI_ItemIndicator") && b.widgets.contains_key("UI_Status");
        ItemIndicators { proto: ok.then(|| Self::proto(b, color)), list: IndexMap::new(), at: IndexMap::new() }
    }

    fn proto(b: &Bundle, color: Rgba) -> Proto {
        let mut t = instantiate(b, "UI_ItemIndicator", "ItemUiWidget");
        let statuses = t.find("statuses");
        let container = t.find_in(statuses, "container");
        // UI_Statuses OnInitialized: RemoveAllStatuses
        for c in t.widget(container).children.clone() {
            t.remove(c);
        }
        // SetUIOptions
        let icon_w = t.find("indicatorIcon");
        t.widget_mut(icon_w).vis = Vis::SelfHitTestInvisible;
        let bar = t.find("percentageBar");
        t.widget_mut(bar).vis = Vis::Hidden;
        t.widget_mut(statuses).vis = Vis::SelfHitTestInvisible;
        let mid = t.find("middleContainer");
        let c = t.widget(mid).brush_color;
        t.widget_mut(mid).brush_color = modified(c, BLACK, 0.0, 0.35);
        t.widget_mut(bar).transform.translation = [0.0, 0.0];
        t.widget_mut(mid).transform.translation = [0.0, -40.0];
        let root = t.root();
        t.widget_mut(root).transform.translation = [0.0, 0.0];
        // SetColor(ColorForIndex(0)): the FColor's linear value
        let lin = via_fcolor(color);
        let icon = t.find("icon");
        t.widget_mut(icon).color = lin;
        t.widget_mut(mid).brush_color = modified(lin, BLACK, 0.85, 0.35);
        // SetRenderOpacity(0); EnterAnim after the Delay
        t.widget_mut(root).opacity = 0.0;
        // the tunnel's BeginPlay: AddStatus(PickUp)
        let status = t.instantiate_in(b, container, "UI_Status", "PickUp");
        t.widget_mut(status).pad.l = 50.0;
        let arrow = brush_path(b, ARROW);
        for n in ["icon", "outline", "flasher"] {
            let w = t.find_in(status, n);
            if let Some(p) = &arrow {
                t.widget_mut(w).tex = Some(p.clone());
            }
        }
        let outline = t.find_in(status, "outline");
        t.widget_mut(outline).color = via_fcolor(color);
        let adj = t.find_in(status, "adjustmentcontainer");
        let a = t.widget_mut(adj);
        a.transform.scale = [1.0, 1.0];
        a.transform.translation = [0.0, 0.0];
        Proto {
            t,
            status,
            enter: Anim::of(b, "UI_ItemIndicator", "EnterAnim"),
            throb: Anim::of(b, "UI_Status", "Throb"),
            shimmer: Anim::of(b, "UI_Status", "ShimmerAnim"),
        }
    }

    /// After each World step: the tunnels there are (read off the owners' spawned actors),
    /// a widget living as long as its actor is listed. Shown: UpdateOverlappingAllies
    /// (@10278-10718) adds a locally controlled ally fighter to OverlappingAllies and shows the component; the end of
    /// an overlap takes the fighter out and shows the component while the list is not empty. So the component is
    /// visible while a locally controlled fighter of the tunnel's team stands in its Box.
    // Unverified: a bot's pawn is locally controlled offline, as the Jumps Reset text takes it.
    pub fn observe(&mut self, w: &World, players: &[BrokerPlayer], now: f64) {
        let Some(proto) = &self.proto else { return };
        let mut seen: Vec<(i32, [f64; 3], bool)> = Vec::new();
        for f in w.all_ids() {
            for &a in &w.fighter(f).items.spawned_actors {
                let ActorRef::Actor(id) = a else { continue };
                let Some(act) = w.actors.actors.get(id) else { continue };
                if act.dead {
                    continue;
                }
                let WorldActorState::Tunnel(t) = &act.state else { continue };
                let shown = t.allies.iter().any(|&g| {
                    players.get(usize::try_from(w.fighter(g).id).unwrap_or(usize::MAX)).is_some_and(|p| p.is_local)
                });
                seen.push((act.seq, [0.0, f64::from(-t.x), f64::from(t.y)], shown));
            }
        }
        self.list.retain(|id, _| seen.iter().any(|s| s.0 == *id));
        for &(id, at, shown) in &seen {
            let v = self.list.entry(id).or_insert_with(|| Indicator {
                t: proto.t.clone(),
                statuses: vec![Status { w: proto.status, shimmer_at: -1.0, throb_at: 0.0 }],
                clock: 0.0,
                last_real: -1.0,
                enter_at: -1.0,
                delay_done: false,
                born: now,
                shimmer_due: now + 2.0,
                shown: false,
                pos: None,
            });
            v.shown = shown;
            self.at.insert(id, at);
        }
        self.at.retain(|id, _| seen.iter().any(|s| s.0 == *id));
    }

    /// Each drawn frame, in the screen layer.
    ///
    /// The component's hidden frames: its slot leaves the screen layer, so the widget is neither painted nor ticked:
    /// its animations' clock (real time while shown) stands still. The Delay and the UI_Status timer count game time
    /// from the spawn, so EnterAnim and ShimmerAnim start at the clock's value then.
    // Unverified: whether a widget whose slot is off runs its latent actions and timers is not read; nor the paint
    // order against the fighters' widgets in the layer (drawn after them, as the skill shot indicators).
    pub fn draw(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        dpi: f64,
        now: f64,
        real: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        let Some(proto) = &self.proto else { return Vec::new() };
        let mut drawn = Vec::new();
        for (k, (id, v)) in self.list.iter_mut().enumerate() {
            // tick
            if v.shown && v.last_real >= 0.0 {
                v.clock += (real - v.last_real).max(0.0);
            }
            v.last_real = real;
            if !v.delay_done && now >= v.born + 0.25 {
                v.delay_done = true;
                v.enter_at = v.clock;
            }
            for s in &mut v.statuses {
                if s.shimmer_at < 0.0 && now >= v.shimmer_due {
                    s.shimmer_at = v.clock;
                }
            }
            if v.shown {
                let root = v.t.root();
                if v.enter_at >= 0.0 {
                    proto.enter.at(&mut v.t, root, (2.0 * (v.clock - v.enter_at)).min(proto.enter.end()));
                }
                for s in &v.statuses {
                    if s.throb_at >= 0.0 {
                        proto.throb.at(&mut v.t, s.w, (v.clock - s.throb_at) % proto.throb.end());
                    }
                    if s.shimmer_at >= 0.0 {
                        proto.shimmer.at(&mut v.t, s.w, (v.clock - s.shimmer_at) % proto.shimmer.end());
                    }
                }
            }
            // place (SWorldWidgetScreenLayer): the Capsule's location projected, rounded to whole pixels
            let p = if v.shown { self.at.get(id).and_then(|&a| project(a)) } else { None };
            v.pos = p.map(|p| [(p[0] + 0.5).floor() as i32, (p[1] + 0.5).floor() as i32]);
            let Some([x, y]) = v.pos else { continue };
            // the widget's top-left there minus (0.5 w, 1.5 h) of its desired size (DrawSize unused)
            measure_tree(&mut v.t, fonts, dpi);
            let r = v.t.root();
            let size = v.t.desired_size(r);
            let pos = [f64::from(x) / dpi - 0.5 * size[0], f64::from(y) / dpi - 1.5 * size[1]];
            layout(&mut v.t, fonts, Geometry { pos, size }, dpi);
            paint(&v.t, fonts, out, 1.0);
            drawn.push(k);
        }
        drawn
    }

    /// An indicator's tree by its place.
    pub fn tree(&self, k: usize) -> Option<&Tree> {
        self.list.get_index(k).map(|(_, v)| &v.t)
    }
}
