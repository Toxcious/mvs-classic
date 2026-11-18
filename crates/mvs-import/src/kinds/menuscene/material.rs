//! A menu scene's material: the material kind's draw of it in the context the scene draws it with (a DrawRef), the
//! menuscene kind being one of the material kind's namers. The preshaders, layout,
//! collections and texture tables are the material kind's.
//!
//! The contexts: an engine Plane of a level (the backgrounds, the slants, the podium's planes) `local`; a
//! BP_Banner_PM piece through the banner kind's rule (`bannerlocal` where its build reads the custom stencil, else
//! `local`; one the probe refuses fails the entry); the preview glow's CharacterDissolve_MAT, drawn on duplicates of the
//! fighter's skinned meshes, `skin`; a banner's mask in its custom depth pass (the podium's mask, a PreMatch piece's
//! `customDepth`), `customdepth`.

use mvs_material::draw::context;
use mvs_material::menuscene::SceneMaterial;

use super::super::banner::stencil::{BannerProbe, BannerProbes, Plain, banner_ref, draw_ref};
use super::super::material::layout::material_layout;
use super::super::material::resolve::{material_shader_map, resolve_material};
use super::util::fail;
use crate::kind::{Cx, KindError};

/// The context a scene draws a material in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Draw {
    /// A static mesh component of a level.
    Plane,
    /// A BP_Banner_PM piece (custom stencil bound where its build reads it).
    Banner,
    /// A skinned mesh's duplicate (the glow shell).
    Skin,
    /// A mesh component's custom depth pass (a banner's mask writing its stencil value).
    CustomDepth,
}

/// The DrawRef of the material at `path` in `how`'s context; the material must resolve through its chain.
pub fn draw_of(cx: &Cx<'_>, path: &str, how: Draw) -> Result<String, KindError> {
    // (the material kind builds it; resolving it here fails the entry that names one it cannot)
    resolve_material(cx, path)?;
    let ctx = |n: &str| context(n).ok_or_else(|| fail(format!("no draw context {n}")));
    match how {
        Draw::Plane => draw_ref(path, ctx("local")?),
        Draw::Skin => draw_ref(path, ctx("skin")?),
        Draw::CustomDepth => draw_ref(path, ctx("customdepth")?),
        Draw::Banner => {
            let probes = cx.shared.get_or_init(BannerProbes::default);
            let (r, probe) = banner_ref(cx, &probes, path, Plain::Local)?;
            if let BannerProbe::Refused(why) = &*probe {
                return Err(fail(format!("banner material {path}: {why}")));
            }
            Ok(r)
        }
    }
}

/// The scene's material at `path`: its draw, nothing set over the material's own values.
pub fn scene_material(cx: &Cx<'_>, path: &str, how: Draw) -> Result<SceneMaterial, KindError> {
    Ok(SceneMaterial { draw: draw_of(cx, path, how)?, ..SceneMaterial::default() })
}

/// The material's named parameters (its layout's scalars then vectors).
pub fn parameter_names(cx: &Cx<'_>, path: &str) -> Result<Vec<String>, KindError> {
    let m = resolve_material(cx, path)?;
    let map = material_shader_map(cx, &m.shader_map)?;
    let layout = material_layout(cx, &m, &map)?;
    Ok(layout.scalar_params.into_iter().map(|p| p.0).chain(layout.vector_params.into_iter().map(|p| p.0)).collect())
}
