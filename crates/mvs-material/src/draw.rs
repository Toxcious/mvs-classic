//! The draw contexts: per context the vertex factory and the pixel shader type a material is drawn with there, and the
//! group of builders that made its draws ; and the DrawRef, a
//! material in a draw context as the game's data names it: the material's build object path and
//! `?ctx=<context>`. Read by the importer (the material list, the stage draws' vertex factories) and the renderer.

/// Which builders made a context's draws: effects, lit or slate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawGroup {
    Effects,
    Lit,
    Slate,
}

/// One draw context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawContextRow {
    /// Its name, a DrawRef's `?ctx=`.
    pub name: &'static str,
    pub group: DrawGroup,
    /// The vertex factory type as the build names it; `None` for a material shader.
    pub vertex_factory: Option<&'static str>,
    /// The base pass pixel shader type; `None` where it is the surface's light-map policy.
    pub pixel_shader: Option<&'static str>,
    /// A material with Refraction connected also draws FDistortionMeshPS with this vertex factory.
    pub distortion: bool,
    /// The custom stencil is the image the renderer binds (the banners'), else read as 0.
    pub stencil_bind: bool,
}

/// The base pass with no light map.
const NO_LIGHTMAP: Option<&str> = Some("TBasePassPSFNoLightMapPolicy");

/// The distortion pass's pixel shader type.
pub const DISTORTION_PS: &str = "FDistortionMeshPS";

const fn row(
    name: &'static str,
    group: DrawGroup,
    vertex_factory: Option<&'static str>,
    pixel_shader: Option<&'static str>,
    distortion: bool,
    stencil_bind: bool,
) -> DrawContextRow {
    DrawContextRow { name, group, vertex_factory, pixel_shader, distortion, stencil_bind }
}

/// The contexts, in the draw kind's order, then the port's own (`customdepth`).
pub const CONTEXTS: [DrawContextRow; 15] = {
    use DrawGroup::{Effects, Lit, Slate};
    let local = Some("FLocalVertexFactory");
    let sprite = Some("FParticleSpriteVertexFactory");
    [
        // a sprite emitter's material (the GPU sprite factory's shaders are the same code)
        row("sprite", Effects, sprite, NO_LIGHTMAP, true, false),
        // a mesh emitter's
        row("mesh", Effects, Some("FMeshParticleVertexFactory"), NO_LIGHTMAP, true, false),
        // a ribbon / beam emitter's
        row("trail", Effects, Some("FParticleBeamTrailVertexFactory"), NO_LIGHTMAP, true, false),
        // a mesh component drawn by the effects renderer
        row("local", Effects, local, NO_LIGHTMAP, true, false),
        // no lightmap (the scenery's plain key); its policy by the light-map rule
        row("locallm", Lit, local, None, false, false),
        // scenery: a static mesh with a texture lightmap
        row("static", Lit, local, None, false, false),
        // scenery: a static mesh with a lightmap and a shadow map texture
        row("shadowmap", Lit, local, None, false, false),
        // scenery: a spline mesh component
        row("spline", Lit, Some("FSplineMeshVertexFactory"), None, false, false),
        // a character's skinned mesh
        row("skin", Lit, Some("TGPUSkinVertexFactoryDefault"), None, false, false),
        // a landscape section
        row("landscape", Lit, Some("FLandscapeVertexFactory"), None, false, false),
        // a deferred decal (a material shader)
        row("decal", Effects, None, Some("FDeferredDecalPS"), false, false),
        // a HUD or menu material (a material shader)
        row("slate", Slate, None, Some("TSlateMaterialShaderPSDefault"), false, false),
        // a banner element's mesh component and sprite emitter, the custom stencil bound
        row("bannerlocal", Effects, local, NO_LIGHTMAP, true, true),
        row("bannersprite", Effects, sprite, NO_LIGHTMAP, true, true),
        // a mesh component's custom depth pass (bRenderCustomDepth: a banner's mask writing its
        // CustomDepthStencilValue), the custom stencil read as 0; it has no
        // row of its own in the draw kind
        row("customdepth", Effects, local, NO_LIGHTMAP, false, false),
    ]
};

/// The context named `name`.
pub fn context(name: &str) -> Option<&'static DrawContextRow> {
    CONTEXTS.iter().find(|c| c.name == name)
}

/// A DrawRef's parts: the material's object path and its context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawRef<'a> {
    pub material: &'a str,
    pub context: &'static DrawContextRow,
}

/// The material and the context of a DrawRef (split at the last `?ctx=`; the material
/// starts with `/`, holds a `.` and no `?`; the context is one of [`CONTEXTS`]); `None` for a string that is not one.
pub fn parse_draw_ref(s: &str) -> Option<DrawRef<'_>> {
    let q = s.rfind("?ctx=")?;
    let material = &s[..q];
    if !material.starts_with('/') || !material.contains('.') || material.contains('?') {
        return None;
    }
    Some(DrawRef { material, context: context(&s[q + 5..])? })
}

/// Whether a string is a DrawRef.
pub fn is_draw_ref(s: &str) -> bool {
    parse_draw_ref(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contexts_in_order() {
        let names: Vec<&str> = CONTEXTS.iter().map(|c| c.name).collect();
        assert_eq!(
            names,
            [
                "sprite",
                "mesh",
                "trail",
                "local",
                "locallm",
                "static",
                "shadowmap",
                "spline",
                "skin",
                "landscape",
                "decal",
                "slate",
                "bannerlocal",
                "bannersprite",
                "customdepth"
            ]
        );
        assert_eq!(context("decal").unwrap().vertex_factory, None);
        assert_eq!(context("skin").unwrap().group, DrawGroup::Lit);
        assert!(context("bannersprite").unwrap().stencil_bind);
    }

    #[test]
    fn draw_refs() {
        let r = parse_draw_ref("/Game/A/M.M?ctx=local").unwrap();
        assert_eq!((r.material, r.context.name), ("/Game/A/M.M", "local"));
        // the last `?ctx=` splits; a material holding a `?` is none
        assert!(parse_draw_ref("/Game/A/M.M?ctx=x?ctx=local").is_none());
        for bad in ["/Game/A/M?ctx=local", "Game/A/M.M?ctx=local", "/Game/A/M.M?ctx=nope", "/Game/A/M.M", ""] {
            assert!(!is_draw_ref(bad), "{bad}");
        }
    }
}
