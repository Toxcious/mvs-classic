//! Velma's police minion's effects (carViews' MinionFx). The car's
//! view and the VelmaJenkinsPin's mesh are mvs-view's chars/velma.rs.
//!
//! The car's events (the hit effect, DestroyVisuals' PS_Velma_PoliceCar_Destroyed burst, its dust) go through the
//! shared minion slots (fx/shared/minion.rs) with the car's colours: GetColorIndex = the ForcedColorIndex SetOutline
//! writes (the owner's colour index), IsLocalOnlinePlayer false.
//! Unverified: which of SetOutline @216 and StartupVisuals @4458 writes ForcedColorIndex last is not read; the effects
//! take SetOutline's.
//! Unverified: no trace match spawns the car (every Velma baseline's police is none): nothing here is on a check.

use mvs_sim::World;
use mvs_sim::chars::velma::police::POLICE_DATA;
use mvs_sim::events::Event;

use super::{CharFxHooks, ProjRule};
use crate::fx::Fx;
use crate::fx::shared::minion::Minions;

const CREATOR: &str = mvs_sim::chars::velma::KEY;

/// What Velma's police minion's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct VelmaPolice {
    cars: Minions,
    /// The companion's skin (its data's default, VelmaAIBotPolice), None until the first look; "" without the data
    /// (the match holds no velmapolice: nothing is drawn).
    skin: Option<String>,
}

impl VelmaPolice {
    /// The car's skin name when a Velma is in the match and the companion's data is held.
    fn skin(&mut self, w: &World) -> Option<String> {
        if self.skin.is_none()
            && !w.pawns.as_deref().unwrap_or(&w.players).iter().any(|&f| w.names.str(w.fighter(f).ch) == CREATOR)
        {
            // (a mode's pawns come after the start: asked again until a Velma is in the match)
            return None;
        }
        let s = self.skin.get_or_insert_with(|| match w.data.chars.get(POLICE_DATA) {
            Some(d) => d.fighter.skin.name.clone(),
            None => {
                eprintln!("warning: Velma: the match holds no {POLICE_DATA}: the police are not drawn");
                String::new()
            }
        });
        (!s.is_empty()).then(|| s.clone())
    }
}

impl CharFxHooks for VelmaPolice {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        let Some(skin) = self.skin(w) else { return };
        self.cars.place(fx, w, CREATOR, POLICE_DATA, &skin);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.cars.reset(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.cars.end(fx);
    }

    fn minion_event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        let Some(skin) = self.skin(w) else { return false };
        self.cars.event(fx, w, e, POLICE_DATA, &skin)
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
