//! UI_Functions PopText -> UI_PopText: a viewport widget placed by PositionForPopParams, shown
//! after its delay with EnterAnim, exited after its lifetime with ExitAnim and destroyed 0.5 s later.

use crate::bundle::Rgba;
use crate::element::ElementList;
use crate::slate::{Geometry, Tree, Vis, WidgetId, layout, paint};
use crate::text::Fonts;

use super::Res;
use super::common::c4;
use super::input_label::next;

/// Where a pop text stands (UI_PopPositioningStruct's space and attach actor).
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// Space 3: projected from the attach fighter's actor location + location, every frame.
    Attached,
    /// Space 3 with an attach actor that is no fighter (a level actor: SceneryHost.pop's `at`; a minion; AryaKillPie):
    /// its location (UE), projected with location every frame. Once the actor is gone the widget keeps its last
    /// position (`PopTexts::hold`: Screen).
    Actor([f64; 3]),
    /// Space 0: the world point, placed once (no attach actor).
    World(Option<[f64; 3]>),
    /// Space 2: location.xy x the viewport size.
    Viewport,
    /// A feeder that positions the widget itself (UI_IGv3_State PositionPopText), in pixels.
    Screen,
}

/// UI_PopTextParams as the feeders fill them.
#[derive(Clone, Debug, PartialEq)]
pub struct PopParams {
    pub text: String,
    pub lifetime: f64,
    pub text_color: Rgba,
    /// UE offset from the attach actor; a fraction of the viewport in the viewport space.
    pub location: [f64; 3],
    pub scale: f64,
    /// 0 left, 1 centre, 2 right.
    pub justification: u8,
    pub delay: f64,
    /// A material.
    pub icon_right: Option<String>,
    /// A texture.
    pub icon_left: Option<String>,
    /// A material (a character's select portrait).
    pub icon_top: Option<String>,
    /// The portrait's colour bar.
    pub team_color: Option<Rgba>,
    /// Posted by Show.
    pub audio: Option<String>,
    /// The attach actor.
    pub fighter: usize,
    pub place: Place,
}

impl PopParams {
    /// The struct most feeders fill: space 3 on `fighter`, scale 1, centred, no delay, icons or audio.
    pub fn attached(fighter: usize, text: &str, lifetime: f64, text_color: Rgba, location: [f64; 3]) -> PopParams {
        PopParams {
            text: text.to_string(),
            lifetime,
            text_color,
            location,
            scale: 1.0,
            justification: 1,
            delay: 0.0,
            icon_right: None,
            icon_left: None,
            icon_top: None,
            team_color: None,
            audio: None,
            fighter,
            place: Place::Attached,
        }
    }
}

/// A pop text's handle.
pub type PopId = u64;

/// UI_PopText.
#[derive(Debug)]
pub struct PopText {
    pub t: Tree,
    pub id: PopId,
    main: WidgetId,
    /// ShowTimer, ExitTimer (game time).
    show_at: f64,
    exit_at: f64,
    /// The HUD frame Exit ran on, while its Delay(0) is pending.
    exit_frame: Option<u64>,
    /// Delay(0) (next frame), then the 0.5 s DestroyTimer.
    destroy_at: f64,
    /// The playing animation (ExitAnim when true), its real start and last evaluated time.
    anim: Option<(bool, f64, f64)>,
    pos: Option<[f64; 2]>,
    /// AddToViewport(0)'s place.
    pub order: u64,
    pub p: PopParams,
}

impl PopText {
    /// Init(params): UI_Functions PopText runs it on a new widget; TrainingGameState again on one still in the
    /// viewport. The text, colour, font and icons are set again (the font size is read back from the Label, so a
    /// scale other than 1 compounds); both timer sequences begin with StopAllAnimations and clear the timers
    /// (@2799-2976, @3834-4028).
    fn init(&mut self, p: PopParams, now: f64) {
        let t = &mut self.t;
        self.anim = None;
        self.destroy_at = f64::INFINITY;
        let label = t.find("Label");
        let flasher = t.find("flasher");
        t.widget_mut(label).text = p.text.clone();
        t.widget_mut(flasher).text = p.text.clone();
        // MainContainer alignment by justification: 0 -> (0, 0.5), 1 -> (0.5, 0.5), 2 -> (1, 0.5).
        t.widget_mut(self.main).align = [[0.0, 0.5, 1.0][usize::from(p.justification.min(2))], 0.5];
        // ModifiedColor(text_color, white, 0, false, 1) = text_color. Outline FTrunc(scale x (icon_right ? 6 : 3));
        // size Round(32 x scale), letter spacing 0; the flasher takes the Label's font.
        let l = t.widget_mut(label);
        l.color = p.text_color;
        l.outline = (p.scale * if p.icon_right.is_some() { 6.0 } else { 3.0 }).trunc();
        l.font_size = js_round(l.font_size * p.scale);
        l.letter_spacing = 0.0;
        let (face, size, outline, oc) = (l.typeface.clone(), l.font_size, l.outline, l.outline_color);
        let fl = t.widget_mut(flasher);
        fl.typeface = face;
        fl.font_size = size;
        fl.outline = outline;
        fl.outline_color = oc;
        fl.letter_spacing = 0.0;
        // icon_left (@1881-2418): IconContainerLeft HitTestInvisible when valid, else Collapsed; its brush the
        // texture, tinted the text colour, 32 x scale.
        let il = t.find("IconContainerLeft");
        t.widget_mut(il).vis = if p.icon_left.is_some() { Vis::HitTestInvisible } else { Vis::Collapsed };
        if let Some(icon) = &p.icon_left {
            let left = t.find("icon_left");
            let w = t.widget_mut(left);
            w.tex = Some(icon.clone());
            w.color = p.text_color;
            w.brush_w = 32.0 * p.scale;
            w.brush_h = 32.0 * p.scale;
        }
        // icon_top: HitTestInvisible with the portrait material and the colour bar in team_color, else Collapsed.
        let it = t.find("IconContainerTop");
        t.widget_mut(it).vis = if p.icon_top.is_some() { Vis::HitTestInvisible } else { Vis::Collapsed };
        if let Some(icon) = &p.icon_top {
            let pw = t.find("Portrait");
            t.widget_mut(pw).tex = Some(icon.clone());
            let bar = t.find("colorBar");
            t.widget_mut(bar).color = p.team_color.unwrap_or([0.0; 4]);
        }
        // SetVisibility(Hidden), ShowTimer at delay + 0.01.
        let r = t.root();
        t.widget_mut(r).vis = Vis::Hidden;
        self.show_at = now + p.delay + 0.01;
        let right = t.find("IconContainerRight");
        let icon_flasher = t.find("IconFlasher");
        let icon = t.find("icon_right");
        let v = if p.icon_right.is_some() { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        t.widget_mut(right).vis = v;
        t.widget_mut(icon_flasher).vis = v;
        if let Some(m) = &p.icon_right {
            for w in [icon_flasher, icon] {
                let w = t.widget_mut(w);
                w.tex = Some(m.clone());
                w.brush_w = 140.0 * p.scale;
                w.brush_h = 140.0 * p.scale;
            }
            let mid = t.dynamic_material(icon);
            mid.set_vector("FillColor", c4(p.text_color));
            mid.set_vector("BackgroundColor", [1.0, 1.0, 1.0, 0.6]);
            // EnterAnim's widget material track holds IconFlasher's FillColor at white.
            t.dynamic_material(icon_flasher).set_vector("FillColor", [1.0; 4]);
        }
        // ExitTimer at lifetime + delay.
        self.exit_at = now + p.lifetime + p.delay;
        self.p = p;
    }

    /// Timers on game time, widget animations on real time. An animation stopped by StopAllAnimations keeps its
    /// evaluated state (KeepState).
    fn tick(&mut self, a: &Res, now: f64, real: f64, frame: u64, sounds: &mut Vec<String>) {
        if let Some(f) = self.exit_frame
            && frame > f
        {
            self.destroy_at = now + 0.5;
            self.exit_frame = None;
        }
        if now >= self.show_at {
            self.show_at = f64::INFINITY;
            // Show: SelfHitTestInvisible, BPI_PostAkEvent(audio) when valid, PlayAnimation(EnterAnim).
            let r = self.t.root();
            self.t.widget_mut(r).vis = Vis::SelfHitTestInvisible;
            if let Some(s) = &self.p.audio {
                sounds.push(s.clone());
            }
            self.anim = Some((false, real, 0.0));
        }
        if now >= self.exit_at {
            self.exit_at = f64::INFINITY;
            // Exit: StopAllAnimations, PlayAnimation(ExitAnim), Delay(0) -> 0.5 s DestroyTimer.
            self.anim = Some((true, real, 0.0));
            self.exit_frame = Some(frame);
        }
        if let Some((exit, start, last)) = self.anim {
            let anim = if exit { &a.pop_exit } else { &a.pop_enter };
            let sec = (real - start).min(anim.end());
            let r = self.t.root();
            anim.apply(&mut self.t, r, sec, last);
            self.anim = Some((exit, start, sec));
        }
    }

    /// SetPositionInViewport(position, bRemoveDPIScale): the zero-size root canvas sits at the point; MainContainer
    /// (auto size) aligns on it.
    /// -> whether its root was painted.
    pub fn draw(&mut self, fonts: &mut Fonts, out: &mut ElementList, dpi: f64) -> bool {
        let Some(p) = self.pos else { return false };
        layout(&mut self.t, fonts, Geometry { pos: [p[0] / dpi, p[1] / dpi], size: [0.0; 2] }, dpi);
        paint(&self.t, fonts, out, 1.0);
        self.t.widget(self.t.root()).shown()
    }
}

/// Rounds half up: floor(x + 0.5).
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// The viewport's pop texts.
#[derive(Debug)]
pub struct PopTexts {
    proto: Tree,
    main: WidgetId,
    pops: Vec<PopText>,
    frame: u64,
    seq: PopId,
}

impl PopTexts {
    pub fn new(proto: Tree) -> PopTexts {
        let main = proto.find("MainContainer");
        PopTexts { proto, main, pops: Vec::new(), frame: 0, seq: 0 }
    }

    pub fn add(&mut self, p: PopParams, now: f64, order: &mut u64) -> PopId {
        self.seq += 1;
        let mut t = PopText {
            id: self.seq,
            t: self.proto.clone(),
            main: self.main,
            show_at: f64::INFINITY,
            exit_at: f64::INFINITY,
            exit_frame: None,
            destroy_at: f64::INFINITY,
            anim: None,
            pos: None,
            order: next(order),
            p: p.clone(),
        };
        t.init(p, now);
        self.pops.push(t);
        self.seq
    }

    /// UserWidget Destroy (RemoveFromParent): gone at once.
    pub fn destroy(&mut self, id: PopId) {
        self.pops.retain(|t| t.id != id);
    }

    /// Init again on a widget still in the viewport (TrainingGameState's reuse): the text, colour, font and icons set
    /// again (the font size is read back from the Label, so a scale other than 1 compounds), AddToViewport(0)
    /// changes nothing for a widget already there, and the timers start over (StopAllAnimations, the Show / Exit /
    /// Destroy timers cleared, @2799-2976, @3834-4028). Exit's Delay(0) is a latent action, not a timer: one still
    /// pending sets the 0.5 s DestroyTimer on the next frame all the same.
    pub fn init_again(&mut self, id: PopId, p: PopParams, now: f64) {
        if let Some(t) = self.pops.iter_mut().find(|t| t.id == id) {
            t.init(p, now);
        }
    }

    /// UI_Functions Pop Text Available For Re Use (@0-209): valid, in the viewport and not Destroyed.
    pub fn available(&self, id: PopId) -> bool {
        self.pops.iter().any(|t| t.id == id)
    }

    /// A Screen pop text's position, pixels.
    pub fn place(&mut self, id: PopId, at: [f64; 2]) {
        if let Some(t) = self.pops.iter_mut().find(|t| t.id == id) {
            t.pos = Some(at);
        }
    }

    /// An Actor-placed pop text's attach actor moved to `at` (UE): a minion's text follows the minion.
    pub fn set_actor(&mut self, id: PopId, at: [f64; 3]) {
        if let Some(t) = self.pops.iter_mut().find(|t| t.id == id)
            && let Place::Actor(p) = &mut t.p.place
        {
            *p = at;
        }
    }

    /// An Actor-placed pop text's attach actor is gone: the widget keeps its last position on the screen.
    pub fn hold(&mut self, id: PopId) {
        if let Some(t) = self.pops.iter_mut().find(|t| t.id == id)
            && matches!(t.p.place, Place::Actor(_))
        {
            t.p.place = Place::Screen;
        }
    }

    /// The HUD pawn `fighter` is gone and its place may be taken by another: every widget attached to it keeps its
    /// last position, as one whose attach actor is gone does.
    pub fn release(&mut self, fighter: usize) {
        for t in &mut self.pops {
            if t.p.fighter == fighter {
                t.p.fighter = usize::MAX;
                if t.p.place == Place::Attached {
                    t.p.place = Place::Screen;
                }
            }
        }
    }

    /// anchors: each fighter's actor location (UE; None while it has none). size: the viewport, pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        a: &Res,
        now: f64,
        real: f64,
        anchors: &[Option<[f64; 3]>],
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
        size: [f64; 2],
        sounds: &mut Vec<String>,
    ) {
        self.frame += 1;
        let frame = self.frame;
        self.pops.retain_mut(|t| {
            if now >= t.destroy_at {
                return false;
            }
            t.tick(a, now, real, frame, sounds);
            // Without a valid attach actor the widget keeps its last position.
            let l = t.p.location;
            match &mut t.p.place {
                Place::Screen => {}
                Place::Viewport => t.pos = Some([l[0] * size[0], l[1] * size[1]]),
                Place::World(at) => {
                    if let Some(q) = at.take() {
                        t.pos = project([q[0] + l[0], q[1] + l[1], q[2] + l[2]]);
                    }
                }
                Place::Actor(q) => t.pos = project([q[0] + l[0], q[1] + l[1], q[2] + l[2]]),
                Place::Attached => {
                    if let Some(q) = anchors.get(t.p.fighter).copied().flatten() {
                        t.pos = project([q[0] + l[0], q[1] + l[1], q[2] + l[2]]);
                    }
                }
            }
            true
        });
    }

    pub fn pops_mut(&mut self) -> impl Iterator<Item = &mut PopText> {
        self.pops.iter_mut()
    }

    pub fn pops(&self) -> &[PopText] {
        &self.pops
    }
}
