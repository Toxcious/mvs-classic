//! The characters' own HUD parts: one per character of the match that
//! has one. Gizmo's attach meter is a custom UI (custom_ui, H1).

mod finn;
mod harley;
mod rick;
mod stripe;
mod tom_and_jerry;
mod velma;

use mvs_sim::World;

use super::common::{BrokerPlayer, PlayerMap};
use super::input_label::InputLabels;
use super::pop::PopTexts;
use super::{HudSetup, Texts};
use crate::bundle::Bundle;
use crate::element::ElementList;
use crate::slate::Tree;
use crate::text::Fonts;

pub(crate) use finn::FinnShops;
pub(crate) use harley::BoxLabels;
pub(crate) use rick::PortalLabels;
pub(crate) use stripe::StripeHud;
pub(crate) use tom_and_jerry::TomJerryHud;
pub(crate) use velma::VelmaHud;

/// A character's HUD part.
#[derive(Debug)]
pub(crate) enum CharHud {
    Finn(FinnShops),
    TomJerry(TomJerryHud),
    Velma(VelmaHud),
    Stripe(StripeHud),
    Rick(PortalLabels),
    Harley(BoxLabels),
}

impl CharHud {
    /// The parts of the match's characters, at most one of each, in the order the HUD observes them: Harley's boxes,
    /// Rick's portals, then Finn, Tom and Jerry, Velma, Stripe (the dash targets and the tunnel indicators, which
    /// share no state with them, are the HUD's own).
    pub(crate) fn for_match(b: &Bundle, setup: &HudSetup, players: &[BrokerPlayer], texts: &Texts) -> Vec<CharHud> {
        let flags =
            |name: &str| -> Vec<bool> { setup.players.iter().map(|p| p.character.as_deref() == Some(name)).collect() };
        let has = |v: &[bool]| v.iter().any(|&x| x);
        let map = PlayerMap::of(setup);
        // (the boxes, the portals, then the characters' own)
        let mut out = Vec::new();
        if has(&flags("harley")) {
            out.push(CharHud::Harley(BoxLabels::new(players, map)));
        }
        if has(&flags("rick")) {
            out.push(CharHud::Rick(PortalLabels::new(players, map, texts)));
        }
        let finn = flags("finn");
        if has(&finn)
            && let Some(x) = FinnShops::new(b)
        {
            out.push(CharHud::Finn(x));
        }
        let tj = flags("tomandjerry");
        if has(&tj) {
            out.push(CharHud::TomJerry(TomJerryHud::new(b, players, &tj, map)));
        }
        let velma = flags("velma");
        if has(&velma) {
            out.push(CharHud::Velma(VelmaHud::new(players, &velma, texts, setup.online, map)));
        }
        let stripe = flags("stripe");
        if has(&stripe) {
            out.push(CharHud::Stripe(StripeHud::new(b, &stripe, map)));
        }
        out
    }

    /// After each World step.
    #[allow(clippy::too_many_arguments, reason = "the HUD's observe call carries its shared lists")]
    pub(crate) fn observe(
        &mut self,
        w: &World,
        now: f64,
        real: f64,
        inputs: &mut InputLabels,
        pops: &mut PopTexts,
        jumps_reset: &str,
        order: &mut u64,
    ) {
        match self {
            CharHud::Finn(x) => x.observe(w, now, real, inputs, order),
            CharHud::TomJerry(x) => x.observe(w, now, real, inputs, order),
            CharHud::Velma(x) => x.observe(w, now, real, inputs, order),
            CharHud::Stripe(x) => x.observe(w, now, real, inputs, order),
            CharHud::Rick(x) => x.observe(w, now, real, inputs, order),
            CharHud::Harley(x) => x.observe(w, now, real, inputs, pops, jumps_reset, order),
        }
    }

    /// Pawn_Velma's GlassesOff widget is up (`VelmaHud::glasses_off`).
    pub(crate) fn glasses_off(&self) -> bool {
        matches!(self, CharHud::Velma(x) if x.glasses_off())
    }

    /// The world-space widgets (Finn's shops, Tom's pointer), before the attached widgets.
    pub(crate) fn draw_world(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        dpi: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        match self {
            CharHud::Finn(x) => x.draw_world(fonts, out, dpi, project),
            CharHud::TomJerry(x) => x.draw_world(fonts, out, dpi, project),
            CharHud::Velma(x) => x.draw_world(fonts, out, dpi, project),
            CharHud::Stripe(x) => x.draw_world(fonts, out, dpi, project),
            CharHud::Rick(x) => x.draw_world(fonts, out, dpi, project),
            CharHud::Harley(x) => x.draw_world(fonts, out, dpi, project),
        }
    }

    /// The screen layer's widgets (Jerry's corks, Stripe's reticle), after the dash targets.
    pub(crate) fn draw_screen(
        &mut self,
        fonts: &mut Fonts,
        out: &mut ElementList,
        dpi: f64,
        real: f64,
        project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        match self {
            CharHud::Finn(x) => x.draw_screen(fonts, out, dpi, real, project),
            CharHud::TomJerry(x) => x.draw_screen(fonts, out, dpi, real, project),
            CharHud::Velma(x) => x.draw_screen(fonts, out, dpi, real, project),
            CharHud::Stripe(x) => x.draw_screen(fonts, out, dpi, real, project),
            CharHud::Rick(x) => x.draw_screen(fonts, out, dpi, real, project),
            CharHud::Harley(x) => x.draw_screen(fonts, out, dpi, real, project),
        }
    }

    /// The part's roots with their owners on the HUD tape (`FinnShopWidgets`, `TomJerryHud`, `CorkCount`,
    /// `Reticle`), by the places the draws return.
    pub(crate) fn roots(&self) -> Vec<(&'static str, &Tree)> {
        match self {
            CharHud::Finn(x) => x.roots(),
            CharHud::TomJerry(x) => x.roots(),
            CharHud::Velma(x) => x.roots(),
            CharHud::Stripe(x) => x.roots(),
            CharHud::Rick(x) => x.roots(),
            CharHud::Harley(x) => x.roots(),
        }
    }
}
