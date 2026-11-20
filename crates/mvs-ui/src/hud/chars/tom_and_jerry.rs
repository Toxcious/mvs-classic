//! Tom and Jerry's HUD parts: Jerry's cork count and Tom's Jerry pointer.

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::chars::tom_and_jerry::jerry::jerry_of;
use mvs_sim::chars::tom_and_jerry::state::tj_of;
use mvs_sim::fighter::{actor_y, facing};

use crate::bundle::{Bundle, Rgba};
use crate::element::ElementList;
use crate::hud::attached::draw_box;
use crate::hud::common::{Anim, BrokerPlayer, PlayerMap};
use crate::hud::input_label::InputLabels;
use crate::slate::{Geometry, Tree, WidgetId, instantiate, layout_widget, paint_widget};
use crate::text::Fonts;

const CORK_WIDGET: &str = "UI_JerryCorkCount";
/// UWidgetComponent's DrawSize default (the component sets none).
const DRAW_SIZE: f64 = 500.0;
/// Widget_GEN_VARIABLE RelativeLocation.
const REL: [f64; 3] = [70.48552, 0.0, -148.9568];
/// RotateMulticast @16751: (0, -150, 0) facing left, (0, 150, 0) else.
const FOLLOW_Y: f64 = 150.0;
const POINTER_TEX: &str = "JerryPointer";
/// JerryPointer RelativeLocation X.
const POINTER_REL: f64 = 185.0;
/// The Plane's half size x RelativeScale3D.
const POINTER_HALF: f64 = 50.0 * 1.25;

/// UI_JerryCorkCount: Jerry_SpawnedActor's Widget, a WidgetComponent in Space Screen under the actor's Capsule at
/// relative (70.48552, 0, -148.9568), bVisible false in the template, the class defaults' DrawSize (500 x 500) and
/// pivot (0.5, 0.5); its tree is a centred SizeBox 80 x 80 holding an Overlay (render scale 0.75) with the Backing
/// image (UIAbility_Icon_Champagne) and the Number text. Construct does nothing (@10 -> return). BeginPlay
/// @13402-13806: OnColor(the owner's broker data: SelectColor(FocusColor, TeamColor, IsMe) into Backing's tint
/// @166-286), SetCount(CorkAmmo). SetCount @67: Number = the count, then PlayAnimation(Update, 0, 1 loop, forward,
/// speed 1) @15 (Overlay_0's scale 0.75 -> 0.47 -> 1.32 -> 0.7 -> 0.75 over 0.5 s). SetCount's callers:
/// CorkAmmoTracker @30862 and CorkAmmoSetrMulticast @31242. RotateMulticast @16718-16814: while IsFollowing the
/// Widget's relative location becomes (0, FacingLeft ? -150 : 150, 0), and stays there after the follow ends (nothing
/// sets it back).
/// The screen layer lays the widget out at the component's projected location, rounded to a whole pixel (as
/// hud/skill_shot.rs).
// Deviation: the count's change is seen after the World step (a SetCount with an unchanged count,
// CorkAmmoSetrMulticast's with the count it had, plays no Update here); BeginPlay's SetCount plays its Update while
// the Widget is hidden, so a new Jerry's count is set without one. The widget is painted after the skill shot
// widgets, not by its view distance among the screen layer's slots.
// Unverified: the follow side is taken from the actor's facing on each step it follows (the build sets it at each
// Rotate while following).
#[derive(Debug)]
struct CorkCount {
    t: Tree,
    number: WidgetId,
    update: Anim,
    /// The real time of the last SetCount's PlayAnimation, -1: none.
    started: f64,
    last: f64,
    /// The Jerry actor it belongs to.
    actor: i32,
    count: i32,
    /// The follow side's relative Y, 0 while it has not followed.
    side: f64,
    on: bool,
    /// UE.
    at: [f64; 3],
}

impl CorkCount {
    fn set_count(&mut self, n: i32, real: f64, play: bool) {
        self.count = n;
        let w = self.number;
        self.t.widget_mut(w).text = n.to_string();
        if play {
            self.started = real;
            self.last = 0.0;
        }
    }

    fn draw(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        dpi: f64,
        real: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> bool {
        if self.started >= 0.0 {
            let sec = (real - self.started).min(self.update.end());
            let root = self.t.root();
            self.update.apply(&mut self.t, root, sec, self.last);
            self.last = sec;
            if sec >= self.update.end() {
                self.started = -1.0;
            }
        }
        let Some(p) = project(self.at) else { return false };
        let (x, y) = ((p[0] + 0.5).floor() as i32, (p[1] + 0.5).floor() as i32);
        draw_box(&mut self.t, fonts, out, x, y, [DRAW_SIZE; 2], dpi)
    }
}

/// One Tom's pointer as the last World step left it.
#[derive(Debug)]
struct Pointer {
    on: bool,
    /// The actor (UE Y = -x, Z = y).
    x: f64,
    y: f64,
    /// The arrow's look direction relative to the actor: UE Y x facing, and Z (unit); 0, 0: never turned.
    rel: f64,
    z: f64,
    /// The actor's.
    facing: f64,
    /// The picture as a one-widget tree: the pointer's quad.
    t: Tree,
    image: WidgetId,
}

/// Jerry's cork count and Tom's Jerry pointer.
///
/// JerryPointer (Pawn_TomAndJerry): a StaticMeshComponent (Engine BasicShapes Plane, 100 x 100 in its XY, UV
/// (x + 50, y + 50) / 100) with JerryPointer_MAT (unlit translucent, two sided, emissive = the JerryPointer texture),
/// under the ArrowComponent JerryPointerArrow (child of the root capsule, no relative transform) at relative
/// (185, 0, 0), rotation (pitch -90, yaw 0, roll 90), scale 1.25, bVisible false in the template. ReceiveTick
/// @480-665: while SpawnedJerryAcor is valid the arrow's world rotation = FindLookAtRotation(the arrow, Jerry); the
/// visibility is the pawn's pointerTick's. With that rotation the plane's normal is the arrow's Y (the view axis for a
/// Jerry left or right of Tom: edge-on for one straight above or below, yaw 0), its V runs from Jerry back to Tom (the
/// picture's top toward Jerry) and its U along the arrow's -Z. The arrow keeps its last rotation relative to the actor
/// while Jerry is not valid (his poof: the pointer stays shown, frozen, until the cooldown ends).
// Deviation: drawn in the HUD overlay over the scene, not as a translucent world quad (a fighter in front does not
// cover it; no tonemapping or fog), as chars/finn.rs. The turn is taken from the sim's positions after the step
// (the build turns it in Tom's tick before the loop that finds Jerry, with Jerry's place of that moment).
// Unverified: the material's opacity is not in the cooked json (only EmissiveColor is listed, as M_Smoke's mask):
// taken as the texture's alpha (its colour is orange everywhere, so an opacity of 1 would draw a square).
#[derive(Debug)]
pub(crate) struct TomJerryHud {
    corks: Vec<Option<CorkCount>>,
    pointers: Vec<Option<Pointer>>,
    map: PlayerMap,
    /// The cork widget's tree and Update animation, when the bundle has the class.
    proto: Option<(Tree, Anim)>,
    colors: Vec<Rgba>,
    /// The pointer picture's object path.
    pointer_tex: Option<String>,
}

impl TomJerryHud {
    /// tj: per fighter, whether it is a Tom and Jerry (the match's characters).
    pub(crate) fn new(b: &Bundle, players: &[BrokerPlayer], tj: &[bool], map: PlayerMap) -> TomJerryHud {
        let pointer_tex = b.textures.get(POINTER_TEX).map(|t| t.entry().to_string());
        let pointers = tj
            .iter()
            .map(|&is| {
                is.then(|| {
                    let mut t = Tree::empty(b);
                    let image = t.create_plain(b, "Image", POINTER_TEX);
                    let w = t.widget_mut(image);
                    w.tex.clone_from(&pointer_tex);
                    w.brush_w = 128.0;
                    w.brush_h = 128.0;
                    w.color = [1.0; 4];
                    Pointer { on: false, x: 0.0, y: 0.0, rel: 0.0, z: 0.0, facing: 1.0, t, image }
                })
            })
            .collect();
        let proto = b
            .widgets
            .contains_key(CORK_WIDGET)
            .then(|| (instantiate(b, CORK_WIDGET, "Widget"), Anim::of(b, CORK_WIDGET, "Update")));
        let colors = (0..tj.len())
            .map(|i| players.get(i).map_or([1.0; 4], |p| if p.is_me { p.focus } else { p.team }))
            .collect();
        TomJerryHud { corks: tj.iter().map(|_| None).collect(), pointers, proto, colors, pointer_tex, map }
    }

    /// After each World step.
    pub(crate) fn observe(&mut self, w: &World, _now: f64, real: f64, _inputs: &mut InputLabels, _order: &mut u64) {
        for i in 0..self.pointers.len() {
            let (Some(pt), Some(f)) = (self.pointers[i].as_mut(), self.map.fighter(w, i)) else { continue };
            let Some(s) = tj_of(w, f) else { continue };
            let fx = w.fighter(f).body.x;
            let fy = actor_y(w, f);
            let fa = facing(w, f);
            // the pointer
            pt.on = s.pawn.pointer_visible && w.fighter(f).life.dead <= 0.0;
            pt.x = f64::from(fx);
            pt.y = f64::from(fy);
            pt.facing = f64::from(fa);
            let target = match s.pawn.spawned_jerry {
                Some(ActorRef::Actor(id)) if w.actors.actors.get(id).is_some_and(|a| !a.dead) => jerry_of(w, id),
                _ => None,
            };
            if let Some(t) = target {
                let (dy, dz) = (-(t.x - fx), t.y - fy);
                let l = dy.hypot(dz);
                if l > 0.0 {
                    pt.rel = f64::from((dy / l) * fa);
                    pt.z = f64::from(dz / l);
                }
            }
            // the cork count
            let jerry = match s.jerry_actor {
                Some(ActorRef::Actor(id)) => w.actors.actors.get(id).filter(|a| !a.dead).zip(jerry_of(w, id)),
                _ => None,
            };
            if jerry.is_some() && self.corks[i].is_none() {
                self.corks[i] = self.proto.as_ref().map(|(t, update)| {
                    let mut t = t.clone();
                    let number = t.find("Number");
                    let backing = t.find("Backing");
                    // OnColor @166-286
                    t.widget_mut(backing).color = self.colors[i];
                    CorkCount {
                        t,
                        number,
                        update: update.clone(),
                        started: -1.0,
                        last: 0.0,
                        actor: -1,
                        count: -1,
                        side: 0.0,
                        on: false,
                        at: [0.0; 3],
                    }
                });
            }
            let Some(c) = self.corks[i].as_mut() else { continue };
            c.on = jerry.is_some_and(|(a, _)| s.champagne.actor_id == a.seq && s.champagne.widget_visible);
            let Some((a, j)) = jerry else { continue };
            if c.actor != a.seq {
                c.actor = a.seq;
                c.side = 0.0;
                c.set_count(s.cork_ammo, real, false);
            } else if s.cork_ammo != c.count {
                c.set_count(s.cork_ammo, real, true);
            }
            if j.following.is_some() {
                c.side = if j.facing < 0.0 { -FOLLOW_Y } else { FOLLOW_Y };
            }
            // the relative location turned by the actor's rotation (pitch, yaw; roll 0)
            let (rx, ry, rz) = if c.side != 0.0 { (0.0, c.side, 0.0) } else { (REL[0], REL[1], REL[2]) };
            let (sp, cp) = j.rot_pitch.to_radians().sin_cos();
            let (sy, cy) = j.rot_yaw.to_radians().sin_cos();
            c.at[0] = rx * f64::from(cp) * f64::from(cy) - ry * f64::from(sy) - rz * f64::from(sp) * f64::from(cy);
            c.at[1] = -f64::from(j.x) + rx * f64::from(cp) * f64::from(sy) + ry * f64::from(cy)
                - rz * f64::from(sp) * f64::from(sy);
            c.at[2] = f64::from(j.y) + rx * f64::from(sp) + rz * f64::from(cp);
        }
    }

    /// The pointers: world quads, before the screen layer.
    pub(crate) fn draw_world(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        _dpi: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        let mut drawn = Vec::new();
        if self.pointer_tex.is_none() {
            return drawn;
        }
        for (k, pt) in self.pointers.iter_mut().flatten().enumerate() {
            // never turned, or edge-on (yaw 0)
            if !pt.on || pt.rel == 0.0 {
                continue;
            }
            // the look direction in UE (Y, Z) for the actor's facing now; the plane's V axis is -d, its U the arrow's -Z
            let (dy, dz) = (pt.rel * pt.facing, pt.z);
            let (cy, cz) = (-pt.x + POINTER_REL * dy, pt.y + POINTER_REL * dz);
            let sy = if dy < 0.0 { -1.0 } else { 1.0 };
            let cpitch = dy.abs();
            let (Some(c), Some(u), Some(v)) = (
                project([0.0, cy, cz]),
                project([0.0, cy + POINTER_HALF * sy * dz, cz - POINTER_HALF * cpitch]),
                project([0.0, cy - POINTER_HALF * dy, cz - POINTER_HALF * dz]),
            ) else {
                continue;
            };
            let (ax, ay, bx, by) =
                ((u[0] - c[0]) / 64.0, (u[1] - c[1]) / 64.0, (v[0] - c[0]) / 64.0, (v[1] - c[1]) / 64.0);
            let root = pt.image;
            layout_widget(&mut pt.t, fonts, root, Geometry { pos: [0.0, 0.0], size: [128.0, 128.0] }, 1.0);
            let m = [ax, ay, bx, by, c[0] - 64.0 * ax - 64.0 * bx, c[1] - 64.0 * ay - 64.0 * by];
            paint_widget(&pt.t, fonts, out, root, m, 1.0);
            drawn.push(k);
        }
        drawn
    }

    /// The cork counts: the screen layer's widgets, after the pointers' places in `roots`.
    pub(crate) fn draw_screen(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        dpi: f64,
        real: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        let base = self.pointers.iter().flatten().count();
        let mut drawn = Vec::new();
        for (k, c) in self.corks.iter_mut().flatten().enumerate() {
            if c.on && c.draw(fonts, out, dpi, real, project) {
                drawn.push(base + k);
            }
        }
        drawn
    }

    /// The pointers (`TomJerryHud`), then the cork counts (`CorkCount`) made so far.
    pub(crate) fn roots(&self) -> Vec<(&'static str, &Tree)> {
        let mut v: Vec<(&'static str, &Tree)> = Vec::new();
        v.extend(self.pointers.iter().flatten().map(|p| ("TomJerryHud", &p.t)));
        v.extend(self.corks.iter().flatten().map(|c| ("CorkCount", &c.t)));
        v
    }
}
