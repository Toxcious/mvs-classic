//! Native classes: what CUE4Parse's C# class reads after (or instead of) the tagged properties, and what its
//! WriteJson adds. Keyed by C# class name as `classes` lists the chains. A class on an export's chain that has no
//! entry here is not ported: the object reads as far as the ported classes go and is `partial`. Ports
//! `content/src/native/`.

pub mod assets;
pub mod class_struct;
pub mod components;
pub mod curves;
pub mod material;
pub mod texture;
pub mod wwise;
pub mod wwise_bank;

// The heavy kinds' readers (skeleton, mesh, animation, lighting, shader maps): never in the NATIVES table, the object
// kind keeps these classes partial.
pub mod anim_codec;
pub mod anim_sequence;
pub mod frozen;
pub mod lighting;
pub mod mesh_buffers;
pub mod skeletal_mesh;
pub mod skeleton;
pub mod static_mesh;

use crate::archive::Result;
use crate::objects::{ExportArchive, UObj};
use crate::value::Props;

/// A native class's Deserialize (see [`Native::deserialize`]).
pub type DeserializeFn = fn(obj: &mut UObj, ar: &mut ExportArchive<'_>, valid_pos: usize, level: usize) -> Result<()>;

/// One native class.
pub struct Native {
    /// C#'s Deserialize override; `level` is this class's index in `obj.chain`, so it calls
    /// `objects::super_deserialize(obj, ar, valid_pos, level + 1)` first. `valid_pos`: the export's end in `ar`.
    pub deserialize: Option<DeserializeFn>,
    /// What its WriteJson writes after its base's fields (run from the base up). On a partial object it writes only
    /// what was read: a field filled by a class above the cut is not written, not defaulted.
    pub json: Option<fn(obj: &UObj, out: &mut Props)>,
    /// What the object kind writes beyond CUE4Parse's WriteJson: binary data the class holds inline, as
    /// `Value::Bytes` (a section of the entry's body). Run base first, as json is.
    pub extra: Option<fn(obj: &UObj, out: &mut Props)>,
}

/// The native class of a C# class name, if the port reads it.
pub fn native(class: &str) -> Option<&'static Native> {
    TABLES.iter().flat_map(|t| t.iter()).find(|(name, _)| *name == class).map(|(_, n)| n)
}

static TABLES: &[&[(&str, Native)]] = &[
    class_struct::NATIVES,
    assets::NATIVES,
    material::NATIVES,
    curves::NATIVES,
    texture::NATIVES,
    components::NATIVES,
    wwise::NATIVES,
];
