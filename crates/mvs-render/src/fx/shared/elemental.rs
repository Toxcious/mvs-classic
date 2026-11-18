//! Buff_Elemental_Fire / Ice on spawned actors: the emitters of the elemental
//! buffs a projectile, an item or a World actor holds (mvs-sim combat/elem.rs), a shared class's effects, made for every
//! match after the characters'.
//!
//! Buff_ElementalParent ReceiveBeginPlay @3484 -> @362: with authority, not OnHitElementOnly (false in every class
//! default) and a valid owner, HandleElementalBuffVisuals @466 -> @16 -> the child's ElementalBuffVisuals (once, at
//! the buff's BeginPlay). ReceiveDestroyed @3499 and the DestroyAfterApplied path @25 -> DestroyElementalBuffVFX
//! @3510 -> @3433: every component of ElementalBuffVFX K2_DestroyComponent (gone at once).
//! Buff_Elemental_Fire ElementalBuffVisuals @9470 (Buff_Elemental_Ice @8893 the same shape):
//! - the owner has the proof buff (Fireproof @9551 / Iceproof @8974): nothing; the elemental is the owner's own
//!   passive (HasPassiveBuff @9798): nothing (mvs-sim combat/elem.rs elem_shows);
//! - AttachToComponent = the owner's BPI_GetVFXAttachComponent (SpawnedActorParent: its Capsule; PortalParent: null,
//!   which draws nothing @10041); the owner's BPI_GetElementalVFXOverride(the class) @10150 (SpawnedActorParent: its
//!   ElementalVFXOverrides map entry; Tom_and_Jerry_TennisBall: IceVFXOverrides for its IceBuff, else the parent's);
//! - an override with a ParticleSystem: SpawnEmitterAttached(it, AttachPointName, Location, Rotation, Scale) @10336;
//!   else SpawnEmitterAttached(PS_Creature_FireWallProjectileBuff, no socket, 0, 0, scale 2) @11409 (Ice:
//!   PS_SupermanIceBreath_IceCube at scale 0.4 @10828), KeepRelativeOffset, auto destroy, kept in ElementalBuffVFX;
//! - then, when the owner's owner has a Comp_Pawn_SkinManager (@10591 / @11635), Team = SelectFloat(0.6,
//!   GetColorIndex, IsLocalOnlinePlayer) @10936 / @11980 and PF_Post_Event_At_Location(
//!   sfx_shared_combat_projectile_ignite (Ice: sfx_cha_superman_icebreath_impact), the buff actor's location)
//!   @11163 / @12207; without one, neither.
//!
//! The sim has no event for the buff's BeginPlay: a buff is seen here at its first sight on its host (the host's list
//! and sequence number, and the class), and the proof and passive tests are made then.
//! Unverified: the Capsule is taken at the host's root place and rotation, its RelativeLocation through them, and its
//! world scale as its RelativeScale3D with bAbsoluteScale (SpawnedActorParent's template), else times the root's; a
//! buff destroyed and made again in one tick (CanOverride) keeps the first's emitter. A host's tunnel carry
//! (Buff_ProjectileThroughTunnel's hide of the root's children) does not hide it.
//! PF_Post_Event_At_Location's sound is queued on `Fx::sounds` at the
//! buff's first sight, after the emitter's (loaded or not), for a host with an owner of its own.
//! Unverified: the buff actor's location (where the sound is posted) is taken as its host's root place.
//! Not here: Audio_Ignite_Start / Audio_Ignite_End (SFX_Ignite on the owner, a do-once around the overlap) are on the
//! TeamIndex -1 branch of ApplyFireOverTime @2277 and the actor end overlap @14581, which the sim does not port;
//! OnTeamColorUpdated @14426 (no team change in a match).
//!
//! Buff_Elemental_Electric (Static Electricity's charged projectile, Garnet's zone) draws nothing: its
//! ElementalBuffVisuals @1449 is the parent's call alone (Buff_ElementalParent ElementalBuffVisuals @3539, an empty
//! pop_flow), then the return @1927; no emitter, no Team, no sound. The charged hands are `electric.rs`'s.

use std::collections::BTreeSet;

use indexmap::IndexMap;
use mvs_data::value::{Props, Value};
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::world_actor_place_of;
use mvs_sim::combat::buff_class::BuffClass;
use mvs_sim::combat::elem::elem_shows;
use mvs_sim::fighter::FighterId;
use mvs_sim::fighter::common::actor_def;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};
use mvs_vfx::{Attachment, SystemId};
use mvs_view::actors::actor_rot;

use crate::fx::Fx;
use crate::fx::attach::{rot_rows, xyz};
use crate::fx::chars::{CharFxHooks, ProjRule};

/// The elemental classes a host's buffs are read for, each with its default emitter and scale; None: its
/// ElementalBuffVisuals spawns nothing (Buff_Elemental_Electric, above).
const CLASSES: [(BuffClass, Option<(&str, f64)>); 3] = [
    (BuffClass::BuffElementalFire, Some(("PS_Creature_FireWallProjectileBuff", 2.0))),
    (BuffClass::BuffElementalIce, Some(("PS_SupermanIceBreath_IceCube", 0.4))),
    (BuffClass::BuffElementalElectric, None),
];

/// A class's ElementalBuffVisuals default emitter and scale; None for a class that draws nothing or is no elemental.
fn visuals(cls: BuffClass) -> Option<(&'static str, f64)> {
    CLASSES.iter().find(|(c, _)| *c == cls).and_then(|(_, v)| *v)
}

/// The sound event each class posts at its buff's first sight (PF_Post_Event_At_Location).
fn ignite_sound(cls: BuffClass) -> &'static str {
    if cls == BuffClass::BuffElementalFire {
        "sfx_shared_combat_projectile_ignite"
    } else {
        "sfx_cha_superman_icebreath_impact"
    }
}

/// BPI_GetVFXAttachComponent null (C020's portals and their children).
const NO_ATTACH: &str = "PortalParent_C";

/// A host's kind: a projectile, an item, a World actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Projectile,
    Item,
    Actor,
}

/// BPI_GetElementalVFXOverride's answer.
#[derive(Debug)]
struct Override {
    ps: String,
    socket: Option<String>,
    loc: V3,
    rot: Rows,
    scale: V3,
}

/// One buff's emitter and where it goes under its host.
#[derive(Debug)]
struct Live {
    h: Option<SystemId>,
    cap_loc: V3,
    cap_scale: V3,
    absolute: bool,
    rel_loc: V3,
    rel_rot: Rows,
    rel_scale: V3,
}

/// What its effects keep between steps: per (host kind, sequence number, class) the buff's emitter.
#[derive(Debug, Default)]
pub(crate) struct Elemental {
    live: IndexMap<(Kind, i32, BuffClass), Live>,
    warned: BTreeSet<String>,
}

/// A host's place, root rotation and scale this tick.
struct Root {
    x: f64,
    y: f64,
    rows: Rows,
    scale: f64,
}

/// A host's reference, sequence number, class, owner and root, `None` for one dead or without elemental buffs.
struct HostOf {
    a: ActorRef,
    seq: i32,
    cls: mvs_sim::names::Name,
    /// The fighter whose class data the host's is (an item: its owner, else the fighter that looked it up).
    owner: Option<FighterId>,
    /// The fighter whose colour is the Team (an item: its owner only).
    team: Option<FighterId>,
    root: Root,
}

/// The class's name in the data (its short form against a soft class path's tail).
fn key_is(key: &str, cls: &str) -> bool {
    key == cls || key.ends_with(&format!(".{cls}"))
}

/// BPI_GetElementalVFXOverride(cls) of the host's class defaults: an entry with a valid ParticleSystem, or None.
fn override_of(p: Option<&Props>, cls: &str) -> Option<Override> {
    let p = p?;
    // Tom_and_Jerry_TennisBall's own: its IceBuff class (a soft class path) answers with IceVFXOverrides
    let ice = p.get("IceBuff").and_then(Value::as_str);
    let v = match (p.get("IceVFXOverrides").and_then(Value::as_obj), ice) {
        (Some(o), Some(ice)) if key_is(ice, cls) => Some(o),
        _ => p.get("ElementalVFXOverrides").and_then(Value::as_arr).and_then(|m| {
            m.iter()
                .filter_map(Value::as_obj)
                .find(|e| e.get("Key").and_then(Value::as_str).is_some_and(|k| key_is(k, cls)))
                .and_then(|e| e.get("Value").and_then(Value::as_obj))
        }),
    }?;
    let ps = v.get("ParticleSystem").and_then(Value::as_str).filter(|s| !s.is_empty())?;
    let socket = v.get("AttachPointName").and_then(Value::as_str).filter(|s| *s != "None").map(str::to_owned);
    Some(Override {
        ps: ps.to_owned(),
        socket,
        loc: xyz(v.get("Location"), 0.0),
        rot: rot_rows(v.get("Rotation")),
        scale: xyz(v.get("Scale"), 1.0),
    })
}

/// The yaw rows of a facing (-90 at facing right).
fn yaw_rows(pitch: f64, facing: f64) -> Rows {
    let yaw = if facing > 0.0 { -std::f64::consts::FRAC_PI_2 } else { std::f64::consts::FRAC_PI_2 };
    rotation_rows(pitch, yaw, 0.0)
}

/// The hosts of this tick, in visiting order (projectiles, items, World actors).
fn hosts(w: &World) -> Vec<HostOf> {
    let has = |e: &Option<mvs_sim::combat::ElemState>| e.as_ref().is_some_and(|e| !e.buffs.is_empty());
    let mut out = Vec::new();
    for (id, pr) in w.actors.projectiles.iter() {
        if pr.dead || !has(&pr.elem) {
            continue;
        }
        let root = Root { x: f64::from(pr.x), y: f64::from(pr.y), rows: actor_rot(w, pr), scale: f64::from(pr.scale) };
        out.push(HostOf {
            a: ActorRef::Projectile(id),
            seq: pr.seq,
            cls: pr.cls,
            owner: pr.owner,
            team: pr.owner,
            root,
        });
    }
    for (id, it) in w.actors.items.iter() {
        if it.dead || !has(&it.elem) {
            continue;
        }
        let root = Root {
            x: f64::from(it.x),
            y: f64::from(it.y),
            rows: yaw_rows(f64::from(it.pitch).to_radians(), f64::from(it.facing)),
            scale: f64::from(it.scale),
        };
        let owner = it.owner.or(it.lookup_fighter);
        out.push(HostOf { a: ActorRef::Item(id), seq: it.seq, cls: it.cls, owner, team: it.owner, root });
    }
    for (id, a) in w.actors.actors.iter() {
        if a.dead || !has(&a.elem) {
            continue;
        }
        // (a class without a place of its own at the origin)
        let (x, y) = world_actor_place_of(w, id).map_or((0.0, 0.0), |(x, y, _)| (x, y));
        // Unverified: a World actor class with a
        // facing of its own is drawn at facing 1 here, and its root scale (the sim's size_scale is not that
        // scale) at 1.
        let root = Root { x: f64::from(x), y: f64::from(y), rows: yaw_rows(0.0, 1.0), scale: 1.0 };
        out.push(HostOf {
            a: ActorRef::Actor(id),
            seq: a.seq,
            cls: a.cls,
            owner: Some(a.owner),
            team: Some(a.owner),
            root,
        });
    }
    out
}

fn kind_of(a: ActorRef) -> Kind {
    match a {
        ActorRef::Projectile(_) => Kind::Projectile,
        ActorRef::Item(_) => Kind::Item,
        _ => Kind::Actor,
    }
}

impl Live {
    /// The emitter's place under its host's root: the Capsule's place, then the relative place through the Capsule's
    /// rotation and scale.
    fn at(&self, r: &Root) -> Attachment {
        let rows = &r.rows;
        let rs = r.scale;
        let cs: V3 = if self.absolute { self.cap_scale } else { self.cap_scale.map(|v| v * rs) };
        let mut c = [0.0, -r.x, r.y];
        for (k, ck) in c.iter_mut().enumerate() {
            *ck += rs * (self.cap_loc[0] * rows[k] + self.cap_loc[1] * rows[3 + k] + self.cap_loc[2] * rows[6 + k]);
        }
        let l = self.rel_loc;
        let loc: V3 = std::array::from_fn(|k| {
            c[k] + cs[0] * l[0] * rows[k] + cs[1] * l[1] * rows[3 + k] + cs[2] * l[2] * rows[6 + k]
        });
        let rot = compose_rows(&self.rel_rot, rows);
        Attachment {
            loc,
            rot: std::array::from_fn(|i| std::array::from_fn(|j| rot[i * 3 + j])),
            scale: Some(std::array::from_fn(|k| cs[k] * self.rel_scale[k])),
        }
    }
}

impl Elemental {
    fn warn(&mut self, s: String) {
        if self.warned.insert(s.clone()) {
            eprintln!("warning: elemental: {s}");
        }
    }

    /// teamOf: the colour index, 0.6 where the pawn's Team is the local-online 0.6.
    fn team_of(fx: &Fx, w: &World, f: FighterId) -> f64 {
        let id = w.fighter(f).id;
        let t = fx.team(id);
        if t == 0.6 { 0.6 } else { f64::from(fx.color_index(id)) }
    }

    fn make(&mut self, fx: &mut Fx, w: &World, h: &HostOf, cls: BuffClass, ps0: &str, scale0: f64) -> Live {
        let mut o = Live {
            h: None,
            cap_loc: [0.0; 3],
            cap_scale: [1.0; 3],
            absolute: true,
            rel_loc: [0.0; 3],
            rel_rot: IDENTITY,
            rel_scale: [scale0; 3],
        };
        let name = w.names.str(h.cls);
        if name == NO_ATTACH || !elem_shows(w, h.a, cls) {
            return o;
        }
        let def = h.owner.and_then(|f| actor_def(w, ActorRef::Fighter(f), h.cls));
        let cap = def.and_then(|d| d.components.get("Capsule")).and_then(Value::as_obj);
        let cname = cls.name(&w.names).to_owned();
        let ov = override_of(def.map(|d| &d.p), &cname);
        o.cap_loc = xyz(cap.and_then(|c| c.get("RelativeLocation")), 0.0);
        o.cap_scale = xyz(cap.and_then(|c| c.get("RelativeScale3D")), 1.0);
        o.absolute = cap.and_then(|c| c.get("bAbsoluteScale")).and_then(Value::as_bool) != Some(false);
        if let Some(ov) = &ov {
            o.rel_loc = ov.loc;
            o.rel_rot = ov.rot;
            o.rel_scale = ov.scale;
            if let Some(s) = &ov.socket {
                self.warn(format!("an override on socket {s} of {name} is drawn at the Capsule"));
            }
        }
        let ps = ov.as_ref().map_or(ps0, |ov| ov.ps.as_str()).to_owned();
        if fx.player.emitter_count(&ps).is_some() {
            let params: Vec<(&str, f64)> = match h.team {
                Some(f) => vec![("Team", Self::team_of(fx, w, f))],
                None => Vec::new(),
            };
            o.h = fx.player.spawn_attached(&ps, &o.at(&h.root), 1.0, &params);
        } else {
            self.warn(format!("{ps} is not loaded"));
        }
        if h.team.is_some() {
            fx.sounds.push((ignite_sound(cls).to_owned(), [0.0, -h.root.x, h.root.y]));
        }
        o
    }

    fn clear(&mut self, fx: &mut Fx) {
        for (_, l) in self.live.drain(..) {
            if let Some(h) = l.h {
                fx.player.kill(h);
            }
        }
    }
}

impl CharFxHooks for Elemental {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = std::collections::HashSet::new();
        for h in hosts(w) {
            let Some(bs) = mvs_sim::actors::world_actor::actor_elem(w, h.a).and_then(|e| e.as_ref()) else {
                continue;
            };
            for (cls, _) in CLASSES {
                if !bs.buffs.iter().any(|b| b.class == cls) {
                    continue;
                }
                let Some((ps, scale)) = visuals(cls) else { continue };
                let key = (kind_of(h.a), h.seq, cls);
                seen.insert(key);
                if !self.live.contains_key(&key) {
                    let l = self.make(fx, w, &h, cls, ps, scale);
                    self.live.insert(key, l);
                    continue;
                }
                // (the closure's place, each tick before the player's)
                let l = &self.live[&key];
                if let Some(id) = l.h {
                    fx.player.attach(id, &l.at(&h.root));
                }
            }
        }
        let gone: Vec<_> = self.live.keys().copied().filter(|k| !seen.contains(k)).collect();
        for k in gone {
            if let Some(Live { h: Some(h), .. }) = self.live.shift_remove(&k) {
                fx.player.kill(h);
            }
        }
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electric_draws_nothing_fire_and_ice_their_defaults() {
        assert_eq!(visuals(BuffClass::BuffElementalElectric), None);
        assert_eq!(visuals(BuffClass::BuffElementalFire), Some(("PS_Creature_FireWallProjectileBuff", 2.0)));
        assert_eq!(visuals(BuffClass::BuffElementalIce), Some(("PS_SupermanIceBreath_IceCube", 0.4)));
        assert_eq!(visuals(BuffClass::BuffElementalFireproof), None);
    }
}
