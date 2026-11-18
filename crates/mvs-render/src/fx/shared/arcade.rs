//! The arcade rounds' bot buffs' effects (the classes Arcade Get Round Buff gives a round's
//! bots): a shared class's effects, made for every match after
//! the characters'. Ported from the decode (kismet_pseudo --extra on each class's bytecode).
//!
//! - Buff_Arcade_MidBoss (Giant Gizmo's round): ReceiveBeginPlay @10 -> CreateBuffVisuals @39-319: with a BaseFighter
//!   owner, BuffVfx = SpawnEmitterAttached(PS_ShaggyRageAuraBuff_Loop, the owner's Mesh, Spine_Socket, 0, rot 0,
//!   scale GetActiveMeshVFXScale, KeepRelativeOffset, auto destroy) @205. No Team, no EndPlay of its own.
//! - Buff_AXBotEnemyHardNemesis (a Rivals round on Hard): ReceiveBeginPlay @85 -> Delay(0.5) @15 -> EyesMulticast
//!   @105-606: with a BP_FighterCharacter owner, SpawnEmitterAttached(PS_RageEyes, its Mesh, L_Eye @289 / R_Eye
//!   @492, the socket's world location, rot 0, scale 2.5, KeepWorldPosition, auto destroy). No Team, no EndPlay.
//!
//! Neither class keeps its components to destroy them, nor adds them to the visibility tracking: they are the pawn's
//! mesh's components (the shared path's `attach_to_mesh`), made once a fighter and kept while its pawn is. The sim
//! has no event for either: the buff is seen on the fighter's list, as the projectile block's shield.
// Unverified: the components are taken to outlive the buff actor (SpawnEmitterAttached's component is outered to the
// mesh's owner, the pawn, as 4.26 has it; not read from the exe). The passive buff stays on the pawn through a defeat
// in the build (world/reset.rs reset_passives) where the port gives it again at each respawn: made once a fighter, the
// port draws what the build does.
// Unverified: the Delay is counted as the sim's own (heal_begin's rule: the spawn frame counts), from the step the
// buff is first seen on.
// GetActiveMeshVFXScale is 1 for every fighter of the port.

use indexmap::{IndexMap, IndexSet};
use mvs_sim::World;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::fighter::common::latent_frames;
use mvs_vfx::math::IDENTITY;

use crate::fx::attach::transpose;
use crate::fx::chars::{CharFxHooks, ProjRule};
use crate::fx::{Fx, warn_once};

const AURA: &str = "PS_ShaggyRageAuraBuff_Loop";
const EYES: &str = "PS_RageEyes";
const EYE_SCALE: f64 = 2.5;
const EYE_DELAY: f32 = 0.5;

/// What its effects keep between steps, by the fighter's id.
#[derive(Debug, Default)]
pub(crate) struct Arcade {
    /// The fighters whose Buff_Arcade_MidBoss aura was made.
    auras: IndexSet<i32>,
    /// Buff_AXBotEnemyHardNemesis: the world tick its eyes are due (None once made).
    eyes: IndexMap<i32, Option<i32>>,
    /// Whether the store holds the aura's and the eyes' templates (looked up once: without one, nothing is tried).
    templates: Option<(bool, bool)>,
}

/// Whether the player has `ps`'s template; a warning, once, when it has not.
fn template(fx: &mut Fx, ps: &str) -> bool {
    let ok = fx.player.emitter_count(ps).is_some();
    if !ok {
        warn_once(format!("arcade: no particle system {ps} in the store: not drawn"));
    }
    ok
}

/// Whether fighter `f` has a live buff of class `c`.
fn has(w: &World, f: mvs_sim::fighter::FighterId, c: BuffClass) -> bool {
    w.fighter(f).buffs.buffs.iter().any(|b| b.class == c && b.frames > 0.0)
}

/// EyesMulticast: PS_RageEyes at each eye socket, KeepWorldPosition (rotation 0 and scale 2.5 in the world, taken
/// relative to the socket at the spawn). False when neither could be placed (no pose yet).
fn spawn_eyes(fx: &mut Fx, w: &World, id: i32) -> bool {
    let mut made = false;
    for n in ["L_Eye", "R_Eye"] {
        let socket = fx.skel_name(id, n);
        let probe = fx.socket_follow(w, id, Some(&socket), [0.0; 3], &IDENTITY, [1.0; 3]);
        let Some(a) = fx.place(w, &probe) else { continue };
        let sc = a.scale.unwrap_or([1.0; 3]);
        let r = a.rot;
        let rows = [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2]];
        let rel_scale = [EYE_SCALE / sc[0], EYE_SCALE / sc[1], EYE_SCALE / sc[2]];
        made |= fx.attach_socket(w, id, EYES, &socket, [0.0; 3], &transpose(&rows), rel_scale, &[]).is_some();
    }
    made
}

impl CharFxHooks for Arcade {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        let tick = w.flow.tick;
        let fighters = w.pawns.as_deref().unwrap_or(&w.players);
        if !fighters
            .iter()
            .any(|&f| has(w, f, BuffClass::BuffArcadeMidBoss) || has(w, f, BuffClass::BuffAXBotEnemyHardNemesis))
        {
            return;
        }
        let (aura_ok, eyes_ok) = match self.templates {
            Some(t) => t,
            None => *self.templates.insert((template(fx, AURA), template(fx, EYES))),
        };
        for &f in fighters {
            let id = w.fighter(f).id;
            // (a component that finds no pose yet is made at a later step)
            if aura_ok
                && !self.auras.contains(&id)
                && has(w, f, BuffClass::BuffArcadeMidBoss)
                && fx.attach_socket(w, id, AURA, "Spine_Socket", [0.0; 3], &IDENTITY, [1.0; 3], &[]).is_some()
            {
                self.auras.insert(id);
            }
            if eyes_ok && !self.eyes.contains_key(&id) && has(w, f, BuffClass::BuffAXBotEnemyHardNemesis) {
                // (the delay's last frame, the spawn frame counted)
                self.eyes.insert(id, Some(tick + latent_frames(EYE_DELAY) as i32 - 1));
            }
            if let Some(Some(due)) = self.eyes.get(&id).copied()
                && tick >= due
                && spawn_eyes(fx, w, id)
            {
                self.eyes.insert(id, None);
            }
        }
    }

    /// A Lab reset: the player's systems are cleared, the pawns new.
    fn reset(&mut self, _fx: &mut Fx) {
        *self = Arcade::default();
    }

    fn end(&mut self, _fx: &mut Fx) {
        *self = Arcade::default();
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
