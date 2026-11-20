//! UI_Finn_DashTarget: the dash targets
//! on the two actors that carry it, in the widget components' screen layer: Finn's FinnDashThrownTarget (the gem, its
//! AimWidget) and LeBron's BasketBallProjectile (its UIVisuals widget component).

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::item::ItemExt;
use mvs_sim::actors::projectile::{ProjectileExt, instigator};
use mvs_sim::chars::finn::gem::fade_hidden;

use super::HudSetup;
use super::attached::draw_box;
use super::common::{Anim, BrokerPlayer, PlayerMap};
use super::input_label::{LabelSources, PlacedInputLabel};
use crate::bundle::Bundle;
use crate::element::ElementList;
use crate::slate::{Tree, Vis, WidgetId, instantiate};
use crate::text::Fonts;

const CLASS: &str = "UI_Finn_DashTarget";
// Unverified: the DrawSize 500 x 500 and pivot 0.5 are UWidgetComponent's class defaults (neither actor's data sets
// them). Deviation: the widgets are painted after the skill shot indicators, not ordered by view depth with the
// layer's other slots; the ball's relative offset (0, -5, 0) is left out.
const DRAW_SIZE: f64 = 500.0;

/// One actor's component as observed after a World step: at its UE location (the gem's: the interpolated item place
/// in draw when the HUD has it).
/// slot: the component's visible flag and its actor not hidden in game: while off the slot leaves the layer and the
/// widget is neither painted nor ticked (its animations stand still). inner: the two Handle calls.
struct Source {
    at: [f64; 3],
    /// The gem's item id (draw's interpolated item places), -1 for none.
    item: i32,
    slot: bool,
    inner: bool,
}

/// The class's tree and animations, made once.
#[derive(Debug)]
struct Proto {
    t: Tree,
    intro: Anim,
    looped: Anim,
}

/// Both are WidgetComponents in Space Screen with the class defaults' DrawSize 500 x 500 and pivot (0.5, 0.5):
/// SWorldWidgetScreenLayer lays the widget out at the component's projected location rounded to a whole pixel
/// (hud/skill_shot.rs). The tree centres its VerticalBox (anchors and alignment 0.5): the 100 x 100 SizeBox with
/// Overlay_0 (render scale 0.75) and its Backing (Finn_DashTargetUI), then InputsHolder (Collapsed in the tree): a
/// WBP_Quadrilateral behind ui_arrow_r, ui_icons_plus and a UI_InputLabel (GameSpecial, modifier SideAir,
/// ForceHideText: the button alone; SizeIconSize's default 32, PlayerIndex 0).
/// Construct: PlayAnimation(IntroFlash) once (Backing, 4000-18001 at 60000) and PlayAnimation(LoopingRotate, loops 0)
/// (Overlay_0's rotation 0 -> 360 over 1 s). HandleInputVisibility(b) / HandleReticleVisibility(b): InputsHolder /
/// Overlay_0 SelfHitTestInvisible or Collapsed (HandleDirection has no caller).
#[derive(Debug)]
struct Target {
    t: Tree,
    overlay: WidgetId,
    inputs: WidgetId,
    label: PlacedInputLabel,
    label_set: bool,
    /// The widget's own time: real time while its slot is on.
    clock: f64,
    last_real: f64,
    intro_last: f64,
    inner: Option<bool>,
    slot: bool,
    item: i32,
    at: [f64; 3],
}

impl Target {
    fn new(p: &Proto) -> Target {
        let mut t = p.t.clone();
        let overlay = t.find("Overlay_0");
        let inputs = t.find("InputsHolder");
        let button = t.find("inputButton");
        let label = PlacedInputLabel::new(&mut t, button, 32.0);
        let mut x = Target {
            t,
            overlay,
            inputs,
            label,
            label_set: false,
            clock: 0.0,
            last_real: -1.0,
            intro_last: 0.0,
            inner: None,
            slot: false,
            item: -1,
            at: [0.0; 3],
        };
        x.tick(p, 0.0, true, &LabelSources::default());
        x
    }

    fn set_inner(&mut self, on: bool) {
        if self.inner == Some(on) {
            return;
        }
        self.inner = Some(on);
        let v = if on { Vis::SelfHitTestInvisible } else { Vis::Collapsed };
        let (i, o) = (self.inputs, self.overlay);
        self.t.widget_mut(i).vis = v;
        self.t.widget_mut(o).vis = v;
    }

    /// real: the HUD's real time; first: Construct's evaluation at 0.
    fn tick(&mut self, p: &Proto, real: f64, first: bool, labels: &LabelSources) {
        if !first {
            if self.last_real >= 0.0 {
                self.clock += real - self.last_real;
            }
            self.last_real = real;
        }
        let root = self.t.root();
        let sec = self.clock.min(p.intro.end());
        p.intro.apply(&mut self.t, root, sec, self.intro_last);
        self.intro_last = sec;
        let l = self.clock % p.looped.end();
        p.looped.apply(&mut self.t, root, l, l);
        if !first {
            if !self.label_set {
                // (Construct's `Params` write: GameSpecial, PlayerIndex 0)
                self.label.set(&mut self.t, 1, 0, labels);
                self.label_set = true;
            }
            self.label.refresh(&mut self.t, labels);
        }
    }
}

/// The dash targets by actor.
#[derive(Debug, Default)]
pub struct DashTargets {
    proto: Option<Proto>,
    targets: IndexMap<i32, Target>,
}

impl DashTargets {
    pub fn new(b: &Bundle) -> DashTargets {
        let proto = b.widgets.contains_key(CLASS).then(|| Proto {
            t: instantiate(b, CLASS, "DashTarget"),
            intro: Anim::of(b, CLASS, "IntroFlash"),
            looped: Anim::of(b, CLASS, "LoopingRotate"),
        });
        DashTargets { proto, targets: IndexMap::new() }
    }

    /// After each World step: the actors that carry a target, read off their owners' spawned actors.
    /// FinnDashThrownTarget: SpawnVisuals @4180 sets the AimWidget's visible flag to its InstigatingFinn's
    /// IsLocallyControlled and @4614 HandleInputVisibility(true); the apple's visuals (weakThrow, @3563) hide the
    /// component; the fade blink hides the actor (Gem.hidden). PickUp by InstigatingFinn ToggleUIOwner(false) @233
    /// and Thrown ToggleUIOwner(true) @5968 (both: HandleInputVisibility and HandleReticleVisibility): the inner
    /// widgets are off while its own Finn holds it. MulticastUIAvailability @6727 destroys the widget online on a
    /// machine whose local pawn is not InstigatingFinn: the locally controlled test above leaves the same. The
    /// UiChecker overlaps are empty and UpdateTargetVisuals / SetTargetVisibilityLocal have no caller.
    /// BasketBallProjectile UIVisuals @5479-6223: the component's visible flag On, input and reticle On and the
    /// instigator locally controlled and GetFighterCharacter == 21 (Pawn_c016, LeBron); its callers pass HitEnemy as
    /// it is written to AllowTeleportTo: shown while allowTeleportTo.
    // Unverified: locally controlled is read as a human player of this page; a bot's pawn may be locally controlled
    // offline.
    pub fn observe(&mut self, w: &World, players: &[BrokerPlayer], setup: &HudSetup) {
        if self.proto.is_none() {
            return;
        }
        let map = PlayerMap::of(setup);
        let broker = |f: mvs_sim::fighter::FighterId| map.index(w, f).and_then(|i| players.get(i));
        let mut seen: Vec<(i32, Source)> = Vec::new();
        for f in w.all_ids() {
            for &a in &w.fighter(f).items.spawned_actors {
                match a {
                    ActorRef::Item(id) => {
                        let Some(it) = w.actors.items.get(id) else { continue };
                        let ItemExt::Gem(g) = &it.ext else { continue };
                        if it.dead {
                            continue;
                        }
                        let inst = g.instigator;
                        let mine = inst.and_then(broker).is_some_and(|b| b.is_local && !b.is_bot);
                        let holder = inst.is_some_and(|i| it.holder == w.fighter(i).id);
                        seen.push((
                            it.seq,
                            Source {
                                at: [0.0, f64::from(-it.x), f64::from(it.y)],
                                item: it.seq,
                                slot: mine && !g.weak_throw && !fade_hidden(g),
                                inner: !(inst.is_some() && holder),
                            },
                        ));
                    }
                    ActorRef::Projectile(id) => {
                        let Some(p) = w.actors.projectiles.get(id) else { continue };
                        let ProjectileExt::Ball(b) = &p.ext else { continue };
                        if p.dead || !b.allow_teleport_to {
                            continue;
                        }
                        // the instigator's test (UIVisuals reads GetInstigator): a team mate's AllyPass ball is in
                        // his list, its Instigator LeBron
                        let inst = instigator(p);
                        let lebron = inst.is_some_and(|i| {
                            map.index(w, i)
                                .and_then(|i| setup.players.get(i))
                                .is_some_and(|s| s.character.as_deref() == Some("lebron"))
                        });
                        let ip = inst.and_then(broker);
                        seen.push((
                            -1 - p.seq,
                            Source {
                                at: [0.0, f64::from(-p.x), f64::from(p.y)],
                                item: -1,
                                slot: true,
                                inner: ip.is_some_and(|b| b.is_local && !b.is_bot) && lebron,
                            },
                        ));
                    }
                    _ => {}
                }
            }
        }
        let Some(proto) = &self.proto else { return };
        self.targets.retain(|id, _| seen.iter().any(|s| s.0 == *id));
        for (id, s) in seen {
            let t = self.targets.entry(id).or_insert_with(|| Target::new(proto));
            t.set_inner(s.inner);
            if t.slot && !s.slot {
                // The slot left the layer: the next tick takes the time from where it stood.
                t.last_real = -1.0;
            }
            t.slot = s.slot;
            t.at = s.at;
            t.item = s.item;
        }
    }

    /// The targets painted, by their place (`tree`); items: the items' actor locations by id.
    #[allow(clippy::too_many_arguments, reason = "the HUD's draw call carries its shared lists")]
    pub fn draw(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        dpi: f64,
        real: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
        items: &[(i32, [f64; 3])],
        labels: &LabelSources,
    ) -> Vec<usize> {
        let Some(proto) = &self.proto else { return Vec::new() };
        let mut drawn = Vec::new();
        for (k, t) in self.targets.values_mut().enumerate() {
            if !t.slot {
                continue;
            }
            t.tick(proto, real, false, labels);
            // nothing of the tree is visible
            if t.inner != Some(true) {
                continue;
            }
            let at = if t.item >= 0 { items.iter().find(|i| i.0 == t.item).map_or(t.at, |i| i.1) } else { t.at };
            if let Some(p) = project(at) {
                let (x, y) = ((p[0] + 0.5).floor() as i32, (p[1] + 0.5).floor() as i32);
                if draw_box(&mut t.t, fonts, out, x, y, [DRAW_SIZE; 2], dpi) {
                    drawn.push(k);
                }
            }
        }
        drawn
    }

    /// A target's tree by its place.
    pub fn tree(&self, k: usize) -> Option<&Tree> {
        self.targets.get_index(k).map(|(_, t)| &t.t)
    }
}
