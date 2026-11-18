//! Stripe's gun reticle: C023B_AimWidget_C on the TargetTracker's target, in the
//! widget components' screen layer.

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::chars::stripe::state::stripe_of;
use mvs_sim::fighter::{actor_y, facing};

use crate::bundle::Bundle;
use crate::element::ElementList;
use crate::hud::attached::draw_box;
use crate::hud::common::Anim;
use crate::hud::input_label::InputLabels;
use crate::slate::{Tree, instantiate};
use crate::text::Fonts;

const WIDGET: &str = "UI_C023B_Target";
/// UWidgetComponent's DrawSize default (the component sets none).
const DRAW_SIZE: f64 = 500.0;
/// AimWidget_GEN_VARIABLE RelativeLocation Y.
const REL_Y: f64 = 100.0;

/// C023B_AimWidget_C (its ubergraph is empty): a DefaultSceneRoot with one WidgetComponent AimWidget in Space Screen,
/// WidgetClass UI_C023B_Target, RelativeLocation (0, 100, 0), RelativeRotation yaw 90, no DrawSize or pivot of its own
/// (the class defaults 500 x 500 and (0.5, 0.5)), visible, no owner-only flag: every page shows it.
/// Comp_C023B_TargetTracker UpdateTargetMulticast(NewTarget, Add): Add true @3541: a valid AimWidget is
/// K2_AttachToActor(CurrentTarget, None, SnapToTarget, KeepWorld, KeepWorld) @3584; else one is spawned at
/// CurrentTarget's transform @3696-3871 and attached the same way @3928. Add false @4190 and the tracker's EndPlay
/// @4323: K2_DestroyActor. So the widget lives exactly while StripeState.target is set.
/// UI_C023B_Target is UI_Finn_DashTarget's tree with MI_State_Targeted as its Backing: Construct @10
/// PlayAnimation(IntroFlash) once and @57 PlayAnimation(LoopingRotate, loops 0); InputsHolder is Collapsed in the
/// tree, and nothing calls the Handle functions: the reticle alone.
/// The place: the actor snapped to the target's actor location; the component's (0, 100, 0) turned by the actor's
/// yaw. A first attach keeps the spawn transform's rotation, the target's own (a pawn's yaw -90 (right) / +90 (left)
/// puts the offset at UE +X / -X); a re-attach keeps the widget's world rotation relative to the new target.
// Unverified: a spawned actor target's (the saw's) yaw is read off its facing like a pawn's.
// Deviation: the widget is painted after the other spawned actors' widgets, not by its view distance among the screen
// layer's slots; the target is read after each World step (a target set and cleared inside one step is not drawn).
#[derive(Debug)]
struct Reticle {
    t: Tree,
    /// The real time of Construct, -1: not yet painted.
    born: f64,
    intro_last: f64,
    /// The actor's yaw relative to its parent, as a sign.
    rel: f64,
    /// The component's location (UE).
    at: [f64; 3],
}

#[derive(Debug, Default)]
struct Seen {
    /// The AimWidget actor, None while there is none.
    reticle: Option<Reticle>,
    /// The target it was last attached to.
    target: Option<ActorRef>,
    /// The widget's world yaw sign at the last step.
    side: f64,
}

#[derive(Debug)]
pub(crate) struct StripeHud {
    seen: Vec<Option<Seen>>,
    proto: Option<(Tree, Anim, Anim)>,
}

/// The target's yaw as a sign: +1 for yaw -90 (local Y = UE +X), -1 for yaw +90.
fn side_of(facing: f64) -> f64 {
    if facing < 0.0 { -1.0 } else { 1.0 }
}

impl StripeHud {
    /// stripe: per fighter slot, whether it is a Stripe.
    pub(crate) fn new(b: &Bundle, stripe: &[bool]) -> StripeHud {
        let proto = b.widgets.contains_key(WIDGET).then(|| {
            (
                instantiate(b, WIDGET, "AimWidget"),
                Anim::of(b, WIDGET, "IntroFlash"),
                Anim::of(b, WIDGET, "LoopingRotate"),
            )
        });
        StripeHud { seen: stripe.iter().map(|&s| s.then(|| Seen { side: 1.0, ..Seen::default() })).collect(), proto }
    }

    /// After each World step.
    pub(crate) fn observe(&mut self, w: &World, _now: f64, _real: f64, _inputs: &mut InputLabels, _order: &mut u64) {
        let Some((proto, _, _)) = &self.proto else { return };
        for i in 0..self.seen.len() {
            let (Some(s), Some(&f)) = (self.seen[i].as_mut(), w.players.get(i)) else { continue };
            let Some(target) = stripe_of(w, f).and_then(|x| x.target) else {
                s.reticle = None;
                continue;
            };
            let place = match target {
                ActorRef::Fighter(t) => Some((w.fighter(t).body.x, actor_y(w, t), facing(w, t))),
                ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|p| (p.x, p.y, p.facing)),
                _ => None,
            };
            let Some((tx, ty, tf)) = place else { continue };
            let sd = side_of(f64::from(tf));
            let r = match s.reticle.as_mut() {
                Some(r) => {
                    // the re-attach @3584 keeps the world rotation
                    if s.target != Some(target) {
                        r.rel = s.side * sd;
                    }
                    r
                }
                // the spawn at the target's transform @3696, attached keeping that rotation @3928
                None => {
                    s.reticle.insert(Reticle { t: proto.clone(), born: -1.0, intro_last: 0.0, rel: 1.0, at: [0.0; 3] })
                }
            };
            s.target = Some(target);
            s.side = sd * r.rel;
            r.at = [REL_Y * s.side, f64::from(-tx), f64::from(ty)];
        }
    }

    pub(crate) fn draw_world(
        &mut self,
        _fonts: &mut Fonts,
        _out: &mut ElementList,
        _dpi: f64,
        _project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        Vec::new()
    }

    /// The screen layer's widgets, after the other spawned actors'.
    pub(crate) fn draw_screen(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        dpi: f64,
        real: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        let Some((_, intro, looped)) = &self.proto else { return Vec::new() };
        let mut drawn = Vec::new();
        for (k, r) in self.seen.iter_mut().flatten().filter_map(|s| s.reticle.as_mut()).enumerate() {
            if r.born < 0.0 {
                r.born = real;
            }
            let t = real - r.born;
            let root = r.t.root();
            let sec = t.min(intro.end());
            intro.apply(&mut r.t, root, sec, r.intro_last);
            r.intro_last = sec;
            let l = t % looped.end();
            looped.apply(&mut r.t, root, l, l);
            let Some(p) = project(r.at) else { continue };
            let (x, y) = ((p[0] + 0.5).floor() as i32, (p[1] + 0.5).floor() as i32);
            if draw_box(&mut r.t, fonts, out, x, y, [DRAW_SIZE; 2], dpi) {
                drawn.push(k);
            }
        }
        drawn
    }

    pub(crate) fn roots(&self) -> Vec<(&'static str, &Tree)> {
        self.seen.iter().flatten().filter_map(|s| s.reticle.as_ref()).map(|r| ("Reticle", &r.t)).collect()
    }
}
