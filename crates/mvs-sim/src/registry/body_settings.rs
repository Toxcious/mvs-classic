//! A simulating body's settings and mass from its class data (materialOf,
//! sphereSettings, sphylMass, sumMass, bodyMass, capsuleSettings): the FBodyInstance reads `mvs-collision`'s steps
//! take as numbers. Read from the class's data, so the same for every body of a class. Each warning names the class
//! (`owner`) and the material; it goes to the World's warned set (`warn_text`).

use crate::Real;
use crate::fighter::common::warn_text;
use crate::math::{max, min};
use crate::registry::props::{flag, num, num_or, obj, text};
use crate::world::World;
use mvs_collision::rigid_sphere::{Settings, WeldedMass};
use mvs_data::value::{Props, Value};

/// Where a read's warnings go: the World's warned set (`warn_text`), or a test's list.
type Warn<'a> = &'a mut dyn FnMut(String);

/// A physical material's values (`Material`): density and raiseMassToPower are UpdateMassProperties'.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub friction: Real,
    pub restitution: Real,
    pub density: Real,
    pub raise_mass_to_power: Real,
}

/// `MATERIALS`: the physical materials a body names (FBodyInstance PhysMaterialOverride), from their assets.
/// SatchelPhysicalMaterial: Friction 20, Restitution 1.0 (its RestitutionCombineMode Min comes with no
/// bOverrideRestitutionCombineMode: the project's Average combines); Density and RaiseMassToPower left (the
/// UPhysicalMaterial constructor's 1.0 and 0.75, exe 142122670).
// Deviation: the table is written by hand from the assets, not read from the data the port loads; a name not in it is
//   refused (sphere_settings).
fn material(name: &str) -> Option<Material> {
    match name {
        "SatchelPhysicalMaterial" => {
            Some(Material { friction: 20.0, restitution: 1.0, density: 1.0, raise_mass_to_power: 0.75 })
        }
        _ => None,
    }
}

/// `DEFAULT_MATERIAL`: the stage's boxes and a body without an override, the engine's default material.
pub const DEFAULT_MATERIAL: Material =
    Material { friction: 0.7, restitution: 0.3, density: 1.0, raise_mass_to_power: 0.75 };

/// `materialOf`: a body's material (its PhysMaterialOverride, else the default); `None` (warned) for one not entered.
fn material_of(bi: Option<&Props>, owner: &str, warn: Warn) -> Option<Material> {
    let Some(n) = bi.and_then(|b| text(b, "PhysMaterialOverride")).filter(|s| !s.is_empty()) else {
        return Some(DEFAULT_MATERIAL);
    };
    let k = material(n);
    if k.is_none() {
        warn(format!("{owner}: physical material {n} not entered (MATERIALS): the body is not stepped"));
    }
    k
}

/// `bi?.[k] === true`.
fn is_true(bi: Option<&Props>, k: &str) -> bool {
    matches!(bi.and_then(|b| b.get(k)), Some(Value::Bool(true)))
}

/// `sphereSettings`: a body's settings from its FBodyInstance data: LinearDamping (FBodyInstance default 0.01),
/// AngularDamping (default 0), the material combined with the stage's under Average, and its rotation locks
/// (FBodyInstance::CreateDOFLock 142c6e270: X rotation -> lock_spin, Y -> swing2 lock_y, Z -> swing1 lock_z).
/// `None` (warned) for a material not entered: the caller does not step the body. A body without the X translation
/// lock, or with a lock the model does not have (a Y or Z translation lock, a DOFMode other than Default), is warned
/// and stepped as if it had none.
// Unverified: Average is the project's combine mode taken for every pair; a material's own override
//   (bOverrideFrictionCombineMode / bOverrideRestitutionCombineMode) is not read: none of the entered ones sets one.
pub fn sphere_settings(w: &World, bi: Option<&Props>, owner: &str) -> Option<Settings> {
    sphere_settings_with(bi, owner, &mut |m| warn_text(w, &m))
}

/// [`sphere_settings`] with its warnings to `warn`.
fn sphere_settings_with(bi: Option<&Props>, owner: &str, warn: Warn) -> Option<Settings> {
    let m = material_of(bi, owner, warn)?;
    if !bi.is_some_and(|b| flag(b, "bLockXTranslation")) {
        warn(format!("{owner}: no bLockXTranslation: the body is stepped in the play plane all the same"));
    }
    for k in ["bLockYTranslation", "bLockZTranslation"] {
        if bi.is_some_and(|b| flag(b, k)) {
            warn(format!("{owner}: {k} is not ported"));
        }
    }
    if let Some(d) = bi.and_then(|b| b.get("DOFMode")).filter(|v| crate::registry::props::truthy(v))
        && d.as_str() != Some("EDOFMode::Default")
    {
        let d = d.as_str().map_or_else(|| format!("{d:?}"), str::to_owned);
        warn(format!("{owner}: DOFMode {d} is not ported"));
    }
    let mut set = Settings {
        linear_damping: bi.map_or(0.01, |b| f64::from(num_or(b, "LinearDamping", 0.01))),
        angular_damping: bi.map_or(0.0, |b| f64::from(num_or(b, "AngularDamping", 0.0))),
        restitution: f64::from((m.restitution + DEFAULT_MATERIAL.restitution) / 2.0),
        friction: f64::from((m.friction + DEFAULT_MATERIAL.friction) / 2.0),
        ..Settings::default()
    };
    let any = |k: &str| bi.is_some_and(|b| flag(b, k));
    if !any("bLockXRotation") && !any("bLockYRotation") && !any("bLockZRotation") {
        return Some(set);
    }
    set.lock_spin = is_true(bi, "bLockXRotation");
    set.lock_y = is_true(bi, "bLockYRotation");
    set.lock_z = is_true(bi, "bLockZRotation");
    Some(set)
}

/// `ShapeMass`: a shape's mass properties at unit density in the actor's frame (UE axes, Z up): its volume, centre
/// and inertia tensor about that centre (xx, yy, zz and the products xy, xz, yz, as PxMat33's off-diagonals).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeMass {
    pub v: Real,
    pub c: [Real; 3],
    pub i: [Real; 6],
}

/// `MassProps`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProps {
    pub m: Real,
    pub c: [Real; 3],
    pub i: [Real; 6],
}

/// `sphylMass`: a UE capsule (radius r, half height hh with its hemispheres; PhysX's cylinder half length
/// max(hh - r, 0)) along the actor's Z at the actor's origin + z: PhysX's capsule mass properties (a cylinder and the
/// two hemispheres).
pub fn sphyl_mass(r: Real, hh: Real, z: Real) -> ShapeMass {
    let l = max(hh - r, 0.0);
    let pi = std::f32::consts::PI;
    let mc = pi * r * r * 2.0 * l;
    let ms = (4.0 / 3.0) * pi * r * r * r;
    let ax = mc * r * r / 2.0 + ms * r * r * 2.0 / 5.0; // about its own axis
    let tr = mc * (r * r / 4.0 + l * l / 3.0) + ms * (r * r * 2.0 / 5.0 + l * r * 3.0 / 4.0 + l * l);
    ShapeMass { v: mc + ms, c: [0.0, 0.0, z], i: [tr, tr, ax, 0.0, 0.0, 0.0] }
}

/// `boxMass`: a box of half extents (hx, hy, hz), not turned, its centre at c.
pub fn box_mass(hx: Real, hy: Real, hz: Real, c: [Real; 3]) -> ShapeMass {
    let v = 8.0 * hx * hy * hz;
    ShapeMass {
        v,
        c,
        i: [
            (v * (hy * hy + hz * hz)) / 3.0,
            (v * (hx * hx + hz * hz)) / 3.0,
            (v * (hx * hx + hy * hy)) / 3.0,
            0.0,
            0.0,
            0.0,
        ],
    }
}

/// `covOf`: the covariance of a solid (the integral of x_i x_j about its centre) from its inertia: the inertia's
/// diagonal is tr - cov_ii and its off-diagonals -cov_ij (PxMassProperties' convention).
fn cov_of(i: [Real; 6]) -> [Real; 6] {
    let h = (i[0] + i[1] + i[2]) / 2.0;
    [h - i[0], h - i[1], h - i[2], -i[3], -i[4], -i[5]]
}

/// `inertiaOf`: [`cov_of`] back.
fn inertia_of(cv: [Real; 6]) -> [Real; 6] {
    let t = cv[0] + cv[1] + cv[2];
    [t - cv[0], t - cv[1], t - cv[2], -cv[3], -cv[4], -cv[5]]
}

/// `hullMass`: a static mesh component's hull (its Hull* numbers: the mesh's simple collision at unit density and
/// scale, written by the import), at the scale s, its origin at c and not turned: volume x sx sy sz, the centre x s,
/// the covariance x sx sy sz s_i s_j. `None` when the data has none.
pub fn hull_mass(m: &Props, s: [Real; 3], c: [Real; 3]) -> Option<ShapeMass> {
    let vol = match m.get("HullVolume") {
        Some(Value::Num(v)) => *v,
        _ => return None,
    };
    let g = |k: &str| num(m, k).unwrap_or(Real::NAN);
    let k = s[0] * s[1] * s[2];
    let cv = cov_of([g("HullIxx"), g("HullIyy"), g("HullIzz"), g("HullIxy"), g("HullIxz"), g("HullIyz")]);
    let sc = [
        cv[0] * k * s[0] * s[0],
        cv[1] * k * s[1] * s[1],
        cv[2] * k * s[2] * s[2],
        cv[3] * k * s[0] * s[1],
        cv[4] * k * s[0] * s[2],
        cv[5] * k * s[1] * s[2],
    ];
    Some(ShapeMass {
        v: crate::math::narrow(vol * f64::from(k)),
        c: [c[0] + g("HullCX") * s[0], c[1] + g("HullCY") * s[1], c[2] + g("HullCZ") * s[2]],
        i: inertia_of(sc),
    })
}

/// `weldedMass`: the welded actor's body in the play plane: the summed mass, its centre over the actor's origin (ox
/// along the plane's x = -UE Y, oy along the body's up) and k = m R^2 / I about the play plane's normal (UE X) through
/// that centre, R the sphere's radius. The build diagonalises the sum (PxDiagonalize at 142c68c6e); with no product
/// along X the turn is about X and Ixx is an eigenvalue as it stands, so the diagonal is kept and the turn not
/// applied (ia, ib are the diagonal's Iyy, Izz). A product with X would turn the frame out of the plane: that warns.
pub fn welded_mass(w: &World, bodies: &[MassProps], radius: Real, owner: &str) -> WeldedMass {
    let s = sum_mass(bodies.iter().copied());
    let big = max(max(s.i[0], s.i[1]), s.i[2]);
    if s.i[3].abs() > 1e-6 * big || s.i[4].abs() > 1e-6 * big {
        warn_text(
            w,
            &format!(
                "{owner}: the welded inertia has a product with X (PxDiagonalize would turn its frame out of the play plane: not ported)"
            ),
        );
    }
    // setMass / setMassSpaceInertiaTensor / setCMassLocalPose take floats
    let (m, ic) = (s.m, s.i[0]);
    WeldedMass {
        m: f64::from(m),
        ox: f64::from(-s.c[1]),
        oy: f64::from(s.c[2]),
        k: f64::from((m * radius * radius) / ic),
        ia: f64::from(s.i[1]),
        ib: f64::from(s.i[2]),
        ic: f64::from(ic),
    }
}

/// `sumMass`: the sum of mass properties about their joint centre (the parallel axis rule: PxMassProperties::sum).
fn sum_mass(ps: impl Iterator<Item = MassProps> + Clone) -> MassProps {
    let mut m = 0.0;
    let mut c = [0.0; 3];
    for p in ps.clone() {
        m += p.m;
        for (ck, pc) in c.iter_mut().zip(p.c) {
            *ck += p.m * pc;
        }
    }
    for ck in &mut c {
        *ck /= m;
    }
    let mut i = [0.0; 6];
    for p in ps {
        let d = [p.c[0] - c[0], p.c[1] - c[1], p.c[2] - c[2]];
        let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        i[0] += p.i[0] + p.m * (dd - d[0] * d[0]);
        i[1] += p.i[1] + p.m * (dd - d[1] * d[1]);
        i[2] += p.i[2] + p.m * (dd - d[2] * d[2]);
        i[3] += p.i[3] - p.m * d[0] * d[1];
        i[4] += p.i[4] - p.m * d[0] * d[2];
        i[5] += p.i[5] - p.m * d[1] * d[2];
    }
    MassProps { m, c, i }
}

/// `bodyMass`: ComputeMassProperties of one body: its shapes at density max(Density x 0.001, 9e-8) (kg/cm^3), then
/// the mass bOverrideMass ? MassInKgOverride (the FBodyInstance default 100) : pow(m, clamp(RaiseMassToPower, 1e-4,
/// 1)) x MassScale, at least 0.001 (maxss 143b52800 at 142c87476; 143b05c98 = 1e-4 the clamp's low bound at
/// 142c87441; a NaN power takes 1e-4), the CRT powf (thunk 14398c5b7), the properties scaled by k = new mass / old
/// mass and the mass stored as k x old mass (142c873a0, not the new mass itself: an ulp apart at most). `None` (warned) for a material not entered. COMNudge and
/// InertiaTensorScale warn when set: how 142c8ec00 applies the second is not read.
pub fn body_mass(w: &World, bi: Option<&Props>, shapes: &[ShapeMass], owner: &str) -> Option<MassProps> {
    body_mass_of(bi, shapes, owner, true, &mut |m| warn_text(w, &m))
}

/// [`body_mass`]; `tensor` false reads the data as if it had no InertiaTensorScale (capsuleSettings' call).
fn body_mass_of(bi: Option<&Props>, shapes: &[ShapeMass], owner: &str, tensor: bool, warn: Warn) -> Option<MassProps> {
    let mat = material_of(bi, owner, warn)?;
    let xyz = |o: &Props, k: &str| num(o, k);
    if let Some(n) = bi.and_then(|b| obj(b, "COMNudge"))
        && ["X", "Y", "Z"].iter().any(|k| flag(n, k))
    {
        warn(format!("{owner}: COMNudge is not applied"));
    }
    if tensor
        && let Some(t) = bi.and_then(|b| obj(b, "InertiaTensorScale"))
        && [xyz(t, "X"), xyz(t, "Y"), xyz(t, "Z")].iter().any(|v| v.unwrap_or(1.0) != 1.0)
    {
        warn(format!("{owner}: InertiaTensorScale is not applied"));
    }
    let density = max(mat.density * 0.001, 9e-8);
    let raw = sum_mass(shapes.iter().map(|s| MassProps { m: s.v * density, c: s.c, i: s.i.map(|x| x * density) }));
    let over = bi.is_some_and(|b| flag(b, "bOverrideMass"));
    let m = if over {
        bi.map_or(100.0, |b| num_or(b, "MassInKgOverride", 100.0))
    } else {
        let raise = mat.raise_mass_to_power;
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the exe's test: a NaN power takes 1e-4")]
        let e = if !(raise >= 1e-4) { 1e-4 } else { min(raise, 1.0) };
        crate::math::ue::powf(raw.m, e) * bi.map_or(1.0, |b| num_or(b, "MassScale", 1.0))
    };
    // maxss(m, 0.001): a NaN mass takes 0.001
    let m = if m > 0.001 { m } else { 0.001 };
    let k = m / raw.m;
    Some(MassProps { m: k * raw.m, c: raw.c, i: raw.i.map(|x| x * k) })
}

/// `capsuleSettings`'s result: the settings and the body's mass.
#[derive(Clone, Copy, Debug)]
pub struct CapsuleSettings {
    pub set: Settings,
    pub mass: Real,
}

/// `capsuleSettings`: a Capsule component's body: [`sphere_settings`] of its
/// BodyInstance, warned without a large InertiaTensorScale on X (its contacts would turn the spin: not ported), and
/// its mass ([`body_mass`] of the capsule, the tensor scale left out). `None` for a material not entered.
pub fn capsule_settings(w: &World, cap: Option<&Props>, owner: &str) -> Option<CapsuleSettings> {
    capsule_settings_with(cap, owner, &mut |m| warn_text(w, &m))
}

/// [`capsule_settings`] with its warnings to `warn`.
fn capsule_settings_with(cap: Option<&Props>, owner: &str, warn: Warn) -> Option<CapsuleSettings> {
    let bi = cap.and_then(|c| obj(c, "BodyInstance"));
    let set = sphere_settings_with(bi, owner, warn)?;
    let t = bi.and_then(|b| obj(b, "InertiaTensorScale"));
    if !t.is_some_and(|t| num(t, "X").unwrap_or(1.0) >= 1e4) {
        warn(format!(
            "{owner}: capsule body without a large InertiaTensorScale on X: its contacts would turn the spin (not ported: capsuleStep)"
        ));
    }
    let r = cap.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(22.0);
    let hh = cap.and_then(|c| num(c, "CapsuleHalfHeight")).unwrap_or(44.0);
    let mp = body_mass_of(bi, &[sphyl_mass(r, hh, 0.0)], owner, false, warn)?;
    Some(CapsuleSettings { set, mass: mp.m })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The f64s of a little-endian hex dump (the oracle probe's Float64Array bytes).
    /// The oracle's doubles, rounded to float32 (the sim's numbers), as bits.
    fn le(hex: &str) -> Vec<u64> {
        (0..hex.len() / 16)
            .map(|k| {
                let b: Vec<u8> =
                    (0..8).map(|j| u8::from_str_radix(&hex[k * 16 + j * 2..k * 16 + j * 2 + 2], 16).unwrap()).collect();
                f64::from(crate::math::narrow(f64::from_bits(u64::from_le_bytes(b.try_into().unwrap())))).to_bits()
            })
            .collect()
    }

    fn props(kv: &[(&str, Value)]) -> Props {
        kv.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
    }

    fn bits(v: &[Real]) -> Vec<u64> {
        v.iter().map(|&x| f64::from(x).to_bits()).collect()
    }

    // The oracle's values (sphylMass, bodyMass, capsuleSettings) for the same inputs.
    #[test]
    fn sphyl_mass_matches_the_oracle() {
        let m = sphyl_mass(33.0, 51.5, 7.0);
        let mut got = m.i.to_vec();
        got.push(m.v);
        // (float32: the inertia's X and Y terms one ulp off the oracle's double sums)
        let mut want = le(
            "1279dda93401ac411279dda93401ac41ccf7a2628608a0410000000000000000000000000000000000000000000000009b2b4bfcf2e91041",
        );
        (want[0], want[1]) = (f64::from(234_920_544.0_f32).to_bits(), f64::from(234_920_544.0_f32).to_bits());
        assert_eq!(bits(&got), want);
        assert_eq!(m.c, [0.0, 0.0, 7.0]);
    }

    #[test]
    fn body_mass_matches_the_oracle() {
        let bi = props(&[("MassScale", Value::Num(1.3))]);
        let mut warned = Vec::new();
        let b = body_mass_of(
            Some(&bi),
            &[sphyl_mass(20.0, 40.0, 0.0), sphyl_mass(10.0, 10.0, 30.0)],
            "probe",
            true,
            &mut |m| warned.push(m),
        )
        .unwrap();
        let mut got = vec![b.m];
        got.extend(b.c);
        got.extend(b.i);
        // (float32: the inertia's Y and Z one ulp off the oracle's double sums)
        let mut want = le(
            "fbc6b70685ab424000000000000000000000000000000000b56ddbb66ddbf63fb351339eca5dd240b351339eca5dd240f1e7028783d6b940000000000000000000000000000000000000000000000000",
        );
        (want[4], want[5]) = (f64::from(18_807.164_f32).to_bits(), f64::from(18_807.164_f32).to_bits());
        assert_eq!(bits(&got), want);
        let bi = props(&[("bLockXTranslation", Value::Bool(true)), ("MassScale", Value::Num(1.7))]);
        let b =
            body_mass_of(Some(&bi), &[sphyl_mass(33.0, 51.5, 0.0)], "probe", true, &mut |m| warned.push(m)).unwrap();
        assert_eq!(bits(&[b.m]), le("17a26391afdd5c40"));
        assert!(warned.is_empty());
    }

    #[test]
    fn capsule_settings_of_the_snack_box() {
        let xyz = |x: Real| {
            Value::Obj(props(&[("X", Value::Num(f64::from(x))), ("Y", Value::Num(1.0)), ("Z", Value::Num(1.0))]))
        };
        let bi = props(&[
            ("LinearDamping", Value::Num(0.5)),
            ("AngularDamping", Value::Num(1.0)),
            ("bLockXTranslation", Value::Bool(true)),
            ("bLockYRotation", Value::Bool(true)),
            ("bLockZRotation", Value::Bool(true)),
            ("MassInKgOverride", Value::Num(145.0)),
            ("bOverrideMass", Value::Bool(true)),
            ("InertiaTensorScale", xyz(100000.0)),
        ]);
        let cap = props(&[
            ("CapsuleRadius", Value::Num(50.0)),
            ("CapsuleHalfHeight", Value::Num(70.0)),
            ("BodyInstance", Value::Obj(bi)),
        ]);
        let mut warned = Vec::new();
        let c = capsule_settings_with(Some(&cap), "ScoobySnackBox_C", &mut |m| warned.push(m)).unwrap();
        assert!(warned.is_empty());
        assert_eq!(c.mass, 145.0);
        let s = c.set;
        assert_eq!(
            (s.linear_damping, s.angular_damping, s.restitution, s.friction),
            (0.5, 1.0, f64::from(0.3_f32), f64::from(0.7_f32))
        );
        assert_eq!((s.lock_spin, s.lock_y, s.lock_z), (false, true, true));
    }

    #[test]
    fn warnings_name_the_class_and_the_material() {
        let bi = props(&[("PhysMaterialOverride", Value::Str("Rubber".into()))]);
        let cap = props(&[("BodyInstance", Value::Obj(bi))]);
        let mut warned = Vec::new();
        assert!(capsule_settings_with(Some(&cap), "Box_C", &mut |m| warned.push(m)).is_none());
        assert_eq!(warned, ["Box_C: physical material Rubber not entered (MATERIALS): the body is not stepped"]);
    }
}
