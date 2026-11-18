//! The stage data's edges (`StageData.edges`): the level's Comp_EdgeTrace owners.

use mvs_data::stage::{Edge, Platform};

use super::super::super::shape::{fail, short_name};
use super::super::super::transform::qrot;
use super::super::context::{StageCtx, js_truthy, s, strings, vec};
use super::solids::LevelSolid;
use crate::kind::KindError;
use crate::objview::ObjView;

/// The Comp_EdgeTrace owners in the levels' actor order, or none where no level holds an EdgeTraceGenerator_C
/// (GetActorOfClass finds none). Per owner: its root's world location, UpdateIntersectionsEveryFrame and Tag, and
/// Generate Shape From Box Collision's four points before the sort (Comp_EdgeTrace @1330-2610: the owner's LAST Box
/// tagged TraceEdges; corner i of (a eX, a eY, b eZ), a = 1 for i < 1 or i > 2 else -1, b = 1 for i < 2 else -1, e =
/// the component's world scale x BoxExtent, rotated by the owner's rotation, plus the component's location less the
/// owner's, X dropped). What the sim moves it with: `floor` (the stage block), `platform` (its index in
/// StageData.platforms) or `solid` (the tagged Box's id in StageData.solids).
pub fn edge_owners(
    ctx: &StageCtx<'_>,
    actors: &[ObjView<'_>],
    block: &str,
    platforms: &[Platform],
    sol: &[LevelSolid],
) -> Result<Option<Vec<Edge>>, KindError> {
    if !actors.iter().any(|a| short_name(a.class()) == "EdgeTraceGenerator_C") {
        return Ok(None);
    }
    let traced = |x: &ObjView<'_>| -> Result<bool, KindError> {
        let p = ctx.with_templates(x)?;
        Ok(match p.get("ComponentTags") {
            Some(v) => strings(Some(v)).is_some_and(|t| t.iter().any(|t| t == "TraceEdges")),
            None => false,
        })
    };
    let mut out = Vec::new();
    for a in actors {
        let li = ctx.li_of(a)?;
        let n = short_name(a.path()).to_owned();
        let cs = ctx.comps(a, None);
        let ap = ctx.properties(a)?;
        for c in &cs {
            if short_name(c.class()) != "Comp_EdgeTrace_C" {
                continue;
            }
            let cp = ctx.with_templates(c)?;
            let Some(rc) = s(&ap, "RootComponent").map(short_name) else {
                return fail(format!("edges {n}: no RootComponent"));
            };
            let Some(root) = cs.iter().find(|x| short_name(x.path()) == rc) else {
                return fail(format!("edges {n}: no root {rc}"));
            };
            let ow = ctx.world(root)?;
            let (oloc, oq) = (ow.loc, ow.q);
            let mut e = Edge {
                owner: n.clone(),
                cls: short_name(a.class()).to_owned(),
                loc: oloc.to_vec(),
                every: js_truthy(cp.get("UpdateIntersectionsEveryFrame")),
                tag: s(&cp, "Tag").unwrap_or("None").to_owned(),
                points: Vec::new(),
                solid: None,
                floor: None,
                platform: None,
            };
            let mut boxes: Vec<&ObjView<'_>> = Vec::new();
            let mut other = false;
            for x in &cs {
                let t = short_name(x.class());
                if t == "BoxComponent" && traced(x)? {
                    boxes.push(x);
                } else if (t == "SphereComponent" || t == "StaticMeshComponent") && traced(x)? {
                    other = true;
                }
            }
            if boxes.is_empty() && other {
                return fail(format!("{n}: a Sphere or mesh tagged TraceEdges, which edge_owners does not read"));
            }
            if let Some(&bx) = boxes.last() {
                let bw = ctx.world(bx)?;
                let (bl, bs) = (bw.loc, bw.scale);
                let ext = vec(ctx.with_templates(bx)?.get("BoxExtent"), [32.0; 3]);
                let ev = [0, 1, 2].map(|i| ext[i] * bs[i]);
                for i in 0..4 {
                    let sg = if !(1..=2).contains(&i) { 1.0 } else { -1.0 };
                    let t = if i < 2 { 1.0 } else { -1.0 };
                    let r = qrot(oq, [sg * ev[0], sg * ev[1], t * ev[2]]);
                    e.points.push(vec![r[1] + bl[1] - oloc[1], r[2] + bl[2] - oloc[2]]);
                }
                let bn = format!("{n}.{}", short_name(bx.path()));
                if let Some(sid) = sol.iter().find(|x| x.li == li && x.e.name == bn) {
                    e.solid = Some(sid.e.id);
                }
            }
            if n == block {
                e.floor = Some(true);
            }
            if let Some(pi) = platforms.iter().position(|p| p.name == n) {
                e.platform = Some(i32::try_from(pi).expect("a few platforms"));
            }
            out.push(e);
        }
    }
    Ok(Some(out))
}
