//! Buff_Disguise's effects: a shared class's effects, made for every match after
//! the characters': the poof of the activation and the end (the disguiseFx events), and the particle and PlayVFX
//! notifies of the copied montages, on the second mesh (mvs_view's second view; fx/second.rs).

use mvs_sim::World;
use mvs_sim::events::Event;
use mvs_sim::fighter::actor_y;
use mvs_view::SecondKind;

use crate::fx::Fx;
use crate::fx::chars::{CharFxHooks, ProjRule};

/// What its effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Disguise;

const POOF: &str = "PS_Arya_Poof";

impl CharFxHooks for Disguise {
    fn event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        match e {
            // ActivateDisguiseMulticast U@23521-23707 and ReceiveEndPlay U@13965-14376 with UseDestroyVFX (CDO true):
            // SpawnEmitterAtLocation(PS_Arya_Poof) at the owner's actor location, scale 1, Team = SelectFloat(0.6,
            // colour, local online) of the owner. (The sounds: mvs-app audio.rs, the DisguiseFx arm.)
            Event::DisguiseFx { fighter, .. } => {
                if let Some(fid) = w.by_id(*fighter) {
                    let at = [0.0, -w.fighter(fid).body.x, actor_y(w, fid)];
                    let team = fx.team(*fighter);
                    fx.player.spawn(POOF, at.map(f64::from), [0.0; 3], [1.0; 3], &[("Team", team)]);
                }
                true
            }
            Event::NotifyVfx { fighter, n, on, left, copy: true, .. } => {
                fx.copy_vfx(w, *fighter, *n, *on, *left);
                true
            }
            Event::ParticleNotify { fighter, n, on, left, copy: true, .. } => {
                fx.second_particle(w, SecondKind::Disguise, *fighter, *n, *on, *left);
                true
            }
            _ => false,
        }
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
