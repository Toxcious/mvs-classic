//! The shared FireWallHazard's drawing: its Decal component on the floor under
//! the wall, and FireStormHazard_C's particle growth. Its ParticleSystem component (PS_Creature_FireWall, Team from
//! BeginPlay @774-938) is the shared path's; PS_Impact is bVisible false with no template.
//!
//! The Decal: CreatureFireGround_Decal_MAT (DBM_Emissive), DecalSize (10, 256, 256) (the box's half extents),
//! RelativeLocation Z -130 and RelativeRotation Pitch 90 under the root, so it projects along the world Z through a
//! box 20 cm deep whose top is the capsule's bottom: the wall resting on a floor draws the decal on it, a falling one
//! draws nothing. BeginPlay @996-1225 sets the Team of its dynamic material as the particles'. With the actor's
//! rotation (yaw -90 facing right, +90 left) the pitch makes the decal's local X the world +Z, its local Y the actor's
//! +Y and its local Z the actor's -X.

use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::Projectile;
use mvs_sim::consts::DELTA_SECONDS;
use mvs_sim::fighter::common::{actor_def, latent_frames};
use mvs_sim::registry::props::num;
use mvs_vfx::math::V3;

use crate::fx::attach::{text, xyz};
use crate::fx::chars::{CharFxHooks, ProjRule};
use crate::fx::{FloorDecal, Fx};

const WALL: &str = "FireWallHazard_C";
const STORM: &str = "FireStormHazard_C";
pub(crate) const DECAL: &str = "CreatureFireGround_Decal_MAT";

/// The firewall classes' decals (placed each drawn frame).
#[derive(Debug, Default)]
pub(crate) struct Firewall;

impl CharFxHooks for Firewall {
    fn frame(&mut self, fx: &mut Fx, w: &World, _alpha: f64) {
        place_decals(fx, w);
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] =
    &[(WALL, ProjRule::NONE), (STORM, ProjRule { scale: Some(storm_scale), ..ProjRule::NONE })];

/// FireStormHazard_C's BeginPlay growth @154-224 -> @15-112: Delay(0.25), then VFX Growth Timeline (0.25 s, linear
/// 0 -> 1) sets the ParticleSystem's relative scale to VLerp(1.25, 1.875, alpha).
// Unverified: the timeline's first update is taken at the Delay's frame with alpha 0 (the drawn scale is the
// component's 1.25 until then); a hitpause's dilation of the actor is not followed.
fn storm_scale(_w: &World, pr: &Projectile, ms: V3) -> Option<V3> {
    let t = f64::from(pr.age) - f64::from(latent_frames(0.25));
    if t < 0.0 {
        return None;
    }
    let a = (t * f64::from(DELTA_SECONDS) / 0.25).min(1.0);
    let k = 1.25 + (1.875 - 1.25) * a;
    Some(ms.map(|v| v * k))
}

/// The Decal component centred at the root plus its relative location through
/// the actor's rotation, DecalSize x RelativeScale3D x the root scale as the half extents, Team the owner's.
// Unverified: placed where the wall is at each drawn frame.
fn place_decals(fx: &mut Fx, w: &World) {
    for (id, pr) in w.actors.projectiles.iter() {
        // (an ownerless wall, a tutorial's, is fx/shared/tutorial.rs's)
        if pr.dead || pr.owner.is_none() {
            continue;
        }
        let cls = w.names.str(pr.cls);
        if cls != WALL && cls != STORM {
            continue;
        }
        // (a peer's data after a reflect)
        let Some(dc) = actor_def(w, ActorRef::Projectile(id), pr.cls)
            .and_then(|d| d.components.get("Decal"))
            .and_then(Value::as_obj)
        else {
            continue;
        };
        let f = if pr.facing < 0.0 { -1.0 } else { 1.0 };
        let rl = xyz(dc.get("RelativeLocation"), 0.0);
        let s = pr.scale;
        let sz = decal_size(dc);
        let sc = xyz(dc.get("RelativeScale3D"), 1.0);
        let team = fx.team(pr.owner_id);
        fx.decals.push(FloorDecal {
            actor: pr.seq,
            material: text(dc, "DecalMaterial").unwrap_or(DECAL).to_string(),
            loc: [
                f64::from(s) * rl[1] * f,
                -(f64::from(pr.x) + f64::from(s) * rl[0] * f),
                f64::from(pr.y) + f64::from(s) * rl[2],
            ],
            rows: [0.0, 0.0, 1.0, f, 0.0, 0.0, 0.0, f, 0.0],
            extent: [0, 1, 2].map(|k| f64::from(s) * sz[k] * sc[k]),
            team: Some(team),
            alpha: None,
            dissolve: None,
        });
    }
}

/// DecalSize, UE's default (128, 256, 256) where unset.
pub(crate) fn decal_size(dc: &Props) -> V3 {
    let o = dc.get("DecalSize").and_then(Value::as_obj);
    let g = |k: &str, d: f64| o.and_then(|o| num(o, k)).unwrap_or(mvs_sim::math::narrow(d));
    [f64::from(g("X", 128.0)), f64::from(g("Y", 256.0)), f64::from(g("Z", 256.0))]
}
