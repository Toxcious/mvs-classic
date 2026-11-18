//! UI_SkillshotTarget: the reticles of the shown skill shot indicators,
//! in the widget components' screen layer.

use indexmap::IndexMap;

use super::attached::draw_box;
use super::common::Anim;
use crate::bundle::Bundle;
use crate::element::ElementList;
use crate::slate::{Tree, Vis, WidgetId, instantiate};
use crate::text::Fonts;

const CLASS: &str = "UI_Skillshot_Aimpunch_Target";
const DRAW_SIZE: f64 = 128.0;

/// One indicator's UI_Skillshot_Aimpunch_Target: a WidgetComponent in Space Screen (DrawSize 128 x 128, the default
/// pivot 0.5, 0.5) under the actor's Sphere, two UI_skillshot_rotator images, Rotator under Backing.
///
/// Construct: PlayAnimation(IntroFlash) (Backing's render transform: scale (3, 2) -> 1 and rotation -166.7 -> 0),
/// Delay(IntroFlash.GetEndTime()), then Backing collapsed and PlayAnimation(LoopingRotate, loops 0 = forever)
/// (Rotator's rotation 0 -> 360 over 1 s). The actor's UnhideOnlyIfOnOwningClient shows the component only for a
/// locally controlled owner.
/// SWorldWidgetScreenLayer lays the widget out at the component's projected location, rounded to a whole pixel,
/// with the DrawSize and the pivot as the slot's alignment.
// Deviation: the Delay runs on real time with the animations; a bot's indicator is not shown.
#[derive(Debug)]
struct Target {
    t: Tree,
    backing: WidgetId,
    born: f64,
    last: f64,
    loop_start: Option<f64>,
    pos: Option<[i32; 2]>,
    /// The component's world location (UE) at the last tick.
    at: [f64; 3],
}

/// The skill shot targets by indicator.
#[derive(Debug)]
pub struct SkillshotTargets {
    /// The class's tree and animations, when the bundle has the class.
    proto: Option<(Tree, Anim, Anim)>,
    targets: IndexMap<i32, Target>,
}

impl SkillshotTargets {
    pub fn new(b: &Bundle) -> SkillshotTargets {
        let proto = b.widgets.contains_key(CLASS).then(|| {
            (
                instantiate(b, CLASS, "SkillShotIndicator"),
                Anim::of(b, CLASS, "IntroFlash"),
                Anim::of(b, CLASS, "LoopingRotate"),
            )
        });
        SkillshotTargets { proto, targets: IndexMap::new() }
    }

    /// The frame's shown indicators (their actor locations by id): one target each, made at the first.
    pub fn tick(&mut self, real: f64, anchors: &[(i32, [f64; 3])], project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>) {
        let Some((proto, intro, looped)) = &self.proto else { return };
        self.targets.retain(|id, _| anchors.iter().any(|a| a.0 == *id));
        for &(id, at) in anchors {
            let t = self.targets.entry(id).or_insert_with(|| {
                let t = proto.clone();
                let backing = t.find("Backing");
                Target { t, backing, born: real, last: 0.0, loop_start: None, pos: None, at }
            });
            let root = t.t.root();
            if t.loop_start.is_none() {
                let sec = (real - t.born).min(intro.end());
                intro.apply(&mut t.t, root, sec, t.last);
                t.last = sec;
                // GetEndTime: the playback range's upper bound.
                if real - t.born >= intro.period() {
                    t.loop_start = Some(real);
                    let b = t.backing;
                    t.t.widget_mut(b).vis = Vis::Collapsed;
                }
            }
            if let Some(start) = t.loop_start {
                let sec = (real - start) % looped.end();
                looped.apply(&mut t.t, root, sec, sec);
            }
            t.at = at;
            // (rounded to a whole pixel: Math.floor(x + 0.5))
            t.pos = project(at).map(|p| [(p[0] + 0.5).floor() as i32, (p[1] + 0.5).floor() as i32]);
        }
    }

    /// The number of targets (their places: `dist`, `draw`, `tree`).
    pub fn len(&self) -> usize {
        self.targets.len()
    }

    /// A target's distance from the view origin (UE cm, a float32 as the layer's ZOrder takes it).
    pub fn dist(&self, k: usize, origin: [f64; 3]) -> f32 {
        let Some((_, t)) = self.targets.get_index(k) else { return 0.0 };
        let d = [0, 1, 2].map(|i| t.at[i] - origin[i]);
        #[allow(clippy::cast_possible_truncation)] // (the slot's ZOrder is a float32)
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32;
        dist
    }

    /// Paints the target at place `k`. -> whether it was painted.
    pub fn draw(&mut self, k: usize, fonts: &mut Fonts, out: &mut ElementList, dpi: f64) -> bool {
        let Some((_, t)) = self.targets.get_index_mut(k) else { return false };
        let Some([x, y]) = t.pos else { return false };
        draw_box(&mut t.t, fonts, out, x, y, [DRAW_SIZE; 2], dpi)
    }

    /// A target's tree by its place.
    pub fn tree(&self, k: usize) -> Option<&Tree> {
        self.targets.get_index(k).map(|(_, t)| &t.t)
    }
}
