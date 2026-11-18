use super::*;
use mvs_material::menuscene::SceneMaterial;

fn piece(name: &str, loc: V3) -> BannerPiece {
    BannerPiece {
        loc,
        rot: [0.0, -90.0, 90.0],
        scale: [6.0; 3],
        material: SceneMaterial { draw: format!("/Game/{name}.{name}?ctx=bannerlocal"), ..SceneMaterial::default() },
    }
}

fn parts(nameplate: bool) -> BannerParts {
    BannerParts {
        mask: piece("Mask", [0.0; 3]),
        background: piece("Background", [0.0, -1.0, 0.0]),
        filter: piece("Filter", [0.0, -0.5, 0.0]),
        trim: piece("Trim", [0.0, 0.5, 0.0]),
        nameplate: nameplate.then(|| piece("Nameplate", [0.0, 1.0, 0.0])),
        slant_angle: 1.0,
        entrance_length: 1.0,
    }
}

fn scalar(m: &MaterialRef, n: &str) -> Option<f32> {
    m.scalars.iter().find(|(k, _)| &**k == n).map(|s| s.1)
}

#[test]
fn view_hidden_until_start_and_drops_in() {
    let mut v = BannerView::new(parts(true), [10.0, 20.0, 30.0], 1.0, 3, 2);
    assert_eq!(v.pieces.len(), 5);
    assert_eq!(v.writer(), 4);
    let (mut out, mut cd) = (Vec::new(), Vec::new());
    v.meshes(&mut out);
    v.custom_depth(&mut cd);
    assert!(out.is_empty() && cd.is_empty());
    assert!(!v.tick(0.05), "an unstarted banner does not play");

    // the values the pieces read
    assert_eq!(scalar(&v.pieces[PIECE_BACKGROUND].material, "StencilValue"), Some(3.0));
    assert_eq!(scalar(&v.pieces[PIECE_BACKGROUND].material, "ColorType"), Some(2.0));
    assert_eq!(scalar(&v.pieces[PIECE_TRIM].material, "UseTrim"), Some(1.0));
    assert_eq!(scalar(&v.pieces[PIECE_TRIM].material, "Angle"), Some(1.0));
    assert_eq!(scalar(&v.pieces[v.writer()].material, "Angle"), Some(1.0));

    v.start();
    v.icon = Some("/Game/Icon.Icon".into());
    assert!(!v.tick(0.05));
    assert!((v.lift - 250.0).abs() < 1e-9, "half way at rate 10: {}", v.lift);
    v.meshes(&mut out);
    v.custom_depth(&mut cd);
    assert_eq!(out.len(), 4);
    assert_eq!(cd.len(), 1);
    assert_eq!(cd[0].custom_stencil, 3);
    assert!(out.iter().all(|(m, p)| m.custom_stencil == 0 && *p == 0));
    let nameplate = &out[PIECE_NAMEPLATE].0.materials[0];
    assert!(nameplate.textures.iter().any(|(n, t)| &**n == PROFILE_TEXTURE && &**t == "/Game/Icon.Icon"));
    // the mask's place: start + lift
    assert!((cd[0].to_world[11] - 280.0).abs() < 1e-3, "{:?}", cd[0].to_world);

    assert!(v.tick(0.05), "lands at the end of the timeline");
    assert_eq!(v.lift, 0.0);
    assert!(!v.tick(0.05), "lands once");
    v.shown[PIECE_FILTER] = false;
    out.clear();
    v.meshes(&mut out);
    assert_eq!(out.len(), 3);
}

/// Every banner of the dev store, both slants: builds, shows and hides, follows the parallax, releases.
#[test]
fn every_store_banner_builds() {
    let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else {
        eprintln!("skip: no dev store");
        return;
    };
    let Ok(index) = entry::load_index(&store) else {
        eprintln!("skip: no banner kind in the dev store");
        return;
    };
    assert!(!index.banners.is_empty());
    let root = [[-200.0, 350.0, -85.0], [0.0, 0.0, 0.0], [1.0; 3]];
    let cam = [2250.0, 0.0, 250.0];
    for slug in index.banners.keys() {
        let data = entry::load_banner(&store, &index, slug).unwrap().unwrap();
        for slant in [0.0, 1.0] {
            let init = BannerInit { banner_index: 2, color_type: 1, slant_angle: slant };
            let mut rng = mvs_vfx::RandomStream::new(7);
            let mut el = BannerElement::of_entry(data.clone(), init, root, cam, None, &mut |_| true, &mut rng);
            el.show(true);
            let mut a = Vec::new();
            el.meshes(&mut a);
            for (m, _) in &a {
                assert!(m.to_world.iter().all(|x| x.is_finite()), "{slug}: {:?}", m.to_world);
                // a masked mesh reads the banner's stencil
                if let Some(s) = scalar(&m.materials[0], "StencilValue") {
                    assert_eq!(s, 2.0, "{slug}");
                }
            }
            el.show(false);
            let mut hidden = Vec::new();
            el.meshes(&mut hidden);
            assert!(hidden.len() <= a.len(), "{slug}");
            el.show(true);

            // a cursor in the corner turns Parallax Pivot's children only
            el.parallax([0.0, 0.0], cam);
            el.root_w = Xf::of(root);
            if let Some((c, l)) = el.look.take() {
                el.apply_look(c, l);
            }
            el.relayout();
            assert!(el.pivot_w.q.iter().all(|x| x.is_finite()), "{slug}");
            let mut b = Vec::new();
            el.meshes(&mut b);
            assert_eq!(a.len(), b.len(), "{slug}");
            // the root's own children stay where they were
            for (comp, ((ma, _), (mb, _))) in
                el.comps.iter().filter(|c| c.draw == Draw::Mesh && !c.hidden).zip(a.iter().zip(&b))
            {
                if comp.parent == Parent::Root {
                    assert_eq!(ma.to_world, mb.to_world, "{slug}");
                }
            }
            // release: nothing drawn after
            el.released = true;
            for comp in &mut el.comps {
                comp.draw = Draw::None;
            }
            let mut c = Vec::new();
            el.meshes(&mut c);
            assert!(c.is_empty());
        }
    }
}

#[test]
fn find_between_normals_turns_a_onto_b() {
    let b = [0.0, 0.6, 0.8];
    let q = find_between_normals([1.0, 0.0, 0.0], b);
    let r = qrot(q, [1.0, 0.0, 0.0]);
    for i in 0..3 {
        assert!((r[i] - b[i]).abs() < 1e-12, "{r:?}");
    }
    // opposite vectors: a half turn
    let q = find_between_normals([1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    let r = qrot(q, [1.0, 0.0, 0.0]);
    assert!((r[0] + 1.0).abs() < 1e-12, "{r:?}");
}

#[test]
fn compose_has_no_shear() {
    let parent = Xf::of([[1.0, 2.0, 3.0], [0.0, 90.0, 0.0], [2.0, 1.0, 1.0]]);
    let child = Xf::of([[1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0; 3]]);
    let w = parent.compose(&child);
    // the child's 1 cm along X, scaled 2 by the parent, turned 90 degrees to +Y
    assert!(
        (w.loc[0] - 1.0).abs() < 1e-9 && (w.loc[1] - 4.0).abs() < 1e-9 && (w.loc[2] - 3.0).abs() < 1e-9,
        "{:?}",
        w.loc
    );
    assert_eq!(w.s, [2.0, 1.0, 1.0]);
}
