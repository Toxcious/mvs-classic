//! The heart skins' petals (the opacity scalar and the rules are mvs-view's
//! heart_skin.rs, which says what the build does).
//!
//! HeartSkin_Initialize's Spawn Attached Particles (BaseFighter: SpawnEmitterAttached(system, Mesh, socket, location,
//! rot(0, 0, 0), scale 1, SnapToTarget, no auto destroy, auto activate) into Character Particles) and
//! HeartSkin_SetOpacity's Deactivate / SetVisibility(false) while the pawn moves or falls and Activate(true) /
//! SetVisibility(true) when it stands again.
//! Unverified: the port draws the hidden, deactivated petals as killed and the reset as new components (as PS_Heal's
//! reset, Fx::place_heal).
//! Team on every petal @1547: SelectFloat(0.2, ColorIndex, Gradient_Curve of slot 0's MID == Gradient_Curve of the
//! skin's LocalPlayerMaterialOverride OR IsLocalOnlinePlayer): 0.2 for the local set or a local online player, else the
//! colour index.

use indexmap::IndexMap;
use mvs_sim::World;
use mvs_vfx::SystemId;
use mvs_vfx::math::IDENTITY;
use mvs_view::heart_skin::{HeartRule, moving, rule_of};
use mvs_view::skins::{Variant, fighter_colours};

use crate::fx::Fx;
use crate::fx::chars::{CharFxHooks, ProjRule};

/// A heart pawn's petals (None: a component the player could not make, not tried again).
type Petals = Vec<Option<SystemId>>;

#[derive(Debug, Default)]
pub(crate) struct HeartSkin {
    /// By the fighter's id.
    petals: IndexMap<i32, Petals>,
}

/// No projectile class of its own.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];

impl HeartSkin {
    /// The petals alive now (the tests').
    #[cfg(test)]
    pub(crate) fn live(&self, player: &mvs_vfx::player::Player) -> usize {
        self.petals.values().flatten().flatten().filter(|h| player.alive(**h)).count()
    }

    fn kill(&mut self, fx: &mut Fx, id: i32) {
        for h in self.petals.entry(id).or_default().drain(..).flatten() {
            fx.player.kill(h);
        }
    }

    fn spawn(&mut self, fx: &mut Fx, w: &World, id: i32, rule: &HeartRule) {
        // (host.skinVariant: the view's colours, with the team-coloured skins, not Fx's own)
        let colours = fighter_colours(&w.setup, fx.me, fx.online, fx.team_skins);
        let local = colours.get(id as usize).is_some_and(|c| c.variant == Variant::Local) || fx.team(id) == 0.6;
        let team = if local { 0.2 } else { f64::from(fx.color_index(id)) };
        let mut out = Vec::new();
        for p in rule.petals {
            // SnapToTarget: the relative scale is 1 over the socket's world scale at the spawn (world scale 1)
            let probe = fx.socket_follow(w, id, Some(p.socket), [0.0; 3], &IDENTITY, [1.0; 3]);
            let sc = fx.place(w, &probe).and_then(|a| a.scale).unwrap_or([1.0; 3]);
            let rel = [1.0 / sc[0], 1.0 / sc[1], 1.0 / sc[2]];
            let f = fx.socket_follow(w, id, Some(p.socket), p.loc, &IDENTITY, rel);
            out.push(fx.attach_to_mesh(w, id, p.ps, p.socket, f, &[("Team", team)]));
        }
        self.petals.insert(id, out);
    }
}

impl CharFxHooks for HeartSkin {
    /// Once per presentation tick, after the views are posed: HeartSkin_SetOpacity's petals (the first call is
    /// Initialize's spawn and the first tick's rule at once).
    fn place(&mut self, fx: &mut Fx, w: &World) {
        let ids: Vec<i32> = fx.pawns.iter().filter(|(_, p)| p.heart && p.minion.is_none()).map(|(id, _)| *id).collect();
        for id in ids {
            let Some(fid) = w.by_id(id) else { continue };
            let Some(rule) = rule_of(w.names.str(w.fighter(fid).ch)) else { continue };
            if moving(w, fid, rule) {
                self.kill(fx, id);
                continue;
            }
            let stale = match self.petals.get(&id) {
                None => true,
                Some(p) => p.is_empty() || p.iter().flatten().any(|h| !fx.player.alive(*h)),
            };
            if stale {
                self.kill(fx, id);
                self.spawn(fx, w, id, rule);
            }
        }
    }

    fn reset(&mut self, _fx: &mut Fx) {
        // (a Lab reset clears the player's systems: the handles are gone)
        self.petals.clear();
    }

    fn end(&mut self, fx: &mut Fx) {
        let ids: Vec<i32> = self.petals.keys().copied().collect();
        for id in ids {
            self.kill(fx, id);
        }
    }
}
