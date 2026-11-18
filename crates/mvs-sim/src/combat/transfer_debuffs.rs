//! Buff_TransferDebuffs (Blueprints/Abilities/Shield/Buff_TransferDebuffs,
//! offsets of its ubergraph): a shared BuffParentActor child Stripe's PerkAttach perk gives (each debuff of the
//! Instigator moved to the owner). Class data: the BuffParentActor defaults (not IsDebuff).
//!
//! Its one spawner in the build is Stripe's Buff_C023B_PerkAttach ("Feeding After Midnight", OnEnemyAttached @195: on
//! the enemy his dash grab hit, Stripe its Instigator, StackCount 0, duration 0; `chars::stripe::perks`).
//!
//! ReceiveBeginPlay @1768 (the parent's first), on authority:
//! - the Instigator's Comp_State_Manager @1828 and Comp_Actor_Parent @1942 valid (a fighter Instigator; none: @954
//!   with PopText false, so the destroy alone): Map_Keys(Children) @2118, the Instigator's buff actors, taken once
//!   ahead of the loop;
//! - per key @15-1701: IsValid @240 (live), cast to BPI_Buffs @338 (a buff actor; else ShouldTransfer false @1706),
//!   BPI On Transfer Debuffs @417 (BuffParentActor's @820, overridden by no class of the build: ShouldTransfer =
//!   IsDebuff, the actor's own value; StackCount = CurrentStackAmount; BuffDuration = GetBuffRemainingDuration when
//!   its life timer is active, else 0.0). ShouldTransfer: PopText = true @491; the Instigator's Comp_State_Manager
//!   SpawnBuffActor(the buff's class, this buff's owner, the Instigator, StackCount, BuffDuration, BreakIfBlocked
//!   true) @766, then the source buff K2_DestroyActor @917 whether the spawn was taken or not;
//! - after the loop @954: PopText -> the owner's BPI_PopText "Transferred" (lifetime 1.0, text_color (0, 1, 0.204562,
//!   1), location (0, 0, 250), space 3, scale 1, centred, no delay, icons or audio) @1067-1571: the `popText` event of
//!   kind Transferred (drawn by the HUD);
//! - K2_DestroyActor @1617 (it never outlives its BeginPlay).
// Deviation: the remaining duration is read from the port's frame count ((frames - 1) x DELTA_SECONDS, as Harley's
// Buff_ApplyConfetti), not from the timer manager's float.
// Unverified: the walk is the buff list's order (spawn order), not Map_Keys' order of Children; the order matters
// only when two transferred classes block or break each other.

use crate::Real;
use crate::combat::BuffClass;
use crate::combat::buffs::{BuffRef, buff, end_buff, inst_view, spawn_buff};
use crate::consts::DELTA_SECONDS;
use crate::events::{Event, PopKind};
use crate::fighter::FighterId;
use crate::math::{max, to_int32};
use crate::registry::entries::{BuffEntry, BuffHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;

/// The class.
pub const TRANSFER_DEBUFFS: &str = "Buff_TransferDebuffs_C";

/// `transferDebuffs`: true when something was transferred (PopText).
fn transfer_debuffs(w: &mut World, f: FighterId, from: FighterId) -> bool {
    let mut pop_text = false;
    let list: Vec<u32> = w.fighter(from).buffs.buffs.iter().map(|b| b.serial).collect();
    for serial in list {
        let r = BuffRef { fighter: from, serial };
        let Some(x) = buff(w, r) else { continue };
        if x.frames <= 0.0 {
            continue; // IsValid @240: destroyed earlier in the loop (or ended in this frame)
        }
        let (cls, stacks, frames) = (x.cls, x.stacks, x.frames);
        if !inst_view(w, r).flag("IsDebuff") {
            continue; // BPI On Transfer Debuffs: ShouldTransfer
        }
        pop_text = true;
        // GetBuffRemainingDuration (BuffParentActor @0-114): the LifeTimer's remaining time while it is active, 0
        // without one (SelectFloat @32). spawn_buff sets frames = ticks + 1, so (frames - 1) ticks round-trips; on a
        // buff's last frame its timer is still active with a remainder under a tick, which is one tick here (0 would
        // read as no timer).
        let duration: Real = if frames == Real::INFINITY { 0.0 } else { max(frames - 1.0, 1.0) * DELTA_SECONDS };
        // @766: the Instigator's manager makes the call.
        spawn_buff(w, f, cls, duration, Some(to_int32(stacks)), Some(from), true, Some(from));
        end_buff(w, from, r); // @917
    }
    pop_text
}

/// ReceiveBeginPlay @1768 (the parent's first), on authority; the buff ends at once (@1617).
fn begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _duration: Option<Real>) {
    if let Some(from) = from
        && transfer_debuffs(w, f, from)
    {
        let id = w.fighter(f).id;
        w.fighter_mut(f).events.push(Event::PopText {
            fighter: id,
            kind: PopKind::Transferred,
            stacks: 0.0,
            gain: None,
            params: None,
        });
    }
    end_buff(w, f, b); // @1617
}

/// This file's registry entries (Buff_TransferDebuffs_C).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.buffs.enter(
        TRANSFER_DEBUFFS,
        BuffEntry::new(BuffClass::BuffTransferDebuffs, BuffHooks { begin: Some(begin), ..BuffHooks::default() }),
    )
}
