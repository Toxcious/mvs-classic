//! LeBron's own fighter state (`LebronState`): Pawn_C016's own variables and
//! c016_AnimBP's HasBall, beside BaseFighter's, and the state's small call surface the other files use. Offsets are
//! Pawn_c016's ubergraph's. Every field is declared here, whichever file writes it (the comment names the owner).

use crate::Real;
use crate::State;
use crate::chars::CharState;
use crate::combat::buffs::buff_of;
use crate::fighter::FighterId;
use crate::name;
use crate::names::Name;
use crate::world::World;

/// `BALL`: the ball in his hands, a buff of the pawn.
pub const BALL: Name = name!("Buff_C016_Ball_Conditional_C");
/// `DUNK`.
pub const DUNK: Name = name!("C016_Special_Dunk1_Montage");

/// `LebronState`.
#[derive(State, Debug)]
pub struct LebronState {
    /// HasBall (class default true): the copy of "the pawn has a Buff_C016_Ball_Conditional" that SetHasBall writes
    /// (@2874), one latent step behind the buff (pawn.rs).
    pub has_ball: bool,
    /// c016_AnimBP's own HasBall (class default false): written by OnSetHasBall @7110 alone; the anim set his machines
    /// read (pawn.rs `anim_set`).
    pub anim_has_ball: bool,
    /// The Delay(0) of BPI_SetHasBall (@2287): its latent action exists ([`request_has_ball`]; pawn.rs's tick fires it).
    pub ball_delay: bool,
    /// The Delay(0) of ReceiveBeginPlay (@2622), pending from the pawn's BeginPlay to its first latent pass.
    pub begin_delay: bool,
    /// SetHasBall calls so far (each one an AnimBP.OnSetHasBall(HasBall) @2893).
    pub ball_sets: i32,
    /// TeleportDunkReady (class default false): SetTeleportDunk @2999, TeleportDunkOff @3031 (ball.rs, pawn.rs's
    /// Buff_C016_RemoveBall, later.rs).
    pub teleport_dunk_ready: bool,
    /// IsTeleportDunking (class default false): BPI_SetIsTeleportDunking @3520 (later.rs's completion; pawn.rs's
    /// C016EndTeleportDunk clears it).
    pub is_teleport_dunking: bool,
    /// LeBronCapsuleHight: the root capsule's unscaled half height saved at BeginPlay @2523 (pawn.rs).
    pub cap_half: Real,
    /// LeBronCapsuleRadius (pawn.rs).
    pub cap_radius: Real,
}

impl Default for LebronState {
    /// The field initial values.
    fn default() -> Self {
        LebronState {
            has_ball: true,
            anim_has_ball: false,
            ball_delay: false,
            begin_delay: true,
            ball_sets: 0,
            teleport_dunk_ready: false,
            is_teleport_dunking: false,
            cap_half: 0.0,
            cap_radius: 0.0,
        }
    }
}

// ---- the call surface of the state

/// `lebronOf`: a fighter's LeBron state, `None` when it is not a LeBron (a failed cast to BPI_C016).
pub fn lebron_of(w: &World, f: FighterId) -> Option<&LebronState> {
    match &w.fighter(f).cs {
        CharState::Lebron(s) => Some(s),
        _ => None,
    }
}

/// `lebronOf`, to write.
pub fn lebron_mut(w: &mut World, f: FighterId) -> Option<&mut LebronState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Lebron(s) => Some(s),
        _ => None,
    }
}

/// `lebron` (his own code's read, the fighter a LeBron). A panic is a hook entered for a fighter of another
/// character.
pub fn lebron(w: &World, f: FighterId) -> &LebronState {
    lebron_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a LeBron"))
}

/// `lebron`, to write.
pub fn lebron_w(w: &mut World, f: FighterId) -> &mut LebronState {
    lebron_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a LeBron"))
}

/// `hasBallBuff`: Comp_State_Manager.CheckHasBuff(Buff_C016_Ball_Conditional_C).HasBuff on any fighter (`buffOf`).
pub fn has_ball_buff(w: &World, f: FighterId) -> bool {
    buff_of(w, f, BALL).is_some()
}

/// `requestHasBall`: BPI_SetHasBall @2257 (HasAuthority, Delay(0) @2287); a second call while the action exists does
/// nothing. A fighter that is no LeBron: nothing.
pub fn request_has_ball(w: &mut World, f: FighterId) {
    if let Some(s) = lebron_mut(w, f) {
        s.ball_delay = true;
    }
}

/// `setTeleportDunkAvailable`: BPI_SetTeleportDunkAvailable -> SetTeleportDunk @2999 (TeleportDunkOff @3031 is the
/// same write of false). A fighter that is no LeBron: nothing.
pub fn set_teleport_dunk_available(w: &mut World, f: FighterId, on: bool) {
    if let Some(s) = lebron_mut(w, f) {
        s.teleport_dunk_ready = on;
    }
}

/// `setIsTeleportDunking`: BPI_SetIsTeleportDunking @3520. A fighter that is no LeBron: nothing.
pub fn set_is_teleport_dunking(w: &mut World, f: FighterId, on: bool) {
    if let Some(s) = lebron_mut(w, f) {
        s.is_teleport_dunking = on;
    }
}
