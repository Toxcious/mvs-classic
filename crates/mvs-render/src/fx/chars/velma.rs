//! Velma's effects: the CharFx hooks, the World actors and
//! the projectile rules. Her meshes (the pose hook, the pawn props, the evidence's clue, the book) are
//! mvs-view's chars/velma.rs; the police minion's are velma_police.rs.
//!
//! The beams: VelmaHealBeam_C / VelmaChildBeam_C (mvs-sim chars/velma/beam.rs):
//!   ToggleVisuals(true) @3412 (not on a dedicated server): SpawnVFX(GetOwner's mesh) @3782 (VelmaHealBeam's own event
//!   @9870 does nothing; VelmaChildBeam's @382 SpawnEmitterAttached(PS_Velma_MegaphoneAlly, the mesh, VB Spine, auto
//!   destroy) with Team = SelectFloat(0.6, the Instigator's colour index, IsLocalOnline) @682, a component nothing
//!   keeps or destroys), then BeamParticle = SpawnEmitterAttached(BeamParticleEmitter: PS_Velma_Beam / PS_Velma_Beam03,
//!   the mesh, BeamAttachSocket: VB HoldPoint / VB Spine) @3805, its Team = SelectFloat(0.6, GetOwner's colour index,
//!   IsLocalOnline) @556 and Color = MakeColor(1, 1, that Team, 1) @879 (SetColorParameter: color_param).
//!   SetVFXPositioning (the 0.02 s timer, sim): SetBeamEndPoint(0, LocalCurrentEnd) @652, (1, LocalCurrentEnd) @702,
//!   (2, the grown point) @1058: the Beam's end_x / end_y and reach_x / reach_y, written into the beam emitters each
//!   tick. ToggleVisuals(false) @3937 -> @160 and ReceiveEndPlay @9105: BeamParticle.K2_DestroyComponent.
//!   Unverified: SetBeamEndPoint is taken as SetBeamTargetPoint(emitter, point, 0) (UE 4.26's body; not read in the
//!   exe).
//!   Deviation: VB HoldPoint and VB Spine are virtual bones the view does not hold: the beam's component is placed at
//!   the sim's socket point (the Beam's x, y: the boxes data, X 0) with the holder's actor yaw for its rotation, and
//!   the ally beam's PS_Velma_MegaphoneAlly on the ally's mesh component (its socket is not sampled).
//!   Deviation: not drawn: the scrolling words (ScrollingWordsWidget, WBP_PannerText: SetPivot @6092, SetWidth @5904).
//!
//! The evidence: VelmaEvidence_C / VelmaEvidenceNoGrav_C: PS_TeamGlow (PS_ShaggyScoobySnack on EvidenceStaticMesh at
//!   Y -0.1, world scale 3 by UserConstructionScript) with Team = SelectFloat(0.6, the owner's colour index,
//!   IsLocalOnline) @6152, under the clue mesh (mvs-view chars/velma.rs ClueMesh: yaw 90, rolled by the sim's spin).
//!   Deviation: not drawn: the pickup's PS_ShaggyScoobySnacksPickup @5833 is the class rule's own emitter below; the
//!   timeout / destroy's PS_ShadowClone_Impact (scale 0.75) @4277 / @6311 the shared spawn of the sim's event.
//!
//! The grenade: SlowtoStunGrenade_C (mvs-sim chars/velma/gadgets.rs): Visuals @1258: FogParticle =
//!   SpawnEmitterAttached(PS_Velma_SlowImpact, Box, (0, 0, -150)) @1359 (the Box at Z -50) with Team = the owner's
//!   BPI_GetColorValue (its colour index) @1609; Delay(MaxLifetime - 0.25) @730 -> @15: SetFloatParameter(Dissolve, 1),
//!   Deactivate. The actor's end takes the component.
//!   Unverified: the Delay's end is taken as the first tick the MaxLifetimeTimer's remaining time is 0.25 s or less.
//!   The Decal: VelmaSlowDecal_MAT (DBM_AlphaComposite), DecalSize (128, 200, 400) at (0, 0, -150) on the Capsule,
//!   pitch 90 (HealOverTimeZoneBP's component, so Steven's zone_decal axes); its dynamic instance @569 with Team = the
//!   owner's BPI_GetColorValue @634; the Delay's end @105 DissolveTimeline.PlayFromStart: 0.25 s, CurveFloat_0_1
//!   (0, 1) cubic with tangents 0 -> (0.25, 0): Dissolve @3994 = 1 - smoothstep (the material's default is 1), read
//!   here at 0.25 - remaining past that tick. Made at the first sight, then carried with the grenade's place each
//!   tick (the Capsule is the root's child: a grenade on a moving platform takes it along; it is made once).
//!   Unverified: the actor's yaw is the owner's facing at the view's first sight (the spawn takes the owner's
//!   rotation, the sim keeps none).
//!   Deviation: the sim's remaining() (gadgets.rs) is private: the same reckoning is made here from the class's
//!   MaxLifetime and the grenade's age.
//! VelmaVialAttack_C: its StaticMesh has no mesh asset and it lives one frame: nothing to draw. VelmaCarryRun_Actor_C:
//!   no visual component (OnHitEffectOverride PS_Velma_HitThrow is the hit's effect, the shared path's).
//!
//! The projectiles (the shared path spawns each ParticleSystem with the owner's Team and ends it with
//! the actor). Velma_SPAIR_CameraFlash_C, Velma_AirSideSp_BookBlast_C and VelmamSassFollowUpProjectile_C: the shared
//! path alone.
//!   VelmaBookProjectile_C: ParticleSystem (PS_VelmaBookHead) and PS_Velma_BookTrail (Z 20 under StaticMesh, Z -67
//!     scale 3), the trail's Team set with the head's @3175: the rule's `more`.
//!   Velma_SassBubblev2_C: ParticleSystem / ParticleSystem1 have no Template and nothing sets one; SpeechBubble =
//!     SpawnEmitterAttached(the calculator text, the Instigator + (80, 0, 150), his rotation) @1134,
//!     PS_Velma_NAD_Calculator_Weak at scale 1 or PS_Velma_NAD_Calculator at (1.5, 1.25, 1.25), destroyed at the
//!     actor's EndPlay @2158: the sim's projectileDeath event, kept here as LeBron's C016_SassBubble.
//!   VelmaMarkProjectile_C: Spawn Attach Point Visuals @4662 destroys the flying ParticleSystem @4730 once the mark is
//!     on a target or on the ground. On the ground and not attached, @1491-1989: the DotDotDot bubble
//!     PS_VelmaSass_HomingTarget at the root + (100, FacingLeftAtThrow ? 15 : -15, 350) under the actor's rotation,
//!     scale 0.9, Team = SelectFloat(0.6, ...); attached to a target, @5049 destroys it. The actor's end takes it.
//!   VelmaAimProjectile_C: ParticleSystem's Text = RandomFloatInRange(1, 5) @4507; BoomMulticast (a DoOnce, from
//!     HitLogic @5407 or ReceiveEndPlay @4668): PS_Velma_SpeechExplosion at the actor's location, scale 0.5, Team
//!     @639 / @917.
//!     Deviation: Text is a random draw at BeginPlay; drawn here from the actor's id (render side, no sim stream).
//!   VelmaEvidence_C / VelmaEvidenceNoGrav_C PickupVisuals @5833: PS_ShaggyScoobySnacksPickup at the actor's
//!     location, scale 1, with no Team written (the shared projectileDeath spawn writes one).
//!
//! The witch skin's sparkles: PS_Velma_WitchSpakles, Pawn_Velma's own particle component, shown by the Witch
//!   skin (C014_S03) alone. The SCS node PS_Velma_WitchSpakles (SCS_Node_44) is on the pawn's mesh (CharacterMesh0) at
//!   fml_un_C_vlma_TopHead, relative transform identity, template PS_Velma_WitchSpakles with bVisible false and the
//!   component's default bAutoActivate. OnSkinApplied (ExecuteUbergraph_Pawn_Velma @7830-7965): SetVisibility(
//!   AppliedSkin.Slug == "skin_c014_s03", PropagateToChildren false) @7835-7923, so only the Witch skin shows it, for the
//!   pawn's life; no graph sets a parameter on it (no Team). Drawn here while the fighter is alive with that skin: a KO
//!   destroys the pawn and the respawned one shows it from its start again; the fighter's hiding hides it, as the other
//!   pawn-attached effects.
//!   Unverified: the system is started when the port first sees the fighter alive with the skin (the build's component
//!   activates with the pawn and is hidden until OnSkinApplied; a looping system's first second differs at most); and
//!   the hiding by mesh_hidden is the port's reading of a mesh child under Comp_FighterVisibilityManager (the component
//!   is not given to AddVFX), not read in the exe.
//!
//! The police ready: Buff_EvidenceCount's Evidence Visuals(true) @275, called @1557 when IncrementEvidenceCount takes
//!   the count to MaxEvidence with AllowPoliceCall (after the Buff_PoliceEvidence spawn, before the Velma check):
//!   SpawnEmitterAttached(PS_ShaggyScoobySnacksPickup, the owner's Mesh, VB_Spine, (0, 0, 200), scale 2,
//!   KeepRelativeOffset, auto destroy) @370 with no parameter, then the same for PS_Velma_CluesCollected @668 with Team
//!   = SelectFloat(0.6, the owner's colour index, IsLocalOnline) @999. Ported from the
//!   decode. The sim keeps no record of the call: it is read off the buff's count and AllowPoliceCall each tick.
//!   Unverified: the call is taken where the count rose to MaxEvidence since the last tick and AllowPoliceCall was set
//!   then (a Delay that ends and a count that reaches the cap within one tick reads as no call). The rotation constant
//!   of both spawns is taken as 0 (the class's bytecode is not read). VB_Spine (an underscore) is
//!   no socket or bone of Velma's skeleton, whose virtual bone is "VB Spine": both are placed on the mesh component
//!   (the attach of a name the mesh lacks), not read in the exe.
//!
//! Not ported: the HUD pieces. The warm-up is the match's (crate::warm).

use std::collections::{BTreeSet, HashMap, HashSet};

use indexmap::IndexMap;

use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::actors::world_actor::{WorldActor, WorldActorState};
use mvs_sim::actors::{ActorId, ActorRef};
use mvs_sim::chars::velma::beam::{Beam, CHILD_BEAM, HEAL_BEAM};
use mvs_sim::chars::velma::evidence::{EVIDENCE, EVIDENCE_COUNT, EVIDENCE_NO_GRAV};
use mvs_sim::chars::velma::gadgets::{BOOK, SASS_BUBBLE, SLOW_TO_STUN, remaining};
use mvs_sim::chars::velma::mark::{AIM_PROJECTILE, MARK};
use mvs_sim::combat::buffs::BuffExt;
use mvs_sim::events::Event;
use mvs_sim::fighter::common::{actor_def, buff_def};
use mvs_sim::fighter::facing;
use mvs_vfx::SystemId;
use mvs_vfx::distribution::color_param;
use mvs_vfx::math::{IDENTITY, Rows, V3, compose_rows, rotation_rows};

use super::steven::zone_decal;
use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{att, rot_rows, text, xyz};
use crate::fx::{FloorDecal, Fx};

const KEY: &str = mvs_sim::chars::velma::KEY;
const ALLY_FX: &str = "PS_Velma_MegaphoneAlly";
const VB_SPINE: &str = "VB Spine";
const SLOW_FX: &str = "PS_Velma_SlowImpact";
pub(crate) const SLOW_DECAL: &str = "VelmaSlowDecal_MAT";
/// The Box at Z -50, the fog at (0, 0, -150) under it.
const FOG_Z: f64 = -50.0 - 150.0;
/// DissolveTimeline's TimelineLength (and the Delay's MaxLifetime - 0.25).
const DISSOLVE_TIME: f64 = 0.25;
/// UserConstructionScript: PS_TeamGlow.SetWorldScale3D((3, 3, 3)).
const GLOW_SCALE: f64 = 3.0;
const TEXTS: [(&str, V3); 2] =
    [("PS_Velma_NAD_Calculator_Weak", [1.0; 3]), ("PS_Velma_NAD_Calculator", [1.5, 1.25, 1.25])];
/// The X of (80, 0, 150); the event's y has the 150.
const TEXT_DEPTH: f64 = 80.0;
const DOTS: &str = "PS_VelmaSass_HomingTarget";
/// Y: 15 facing left at the throw, else -15.
const DOTS_AT: V3 = [100.0, 15.0, 350.0];
const DOTS_SCALE: f64 = 0.9;
const BOOM: &str = "PS_Velma_SpeechExplosion";
const BOOM_SCALE: f64 = 0.5;
const WITCH_FX: &str = "PS_Velma_WitchSpakles";
const WITCH_SOCKET: &str = "fml_un_C_vlma_TopHead";
const WITCH_SLUG: &str = "skin_c014_s03";
const PICKUP_VFX: &str = "PS_ShaggyScoobySnacksPickup";
/// Evidence Visuals' two systems (@370, @668), their relative location and scale.
const POLICE_READY_FX: [&str; 2] = [PICKUP_VFX, "PS_Velma_CluesCollected"];
const POLICE_READY_AT: V3 = [0.0, 0.0, 200.0];
const POLICE_READY_SCALE: f64 = 2.0;
/// Buff_EvidenceCount's MaxEvidence where its data has none (its CDO's 5).
const EVIDENCE_MAX: f64 = 5.0;

/// The yaw rows facing left (`true`) or right.
fn yaw(left: bool) -> Rows {
    let h = std::f64::consts::FRAC_PI_2;
    rotation_rows(0.0, if left { h } else { -h }, 0.0)
}

/// A World actor's view.
#[derive(Debug)]
enum ActorView {
    /// BeamView: the beam's component (None: off).
    Beam { h: Option<SystemId> },
    /// EvidenceView: PS_TeamGlow, the mesh's relative place, rotation and scale, and the glow's relative place.
    Evidence { glow: Option<SystemId>, rel: V3, rel_rot: Rows, scale: V3, glow_rel: V3 },
    /// GrenadeView: the fog, the decal, the grenade's place the decal was last put at, whether the dissolve began.
    Grenade { fog: Option<SystemId>, decal: Option<FloorDecal>, at: (f64, f64), dissolved: bool },
}

/// The mark's dots (the mark's `own.dots`): no entry until decided; an entry with no system once gone.
#[derive(Debug, Default)]
struct Dots {
    h: Option<SystemId>,
    /// FacingLeftAtThrow ? 1 : -1.
    side: f64,
}

/// What Velma's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Velma {
    /// A Velma is in the match (velmaFx: no hook draws otherwise); None until the first look.
    mine: Option<bool>,
    /// Per World actor (its sequence number), in spawn order.
    actors: IndexMap<i32, ActorView>,
    /// The sass bubbles' SpeechBubble systems, by the bubble's sequence number.
    speech: HashMap<i32, SystemId>,
    /// The marks' dots, by the mark's sequence number.
    dots: HashMap<i32, Dots>,
    /// The aim projectiles' owners (the boom's Team), by sequence number; and the ones whose DoOnce has passed.
    aims: HashMap<i32, i32>,
    boomed: HashSet<i32>,
    warned: BTreeSet<String>,
    /// The Witch skin's sparkles, by the fighter's id.
    witch: HashMap<i32, SystemId>,
    /// Per fighter id: its Buff_EvidenceCount's EvidenceCount and AllowPoliceCall at the last tick.
    evidence: HashMap<i32, (i32, bool)>,
}

fn me(c: &mut CharFx) -> &mut Velma {
    match c {
        CharFx::Velma(s) => s,
        _ => unreachable!("a Velma rule entered by another module"),
    }
}

impl CharFxHooks for Velma {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        if self.mine(w) {
            self.place_actors(fx, w);
        }
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        if self.mine(w) {
            self.place_witch(fx, w);
            self.place_evidence(fx, w);
        }
    }

    fn frame(&mut self, fx: &mut Fx, _w: &World, _alpha: f64) {
        for v in self.actors.values() {
            if let ActorView::Grenade { decal: Some(d), .. } = v {
                fx.decals.push(d.clone());
            }
        }
    }

    /// The pose hooks' reset is the view's (PoseHook::reset); the actors' views go.
    fn reset(&mut self, fx: &mut Fx) {
        self.clear_actors(fx);
        self.clear_witch(fx);
        self.evidence.clear();
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear_actors(fx);
        self.clear_witch(fx);
        self.evidence.clear();
        for (_, h) in self.speech.drain() {
            fx.player.kill(h);
        }
        for (_, d) in self.dots.drain() {
            if let Some(h) = d.h {
                fx.player.kill(h);
            }
        }
        self.aims.clear();
        self.boomed.clear();
    }
}

impl Velma {
    /// The first grenade's decal location (the tests').
    #[cfg(test)]
    pub(crate) fn grenade_decal_loc(&self) -> Option<[f64; 3]> {
        self.actors.values().find_map(|v| match v {
            ActorView::Grenade { decal: Some(d), .. } => Some(d.loc),
            _ => None,
        })
    }

    /// Fighter `id`'s witch sparkles are alive (the tests').
    #[cfg(test)]
    pub(crate) fn witch_live(&self, id: i32, player: &mvs_vfx::player::Player) -> bool {
        self.witch.get(&id).is_some_and(|h| player.alive(*h))
    }

    fn mine(&mut self, w: &World) -> bool {
        // (a mode's pawns come after the start: the answer latches once a Velma is in the match)
        if self.mine.is_none()
            && w.pawns.as_deref().unwrap_or(&w.players).iter().any(|&f| w.names.str(w.fighter(f).ch) == KEY)
        {
            self.mine = Some(true);
        }
        self.mine == Some(true)
    }

    /// The witch's effects: once a presentation tick.
    fn place_witch(&mut self, fx: &mut Fx, w: &World) {
        for fid in w.all_ids() {
            let fi = w.fighter(fid);
            if w.names.str(fi.ch) != KEY {
                continue;
            }
            let id = fi.id;
            // (the Lab's skin change gives the next spawn its skin: read each tick)
            let slug = fx
                .pawns
                .get(&id)
                .and_then(|p| w.char_data(fid).fighter.skins.as_ref()?.get(&p.skin))
                .map(|l| l.slug.as_str());
            if fi.life.dead > 0.0 || slug != Some(WITCH_SLUG) {
                if let Some(h) = self.witch.remove(&id) {
                    fx.player.kill(h);
                }
                continue;
            }
            let mut h = self.witch.get(&id).copied().filter(|h| fx.player.alive(*h));
            if h.is_none() {
                if !self.has(fx, WITCH_FX) {
                    continue;
                }
                let f = fx.socket_follow(w, id, Some(WITCH_SOCKET), [0.0; 3], &IDENTITY, [1.0; 3]);
                h = fx.attach_to_mesh(w, id, WITCH_FX, WITCH_SOCKET, f, &[]);
                if let Some(h) = h {
                    self.witch.insert(id, h);
                }
            }
            if let Some(h) = h {
                fx.player.set_visible(h, !crate::fx::mesh_hidden(w, fid));
            }
        }
    }

    /// Evidence Visuals(true) @275 (the header's "police ready"): each fighter's Buff_EvidenceCount read once a
    /// presentation tick.
    fn place_evidence(&mut self, fx: &mut Fx, w: &World) {
        for fid in w.all_ids() {
            let fi = w.fighter(fid);
            let id = fi.id;
            let now = fi.buffs.buffs.iter().find_map(|b| match &b.ext {
                BuffExt::EvidenceCount(e) if b.frames > 0.0 && w.names.str(b.cls) == EVIDENCE_COUNT => {
                    Some((e.count, e.allow, b.cls))
                }
                _ => None,
            });
            let Some((count, allow, cls)) = now else {
                self.evidence.remove(&id);
                continue;
            };
            let before = self.evidence.insert(id, (count, allow));
            let max =
                buff_def(w, ActorRef::Fighter(fid), cls).and_then(|d| d.get("MaxEvidence")).and_then(Value::as_f64);
            let max = max.unwrap_or(EVIDENCE_MAX);
            let Some((was, was_allowed)) = before else { continue };
            let fire = was_allowed && count > was && f64::from(count) >= max;
            if !fire {
                continue;
            }
            let team = fx.team(id);
            for (ps, params) in [(POLICE_READY_FX[0], &[][..]), (POLICE_READY_FX[1], &[("Team", team)][..])] {
                if !self.has(fx, ps) {
                    continue;
                }
                let f = fx.socket_follow(w, id, None, POLICE_READY_AT, &IDENTITY, [POLICE_READY_SCALE; 3]);
                fx.attach_to_mesh(w, id, ps, "None", f, params);
            }
        }
    }

    fn clear_witch(&mut self, fx: &mut Fx) {
        for (_, h) in self.witch.drain() {
            fx.player.kill(h);
        }
    }

    /// The system when it is loaded, else a warning once.
    fn has(&mut self, fx: &mut Fx, ps: &str) -> bool {
        if fx.player.emitter_count(ps).is_some() {
            return true;
        }
        if self.warned.insert(ps.to_owned()) {
            eprintln!("warning: Velma: {ps} is not loaded");
        }
        false
    }

    fn warn(&mut self, msg: String) {
        if self.warned.insert(msg.clone()) {
            eprintln!("warning: {msg}");
        }
    }

    // ---- the World actors, once a tick

    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        let mut seen = Vec::new();
        let mut actors = std::mem::take(&mut self.actors);
        for (aid, a) in w.actors.actors.iter() {
            if a.dead {
                continue;
            }
            let cls = w.names.str(a.cls);
            if ![HEAL_BEAM, CHILD_BEAM, EVIDENCE, EVIDENCE_NO_GRAV, SLOW_TO_STUN].contains(&cls) {
                continue;
            }
            seen.push(a.seq);
            if !actors.contains_key(&a.seq) {
                let Some(v) = self.make_actor(fx, w, aid, a) else { continue };
                actors.insert(a.seq, v);
            }
            let v = actors.get_mut(&a.seq).expect("the view just made");
            self.place_actor(fx, w, aid, a, v);
        }
        self.actors = actors;
        // K2_DestroyActor takes the actor's components with it.
        let gone: Vec<i32> = self.actors.keys().copied().filter(|s| !seen.contains(s)).collect();
        for s in gone {
            if let Some(v) = self.actors.shift_remove(&s) {
                drop_view(fx, v);
            }
        }
    }

    fn make_actor(&mut self, fx: &mut Fx, w: &World, aid: ActorId, a: &WorldActor) -> Option<ActorView> {
        let cs = actor_def(w, ActorRef::Actor(aid), a.cls).map(|d| &d.components);
        match &a.state {
            WorldActorState::Beam(_) => Some(ActorView::Beam { h: None }),
            WorldActorState::Evidence(e) => {
                let m = cs.and_then(|c| c.get("EvidenceStaticMesh")).and_then(Value::as_obj);
                let rel = xyz(m.and_then(|m| m.get("RelativeLocation")), 0.0);
                let rel_rot = rot_rows(m.and_then(|m| m.get("RelativeRotation")));
                let scale = xyz(m.and_then(|m| m.get("RelativeScale3D")), 1.0);
                let g = cs.and_then(|c| c.get("PS_TeamGlow")).and_then(Value::as_obj);
                let glow_rel = xyz(g.and_then(|g| g.get("RelativeLocation")), 0.0);
                let team = fx.team(a.owner_id);
                let mut glow = None;
                if let Some(ps) = g.and_then(|g| text(g, "Template"))
                    && self.has(fx, ps)
                {
                    let at = evidence_glow(
                        f64::from(e.x),
                        f64::from(e.y),
                        f64::from(e.spin),
                        f64::from(a.size_scale.unwrap_or(1.0)),
                        rel,
                        &rel_rot,
                        scale,
                        glow_rel,
                    );
                    glow = fx.player.spawn_attached(ps, &at, 1.0, &[("Team", team)]);
                }
                Some(ActorView::Evidence { glow, rel, rel_rot, scale, glow_rel })
            }
            WorldActorState::SlowToStunGrenade(g) => {
                let k = a.size_scale.unwrap_or(1.0);
                let ci = f64::from(fx.color_index(a.owner_id));
                let fog = if self.has(fx, SLOW_FX) {
                    let at = att(
                        [0.0, f64::from(-g.x), f64::from(g.y) + FOG_Z * f64::from(k)],
                        &IDENTITY,
                        Some([f64::from(k); 3]),
                    );
                    fx.player.spawn_attached(SLOW_FX, &at, 1.0, &[("Team", ci)])
                } else {
                    None
                };
                // (the root's scale at the first sight: its size comes in its spawn frame)
                let decal = cs.and_then(|c| c.get("Decal")).and_then(Value::as_obj).map(|dc| {
                    let f = if facing(w, a.owner) < 0.0 { -1.0 } else { 1.0 };
                    zone_decal(dc, SLOW_DECAL, a.seq, f, f64::from(k), (f64::from(g.x), f64::from(g.y)), ci)
                });
                Some(ActorView::Grenade { fog, decal, at: (f64::from(g.x), f64::from(g.y)), dissolved: false })
            }
            _ => None,
        }
    }

    fn place_actor(&mut self, fx: &mut Fx, w: &World, aid: ActorId, a: &WorldActor, v: &mut ActorView) {
        match (v, &a.state) {
            (ActorView::Beam { h }, WorldActorState::Beam(b)) => {
                if b.on && h.is_none() {
                    *h = self.beam_on(fx, w, aid, a, b);
                } else if !b.on
                    && let Some(x) = h.take()
                {
                    fx.player.kill(x);
                }
                let Some(h) = *h else { return };
                let holder = b.holder.unwrap_or(a.owner);
                let at = att([0.0, f64::from(-b.x), f64::from(b.y)], &yaw(facing(w, holder) < 0.0), None);
                fx.player.attach(h, &at);
                let end = [0.0, -b.end_x, b.end_y];
                fx.player.set_beam_target_point(h, 0, end.map(f64::from), 0);
                fx.player.set_beam_target_point(h, 1, end.map(f64::from), 0);
                fx.player.set_beam_target_point(h, 2, [0.0, f64::from(-b.reach_x), f64::from(b.reach_y)], 0);
            }
            (ActorView::Evidence { glow: Some(h), rel, rel_rot, scale, glow_rel }, WorldActorState::Evidence(e)) => {
                let g = a.size_scale.unwrap_or(1.0);
                let at = evidence_glow(
                    f64::from(e.x),
                    f64::from(e.y),
                    f64::from(e.spin),
                    f64::from(g),
                    *rel,
                    rel_rot,
                    *scale,
                    *glow_rel,
                );
                fx.player.attach(*h, &at);
            }
            (ActorView::Grenade { fog, decal, at, dissolved }, WorldActorState::SlowToStunGrenade(g)) => {
                let k = a.size_scale.unwrap_or(1.0);
                // the decal with the grenade's place (zone_decal's location is the place plus a fixed offset)
                let now = (f64::from(g.x), f64::from(g.y));
                if let Some(d) = decal {
                    d.loc[1] -= now.0 - at.0;
                    d.loc[2] += now.1 - at.1;
                }
                *at = now;
                if let Some(h) = *fog {
                    fx.player.attach(
                        h,
                        &att(
                            [0.0, f64::from(-g.x), f64::from(g.y) + FOG_Z * f64::from(k)],
                            &IDENTITY,
                            Some([f64::from(k); 3]),
                        ),
                    );
                }
                let rem = remaining(w, aid);
                if !*dissolved && rem <= mvs_sim::math::narrow(DISSOLVE_TIME) {
                    *dissolved = true;
                    if let Some(h) = *fog {
                        fx.player.set_param(h, "Dissolve", 1.0);
                        fx.player.deactivate(h);
                    }
                }
                if *dissolved && let Some(d) = decal {
                    let s = ((DISSOLVE_TIME - f64::from(rem)) / DISSOLVE_TIME).clamp(0.0, 1.0);
                    d.dissolve = Some(1.0 - s * s * (3.0 - 2.0 * s));
                }
            }
            _ => {}
        }
    }

    /// ToggleVisuals(true): the child beam's PS_Velma_MegaphoneAlly on the ally's mesh, then the beam's component.
    fn beam_on(&mut self, fx: &mut Fx, w: &World, aid: ActorId, a: &WorldActor, b: &Beam) -> Option<SystemId> {
        let holder = w.fighter(b.holder.unwrap_or(a.owner)).id;
        if w.names.str(a.cls) == CHILD_BEAM {
            // VelmaChildBeam SpawnVFX @382: on the ally's mesh (VB Spine is not a bone of the view: the component)
            let socket = if fx.rig(holder).is_some_and(|(_, r)| r.has_bone(VB_SPINE)) { VB_SPINE } else { "None" };
            let f = fx.socket_follow(w, holder, Some(socket), [0.0; 3], &IDENTITY, [1.0; 3]);
            let team = fx.team(a.owner_id);
            fx.attach_to_mesh(w, holder, ALLY_FX, socket, f, &[("Team", team)]);
        }
        let ps = actor_def(w, ActorRef::Actor(aid), a.cls).and_then(|d| text(&d.p, "BeamParticleEmitter"));
        let Some(ps) = ps else {
            self.warn(format!("{}: no BeamParticleEmitter in the class data", w.names.str(a.cls)));
            return None;
        };
        let team = fx.team(holder);
        let c = color_param([1.0, 1.0, team]);
        let at = att([0.0, f64::from(-b.x), f64::from(b.y)], &yaw(facing(w, b.holder.unwrap_or(a.owner)) < 0.0), None);
        let params = [("Team", team), ("Color.X", c[0]), ("Color.Y", c[1]), ("Color.Z", c[2])];
        fx.player.spawn_attached(ps, &at, 1.0, &params)
    }

    fn clear_actors(&mut self, fx: &mut Fx) {
        for (_, v) in self.actors.drain(..) {
            drop_view(fx, v);
        }
    }
}

fn drop_view(fx: &mut Fx, v: ActorView) {
    let h = match v {
        ActorView::Beam { h } => h,
        ActorView::Evidence { glow, .. } => glow,
        ActorView::Grenade { fog, .. } => fog,
    };
    if let Some(h) = h {
        fx.player.kill(h);
    }
}

/// PS_TeamGlow's place: under the clue mesh (the root at yaw 90 rolled by the spin, at the root's scale `g`; the
/// mesh's relative place and rotation under it), its relative place scaled by the mesh's, at world scale 3.
#[allow(clippy::too_many_arguments)]
fn evidence_glow(
    x: f64,
    y: f64,
    spin: f64,
    g: f64,
    rel: V3,
    rel_rot: &Rows,
    scale: V3,
    glow_rel: V3,
) -> mvs_vfx::Attachment {
    let mut root = compose_rows(&yaw(true), &rotation_rows(0.0, 0.0, spin.to_radians()));
    if g != 1.0 {
        for v in &mut root {
            *v *= g;
        }
    }
    let mut mesh = [0.0, -x, y];
    for (k, m) in mesh.iter_mut().enumerate() {
        *m += rel[0] * root[k] + rel[1] * root[3 + k] + rel[2] * root[6 + k];
    }
    let rows = compose_rows(rel_rot, &root);
    let loc = [0, 1, 2].map(|k| {
        mesh[k]
            + scale[0] * glow_rel[0] * rows[k]
            + scale[1] * glow_rel[1] * rows[3 + k]
            + scale[2] * glow_rel[2] * rows[6 + k]
    });
    att(loc, &rows, Some([GLOW_SCALE; 3]))
}

// ---- the projectile rules

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (BOOK, ProjRule { more: &["PS_Velma_BookTrail"], ..ProjRule::NONE }),
    (SASS_BUBBLE, ProjRule { own_emitter: Some(speech_emitter), end: Some(speech_end), ..ProjRule::NONE }),
    (MARK, ProjRule { tick: Some(mark_tick), end: Some(mark_end), ..ProjRule::NONE }),
    (AIM_PROJECTILE, ProjRule { made: Some(aim_made), tick: Some(aim_tick), end: Some(aim_end), ..ProjRule::NONE }),
    (EVIDENCE, ProjRule { own_emitter: Some(evidence_pickup), ..ProjRule::NONE }),
    (EVIDENCE_NO_GRAV, ProjRule { own_emitter: Some(evidence_pickup), ..ProjRule::NONE }),
];

/// SpeechBubble: on the owner's activated bubble (the newest, were there two); one gone within the tick has destroyed
/// the component with itself.
fn speech_emitter(c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx, fighter, x, y, .. } = *e else { return false };
    let Some(sys) = vfx.map(|v| w.names.str(v)) else { return false };
    let Some(&(sys, scale)) = TEXTS.iter().find(|(s, _)| *s == sys) else { return false };
    let seq = w
        .actors
        .projectiles
        .iter()
        .filter(|(_, pr)| {
            w.names.str(pr.cls) == SASS_BUBBLE
                && pr.owner_id == fighter
                && pr.owner.is_some()
                && pr.activated
                && !pr.dead
        })
        .map(|(_, pr)| pr.seq)
        .last();
    let Some(seq) = seq else { return true };
    let s = me(c);
    if let Some(h) = s.speech.remove(&seq) {
        fx.player.kill(h);
    }
    let right = w.by_id(fighter).is_some_and(|f| w.fighter(f).body.facing_dir > 0.0);
    let at = att([TEXT_DEPTH, f64::from(-x), f64::from(y)], &yaw(!right), Some(scale));
    let team = fx.team(fighter);
    if let Some(h) = fx.player.spawn_attached(sys, &at, 1.0, &[("Team", team)]) {
        s.speech.insert(seq, h);
    }
    true
}

fn speech_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(h) = me(c).speech.remove(&st.seq) {
        fx.player.kill(h);
    }
}

fn mark_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let ProjectileExt::Mark(m) = &pr.ext else { return };
    if (m.target.is_some() || m.ground)
        && let Some(h) = st.h.take()
    {
        fx.player.kill(h);
    }
    let s = me(c);
    if m.target.is_some() {
        let d = s.dots.entry(st.seq).or_default();
        if let Some(h) = d.h.take() {
            fx.player.kill(h);
        }
        return;
    }
    if !m.ground || s.dots.contains_key(&st.seq) {
        if let Some(d) = s.dots.get(&st.seq)
            && let Some(h) = d.h
        {
            fx.player.attach(h, &dots_place(w, st, d.side));
        }
        return;
    }
    let side = if pr.facing < 0.0 { 1.0 } else { -1.0 };
    let at = dots_place(w, st, side);
    let team = fx.team(pr.owner_id);
    let h = fx.player.spawn_attached(DOTS, &at, 1.0, &[("Team", team)]);
    s.dots.insert(st.seq, Dots { h, side });
}

fn mark_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    if let Some(h) = me(c).dots.remove(&st.seq).and_then(|d| d.h) {
        fx.player.kill(h);
    }
}

/// The dots' place: the root + (100, side x 15, 350) under the actor's rotation and root scale, scale 0.9.
fn dots_place(w: &World, st: &ProjFxState, side: f64) -> mvs_vfx::Attachment {
    let Some(pr) = w.actors.projectiles.get(st.id) else {
        return att([0.0, -st.x, st.y], &IDENTITY, Some([DOTS_SCALE; 3]));
    };
    let rot = mvs_view::actors::actor_rot(w, pr);
    let rel = [DOTS_AT[0], DOTS_AT[1] * side, DOTS_AT[2]];
    let mut loc = [0.0, -st.x, st.y];
    for (k, l) in loc.iter_mut().enumerate() {
        *l += f64::from(pr.scale) * (rel[0] * rot[k] + rel[1] * rot[3 + k] + rel[2] * rot[6 + k]);
    }
    att(loc, &rot, Some([DOTS_SCALE; 3]))
}

/// Text: a value in [1, 5) from the actor's id (the Deviation at the top).
fn hash01(id: i32) -> f64 {
    let h = (id.wrapping_add(7) as u32).wrapping_mul(2_246_822_519);
    f64::from(h % 1000) / 1000.0
}

fn aim_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    if let Some(h) = st.h {
        fx.player.set_param(h, "Text", 1.0 + 4.0 * hash01(st.seq));
    }
    if let Some(pr) = w.actors.projectiles.get(st.id) {
        me(c).aims.insert(st.seq, pr.owner_id);
    }
}

fn aim_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let hit = w
        .actors
        .projectiles
        .get(st.id)
        .is_some_and(|pr| matches!(&pr.ext, ProjectileExt::AimProjectile(a) if a.hit_once));
    if hit {
        boom(me(c), fx, st);
    }
}

fn aim_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    let s = me(c);
    boom(s, fx, st);
    s.aims.remove(&st.seq);
    s.boomed.remove(&st.seq);
}

/// BoomMulticast's DoOnce.
fn boom(s: &mut Velma, fx: &mut Fx, st: &ProjFxState) {
    if !s.boomed.insert(st.seq) {
        return;
    }
    let Some(&owner) = s.aims.get(&st.seq) else { return };
    let team = fx.team(owner);
    fx.player.spawn(BOOM, [0.0, -st.x, st.y], [0.0; 3], [BOOM_SCALE; 3], &[("Team", team)]);
}

/// PickupVisuals: PS_ShaggyScoobySnacksPickup at the event's place, no Team.
fn evidence_pickup(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx: Some(v), x, y, ox, scale, .. } = *e else { return false };
    if w.names.str(v) != PICKUP_VFX {
        return false;
    }
    fx.player.spawn(PICKUP_VFX, [f64::from(ox), f64::from(-x), f64::from(y)], [0.0; 3], [f64::from(scale); 3], &[]);
    true
}
