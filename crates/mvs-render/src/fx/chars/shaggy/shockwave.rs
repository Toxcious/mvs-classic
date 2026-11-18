//! ShaggyShockwaveActor on the render side (a shared class's module made
//! after the characters'): its stages' emitters, slowed through their hitpause, and its decal.

use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::events::Event;
use mvs_sim::fighter::common::{actor_def, latent_frames};
use mvs_sim::names::Name;
use mvs_vfx::SystemId;
use mvs_vfx::math::V3;

use crate::fx::attach::{num, text, xyz};
use crate::fx::chars::{CharFxHooks, ProjRule};
use crate::fx::{FloorDecal, Fx};

/// The shockwave classes drawn here: the parent, and in [`ENTERED`] the data children
/// their characters enter (LeBron's C016_DunkShockwave_C), each drawn from its own class's data on its owner.
const SHOCKWAVES: &[&str] = &["ShaggyShockwaveActor_C"];
/// The characters' lists of shockwave classes (their modules' `shockwaveViews` calls).
const ENTERED: &[&[&str]] = &[crate::fx::chars::lebron::SHOCKWAVES];

/// Whether `cls` is drawn here.
fn entered(cls: &str) -> bool {
    SHOCKWAVES.contains(&cls) || ENTERED.iter().any(|l| l.contains(&cls))
}

/// `warnOnce` of a shockwave class nobody entered: it shows nothing.
fn unknown(cls: &str) {
    static W: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut w = W.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !w.iter().any(|c| c == cls) {
        eprintln!("warning: {cls}: a shockwave class without a view (fx/chars/shaggy/shockwave.rs SHOCKWAVES)");
        w.push(cls.to_owned());
    }
}

/// Its classes' own effects (none: the actor is a World actor, not a projectile).
pub(crate) const RULES: &[(&str, ProjRule)] = &[];

/// ShaggyShockwaveActor: the stages' emitters, slowed through their hitpause, and the decal.
#[derive(Debug, Default)]
pub(crate) struct Shockwave {
    /// Shockwave emitters slowed by their hitpause, and the latent frames left.
    slowed: Vec<(SystemId, f64)>,
}

impl CharFxHooks for Shockwave {
    fn event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        let Event::ShockwaveVfx { fighter, cls, n, x, y, facing, hit_pause } = *e else { return false };
        self.shockwave_vfx(
            fx,
            w,
            fighter,
            cls,
            n,
            [f64::from(x), f64::from(y), f64::from(facing)],
            f64::from(hit_pause),
        );
        true
    }

    fn after_tick(&mut self, fx: &mut Fx, _w: &World) {
        self.tick_slowed(fx);
    }

    fn frame(&mut self, fx: &mut Fx, w: &World, _alpha: f64) {
        place_shockwave_decals(fx, w);
    }

    fn reset(&mut self, _fx: &mut Fx) {
        self.slowed.clear();
    }

    fn end(&mut self, _fx: &mut Fx) {
        self.slowed.clear();
    }
}

impl Shockwave {
    /// Playing the shockwave's VFX: ShaggyShockwaveActor Spawn Shockwave Visuals, SpawnEmitterAtLocation(Particle[n],
    /// ImpactPoint, yaw -90 facing right / +90 left, scale 1), Team. With a hitpause the emitter's CustomTimeDilation
    /// is 0.005 until an Actor_HitPauseContainer's Delay(HitPauseTime) sets it back to 1.
    /// `at`: the impact point's x, y and the actor's facing.
    #[allow(clippy::too_many_arguments)]
    fn shockwave_vfx(&mut self, fx: &mut Fx, w: &World, fighter: i32, cls: Name, n: i32, at: [f64; 3], hit_pause: f64) {
        if !entered(w.names.str(cls)) {
            return; // (warned by the decal pass)
        }
        let Some(owner) = w.by_id(fighter) else { return };
        let Some(def) = actor_def(w, ActorRef::Fighter(owner), cls) else { return };
        let Some(sys) = text(&def.p, &format!("Particle{n}")).map(str::to_string) else { return };
        let facing = at[2];
        let team = fx.team(fighter);
        let yaw = if facing > 0.0 { -90.0 } else { 90.0 };
        let Some(h) = fx.player.spawn(&sys, [0.0, -at[0], at[1]], [0.0, yaw, 0.0], [1.0; 3], &[("Team", team)]) else {
            return;
        };
        if hit_pause > 0.0 {
            fx.player.set_time_dilation(h, 0.005);
            self.slowed.push((h, f64::from(latent_frames(mvs_sim::math::narrow(hit_pause)))));
        }
    }

    /// The slowed VFX's tick (after the step's ticks).
    fn tick_slowed(&mut self, fx: &mut Fx) {
        let mut i = self.slowed.len();
        while i > 0 {
            i -= 1;
            self.slowed[i].1 -= 1.0;
            if self.slowed[i].1 > 0.0 {
                continue;
            }
            let (h, _) = self.slowed.remove(i);
            fx.player.set_time_dilation(h, 1.0);
        }
    }
}

/// Placing the shockwaves and making the decal: the Decal component (RelativeLocation, RelativeRotation (90,
/// 26.565, -63.435) in the actor's frame: local X the world +Z) at the actor's size scale, DecalMIC Alpha the
/// actor's timeline, Team as the emitters'.
fn place_shockwave_decals(fx: &mut Fx, w: &World) {
    for (aid, a) in w.actors.actors.iter() {
        let WorldActorState::Shockwave(sw) = &a.state else { continue };
        if a.dead {
            continue;
        }
        let cls = w.names.str(a.cls);
        if !entered(cls) {
            unknown(cls);
            continue;
        }
        let Some(dc) =
            actor_def(w, ActorRef::Actor(aid), a.cls).and_then(|d| d.components.get("Decal")).and_then(Value::as_obj)
        else {
            continue;
        };
        let Some(material) = text(dc, "DecalMaterial") else { continue };
        let rl = xyz(dc.get("RelativeLocation"), 0.0);
        let sc = xyz(dc.get("RelativeScale3D"), 1.0);
        let sz = decal_size(dc);
        let (f, k) = (sw.facing, a.size_scale.unwrap_or(1.0));
        let team = fx.team(a.owner_id);
        fx.decals.push(FloorDecal {
            actor: a.seq,
            material: material.to_string(),
            loc: [
                rl[1] * f64::from(f) * f64::from(k),
                -(f64::from(sw.x) + rl[0] * f64::from(f) * f64::from(k)),
                f64::from(sw.y) + rl[2] * f64::from(k),
            ],
            rows: [0.0, 0.0, 1.0, 0.0, f64::from(f), 0.0, f64::from(-f), 0.0, 0.0],
            extent: [sz[0] * sc[0] * f64::from(k), sz[1] * sc[1] * f64::from(k), sz[2] * sc[2] * f64::from(k)],
            team: Some(team),
            alpha: Some(f64::from(sw.decal_alpha)),
            dissolve: None,
        });
    }
}

/// DecalSize, UE's default (128, 256, 256) where unset.
fn decal_size(dc: &Props) -> V3 {
    let o = dc.get("DecalSize").and_then(Value::as_obj);
    let g = |k: &str, d: f64| o.and_then(|o| num(o, k)).unwrap_or(d);
    [g("X", 128.0), g("Y", 256.0), g("Z", 256.0)]
}
