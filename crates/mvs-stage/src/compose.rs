//! The map's stage data: the base stage's with the layout's play shapes over it.

use mvs_data::stage::{BlastBox, Edge, Floor, Platform, StageData};
use mvs_data::{GameData, SpawnArea};

use crate::layout::{BoxSpec, FloorSpec, ItemArea, Layout, Piece};
use crate::{LayoutError, Light};

/// The team styles' tags on the starts (the sim's `START_TAGS`) and the players each one starts.
pub const START_STYLES: [(&str, usize); 3] = [("1v1", 2), ("2v2", 4), ("FreeForAll", 4)];

/// A stage holds at most this many platforms: a fighter's platforms to ignore are one 32-bit mask.
pub const MAX_PLATFORMS: usize = 32;

/// The blast box the match plays by.
pub const BLAST_BOX: &str = "CollisionComp";

/// A parked floor lies this far under the lowest of the blast box's bottom, the waypoints and the starts.
pub const PARK_DEPTH: f64 = 25000.0;

/// A parked floor's half size on every axis.
pub const PARK_HALF: f64 = 10.0;

/// A mesh's local bounds: the box's origin and half extent (the mesh kind's `bounds`), cm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshBounds {
    pub origin: [f64; 3],
    pub extent: [f64; 3],
}

/// A world box: centre and half extent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBox {
    pub center: [f64; 3],
    pub half: [f64; 3],
}

/// The map's stage data and its item spawn areas.
#[derive(Clone, Debug)]
pub struct ComposedStage {
    pub data: StageData,
    pub spawn_areas: Vec<SpawnArea>,
}

/// The layout's checks that need no scenery: its pieces' names and platforms, its boxes, at most
/// [`MAX_PLATFORMS`] platforms, every start a team style needs, the blast box [`BLAST_BOX`], and no baked piece from
/// another stage than the base.
pub fn check(layout: &Layout) -> Result<(), LayoutError> {
    let invalid = |why: String| Err(LayoutError::Invalid(why));
    if layout.platforms.len() > MAX_PLATFORMS {
        return Err(LayoutError::TooManyPlatforms(layout.platforms.len()));
    }
    for (style, n) in START_STYLES {
        for k in 1..=n {
            for (list, prefix) in [(&layout.spawns, "Player"), (&layout.respawns, "Respawn")] {
                let tag = format!("{prefix}{k}");
                if !list.iter().any(|s| s.tag.as_deref() == Some(tag.as_str()) && s.tags.iter().any(|t| t == style)) {
                    return Err(LayoutError::NoStart { tag, style });
                }
            }
        }
    }
    if !layout.blast.contains_key(BLAST_BOX) {
        return Err(LayoutError::NoBlastBox);
    }
    let mut names: Vec<String> = Vec::new();
    for p in &layout.pieces {
        let stage = layout.piece_stage(p);
        if p.light == Light::Baked && stage != layout.base {
            return Err(LayoutError::BakedFromOtherStage { piece: layout.piece_name(p), stage: stage.to_owned() });
        }
        if let Some(pl) = &p.platform
            && layout.platform_index(pl).is_none()
        {
            return invalid(format!("the piece {} draws the platform {pl}, which the layout has not", p.instance));
        }
        let name = layout.piece_name(p);
        if names.contains(&name) {
            return invalid(format!("two pieces named {name}"));
        }
        names.push(name);
    }
    for (i, a) in layout.platforms.iter().enumerate() {
        if layout.platforms[..i].iter().any(|b| b.name == a.name) {
            return invalid(format!("two platforms named {}", a.name));
        }
        if a.moving.as_ref().is_some_and(|m| m.curves.is_empty()) {
            return invalid(format!("the platform {} moves with no moves", a.name));
        }
        if let Some(piece) = &a.bx.piece {
            let p = piece_named(layout, piece)?;
            if p.platform.as_deref() != Some(a.name.as_str()) {
                return invalid(format!(
                    "the platform {}'s box is tied to the piece {piece}, which draws another",
                    a.name
                ));
            }
        }
    }
    Ok(())
}

/// The piece of a box's `piece` (a piece's composed name or its source instance's).
fn piece_named<'a>(layout: &'a Layout, name: &str) -> Result<&'a Piece, LayoutError> {
    layout
        .pieces
        .iter()
        .find(|p| layout.piece_name(p) == name || p.name.as_deref() == Some(name))
        .ok_or_else(|| LayoutError::Invalid(format!("a box is tied to the piece {name}, which the layout has not")))
}

/// The base's stage data with the layout's play shapes: its name `Custom_<key>`; its floor (or a parked one), solids,
/// platforms (each with the edge polygon a BasePlatformBP_Visual registers), blast boxes and corners, starts,
/// waypoints and item areas; its camera numbers, camera centre and stage centre where the layout gives them. The
/// base's edges, recovery zones, stage actors, query bodies and complex collision belong to the pieces it draws and
/// are not kept. `bounds` gives a piece's mesh bounds, for the boxes tied to a piece.
pub fn compose_stage(
    base: &StageData,
    base_areas: &[SpawnArea],
    layout: &Layout,
    bounds: &dyn Fn(&Piece) -> Option<MeshBounds>,
) -> Result<ComposedStage, LayoutError> {
    check(layout)?;
    let mut d = base.clone();
    d.name = layout.stage_name();
    let world_box =
        |spec: &BoxSpec, what: &str| -> Result<WorldBox, LayoutError> { box_of(layout, spec, bounds, what) };
    d.floor = match layout.floor.box_spec() {
        Some(spec) => {
            let b = world_box(&spec, "the floor")?;
            Floor {
                center: b.center.to_vec(),
                half_extent: b.half.to_vec(),
                top: b.center[2] + b.half[2],
                profile: base.floor.profile.clone(),
                tags: base.floor.tags.clone(),
            }
        }
        None => parked_floor(base, layout),
    };
    let mut platforms = Vec::new();
    let mut edges = Vec::new();
    for (i, p) in layout.platforms.iter().enumerate() {
        let b = world_box(&p.bx, &p.name)?;
        let [_, cy, cz] = b.center;
        let [_, hy, hz] = b.half;
        platforms.push(Platform {
            name: p.name.clone(),
            center: b.center.to_vec(),
            half_extent: b.half.to_vec(),
            top: cz + hz,
            y: vec![cy - hy, cy + hy],
            z: vec![cz - hz, cz + hz],
            moving: p.moving.clone(),
            mesh: None,
        });
        // (BasePlatformBP_Visual's polygon for the EdgeTraceGenerator, as the build's platforms register theirs)
        // Unverified: the polygon is the platform box's section, as the base's platforms' are; a floor box (not
        // parked) gets no edge polygon of its own, where a stage's floor mesh registers one.
        edges.push(Edge {
            owner: p.name.clone(),
            cls: "BasePlatformBP_Visual_C".to_owned(),
            loc: b.center.to_vec(),
            every: false,
            tag: "None".to_owned(),
            points: vec![vec![hy, hz], vec![-hy, hz], vec![-hy, -hz], vec![hy, -hz]],
            solid: None,
            floor: None,
            platform: Some(i32::try_from(i).expect("at most 32 platforms")),
        });
    }
    d.platforms = (!platforms.is_empty()).then_some(platforms);
    d.edges = Some(edges);
    d.edge_box = None;
    d.solids = (!layout.solids.is_empty()).then(|| layout.solids.clone());
    d.bot_zones = None;
    d.placed = None;
    d.query_bodies = None;
    d.complex_segments = None;
    d.tagged = None;
    d.blast_boxes = layout.blast.clone();
    d.blast_box_corners = layout.blast_box_corners.clone();
    // Unverified: the base's shrink box is kept as it is (FreeForAll's shrinking blast box), whatever the layout's
    // blast box.
    d.spawns = layout.spawns.clone();
    d.respawns = layout.respawns.clone();
    d.waypoints = layout.waypoints.clone();
    if let Some(c) = &layout.camera {
        d.camera = c.clone();
    }
    if let Some(c) = &layout.camera_center {
        d.camera_center = c.clone();
    }
    if layout.center.is_some() {
        d.center = layout.center.clone();
    }
    let level = base_areas.first().map_or_else(String::new, |a| a.level.clone());
    let spawn_areas = layout
        .items
        .iter()
        .enumerate()
        .map(|(i, a)| SpawnArea {
            level: level.clone(),
            actor: format!("{}_ItemArea{i}", layout.stage_name()),
            origin: a.origin.to_vec(),
            extent: a.extent.to_vec(),
            root: a.origin.to_vec(),
            big_object_x: a.big_object_x.clone(),
        })
        .collect();
    Ok(ComposedStage { data: d, spawn_areas })
}

/// A new map over the base `base_key`, before anything is built on it: all of the base's scenery drawn (none of its
/// blocks kept), no main floor (parked), no platform or solid; the base's blast boxes and corners, starts, waypoints
/// and item areas, which a layout must have to play. The camera's numbers are the base's (left out).
pub fn starter(base: &StageData, base_areas: &[SpawnArea], key: &str, base_key: &str) -> Layout {
    let v3 = |v: &[f64]| [0, 1, 2].map(|i| v.get(i).copied().unwrap_or(0.0));
    Layout {
        key: key.to_owned(),
        base: base_key.to_owned(),
        blocks: Vec::new(),
        hide: Vec::new(),
        pieces: Vec::new(),
        floor: FloorSpec { parked: true, ..FloorSpec::default() },
        bot_ground: None,
        solids: Vec::new(),
        platforms: Vec::new(),
        blast: base.blast_boxes.clone(),
        blast_box_corners: base.blast_box_corners.clone(),
        spawns: base.spawns.clone(),
        respawns: base.respawns.clone(),
        camera: None,
        camera_center: None,
        center: None,
        waypoints: base.waypoints.clone(),
        items: base_areas
            .iter()
            .map(|a| ItemArea { origin: v3(&a.origin), extent: v3(&a.extent), big_object_x: a.big_object_x.clone() })
            .collect(),
    }
}

/// A match's data on the layout: the stage data and spawn areas composed over the base's (`data` loaded on the
/// base's key, which stays its `stage_key`: the renderer, the sound and the scenery go by it).
pub fn apply(
    data: &mut GameData,
    layout: &Layout,
    bounds: &dyn Fn(&Piece) -> Option<MeshBounds>,
) -> Result<(), LayoutError> {
    if data.stage_key != layout.base {
        return Err(LayoutError::Invalid(format!(
            "the layout's base is {}, the data's stage {}",
            layout.base, data.stage_key
        )));
    }
    let c = compose_stage(&data.stage, &data.spawn_areas, layout, bounds)?;
    data.stage = c.data;
    data.spawn_areas = c.spawn_areas;
    Ok(())
}

/// A floor no fighter reaches: [`PARK_HALF`] on each side, [`PARK_DEPTH`] under the lowest of the blast box's bottom,
/// the waypoints and the starts, under the blast box's centre.
fn parked_floor(base: &StageData, layout: &Layout) -> Floor {
    let blast: &BlastBox = &layout.blast[BLAST_BOX];
    let low = layout
        .waypoints
        .iter()
        .map(|w| w.loc[2])
        .chain(layout.spawns.iter().chain(&layout.respawns).map(|s| s.loc[2]))
        .fold(blast.z[0], f64::min);
    let z = low - PARK_DEPTH;
    Floor {
        center: vec![0.0, blast.center[1], z],
        half_extent: vec![PARK_HALF; 3],
        top: z + PARK_HALF,
        profile: base.floor.profile.clone(),
        tags: base.floor.tags.clone(),
    }
}

/// A box spec's world box: by hand, or the tied piece's mesh bounds where the piece stands (cut to its top
/// `thickness`).
pub fn box_of(
    layout: &Layout,
    spec: &BoxSpec,
    bounds: &dyn Fn(&Piece) -> Option<MeshBounds>,
    what: &str,
) -> Result<WorldBox, LayoutError> {
    let invalid = |why: &str| Err(LayoutError::Invalid(format!("{what}: {why}")));
    let b = match (&spec.piece, spec.center, spec.half_extent) {
        (Some(name), None, None) => {
            let p = piece_named(layout, name)?;
            let Some(mb) = bounds(p) else {
                return invalid(&format!("no mesh bounds for the piece {name}"));
            };
            piece_box(p, mb)
        }
        (None, Some(center), Some(half)) => WorldBox { center, half },
        _ => return invalid("a box is a piece, or a centre and a half extent"),
    };
    Ok(match spec.thickness {
        None => b,
        Some(t) if t > 0.0 && t <= 2.0 * b.half[2] => {
            let top = b.center[2] + b.half[2];
            WorldBox { center: [b.center[0], b.center[1], top - t / 2.0], half: [b.half[0], b.half[1], t / 2.0] }
        }
        Some(t) => return invalid(&format!("thickness {t} is not within the box's height")),
    })
}

/// The axis-aligned world box around a piece's mesh bounds placed by its transform (UE's FRotationTranslationMatrix
/// with the scale, rows as `mvs-render`'s `transform`), through `mvs-math`'s fdlibm sine and cosine so every machine
/// composes the same numbers.
pub fn piece_box(p: &Piece, mb: MeshBounds) -> WorldBox {
    let [pi, ya, ro] = p.rot.map(|d| d * std::f64::consts::PI / 180.0);
    let (sp, cp) = (mvs_math::libm::sin(pi), mvs_math::libm::cos(pi));
    let (sy, cy) = (mvs_math::libm::sin(ya), mvs_math::libm::cos(ya));
    let (sr, cr) = (mvs_math::libm::sin(ro), mvs_math::libm::cos(ro));
    let rows = [
        [cp * cy, cp * sy, sp],
        [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp],
        [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp],
    ];
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for corner in 0..8 {
        let local: [f64; 3] = std::array::from_fn(|i| {
            let sign = if corner & (1 << i) == 0 { -1.0 } else { 1.0 };
            (mb.origin[i] + sign * mb.extent[i]) * p.scale[i]
        });
        for j in 0..3 {
            let w = local[0] * rows[0][j] + local[1] * rows[1][j] + local[2] * rows[2][j] + p.loc[j];
            lo[j] = lo[j].min(w);
            hi[j] = hi[j].max(w);
        }
    }
    WorldBox {
        center: std::array::from_fn(|j| (lo[j] + hi[j]) / 2.0),
        half: std::array::from_fn(|j| (hi[j] - lo[j]) / 2.0),
    }
}
