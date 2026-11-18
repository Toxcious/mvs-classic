//! Wonder Woman's effects: the lasso's rope, tip and wrist points,
//! WonderWoman_MeterBracerClash's charging component and the Buff_NullifyNextHit aura. The blast's glow on
//! her body material is the view's (mvs_view::chars::wonder_woman); the lasso's rotation is its mesh rule.
//!
//! What the classes have to draw (decoded): WonderWoman_MeterBracerClash's ParticleSystem
//! (PS_WonderWoman_Charging, the shared path's with the rule below; StaticMesh has no mesh; Cleanse is bHiddenInGame);
//! WW_ShieldActor / _U_Special: no mesh or particle component; WW_BracerShield: BeamParticleSystem (PS_TetherBeam to
//! GuardedAlly, which nothing writes).
//! Unverified: WW_BracerShield's beam is not drawn: the component auto-activates with BeamSource (0, 0, 0) and a null
//! BeamTarget actor for the actor's one tick; what a beam emitter with no target draws is not read.
//!
//! LassoProjectileFastV3: BeamParticleSystem (PS_WonderWomanLassoBeam under DamageBox): emitter 0 'Whip'
//! and emitter 1 'Lasso', the rope from the UserSet source point to the actor parameter BeamTarget. AttachBeam
//! @0-1640 (BeginPlay @4803): SetBeamSourcePoint(1 and 2, the instigator's fml_un_L_wristSocket, 0), BeamTarget.Actor
//! the lasso itself @948. ReceiveTick @7920-8439: SetBeamSourcePoint(1 and 2, FacingLeft ? fml_un_R_wristSocket :
//! fml_un_L_wristSocket, 0) (the system has two emitters: the write to 2 takes nothing). PS_TetherProjectile
//! (PS_WonderWomanLassoImpact) is the shared path's (no Team). TetherSourceVFX: SpawnEmitterAttached(PS_TetherPoints,
//! the owner's Mesh, fml_un_L_wristSocket, SnapToTarget) @4922. ReceiveDestroyed @7113 destroys the three. The
//! colour writes (BeginPlay @5597-5763, StartReturn @912-1301, TetherPointColorUpdate @12755) reach nothing drawn:
//! AttachBeam's InstanceParameters @948 (BeamSource, BeamTarget, TetherColor = the colour index's vector @906) and
//! TetherPointColor's SetVectorParameter / SetColorParameter(TetherColor) @533 / @694 name a parameter no module of
//! PS_WonderWomanLassoBeam reads (its one parameter is the ParameterDynamic 'Team', which nothing sets: the
//! distribution's default), and AttachBeam's SetScalarParameterValue(Team Index) on each material's dynamic instance
//! @1509 names a scalar neither WonderWomanLasso_MAT nor LassoSwingTrail_MAT has (their parameters: the dynamic
//! 'Team' through MF_TeamColor).
//! The tipper perk: BeginPlay @4106-4544 SetTemplate(PS_WonderWomanLassoPERK_Projectile) on PS_TetherProjectile;
//! MulticastLassoGrappleEffects @22326: PS_WW_LassoPerk_LightningGrab at the lasso, rotation (P -10, Y 90, R 0), scale
//! (0.5, 0.9, 0.9) (the sim's projectileDeath event).

use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::projectile::{Projectile, ProjectileExt};
use mvs_sim::chars::wonder_woman::perks_lasso::lasso_tipper_active;
use mvs_sim::events::Event;
use mvs_sim::fighter::common::actor_def;
use mvs_sim::fighter::{actor_y, facing};
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, V3, rotation_rows};

use super::{CharFx, CharFxHooks, ProjFxState, ProjRule};
use crate::fx::attach::{att, xyz};
use crate::fx::{Follow, Fx, Scale};

const BEAM: &str = "PS_WonderWomanLassoBeam";
const TIP_PERK: &str = "PS_WonderWomanLassoPERK_Projectile";
const GRAB: &str = "PS_WW_LassoPerk_LightningGrab";
const GRAB_SCALE: V3 = [0.5, 0.9, 0.9];
const POINTS: &str = "PS_TetherPoints";
const L_WRIST: &str = "fml_un_L_wristSocket";
const R_WRIST: &str = "fml_un_R_wristSocket";
const NULLIFY: &str = "Buff_NullifyNextHit_C";
const AURA: &str = "PS_ShieldBuff_P0";
/// @2329-2449: the variant when the owner's AppliedSkin is WonderWoman_S05.
const AURA_S05: &str = "PS_ShieldBuff_P0_Variant";
const S05: &str = "WonderWoman_S05";
/// One sim step (s).
const STEP: f64 = 1.0 / 60.0;
/// The ticks the sim counts ProcBuff's timeline in.
const PROC_TICKS: i32 = 9;
/// Timeline_0 (0.15 s, linear): ScaleOut and NewTrack_0, set @4002-4214 as the relative scale (ScaleOut, ScaleOut,
/// NewTrack_0).
const SCALE_OUT: [[f64; 2]; 5] =
    [[0.0, 1.0], [0.014353976, 0.804669], [0.039384767, 1.1347135], [0.083381124, 1.0], [0.15, 1.5]];
const NEW_TRACK_0: [[f64; 2]; 3] = [[0.0, 1.0], [0.017214641, 1.5070713], [0.15, 0.05]];

/// One lasso's own components.
#[derive(Debug)]
struct LassoFx {
    seq: i32,
    beam: Option<SystemId>,
    points: Option<SystemId>,
}

/// What Wonder Woman's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct WonderWoman {
    lassos: Vec<LassoFx>,
    /// Per fighter id: the aura while Buff_NullifyNextHit is on.
    auras: Vec<(i32, SystemId)>,
}

impl CharFxHooks for WonderWoman {
    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_auras(fx, w);
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[
    (
        "LassoProjectileFastV3_C",
        ProjRule {
            no_team: true,
            own_emitter: Some(grab_emitter),
            made: Some(lasso_made),
            tick: Some(lasso_tick),
            end: Some(lasso_end),
            ..ProjRule::NONE
        },
    ),
    ("WonderWoman_MeterBracerClash_C", ProjRule { scale: Some(clash_scale), tick: Some(clash_tick), ..ProjRule::NONE }),
];

fn me(c: &mut CharFx) -> &mut WonderWoman {
    match c {
        CharFx::WonderWoman(s) => s,
        _ => unreachable!("a Wonder Woman rule entered by another module"),
    }
}

/// MulticastLassoGrappleEffects' emitter at the event's place (SpawnEmitterAtLocation from the lasso's latent
/// resume: first ticked at the next frame's manager tick). No Team.
fn grab_emitter(_c: &mut CharFx, fx: &mut Fx, w: &World, e: &Event) -> bool {
    let Event::ProjectileDeath { vfx, x, y, ox, rot, .. } = *e else { return false };
    if !vfx.is_some_and(|v| w.names.str(v) == GRAB) {
        return false;
    }
    fx.player.spawn(GRAB, [f64::from(ox), f64::from(-x), f64::from(y)], rot.map(f64::from), GRAB_SCALE, &[]);
    true
}

/// The wrist socket's place of fighter `id` now (a mesh without it: the mesh component's place).
// Unverified: USkinnedMeshComponent::GetSocketLocation of a missing socket, 4.26 source, not read in the exe.
fn wrist(fx: &Fx, w: &World, id: i32, socket: &str) -> Option<V3> {
    let f = fx.socket_follow(w, id, Some(socket), [0.0; 3], &IDENTITY, [1.0; 3]);
    fx.place(w, &f).map(|a| a.loc)
}

fn lasso_made(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let id = pr.owner_id;
    // The tipper perk's SetTemplate: the default component made anew with the perk's system, at the actor with its
    // rotation and scale.
    // Unverified: swapped in at `made` (the build's SetTemplate runs in BeginPlay @4544), with no Team parameter.
    if lasso_tipper_active(w, st.id) {
        if let Some(h) = st.h.take() {
            fx.player.kill(h);
        }
        let f = Follow::Actor {
            turned: true,
            a: ActorRef::Projectile(st.id),
            rel: [0.0; 3],
            rel_rot: IDENTITY,
            scale: [1.0; 3],
        };
        st.h = fx.spawn_follow(w, TIP_PERK, f, 1.0, &[]);
    }
    // BeamParticleSystem: bAutoManageAttachment with AutoAttachSocketName Gem under DamageBox, which has no socket:
    // at the actor, with its rotation and scale.
    let f = Follow::Actor {
        turned: true,
        a: ActorRef::Projectile(st.id),
        rel: [0.0; 3],
        rel_rot: IDENTITY,
        scale: [1.0; 3],
    };
    let beam = fx.spawn_follow(w, BEAM, f, 1.0, &[]);
    if let Some(b) = beam {
        // BeamTarget.Actor = self: the actor's root (AttachBeam @948), set again at each tick
        fx.player.set_beam_actor(b, "BeamTarget", Some([0.0, f64::from(-pr.x), f64::from(pr.y)]));
        // AttachBeam's own source point (the left wrist whatever the facing) lasts until the actor's first tick
        if let Some(l) = wrist(fx, w, id, L_WRIST) {
            fx.player.set_beam_source_point(b, 1, l, 0);
        }
    }
    // TetherSourceVFX. SnapToTarget: world scale 1.
    let mut f = fx.socket_follow(w, id, Some(L_WRIST), [0.0; 3], &IDENTITY, [1.0; 3]);
    if let Follow::Socket { scale, .. } = &mut f {
        *scale = Scale::Fixed([1.0; 3]);
    }
    let points = fx.attach_to_mesh(w, id, POINTS, L_WRIST, f, &[]);
    me(c).lassos.push(LassoFx { seq: st.seq, beam, points });
}

/// ReceiveTick @7920-8439: the source at the wrist of her facing; the target at the actor.
// Unverified: the actor's first tick is taken in the frame of its spawn (right after `made`).
fn lasso_tick(c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    let Some(b) = me(c).lassos.iter().find(|l| l.seq == st.seq).and_then(|l| l.beam) else { return };
    fx.player.set_beam_actor(b, "BeamTarget", Some([0.0, f64::from(-pr.x), f64::from(pr.y)]));
    let left = w.by_id(pr.owner_id).is_some_and(|f| facing(w, f) < 0.0);
    if let Some(at) = wrist(fx, w, pr.owner_id, if left { R_WRIST } else { L_WRIST }) {
        fx.player.set_beam_source_point(b, 1, at, 0);
    }
}

/// ReceiveDestroyed @7216, @7249 (PS_TetherProjectile @7282: the shared path's).
fn lasso_end(c: &mut CharFx, fx: &mut Fx, _w: &World, st: &mut ProjFxState) {
    let s = me(c);
    if let Some(k) = s.lassos.iter().position(|l| l.seq == st.seq) {
        let l = s.lassos.remove(k);
        for h in [l.beam, l.points].into_iter().flatten() {
            fx.player.kill(h);
        }
    }
}

/// ReceiveTick @1576-1658: StaticMesh.SetWorldScale3D(GrowthScale) while not activated; the ParticleSystem is its
/// child with RelativeScale3D 0.125: its world scale is GrowthScale x 0.125. `ms`: the parent's scale under the root.
fn clash_scale(w: &World, pr: &Projectile, ms: V3) -> Option<V3> {
    let ProjectileExt::BracerClash(c) = &pr.ext else { return None };
    let comp = pr
        .owner
        .and_then(|o| actor_def(w, ActorRef::Fighter(o), pr.cls))
        .and_then(|d| d.components.get("ParticleSystem"))
        .and_then(Value::as_obj);
    let rs = xyz(comp.and_then(|c| c.get("RelativeScale3D")), 1.0);
    let g = if pr.scale != 0.0 { c.growth / pr.scale } else { 1.0 };
    Some([0, 1, 2].map(|k| ms[k] * rs[k] * f64::from(g)))
}

/// ParticleSystem.K2_DestroyComponent @810 at the activation: its particles go at once.
fn clash_tick(_c: &mut CharFx, fx: &mut Fx, w: &World, st: &mut ProjFxState) {
    let Some(pr) = w.actors.projectiles.get(st.id) else { return };
    if pr.activated
        && let Some(h) = st.h.take()
    {
        fx.player.kill(h);
    }
}

fn linear(keys: &[[f64; 2]], t: f64) -> f64 {
    if t <= keys[0][0] {
        return keys[0][1];
    }
    for i in 1..keys.len() {
        let (a, b) = (keys[i - 1], keys[i]);
        if t <= b[0] {
            return a[1] + (b[1] - a[1]) * (t - a[0]) / (b[0] - a[0]);
        }
    }
    keys[keys.len() - 1][1]
}

impl WonderWoman {
    /// Buff_NullifyNextHit's aura. CreateBuffVisuals @2234-3053: SpawnEmitterAttached(
    /// PS_ShieldBuff_P0, the owner's CapsuleComponent, VFX_Root (no socket on a capsule: the capsule itself),
    /// KeepRelativeOffset) with Team; RemoveBuffVisuals @181-1382 destroys it. ProcBuff's timeline scales it.
    // Unverified: "no socket on a capsule" is USceneComponent's GetSocketTransform (4.26 source, not read).
    // Deviation: AddVFX (hidden with the fighter) is not modelled.
    fn place_auras(&mut self, fx: &mut Fx, w: &World) {
        let h = std::f64::consts::FRAC_PI_2;
        for fid in w.all_ids() {
            let fi = w.fighter(fid);
            let id = fi.id;
            let buff = (fi.life.dead == 0.0)
                .then(|| fi.buffs.buffs.iter().find(|b| w.names.str(b.cls) == NULLIFY && b.frames > 0.0))
                .flatten();
            let k = self.auras.iter().position(|a| a.0 == id);
            let Some(b) = buff else {
                if let Some(k) = k {
                    fx.player.kill(self.auras.remove(k).1);
                }
                continue;
            };
            let proc = match &b.ext {
                mvs_sim::combat::buffs::BuffExt::NullifyNextHit(n) => n.proc,
                _ => -1,
            };
            let t = if proc < 0 { -1.0 } else { f64::from(PROC_TICKS - proc) * STEP };
            let (xy, z) = if t < 0.0 { (1.0, 1.0) } else { (linear(&SCALE_OUT, t), linear(&NEW_TRACK_0, t)) };
            let s = fi.body.size_scale;
            let rows = rotation_rows(0.0, if facing(w, fid) > 0.0 { -h } else { h }, 0.0);
            let at = att(
                [0.0, f64::from(-fi.body.x), f64::from(actor_y(w, fid))],
                &rows,
                Some([f64::from(s) * xy, f64::from(s) * xy, f64::from(s) * z]),
            );
            match k {
                Some(k) => fx.player.attach(self.auras[k].1, &at),
                None => {
                    let s05 = fx.pawns.get(&id).is_some_and(|p| p.skin == S05);
                    let team = fx.team(id);
                    if let Some(a) =
                        fx.player.spawn_attached(if s05 { AURA_S05 } else { AURA }, &at, 1.0, &[("Team", team)])
                    {
                        self.auras.push((id, a));
                    }
                }
            }
        }
    }

    fn clear(&mut self, fx: &mut Fx) {
        for (_, h) in self.auras.drain(..) {
            fx.player.kill(h);
        }
        for l in self.lassos.drain(..) {
            for h in [l.beam, l.points].into_iter().flatten() {
                fx.player.kill(h);
            }
        }
    }
}
