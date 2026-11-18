//! Second views on a fighter: a skinned mesh component a buff puts on a fighter's pawn and poses with another
//! character's montages and clips. Each is a whole
//! view (crate::pose::Rec: its meshes, materials, parts, armor copies and its character's pose hook) made with the
//! match, kept out of the frame until a buff takes it and handed back when the buff ends; the buff's own module
//! poses it each tick from the World, and the frame draws it as an actor pose of its own,
//! tagged with the fighter it is on (frame::SecondRef: the effects spawn its montages' particle notifies on it).
//!
//! The port's own: every view the match can need is made when the match is made (ViewPoser::new), where a
//! Polymorph view that no kit names, and a Disguise copy of an owner whose kit does not give the buff, would be made
//! when the buff appears. A buff with no view made for it is warned once and not drawn.
//! Deviation: no first, empty draw of each view is made under the loading screen (two warm-up frames): the renderer
//! builds its programs when a mesh is first drawn.
//! Deviation: the second views are not in their pawn's priority capture, and the Disguise's HUD anchors
//! (uiAnchor: the victim's widget offsets) are not given to the HUD. The Polymorph's
//! (UITopOffset) is mvs-app run.rs's `polymorph_top`, from the sim's state.

mod disguise;
mod polymorph;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mvs_sim::World;
use mvs_sim::fighter::FighterId;

use crate::anim::Clips;
use crate::frame::{ActorPose, SecondKind, SecondRef};
use crate::math::{self, Mat4};
use crate::pose::{Rec, ViewPoser, meshes_of};

pub(crate) use disguise::Copies;
pub(crate) use polymorph::Polymorphs;

/// The presentation step.
const STEP: f64 = 1.0 / 60.0;

/// One second view: its Rec, where it was drawn and is, and whether a buff has it.
#[derive(Debug)]
struct SecondView {
    rec: Rec,
    kind: SecondKind,
    /// The character whose skeleton data its main mesh has (None: a kit's prop mesh).
    skeleton: Option<Arc<str>>,
    /// A buff has it now.
    active: bool,
    /// The take's number (SecondRef::take), from TAKES.
    take: u64,
    shown: bool,
    px: f64,
    py: f64,
    x: f64,
    y: f64,
    raw_x: f64,
    raw_y: f64,
    facing: f64,
}

/// The takes counted in the process (a take's number is unique to it).
static TAKES: AtomicU64 = AtomicU64::new(0);

impl SecondView {
    fn new(rec: Rec, kind: SecondKind, skeleton: Option<&str>) -> SecondView {
        SecondView {
            rec,
            kind,
            skeleton: skeleton.map(Arc::from),
            active: false,
            take: 0,
            shown: false,
            px: 0.0,
            py: 0.0,
            x: 0.0,
            y: 0.0,
            raw_x: f64::NAN,
            raw_y: f64::NAN,
            facing: 1.0,
        }
    }

    /// A buff takes it: it starts as a new view (CharacterView.restart), its pose hook keeping what it held.
    fn take(&mut self) {
        self.active = true;
        self.take = TAKES.fetch_add(1, Ordering::Relaxed) + 1;
        self.rec.view.restart();
        self.shown = false;
        self.raw_x = f64::NAN;
        self.raw_y = f64::NAN;
    }

    /// The buff ended (or a reset): out of the frame until a buff takes it again.
    fn drop_view(&mut self) {
        self.active = false;
        self.shown = false;
    }

    /// The tick's place of the mesh at (x, y), from fighter `f`'s sim place: a mesh the step did not carry here is
    /// not drawn on the way.
    fn placed(&mut self, w: &World, f: FighterId, x: f64, y: f64, shown: bool) {
        let b = &w.fighter(f).body;
        let carried = shown && self.shown && self.raw_x == f64::from(b.prev_x) && self.raw_y == f64::from(b.prev_y);
        self.px = if carried { self.x } else { x };
        self.py = if carried { self.y } else { y };
        if !carried {
            self.rec.view.cut();
        }
        self.x = x;
        self.y = y;
        self.raw_x = f64::from(b.x);
        self.raw_y = f64::from(b.y);
        self.facing = f64::from(b.facing_dir);
        self.shown = shown;
    }

    /// The frame (the module's `frame`): placed between the two ticks and presented, its meshes into `out`.
    fn present(&mut self, a: f64, cs: &mut Vec<Mat4>, cs_ue: &mut Vec<Mat4>, out: &mut Vec<ActorPose>) {
        if !self.active || !self.shown {
            return;
        }
        let v = &mut self.rec.view;
        v.place(self.px + (self.x - self.px) * a, self.py + (self.y - self.py) * a, self.facing);
        v.present(a);
        v.bones_cs(cs);
        cs_ue.clear();
        cs_ue.extend(cs.iter().map(math::to_ue));
        let meshes = meshes_of(&self.rec, cs, cs_ue);
        let second = SecondRef {
            fighter: self.rec.id,
            kind: self.kind,
            skeleton: self.skeleton.clone(),
            left: self.rec.view.facing_left,
            take: self.take,
        };
        out.push(ActorPose { meshes, second: Some(second), ..ActorPose::default() });
    }
}

/// The match's second views (the buffs' modules' state).
#[derive(Debug, Default)]
pub(crate) struct Seconds {
    poly: Polymorphs,
    copies: Copies,
    cs: Vec<Mat4>,
    cs_ue: Vec<Mat4>,
}

impl Seconds {
    /// Every second view the match can need (the header), after the fighters' own views.
    pub(crate) fn new(p: &mut ViewPoser, store: &mvs_store::Store, w: &World) -> crate::Result<Seconds> {
        Ok(Seconds {
            poly: Polymorphs::new(p, store, w)?,
            copies: Copies::new(p, store, w)?,
            cs: Vec::new(),
            cs_ue: Vec::new(),
        })
    }

    /// One presentation step, after the fighters' views are placed: each buff's view taken, posed or handed back
    /// (`items`: the items part's clips, in a match with items).
    pub(crate) fn place(&mut self, w: &World, clips: &mut HashMap<String, Clips>, items: Option<&mut Clips>) {
        self.poly.place(w, clips, items);
        self.copies.place(w, clips);
    }

    /// One frame: each shown view's meshes into `out`.
    pub(crate) fn present(&mut self, a: f64, out: &mut Vec<ActorPose>) {
        let Seconds { poly, copies, cs, cs_ue } = self;
        for s in poly.views_mut().chain(copies.views_mut()) {
            s.present(a, cs, cs_ue, out);
        }
    }

    /// The Lab's reset (the modules' `reset`): every view handed back.
    pub(crate) fn reset(&mut self) {
        for s in self.poly.views_mut().chain(self.copies.views_mut()) {
            s.drop_view();
        }
        self.poly.reset();
        self.copies.reset();
    }
}

/// `warnOnce`: `what` on stderr the first time it is met in the process.
fn warn_once(what: String) {
    static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !seen.contains(&what) {
        eprintln!("warning: {what}");
        seen.push(what);
    }
}
