//! The shared ice actors (IceBlockItem's block and the other actors of
//! the sim's ice). IceBlockItem has no mesh: its block is a particle component, drawn by mvs-render's
//! fx/shared/ice.rs.

use crate::actors::MeshRule;

/// The classes' own mesh drawing, by class name.
pub(crate) const RULES: &[(&str, MeshRule)] = &[];
