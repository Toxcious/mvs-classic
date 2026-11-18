//! The bots: the BehaviorTree runtime that runs the build's bot trees ([`bt`]), the Blueprint
//! node classes it runs ([`tasks`], [`moves`], [`decorators`], [`services`], the characters' own in
//! `chars/<key>/bots.rs`, all listed in [`classes`]), and what they share: the blackboard, the pad a bot's tasks
//! write its input on, the node's context and the reads of the world.
//!
//! A module of the sim, not a crate of its own: the nodes write the World (BotTaunt's BFTaunt on its pawn, the
//! sweep's `World::blockers`, a minion's tree that runs inside the step), so they need the sim's types and calls
//! as the fighter's own code does.

pub mod blackboard;
pub mod bt;
pub mod classes;
pub mod cx;
pub mod decorators;
pub mod expert;
pub mod log;
pub mod moves;
pub mod pad;
pub mod props;
mod runner;
#[cfg(test)]
mod runner_tests;
pub mod services;
pub mod sweep;
#[cfg(test)]
mod task_tests;
pub mod tasks;
mod tree;
pub mod util;
