//! Tom and Jerry's spawned actors other than Jerry; the sim's kinds: rocket.rs,
//! kicked.rs, fishing.rs, dynamite.rs, trap.rs, champagne.rs (mvs-sim chars/tom_and_jerry).
//!
//! Materials: a skeletal mesh that is one of the skin's sub-meshes (Jerry_SkelMesh) wears the skin's set for it in the
//! owner's colour, as Jerry_SpawnedActor's SetMaterial @12958. The components a class's SetTeamColor gives slot 0 by
//! the owner's colour (IsLocalOnline the local set, else colour index 0 red, 1 blue) are in TEAM_MATS; a reflect's new
//! owner makes the views anew (SetTeamColor runs again: JerryRocket @10484, JerryKickedRocket @8057, TomAimRocket1
//! @4508, Dynamite @5693). The others wear their own materials. TEAM_MATS: the components whose slot 0 the class's
//! SetTeamColor sets by the owner's colour, and the actors entry of that pick: JerryRocket / JerryKickedRocket
//! RocketMesh @10289 / @7671 (slot 1 keeps Prop_MIC), TomAimRocket1 RocketMesh @4812, Dynamite_SpawnedActor (and its
//! CantSplit child) DynamiteMesh @5507 (the local player's Yellow), MouseTrap_SpawnedActor MouseTrapMesh @3692.
//!
//! The classes drawn as their mesh components: JerryRocket_C, JerryKickedRocket_C: JerryMesh (Jerry_SkelMesh,
//! AnimationSingleNode Tom_and_Jerry_Rocket_Jerry_Struggle, looping) and RocketMesh (JerryRocket) on the Capsule;
//! PS_TomJerry_Rocket is the shared projectile path's (its ParticleSystem component). TomAimRocket1_C: RocketMesh
//! (JerryRocket_NoRope). Tom_and_Jerry_TennisBall_C, _Reflected_C: StaticMesh (TennisBall, Z 1.48, scale 1.2); their
//! particle components are the effects side's (mvs-render fx/chars/tom_and_jerry.rs).
//!
//! Dynamite_SpawnedActor_C, CantSplitDynamite_SpawnedActor_C: DynamiteMesh (Dynamite, scale 1.6, Dynamite_Spin
//! looping), hidden while Dynamite::hidden (ReflectedMulticast @5084 DynamiteMesh.SetVisibility(false)). The wick:
//! BeginPlay @436 SpawnEmitterAttached(PS_TomJerry_Wick, DynamiteMesh, socket Wick, KeepRelativeOffset), destroyed
//! @5129 with the mesh's hiding. The socket is Dynamite_Skeleton's SkeletalMeshSocket_0 (Wick on the bone Root at
//! (1.1382835, -0.0000992, 38.81231), no rotation, scale 1), entered here by hand (the data has no sockets of an
//! actor's skeletal mesh); the bone's pose is Dynamite_Spin's. The view enters the socket's world transform as the
//! bone [`WICK`] the effects side follows.
//!
//! FishingLure_C: JerryMesh (the struggle loop), Lure (Lure) and LureNoRope (LureBase) on it. JerryMesh's relative
//! rotation is BeginPlay's (Lure::jerry_left: rot(-90, -180, 0) @5636 facing left, the template's (90, 0, 0) @5693):
//! the class's components with it, so Lure and LureNoRope turn with it. SetVisualsMulticast: true @13146 hides
//! JerryMesh (SetHiddenInGame, children not propagated) and Lure, false @13240 LureNoRope (Lure's jerry_mesh /
//! lure_mesh / no_rope).
//! Its Cable (UCableComponent under Lure) is the effects side's component mesh
//! (mvs-render fx/chars/tom_and_jerry.rs); the view enters the owner's Tom_FishingPole bone Rod5 as the lure's bone
//! [`ROD`] for its end.
//!
//! MouseTrap_SpawnedActor_C (World actor): MouseTrapMesh (MouseTrap at (0, -123, -54.7), scale 1.5, MouseTrap_Idle;
//! MouseTrap_SnapFast when sprung: the hit gate @4492, MulticastDamageTaken @5439), Cheese on it (SCS AttachToName
//! Root: the bone's transform, at (0, -0.396, 10.295), scale 0.75, while MouseTrap::cheese: hidden by the snap @4534,
//! shown by AddCheeseVisual @5351) and PS_TomJerryMouseTrapAura (no socket, Z 10, scale 0.75: world scale 1.125;
//! K2_DestroyComponent at Detonate @5135; the effects side's, on the bone [`AURA`] the view enters). The Decal is
//! bHiddenInGame. The actor's rotation: Comp_Spawner HandleActorPostSpawn @1385-2362, yaw SelectFloat(90, -90, the
//! owner FacingLeft) plus the zero spawn transform's; BeginPlay @1535-1669 multiplies the actor's scale by (1, -1, 1)
//! when the owner faces left (MouseTrap::facing at the spawn; the root Capsule has bAbsoluteScale, so its world scale
//! is that): its components are the mirror image, drawn by matrix composition.

use std::sync::Arc;

use mvs_data::Value;
use mvs_data::value::Props;
use mvs_sim::World;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::actors::{ActorId, ActorRef, ProjectileId};
use mvs_sim::chars::tom_and_jerry::champagne::CORK;
use mvs_sim::chars::tom_and_jerry::dynamite::{CANT_SPLIT_DYNAMITE, DYNAMITE};
use mvs_sim::chars::tom_and_jerry::fishing::LURE;
use mvs_sim::chars::tom_and_jerry::jerry::{JERRY, JERRY_TEMP_INVINCIBLE};
use mvs_sim::chars::tom_and_jerry::kicked::{AIM_ROCKET, KICKED_ROCKET};
use mvs_sim::chars::tom_and_jerry::rocket::ROCKET;
use mvs_sim::chars::tom_and_jerry::trap::TRAP;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::actor_def_owner;
use mvs_vfx::math::{V3, rotation_rows};

use crate::actor_view::{Comp, IDENT, SkinnedActor, StaticView, Xform, comp_of, place_under};
use crate::actors::{ActorComponents, ActorHost, ActorMesh, ActorPlace, Component, ComponentKind, Rows, class_def};
use crate::frame::MeshPose;
use crate::skins::Variant;

pub(super) const STEP: f64 = 1.0 / 60.0;
const DEG: f64 = std::f64::consts::PI / 180.0;

/// The bones the views enter for the effects side (mvs-render fx/chars/tom_and_jerry.rs follows them): the
/// dynamite's Wick socket and the trap's aura place.
pub(crate) const WICK: &str = "tj:Wick";
pub(crate) const AURA: &str = "tj:Aura";

/// The components whose slot 0 the class's SetTeamColor sets, and the actors entry of that pick.
fn team_key(cls: &str, comp: &str) -> Option<&'static str> {
    match (cls, comp) {
        (ROCKET | KICKED_ROCKET, "RocketMesh") => Some("JerryRocket"),
        (AIM_ROCKET, "RocketMesh") => Some("JerryRocket_NoRope"),
        (DYNAMITE | CANT_SPLIT_DYNAMITE, "DynamiteMesh") => Some(DYNAMITE),
        (TRAP, "MouseTrapMesh") => Some("MouseTrap"),
        _ => None,
    }
}

/// An object path's or a reference's asset name (`/Game/A/B.B` -> `B`).
fn asset_name(s: &str) -> &str {
    let s = s.rsplit('/').next().unwrap_or(s);
    s.split('.').next_back().unwrap_or(s).trim_end_matches('\'')
}

/// A component of the class, as a placed mesh component.
fn comp(c: &Component) -> Comp {
    Comp { par: c.parent.clone(), rl: c.loc, rot: c.rot, rs: c.scale }
}

/// The skin fighter `f`'s player picked.
pub(super) fn skin_pick(w: &World, f: FighterId) -> Option<&str> {
    usize::try_from(w.fighter(f).id).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.skin.as_deref())
}

/// A warning printed once a run.
pub(super) fn warn_once(msg: String) {
    static WARNED: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    let mut w = WARNED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !w.contains(&msg) {
        eprintln!("warning: {msg}");
        w.insert(msg);
    }
}

/// `selectedVariant` as a skin's material set pick.
pub(super) fn selected(host: &ActorHost<'_>, w: &World, f: FighterId) -> Variant {
    match host.colour(w, f).variant {
        Variant::Menu => Variant::Local,
        v => v,
    }
}

/// `under`: a child's world transform under its parent's: `pl` the parent's location, `pm` its rows with its world
/// scale in them; `rl` / `rr` / `rs` the child's relative location, rotation rows and scale. The child's location and
/// scaled rows.
pub(super) fn under(pl: V3, pm: &Rows, rl: V3, rr: &Rows, rs: V3) -> (V3, Rows) {
    let ol = std::array::from_fn(|k| pl[k] + rl[0] * pm[k] + rl[1] * pm[3 + k] + rl[2] * pm[6 + k]);
    let om = std::array::from_fn(|n| {
        let (i, c) = (n / 3, n % 3);
        rs[i] * (rr[3 * i] * pm[c] + rr[3 * i + 1] * pm[3 + c] + rr[3 * i + 2] * pm[6 + c])
    });
    (ol, om)
}

/// `unscale`: scaled rows split into unit rows and the per-axis lengths.
pub(super) fn unscale(m: &Rows) -> (Rows, V3) {
    let mut rot = [0.0; 9];
    let mut scale = [1.0; 3];
    for i in 0..3 {
        let l = (m[3 * i].powi(2) + m[3 * i + 1].powi(2) + m[3 * i + 2].powi(2)).sqrt();
        let l = if l == 0.0 || l.is_nan() { 1.0 } else { l };
        scale[i] = l;
        for c in 0..3 {
            rot[3 * i + c] = m[3 * i + c] / l;
        }
    }
    (rot, scale)
}

/// `boneUE`: bone `b` of a skinned view as last set and shown: its world location and rows with its world scale in
/// them.
pub(super) fn bone_ue(sk: &SkinnedActor, b: usize) -> Option<(V3, Rows)> {
    let mut all = Vec::new();
    sk.bones_world(&mut all);
    let x = &all.get(b)?.1;
    Some((x.loc, std::array::from_fn(|n| x.rot[n] * x.scale[n / 3])))
}

/// A UE place as a bone the effects side follows: unit rows and the scale.
fn as_bone(loc: V3, m: &Rows) -> Xform {
    let (rot, scale) = unscale(m);
    Xform { loc, rot, scale }
}

// ---- a class's mesh components ----------------------------------------------------------------------------------

#[derive(Debug)]
enum PartMesh {
    Sk { v: Box<SkinnedActor>, clip: Option<usize> },
    St(StaticView),
}

/// One mesh component's view.
#[derive(Debug)]
struct Part {
    c: Comp,
    name: String,
    m: PartMesh,
}

impl Part {
    fn set_visible(&mut self, on: bool) {
        match &mut self.m {
            PartMesh::Sk { v, .. } => v.visible = on,
            PartMesh::St(v) => v.visible = on,
        }
    }

    fn set(&mut self, x: &Xform) {
        match &mut self.m {
            PartMesh::Sk { v, .. } => v.set(x),
            PartMesh::St(v) => v.set(x),
        }
    }

    fn draw(&self, out: &mut Vec<MeshPose>) {
        match &self.m {
            PartMesh::Sk { v, .. } => v.draw(out),
            PartMesh::St(v) => v.draw(out),
        }
    }

    fn sk(&self) -> Option<&SkinnedActor> {
        match &self.m {
            PartMesh::Sk { v, .. } => Some(&**v),
            PartMesh::St(_) => None,
        }
    }
}

/// `skelView`: a skin sub-mesh in the skin's set (the owner's pick), else the class's team pick (`team`), else the
/// mesh's own materials. The sub-mesh is the component's (default) mesh among the default skin's entries
/// (ActorComponents' sub_mesh), drawn as the applied skin's part of that key.
fn skel_view(
    host: &ActorHost<'_>,
    w: &World,
    from: FighterId,
    owner: FighterId,
    sub: Option<&str>,
    mesh: &str,
    team: Option<&str>,
) -> Option<SkinnedActor> {
    let d = host.draws_of(w, from)?;
    if let Some(key) = sub
        && let Ok(skin) = crate::skins::skin_of(&w.char_data(from).fighter, skin_pick(w, from))
        && let Some(p) = skin.parts.iter().find(|p| p.key.eq_ignore_ascii_case(key))
    {
        let info = p.path.as_deref().and_then(|x| host.mesh(x))?;
        let keys = if p.own {
            d.actor(&p.mesh, None).map(|a| a.1)
        } else {
            d.set(&skin.name, &p.key, selected(host, w, owner)).ok()
        }?;
        return Some(SkinnedActor::new(info, &keys));
    }
    let (path, own) = d.actor(asset_name(mesh), None)?;
    let keys = match team {
        Some(k) => {
            let v = host.variant(w, owner);
            d.actor(k, (v != "local").then_some(v))?.1
        }
        None => own,
    };
    Some(SkinnedActor::new(host.mesh(&path)?, &keys))
}

/// `parts`: the views of the components `names` (a skeletal one with its AnimToPlay clip), `raw` the class's
/// components where a graph changed one (comp_of on them), else the resolved ones.
#[allow(clippy::too_many_arguments, reason = "the arguments of the parts' views")]
fn parts(
    host: &mut ActorHost<'_>,
    w: &World,
    from: FighterId,
    owner: FighterId,
    cs: &ActorComponents,
    raw: Option<&Props>,
    names: &[&str],
    cls: &str,
) -> Vec<Part> {
    let mut out = Vec::new();
    for &n in names {
        let Some(c) = cs.get(n) else { continue };
        let place = match raw {
            Some(r) => match comp_of(r, n) {
                Some(p) => p,
                None => continue,
            },
            None => comp(c),
        };
        match &c.kind {
            ComponentKind::SkeletalMesh { mesh, sub_mesh, anim } => {
                let Some(v) = skel_view(host, w, from, owner, sub_mesh.as_deref(), mesh, team_key(cls, n)) else {
                    eprintln!("warning: Tom and Jerry: {mesh} is not drawn (its mesh or materials are not loaded)");
                    continue;
                };
                let clip = anim.as_deref().and_then(|a| {
                    let c = host.clips_of(w, from).and_then(|c| c.get(a));
                    if c.is_none() {
                        eprintln!("warning: Tom and Jerry: clip missing: {a}");
                    }
                    c
                });
                out.push(Part { c: place, name: n.to_owned(), m: PartMesh::Sk { v: Box::new(v), clip } });
            }
            ComponentKind::StaticMesh { mesh, .. } => {
                let variant = selected(host, w, owner).key();
                let Some(v) = StaticView::new(host, w, from, asset_name(mesh), variant) else {
                    eprintln!("warning: Tom and Jerry: {mesh} is not drawn (its mesh or materials are not loaded)");
                    continue;
                };
                out.push(Part { c: place, name: n.to_owned(), m: PartMesh::St(v) });
            }
            _ => {}
        }
    }
    out
}

/// What a components rule adds to the shared drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Plain,
    /// The graph's hide, and the Wick socket.
    Dynamite,
    /// The parts' own visibility.
    Lure,
}

/// `componentsRule`: a projectile class drawn as its mesh components, each animation looping from the spawn.
#[derive(Debug)]
struct ComponentsMesh {
    id: ProjectileId,
    from: FighterId,
    owner: FighterId,
    cls: &'static str,
    names: &'static [&'static str],
    cs: ActorComponents,
    raw: Option<Props>,
    parts: Vec<Part>,
    sampled: i32,
    kind: Kind,
    wick: Option<Xform>,
    /// The lure's: the owner's Tom_FishingPole bone Rod5 as last drawn (the fish line's end, mvs-render's).
    rod: Option<Xform>,
}

const WICK_LOC: V3 = [1.1382835, -0.000099182, 38.81231];
const LURE_PARTS: &[&str] = &["JerryMesh", "Lure", "LureNoRope"];
/// The fish line's end: Tom_FishingPole's bone Rod5 (entered as the lure's bone for the effects side).
const POLE: &str = "Tom_FishingPole";
pub(crate) const ROD: &str = "Rod5";

fn components(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    cs: &ActorComponents,
    names: &'static [&'static str],
    kind: Kind,
) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let from = actor_def_owner(w, owner, pr.cls);
    let cls = w.names.str(pr.cls);
    let cls: &'static str =
        [ROCKET, KICKED_ROCKET, AIM_ROCKET, LURE, DYNAMITE, CANT_SPLIT_DYNAMITE, TENNIS_BALL, TENNIS_BALL_R]
            .into_iter()
            .find(|c| *c == cls)?;
    // FishingLure_C: JerryMesh's relative rotation of BeginPlay facing left (the class's components with it)
    let raw = match &pr.ext {
        ProjectileExt::Lure(l) if l.jerry_left => {
            let mut r = class_def(w, a)?.components.clone();
            if let Some(Value::Obj(j)) = r.get_mut("JerryMesh") {
                let mut rot = Props::new();
                rot.insert("Pitch".to_owned(), Value::Num(-90.0));
                rot.insert("Yaw".to_owned(), Value::Num(-180.0));
                rot.insert("Roll".to_owned(), Value::Num(0.0));
                j.insert("RelativeRotation".to_owned(), Value::Obj(rot));
            }
            Some(r)
        }
        _ => None,
    };
    let ps = parts(host, w, from, owner, cs, raw.as_ref(), names, cls);
    if ps.is_empty() {
        return None;
    }
    Some(Box::new(ComponentsMesh {
        id,
        from,
        owner,
        cls,
        names,
        cs: cs.clone(),
        raw,
        parts: ps,
        sampled: -1,
        kind,
        wick: None,
        rod: None,
    }))
}

use mvs_sim::chars::tom_and_jerry::tennis::{TENNIS_BALL, TENNIS_BALL_REFLECTED as TENNIS_BALL_R};

fn rocket_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    components(host, w, a, cs, &["JerryMesh", "RocketMesh"], Kind::Plain)
}

fn aim_rocket_mesh(
    host: &mut ActorHost<'_>,
    w: &World,
    a: ActorRef,
    cs: &ActorComponents,
) -> Option<Box<dyn ActorMesh>> {
    components(host, w, a, cs, &["RocketMesh"], Kind::Plain)
}

fn dynamite_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    components(host, w, a, cs, &["DynamiteMesh"], Kind::Dynamite)
}

fn ball_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    components(host, w, a, cs, &["StaticMesh"], Kind::Plain)
}

fn lure_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    components(host, w, a, cs, LURE_PARTS, Kind::Lure)
}

impl ActorMesh for ComponentsMesh {
    fn place(
        &mut self,
        host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(pr) = w.actors.projectiles.get(self.id) else { return };
        // (a reflect gives it a new owner: SetTeamColor and the skin's set again; made anew, once a reflect)
        if let Some(o) = pr.owner
            && o != self.owner
        {
            self.owner = o;
            self.parts = parts(host, w, self.from, o, &self.cs, self.raw.as_ref(), self.names, self.cls);
            self.sampled = -1;
        }
        let shown = shown && !matches!(&pr.ext, ProjectileExt::Dynamite(d) if d.hidden);
        for q in &mut self.parts {
            let own = match (&pr.ext, q.name.as_str()) {
                (ProjectileExt::Lure(l), "JerryMesh") => l.jerry_mesh,
                (ProjectileExt::Lure(l), "Lure") => l.lure_mesh,
                (ProjectileExt::Lure(l), "LureNoRope") => l.no_rope,
                _ => true,
            };
            q.set_visible(shown && (self.kind != Kind::Lure || own));
        }
        if !shown {
            return;
        }
        if pr.age != self.sampled {
            self.sampled = pr.age;
            if let Some(clips) = host.clips_of(w, self.from) {
                for q in &mut self.parts {
                    if let PartMesh::Sk { v, clip: Some(c) } = &mut q.m {
                        v.play(clips, *c, f64::from(pr.age) * STEP, true);
                    }
                }
            }
        }
        let (x, y) = at.at(alpha);
        for q in &mut self.parts {
            let xf = place_under(&q.c, &at.rot, at.scale, x, y);
            q.set(&xf);
            q.draw(out);
        }
        // the fish line's end: the pole's Tip on Rod5 (the effects side adds the Tip's offset)
        if self.kind == Kind::Lure {
            self.rod = host.montage_bone(w, self.owner, POLE, ROD);
        }
        // the Wick socket: the bone Root's world transform, then the socket's relative location under it
        if self.kind == Kind::Dynamite {
            let q = self.parts.iter().find(|p| p.name == "DynamiteMesh").and_then(Part::sk);
            let b = q.and_then(|s| s.bone("Root").and_then(|b| bone_ue(s, b)));
            match b {
                Some((loc, l)) => {
                    let (ol, om) = under(loc, &l, WICK_LOC, &IDENT, [1.0; 3]);
                    self.wick = Some(as_bone(ol, &om));
                }
                None => {
                    warn_once("Tom and Jerry: DynamiteMesh has no bone Root (PS_TomJerry_Wick not drawn)".to_owned())
                }
            }
        }
    }

    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        if let Some(x) = self.wick {
            out.push((Arc::from(WICK), x));
        }
        if let Some(x) = self.rod {
            out.push((Arc::from(ROD), x));
        }
    }

    fn split(&self) -> bool {
        true
    }
}

// ---- JerryCork_C ----
// StaticMesh (Champagne_Cork at (-17.33, 0, 0), rot (pitch -90, yaw -360)) on the Capsule. The pop's Spin timeline
// (Spin.PlayFromStart @34; Spin_Template: TimelineLength 0.35, CurveFloat_0 linear (0, 0) to (0.5, 1)) sets the
// mesh's relative rotation to pitch Lerp(-90, BounceLeft ? 260 : -320, Spin) @2353-2498 and its relative scale
// VLerp(1, 0.5, Spin) @2550-2613. Its ParticleSystem / ParticleSystem2 are the effects side's.
// Unverified: the timeline's value at a tick is taken as (tick - pop_tick) / 60 s on its curve (the timeline
// component's tick against the pop's within a frame not read).
const SPIN_LENGTH: f64 = 0.35;
const SPIN_CURVE_END: f64 = 0.5;

#[derive(Debug)]
struct CorkMesh {
    id: ProjectileId,
    part: Part,
}

fn cork_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Projectile(id) = a else { return None };
    let pr = w.actors.projectiles.get(id)?;
    let owner = pr.owner?;
    let from = actor_def_owner(w, owner, pr.cls);
    let part = parts(host, w, from, owner, cs, None, &["StaticMesh"], CORK).into_iter().next()?;
    if !matches!(part.m, PartMesh::St(_)) {
        return None;
    }
    Some(Box::new(CorkMesh { id, part }))
}

impl ActorMesh for CorkMesh {
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        at: &ActorPlace,
        alpha: f64,
        shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        self.part.set_visible(shown);
        if !shown {
            return;
        }
        if let Some(pr) = w.actors.projectiles.get(self.id)
            && let ProjectileExt::JerryCork(k) = &pr.ext
            && k.pop_tick >= 0
        {
            let a = (f64::from((w.flow.tick - k.pop_tick).max(0)) * STEP).min(SPIN_LENGTH) / SPIN_CURVE_END;
            let to = if k.bounce_left { 260.0 } else { -320.0 };
            self.part.c.rot = rotation_rows((-90.0 + (to + 90.0) * a) * DEG, 0.0, 0.0);
            self.part.c.rs = [1.0 - 0.5 * a; 3];
        }
        let (x, y) = at.at(alpha);
        let xf = place_under(&self.part.c, &at.rot, at.scale, x, y);
        self.part.set(&xf);
        self.part.draw(out);
    }
}

// ---- MouseTrap_SpawnedActor_C ----

const SNAP: &str = "MouseTrap_SnapFast";
const AURA_LOC: V3 = [0.0, 0.0, 10.0];
const AURA_SCALE: V3 = [0.75; 3];

#[derive(Debug)]
struct TrapMesh {
    id: ActorId,
    from: FighterId,
    trap: Part,
    cheese: Option<Part>,
    snap: Option<usize>,
    age: i32,
    sprung_at: i32,
    aura: Option<Xform>,
}

fn trap_mesh(host: &mut ActorHost<'_>, w: &World, a: ActorRef, cs: &ActorComponents) -> Option<Box<dyn ActorMesh>> {
    let ActorRef::Actor(id) = a else { return None };
    let x = w.actors.actors.get(id)?;
    let WorldActorState::MouseTrap(t) = &x.state else { return None };
    let owner = x.owner;
    let from = actor_def_owner(w, owner, x.cls);
    let mut ps = parts(host, w, from, owner, cs, None, &["MouseTrapMesh", "Cheese"], TRAP).into_iter();
    let mut trap = ps.next().filter(|p| p.name == "MouseTrapMesh" && p.sk().is_some())?;
    let mut cheese = ps.next();
    let snap = host.clips_of(w, from).and_then(|c| c.get(SNAP)); // @4492 / @5439
    if snap.is_none() {
        eprintln!("warning: Tom and Jerry: clip missing: {SNAP}");
    }
    if let Some(c) = &mut cheese {
        c.set_visible(t.cheese);
    }
    if let PartMesh::Sk { v, clip: Some(c) } = &mut trap.m
        && let Some(clips) = host.clips_of(w, from)
    {
        v.play(clips, *c, 0.0, true);
    }
    Some(Box::new(TrapMesh { id, from, trap, cheese, snap, age: 0, sprung_at: -1, aura: None }))
}

impl ActorMesh for TrapMesh {
    /// The trap's actors() step: the age, the snap, the clip and the cheese.
    fn tick(&mut self, host: &mut ActorHost<'_>, w: &World) {
        let Some(x) = w.actors.actors.get(self.id).filter(|x| !x.dead) else { return };
        let WorldActorState::MouseTrap(t) = &x.state else { return };
        self.age += 1;
        if t.sprung && self.sprung_at < 0 {
            self.sprung_at = self.age;
        }
        if let PartMesh::Sk { v, clip } = &mut self.trap.m
            && let Some(clips) = host.clips_of(w, self.from)
        {
            match (self.sprung_at >= 0, self.snap, *clip) {
                (true, Some(s), _) => v.play(clips, s, f64::from(self.age - self.sprung_at) * STEP, false),
                (_, _, Some(c)) => v.play(clips, c, f64::from(self.age) * STEP, true),
                _ => {}
            }
        }
        if let Some(c) = &mut self.cheese {
            c.set_visible(t.cheese);
        }
    }

    /// `placeTrap`: the actor (yaw -90 facing right, +90 and its Y axis negated facing left), the mesh under its
    /// parent chain, the aura under the mesh, the cheese under the mesh's Root bone.
    fn place(
        &mut self,
        _host: &mut ActorHost<'_>,
        w: &World,
        _at: &ActorPlace,
        _alpha: f64,
        _shown: bool,
        out: &mut Vec<MeshPose>,
    ) {
        let Some(x) = w.actors.actors.get(self.id) else { return };
        let WorldActorState::MouseTrap(t) = &x.state else { return };
        let left = t.facing < 0.0;
        let mut tm = rotation_rows(0.0, (if left { 90.0 } else { -90.0 }) * DEG, 0.0);
        // SetSizeScale's SetActorScale3D (k, k, k) @566 comes after BeginPlay's mirror and writes the scale whole: a
        // grown trap has lost the (1, -1, 1) and is drawn unmirrored at k.
        match x.size_scale {
            None if left => {
                for v in &mut tm[3..6] {
                    *v = -*v;
                }
            }
            Some(g) => {
                for v in &mut tm {
                    *v *= f64::from(g);
                }
            }
            None => {}
        }
        let ta = [0.0, -t.x, t.y];
        let c = &self.trap.c;
        let pr: Rows = std::array::from_fn(|k| c.par.rot[k]).map(f64::from);
        let (pl, pm) = under(ta.map(f64::from), &tm, c.par.loc.map(f64::from), &pr, c.par.scale.map(f64::from));
        let (ml, mm) = under(pl, &pm, c.rl, &c.rot, c.rs);
        self.trap.set(&Xform { loc: ml, rot: mm, scale: [1.0; 3] });
        self.trap.draw(out);
        let (al, am) = under(ml, &mm, AURA_LOC, &IDENT, AURA_SCALE);
        self.aura = Some(as_bone(al, &am));
        let Some(ch) = &mut self.cheese else { return };
        if !matches!(&ch.m, PartMesh::Sk { v, .. } if v.visible) {
            return;
        }
        let sk = self.trap.sk().expect("the trap's skinned view");
        let Some((bl, bm)) = sk.bone("Root").and_then(|b| bone_ue(sk, b)) else {
            warn_once("Tom and Jerry: MouseTrap has no bone Root (the Cheese not drawn)".to_owned());
            return;
        };
        let (cl, cm) = under(bl, &bm, ch.c.rl, &ch.c.rot, ch.c.rs);
        ch.set(&Xform { loc: cl, rot: cm, scale: [1.0; 3] });
        ch.draw(out);
    }

    fn bones(&self, out: &mut Vec<(Arc<str>, Xform)>) {
        if let Some(x) = self.aura {
            out.push((Arc::from(AURA), x));
        }
    }

    fn split(&self) -> bool {
        true
    }
}

/// The classes' rules of this file.
pub(super) const RULES: &[(&str, crate::actors::MeshRule)] = &[
    (ROCKET, crate::actors::MeshRule { rot: None, mesh: Some(rocket_mesh) }),
    (KICKED_ROCKET, crate::actors::MeshRule { rot: None, mesh: Some(rocket_mesh) }),
    (AIM_ROCKET, crate::actors::MeshRule { rot: None, mesh: Some(aim_rocket_mesh) }),
    (LURE, crate::actors::MeshRule { rot: None, mesh: Some(lure_mesh) }),
    (DYNAMITE, crate::actors::MeshRule { rot: None, mesh: Some(dynamite_mesh) }),
    (CANT_SPLIT_DYNAMITE, crate::actors::MeshRule { rot: None, mesh: Some(dynamite_mesh) }),
    (TENNIS_BALL, crate::actors::MeshRule { rot: None, mesh: Some(ball_mesh) }),
    (TENNIS_BALL_R, crate::actors::MeshRule { rot: None, mesh: Some(ball_mesh) }),
    (CORK, crate::actors::MeshRule { rot: None, mesh: Some(cork_mesh) }),
    (TRAP, crate::actors::MeshRule { rot: None, mesh: Some(trap_mesh) }),
    (JERRY, crate::actors::MeshRule { rot: None, mesh: Some(super::jerry::jerry_mesh) }),
    (JERRY_TEMP_INVINCIBLE, crate::actors::MeshRule { rot: None, mesh: Some(super::jerry::jerry_mesh) }),
];
