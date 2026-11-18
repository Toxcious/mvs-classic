//! The effects on a fighter's second views (a Disguise's copy, a Polymorph mesh: mvs_view's second views, the actor
//! poses with a SecondRef): their bodies each step, and the particle and PlayVFX notifies of their montages, which
//! spawn on that mesh (the rules are Fx::particle_notify's and
//! Fx::notify_vfx's for the fighters' own mesh).
//!
//! Unverified: a second view hidden by its rules (not presented) keeps the body of its last present; an
//! emitter that begins while it is hidden follows its last shown pose.
//! Unverified: the particle notifies' emitters are not AddVFX'ed; whether the second mesh's SetVisibility reaches its
//! attached children was not read: they stay drawn.

use mvs_data::fighter::Skeleton;
use mvs_sim::World;
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::anim::montages::notify_of;
use mvs_sim::fighter::anim::visibility::invisibility_count;
use mvs_vfx::math::{Rows, V3};
use mvs_vfx::{Attachment, SystemId};
use mvs_view::mesh::MeshInfo;
use mvs_view::{ActorPose, SecondKind};

use super::attach::{
    Body, Rig, SocketFollow, att, flag, mirror_play_vfx, play_vfx_particle, rot_rows, swap_lr, text, xyz,
};
use super::{Follow, Fx};
use crate::Result;

/// One second view's effects state, by the fighter it is on and its kind.
#[derive(Debug, Default)]
pub(super) struct SecondFx {
    body: Option<Body>,
    /// Its first mesh's path (its rig's key).
    mesh: String,
    /// The view's take (SecondRef::take) the emitters belong to.
    take: u64,
    /// The character whose skeleton data its first mesh has (None: a kit's prop mesh, no sockets).
    skel: Option<std::sync::Arc<str>>,
    /// The particle notifies' components attached to it, in attach order: system, socket, handle.
    psc: Vec<(String, String, SystemId)>,
    /// The copy's PlayVFX emitters, by notify (the Disguise's DisguiseVFXs).
    notify: Vec<(NotifyRef, Option<SystemId>)>,
}

/// A prop mesh's skeleton data: no sockets, no mirror table.
fn no_skeleton() -> Skeleton {
    Skeleton {
        sockets: Default::default(),
        virtual_bones: None,
        mirror_axis: None,
        mirror: Default::default(),
        flip: None,
        mirror_node: None,
    }
}

/// The buff that holds the view is on fighter `id`.
fn held(w: &World, id: i32, kind: SecondKind) -> bool {
    w.by_id(id).is_some_and(|f| {
        let s = &w.fighter(f).shapes;
        match kind {
            SecondKind::Disguise => s.disguise.is_some(),
            SecondKind::Polymorph => s.polymorph.is_some(),
        }
    })
}

impl Fx {
    /// The second views' bodies from this tick's actor poses (kept from the last pose for a view not presented now);
    /// a view whose buff ended, or a new take of one (another view, a victim switch to an identical fighter
    /// included), drops its emitters (the modules' `drop`).
    pub(super) fn second_bodies(&mut self, w: &World, actors: &[ActorPose]) -> Result<()> {
        for a in actors {
            let Some(sr) = &a.second else { continue };
            let Some(mesh) = a.meshes.first() else { continue };
            let path = mesh.mesh.to_string();
            if !self.rigs.contains_key(&path) {
                let info = MeshInfo::read(&self.meshes.bytes(&path)?, &path)?;
                let rig = info.skeleton.is_some().then(|| match &sr.skeleton {
                    Some(k) => Rig::new(&info, &w.data.chars[&**k].fighter.skeleton)
                        .with_kept(mvs_view::view::kept_bones(k, &info)),
                    None => Rig::new(&info, &no_skeleton()),
                });
                self.rigs.insert(path.clone(), rig);
            }
            let Some(Some(rig)) = self.rigs.get(&path) else { continue };
            let body = Body::of(mesh, rig, sr.left);
            let e = self.seconds.entry((sr.fighter, sr.kind)).or_default();
            if !e.mesh.is_empty() && (e.mesh != path || e.take != sr.take) {
                let old = std::mem::take(e);
                kill_all(&mut self.player, old);
            }
            let e = self.seconds.entry((sr.fighter, sr.kind)).or_default();
            e.body = Some(body);
            e.mesh = path;
            e.take = sr.take;
            e.skel.clone_from(&sr.skeleton);
        }
        let ended: Vec<(i32, SecondKind)> =
            self.seconds.keys().filter(|&&(id, kind)| !held(w, id, kind)).copied().collect();
        for k in ended {
            if let Some(e) = self.seconds.shift_remove(&k) {
                kill_all(&mut self.player, e);
            }
        }
        // OnVFXAdded / OnFighterVisibilityUpdated: the copy's PlayVFX emitters are visible while the owner's count is 1
        for (&(id, kind), e) in &mut self.seconds {
            if kind != SecondKind::Disguise || e.notify.is_empty() {
                continue;
            }
            let on = w.by_id(id).is_some_and(|f| invisibility_count(w, f) == 1.0);
            let player = &mut self.player;
            e.notify.retain(|(_, h)| h.is_none_or(|h| player.alive(h)));
            for h in e.notify.iter().filter_map(|(_, h)| *h) {
                player.set_visible(h, on);
            }
        }
        Ok(())
    }

    /// Follow::Second's place: on the socket, else on the mesh component (`rel`, `rows`, `scale` times its world
    /// scale).
    pub(super) fn place_second(
        &self,
        id: i32,
        kind: SecondKind,
        s: Option<&SocketFollow>,
        rel: V3,
        rows: &Rows,
        scale: V3,
    ) -> Option<Attachment> {
        let e = self.seconds.get(&(id, kind))?;
        let body = e.body.as_ref()?;
        let Some(s) = s else {
            let (loc, rot) = body.mesh_attach(rel, Some(rows));
            return Some(att(loc, &rot, Some(scale.map(|v| v * body.scale))));
        };
        let rig = self.rigs.get(&e.mesh)?.as_ref()?;
        match s.eval(body, rig) {
            Some((loc, rot, sc)) => Some(att(loc, &rot, Some(sc))),
            // (the bone is not in the pose: the mesh component's origin)
            None => {
                let (loc, rot) = body.mesh_attach([0.0; 3], None);
                Some(att(loc, &rot, None))
            }
        }
    }

    /// `socketFollow` on the second view: a socket or bone (in any case, the view's character's sockets), or its mesh
    /// component for None.
    #[allow(clippy::too_many_arguments)]
    fn second_follow(
        &self,
        w: &World,
        id: i32,
        kind: SecondKind,
        socket: &str,
        loc: V3,
        rot: &Rows,
        scale: V3,
    ) -> Follow {
        let e = self.seconds.get(&(id, kind));
        if socket.is_empty() || socket == "None" {
            return Follow::Second { id, kind, s: None, rel: loc, rows: *rot, scale };
        }
        let rig = e.and_then(|e| self.rigs.get(&e.mesh)?.as_ref());
        let name = rig.map_or_else(|| socket.to_string(), |r| r.name(socket));
        let none = no_skeleton();
        let skel = match e.and_then(|e| e.skel.as_deref()) {
            Some(k) => &w.data.chars[k].fighter.skeleton,
            None => &none,
        };
        let s = SocketFollow::new(skel, &name, loc, rot, scale);
        Follow::Second { id, kind, s: Some(Box::new(s)), rel: [0.0; 3], rows: *rot, scale }
    }

    /// The engine's and the game's particle notifies of the second view's montages (copyParticle, polyParticle):
    /// Fx::particle_notify's rules on that mesh and its character's sockets.
    pub(crate) fn second_particle(&mut self, w: &World, kind: SecondKind, id: i32, n: NotifyRef, on: bool, left: bool) {
        let what = match kind {
            SecondKind::Disguise => "disguise VFX: no second mesh",
            SecondKind::Polymorph => "Polymorph VFX: no mesh",
        };
        if !self.seconds.get(&(id, kind)).is_some_and(|e| e.body.is_some()) {
            if on {
                warn_once(format!("{what} on fighter {id}"));
            }
            return;
        }
        let Some(fid) = w.by_id(id) else { return };
        let Some(note) = notify_of(w, fid, n) else { return };
        let p = &note.p;
        let ps = text(p, "PSTemplate").map(str::to_string);
        let game = note.cls == "AnimNotify_PlaySingleShotVFX" || note.cls == "AnimNotifyState_PlayTimedVFX";
        let timed = note.cls == "AnimNotifyState_TimedParticleEffect" || note.cls == "AnimNotifyState_PlayTimedVFX";
        let name = text(p, "SocketName").unwrap_or("None").to_string();
        if !on {
            let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
            let swapped = swap_lr(&name);
            let player = &self.player;
            let Some(e) = self.seconds.get_mut(&(id, kind)) else { return };
            e.psc.retain(|(_, _, h)| player.alive(*h));
            let c = e.psc.iter().find(|(s, k, h)| {
                (same(k, &name) || (game && same(k, &swapped)))
                    && Some(s.as_str()) == ps.as_deref()
                    && !player.deactivated(*h)
            });
            if let Some(&(_, _, h)) = c {
                if flag(p, "bDestroyAtEnd") {
                    self.player.kill(h);
                } else {
                    self.player.deactivate(h);
                }
            }
            return;
        }
        let Some(ps) = ps else { return };
        if !timed && self.player.immortal(&ps) {
            return;
        }
        if timed {
            let e = &self.seconds[&(id, kind)];
            let rig = self.rigs.get(&e.mesh).and_then(Option::as_ref);
            let own = rig.map_or_else(|| name.clone(), |r| r.name(&name));
            let socket = e.skel.as_deref().is_some_and(|k| w.data.chars[k].fighter.skeleton.sockets.contains_key(&own));
            if !socket && !rig.is_some_and(|r| r.has_bone(&own)) {
                return;
            }
        }
        let socket = if game && left { swap_lr(&name) } else { name };
        let scale = if timed { [1.0; 3] } else { xyz(p.get("Scale"), 1.0) };
        let (loc, rot) = (xyz(p.get("LocationOffset"), 0.0), rot_rows(p.get("RotationOffset")));
        if timed || p.get("Attached").and_then(|v| v.as_bool()).unwrap_or(true) {
            let f = self.second_follow(w, id, kind, &socket, loc, &rot, scale);
            if let Some(h) = self.spawn_follow(w, &ps, f, 1.0, &[])
                && let Some(e) = self.seconds.get_mut(&(id, kind))
            {
                e.psc.push((ps, socket, h));
            }
            return;
        }
        let f = self.second_follow(w, id, kind, &socket, loc, &rot, [1.0; 3]);
        let Some(a) = self.place(w, &f) else { return };
        let s = a.scale.unwrap_or([1.0; 3]);
        let fixed = Attachment { scale: Some([0, 1, 2].map(|k| sign(s[k]) * scale[k])), ..a };
        self.player.spawn_attached(&ps, &fixed, 1.0, &[]);
    }

    /// AnimNotify_PlayVFX of the Disguise's copy (copyVfx): Fx::notify_vfx's rules (the template by the owner's skin
    /// and colour index, Mirror, the Team select), attached to the second mesh; NotifyEnd kills it.
    pub(crate) fn copy_vfx(&mut self, w: &World, id: i32, n: NotifyRef, on: bool, left: bool) {
        let key = (id, SecondKind::Disguise);
        if !self.seconds.get(&key).is_some_and(|e| e.body.is_some()) {
            if on {
                warn_once(format!("disguise VFX: no second mesh on fighter {id}"));
            }
            return;
        }
        if !on {
            if let Some(e) = self.seconds.get_mut(&key)
                && let Some(k) = e.notify.iter().position(|(m, _)| *m == n)
                && let (_, Some(h)) = e.notify.remove(k)
            {
                self.player.kill(h);
            }
            return;
        }
        let Some(fid) = w.by_id(id) else { return };
        let Some(note) = notify_of(w, fid, n) else { return };
        let p = &note.p;
        // (the notify's owner is the buff's holder: the skin looked up in the victim's VFX_Skin map is the holder's)
        let ci = self.color_index(id);
        let skin = self.pawns.get(&id).map_or("", |p| p.skin.as_str());
        let Some(ps) = play_vfx_particle(p, skin, ci) else { return };
        let (socket, loc, rot, scale) = mirror_play_vfx(p, left);
        // Deviation: socket None goes to BPI_VFXScaling, not ported.
        let Some(socket) = socket.filter(|s| s != "None" && !s.is_empty()) else {
            return warn_once(format!("notify VFX without a socket: {ps}"));
        };
        let team = if flag(p, "UseTeamColorParam?") {
            let local = p.get("UseLocalOnlineColoring").and_then(|v| v.as_bool()).unwrap_or(true);
            Some(if local { self.team(id) } else { f64::from(ci) })
        } else {
            None
        };
        let params: Vec<(&str, f64)> = team.map(|t| ("Team", t)).into_iter().collect();
        let f = self.second_follow(w, id, SecondKind::Disguise, &socket, loc, &rot, scale);
        let h = self.spawn_follow(w, &ps, f, 1.0, &params);
        if let Some(h) = h
            && invisibility_count(w, fid) != 1.0
        {
            self.player.set_visible(h, false);
        }
        if let Some(e) = self.seconds.get_mut(&key) {
            e.notify.retain(|(k, _)| *k != n);
            e.notify.push((n, h));
        }
    }
}

/// The view's emitters end with it (the modules' `drop`).
fn kill_all(player: &mut mvs_vfx::player::Player, e: SecondFx) {
    for (_, _, h) in e.psc {
        player.kill(h);
    }
    for h in e.notify.into_iter().filter_map(|(_, h)| h) {
        player.kill(h);
    }
}

fn sign(v: f64) -> f64 {
    if v < 0.0 {
        -1.0
    } else if v > 0.0 {
        1.0
    } else {
        0.0
    }
}

/// `warnOnce`.
fn warn_once(what: String) {
    static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !seen.contains(&what) {
        eprintln!("warning: {what}");
        seen.push(what);
    }
}
