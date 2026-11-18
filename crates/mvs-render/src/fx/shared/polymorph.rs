//! Buff_PolymorphParent's effects: a shared class's effects, made for every
//! match after the characters': the change emitter (the polymorphFx events) and the particle notifies of the
//! Polymorph mesh's montages, on that mesh (mvs_view's second view; fx/second.rs).

use mvs_sim::World;
use mvs_sim::events::Event;
use mvs_vfx::math::IDENTITY;
use mvs_view::SecondKind;

use crate::fx::Fx;
use crate::fx::chars::{CharFxHooks, ProjRule};

/// What its effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Polymorph {
    warned: bool,
}

// Deviation: AnimNotify_PlayVFX on the mesh (`poly` notifyVfx) is not drawn: none of the montages the buff plays has
// one.

impl CharFxHooks for Polymorph {
    fn event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        match e {
            Event::PolymorphFx { fighter, vfx, .. } => {
                change(fx, w, *fighter, w.names.str(*vfx));
                true
            }
            Event::ParticleNotify { fighter, n, on, left, poly: true, .. } => {
                fx.second_particle(w, SecondKind::Polymorph, *fighter, *n, *on, *left);
                true
            }
            Event::NotifyVfx { poly: true, on, .. } => {
                if *on && !self.warned {
                    self.warned = true;
                    eprintln!("warning: Polymorph: AnimNotify_PlayVFX on the mesh is not drawn");
                }
                true
            }
            _ => false,
        }
    }
}

/// PolymorphChangeVFX (PS_ShadowClone_Impact) at the start (PP@7572-8093) and in the end burst (PP@2473-3412) on the
/// victim's pawn Mesh, socket VFX_Root, (0, 0, 80), SnapToTarget (world scale 1: the socket's world scale taken out),
/// Team = the victim's.
fn change(fx: &mut Fx, w: &World, id: i32, ps: &str) {
    if ps.is_empty() || ps == "None" {
        return;
    }
    let f0 = fx.socket_follow(w, id, Some("VFX_Root"), [0.0; 3], &IDENTITY, [1.0; 3]);
    let sc = fx.place(w, &f0).and_then(|a| a.scale).unwrap_or([1.0; 3]);
    let inv = sc.map(|v| 1.0 / v);
    let f = fx.socket_follow(w, id, Some("VFX_Root"), [0.0, 0.0, 80.0], &IDENTITY, inv);
    let team = fx.team(id);
    fx.attach_to_mesh(w, id, ps, "VFX_Root", f, &[("Team", team)]);
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
