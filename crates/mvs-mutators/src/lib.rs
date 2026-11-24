//! The Chaos Wheel's mutators, the port's own list: each one's key, its tier and whether a match's Mutators row may
//! pick it too. The sim runs them (`mvs_sim::world::wheel`), the lobby rules carry a pool of them, the HUD and the
//! menus name and colour them. The list's order is the wheel's: a pool and the HUD's indexes are into [`ALL`].
//!
//! A key is the build's MutatorData_Map key where the mutator is one of the build's (reworked or not: the sim says
//! where it differs), and `Chaos_<Name>` where it is the port's own.

/// How much a mutator changes a match, shown by its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Changes the feel and keeps the fight readable: jumps, gravity, size.
    Mild,
    /// Changes how the fight is won: knockback, items, cooldowns.
    Wild,
    /// An event that flips the match. Rarer on the wheel (`ChaosRate`).
    Chaos,
}

/// One mutator of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mutator {
    pub key: &'static str,
    pub tier: Tier,
    /// Played only on the wheel: an event at a window's start, or a rule that needs the window's end. The Mutators
    /// row of a custom game and the Lab does not offer it.
    pub wheel_only: bool,
}

const fn m(key: &'static str, tier: Tier) -> Mutator {
    Mutator { key, tier, wheel_only: false }
}

const fn wheel_only(key: &'static str, tier: Tier) -> Mutator {
    Mutator { key, tier, wheel_only: true }
}

/// Every mutator the wheel may roll, in the wheel's order.
pub const ALL: &[Mutator] = &[
    m("Mutator_Giant", Tier::Mild),
    m("Mutator_Miniature", Tier::Mild),
    m("Mutator_BigHeadAbilityBoost", Tier::Wild),
    m("Mutator_LowGravity", Tier::Mild),
    m("Mutator_Boost_UnlimitedJumps", Tier::Wild),
    m("Mutator_Boost_CooldownReductionLarge", Tier::Wild),
    m("Mutator_Boost_AttacksBreakArmor", Tier::Wild),
    m("Mutator_Boost_WeakenOnHit", Tier::Wild),
    m("Mutator_Boost_ChargeFreeze", Tier::Wild),
    m("Mutator_Boost_IceGauntlets", Tier::Wild),
    m("Mutator_Boost_AdditionalAirSpecial", Tier::Mild),
    m("Mutator_GreyHealth", Tier::Mild),
    m("Mutator_RandomItemSpawns", Tier::Wild),
    m("Mutator_GrowWithDamage", Tier::Wild),
    m("Mutator_InstantKill", Tier::Chaos),
    m("Mutator_ShrinkBlastBoxLastPoint", Tier::Chaos),
    m("Chaos_MoonJump", Tier::Mild),
    m("Chaos_Featherweight", Tier::Wild),
    m("Chaos_Heavyweight", Tier::Mild),
    m("Chaos_Vampire", Tier::Mild),
    m("Chaos_DodgeFrenzy", Tier::Wild),
    // (the item area's spawner with a list of its own: the wheel's, as Random Items on the wheel is)
    wheel_only("Chaos_BombRain", Tier::Chaos),
    wheel_only("Chaos_ProjectileParty", Tier::Wild),
    m("Chaos_LavaFloor", Tier::Wild),
    m("Chaos_Ghosts", Tier::Wild),
];

/// The list's index of `key`.
pub fn find(key: &str) -> Option<usize> {
    ALL.iter().position(|m| m.key == key)
}

/// The entry of `key`.
pub fn get(key: &str) -> Option<&'static Mutator> {
    ALL.iter().find(|m| m.key == key)
}

/// The tier of `key`; a key not on the list is Wild.
pub fn tier_of(key: &str) -> Tier {
    get(key).map_or(Tier::Wild, |m| m.tier)
}

/// Every key of the list, the wheel's whole pool.
pub fn keys() -> impl Iterator<Item = &'static str> {
    ALL.iter().map(|m| m.key)
}

/// How many of the pool's Chaos mutators a round of the wheel's bag holds (the rest of the pool is always in it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChaosRate {
    /// None: the wheel never rolls one.
    Off,
    /// A quarter of them, at least one.
    Rare,
    /// Half of them, at least one.
    #[default]
    Normal,
    /// All of them.
    Often,
}

impl ChaosRate {
    pub const ALL: [ChaosRate; 4] = [ChaosRate::Off, ChaosRate::Rare, ChaosRate::Normal, ChaosRate::Often];

    /// How many of `n` Chaos mutators a round holds.
    pub fn of(self, n: usize) -> usize {
        match self {
            ChaosRate::Off => 0,
            ChaosRate::Rare => n.div_ceil(4),
            ChaosRate::Normal => n.div_ceil(2),
            ChaosRate::Often => n,
        }
    }

    /// The rate's name on the wire and in a trace's header.
    pub fn key(self) -> &'static str {
        match self {
            ChaosRate::Off => "off",
            ChaosRate::Rare => "rare",
            ChaosRate::Normal => "normal",
            ChaosRate::Often => "often",
        }
    }

    /// [`ChaosRate::key`]'s inverse.
    pub fn from_key(k: &str) -> Option<ChaosRate> {
        ChaosRate::ALL.into_iter().find(|r| r.key() == k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_listed_once() {
        for (i, m) in ALL.iter().enumerate() {
            assert_eq!(find(m.key), Some(i), "{}", m.key);
            assert!(m.key.starts_with("Mutator_") || m.key.starts_with("Chaos_"), "{}", m.key);
        }
    }

    #[test]
    fn a_rate_keeps_at_least_one_unless_off() {
        assert_eq!(ChaosRate::ALL.map(|r| r.of(0)), [0, 0, 0, 0]);
        assert_eq!(ChaosRate::ALL.map(|r| r.of(1)), [0, 1, 1, 1]);
        assert_eq!(ChaosRate::ALL.map(|r| r.of(6)), [0, 2, 3, 6]);
        for r in ChaosRate::ALL {
            assert_eq!(ChaosRate::from_key(r.key()), Some(r));
        }
    }
}
