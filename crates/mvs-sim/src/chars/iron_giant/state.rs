//! Iron Giant's own fighter state (`GiantState`): Pawn_C017's and
//! C017_AnimBP's own variables beside BaseFighter's, and the reads shared code makes of them. Every field is declared
//! here, whichever file writes it (the comment names the owner). The component's calls back into the pawn
//! (`hoverRageOwner`, closures over the fighter) are no state: the hover rage's functions take the World
//! and the fighter.

use super::hover_rage::HoverRage;
use super::pose::GiantAnim;
use super::turret::AllyTurret;
use crate::Real;
use crate::State;
use crate::actors::ActorId;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::world::World;

/// `GiantState`.
#[derive(State, Debug, Default)]
pub struct GiantState {
    /// Comp_C017_Hover_Rage (`hover_rage.rs`), made by `init` (pawn.rs).
    pub hover_rage: Option<HoverRage>,
    /// IsMaxRage: set by the component's ReachedMaxRage (@13997), cleared on entering weapon mode (@7683), by the
    /// reset (@21840) and the death (@22336). pawn.rs.
    pub is_max_rage: bool,
    /// AlternateMode: weapon mode (setAlternateMode, pawn.rs).
    pub alternate_mode: bool,
    /// isGliding: the hover (readHoverInput, pawn.rs).
    pub gliding: bool,
    /// The hover's other flags (pawn.rs): CanAttackHover and the two bound delegates (AnimNotify_C017_CanHover:
    /// BindHoverJump), AttackForceHover / IgnoreFuel (BPI_ForceHover, AnimNotify_C017_ForceHover) and
    /// HoverFastFallBlock (BlockFastfallServer: one BlockFastFallStacks count while set).
    pub can_attack_hover: bool,
    pub attack_force_hover: bool,
    pub hover_ignore_fuel: bool,
    pub hover_fast_fall_block: bool,
    /// Comp_C017_Hover_Rage ReceiveBeginPlay's Delay(2.0) (@5901) ahead of the ally bindings (@440-1183), in pawn
    /// ticks left (set by `init`); 0: AllyHitRageBuild / OnAllyDeathRage are bound. pawn.rs.
    pub ally_rage_delay: Real,
    /// OutOfRageInturupted bound to the anim instance's OnMontageEnded (@2091; removed at the exit @9571). pawn.rs.
    pub out_of_rage_bound: bool,
    /// Pawn_C017 IsBlocking (BPI_SetBlocking @20025, AnimNotify_C017_IsBlocking): read by `Fighter.maxWalk`. pawn.rs.
    pub giant_blocking: bool,
    /// Pawn_C017 SetTurretAim (@10153, AnimNotify_C017_TurretAim): TurretAimNumber, 'Turrent Aim On' and
    /// TurretAimStartFacingLeft (BPI_CheckTurretFacing / BPI_TurretInitialFacingLedt, read by his aimers). pawn.rs.
    pub turret_aim_number: Real,
    pub turret_aim_on: bool,
    pub turret_aim_start_left: bool,
    /// Comp_C017_Ally_Turret's variables (`turret.rs`): from his first ToggleAllyTurret, `None` before it.
    pub ally_turret: Option<AllyTurret>,
    /// C017_AnimBP's own variables and clock under the shapes (`pose.rs`, the split's in `split.rs`), made by `init`.
    pub giant_anim: Option<GiantAnim>,
    /// C017_AnimBP HoldingBuffValid (BPI_GrabOverride). hold.rs.
    pub hold_valid: bool,
    /// C017_AnimBP HoldAlpha (AnimNotify_C017_Hold_Overide, Buff_HeldBy_C017 SetHoldOverride). hold.rs.
    pub hold_alpha: Real,
    /// Comp_Pawn_Giant GiantCrouched (SetGiantCrouched, from AnimNotify_GiantIsCrouched's Begin / End through
    /// BPI_SetGiantCrouched: pawn.rs), from his first such call (`None` before it, read as false: [`giant_crouched`]).
    pub giant_crouched: Option<bool>,
    /// The spawned actors bound to the component's GiantCrouchChanged (SmokeOcclusionParent OnBeginOverlap @94-168 ->
    /// HandleGiantCrouch), in bind order; from the first bind, `None` before it. The occlusion buffs' binding
    /// (Buff_Invisibility @1795) is the buff list's (BuffSignal 'giantCrouchChanged').
    pub crouch_listeners: Option<Vec<ActorId>>,
}

// ---- the call surface of the state

/// `giantOf`: the fighter's Giant state, `None` for a fighter of another character (the build's failed cast to
/// Pawn_C017 / missing component).
pub fn giant_of(w: &World, f: FighterId) -> Option<&GiantState> {
    match &w.fighter(f).cs {
        CharState::IronGiant(s) => Some(s),
        _ => None,
    }
}

/// `giantOf`, to write.
pub fn giant_mut(w: &mut World, f: FighterId) -> Option<&mut GiantState> {
    match &mut w.fighter_mut(f).cs {
        CharState::IronGiant(s) => Some(s),
        _ => None,
    }
}

/// `giant` (his own code's read: the fighter is an Iron Giant, as his notifies, hooks, actors' owner and hold buff's
/// instigator are). A panic is a hook entered for a fighter of another character.
pub fn giant(w: &World, f: FighterId) -> &GiantState {
    giant_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not an Iron Giant"))
}

/// `giant`, to write.
pub fn giant_w(w: &mut World, f: FighterId) -> &mut GiantState {
    giant_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not an Iron Giant"))
}

/// `giantCrouched`: BPI_GetGiantCrouched of any fighter (SmokeOcclusionParent @622,
/// Buff_Invisibility @7782): false on another pawn (the failed cast's default @730 / @8634).
pub fn giant_crouched(w: &World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| g.giant_crouched == Some(true))
}

/// `bindGiantCrouch`: Comp_Pawn_Giant.GiantCrouchChanged's AddMulticastDelegate
/// (AddUnique: a second bind of the same actor adds nothing); nothing on another pawn (the IsValid of
/// GetComponentByClass(Comp_Pawn_Giant) ahead of it, @51-80).
// The actor lives in the one World, so no world is kept beside it.
pub fn bind_giant_crouch(w: &mut World, a: ActorId, f: FighterId) {
    let Some(g) = giant_mut(w, f) else { return };
    let l = g.crouch_listeners.get_or_insert_with(Vec::new);
    if !l.contains(&a) {
        l.push(a);
    }
}

/// `unbindGiantCrouch`: RemoveMulticastDelegate. An unbind makes a new list, so a broadcast in progress still calls
/// what was bound when it began (the broadcast's own copy here).
pub fn unbind_giant_crouch(w: &mut World, a: ActorId, f: FighterId) {
    let Some(g) = giant_mut(w, f) else { return };
    if let Some(l) = g.crouch_listeners.as_mut()
        && l.contains(&a)
    {
        l.retain(|&x| x != a);
    }
}

/// LandLaunchHover's gate: `g.canAttackHover && g.gliding && g.hoverRage.fuel > 0`.
pub fn giant_land_hover(w: &World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| {
        g.can_attack_hover
            && g.gliding
            && g.hover_rage.as_ref().expect("an Iron Giant's hover rage (made by init)").fuel > 0.0
    })
}

/// `giantOf(f)?.hoverFastFallBlock`.
pub fn giant_hover_fast_fall_block(w: &World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| g.hover_fast_fall_block)
}

/// IsBlocking, read by `Fighter.maxWalk`.
pub fn giant_blocking(w: &World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| g.giant_blocking)
}

/// isGliding, read by `Fighter.maxWalk`.
pub fn giant_gliding(w: &World, f: FighterId) -> bool {
    giant_of(w, f).is_some_and(|g| g.gliding)
}

/// `g.hoverRage?.ownHit(g.hoverRageOwner, hitBoxDamage)`, read by
/// `dealtRage` for an Iron Giant's hit on another team: nothing when he has no hover rage.
pub fn hover_rage_own_hit(w: &mut World, f: FighterId, hit_box_damage: Real) {
    if giant_of(w, f).is_some_and(|g| g.hover_rage.is_some()) {
        super::hover_rage::own_hit(w, f, hit_box_damage);
    }
}
