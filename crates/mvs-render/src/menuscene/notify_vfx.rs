//! The montage notifies a BP_CharacterPreview runs on its fighter's view, shared by the pre-match and main menu
//! previews and the podium's fighters: AnimNotify_PlayVFX's components (AnimNotify_PlayVFX SpawnVFX on a preview
//! owner, the preview's AddVFX / VFX Scaling / RemoveVFX) and Comp_FighterVisibilityManager's submesh visibility.

use mvs_data::Props;
use mvs_data::fighter::Skeleton;
use mvs_vfx::{Player, SystemId};
use mvs_view::{M4, PreviewView};

use crate::fx::attach::{Body, Rig, SocketFollow, att, flag, mirror_play_vfx, play_vfx_particle, swap_lr, text};
use crate::fx::warn_once;

/// Comp_FighterVisibilityManager SetSubMeshVisibilityMulticast(tag, on): the view's parts of the tag.
pub fn set_tag_visible(view: &mut PreviewView, tag: &str, on: bool) {
    for k in view.parts_of(tag) {
        view.set_part_visible(k, on);
    }
}

/// A PlayVFX component on the view.
#[derive(Clone, Debug)]
struct Live {
    /// The notify (its index in the montage's list) that spawned it.
    notify: usize,
    id: SystemId,
    /// The socket it is attached to (None: spawned at a location, not attached).
    follow: Option<SocketFollow>,
    /// It is the notify's entry in VFXInstances (the newest of the notify's).
    keyed: bool,
}

/// A preview actor's PlayVFX components (VFXInstances and the components the map no longer holds), with the rig and
/// the buffers that place them each tick. Made with the fighter's view, let go with it.
pub struct NotifyVfx {
    rig: Rig,
    /// The character's skeleton, with no mirror node (the view's mirror is the mesh's, `SocketFollow`'s).
    skel: Skeleton,
    live: Vec<Live>,
    bones: Vec<[f32; 12]>,
    body: Body,
}

impl std::fmt::Debug for NotifyVfx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotifyVfx").field("live", &self.live).finish_non_exhaustive()
    }
}

impl NotifyVfx {
    pub fn new(view: &PreviewView) -> NotifyVfx {
        let (info, skel) = view.body();
        let rig = Rig::new(info, skel);
        let mut skel = skel.clone();
        skel.mirror_node = None;
        NotifyVfx { rig, skel, live: Vec::new(), bones: Vec::new(), body: Body::default() }
    }

    /// The components playing (spawned and not let go).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// The body's bones on `world` from the view's last pose.
    fn place_body(&mut self, view: &mut PreviewView, world: &M4, left: bool) {
        let to_world = view.body_bones(world, &mut self.bones);
        self.body.set(&to_world, &self.bones, &self.rig, left);
    }

    /// NotifyBegin: AnimNotify_PlayVFX SpawnVFX on a BP_CharacterPreview owner (@709-3108). The particle is
    /// UseSkinVFX's skin entry (Map_Find(VFX_Skin, AppliedSkin) @401-690, the map's first without one) or
    /// ParticleTeam0 (@2669: the preview path has no colour select). Mirror(Preview.FacingLeft) (@2560) gives the
    /// offsets; the socket's L_ / R_ are swapped whenever it mirrors (@2688-2872: the preview path does not read
    /// BlockAttachSocketMirror). With a socket, the preview's AddVFX (@36639): SpawnEmitterAttached(the particle, the
    /// mesh, the socket, ...), Map_Add(VFXInstances, the notify) @36765 (a notify's earlier component leaves the map
    /// and plays on), and with UseTeamColorParam? Team = SelectFloat(0.6, GetColorIndex, IsLocalOnlinePlayer and
    /// UseLocalOnlineColoring) @36869-37059. Without one (@2276 -> @3932), the preview's VFX Scaling (@37123-37522):
    /// SpawnEmitterAtLocation at the mesh's location plus the offset in its rotation, scaled by the mesh, not
    /// attached and not in the map.
    /// `color_index` is the actor's Comp_Pawn_SkinManager GetColorIndex; IsLocalOnlinePlayer is false on a preview
    /// (GetColorIndex @3068 or ForcedColorIndex @145), so Team is the index.
    // Unverified: UseOverrideAttachParent's tagged mesh (GetComponentsByTag @751) is taken as the body mesh: the
    // tagged meshes are skinned to the one skeleton by bone name, so the socket is the same bone's.
    // Deviation: a socket that names no socket or bone of the body, or a socket on a bone the body lacks, plays
    // nothing (warned once); the rig has no kept-bones list (a bone a view keeps unmirrored facing left follows its
    // mirror partner).
    // Deviation: Comp_FighterVisibilityManager.AddVFX (@36824) is not run: the visibility manager's hide does not
    // reach the components.
    #[allow(clippy::too_many_arguments)]
    pub fn begin(
        &mut self,
        view: &mut PreviewView,
        world: &M4,
        notify: usize,
        p: &Props,
        left: bool,
        color_index: u8,
        vfx: &mut Player,
    ) {
        let Some(ps) = play_vfx_particle(p, &view.skin, 0).filter(|s| !s.is_empty()) else { return };
        let ps = system_name(&ps);
        let (_, loc, rot, scale) = mirror_play_vfx(p, left);
        let mirror = left && flag(p, "ApplyMirroring");
        let socket = text(p, "SocketName")
            .filter(|s| !s.is_empty() && *s != "None")
            .map(|s| if mirror { swap_lr(s) } else { s.to_owned() });
        let Some(socket) = socket else {
            self.place_body(view, world, left);
            let (at, rows) = self.body.mesh_attach(loc, Some(&rot));
            let s = scale.map(|s| s * self.body.scale);
            if let Some(id) = vfx.spawn_attached(ps, &att(at, &rows, Some(s)), 1.0, &[]) {
                self.live.push(Live { notify, id, follow: None, keyed: false });
            } else {
                warn_once(format!("preview: notify VFX {ps}: no such system"));
            }
            return;
        };
        let socket = self.rig.name(&socket);
        let bone = self.skel.sockets.get(&socket).map_or(socket.as_str(), |s| s.bone.as_str());
        let virtual_bone = self.skel.virtual_bones.as_ref().is_some_and(|v| v.contains_key(bone));
        if !self.rig.has_bone(bone) && !virtual_bone {
            warn_once(format!("preview {}: notify VFX {ps}: socket {socket} on no bone of the body", view.key));
            return;
        }
        let follow = SocketFollow::new(&self.skel, &socket, loc, &rot, scale);
        let team = flag(p, "UseTeamColorParam?");
        let params: &[(&str, f64)] = if team { &[("Team", f64::from(color_index))] } else { &[] };
        let Some(id) = vfx.spawn_attached(ps, &att([0.0; 3], &mvs_vfx::math::IDENTITY, Some([1.0; 3])), 1.0, params)
        else {
            warn_once(format!("preview: notify VFX {ps}: no such system"));
            return;
        };
        for l in self.live.iter_mut().filter(|l| l.notify == notify) {
            l.keyed = false;
        }
        self.live.push(Live { notify, id, follow: Some(follow), keyed: true });
    }

    /// NotifyEnd with KillOnNotifyEnd: the preview's RemoveVFX (@36138-36545): the notify's VFXInstances entry
    /// destroyed and removed (its earlier components play on).
    pub fn end(&mut self, notify: usize, vfx: &mut Player) {
        if let Some(k) = self.live.iter().position(|l| l.notify == notify && l.keyed) {
            vfx.kill(self.live.remove(k).id);
        }
    }

    /// The attached components follow their sockets on the view's pose of the tick; the ones that ended
    /// (SpawnEmitterAttached's auto destroy) are let go.
    pub fn follow(&mut self, view: &mut PreviewView, world: &M4, left: bool, vfx: &mut Player) {
        self.live.retain(|l| vfx.alive(l.id));
        if !self.live.iter().any(|l| l.follow.is_some()) {
            return;
        }
        self.place_body(view, world, left);
        for l in &self.live {
            if let Some(f) = &l.follow
                && let Some((loc, rot, scale)) = f.eval(&self.body, &self.rig)
            {
                vfx.attach(l.id, &att(loc, &rot, Some(scale)));
            }
        }
    }

    /// Every component let go (the mesh destroyed: the components attached to it go with it).
    pub fn drain(&mut self) -> impl Iterator<Item = SystemId> + '_ {
        self.live.drain(..).map(|l| l.id)
    }
}

/// A particle reference's system name (a package below Content/, or an object path): its last segment, before a '.'.
pub fn system_name(r: &str) -> &str {
    let last = r.rsplit('/').next().unwrap_or(r);
    last.split('.').next().unwrap_or(last)
}

#[cfg(test)]
mod tests {
    use mvs_data::Value;

    use super::*;
    use crate::menuscene::preview::mesh_world;

    const HAND_ELEC: &str = "Panda_Main/Characters/C021/VFX/PS_C021_Emote_HandElec";

    fn props(socket: &str) -> Props {
        let mut p = Props::new();
        p.insert("ParticleTeam0".into(), Value::Str(HAND_ELEC.into()));
        p.insert("SocketName".into(), Value::Str(socket.into()));
        p
    }

    /// A socket on no bone of the body plays nothing; a notify begun again keeps its first component playing and
    /// followed, its end takes the newest alone, and the mesh's end lets the rest go; with no socket the component
    /// plays unattached.
    #[test]
    fn sockets_and_a_second_begin() {
        let Ok(store) = mvs_store::Store::open(mvs_store::dev_store_dir()) else { return };
        let Ok(mut view) = PreviewView::new(&store, "blackadam", "") else { return };
        let mut player = Player::new(&store, 7).unwrap();
        let world = mesh_world([0.0; 3], [1.0; 3], false);
        let mut fx = NotifyVfx::new(&view);
        fx.begin(&mut view, &world, 0, &props("NoSuchSocket"), false, 0, &mut player);
        assert!(fx.is_empty(), "a name of no socket or bone");
        let bone = fx.skel.sockets.keys().find(|k| !k.eq_ignore_ascii_case("R_Hand")).cloned().expect("a socket");
        fx.skel.sockets.get_mut(&bone).unwrap().bone = "NoSuchBone".into();
        fx.begin(&mut view, &world, 0, &props(&bone), false, 0, &mut player);
        assert!(fx.is_empty(), "socket {bone} on a bone the body lacks");
        fx.begin(&mut view, &world, 1, &props("R_Hand"), false, 0, &mut player);
        fx.begin(&mut view, &world, 1, &props("R_Hand"), true, 0, &mut player);
        assert_eq!(fx.len(), 2, "{fx:?}");
        let first = fx.live[0].id;
        fx.follow(&mut view, &world, false, &mut player);
        fx.end(1, &mut player);
        assert_eq!(fx.len(), 1);
        assert_eq!(fx.live[0].id, first, "the first plays on");
        assert!(player.alive(first));
        fx.end(1, &mut player);
        assert_eq!(fx.len(), 1, "no entry left in the map for the notify");
        fx.begin(&mut view, &world, 2, &props("None"), false, 0, &mut player);
        assert_eq!(fx.len(), 2);
        assert!(fx.live[1].follow.is_none(), "unattached");
        let ids: Vec<SystemId> = fx.drain().collect();
        assert_eq!(ids.len(), 2);
        assert!(fx.is_empty());
        assert_eq!(system_name(HAND_ELEC), "PS_C021_Emote_HandElec");
    }
}
