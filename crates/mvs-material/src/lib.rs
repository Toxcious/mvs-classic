//! What the D3D11 renderer draws a material with, as the import resolved it: the `material` kind (a material's
//! cooked FShaders per vertex factory and pass, its uniform buffer layout and uniform expression set, its parameters
//! merged down the instance chain, its textures and draw state; and the global shader cache's rows by type name and
//! permutation), the `shader` kind's codes (the DXBC container and its FD3D11ShaderResourceTable) and the preshader
//! evaluator that fills the material uniform buffer; the draw contexts and DrawRefs (`draw`); the `menuscene` kind's
//! entries (`menuscene`), read by the renderer and written by the importer.

pub mod draw;
pub mod dxbc;
pub mod hash;
pub mod jsonbin;
pub mod material;
pub mod menuscene;
pub mod preshader;
pub mod shader;

pub use hash::hashed_name;
pub use material::{
    Entry, GLOBAL_SHADER_CACHE, GlobalShaders, MATERIAL_FORMAT_VERSION, Material, MaterialTexture, ShaderParams,
    ShaderRow, ShaderSet, TextureBind,
};
pub use preshader::PreshaderInputs;
pub use shader::{Dxbc, Frequency, SHADER_CODE_PREFIX, Sha1, ShaderCode};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An entry that does not read as its format says.
    #[error("{what}: {why}")]
    Format { what: String, why: String },
    #[error(transparent)]
    Store(#[from] mvs_store::StoreError),
}

impl Error {
    pub fn format(what: &str, why: String) -> Error {
        Error::Format { what: what.to_string(), why }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The `material` kind's entry at `path` (a material's object path, or `GLOBAL_SHADER_CACHE`) in its pack.
pub fn read_entry(materials: &mvs_store::Pack, path: &str) -> Result<Entry> {
    Entry::decode(&materials.bytes(path)?, path)
}

/// The `shader` kind's code `sha` in its pack.
pub fn read_code(shaders: &mvs_store::Pack, sha: Sha1) -> Result<ShaderCode> {
    let path = sha.path();
    ShaderCode::decode(&shaders.bytes(&path)?, &path)
}
