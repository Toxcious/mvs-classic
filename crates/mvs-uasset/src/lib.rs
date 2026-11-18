//! UE 4.26 cooked packages of the beta (.uasset + .uexp): the summary, names, imports resolved through the packages
//! they name, exports, and each export's tagged properties and the native classes' data, as the object kind writes
//! them (`content/README.md` "The object kind").
//!
//! A port of CUE4Parse 1.2.2
//! (Apache-2.0, `LICENSE-CUE4Parse` beside this crate): its package, property, struct and native-class readers,
//! changed to read only what an unversioned UE 4.26 cooked package holds. Values are read
//! straight into the object kind's values ([`value::Value`]), not into cue4dump's JSON first.
//!
//! Modules and who reads what:
//! - `archive`: the byte reader, the error kinds, GUIDs.
//! - `package`: [`package::Package`] (summary, names, imports, exports), [`package::Resolved`] (an index resolved,
//!   imports through the package they name), [`package::PackageCache`] (packages read from the paks, bounded).
//! - `objects`: [`objects::UObj`], the export as read: the C# type CUE4Parse builds it as, its native chain, its
//!   tagged properties and native data; `classes` the registry of native chains; `versions`, `flags`, `bulk`.
//! - `props`, `structs`, `fields`: tagged properties, native structs, a class's or struct's reflected fields.
//! - `natives`: the native classes' Deserialize and WriteJson, and the heavy kinds' native readers (skeleton, meshes,
//!   animations, lighting, shader maps), which the object kind leaves partial.
//! - `typed`: an export as an [`typed::AssetObject`] (the object kind writes its JSON).
//! - `locres`: a .locres file's strings.

pub mod archive;
pub mod bulk;
pub mod classes;
pub mod fields;
pub mod flags;
pub mod locres;
pub mod natives;
pub mod objects;
pub mod package;
pub mod props;
pub mod structs;
pub mod typed;
pub mod value;
pub mod versions;

pub use archive::ReadError;
pub use package::{Package, PackageCache, Resolved};
pub use value::{Props, Value};
