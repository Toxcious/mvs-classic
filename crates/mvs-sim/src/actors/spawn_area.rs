//! SpawnObjectRandomArea, the pawn component that spawns an actor at a random place of an area:
//! at the pawn or on what a box trace finds under a random place beside it. Arya's pawn calls it (AryaKillPie).
//!
//! SpawnObject -> ubergraph @1343: the owner HasAuthority (@1363-1405: holds offline) and is a BP_Fighter_Character
//! (@1419-1504).
//! AtPawnLocation (@10): the transform is the pawn's GetTransform location + (0, 0, 200), rotation zero, the pawn's
//! scale (@24-221), and the pawn's Comp_Spawner gets CompSpawnActor(SpawnedActor, AttachToActor false, no actor,
//! socket None, WorldTransforms true, that transform, direction (0, 0, 1), InitialSpeed 0, MaxSpeed 0, no charge,
//! TotalDuration 0, AttackID 0, no notify, no owner, false, 0, false) @535.
//! Otherwise (@658-1324): BoxTraceSingle from (X, Y + RandomFloatInRange(MinYAway, MaxYAway), Z + ZAway) straight
//! down 3,600,000 with Half Size; no hit, nothing; a hit, the same call at the hit's Location alone. Not ported: the
//! trace's channel and the random stream are not decoded for it and no ported kit takes that branch, so it is refused
//! with a warning.
//! ActivateObject is empty (@1523).
//! The request carries literal properties, as a graph's other calls of the owner's spawner do (`SpawnRequest::props`),
//! read by the spawned class's own entry: Transforms.Translation is the world place in UE axes (world Y is sim -x, Z
//! the actor's height; the sim has no X: 0), with WorldTransforms so that no facing mirror and no socket applies. The
//! pawn's actor scale is 1 (the scale is the mesh's). AttackID 0 is replaced by the owner's attack count at the spawn
//! (Comp_Spawner HandleActorPostSpawn @2444-2747).

use std::sync::Arc;

use mvs_data::value::{Props, Value};

use crate::Real;
use crate::combat::elem::instigator_charge;
use crate::events::NotifyRef;
use crate::fighter::common::{data_of, warn_text};
use crate::fighter::match_side::{Foreign, SpawnRequest};
use crate::fighter::{FighterId, actor_y};
use crate::names::Name;
use crate::registry::props::{flag, name_of};
use crate::world::World;

const COMPONENT: &str = "Comp_Pawn_SpawnObjectRandomArea_GEN_VARIABLE";

fn vec3(a: [(&str, Real); 3]) -> Value {
    Value::Obj(a.into_iter().map(|(k, v)| (k.to_owned(), Value::Num(f64::from(v)))).collect())
}

/// The request to the owner's spawner; true when it was made.
pub fn spawn_object(w: &mut World, f: FighterId) -> bool {
    let key = w.names.str(w.fighter(f).ch).to_owned();
    let Some(comp) =
        data_of(w, f).fighter.components.pawn.get(COMPONENT).map(|c| c.p.as_obj().cloned().unwrap_or_default())
    else {
        warn_text(w, &format!("SpawnObjectRandomArea: the pawn has no such component ({key})"));
        return false;
    };
    if !flag(&comp, "AtPawnLocation") {
        warn_text(w, &format!("SpawnObjectRandomArea: the box trace branch is not ported ({key})"));
        return false;
    }
    // CompSpawnActor with no class spawns nothing
    let Some(spawned) = comp.get("SpawnedActor").filter(|v| v.as_str().is_some_and(|s| !s.is_empty())) else {
        return false;
    };
    let Some(cls) = name_of(w, &comp, "SpawnedActor") else { return false };
    let mut tr = Props::new();
    tr.insert("Translation".into(), vec3([("X", 0.0), ("Y", -w.fighter(f).body.x), ("Z", actor_y(w, f) + 200.0)]));
    tr.insert("Rotation".into(), vec3([("Pitch", 0.0), ("Yaw", 0.0), ("Roll", 0.0)]));
    tr.insert("Scale3D".into(), vec3([("X", 1.0), ("Y", 1.0), ("Z", 1.0)]));
    let mut p = Props::new();
    p.insert("ActorClass".into(), spawned.clone());
    p.insert("AttachToActor".into(), Value::Bool(false));
    p.insert("Socket".into(), Value::Str("None".into()));
    p.insert("WorldTransforms".into(), Value::Bool(true));
    p.insert("Transforms".into(), Value::Obj(tr));
    p.insert("InitialMovementDirection".into(), vec3([("X", 0.0), ("Y", 0.0), ("Z", 1.0)]));
    p.insert("InitialSpeed".into(), Value::Num(0.0));
    p.insert("MaxSpeed".into(), Value::Num(0.0));
    let r = SpawnRequest {
        cls,
        n: NotifyRef { montage: Name::NONE, index: 0 },
        attack_id: w.fighter(f).decay.attack_count,
        foreign: Foreign::None,
        copy: None,
        charge: instigator_charge(w, f),
        aim: None,
        props: Some(Arc::new(p)),
        instigator: None,
        origin: None,
    };
    w.fighter_mut(f).requests.spawn_requests.push(r);
    true
}
