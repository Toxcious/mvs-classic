//! Wonder Woman's own fighter state (`WonderWomanState`): Pawn_WonderWomanV3's
//! own variables beside BaseFighter's, Comp_BracerCharge's and WonderWoman_AnimBPV2's sim-side inputs, and the meter's
//! three writes. Every field is declared here, whichever file writes it (the comment names the owner).

use crate::Real;
use crate::State;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::fighter::anim::SlotMix;
use crate::math::{max, min};
use crate::world::World;

/// Default__Pawn_WonderWomanV3_C MeterPerArmorHit.
pub const METER_PER_ARMOR_HIT: Real = 0.1;
/// Default__Pawn_WonderWomanV3_C MeterPerProjectile.
pub const METER_PER_PROJECTILE: Real = 0.2;
/// Default__Pawn_WonderWomanV3_C MeterPerBuffBlock.
pub const METER_PER_BUFF_BLOCK: Real = 0.2;
/// Default__Comp_BracerCharge_C ChargeMax (a float variable).
pub const BRACER_CHARGE_MAX: Real = 1.9_f32 as Real;

/// `WonderWomanState`.
#[derive(State, Debug)]
pub struct WonderWomanState {
    /// MeterPercentage, 0..1: the bracer meter the HUD's bar shows and WonderWoman_MeterBracerClash reads at its spawn
    /// (BPI_GetMeterPercentage). Nothing drains it over time: the writes are [`add_meter`] and [`clear_meter`].
    pub meter: Real,
    /// Comp_BracerCharge: ChargeAmount (seconds, up to ChargeMax 1.9) and ChargeGate
    /// (AnimNotify_WonderWoman_BracerCharge); pawn.rs.
    pub bracer_charge: Real,
    pub bracer_gate: bool,
    /// WonderWoman_AnimBPV2's LassoAimU / LassoAimD (the alphas of the lasso throw's up and down aim layers), written
    /// by the lasso actor alone (actors.rs: 0 at its BeginPlay and its destroy, the control multicast in between). The
    /// AnimBP's class default of both is 1 and nothing writes them before the first lasso's BeginPlay.
    pub lasso_aim_u: Real,
    pub lasso_aim_d: Real,
    /// The AnimBP's two nested bool lists over the slots ShieldWalk_N / _F / _B (pawn.rs shieldWalkUpdate):
    /// `shield_move_w` the weight of BlendListByBool_2's pose 0 (IsMoving), `shield_fwd_w` that of BlendListByBool_1's
    /// pose 0 (MovingForward), -1 until that list has had its first update.
    pub shield_move_w: Real,
    pub shield_fwd_w: Real,
    /// The tracks mixed into the fighter's pose in order (CharHooks.slot_mix, pawn.rs's slotCompose), empty outside
    /// the Loop of the ground side special; `slot_blend` and `slot_players` stay `None`.
    pub slot_mix: SlotMix,
    /// `Fighter.sampleBoxes`' buffer for a `slot_mix` pose. Not state.
    #[state(skip)]
    pub mix_buf: Option<Vec<f32>>,
}

impl Default for WonderWomanState {
    /// The field initial values.
    fn default() -> Self {
        WonderWomanState {
            meter: 0.0,
            bracer_charge: 0.0,
            bracer_gate: false,
            lasso_aim_u: 1.0,
            lasso_aim_d: 1.0,
            shield_move_w: 0.0,
            shield_fwd_w: -1.0,
            slot_mix: SlotMix::default(),
            mix_buf: None,
        }
    }
}

// ---- the call surface of the state

/// `wonderWomanOf`: a fighter's Wonder Woman state, `None` when it is not a Wonder Woman.
pub fn wonder_woman_of(w: &World, f: FighterId) -> Option<&WonderWomanState> {
    match &w.fighter(f).cs {
        CharState::WonderWoman(s) => Some(s),
        _ => None,
    }
}

/// `wonderWomanOf`, to write.
pub fn wonder_woman_mut(w: &mut World, f: FighterId) -> Option<&mut WonderWomanState> {
    match &mut w.fighter_mut(f).cs {
        CharState::WonderWoman(s) => Some(s),
        _ => None,
    }
}

/// `wonderWoman` (her own code's read, the fighter a Wonder Woman). A panic is a hook entered for a fighter of another
/// character.
pub fn wonder_woman(w: &World, f: FighterId) -> &WonderWomanState {
    wonder_woman_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Wonder Woman"))
}

/// `wonderWoman`, to write.
pub fn wonder_woman_w(w: &mut World, f: FighterId) -> &mut WonderWomanState {
    wonder_woman_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Wonder Woman"))
}

/// `addMeter`: the pawn's three meter gains, each MeterPercentage = FClamp(MeterPercentage + amount, 0, 1) and the
/// multicast: On Armor Break @1361-1484 (MeterPerArmorHit), BPI_BlockedProjectile @1837-1987 (MeterPerProjectile),
/// BPI_BuffBlocked @2133-2283 (MeterPerBuffBlock). The callers reach the pawn through a BPI_WonderWoman cast, so a
/// fighter that is no Wonder Woman takes nothing.
pub fn add_meter(w: &mut World, f: FighterId, amount: Real) {
    if let Some(s) = wonder_woman_mut(w, f) {
        s.meter = min(max(s.meter + amount, 0.0), 1.0);
    }
}

/// `clearMeter`: BPI_ClearMeter @2011-2041, MeterPercentage = 0.
pub fn clear_meter(w: &mut World, f: FighterId) {
    if let Some(s) = wonder_woman_mut(w, f) {
        s.meter = 0.0;
    }
}

/// `meterOf`: BPI_GetMeterPercentage; 0 for a fighter that is no Wonder Woman (the failed cast,
/// WonderWoman_BracerClash @935).
pub fn meter_of(w: &World, f: FighterId) -> Real {
    wonder_woman_of(w, f).map_or(0.0, |s| s.meter)
}

/// CharHooks.slotMix (`mod.rs` `slotMix: wonderWoman`).
pub(super) fn slot_mix(w: &World, f: FighterId) -> Option<&SlotMix> {
    Some(&wonder_woman(w, f).slot_mix)
}

/// CharHooks.slot_mix_mut: `slotMix`, written.
pub(super) fn slot_mix_mut(w: &mut World, f: FighterId) -> Option<&mut SlotMix> {
    wonder_woman_mut(w, f).map(|s| &mut s.slot_mix)
}
