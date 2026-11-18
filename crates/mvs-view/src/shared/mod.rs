//! The shared classes' render modules on the view side (the ones that enter the actor meshes):
//! a class any character's perk or buff spawns, its mesh rules after the characters' (crate::chars::RULES).

mod ice;

/// Every shared module's mesh rules, after the characters' (resolved once into crate::actors::mesh_rule's table).
pub(crate) static RULES: [&[(&str, crate::actors::MeshRule)]; 1] = [ice::RULES];
