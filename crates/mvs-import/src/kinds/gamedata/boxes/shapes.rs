//! The boxes bind's shapes: the pawn's sphere, capsule and box components classed as hit, hurt or detector shapes over
//! the build's collision profiles (`profiles.rs`), each on its bone or skeleton socket, and a spawned actor's shapes on
//! its own mesh, in UE space (cm, X fwd, Y depth, Z up).

use indexmap::IndexMap;
use mvs_data::boxes::BoxKind;
use mvs_data::boxes::BoxShape;
use mvs_data::fighter::{Component, Map, Socket};
use mvs_data::{Props, Value};

use super::super::profiles::{CollisionTable, Responses, resolve_body};
use super::super::shape::fail;
use super::super::transform::{Quat, qmul, qrot, rotator_quat};
use super::{Placed, get, num_or, strings, truthy, vec_of};
use crate::kind::KindError;

/// The channels the HitBox profile overlaps (DefaultEngine.ini): what a hit shape's overlap meets. A detector's
/// `overlaps` lists those of them its own body overlaps. Checked against the profile.
pub const HIT_CHANNELS: [&str; 5] =
    ["Pawn", "DamageDestructible", "ProjectileDestructible", "ItemDestructible", "HurtBox"];

pub const IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];

pub fn rot_of(v: Option<&Value>) -> Quat {
    let v = v.unwrap_or(&Value::Null);
    rotator_quat(num_or(get(v, "Pitch"), 0.0), num_or(get(v, "Yaw"), 0.0), num_or(get(v, "Roll"), 0.0))
}

/// A mesh's bones: each name's parent (`None` the root), in the mesh's order.
pub type Bones = IndexMap<String, Option<String>>;

/// Whether `bone` is `top` or under it.
pub fn under(bones: &Bones, bone: Option<&str>, top: &str) -> bool {
    let mut b = bone;
    while let Some(x) = b {
        if x == top {
            return true;
        }
        b = bones.get(x).and_then(|p| p.as_deref());
    }
    false
}

/// A shape of the index with only the fields every shape has.
pub fn new_shape(name: String, kind: BoxKind, bone: Option<String>, tags: Vec<String>) -> BoxShape {
    BoxShape {
        name,
        kind,
        bone,
        tags,
        order: None,
        is_2d: false,
        capsule: false,
        abs_scale: None,
        radius: 0.0,
        half_height: 0.0,
        vbone: None,
        on_capsule: None,
        is_box: None,
        extent: None,
        abs_rot: None,
        missing: None,
        overlaps: None,
        unscaled: None,
        rot: None,
        scale: None,
        com: None,
        axes: None,
        axes_const: None,
    }
}

/// The bone a shape attached to `bone` (a component's AttachToName / AttachSocketName) rides, with a skeleton
/// socket's relative transform put in front of the shape's own `loc` / `rot` (`sh` is changed; its `bone` set).
/// AttachToName is an FName, compared without case (Pawn_Reindog's Head and Head2D attach to 'Head', the skeleton's
/// socket 'head'). Sockets over bones.
pub fn attach_bone(
    sh: &mut Placed,
    bone: Option<String>,
    bones: &Bones,
    sockets: &Map<Socket>,
    what: &str,
) -> Result<Option<String>, KindError> {
    let mut bone = bone;
    if let Some(b) = bone.clone()
        && !bones.contains_key(&b)
        && !sockets.contains_key(&b)
    {
        let l = b.to_lowercase();
        let mut hit: Vec<&String> = sockets.keys().filter(|s| s.to_lowercase() == l).collect();
        if hit.is_empty() {
            hit = bones.keys().filter(|s| s.to_lowercase() == l).collect();
        }
        if hit.len() == 1 {
            let h = hit[0].clone();
            sh.shape.bone = Some(h.clone());
            bone = Some(h);
        }
    }
    if let Some(b) = bone.clone()
        && !bones.contains_key(&b)
        && let Some(sk) = sockets.get(&b)
    {
        // (the fighter data fills an unset RelativeScale with 1, so a scale other than 1 is a serialised one)
        if sk.scale.iter().any(|v| *v != 1.0) {
            return fail(format!("socket {b} of {what} has a scale (not handled)"));
        }
        let sq = rotator_quat(sk.rot[0], sk.rot[1], sk.rot[2]);
        let off = qrot(sq, sh.loc);
        sh.loc = [sk.loc[0] + off[0], sk.loc[1] + off[1], sk.loc[2] + off[2]];
        sh.rot = qmul(sq, sh.rot);
        sh.shape.bone = Some(sk.bone.clone());
        bone = Some(sk.bone.clone());
    }
    if let Some(b) = &bone
        && !bones.contains_key(b)
    {
        return fail(format!("bone {b} missing for {what}"));
    }
    Ok(bone)
}

/// The channels on which two response tables differ, as text (empty when none).
pub fn body_diff(resp: &Responses, ref_: &Responses) -> String {
    let mut keys: Vec<&String> = resp.keys().collect();
    keys.sort();
    keys.into_iter()
        .filter(|c| resp.get(*c) != ref_.get(*c))
        .map(|c| {
            let tail =
                |s: Option<&String>| s.map_or_else(|| "undefined".to_owned(), |s| s.get(4..).unwrap_or("").to_owned());
            format!("{c} {} ({})", tail(resp.get(c)), tail(ref_.get(c)))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The HitBox profile's responses (checked present by [`check_table`]).
fn hitbox_resp(t: &CollisionTable) -> &Responses {
    &t.profiles["HitBox"].resp
}

/// The profiles the classes rest on, checked.
pub fn check_table(t: &CollisionTable) -> Result<(), KindError> {
    for need in ["HitBox", "HurtBox", "KnockBack"] {
        if !t.profiles.contains_key(need) {
            return fail(format!("DefaultEngine.ini: no profile {need}"));
        }
    }
    if t.profiles["HurtBox"].obj != "HurtBox"
        || t.profiles["HitBox"].obj != "HitBox"
        || !t.defaults.contains_key("HurtBox")
    {
        return fail("DefaultEngine.ini: the HitBox / HurtBox profiles are not on the channels of those names");
    }
    let mut over: Vec<&str> =
        hitbox_resp(t).iter().filter(|(_, r)| *r == "ECR_Overlap").map(|(c, _)| c.as_str()).collect();
    over.sort_unstable();
    let mut want = HIT_CHANNELS;
    want.sort_unstable();
    if over != want {
        return fail(format!(
            "DefaultEngine.ini: the HitBox profile does not overlap exactly {}",
            HIT_CHANNELS.join(", ")
        ));
    }
    Ok(())
}

/// A resolved body's response on `ch` (`resp![ch]`).
fn resp_of<'r>(resp: Option<&'r Responses>, ch: &str) -> Option<&'r str> {
    resp.and_then(|r| r.get(ch)).map(String::as_str)
}

/// Whether a body is a hit shape by the actors' rule: profile HitBox or KnockBack, or a Custom one on the HitBox
/// channel with the HitBox profile's responses.
fn plain_hit(
    prof: Option<&str>,
    obj: Option<&str>,
    resp: Option<&Responses>,
    t: &CollisionTable,
) -> Result<bool, KindError> {
    if prof == Some("HitBox") || prof == Some("KnockBack") {
        return Ok(true);
    }
    if prof == Some("Custom") && obj == Some("HitBox") {
        let Some(r) = resp else { return fail("a Custom body with no responses") };
        return Ok(body_diff(r, hitbox_resp(t)).is_empty());
    }
    Ok(false)
}

const COLLISION_FIELDS: [&str; 4] = ["CollisionProfileName", "ObjectType", "CollisionResponses", "CollisionEnabled"];
const SHAPE_TYPES: [&str; 3] = ["SphereComponent", "CapsuleComponent", "BoxComponent"];

/// What pawn_shapes reads of the pawn.
pub struct PawnShapeInput<'a> {
    /// fighter data components.pawn and components.base
    pub comps: &'a Map<Component>,
    pub base: &'a Map<Component>,
    /// the SCS: a component's AttachToName, its parent component, its place in the construction order
    pub attach: &'a IndexMap<String, Option<String>>,
    pub scs_parent: &'a IndexMap<String, Option<String>>,
    pub scs_order: &'a [String],
    pub mesh_scale: f64,
    /// the component tags a notify's HitBoxTags reaches (with Use 2D Hitboxes: tag + 2D)
    pub hit_reach: &'a [String],
    pub bones: &'a Bones,
    pub sockets: &'a Map<Socket>,
    pub table: &'a CollisionTable,
}

/// The HIT_CHANNELS a body overlaps.
fn overlapped(resp: Option<&Responses>, what: &str) -> Result<Vec<String>, KindError> {
    let Some(r) = resp else { return fail(format!("{what}: no responses")) };
    Ok(HIT_CHANNELS
        .iter()
        .filter(|ch| r.get(**ch).map(String::as_str) == Some("ECR_Overlap"))
        .map(|c| (*c).to_owned())
        .collect())
}

/// The pawn's shape components (sphere, capsule, box) by name, each a hit, hurt or detector shape by its body's
/// resolved profile, object type and responses, with its place on its bone. BaseFighter's own components are left
/// out (the sim has them in code: FLOOR_BOUNCE_HURT).
pub fn pawn_shapes(k: &PawnShapeInput<'_>) -> Result<Vec<Placed>, KindError> {
    let table = k.table;
    let mut shapes: Vec<Placed> = Vec::new();
    let mut names: Vec<&String> = k.comps.keys().collect();
    names.sort();
    for name in names {
        let c = &k.comps[name];
        if !SHAPE_TYPES.contains(&c.ty.as_str()) {
            continue;
        }
        // BaseFighter's own component, which the pawn's data entry resolves in full (Pawn_WonderWomanV3 overrides
        // HurtBoxFloorBounce): not a bone-attached shape of the pawn's SCS
        if k.base.contains_key(name) {
            continue;
        }
        let p = &c.p;
        let v = name.replace("_GEN_VARIABLE", "");
        let mut bi: Props = get(p, "BodyInstance").and_then(Value::as_obj).cloned().unwrap_or_default();
        let tags = strings(get(p, "ComponentTags"));
        let reached = tags.iter().any(|t| k.hit_reach.contains(t));
        // A template that saves no collision field keeps its class default: a UShapeComponent's ctor sets the profile
        // OverlapAllDynamic (ctor 142984280). Pawn_C023B's ChainSawHitBox / ChainSawHitBox2: the handler's overlap
        // event (Comp_Pawn_HitBoxHandler @7466 -> pfgDoDamage @7502) takes any overlapped component's actor, so they
        // meet a pawn's root capsule and no hurt shape.
        if !COLLISION_FIELDS.iter().any(|f| bi.contains_key(*f)) {
            bi = Props::new();
            bi.insert("CollisionProfileName".to_owned(), Value::Str("OverlapAllDynamic".to_owned()));
        }
        let body = resolve_body(&bi, table)?;
        let prof = body.prof.as_deref();
        let obj = body.obj.as_deref();
        let resp = body.resp.as_ref();
        let kind: BoxKind;
        let mut overlaps: Option<Vec<String>> = None;
        let custom_hit = prof == Some("Custom") && obj == Some("HitBox");
        if prof == Some("OverlapAllDynamic") && reached {
            // a shape of that profile a Hitframe reaches: a hit shape whose overlaps are the HitBox profile's channels
            // its body overlaps (Overlap alone. Unverified: a Block response is not counted, as for the detectors)
            kind = BoxKind::Hit;
            overlaps = Some(overlapped(resp, &v)?);
        } else if prof == Some("HitBox") || prof == Some("KnockBack") {
            kind = BoxKind::Hit;
        } else if obj == Some("HurtBox") && resp_of(resp, "HitBox") == Some("ECR_Overlap") {
            // a hurt shape by what the engine tests, whatever the profile's name
            if body.enabled.as_deref() == Some("NoCollision") {
                return fail(format!("{name}: on the HurtBox channel with NoCollision (not handled)"));
            }
            kind = BoxKind::Hurt;
        } else if custom_hit && resp.is_some_and(|r| body_diff(r, hitbox_resp(table)).is_empty()) {
            kind = BoxKind::Hit;
        } else if custom_hit
            && resp_of(resp, "HurtBox") != Some("ECR_Overlap")
            && (resp_of(resp, "ProjectileDestructible") == Some("ECR_Overlap")
                || resp_of(resp, "ItemDestructible") == Some("ECR_Overlap"))
        {
            // a detector: written only when a Hitframe's tag reaches it
            // (GetHitBoxes: with Use 2D Hitboxes a tag other than FullBody finds tag + "2D"; World.hitSpheres)
            if !reached {
                continue;
            }
            kind = BoxKind::Hit;
            let o = overlapped(resp, &v)?;
            // the sim tests a hit shape that overlaps Pawn against the victim's root capsule (World.meleeTest), which
            // BaseFighter's construction script sets to Ignore the HitBox object type: not this shape's rule
            if o.iter().any(|c| c == "Pawn") {
                return fail(format!("detector {v} of object type HitBox overlaps Pawn (not handled)"));
            }
            overlaps = Some(o);
        } else {
            continue;
        }
        let mut bone: Option<String> = k.attach.get(&v).cloned().flatten();
        let is_box = c.ty == "BoxComponent";
        if is_box && kind != BoxKind::Hit {
            return fail(format!("BoxComponent {name} is a hurt box (not handled)"));
        }
        // A shape attached to the root capsule (Pawn_BatmanV2's HurtBoxGlide): capsule space at the capsule's scale
        // (none: the actor's alone), not the mesh's (BoxSampler.onCapsule)
        let on_cap = k.scs_parent.get(&v).and_then(|x| x.as_deref()) == Some("CollisionCylinder");
        if on_cap && bone.as_deref().is_some_and(|b| b != "None") {
            return fail(format!(
                "{v}: on the root capsule with AttachToName {} (not handled)",
                bone.unwrap_or_default()
            ));
        }
        let abs_scale = truthy(get(p, "bAbsoluteScale"));
        let un = if abs_scale || on_cap { 1.0 } else { k.mesh_scale };
        // A capsule stays a capsule for a hurt shape and an OverlapAllDynamic hit shape (Pawn_C023B's ChainSawHitBox),
        // which the sim tests against the victim's root capsule; every other hit capsule is a sphere of its radius
        // (the 2D ones extruded along the depth)
        let keep_cap = kind == BoxKind::Hurt || prof == Some("OverlapAllDynamic");
        let order = k.scs_order.iter().position(|x| *x == v).map_or(-1, |i| i as i32);
        let nn = |key: &str| get(p, key).filter(|x| !x.is_null());
        let mut shape = new_shape(v.clone(), kind, bone.clone(), tags.clone());
        shape.order = Some(order);
        shape.is_2d = v.ends_with("2D");
        shape.capsule = c.ty == "CapsuleComponent" && keep_cap;
        shape.abs_scale = Some(abs_scale);
        shape.radius = num_or(nn("SphereRadius").or_else(|| nn("CapsuleRadius")), 0.0) * un;
        shape.half_height = num_or(nn("CapsuleHalfHeight"), 0.0) * un;
        shape.overlaps = overlaps;
        if on_cap {
            shape.on_capsule = Some(true);
            shape.bone = None;
            bone = None;
        }
        if is_box {
            let e = nn("BoxExtent");
            // Pawn_Jake's PropHitBox1: no BoxExtent in the data and no notify's HitBoxTags reaches it: left out, not
            // built on the class default
            if e.is_none() && !reached {
                continue;
            }
            let Some(e) = e else {
                return fail(format!(
                    "BoxComponent {name} has no BoxExtent in the data (the class default is not read)"
                ));
            };
            shape.is_box = Some(true);
            shape.extent = Some(vec_of(Some(e)).to_vec());
            shape.abs_rot = Some(truthy(get(p, "bAbsoluteRotation")));
        }
        let mut sh = Placed {
            shape,
            loc: vec_of(get(p, "RelativeLocation")),
            rot: rot_of(get(p, "RelativeRotation")),
            vb: None,
            srot: None,
        };
        attach_bone(&mut sh, bone, k.bones, k.sockets, &v)?;
        shapes.push(sh);
    }
    if !shapes.iter().any(|s| s.shape.kind == BoxKind::Hurt) {
        return fail("no hurt shape among the pawn's components (no hit box could reach it)");
    }
    Ok(shapes)
}

/// The sphere and capsule components attached to an actor's mesh component that are hit shapes by the pawn's rule,
/// on the bone of their AttachSocketName; a capsule stays a capsule. Radius and half height x the mesh scale (the
/// component's own RelativeScale3D is the handler's HitBoxScale at run time).
pub fn actor_shapes(
    cls: &str,
    comps: &Props,
    mesh_comp: &str,
    mesh_scale: f64,
    bones: &Bones,
    sockets: &Map<Socket>,
    table: &CollisionTable,
) -> Result<Vec<Placed>, KindError> {
    let mut shapes = Vec::new();
    for (name, c) in comps {
        if get(c, "AttachParent").and_then(Value::as_str) != Some(mesh_comp) {
            continue;
        }
        let has = |k: &str| get(c, k).is_some();
        if has("BoxExtent") {
            return fail(format!("{cls}: BoxComponent {name} on the mesh (not handled)"));
        }
        if !has("SphereRadius") && !has("CapsuleRadius") {
            continue;
        }
        let bi = get(c, "BodyInstance").and_then(Value::as_obj).cloned().unwrap_or_default();
        let body = resolve_body(&bi, table)?;
        if !plain_hit(body.prof.as_deref(), body.obj.as_deref(), body.resp.as_ref(), table)? {
            continue;
        }
        if truthy(get(c, "bAbsoluteScale")) || truthy(get(c, "bAbsoluteRotation")) {
            return fail(format!("{cls}: {name} has an absolute scale or rotation (not handled)"));
        }
        let capsule = has("CapsuleRadius");
        let nn = |key: &str| get(c, key).filter(|x| !x.is_null());
        let bone = nn("socket").and_then(Value::as_str).map(str::to_owned);
        let mut shape = new_shape(name.clone(), BoxKind::Hit, bone.clone(), strings(get(c, "ComponentTags")));
        shape.capsule = capsule;
        shape.radius = num_or(nn(if capsule { "CapsuleRadius" } else { "SphereRadius" }), 0.0) * mesh_scale;
        shape.half_height = num_or(nn("CapsuleHalfHeight"), 0.0) * mesh_scale;
        if bone.is_none() {
            return fail(format!("{cls}: {name} on the mesh with no socket (not handled)"));
        }
        let mut sh = Placed {
            shape,
            loc: vec_of(get(c, "RelativeLocation")),
            rot: rot_of(get(c, "RelativeRotation")),
            vb: None,
            srot: None,
        };
        attach_bone(&mut sh, bone, bones, sockets, &format!("{cls}.{name}"))?;
        shapes.push(sh);
    }
    if shapes.is_empty() {
        return fail(format!("{cls}: no hit shape on {mesh_comp}"));
    }
    Ok(shapes)
}
