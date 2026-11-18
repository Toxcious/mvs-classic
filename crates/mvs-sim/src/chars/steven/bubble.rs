//! The bubble charge: Steven_BubbleCharge_C ([`BubbleCharge`], a projectile's
//! `ext`). No pawn hook: its graph acts through its own kind alone.
//!
//! Steven_BubbleCharge (offsets of its ubergraph): a ParentProjectilePhysics child that
//! Steven_Attack_Air_BubbleBlast_Montage spawns at 0.258 s, attached to his Root socket 150 up at scale 5 (the Capsule
//! has bAbsoluteScale), direction (0, 1, 0) at speed 0.01, EnableProjectileMovement false, destroyed with the spawner
//! notify's end (DestroyAllActorsOnNotifyEnd, 0.879 s); its AnimNotify_ActivateSpawned (all, UpdateCharge) at 0.800 s
//! activates it. The graph is WonderWoman_BracerClash's with the WW_ names kept, less its HitDuration and
//! HitboxDuration: the world's Projectile is the parent (the attach, the hit of HitBoxDamage, the charge values, the
//! clank as Radial type 3).

use crate::Real;
use crate::State;
use crate::actors::projectile::{ProjectileExt, class_p, hit_collision, set_hit_buff_duration};
use crate::actors::{ActorClass, ProjectileId};
use crate::actors::{Projectile, ProjectileClass};
use crate::combat::DataVec;
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::common::latent_frames;
use crate::fighter::facing;
use crate::math::to_int32;
use crate::math::ue::size2d;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, ProjectileHooks};
use crate::registry::props::num;
use crate::registry::{Registry, RegistryError};
use crate::world::World;

const BLAST_01: Name = name!("sfx_cha_steven_bubbleblast_blast_01");
const BLAST_02: Name = name!("sfx_cha_steven_bubbleblast_blast_02");
const BURST: Name = name!("PS_StevenNANAIR_BubbleBurst");

/// `BubbleCharge extends Projectile` (Steven_BubbleCharge_C): what the class adds.
#[derive(State, Debug)]
pub struct BubbleCharge {
    /// GrowthScale (uniform): the world scale of HitBox, StaticMesh and HitBox2D, which ReceiveTick sets to it. It
    /// starts at the root's scale (the notify's Scale3D 5; StaticMesh has no relative scale of its own).
    pub growth: Real,
    /// HitBox2D's CapsuleRadius (HitBox's SphereRadius, the same 37) at scale 1.
    pub unit: Real,
    /// The age at which ActivateObject's Delay(0.02) resumes (the parent's own), -1 for none.
    pub on_age: i32,
}

impl Default for BubbleCharge {
    /// The field initial values.
    fn default() -> Self {
        BubbleCharge { growth: 1.0, unit: 0.0, on_age: -1 }
    }
}

fn pr_ref(w: &World, id: ProjectileId) -> &Projectile {
    w.actors.projectiles.get(id).expect("a live projectile")
}

fn pr_mut(w: &mut World, id: ProjectileId) -> &mut Projectile {
    w.actors.projectiles.get_mut(id).expect("a live projectile")
}

fn charge(w: &World, id: ProjectileId) -> &BubbleCharge {
    match &pr_ref(w, id).ext {
        ProjectileExt::BubbleCharge(s) => s,
        _ => panic!("a bubble charge"),
    }
}

fn charge_mut(w: &mut World, id: ProjectileId) -> &mut BubbleCharge {
    match &mut pr_mut(w, id).ext {
        ProjectileExt::BubbleCharge(s) => s,
        _ => panic!("a bubble charge"),
    }
}

fn construct(_w: &World, p: &mut Projectile) {
    p.ext = ProjectileExt::BubbleCharge(BubbleCharge::default());
}

/// ReceiveBeginPlay @1059: SpawnedActorParent's BeginPlay (the parent's), then the owner's BPI_IsFacingLeft (false for
/// an owner that is no BPI_BaseFighter @1498): HitBoxDamage.KnockBackDirectionVector.X x -1 when it faces left
/// @1168-1413. Then, with authority, SetVisuals(false) @700 (the instigator's material and the glow: the view's) and
/// NewTime = GetGameTimeInSeconds @715 (read by nothing of its own).
// Unverified (hit side): the world turns the vector once more by the actor's yaw (`projectile_hit_dir`: yawX, the
// facing for an attached actor with AimDownProjectileMovement), then pfgDoDamage's KnockBackMirror (true in this
// HitBoxDamage; the handler facing right) sends the victim away from the bubble at both facings; the attached
// actor's world yaw is the port's rule, not a read.
fn begin(w: &mut World, id: ProjectileId) {
    let left = pr_ref(w, id).owner.is_some_and(|f| facing(w, f) < 0.0);
    let p = pr_mut(w, id);
    let d = p.hit.knock_back_direction_vector.unwrap_or(DataVec { x: Some(0.0), y: Some(0.0), z: None });
    // (a new vector of X and Y alone: a spread replaces the member)
    p.hit.knock_back_direction_vector = Some(DataVec { x: if left { d.x.map(|x| -x) } else { d.x }, y: d.y, z: None });
    let (scale, radius) = (p.scale, p.hit_radius);
    let c = charge_mut(w, id);
    c.growth = scale;
    c.unit = if scale != 0.0 { radius / scale } else { 0.0 };
}

/// ReceiveTick @1514 (the parent's first), while not Activated @1533: GrowthScale = StaticMesh's world scale x (1 +
/// DeltaSeconds x GrowthValue 1.05) @1572-1756, and that scale (computed again the same way) is set as the world
/// scale of HitBox, StaticMesh and HitBox2D @1967-2049: multiplicative, no clamp. The team mates inside the hit box
/// get the Buff_ArmorPack row (`clash_allies`, the same graph as WonderWoman_BracerClash's).
// Unverified (as WonderWoman_BracerClash): the actor's delta is taken as the world's.
fn step(w: &mut World, id: ProjectileId) -> bool {
    if !pr_ref(w, id).activated {
        let growth_value = num(class_p(w, pr_ref(w, id)), "GrowthValue").unwrap_or(0.0);
        let c = charge_mut(w, id);
        c.growth *= 1.0 + (DELTA_SECONDS * growth_value);
        let (unit, growth) = (c.unit, c.growth);
        pr_mut(w, id).hit_radius = unit * growth;
    } else if pr_ref(w, id).age == charge(w, id).on_age {
        // @15, behind ActivateObject's Delay(0.02) @2136: every OnHitBuffs row of class Buff_ArmorPack gets
        // BuffDuration = GrowthScale.Z x 0.2 @281-426, then HitBoxOn? = true and UpdateHitbox @482-493 (the parent's
        // hitOn at the same age). No own off: the hit box stays on until the actor ends.
        let growth = charge(w, id).growth;
        set_hit_buff_duration(w, id, "Buff_ArmorPack_C", growth * 0.2);
        // UpdateHitbox -> SpawnedActorParent @15092 -> @8122 -> HitBoxesOn(HitBoxTags, ..., HitBoxScale (1, 1, 1))
        // @8531 -> Comp_Pawn_HitBoxHandler ActivateHitbox @16855 SetRelativeScale3D(HitBoxScale): the growth's world
        // scale on the hit box gives way to the Capsule's (the spawn scale 5) x 1, so it deals at 37 x 5 = 185
        // whatever the charge; the growth drives the mesh, the armor duration, the blast sound and the burst's scale
        // alone.
        let unit = charge(w, id).unit;
        let p = pr_mut(w, id);
        p.hit_radius = unit * p.scale;
    }
    if hit_collision(pr_ref(w, id)) {
        crate::chars::wonder_woman::actors::clash_allies(w, id);
    }
    false
}

/// ActivateObject @2191: SpawnedActorParent's (an empty event @8831), then with authority @2091: Activated = true,
/// SetVisuals(true), EarlyTrigger(0.8) (the glow timeline's time: the view's) and Delay(0.02) (the step above).
/// SetVisuals(true) @2236 on a BaseFighter Instigator: the material swap and PS_StevenNANAIR_BubbleBurst attached to
/// the instigator's mesh at fml_un_C_spine_c at GrowthScale x 0.1 @2814 (team parameter: the view's), then, Slam true,
/// FX_Blast @3280: sfx_cha_steven_bubbleblast_blast_01 at the actor's location while VSizeXY(GrowthScale) < 12, else
/// _02 (FX_Blast @0-402), and Activated = true again @3294.
/// The burst goes out as the actor's emitter event at its own place (the spine socket is the view's to find).
fn activate(w: &mut World, id: ProjectileId) {
    if charge(w, id).on_age >= 0 {
        return;
    }
    let p = pr_ref(w, id);
    let (age, cls, px, py, owner) = (p.age, p.cls, p.x, p.y, p.owner);
    let growth = charge(w, id).growth;
    charge_mut(w, id).on_age = age + to_int32(latent_frames(0.02));
    // (the owner's id: a projectile of no owner posts -1, as the pooled post)
    let fighter = owner.map_or(-1, |f| w.fighter(f).id);
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter,
        vfx: Some(BURST),
        x: px,
        y: py,
        ox: 0.0,
        rot: [0.0; 3],
        scale: (growth * 0.1),
        tint: None,
        to: None,
    });
    let ev = if size2d([growth, growth]) < 12.0 { BLAST_01 } else { BLAST_02 };
    w.events.push(Event::Sound { fighter, ev, x: Some(px), y: Some(py), hit: None, pooled: false, key: None });
}

/// This file's registry entries (Steven_BubbleCharge_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let kind = ProjectileHooks {
        construct: Some(construct),
        begin: Some(begin),
        step: Some(step),
        activate: Some(activate),
        ..ProjectileHooks::default()
    };
    r.actors.enter(
        "Steven_BubbleCharge_C",
        ActorEntry::new(ActorClass::Projectile(ProjectileClass::StevenBubbleCharge), ActorHooks::Projectile(kind)),
    )
}
