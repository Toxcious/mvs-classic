//! The `material` and `shader` kinds of the developer's store, read the way the renderer will: every material entry
//! decodes; for the materials Shaggy (his skins, his effects) and the Lab (Training Room Small's scenery, lights and
//! items) name, every code a row names is a `shader` entry whose DXBC container parses, whose resource table binds
//! the Material buffer's resources the entry lists, and every vertex, pixel, geometry, hull and domain shader is
//! created on a D3D11 WARP device. A tree with no store, or a store with no material kind, says so and passes.

use std::collections::{BTreeMap, BTreeSet};

use mvs_material::material::TextureBind;
use mvs_material::{Entry, Frequency, GLOBAL_SHADER_CACHE, Sha1, ShaderCode, hashed_name, read_code, read_entry};
use mvs_store::{Pack, Store, dev_store_dir};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};

fn store() -> Option<(Store, std::sync::Arc<Pack>, std::sync::Arc<Pack>)> {
    let dir = dev_store_dir();
    let store = match Store::open(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no store at {} ({e}): nothing to check", dir.display());
            return None;
        }
    };
    let (Ok(materials), Ok(shaders)) = (store.kind("material"), store.kind("shader")) else {
        eprintln!("the store at {} has no material or shader kind (run the import): nothing to check", dir.display());
        return None;
    };
    Some((store, materials, shaders))
}

/// The materials the entries of `kind` whose paths `pick` takes name: every `<material>?ctx=<context>` string in them.
fn named_materials(store: &Store, kind: &str, pick: impl Fn(&str) -> bool, out: &mut BTreeSet<String>) -> usize {
    let pack = store.kind(kind).unwrap_or_else(|e| panic!("{e}"));
    let mut n = 0;
    for path in pack.paths().filter(|p| pick(p)).map(str::to_string).collect::<Vec<_>>() {
        n += 1;
        let bytes = pack.bytes(&path).unwrap();
        let mut at = 0;
        while let Some(q) = bytes[at..].windows(5).position(|w| w == b"?ctx=").map(|q| q + at) {
            at = q + 5;
            let Some(open) = bytes[..q].iter().rposition(|&b| b == b'"') else { continue };
            let material = String::from_utf8_lossy(&bytes[open + 1..q]).into_owned();
            if material.starts_with('/') && material.contains('.') {
                out.insert(material);
            }
        }
    }
    n
}

fn warp() -> ID3D11Device {
    let mut dev = None;
    // SAFETY: every pointer argument is None or a live local (the feature level slice, `dev`); a null adapter with
    // D3D_DRIVER_TYPE_WARP and a null software module is the documented WARP form.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut dev),
            None,
            None,
        )
        .expect("D3D11CreateDevice (WARP)");
    }
    dev.unwrap()
}

/// Creates the code's shader on the device; None for a frequency D3D11 has no Create* of here (compute, ray).
fn create(dev: &ID3D11Device, code: &ShaderCode) -> Option<windows::core::Result<()>> {
    let b = &code.dxbc;
    // SAFETY: `b` is a live byte slice for the call's duration (D3D copies the bytecode), no class linkage, and the
    // out parameter is a live local Option the call writes and drops at the end of the statement.
    unsafe {
        Some(match code.frequency {
            Frequency::Vertex => dev.CreateVertexShader(b, None, Some(&mut None)),
            Frequency::Pixel => dev.CreatePixelShader(b, None, Some(&mut None)),
            Frequency::Geometry => dev.CreateGeometryShader(b, None, Some(&mut None)),
            Frequency::Hull => dev.CreateHullShader(b, None, Some(&mut None)),
            Frequency::Domain => dev.CreateDomainShader(b, None, Some(&mut None)),
            Frequency::Compute => dev.CreateComputeShader(b, None, Some(&mut None)),
            Frequency::RayTracing(_) => return None,
        })
    }
}

#[test]
fn every_entry_decodes() {
    let Some((_, materials, _)) = store() else { return };
    let mut n = 0;
    let mut pairs = 0;
    let mut overrides = 0;
    for path in materials.paths() {
        match read_entry(&materials, path).unwrap_or_else(|e| panic!("{e}")) {
            Entry::Material(m) => {
                n += 1;
                assert_eq!(m.path, path);
                let rows = m.uniform_rows(&m.default_inputs()).unwrap_or_else(|e| panic!("{e}"));
                // (the entry is JSON, which writes -0 as 0)
                let got: Vec<u64> = rows.iter().flatten().map(|v| (v + 0.0).to_bits()).collect();
                let want: Vec<u64> = m.uniforms.values.iter().map(|v| v.to_bits()).collect();
                assert_eq!(got, want, "{path}: the preshaders' rows against the import's");
                pairs += m.shaders.vertex_factories().len();
                // a bOverride_TwoSided set with no TwoSided beside it: the struct's false, not the parent's true
                if path.ends_with("/HammerTrail01_MIC.HammerTrail01_MIC")
                    || path.ends_with("/WonderWomanShield_MIC.WonderWomanShield_MIC")
                {
                    assert!(!m.state.two_sided, "{path}");
                    overrides += 1;
                }
            }
            Entry::Global(g) => assert_eq!(path, GLOBAL_SHADER_CACHE, "{} rows", g.shaders.rows.len()),
        }
    }
    eprintln!("{n} materials decode, {pairs} vertex factory maps, every uniform row as the import evaluated it");
    assert!(n > 0 && overrides == 2);
}

#[test]
fn global_shaders_by_name() {
    let Some((_, materials, shaders)) = store() else { return };
    let Entry::Global(g) = read_entry(&materials, GLOBAL_SHADER_CACHE).unwrap() else { panic!("not the global entry") };
    let dev = warp();
    // the permutations trial-light draws with
    for (name, perm, freq) in [
        ("FTonemapPS", 521, Frequency::Pixel),
        ("FDeferredLightPS", 48, Frequency::Pixel),
        ("TDeferredLightVS<true>", 0, Frequency::Vertex),
        ("TDeferredLightVS<false>", 0, Frequency::Vertex),
    ] {
        let row = g
            .find(name, perm, freq)
            .unwrap_or_else(|| panic!("no global {name} permutation {perm}: {:?}", g.permutations(name, freq)));
        let code = read_code(&shaders, g.shaders.code(row)).unwrap_or_else(|e| panic!("{name}: {e}"));
        create(&dev, &code).unwrap().unwrap_or_else(|e| panic!("{name} {perm}: {e}"));
    }
    for (name, freq) in [
        ("FTonemapPS", Frequency::Pixel),
        ("FTonemapVS", Frequency::Vertex),
        ("FDeferredLightPS", Frequency::Pixel),
        ("FSlateElementVS", Frequency::Vertex),
        // IMPLEMENT_SLATE_PIXELSHADER_TYPE's typedef: TSlateElementPS<ShaderType><bDrawDisabledEffect><bUseTextureAlpha>A
        ("TSlateElementPSDefaultfalsetrueA", Frequency::Pixel),
        ("FLUTBlenderPS", Frequency::Pixel),
        ("FFXAAPS", Frequency::Pixel),
        ("FScreenVS", Frequency::Vertex),
        ("FScreenPS", Frequency::Pixel),
    ] {
        let perms = g.permutations(name, freq);
        assert!(!perms.is_empty(), "no global {name}");
        let row = g.find(name, perms[0], freq).unwrap();
        let code = read_code(&shaders, g.shaders.code(row)).unwrap_or_else(|e| panic!("{name}: {e}"));
        code.container().unwrap_or_else(|e| panic!("{name}: {e}"));
        create(&dev, &code).unwrap().unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn shaggy_and_the_lab_create_on_warp() {
    let Some((store, materials, shaders)) = store() else { return };
    let mut named = BTreeSet::new();
    let lab = |p: &str| p.to_ascii_lowercase().contains("trainingroomsmall");
    let sources = [
        named_materials(&store, "fighter", |p| p.to_ascii_lowercase().ends_with("/shaggy.shaggy"), &mut named),
        named_materials(&store, "particles", |p| p.to_ascii_lowercase().contains("/characters/shaggy/"), &mut named),
        named_materials(&store, "scenery", lab, &mut named),
        named_materials(&store, "stagelight", lab, &mut named),
    ];
    // The native `stage` kind holds only the Items entry (no material names), so it is not a source.
    assert!(sources.iter().all(|&n| n > 0), "source entries per kind: {sources:?}");
    assert!(named.len() >= 50, "only {} materials named", named.len());
    let material_hash = hashed_name("Material");
    let dev = warp();
    let (mut pairs, mut created, mut codes_seen) = (0usize, BTreeMap::<&str, usize>::new(), BTreeSet::<Sha1>::new());
    let mut failures = Vec::new();
    let mut slots = 0;
    let mut not_material = Vec::new();
    for path in &named {
        if !materials.has(path) {
            not_material.push(path.clone());
            continue;
        }
        let Entry::Material(m) = read_entry(&materials, path).unwrap_or_else(|e| panic!("{e}")) else { unreachable!() };
        let mut vf_pass = BTreeSet::new();
        let ntex = m.uniforms.textures.len();
        for t in &m.uniforms.textures {
            if let TextureBind::Texture { path: tp, .. } = &t.texture {
                assert!(store.kind("texture").unwrap().has(tp), "{path}: texture {tp} is not a texture entry");
            }
        }
        for row in &m.shaders.rows {
            vf_pass.insert((row.map, row.shader_type));
            let sha = m.shaders.code(row);
            let code = read_code(&shaders, sha).unwrap_or_else(|e| panic!("{path}: {e}"));
            let params = m.shaders.params(row);
            assert_eq!(code.frequency, row.frequency, "{path}: code {sha}'s frequency");
            code.container().unwrap_or_else(|e| panic!("{path}: code {sha}: {e}"));
            // the Material buffer's register ('u' names) is one the FShader's parameter map binds (the record's
            // UniformBufferParameterStructs holds the globally named structs, View and Primitive; Material is
            // FMaterialShader's own MaterialUniformBuffer parameter)
            let reg = code.buffer_register("Material");
            let bound = |r: u32| params.parameter_map.uniform_buffers.iter().any(|p| p.base_index == r);
            assert!(reg.is_none_or(bound), "{path}: code {sha}'s Material buffer register {reg:?} is not in its map");
            assert!(!params.uniform_buffers.iter().any(|u| u.0 == material_hash && Some(u.1) != reg));
            for &(_, r) in &params.uniform_buffers {
                assert!(bound(r), "{path}: code {sha}: uniform buffer register {r} is not in its map");
            }
            let t = &code.resource_table;
            for (map, sampler) in [(&t.texture_map, false), (&t.srv_map, false), (&t.sampler_map, true)] {
                for s in t.slots(map).unwrap_or_else(|e| panic!("{path}: code {sha}: {e}")) {
                    if Some(s.buffer as u32) != reg {
                        continue;
                    }
                    let k = s.resource as usize;
                    slots += 1;
                    // resources: (texture, sampler) per texture parameter, then the Wrap and Clamp samplers
                    let ok = if sampler {
                        !k.is_multiple_of(2) && k / 2 < ntex || k >= 2 * ntex
                    } else {
                        k.is_multiple_of(2) && k / 2 < ntex
                    };
                    assert!(
                        ok && k < m.uniforms.resources.len(),
                        "{path}: code {sha} binds Material resource {k} as a {} ({ntex} textures, {} resources)",
                        if sampler { "sampler" } else { "texture" },
                        m.uniforms.resources.len()
                    );
                }
            }
            if codes_seen.insert(sha)
                && !code.no_create
                && let Some(r) = create(&dev, &code)
            {
                let f = match row.frequency {
                    Frequency::Vertex => "VS",
                    Frequency::Pixel => "PS",
                    Frequency::Geometry => "GS",
                    Frequency::Hull => "HS",
                    Frequency::Domain => "DS",
                    _ => "CS",
                };
                match r {
                    Ok(()) => *created.entry(f).or_default() += 1,
                    Err(e) => failures.push(format!("{path}: {f} {sha}: {e}")),
                }
            }
        }
        pairs += vf_pass.len();
    }
    eprintln!(
        "{} materials named ({} not material entries: {not_material:?}), {pairs} (map, shader type) pairs, {slots} Material resource slots, {} codes, created on WARP: {created:?}",
        named.len(),
        not_material.len(),
        codes_seen.len()
    );
    assert!(failures.is_empty(), "{} failed: {failures:#?}", failures.len());
    assert!(created.get("VS").is_some_and(|&n| n > 0) && created.get("PS").is_some_and(|&n| n > 0), "{created:?}");
    assert!(slots > 0);
    assert!(not_material.is_empty(), "named materials with no entry: {not_material:?}");
}

/// A depth pass of Shaggy's skin takes the DepthNoColorOutputPipeline's VS and PS (the exe's type names), which are
/// other codes than the standalone TDepthOnlyVS<false> / FDepthOnlyPS<false> rows of the same map; a pass with no pipeline in the map takes the standalone ones.
#[test]
fn depth_pass_takes_the_pipeline() {
    let Some((store, materials, _)) = store() else { return };
    let mut named = BTreeSet::new();
    named_materials(&store, "fighter", |p| p.to_ascii_lowercase().ends_with("/shaggy.shaggy"), &mut named);
    let vf = "TGPUSkinVertexFactoryDefault";
    let types = [("TDepthOnlyVS<false>", 0), ("FDepthOnlyPS<false>", 0)];
    let (mut with_pipeline, mut differ) = (0, 0);
    for path in named.iter().filter(|p| p.contains("/Characters/Shaggy/")) {
        let Entry::Material(m) = read_entry(&materials, path).unwrap() else { unreachable!() };
        let Some((pipeline, rows)) = m.shaders.pass(Some(vf), Some("DepthNoColorOutputPipeline"), &types) else {
            continue;
        };
        let Some(p) = pipeline else { continue };
        with_pipeline += 1;
        assert_eq!(p, hashed_name("DepthNoColorOutputPipeline"));
        assert!(rows.iter().all(|r| r.pipeline == p && r.map == hashed_name(vf)), "{path}");
        assert_eq!(rows[0].frequency, Frequency::Vertex);
        assert_eq!(rows[1].frequency, Frequency::Pixel);
        let (none, alone) = m.shaders.pass(Some(vf), None, &types).expect("the standalone depth shaders");
        assert!(none.is_none() && alone.iter().all(|r| r.pipeline == 0));
        if rows.iter().zip(&alone).any(|(a, b)| m.shaders.code(a) != m.shaders.code(b)) {
            differ += 1;
        }
    }
    eprintln!(
        "{with_pipeline} Shaggy materials with a skinned DepthNoColorOutputPipeline, {differ} with other codes than the standalone rows"
    );
    assert!(with_pipeline > 0 && differ > 0);
}
