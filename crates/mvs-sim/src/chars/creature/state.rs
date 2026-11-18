//! Reindog's own fighter state (`ReindogState`): Pawn_Reindog's own variables
//! beside BaseFighter's and Creature_AnimBP's sim-side inputs, and BPI_Reindog's small call surface the other files
//! use. Offsets are Pawn_Reindog's ubergraph's (Creature_AnimBP's where named). Every field is declared here, whichever
//! file writes it (the comment names the owner: pawn.rs unless said).

use crate::Real;
use crate::State;
use crate::actors::ProjectileId;
use crate::actors::projectile::ProjectileExt;
use crate::chars::CharState;
use crate::fighter::FighterId;
use crate::registry::char_hooks::TetherInfo;
use crate::world::World;

/// `ReindogState`.
#[derive(State, Debug)]
pub struct ReindogState {
    /// Original Capsule Half Height / Original Capsule Radius: BeginPlay @106 / @155 read the capsule's unscaled
    /// CapsuleHalfHeight / CapsuleRadius once; EndCrouch @1271 / @1320 writes them back.
    pub original_cap_half: Real,
    pub original_cap_radius: Real,
    /// Is Held: SetIsBeingHeld @1221 (BPI_SetIsBeingHeld, `fighter::tick::set_is_being_held`); BPI_IsHeld reads it.
    pub is_held: bool,
    /// Tether: SetTether @2251 (`None` for an invalid argument, the TetherProjectile otherwise). A tether destroyed
    /// after it was set is still named here; its `dead` is what IsValid reads (`get_tether_info`).
    pub tether: Option<ProjectileId>,
    /// Creature_AnimBP.CreatureHeadScale: written by nothing in the sim (the class default 1); the view draws the
    /// fighter's head scale.
    pub head_scale: Real,
    /// ComponentBoundEvent_1 (the capsule's OnComponentHit) @1425 -> Delay(0.01) @1370 pending until it resumes @15
    /// (HACKCreatureWallBounceMulticast). A second call while pending is ignored.
    pub bounce_delay: bool,
    /// Creature_AnimBP COM_Spin: AnimNotify_ReindogSpin's begin / end; StopRotation @21873 false.
    pub com_spin: bool,
    /// Creature_AnimBP Spin_Rotation: BlueprintUpdateAnimation @1528-2007 (FInterpTo_Constant toward -360 while
    /// COM_Spin, else 0); StopRotation @21873 0.
    pub spin_rotation: Real,
    /// The queued anim events of the notify and of StopRotation (pawn.rs animUpdate): this frame's (`*_cur`, written
    /// at fighter tick `cur_at`) and earlier frames' not yet seen (`*_old`). spin: -1 none, 0 COM_Spin false, 1
    /// true (the last write); stop: StopRotation ran.
    pub spin_cur: i32,
    pub stop_cur: bool,
    pub cur_at: i32,
    pub spin_old: i32,
    pub stop_old: bool,
    /// Creature_AnimBP Is Hit Stunned: Comp_Pawn_HitstunManager.CheckHitstunActive @645-712, each update.
    pub is_hit_stunned: bool,
    /// Creature_AnimBP Is Respawning: SetIsRespawning @21977 (bound to OnFighterReset @2812-2835) true, then
    /// Delay(0.1) @21988 -> @2031 false: the frames left of that Delay (`respawn_delay`, 0 when none).
    pub is_respawning: bool,
    pub respawn_delay: Real,
}

impl Default for ReindogState {
    /// The field initial values.
    fn default() -> Self {
        ReindogState {
            original_cap_half: 0.0,
            original_cap_radius: 0.0,
            is_held: false,
            tether: None,
            head_scale: 1.0,
            bounce_delay: false,
            com_spin: false,
            spin_rotation: 0.0,
            spin_cur: -1,
            stop_cur: false,
            cur_at: 0,
            spin_old: -1,
            stop_old: false,
            is_hit_stunned: false,
            is_respawning: false,
            respawn_delay: 0.0,
        }
    }
}

// ---- the call surface of the state

/// `reindogOf`: a fighter's Reindog state, `None` when it is not a Reindog (a failed cast to BPI_Reindog).
pub fn reindog_of(w: &World, f: FighterId) -> Option<&ReindogState> {
    match &w.fighter(f).cs {
        CharState::Creature(s) => Some(s),
        _ => None,
    }
}

/// `reindogOf`, to write.
pub fn reindog_mut(w: &mut World, f: FighterId) -> Option<&mut ReindogState> {
    match &mut w.fighter_mut(f).cs {
        CharState::Creature(s) => Some(s),
        _ => None,
    }
}

/// `reindog` (his own code's read, the fighter a Reindog). A panic is a hook entered for a fighter of another
/// character.
pub fn reindog(w: &World, f: FighterId) -> &ReindogState {
    reindog_of(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Reindog"))
}

/// `reindog`, to write.
pub fn reindog_w(w: &mut World, f: FighterId) -> &mut ReindogState {
    reindog_mut(w, f).unwrap_or_else(|| panic!("fighter {f:?} is not a Reindog"))
}

/// A live tether projectile: in the list, not dead (IsValid).
fn live(w: &World, t: Option<ProjectileId>) -> Option<&crate::actors::Projectile> {
    t.and_then(|id| w.actors.projectiles.get(id)).filter(|p| !p.dead)
}

/// `setTether` (BPI_SetTether -> SetTether @2251): IsValid(arg) false -> Tether = null @1209, else the tether. No-op
/// for a fighter that is not a Reindog.
pub fn set_tether(w: &mut World, f: FighterId, t: Option<ProjectileId>) {
    let t = live(w, t).map(|_| t).unwrap_or(None);
    if let Some(s) = reindog_mut(w, f) {
        s.tether = t;
    }
}

/// `getTetherInfo` (BPI_GetTetherInfo): IsValid(Tether) -> (true, Tether.AttachedToFighter), else (false, false); a
/// fighter that is not a Reindog answers (false, false) (the callers' failed cast).
pub fn get_tether_info(w: &World, f: FighterId) -> TetherInfo {
    let t = reindog_of(w, f).and_then(|s| live(w, s.tether));
    match t {
        Some(p) => TetherInfo { out: true, attached_to_ally: attached_to_fighter(p) },
        None => TetherInfo::default(),
    }
}

/// `isHeld` (BPI_IsHeld): Is Held, false for a fighter that is not a Reindog.
pub fn is_held(w: &World, f: FighterId) -> bool {
    reindog_of(w, f).is_some_and(|s| s.is_held)
}

/// `TetherProjectile.attachedToFighter` (AttachedToFighter): it has an ally. False for another class.
fn attached_to_fighter(p: &crate::actors::Projectile) -> bool {
    matches!(&p.ext, ProjectileExt::TetherProjectile(t) if t.ally.is_some())
}
