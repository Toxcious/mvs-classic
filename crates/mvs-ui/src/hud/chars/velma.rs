//! Velma's input labels, made through the HUD's InputLabels after each World step, and Pawn_Velma's GlassesOff widget
//! (`glasses_off`).

use mvs_sim::World;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::chars::velma::carry::carry_run_of;
use mvs_sim::chars::velma::evidence::POLICE_EVIDENCE_N;
use mvs_sim::chars::velma::state::velma_of;
use mvs_sim::combat::buffs::has_buff;
use mvs_sim::fighter::grounded;

use crate::element::ElementList;
use crate::hud::Texts;
use crate::hud::common::{BrokerPlayer, PlayerMap};
use crate::hud::input_label::{InputLabels, LabelParams, LabelSources};
use crate::slate::Tree;
use crate::text::Fonts;

/// The keys of VelmaCarryRun_Actor's InputUI (the jump-on label) and Pawn_Velma's Widget (the call-police label), by
/// Velma's slot.
fn carry_label(slot: usize) -> i64 {
    -6001 - i64::try_from(slot).unwrap_or(0)
}

fn police_label(slot: usize) -> i64 {
    -7001 - i64::try_from(slot).unwrap_or(0)
}

#[derive(Clone, Copy, Debug)]
struct Sent {
    /// The ally slot whose jump-on label shows.
    jump_on: Option<usize>,
    /// A carry run was live at the last read.
    carry: bool,
    /// The call-police label: 0 none, 1 shown, 2 hidden (not walking).
    police: u8,
    grounded: bool,
}

/// The jump-on label (VelmaCarryRun_Actor JumpOnUiMulticast @8472-9704): for the ally in the JumpOnCollision sphere
/// (CarryRunFields.jumpOnAlly), when that pawn IsLocallyControlled (@8472: BrokerPlayer.is_local), InputUI =
/// PopInputLabel once per actor (Action 0, no modifier, an empty Label, text 14, icon 32 x 32 @8577-9101; Lifetime 0,
/// Location (0, 0, 350), attached to the actor itself @9121-9251), TurnOn: Input Params.PlayerIndex = the ally's team
/// index (@9369-9436, as the build has it), Refresh, SetVisibility(Visible), Enter; off: SetVisibility(Hidden)
/// @9666. The actor's end Exits it @9854.
/// The call-police label (Pawn_Velma InputCallPolice @3970-5091, UpdateTargetVisuals @6764-7000): BPI_CallPolice(true)
/// from Buff_EvidenceCount @1857 makes the PopInputLabel (Action 1 Special, Modifier 4 Down @4499, the Label of
/// @4528, text 16, icon 32 x 32; Lifetime 0, Location (0, 0, 350), attached to her) for a locally
/// controlled pawn that is no bot's (UI_Functions PopInputLabel @213-372), and binds UpdateTargetVisuals to
/// OnMovementModeChange @2505: Walking -> Visible + Enter, any other mode -> Hidden. InputCallPolice(false) @5166
/// (the police's spawn, which also destroys Buff_PoliceEvidence @5181): Exit, then Hidden.
/// Read here: on while she holds Buff_PoliceEvidence; the movement mode as the sim's `grounded`.
// Unverified: a hidden widget shown again is a new label here (Enter from its start); the team index is the ally's
// `team`; the Delay(1.0) of Exit's destroy taken as running while the widget is Hidden (hud/input_label.rs: the
// label is dropped at once).
// Deviation: the build's widget is Exited and Hidden at the spawn and stays valid with UpdateTargetVisuals bound
// until Exit's Delay(1.0) destroys it a second later, so a landing inside that second shows it and lets its ExitAnim
// run: the port lets it go at the spawn.
#[derive(Debug)]
pub(crate) struct VelmaHud {
    sent: Vec<Option<Sent>>,
    players: Vec<BrokerPlayer>,
    /// The call-police label's text and its modifier text (Texts.solve_mystery, Texts.down_prefix).
    police_label: String,
    down_prefix: String,
    /// IsPlayingOnline(GetPlayerController(0)): an online match.
    online: bool,
    /// The GlassesOff widget is up (`glasses_off`), as of the last observe.
    glasses: bool,
    map: PlayerMap,
}

impl VelmaHud {
    /// velma: per fighter slot, whether it is a Velma.
    pub(crate) fn new(
        players: &[BrokerPlayer],
        velma: &[bool],
        texts: &Texts,
        online: bool,
        map: PlayerMap,
    ) -> VelmaHud {
        VelmaHud {
            sent: velma
                .iter()
                .map(|&v| v.then_some(Sent { jump_on: None, carry: false, police: 0, grounded: true }))
                .collect(),
            players: players.to_vec(),
            police_label: texts.solve_mystery.clone(),
            down_prefix: texts.down_prefix.clone(),
            online,
            glasses: false,
            map,
        }
    }

    /// Pawn_Velma's GlassesOff_C (a full-screen BackgroundBlur, strength 2), AddToViewport(0) by GlassesOff @2548-2697
    /// and destroyed by GlassesOn at a count of 0 or less @2717, by ReceiveDestroyed @6133, AbductPlayer @6233 and
    /// EndOfMatch @6333: both gated on IsPlayingOnline(GetPlayerController(0)) && IsLocallyControlled() @2764 / @3081.
    /// Here: an online match, this machine's own Velma (BrokerPlayer.is_me: online is_local holds for every pawn), alive, whose GlassesOffCount
    /// (`mvs_sim` chars/velma/pawn.rs, kept for every Velma) is above 0.
    pub(crate) fn glasses_off(&self) -> bool {
        self.glasses
    }

    pub(crate) fn observe(&mut self, w: &World, _now: f64, real: f64, inputs: &mut InputLabels, order: &mut u64) {
        self.glasses = self.online
            && (0..self.sent.len()).any(|i| {
                let (Some(_), Some(f), Some(p)) = (self.sent[i], self.map.fighter(w, i), self.players.get(i)) else {
                    return false;
                };
                p.is_me && w.fighter(f).life.dead <= 0.0 && velma_of(w, f).is_some_and(|s| s.glasses_off_count > 0)
            });
        for i in 0..self.sent.len() {
            let Some(f) = self.map.fighter(w, i) else { continue };
            let Some(mut s) = self.sent[i] else { continue };
            self.jump_on(w, i, f, &mut s, real, inputs, order);
            self.police(w, i, f, &mut s, real, inputs, order);
            self.sent[i] = Some(s);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn jump_on(
        &self,
        w: &World,
        i: usize,
        f: mvs_sim::fighter::FighterId,
        s: &mut Sent,
        real: f64,
        inputs: &mut InputLabels,
        order: &mut u64,
    ) {
        let key = carry_label(i);
        let run = carry_run_of(w, f);
        if run.is_none() {
            // The actor's end Exits the label @9854 (the ExitAnim fade), where no ally only hides it.
            if s.carry && s.jump_on.is_some() {
                inputs.exit(key, real);
            }
            s.carry = false;
            s.jump_on = None;
            return;
        }
        s.carry = true;
        let want = run.and_then(|run| {
            let WorldActorState::VelmaCarryRun(r) = &w.actors.actors.get(run)?.state else { return None };
            let ally = r.jump_on_ally.filter(|_| r.jump_on_ui)?;
            let fi = w.fighter(ally);
            let slot = self.map.index(w, ally)?;
            let local = self.players.get(slot).is_some_and(|p| p.is_local);
            (fi.life.dead <= 0.0 && local).then(|| (slot, usize::try_from(fi.team.team).unwrap_or(0)))
        });
        if want.map(|x| x.0) == s.jump_on {
            return;
        }
        if s.jump_on.is_some() {
            inputs.hide_for(key);
        }
        s.jump_on = want.map(|x| x.0);
        if let Some((_, team)) = want {
            let p = LabelParams { player: team, ..labelled(i, 0, "", 14.0, "") };
            inputs.show_for(key, p, real, order, &LabelSources::default());
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn police(
        &self,
        w: &World,
        i: usize,
        f: mvs_sim::fighter::FighterId,
        s: &mut Sent,
        real: f64,
        inputs: &mut InputLabels,
        order: &mut u64,
    ) {
        let p = self.players.get(i);
        let on = p.is_some_and(|p| p.is_local && !p.is_bot) && has_buff(w, f, POLICE_EVIDENCE_N);
        let key = police_label(i);
        let g = grounded(w, f);
        let moved = g != s.grounded;
        s.grounded = g;
        if !on {
            if s.police != 0 {
                inputs.hide_for(key);
            }
            s.police = 0;
            return;
        }
        // BPI_CallPolice(true): a new label, then each change of the movement mode
        if s.police == 0 || (moved && g) {
            inputs.show_for(
                key,
                labelled(i, 1, &self.police_label, 16.0, &self.down_prefix),
                real,
                order,
                &LabelSources::default(),
            );
            s.police = 1;
        } else if moved && !g && s.police == 1 {
            inputs.hide_for(key);
            s.police = 2;
        }
    }

    pub(crate) fn draw_world(
        &mut self,
        _fonts: &mut Fonts,
        _out: &mut ElementList,
        _dpi: f64,
        _project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        Vec::new()
    }

    pub(crate) fn draw_screen(
        &mut self,
        _fonts: &mut Fonts,
        _out: &mut ElementList,
        _dpi: f64,
        _real: f64,
        _project: &dyn Fn([f64; 3]) -> Option<[f64; 2]>,
    ) -> Vec<usize> {
        Vec::new()
    }

    pub(crate) fn roots(&self) -> Vec<(&'static str, &Tree)> {
        Vec::new()
    }
}

/// A label on Velma `i` at (0, 0, 350): the action, text, text size and modifier given, icon 32, her player index.
fn labelled(i: usize, action: u8, label: &str, text_size: f64, modifier: &str) -> LabelParams {
    LabelParams {
        action,
        location: [0.0, 0.0, 350.0],
        label: label.to_string(),
        text_size,
        attach: Some(i),
        player: i,
        icon_size: 32.0,
        custom_icon: None,
        modifier: modifier.to_string(),
    }
}
