//! The PreMatch level's Emitter actors that its level sequences drive:
//! PS_MainMenuLensFlare_2 (Seq_AnimInRed activates it at 1.1667 s of the face-off; Seq_Clash / Seq_Clash_FFA_New scale
//! it 0.5 -> 0 over 0.333 s), PS_MainMenuGlowOrb_2 (Seq_Clash activates it at 1.0833 s: the clash's burst) and
//! PS_MainMenuGlowOrb_FFA_Bottom / _Top (Seq_Clash_FFA_New, the same key).
//!
//! Each is a ParticleSystemComponent root with bAutoActivate false, so nothing shows until a Particle key.

use indexmap::IndexMap;
use mvs_vfx::{Attachment, Player, SystemId};
use mvs_view::math::rotator_quat;

use crate::menuscene::format::{LevelEmitter, V3};

/// A vector turned by the unit quaternion (x, y, z, w).
fn qrot([qx, qy, qz, qw]: [f64; 4], [vx, vy, vz]: V3) -> V3 {
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [vx + qw * tx + qy * tz - qz * ty, vy + qw * ty + qz * tx - qx * tz, vz + qw * tz + qx * ty - qy * tx]
}

#[derive(Debug)]
struct Emitter {
    asset: LevelEmitter,
    /// The component's world place now (a 3DTransform track writes the location and scale).
    at: Attachment,
    handle: Option<SystemId>,
}

impl Emitter {
    fn alive(&self, vfx: &Player) -> bool {
        self.handle.is_some_and(|h| vfx.alive(h) && !vfx.deactivated(h))
    }
}

/// The Emitter actors of the level.
///
/// A Particle key (FMovieSceneParticleSectionTemplate): Activate is SetActive(true, bReset) on a component that is not
/// active (an active one is left alone), Deactivate is SetActive(false) (DeactivateSystem: no more spawning, the
/// particles alive finish), Trigger is ActivateSystem(true). An activation (re)starts the system: its particles of
/// before go (ResetParticles) and it spawns anew.
// Unverified: from the UE 4.26 source, not a native read of this build. The system first ticks in the frame of the key
// (the level ticks before the scene's effects); the build's order of the sequence player's tick and the PSC manager's
// tick group is not read.
#[derive(Debug, Default)]
pub struct LevelEmitters {
    list: IndexMap<String, Emitter>,
}

impl LevelEmitters {
    pub fn new(assets: &IndexMap<String, LevelEmitter>) -> LevelEmitters {
        let list = assets
            .iter()
            .map(|(name, a)| {
                let q = rotator_quat(a.rot[0], a.rot[1], a.rot[2]);
                let rot = [qrot(q, [1.0, 0.0, 0.0]), qrot(q, [0.0, 1.0, 0.0]), qrot(q, [0.0, 0.0, 1.0])];
                (
                    name.clone(),
                    Emitter {
                        asset: a.clone(),
                        at: Attachment { loc: a.loc, rot, scale: Some(a.scale) },
                        handle: None,
                    },
                )
            })
            .collect();
        LevelEmitters { list }
    }

    /// A binding `PreMatch:<actor>` of a sequence track; None for an actor the level lacks.
    fn of(&mut self, binding: &str) -> Option<&mut Emitter> {
        let (level, actor) = binding.split_once(':')?;
        if level == "PreMatch" { self.list.get_mut(actor) } else { None }
    }

    /// A Particle key on the actor's component.
    pub fn key(&mut self, vfx: Option<&mut Player>, binding: &str, key: &str) {
        let Some(e) = self.of(binding) else { return };
        let active = vfx.as_deref().is_some_and(|v| e.alive(v));
        if key == "Deactivate" {
            if let (true, Some(v), Some(h)) = (active, vfx, e.handle) {
                v.deactivate(h);
            }
            return;
        }
        if key == "Activate" && active {
            return;
        }
        let Some(v) = vfx else { return };
        if let Some(h) = e.handle.take() {
            v.kill(h);
        }
        e.handle = v.spawn_level(&e.asset.system, &e.at, false, &[], 0, false);
        if let (Some(h), false) = (e.handle, e.asset.visible) {
            v.set_visible(h, false);
        }
    }

    /// A 3DTransform channel, 0..2 location X / Y / Z, 6..8 scale X / Y / Z (the build refuses
    /// rotation channels).
    pub fn set_channel(&mut self, binding: &str, channel: usize, v: f64) {
        let Some(e) = self.of(binding) else { return };
        if channel < 3 {
            e.at.loc[channel] = v;
        } else if channel >= 6
            && let Some(s) = e.at.scale.as_mut()
            && let Some(x) = s.get_mut(channel - 6)
        {
            *x = v;
        }
    }

    /// Dispose: each component destroyed, its particles gone at once.
    pub fn dispose(&mut self, vfx: Option<&mut Player>) {
        let Some(v) = vfx else { return };
        for e in self.list.values_mut() {
            if let Some(h) = e.handle.take() {
                v.kill(h);
            }
        }
    }

    /// The live systems follow their component (`follow`).
    pub fn follow(&self, vfx: &mut Player) {
        for e in self.list.values() {
            if let Some(h) = e.handle {
                vfx.attach(h, &e.at);
            }
        }
    }
}
