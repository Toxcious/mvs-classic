//! The class registry: one table per family, keyed by the Blueprint class name (a
//! character's by its roster key, a stage's by its map), filled once by every module's `register` and read-only
//! after. A name is looked up once, at bind: a data row's class name resolves to
//! its entry (an enum value and fn pointers) when the World is made, and an unknown name is a [`BindError`] naming the
//! asset that holds it. A name entered twice is a [`RegistryError`] when the tables are built.

pub mod actor_notifies;
pub mod body_settings;
pub mod char_hooks;
pub mod entries;
pub mod notifies;
pub mod props;
pub mod signals;
pub mod spawner_notifies;

pub use char_hooks::CharHooks;
pub use entries::{
    ActorEntry, ActorHooks, BuffEntry, Condition, NotifyHandler, PickupEntry, StageActorEntry, StageEntry,
};

use indexmap::IndexMap;
use std::sync::OnceLock;

/// A name entered twice (`X is registered twice`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{family} {name} is registered twice")]
pub struct RegistryError {
    pub family: &'static str,
    pub name: &'static str,
}

/// A name the data holds that no module registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{family} {name} is not registered (named by {asset})")]
pub struct BindError {
    pub family: &'static str,
    pub name: String,
    /// The asset (or set-up field) that names it.
    pub asset: String,
}

/// One family's table, in the order its entries were entered.
#[derive(Debug)]
pub struct Table<V> {
    family: &'static str,
    map: IndexMap<&'static str, V>,
}

impl<V> Table<V> {
    pub fn new(family: &'static str) -> Self {
        Table { family, map: IndexMap::new() }
    }

    pub fn family(&self) -> &'static str {
        self.family
    }

    /// Enters `v` as `name`'s entry; a name already in the table is an error.
    pub fn enter(&mut self, name: &'static str, v: V) -> Result<(), RegistryError> {
        if self.map.contains_key(name) {
            return Err(RegistryError { family: self.family, name });
        }
        self.map.insert(name, v);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&V> {
        self.map.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// `name`'s entry at bind: an unknown name is an error naming `asset`.
    pub fn resolve(&self, name: &str, asset: &str) -> Result<&V, BindError> {
        self.map.get(name).ok_or_else(|| BindError {
            family: self.family,
            name: name.to_owned(),
            asset: asset.to_owned(),
        })
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.map.keys().copied()
    }
}

/// Every table. `BOT_TASKS` and `BOT_TREE_ROOTS` are the bots' port, not here yet.
#[derive(Debug)]
pub struct Registry {
    /// `CONDITIONS` and `COND_CLASSES`, one namespace.
    pub conditions: Table<Condition>,
    pub notifies: Table<NotifyHandler>,
    pub actors: Table<ActorEntry>,
    pub buffs: Table<BuffEntry>,
    /// `BUFF_GRAPHS_PORTED`: the buff classes whose graph is ported (their data-only default is off).
    pub buff_graphs_ported: Table<()>,
    /// By character key.
    pub pickups: Table<PickupEntry>,
    /// By character key, the roster's and the data-only ones.
    pub chars: Table<CharHooks>,
    /// By map.
    pub stages: Table<StageEntry>,
    pub stage_actors: Table<StageActorEntry>,
    /// The child classes of Buff_PolymorphParent (`combat::polymorph` `polymorphChildren`), each also in `buffs`.
    pub polymorph_children: Table<crate::combat::polymorph::PolymorphChild>,
    /// The classes of the Buff_DynamicMoveToActor chain with their overrides (`combat::dynamic_move`
    /// `dynamicMoveBuffs`), each also in `buffs`.
    pub dynamic_moves: Table<crate::combat::dynamic_move::DynamicMoveClass>,
    /// Buff_MoveToActor2's ReachedActor binders by name (`combat::move_to_actor_2` `moveToActor2Reached`).
    pub move_to_actor_2_reached: Table<crate::combat::move_to_actor_2::ReachedActorFn>,
    /// The functions a listener binds on a stage wall's delegates, by `"<Class>.<Function>"`
    /// (`world::wall_bp` `wallDelegateFns`).
    pub wall_listeners: Table<crate::world::wall_bp::WallListenerFn>,
}

impl Registry {
    pub fn empty() -> Self {
        Registry {
            conditions: Table::new("condition"),
            notifies: Table::new("notify"),
            actors: Table::new("actor"),
            buffs: Table::new("buff"),
            buff_graphs_ported: Table::new("buff graph"),
            pickups: Table::new("pickup"),
            chars: Table::new("character"),
            stages: Table::new("stage"),
            stage_actors: Table::new("stage actor"),
            polymorph_children: Table::new("polymorph child"),
            dynamic_moves: Table::new("dynamic move class"),
            move_to_actor_2_reached: Table::new("ReachedActor binder"),
            wall_listeners: Table::new("wall listener"),
        }
    }

    /// Every module's entries, in the order of the registration: the conditions, the spawner notifies
    /// and the notifies (the spawner notifies come first), the buffs and the other shared
    /// modules, then the characters in roster order, then the stages.
    pub fn build() -> Result<Self, RegistryError> {
        let mut r = Registry::empty();
        // the conditions' register goes here (the control track's `combat::conditions::register`).
        spawner_notifies::register(&mut r)?;
        notifies::register(&mut r)?;
        crate::fighter::anim::branches::register(&mut r)?; // AnimNotify_BlockSameAttackBranch
        crate::combat::register(&mut r)?; // the buffs and the other shared modules
        crate::chars::register(&mut r)?;
        crate::world::stage::register(&mut r)?;
        crate::world::catch_up::register(&mut r)?; // Buff_CatchUpSpeedBuff
        Ok(r)
    }
}

/// The registry, built on first use. A build error is a bug of the code (two modules entering one name), caught by
/// the crate's tests.
pub fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| Registry::build().unwrap_or_else(|e| panic!("the sim's registry: {e}")))
}
