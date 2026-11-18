//! The Court's basketball (Item_Basketball_C; the sim's
//! stages/the_court/ball.rs), a World.items actor with no spawner: a class rule, as the characters' items are.
//! - Its BasketballGeo (StaticMesh M010_Ball_01 under the Capsule root, no relative place, scale 0.9) is the scenery
//!   file's (one hidden instance per material the graph sets on slot 0: `gray`, `red`, `blue`), which the court block
//!   of mvs-render claims; this rule places it: the stage's meshes come with the stage, not a fighter. It gives its
//!   variant and UE transform in [`ActorMesh::scenery`], which the caller hands to `Renderer::scenery_place`.
//! - Out of a hand the actor's rotation is the item's (yaw by facing, its pitch), as the shared item path draws Geo;
//!   in a hand the root is on the holder's hold socket (as the dynamite's).
//! - The material: BeginPlay makes the three instances (@225-398) and the template's Gray stays on. SetTeamColor
//!   (@1445; the parent's PickUp calls it, ItemActorParent @7757 -> @80) with an owning BP_FighterCharacter: Red for
//!   FTrunc(the colour value) 0, Blue for 1 (@1772); ItemThrowHitMulticast (@1973) puts Gray back after a hit on an
//!   enemy or on anything but a player (@2019-2047, SetMaterial @1918). Read from the sim each frame: held -> the
//!   holder's; thrown -> its LastHolder's (the thrower: a throw comes from the hand); else (on the floor, never held,
//!   after a throw's hit) Gray.
// Deviation: a throw that hits an ally keeps the colour in the build (HitPlayer and not Enemy) and turns Gray here
// once IsThrown is off; the colour value is the holder's GetColorIndex; one other than 0 / 1 (the select's unwritten
// default, SetMaterial(0, None): the mesh's own M010_Ball_01_M) is drawn Gray.
// Deviation: the custom depth outline a held item gets from its grabber is not drawn.

use mvs_sim::World;
use mvs_sim::actors::{ActorRef, ItemExt, ItemId};
use mvs_sim::fighter::match_side::hold_socket;

use crate::M4;
use crate::actor_view::{Comp, Xform, item_rows, place_at, place_under};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind, MeshRule};
use crate::frame::{MeshPose, SceneryPlace};

const CLS: &str = "Item_Basketball_C";
const COMP: &str = "BasketballGeo";

/// The scenery block and part the ball's meshes are (the court block's `ball`).
const BLOCK: &str = "court";
const PART: &str = "ball";

#[derive(Debug)]
struct Ball {
    id: ItemId,
    c: Comp,
    /// The variant and transform of the last `place`; None: hidden.
    shown: Option<(&'static str, M4)>,
}

/// An Xform as the row-vector M4 (rows scaled by the scale of their axis).
fn m4_of(x: &Xform) -> M4 {
    let row = |i: usize| [x.rot[3 * i] * x.scale[i], x.rot[3 * i + 1] * x.scale[i], x.rot[3 * i + 2] * x.scale[i], 0.0];
    [row(0), row(1), row(2), [x.loc[0], x.loc[1], x.loc[2], 1.0]]
}

/// The variant of fighter `id` (a pawn id): its colour index 0 red, 1 blue, any other (or none) gray.
fn variant_of(host: &ActorHost<'_>, w: &World, id: i32) -> &'static str {
    let Some(f) = w.by_id(id) else { return "gray" };
    match host.colour(w, f).color_index {
        0 => "red",
        1 => "blue",
        _ => "gray",
    }
}

fn ball_mesh(_host: &mut ActorHost<'_>, _w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let g = cs.get(COMP)?;
    if !matches!(g.kind, ComponentKind::StaticMesh { .. }) {
        return None;
    }
    let c = Comp { par: g.parent.clone(), rl: g.loc, rot: g.rot, rs: g.scale };
    Some(Box::new(Ball { id, c, shown: None }))
}

impl ActorMesh for Ball {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        _out: &mut Vec<MeshPose>,
    ) {
        self.shown = None;
        let Some(it) = w.actors.items.get(self.id) else { return };
        let ItemExt::Basketball(b) = &it.ext else { return };
        let hold = at.holder.map(|h| (h, hold_socket(w, h)));
        let no_socket = hold.as_ref().is_some_and(|(_, s)| s == "None");
        if no_socket {
            warn_no_socket();
        }
        if !shown || no_socket {
            return;
        }
        let v = if it.holder >= 0 {
            variant_of(host, w, it.holder)
        } else if it.thrown {
            variant_of(host, w, b.last_holder)
        } else {
            "gray"
        };
        // (socketFollow's `missing`, a bone the view lacks: the holder's mesh origin)
        let held = hold.and_then(|(h, s)| host.hold_place(w, h, &s));
        let xf = match held {
            Some(s) => place_at(&self.c, &self.c.rot, self.c.rs, &s.rot, f64::from(it.scale), s.loc),
            None => {
                let (x, y) = at.at(alpha);
                place_under(&self.c, &item_rows(it), f64::from(it.scale), x, y)
            }
        };
        self.shown = Some((v, m4_of(&xf)));
    }

    fn scenery(&self) -> Option<SceneryPlace> {
        Some(SceneryPlace { block: BLOCK, part: PART, shown: self.shown })
    }

    fn end(&mut self) {
        self.shown = None;
    }
}

/// The warning of a held ball with no hold socket, once a run.
fn warn_no_socket() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        eprintln!("warning: {CLS}: a holder with no hold socket: the held ball is hidden");
    });
}

/// The class's own drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[(CLS, MeshRule { rot: None, mesh: Some(ball_mesh) })];
