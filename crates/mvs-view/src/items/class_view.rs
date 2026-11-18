//! The items part's actors drawn from their class data: what a match
//! with random items spawns with no owner fighter (FFAGameMode's MapRandomActorSpawnArea), whose meshes and materials
//! no fighter's assets hold. An items-part actor that no class rule draws takes this view (actors.rs make_view); a
//! class rule draws such an actor's mesh with `StaticView::item` / `SkinnedActor::item`.
//!
//! Every mesh component of the actor's class that the items entry draws, each at its relative transform under its
//! attach parents (actor_view.rs comp-place), under the actor's root: an item's yaw by facing and pitch, a
//! projectile's actorRot, at the actor's scale. A held item hangs on its holder's hold socket (as Item_Dynamite's). A
//! component shows as its template says (bVisible, bHiddenInGame); a class whose graph shows or hides one gets its
//! own rule.
//! An AnimationSingleNode component (Item_QuestionMark, Item_BouncePad's pad) plays its AnimToPlay from BeginPlay,
//! looping (the actor's age): their templates' AnimationData hold AnimToPlay alone, so FSingleAnimationPlayData's
//! defaults of the mesh component's constructor hold (bSavedLooping and bSavedPlaying true, SavedPlayRate 1,
//! SavedPosition 0).
//! Unverified: SetTeamColor puts no set on these actors: the area spawns them with no Instigator (SpawnedActorParent
//! @22391-22784 sets the material through the Instigator's Comp_Pawn_SkinManager alone), and whether a pickup makes the
//! holder the Instigator is not read; a class that sets its own (Item_Dynamite) has a rule.
//! Its particle components that start shown and active are mvs-render's (fx/items.rs).
//! Deviation: no custom depth outline (a held item's from its grabber, an actor's from UpdateLocality).

use mvs_data::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_sim::fighter::match_side::hold_socket;
use mvs_sim::names::Name;

use crate::actor_view::{Comp, SkinnedActor, StaticView, place_at, place_under};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, ComponentKind};
use crate::frame::MeshPose;
use crate::skin_intro::warn_once;

/// The classes the renderer's effects draw by a rule of their own, held here (no mesh: mvs-render fx/shared/ice.rs).
const FX_DRAWN: &[&str] = &["IceBlockItem_C"];

/// `classView`'s `fxs`: actor `a`'s particle components that start shown and active (bAutoActivate, bVisible, not
/// bHiddenInGame), as (name, template); an item's ParticleSystem and an owned projectile's are left to the trails.
pub fn particle_components(w: &World, a: ActorRef) -> Vec<(String, String)> {
    let Some(d) = crate::actors::class_def(w, a) else { return Vec::new() };
    let skip_ps = match a {
        ActorRef::Item(_) => true,
        ActorRef::Projectile(id) => w.actors.projectiles.get(id).is_some_and(|p| p.owner.is_some()),
        _ => false,
    };
    let mut out = Vec::new();
    for (name, c) in &d.components {
        let Some(c) = c.as_obj() else { continue };
        let Some(t) = c.get("Template").and_then(Value::as_str).filter(|t| !t.is_empty() && *t != "None") else {
            continue;
        };
        let is = |k: &str| c.get(k).and_then(Value::as_bool);
        if is("bAutoActivate") == Some(false) || is("bVisible") == Some(false) || is("bHiddenInGame") == Some(true) {
            continue;
        }
        if name == "ParticleSystem" && skip_ps {
            continue;
        }
        out.push((name.clone(), t.to_owned()));
    }
    out
}

/// `noFighterData`: the fighter whose data has class `cls` (fighter `f`, else a peer) has no
/// class data of it.
fn no_fighter_data(w: &World, f: FighterId, cls: Name) -> bool {
    let from = actor_def_owner(w, f, cls);
    !w.char_data(from).fighter.actors.as_ref().is_some_and(|a| a.contains_key(w.names.str(cls)))
}

/// `fromItems`: an items-part actor with no fighter's class data: a random item (no spawner,
/// not the stage's), a projectile with no owner, or an actor whose spawner or owner and their peers have no class
/// data of it (what a random item spawns for its user: the Meeseek box's runner, Item_CarWheel's car,
/// Item_Mystery's SpawnAndEquipItem item).
pub(crate) fn from_items(w: &World, a: ActorRef) -> bool {
    let Some(items) = mvs_sim::world::flow::items_data_of(w) else { return false };
    if mvs_sim::actors::world_actor::actor_cls(w, a).is_some_and(|c| FX_DRAWN.contains(&w.names.str(c))) {
        return false;
    }
    match a {
        ActorRef::Item(id) => {
            let Some(it) = w.actors.items.get(id) else { return false };
            let cls = w.names.str(it.cls);
            items.actors.contains_key(cls)
                && match it.spawner {
                    Some(s) => no_fighter_data(w, s, it.cls),
                    None => !w.data.stage.actors.as_ref().is_some_and(|s| s.contains_key(cls)),
                }
        }
        ActorRef::Projectile(id) => {
            let Some(pr) = w.actors.projectiles.get(id) else { return false };
            items.actors.contains_key(w.names.str(pr.cls)) && pr.owner.is_none_or(|o| no_fighter_data(w, o, pr.cls))
        }
        _ => false,
    }
}

/// A mesh component's view.
#[derive(Debug)]
enum PartView {
    Static(StaticView),
    /// With its draw's key, which names its AnimToPlay (ActorHost::item_clip).
    Skinned(Box<SkinnedActor>, String),
}

#[derive(Debug)]
struct Part {
    comp: Comp,
    view: PartView,
    /// bVisible and not bHiddenInGame.
    shown: bool,
}

/// classView's view of one actor.
#[derive(Debug)]
struct ClassView {
    a: ActorRef,
    parts: Vec<Part>,
    /// The no-hold-socket warning given (its text made once).
    warned: bool,
}

/// `classView`: the view of every drawn mesh component of the actor's class `cs`; None with none and no particle
/// component (warned per component the items part does not draw).
pub(crate) fn class_view(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    cs: &ActorComponents,
) -> Option<Box<dyn ActorMesh>> {
    let cls = mvs_sim::actors::world_actor::actor_cls(w, a)?;
    let cls = w.names.str(cls);
    let mut parts = Vec::new();
    for c in &cs.components {
        let skinned = match c.kind {
            ComponentKind::StaticMesh { .. } => false,
            ComponentKind::SkeletalMesh { .. } => true,
            _ => continue,
        };
        let key = format!("{cls}.{}", c.name);
        let view = if skinned {
            SkinnedActor::item(host, &key).map(|v| PartView::Skinned(Box::new(v), key.clone()))
        } else {
            StaticView::item(host, &key, "template").map(PartView::Static)
        };
        let Some(view) = view else {
            warn_once(format!("item mesh not drawn: {key} (not in the items part's draws, or not loaded)"));
            continue;
        };
        let comp = Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale };
        parts.push(Part { comp, view, shown: c.shown });
    }
    if parts.is_empty() && particle_components(w, a).is_empty() {
        return None;
    }
    Some(Box::new(ClassView { a, parts, warned: false }))
}

impl ActorMesh for ClassView {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let age = match self.a {
            ActorRef::Item(id) => w.actors.items.get(id).map(|it| it.age),
            ActorRef::Projectile(id) => w.actors.projectiles.get(id).map(|pr| pr.age),
            _ => None,
        };
        let Some(age) = age else { return };
        // (the holder's hold socket; with none, the holder's place)
        let hold = at.holder.and_then(|h| {
            let socket = hold_socket(w, h);
            if socket == "None" {
                if !self.warned {
                    self.warned = true;
                    let cls = mvs_sim::actors::world_actor::actor_cls(w, self.a).map_or("", |c| w.names.str(c));
                    let id = w.fighter(h).id;
                    warn_once(format!("{cls}: no hold socket on fighter {id}: drawn at the holder's place"));
                }
                return None;
            }
            host.hold_place(w, h, &socket)
        });
        let (x, y) = at.at(alpha);
        for pt in &mut self.parts {
            let on = shown && pt.shown;
            let xf = match &hold {
                Some(s) => place_at(&pt.comp, &pt.comp.rot, pt.comp.rs, &s.rot, at.scale, s.loc),
                None => place_under(&pt.comp, &at.rot, at.scale, x, y),
            };
            match &mut pt.view {
                PartView::Static(v) => {
                    v.visible = on;
                    if on {
                        v.set(&xf);
                        v.draw(out);
                    }
                }
                PartView::Skinned(v, key) => {
                    v.visible = on;
                    if !on {
                        continue;
                    }
                    if let Some((clips, c)) = host.item_clip(key, None) {
                        v.play(clips, c, f64::from(age) * f64::from(mvs_sim::consts::DELTA_SECONDS), true);
                    }
                    v.set(&xf);
                    v.draw(out);
                }
            }
        }
    }

    /// Each mesh view is a scene child of its own.
    fn split(&self) -> bool {
        true
    }
}
