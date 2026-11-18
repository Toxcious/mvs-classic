//! The portal: C019_Accelerator, an `actors::lists::Accelerator` of the World's typed
//! lists (its Box is a projectile barrier's, `actors::barrier`). actors.rs's acceleratorTick steps it ([`step`]).
//!
//! ProjectileAccelerator_C (the portal Morty's air side special leaves when ability 4 is ready; a SpawnedActorParent).
//! Sim space: x = -UE Y, y = UE Z. AnimNotify_ConditionalSpawner (Ability4Ready) with socket None and no
//! translation: the owner's location - (0, 0, 100), after the teleport. TriggerAbilityOnSpawnIndex with index 4
//! starts the ability's cooldown (35 s). It does not move. Its Box overlaps the ProjectileDestructible and
//! ItemDestructible channels only; a projectile of the portal's team inside it gets Buff_SpeedUpProjectile (speed x 2).
//! The end: MaxLifetime 10 s (the parent's timer) -> OnMaxLifetimeExpired -> Delay(DestroyActor_Delay 0.5) ->
//! DestroySpawnedActor. Die With Instigator.
// Deviation: a projectile overlaps by its HurtBox sphere against the box in the play plane, tested in the actor's tick
// after the projectiles' moves; other spawned actors and items are not tested; a second overlap of a projectile that
// still has the buff replays the visuals only (taken as a stack added to the live buff), and the buff is not removed
// at the projectile's hits; the fighter branch cannot fire: the box overlaps only ProjectileDestructible and
// ItemDestructible and no fighter body has those object types; the Delay is taken to count from the frame after the
// lifetime timer fires.

use crate::actors::AcceleratorId;
use crate::actors::ActorRef;
use crate::actors::lists::Accelerator;
use crate::fighter::common::{actor_data, latent_frames};
use crate::fighter::match_side::{SpawnRequest, notify_props};
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::props::{num, obj};
use crate::world::World;
use crate::world::blast::new_blast_bounds;
use indexmap::IndexSet;

const CLASS: Name = name!("ProjectileAccelerator_C");

/// The tick the MaxLifetime timer and the destroy Delay end the portal at (0: no lifetime). The timer fires at the
/// first timer tick with counted deltas > its rate (the spawn frame is not counted), as Projectile's; then the Delay.
fn end_tick_of(life: crate::Real, delay: crate::Real) -> i32 {
    let mut k: i32 = 0;
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "!(a > b): NaN keeps counting")]
    while !(crate::consts::clock(crate::real(k)) > f64::from(life)) {
        k += 1;
    }
    if life > 0.0 { k + 1 + to_int32(latent_frames(delay)) } else { 0 }
}

/// `new Accelerator(f, a, r.p)`.
pub fn new_accelerator(w: &mut World, f: FighterId, r: &SpawnRequest) -> Accelerator {
    let n = notify_props(w, f, r.n);
    let t = obj(n, "Transforms").and_then(|t| obj(t, "Translation"));
    let (ty, tz) = (t.and_then(|t| num(t, "Y")).unwrap_or(0.0), t.and_then(|t| num(t, "Z")).unwrap_or(0.0));
    let face = facing(w, f);
    let x = w.fighter(f).body.x + ty * face;
    let y = actor_y(w, f) - 100.0 + tz;
    let data = w.data.clone();
    let p = actor_data(w, &data, ActorRef::Fighter(f), CLASS).map(|d| &d.p);
    let life = p.and_then(|p| num(p, "MaxLifetime")).unwrap_or(0.0);
    let delay = p.and_then(|p| num(p, "DestroyActor_Delay")).unwrap_or(0.0);
    Accelerator {
        cls: CLASS,
        seq: -1,
        x,
        y,
        facing: face,
        age: 0,
        dead: false,
        blast_at: f64::from(0.1_f32),
        blast_bounds: new_blast_bounds(),
        inside: IndexSet::new(),
        doom: None,
        end_tick: end_tick_of(life, delay),
        owner: f,
    }
}

/// `Accelerator.step()`: true when its life is over.
pub fn step(w: &mut World, ac: AcceleratorId) -> bool {
    let Some(a) = w.actors.accelerators.get_mut(ac) else { return false };
    a.age += 1;
    a.end_tick > 0 && a.age >= a.end_tick
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_lifetime_never_ends() {
        assert_eq!(end_tick_of(0.0, 0.5), 0);
    }

    #[test]
    fn lifetime_counts_the_timer_then_the_delay() {
        // 10 s: the first count k with k x DeltaSeconds (a float) > 10 is 600; one tick later the timer fires, then the
        // 30 tick Delay (endTick 631).
        let e = end_tick_of(10.0, 0.5);
        assert_eq!(e, 631);
    }
}
