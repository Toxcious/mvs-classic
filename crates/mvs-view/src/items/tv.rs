//! Item_Helper (the TV, one of Bugs Bunny's ACME box items and a random item; mvs-sim items/helper.rs) as a match draws
//! it.
//!
//! Item_Helper_C (SCS_Node_3): Item_TV_Mesh = StaticMesh Item_TV_Mesh (Blueprints/Items/Item_TV) under the Capsule
//! (SpawnedActorParent's root, bAbsoluteScale: the spawn transform's scale 1, the sim's item.scale) at (0, -10, -40),
//! yaw -50, scale 9, CastShadow false, no OverrideMaterials, and no graph of Item_Helper or its parents sets one: the
//! mesh's two slots, Tv_Mat (TV_Color) and TV_Static_MAT (the screen, unlit). Out of a hand the actor's rotation is
//! the item's (yaw by facing, its pitch), as the shared item path draws Geo and the dynamite's rule does; in a hand
//! the root is on the holder's hold socket (as the dynamite's and the Meeseek box's).
//! The mesh is the asset of the spawner's character that has the class (the TV comes with Bugs Bunny's data); a
//! random item, with no spawner, and a Mystery box's, whose spawner's data may not have it, take the items part's.
//! Deviation: the custom depth outline a held item gets from its grabber is not drawn.

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_sim::fighter::match_side::hold_socket;

use crate::actor_view::{Comp, StaticView, place_at, place_under};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, MeshRule};
use crate::frame::MeshPose;
use crate::skin_intro::warn_once;

const CLS: &str = "Item_Helper_C";
const COMP: &str = "Item_TV_Mesh";
/// The TV's actors entry in Bugs Bunny's draws (the mesh's name).
pub(crate) const TV_MESH: &str = "Item_TV_Mesh";

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[(CLS, MeshRule { rot: None, mesh: Some(tv_mesh) })];

#[derive(Debug)]
struct TvMesh {
    view: StaticView,
    c: Comp,
    /// The no-hold-socket warning given (its text made once).
    warned: bool,
}

fn tv_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Item(id) = a else { return None };
    let it = w.actors.items.get(id)?;
    let t = cs.get(COMP)?;
    let item = |host: &ActorHost<'_>| StaticView::item(host, &format!("{CLS}.{COMP}"), "template");
    let view = match it.spawner {
        Some(s) => StaticView::new(host, w, actor_def_owner(w, s, it.cls), TV_MESH, "local").or_else(|| item(host)),
        None => item(host),
    }?;
    let c = Comp { par: t.parent.clone(), rl: t.loc, rot: t.rot, rs: t.scale };
    Some(Box::new(TvMesh { view, c, warned: false }))
}

impl ActorMesh for TvMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let mut no_socket = false;
        let hold = at.holder.and_then(|h| {
            let socket = hold_socket(w, h);
            if socket == "None" {
                no_socket = true;
                if !self.warned {
                    self.warned = true;
                    warn_once(format!("{CLS}: no hold socket on fighter {}: the held TV is hidden", w.fighter(h).id));
                }
                return None;
            }
            host.hold_place(w, h, &socket)
        });
        let on = shown && !no_socket;
        self.view.visible = on;
        if !on {
            return;
        }
        let xf = match hold {
            Some(s) => place_at(&self.c, &self.c.rot, self.c.rs, &s.rot, at.scale, s.loc),
            None => {
                let (x, y) = at.at(alpha);
                place_under(&self.c, &at.rot, at.scale, x, y)
            }
        };
        self.view.set(&xf);
        self.view.draw(out);
    }
}
