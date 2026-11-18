//! Iron Giant's actor classes: the world actors [`AntenaBolts`]
//! (C017_AntenaBolts_C), [`CannonBall`] (C017_CannonBall_C), [`TurretAim`] (C017_CannonAim_C), [`StompWave`]
//! (C017_ShockwaveActor_C) and [`DownThruster`] (C017_DownThrusterAttack_C), and the projectile subclasses
//! [`Missile`], [`DiscShot`] and [`Bomb`] (`ProjectileExt` variants). Their kinds and steps are actors.rs.
//!
//! The shared code reaches their places (`actors::world_actor`) and the dealers' fields (`combat::hits` dealer,
//! `combat::spawned_hits`): [`antena_place`] and its kin per class, [`antena_dealer_parts`] and [`antena_hit`] per
//! dealer class. The `SimActor` fields (`dead`, `sizeScale`, the charge, the dodge variables, `elem`, the lifetime
//! pause) are the [`WorldActor`]'s; a class keeps its own, and its class data (`p`) is read from the data.
//!
//! Sim space: x = -UE Y, y = UE Z. A Delay made in BeginPlay is first counted in the spawn frame: with age counted
//! from 1 in that frame's tick, its link runs in the tick where age = latentFrames(seconds) (as Shockwave's).

use crate::Real;
use crate::State;
use crate::actors::world_actor::WorldActor;
use crate::combat::BuffRef;
use crate::combat::HitPacket;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::fighter::anim::sample_boxes;
use crate::fighter::common::latent_frames;
use crate::fighter::{actor_y, facing, mesh_wx, mesh_wy};
use crate::math as js;
use crate::math::to_int32;
use crate::math::ue::{cosf, deg_to_rad, quat_rotate_vector, rotator_quaternion, sinf, size2d};
use crate::registry::props::{num, obj, text};
use crate::world::World;
use crate::world::stage_query::Shapes;
use mvs_collision::Ground;
use mvs_data::value::{Props, Value};

pub use crate::chars::shaggy::shockwave::Then;

/// `spawnPoint`: Comp_Spawner CompSpawnActor's place: the mesh socket (L_ / R_ swapped by GetSocket for an owner
/// facing left is the caller's business), or with no socket the attach actor's location - (0, 0, 100); plus the
/// notify's translation (Y along the owner's facing). `socket`: the socket's box index (`boxes.byName`).
pub fn spawn_point(w: &World, f: FighterId, socket: Option<u32>, rel_y: Real, rel_z: Real) -> (Real, Real) {
    let face = facing(w, f);
    let mut pose: Vec<f32> = Vec::new();
    if let Some(si) = socket
        && sample_boxes(w, f, &mut pose)
    {
        let ch = w.fighter(f).ch;
        let o =
            w.fixed.samplers.get(&ch).and_then(|b| b.offsets.get(usize::try_from(si).expect("a box index")).copied());
        if let Some(o) = o {
            let o = usize::try_from(o).expect("a box offset");
            return (
                mesh_wx(w, f) + (crate::real(pose[o]) + rel_y) * face,
                mesh_wy(w, f) + crate::real(pose[o + 1]) + rel_z,
            );
        }
    }
    (w.fighter(f).body.x + rel_y * face, actor_y(w, f) - 100.0 + rel_z)
}

/// The box index of the mesh socket `socket` (`owner.boxes.byName.get('socket:' + socket)`).
pub fn socket_index(w: &World, f: FighterId, socket: &str) -> Option<u32> {
    let ch = w.fighter(f).ch;
    let n = w.names.get(&format!("socket:{socket}"))?;
    w.fixed.samplers.get(&ch)?.by_name.get(&n).copied()
}

/// The notify's translation (Y, Z): `notify.Transforms?.Translation ?? {}`.
pub fn translation(np: &Props) -> (Real, Real) {
    let t = obj(np, "Transforms").and_then(|t| obj(t, "Translation"));
    (t.and_then(|t| num(t, "Y")).unwrap_or(0.0), t.and_then(|t| num(t, "Z")).unwrap_or(0.0))
}

/// A timer's expiry against the ticks counted since it was set (the rule of Projectile's MaxLifetime: it fires at
/// the first timer tick whose counted deltas exceed the rate).
fn elapsed(ticks: i32) -> crate::Clock {
    crate::consts::clock(crate::real(ticks))
}

/// `AntenaBolts` (C017_AntenaBolts_C; SpawnedActorParent; NumberOfActorsAllowed 1, Die With Instigator), attached to
/// the owner.
/// ReceiveBeginPlay (@6376): StartingBoltNumber N = the owner's ability slot 0 CurrentAmmo (@7294),
/// SetBoltNumberMulticast (@11073: the lead-in and loop sounds for N > 0, and after Delay(0.5) @2195 the burst
/// cast sounds), UpdateAbilityAmmo(0, -N, true) (@7543), TriggerAbilityActual(0) (@7665); then in order: every
/// other fighter of the team gets Buff_C017_AllyBurst_C with N + 1 stacks (@8711, @6213); Delay(BustDelay 0.5) ->
/// the burst (@258); for N > 0 (@8280) BoltNumber = N, the owner's KnockedBack is bound to HitEvent,
/// ProtectiveBuff = SpawnBuffActor(Buff_C017_Passive_C) on the owner and the bolts orbit (FindBoltStartLocations,
/// Timeline_0 0.25 s); Delay(5.0) (@8772) -> Death = true and Timeline_0.ReverseFromEnd (@15), whose end destroys
/// the actor. HitEvent (@11611): a bolt is lost per KnockedBack; the last one unbinds and destroys ProtectiveBuff.
/// AllyTempHealthSphere (radius 500) BeginOverlap (@8827): an ally not yet in TempHealthAllies gets
/// AddTempHealthPack(IGBoltTempHealth, 3.0, CanOverride, Time 3.0). ReceiveEndPlay (@10345) destroys ProtectiveBuff.
/// The orbiting spheres (radius 70) have no overlap binding: BoltOverlapEvent and HitBoxDamage are never reached.
// Deviation: the owner's KnockedBack is read at the actor's tick, not inside the hit; Timeline_0's reverse
// is counted as latentFrames(0.25); MagnetScrap (Item_C017_Scrap.MagnetTo_C017, an empty function) is left out.
//
// For the renderer. spin: the root's roll in degrees, RotatingMovement (world space, RotationRate Roll -180
// for an owner facing left at BeginPlay, else 180: @7894-7988; the Capsule's rotation is absolute @8037 and
// the spawner leaves the relative rotation at identity, Comp_Spawner @10515). alpha: Timeline_0's ExpandAlpha
// (linear 0 -> 1 in 0.25 s; PlayFromStart @4786, ReverseFromEnd @31), also ElectricRing's relative scale
// (@13298). ring: ElectricRing's visibility (true @8660 with a bolt, false @9608 when bolt 0 is lost).
// Unverified: spin and alpha are counted from the spawn frame's tick (the first RotatingMovement and
// timeline ticks against the spawn are not read), and neither stops for a hit pause of the owner.
#[derive(State, Debug, Default)]
pub struct AntenaBolts {
    pub attack_id: i32,
    /// PushHitBox with Damage = N + 1.
    pub hit: HitPacket,
    /// The owner's at the spawn.
    pub facing: Real,
    pub x: Real,
    pub y: Real,
    pub age: i32,
    /// StartingBoltNumber.
    pub start: Real,
    /// BoltNumber.
    pub bolts: Real,
    /// KnockedBack -> HitEvent.
    pub bound: bool,
    pub kb_seq: i32,
    /// ProtectiveBuff.
    pub buff: Option<BuffRef>,
    /// Death.
    pub dying: bool,
    /// TempHealthAllies, by fighter id.
    pub allies: Vec<i32>,
    /// AllyTempHealthSphere's SphereRadius (unscaled: FindBoltStartLocations reads it so).
    pub radius: Real,
    pub spin: Real,
    pub alpha: Real,
    pub ring: bool,
    pub socket: Option<u32>,
    pub rel_y: Real,
    pub rel_z: Real,
}

impl AntenaBolts {
    /// The constructor. `p`, `components`: the class data; `np`: the request's properties (Socket, Transforms);
    /// `ammo`: the owner's ability slot 0 CurrentAmmo.
    pub fn new(w: &World, f: FighterId, attack_id: i32, p: &Props, components: &Props, np: &Props, ammo: Real) -> Self {
        let mut hit = p.get("PushHitBox").and_then(|v| if let Value::Obj(o) = v { Some(o.clone()) } else { None });
        let mut hp = hit.take().unwrap_or_default();
        hp.insert("Damage".to_owned(), Value::Num(f64::from(ammo + 1.0)));
        let (rel_y, rel_z) = translation(np);
        let socket = socket_index(w, f, text(np, "Socket").unwrap_or(""));
        let mut s = AntenaBolts {
            attack_id,
            hit: HitPacket::from_props(&w.names, &hp, None),
            facing: facing(w, f),
            start: ammo,
            radius: obj(components, "AllyTempHealthSphere").and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0),
            socket,
            rel_y,
            rel_z,
            ..AntenaBolts::default()
        };
        s.follow(w, f);
        s
    }

    /// `follow`: the place on the owner.
    pub fn follow(&mut self, w: &World, f: FighterId) {
        (self.x, self.y) = spawn_point(w, f, self.socket, self.rel_y, self.rel_z);
    }

    pub fn burst_tick(p: &Props) -> Real {
        latent_frames(num(p, "BustDelay").unwrap_or(0.0))
    }

    pub fn death_tick() -> Real {
        latent_frames(5.0)
    }

    pub fn end_tick() -> Real {
        Self::death_tick() + latent_frames(0.25)
    }

    /// Bolt k (0 .. StartingBoltNumber - 1) is shown while k < BoltNumber: HitEvent hides Values[BoltNumber - 1]
    /// (@11886, SetBoltVisualsMulticast @12391 with its PS_C017_SPN_BoltElec).
    pub fn bolt_on(&self, k: Real) -> bool {
        k < self.bolts
    }

    /// Bolt k's place in sim space. FindBoltStartLocations (@214-618): with a = (k + 1) * 360 / N the relative
    /// start (0, FacingLeft ? R cos a : -R cos a, R sin a) (SelectFloat(A, B, bPickA) @444), R =
    /// AllyTempHealthSphere's radius; the timeline sets the relative location to VLerp(0, start, ExpandAlpha)
    /// (@5151); the root's roll turns (Y, Z) into (Y cos + Z sin, Z cos - Y sin). The mesh (scale 10) has pitch
    /// -(a + 180), facing left -(-a) (@3667). A relative location under the root: at the root's scale `sc` in the
    /// world. The angle is Multiply_IntFloat(k + 1, 360 / N) (@65, @214) into DegCos / DegSin (@260, @407: the CRT
    /// of A x pi/180); the root's roll turn is the component transform's (FRotator::Quaternion, then the quaternion
    /// turn of the relative location).
    pub fn bolt_at(&self, k: Real, sc: Real) -> (Real, Real) {
        let a = (k + 1.0) * (360.0 / js::max(self.start, 1.0));
        let ly = self.radius * cosf(deg_to_rad(a)) * (if self.facing < 0.0 { 1.0 } else { -1.0 }) * self.alpha * sc;
        let lz = self.radius * sinf(deg_to_rad(a)) * self.alpha * sc;
        let r = quat_rotate_vector(rotator_quaternion([0.0, 0.0, self.spin]), [0.0, ly, lz]);
        (self.x - r[1], self.y + r[2])
    }
}

/// `CannonBall` (C017_CannonBall_C; SpawnedActorParent, attached to the owner; HitBoxOn? false). ReceiveBeginPlay
/// (@10): the owner's HitstunHandler OnIsHitStunned bound to OnHit (@3223: K2_DestroyActor), FacingLeft = the
/// owner's, CurrentOffset = StartingOffset, the Aim component at relative (offset, 0, -50). ReceiveTick (@3172):
/// with adj = FacingLeft ? -MoveX : MoveX not 0, CurrentOffset = FClamp(offset +- MoveSpeed x dt, 0, MaxRange).
/// ActivateObject (@1452, no hitstun check): with the owner at (oY, oZ) and Aim at (aY, aZ): vz = (oZ - aZ) + 3500
/// plus |aY - oY| / 5, g = 9.8 x GravityMult x 100, t = (vz + g x sqrt(((aZ + vz^2 / (2 g)) - oZ) x 2 / g)) / g,
/// vy = |aY - oY| / t x 2.25; LaunchCharacter((0, FacingLeft ? vy : -vy, vz), true, true); K2_DestroyActor.
// Deviation: the owner's hitstun is read at the actor's tick (the delegate fires inside the hit).
#[derive(State, Debug, Default)]
pub struct CannonBall {
    pub facing: Real,
    /// CurrentOffset.
    pub offset: Real,
    pub age: i32,
    pub rel_y: Real,
    pub rel_z: Real,
}

/// The cannon ball's place: its `x` and `y` getters.
pub fn ball_place(w: &World, a: &WorldActor, s: &CannonBall) -> Option<(Real, Real)> {
    Some((w.fighter(a.owner).body.x + s.rel_y * s.facing, actor_y(w, a.owner) - 100.0 + s.rel_z))
}

impl CannonBall {
    pub fn new(w: &World, f: FighterId, p: &Props, np: &Props) -> Self {
        let (rel_y, rel_z) = translation(np);
        CannonBall { facing: facing(w, f), offset: num(p, "StartingOffset").unwrap_or(0.0), age: 0, rel_y, rel_z }
    }

    /// ReceiveTick's move of CurrentOffset: `mx`: the owner's input X.
    pub fn step(&mut self, mx: Real, p: &Props) {
        self.age += 1;
        let adj = if self.facing < 0.0 { -mx } else { mx };
        if adj == 0.0 {
            return;
        }
        let d = num(p, "MoveSpeed").unwrap_or(0.0) * DELTA_SECONDS;
        let moved = if adj > 0.0 { self.offset + d } else { self.offset - d };
        self.offset = js::min(js::max(moved, 0.0), num(p, "MaxRange").unwrap_or(0.0));
    }

    /// The Aim component: its relative location (offset, 0, -50) at the root's scale `k`.
    pub fn aim(&self, w: &World, a: &WorldActor) -> (Real, Real) {
        let k = a.size_scale.unwrap_or(1.0);
        let (x, y) = ball_place(w, a, self).expect("a place");
        (x + self.offset * self.facing * k, y - 50.0 * k)
    }

    /// ActivateObject's launch (sim vx, vy). `p`: the class data.
    pub fn launch(&self, w: &World, a: &WorldActor, p: &Props) -> (Real, Real) {
        let f = a.owner;
        let (ax, az) = self.aim(w, a);
        let dist = (ax - w.fighter(f).body.x).abs();
        let oz = actor_y(w, f);
        let vz = oz - az + 3500.0 + dist / 5.0;
        let g = 9.8 * num(p, "GravityMult").unwrap_or(0.0) * 100.0;
        let t = (vz + g * (((az + (vz * vz) / (2.0 * g) - oz) * 2.0) / g).sqrt()) / g;
        ((dist / t) * 2.25 * self.facing, vz)
    }
}

/// `TurretAim` (C017_CannonAim_C; SpawnedActorParent; not attached). ReceiveBeginPlay (@2083): AimVector =
/// StartVector, CurrentOffset = 0, Delay(0.2) -> OpenAimGate (@15). ReceiveTick (@60) with the gate open:
/// CurrentOffset = FInterpTo_Constant(CurrentOffset, 1, dt, MoveSpeed), AimVector = VLerp(StartVector, EndVector,
/// CurrentOffset) (and the AnimBP's BPI_SetAimAlpha2). ActivateObject (@986): nothing while the owner is in
/// hitstun; else the owner's BPI_CheckTurretFacing(Facing) (Pawn_C017: FacingLeft == TurretAimStartFacingLeft), the
/// owner's CompSpawnActor(C017_CannonShot_C, socket fml_un_R_wrist, direction (X, Facing ? Y : -Y, Z), 5000, 5000)
/// and K2_DestroyActor.
/// BPI_SetAimAlpha2 (the AnimBP's arm aim, sim state: pose.rs): ReceiveBeginPlay's ServerSetOffset(0) and each
/// gated tick's write of CurrentOffset pass the offset on (@810-939).
#[derive(State, Debug, Default)]
pub struct TurretAim {
    pub attack_id: i32,
    /// CurrentOffset.
    pub offset: Real,
    /// OpenAimGate.
    pub gate: bool,
    pub age: i32,
}

impl TurretAim {
    /// One tick: the new offset to hand the owner's AnimBP (`BPI_SetAimAlpha2`) when the gate is open.
    pub fn step(&mut self, p: &Props) -> Option<Real> {
        self.age += 1;
        let mut set = None;
        if self.gate {
            let ms = num(p, "MoveSpeed").unwrap_or(0.0);
            self.offset += js::min(js::max(1.0 - self.offset, -ms * DELTA_SECONDS), ms * DELTA_SECONDS);
            set = Some(self.offset);
        }
        if crate::real(self.age) == latent_frames(0.2) {
            self.gate = true;
        }
        set
    }

    /// AimVector in the owner's facing frame (Y forward, Z up). `p`: the class data.
    pub fn aim(&self, p: &Props) -> (Real, Real) {
        let s = obj(p, "StartVector");
        let e = obj(p, "EndVector");
        let g = |o: Option<&Props>, k: &str| o.and_then(|o| num(o, k)).unwrap_or(0.0);
        let l = |a: Real, b: Real| a + (b - a) * self.offset;
        (l(g(s, "Y"), g(e, "Y")), l(g(s, "Z"), g(e, "Z")))
    }
}

/// One shockwave_hit of C017_ShockwaveActor: the sphere at its ImpactPoint and the hit handed to pfgDoDamage.
#[derive(Debug)]
pub struct StompHit {
    pub n: i32,
    pub x: Real,
    pub y: Real,
    pub r: Real,
    pub hit: Props,
}

/// `StompWave` (C017_ShockwaveActor_C; SpawnedActorParent with its own graph; like ShaggyShockwaveActor but not its
/// child).
/// ReceiveBeginPlay (@1484): NumberOfShocks from the instigator's GetChargeValue(4): 1, 2 from 0.35, 3 from 0.65,
/// 4 from 0.95; AddActorWorldOffset 200 forward; SphereTraceSingle (radius 0.5, channel 3, fighters ignored) from
/// 5 above to 400 under the actor: starting_height = the hit's Location Z, else the actor's. Wave n (@1245 on):
/// spawn_shockwave traces at Shockwave<n>'s XY from height + 100 to height - 120; a blocking hit that does not
/// start in penetration moves the sphere to the ImpactPoint, spawns ParticleSystem<n> there and runs
/// shockwave_hit on every actor overlapping the sphere; then the stomp sound, and with more shocks to come
/// Delay(DelayTime 0.05) and the next wave from this sphere's Z. Otherwise, and on a miss, Delay(0.5) (@365) ->
/// K2_DestroyActor; wave 3 (@420) sets 'WaveKnock Back Scaling' = 10 first and ends through the DecalAlpha timeline
/// (0.1 s) and the Delay(0.5).
/// shockwave_hit: not the instigator; the vector (0, facing_modifier x 400, ZKnockback) x scale 1 gives
/// BaseKnockBack = its size and the direction (400, 900); KnockBackScaling = 'WaveKnock Back Scaling' (0 before
/// wave 3), Damage = damage_shockwave<n>, HitReaction 4, HitPauseTime = 'ShockWave Hit Pause Time', HitStunMult 1,
/// OnHitAudio sfx_share_class_tank_spike, OnHitEffectOverride PS_ShaggyRageImpact; pfgDoDamage with no buffs.
// Unverified: the trace is taken against the floor block and the boxes that block a ProjectileGroundTrace,
// as Shaggy's wave (channel 3 read as the same trace type); the actor's tick slot is the world's actor pass.
#[derive(State, Debug, Default)]
pub struct StompWave {
    pub attack_id: i32,
    pub hit: HitPacket,
    pub facing: Real,
    pub x: Real,
    pub y: Real,
    /// NumberOfShocks.
    pub shocks: i32,
    pub stage: i32,
    pub wait: i32,
    pub then: Option<Then>,
    pub height: Real,
    /// 'WaveKnock Back Scaling'.
    pub kb_scaling: Real,
    /// The actor's rotation (UE Pitch, Yaw, Roll in degrees), for its emitters' events: Comp_Spawner's spawn
    /// transform (@1385-2417), yaw 90 for an owner facing left, else -90; the one notify that spawns the class
    /// (C017_Stomp_Montage) adds no rotation of its own.
    pub rot: [Real; 3],
    /// Shockwave0..3: local X (forward from the actor origin), local Z and SphereRadius x scale.
    pub spheres: Vec<[Real; 3]>,
    /// The world Z spawn_shockwave's K2_SetWorldLocation put each sphere at (its ImpactPoint); `None` until the
    /// first.
    pub placed: Option<Vec<Option<Real>>>,
    pub ground: Ground,
}

/// A SphereTraceSingle's hit: the hit sphere's Location Z, the ImpactPoint Z, and whether it started inside.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Trace {
    loc: Real,
    impact: Real,
    initial: bool,
}

fn trace_box(g: &Ground, x: Real, z0: Real, z1: Real) -> Option<Trace> {
    let r = 0.5;
    if x < crate::math::narrow(g.left - r) || x > crate::math::narrow(g.right + r) {
        return None;
    }
    if z0 - crate::math::narrow(r) < crate::math::narrow(g.top)
        && z0 + crate::math::narrow(r) > crate::math::narrow(g.bottom)
    {
        return Some(Trace { loc: z0, impact: z0, initial: true });
    }
    if z0 - crate::math::narrow(r) < crate::math::narrow(g.top)
        || z1 - crate::math::narrow(r) > crate::math::narrow(g.top)
    {
        return None;
    }
    Some(Trace { loc: crate::math::narrow(g.top + r), impact: crate::math::narrow(g.top), initial: false })
}

impl StompWave {
    /// The constructor past the placement. `charged`: the owner's `percentCharged`; `p` and `components`: the class
    /// data.
    #[allow(clippy::too_many_arguments, reason = "the constructor's parameters")]
    pub fn new(
        attack_id: i32,
        facing: Real,
        charged: Real,
        x: Real,
        y: Real,
        p: &Props,
        components: &Props,
        ground: Ground,
    ) -> Self {
        let spheres = (0..4)
            .map(|n| {
                let c = obj(components, &format!("Shockwave{n}"));
                let rel = |key: &str, sub: &str| c.and_then(|c| obj(c, key)).and_then(|o| num(o, sub));
                [
                    rel("RelativeLocation", "X").unwrap_or(0.0),
                    rel("RelativeLocation", "Z").unwrap_or(0.0),
                    c.and_then(|c| num(c, "SphereRadius")).unwrap_or(32.0) * rel("RelativeScale3D", "X").unwrap_or(1.0),
                ]
            })
            .collect();
        StompWave {
            attack_id,
            hit: HitPacket { sparse: true, ..HitPacket::default() }, // (an empty `{}`, never filled)
            facing,
            x,
            y,
            shocks: if charged >= 0.95 {
                4
            } else if charged >= 0.65 {
                3
            } else if charged >= 0.35 {
                2
            } else {
                1
            },
            stage: 0,
            wait: 0,
            then: None,
            height: 0.0,
            kb_scaling: num(p, "WaveKnock Back Scaling").unwrap_or(0.0),
            rot: [0.0, if facing < 0.0 { 90.0 } else { -90.0 }, 0.0],
            spheres,
            placed: None,
            ground,
        }
    }

    /// ReceiveBeginPlay: AddActorWorldOffset(200 forward), the ground probe, then wave 0. `boxes`: what blocks the
    /// trace besides the floor block (World.blockers('trace')); `k`: the root's scale.
    pub fn begin(&mut self, boxes: Shapes, k: Real, p: &Props) -> Vec<StompHit> {
        self.x += 200.0 * self.facing;
        let t = self.trace(boxes, self.x, self.y + 5.0, self.y - 400.0);
        self.height = t.map_or(self.y, |t| t.loc);
        self.fire(boxes, k, p)
    }

    /// The actor tick's latent actions.
    pub fn step(&mut self, dead: &mut bool, boxes: Shapes, k: Real, p: &Props) -> Vec<StompHit> {
        let Some(then) = self.then else { return Vec::new() };
        if *dead {
            return Vec::new();
        }
        self.wait -= 1;
        if self.wait > 0 {
            return Vec::new();
        }
        if then == Then::Destroy {
            *dead = true;
            return Vec::new();
        }
        self.fire(boxes, k, p)
    }

    /// The spheres are the root's children: their relative X and radius at the root's scale (SetSizeScale comes
    /// after BeginPlay, so wave 0 is at 1).
    fn fire(&mut self, boxes: Shapes, k: Real, p: &Props) -> Vec<StompHit> {
        let n = self.stage;
        let i = usize::try_from(n).expect("a stage of 0..3");
        let sp = self.spheres[i];
        let x = self.x + sp[0] * self.facing * k;
        if n == 3 {
            self.kb_scaling = 10.0;
        }
        let t = self.trace(boxes, x, self.height + 100.0, self.height - 120.0);
        self.then = Some(Then::Destroy);
        self.wait = to_int32(latent_frames(0.5));
        let Some(t) = t.filter(|t| !t.initial) else { return Vec::new() };
        self.height = t.impact;
        let placed = self.placed.get_or_insert_with(Vec::new);
        if placed.len() <= i {
            placed.resize(i + 1, None);
        }
        placed[i] = Some(t.impact); // the sphere's K2_SetWorldLocation(ImpactPoint)
        if n == 3 {
            self.wait += to_int32(latent_frames(0.1));
        } else if self.shocks > n + 1 {
            self.stage += 1;
            self.then = Some(Then::Stage);
            self.wait = to_int32(latent_frames(num(p, "DelayTime").unwrap_or(0.0)));
        }
        let kx = num(p, "facing_modifier").unwrap_or(0.0) * 400.0;
        let kz = num(p, "ZKnockback").unwrap_or(0.0);
        let mut hit = Props::new();
        let mut put = |k: &str, v: Value| {
            hit.insert(k.to_owned(), v);
        };
        put("Damage", Value::Num(f64::from(num(p, &format!("damage_shockwave{n}")).unwrap_or(0.0))));
        put("BaseKnockBack", Value::Num(f64::from(size2d([kx, kz])))); // VSize
        put("KnockBackScaling", Value::Num(f64::from(self.kb_scaling)));
        let mut d = Props::new();
        d.insert("X".to_owned(), Value::Num(f64::from(kx)));
        d.insert("Y".to_owned(), Value::Num(f64::from(kz)));
        put("KnockBackDirectionVector", Value::Obj(d));
        put("HitReaction", Value::Num(4.0));
        put("HitPauseTime", Value::Num(f64::from(num(p, "ShockWave Hit Pause Time").unwrap_or(0.0))));
        put("HitStunMult", Value::Num(1.0));
        put("OnHitAudio", Value::Str("sfx_share_class_tank_spike".to_owned()));
        put("OnHitEffectOverride", Value::Str("PS_ShaggyRageImpact".to_owned()));
        put("PlayOnHitEffects", Value::Bool(true));
        put("ApplyHitPauseToDamageDealer", Value::Bool(true));
        put("AttackDecayType", Value::Num(0.0));
        put("KBI_Multiplier", Value::Num(1.0));
        vec![StompHit { n, x, y: t.impact, r: sp[2] * k, hit }]
    }

    /// Shockwave0..3 (Custom, object type HitBox, ProjectileDestructible: Overlap, on from the spawn) where they
    /// stand, as Shockwave.hitShapes (shaggy/shockwave.rs).
    pub fn hit_shapes(&self, k: Real, out: &mut Vec<crate::combat::boxes::WorldSphere>) {
        let f = self.facing * k;
        for (i, sp) in self.spheres.iter().enumerate().take(4) {
            let at = self.placed.as_ref().and_then(|p| p.get(i).copied().flatten());
            out.push(crate::combat::boxes::WorldSphere {
                x: self.x + sp[0] * f,
                y: at.unwrap_or(self.y + sp[1] * k),
                r: sp[2] * k,
                ..crate::combat::boxes::WorldSphere::default()
            });
        }
    }

    /// SphereTraceSingle (radius 0.5) straight down, as Shockwave.trace.
    fn trace(&self, s: Shapes, x: Real, z0: Real, z1: Real) -> Option<Trace> {
        let mut best = trace_box(&self.ground, x, z0, z1);
        for b in s.boxes {
            if best.is_some_and(|b| b.initial) {
                break;
            }
            let t = trace_box(b, x, z0, z1);
            if let Some(t) = t
                && (t.initial || best.is_none_or(|b| t.impact > b.impact))
            {
                best = Some(t);
            }
        }
        // a polygon solid: the shape traces' sphere swept down
        for q in s.polys {
            if best.is_some_and(|b| b.initial) {
                break;
            }
            if let Some((loc, impact, initial)) = q.down(x, z0, z1, 0.5)
                && (initial || best.is_none_or(|b| impact > b.impact))
            {
                best = Some(Trace { loc, impact, initial });
            }
        }
        best
    }
}

/// One of the thruster's four boxes in the play plane (centre and half extents).
#[derive(State, Debug, Default, Copy, PartialEq)]
pub struct FlameBox {
    pub x: Real,
    pub y: Real,
    pub hx: Real,
    pub hy: Real,
}

/// `DownThruster` (C017_DownThrusterAttack_C; SpawnedActorParent, attached to the owner; MaxLifetime 6,
/// NumberOfActorsAllowed 1).
/// The construction script (lines 239-245) puts LeftFlame at relative (FootOffset, 0, 0) and RightFlame at
/// (-FootOffset, 0, 0): along the actor's X, the owner's facing at the spawn. ReceiveTick (@1761): each foot's
/// start is (the box's X and Y, the actor's Z); TraceFlame (lines 249-358) is a LineTraceSingle (channel 5) from
/// it FlameLength down. With a hit at distance d the main box is at start - (0, 0, d / 2) with scale
/// (base.X, base.Y, d / 2) and the side box, with r = FlameLength - d, at world Y = start.Y + r / 2 + base.Y for
/// the foot traced with LeftFoot (start.Y - r / 2 - base.Y for the other), Z = start.Z - d + base.Z, scale
/// (base.X, r / 2, base.Z); without a hit both are at start - (0, 0, FlameLength / 2) with scale (base.X, base.Y,
/// FlameLength / 2). The transforms are world ones with a constant rotation: the boxes (unit extents) are axis
/// aligned, and world Y is sim -x whatever the facing. FlameBaseScale is the LeftFlame template's scale (100).
/// ReceiveActorBeginOverlap (@907) of an enemy fighter: NumberOfActorsInFlame + 1, the looping HitTimer
/// (DamageInterval 0.35) is set when it is not running, and DamageActor at once; EndOverlap (@1469) counts down and
/// clears the timer at 0. The timer's DealDamage runs DamageActor (@5529) on every overlapping enemy fighter:
/// pfgDoDamage(HitBoxDamage) with no elements and no buffs. HitBoxDamage is the parent's (Damage 0, no knockback,
/// HitStunMult 0): a light hit of 0.
/// The elemental side (combat/elem.rs): the class's Comp_Pawn_PassiveBuffManager row is Buff_Elemental_Fire,
/// BuffSelf, StartingStacks 2, Duration 0; it answers BPI_ShouldActorOverlap true (line 363) and
/// BPI_GetBoxCollision = LeftFlame (line 368), and CanBePooled is true in its defaults. `elem` holds the actor's
/// buff children. The CDO's own ElementalBuffs ([Buff_Elemental_Fire]) and 'On Hit Buffs' (Buff_Fire, 1 stack,
/// 1 s) are the parent's HitBoxesOn arguments (SpawnedActorParent @8531; the class has 'HitBoxOn?' false) and are
/// not what DamageActor passes (@5529: two empty arrays).
// Unverified: the trace is taken against the floor block and the stage solids that block channel 5 (read as
// ProjectileGroundNoPlatform; `static_grounds`) and the boxes against the hurtboxes in the play plane;
// ThrusterHitGround is sent by the kind's step (actors.rs) to Buff_C017_ScorchedEarth (perks.rs) (the flames are
// drawn in render/chars/iron-giant/; the cha_C017_attack_thruster_length RTPC is posted by the step too); base.Y is
// taken as the template's 100 for both boxes.
#[derive(State, Debug, Default)]
pub struct DownThruster {
    pub attack_id: i32,
    pub hit: HitPacket,
    pub facing: Real,
    pub x: Real,
    pub y: Real,
    pub age: i32,
    pub life_ticks: i32,
    /// The enemy fighters overlapping at the last tick, by id.
    pub in_flame: Vec<i32>,
    /// HitTimer.
    pub timer_on: bool,
    pub t0: i32,
    pub next_tick: Real,
    pub boxes: Vec<FlameBox>,
    /// TraceFlame's line trace of the tick per foot (left, then right): FlameLength - the hit's distance, -1 on a
    /// miss.
    pub flame: Vec<Real>,
    pub rel_y: Real,
    pub rel_z: Real,
    pub base: Real,
}

impl DownThruster {
    /// The constructor: `p` and `components` are the class data.
    pub fn new(attack_id: i32, facing: Real, hit: HitPacket, np: &Props, p: &Props, components: &Props) -> Self {
        let (rel_y, rel_z) = translation(np);
        let life = num(p, "MaxLifetime").unwrap_or(0.0);
        let mut k = 0;
        if life > 0.0 {
            while k < i32::MAX
                && crate::consts::clock(crate::real(k)).partial_cmp(&f64::from(life))
                    != Some(std::cmp::Ordering::Greater)
            {
                k += 1;
            }
        }
        DownThruster {
            attack_id,
            hit,
            facing,
            life_ticks: if life > 0.0 { k + 1 } else { 0 },
            rel_y,
            rel_z,
            base: obj(components, "LeftFlame")
                .and_then(|c| obj(c, "RelativeScale3D"))
                .and_then(|s| num(s, "Y"))
                .unwrap_or(0.0),
            ..DownThruster::default()
        }
    }

    /// The actor's tick: its place under the owner and the four boxes. False when MaxLifetime ran out. `owner`: the
    /// owner's (x, actorY); `grounds`: the stage block and the stage solids the trace meets; `k`: the root's scale.
    pub fn step(&mut self, owner: (Real, Real), grounds: Shapes, k: Real, p: &Props) -> bool {
        self.age += 1;
        if self.life_ticks > 0 && self.age >= self.life_ticks {
            return false;
        }
        self.x = owner.0 + self.rel_y * self.facing;
        self.y = owner.1 - 100.0 + self.rel_z;
        let len = num(p, "FlameLength").unwrap_or(0.0);
        let b = self.base;
        self.boxes.clear();
        self.flame.clear();
        for left in [true, false] {
            let sx = self.foot_x(left, k, p);
            let top = self.foot_top(grounds, sx, len);
            let d = top.map_or(-1.0, |t| self.y - t);
            if d < 0.0 {
                self.flame.push(-1.0);
                let bx = FlameBox { x: sx, y: self.y - len / 2.0, hx: b, hy: len / 2.0 };
                self.boxes.push(bx);
                self.boxes.push(bx);
                continue;
            }
            let r = len - d;
            // TraceFlame's LeftFoot is FacingLeft for LeftFlame (@2289) and its opposite for RightFlame (@3858,
            // @3887): the side box lies outward of each foot whatever the facing.
            let left_foot = left == (self.facing < 0.0);
            self.flame.push(r);
            self.boxes.push(FlameBox { x: sx, y: self.y - d / 2.0, hx: b, hy: d / 2.0 });
            self.boxes.push(FlameBox {
                x: if left_foot { sx - (r / 2.0 + b) } else { sx + (r / 2.0 + b) },
                y: self.y - d + b,
                hx: r / 2.0,
                hy: b,
            });
        }
        true
    }

    /// The top a foot's trace (x, from the actor's Z `len` down) meets first, or `None`: the highest box top in
    /// reach (a polygon solid's: its upper outline at x).
    pub fn foot_top(&self, grounds: Shapes, x: Real, len: Real) -> Option<Real> {
        let mut best: Option<Real> = None;
        for g in grounds.boxes {
            if !(x >= crate::math::narrow(g.left)
                && x <= crate::math::narrow(g.right)
                && self.y >= crate::math::narrow(g.top)
                && self.y - len <= crate::math::narrow(g.top))
            {
                continue;
            }
            if best.is_none_or(|b| crate::math::narrow(g.top) > b) {
                best = Some(crate::math::narrow(g.top));
            }
        }
        for q in grounds.polys {
            let g = q.bounds();
            if !(x >= crate::math::narrow(g.left) && x <= crate::math::narrow(g.right)) {
                continue;
            }
            let top = q.top_at(x);
            if self.y >= top && self.y - len <= top && best.is_none_or(|b| top > b) {
                best = Some(top);
            }
        }
        best
    }

    /// A foot's start X: LeftFlame at relative (FootOffset, 0, 0), RightFlame at (-FootOffset, 0, 0), at the root's
    /// scale `k`. (The boxes' own transforms are world ones: the root's scale does not reach them.)
    pub fn foot_x(&self, left: bool, k: Real, p: &Props) -> Real {
        let off = num(p, "FootOffset").unwrap_or(0.0) * k;
        self.x + (if left { off } else { -off }) * self.facing
    }

    /// The four flame boxes (object type HitBox, ProjectileDestructible: Overlap) as world boxes, upright, the depth
    /// taken as their X extent (a box met at its centre's depth needs one): what a ProjectileBarrier's
    /// StopProjectiles meets.
    pub fn hit_shapes(&self, out: &mut Vec<crate::combat::boxes::WorldSphere>) {
        for b in &self.boxes {
            out.push(crate::combat::boxes::WorldSphere {
                x: b.x,
                y: b.y,
                z: Some(0.0),
                r: 0.0,
                bx: Some([b.hx, 0.0, 0.0, 0.0, b.hy, 0.0, 0.0, 0.0, b.hx]),
                ..crate::combat::boxes::WorldSphere::default()
            });
        }
    }

    /// The looping HitTimer: true when it expires in this tick.
    pub fn timer_due(&mut self, p: &Props) -> bool {
        #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "a NaN tick is not due")]
        let not_due = !(elapsed(self.age - self.t0) > f64::from(self.next_tick));
        if !self.timer_on || not_due {
            return false;
        }
        self.next_tick += num(p, "DamageInterval").unwrap_or(0.0);
        true
    }

    pub fn start_timer(&mut self, p: &Props) {
        self.timer_on = true;
        self.t0 = self.age;
        self.next_tick = num(p, "DamageInterval").unwrap_or(0.0);
    }
}

/// `Missile extends Projectile` (WeaponModeMissile_C's own variable). FaceAtStart (@761): the owner faces left at
/// BeginPlay. It picks the end of the mesh's Rotation timeline (@2328-2451: relative rotation RLerp((0, 0, 0),
/// FaceAtStart ? (Pitch -90, Roll -110) : (Pitch -90, Roll 110), RotationAlpha) over 0.75 s) and the side of the
/// launch velocity (@384).
#[derive(State, Debug, Default)]
pub struct Missile {
    pub face_at_start: bool,
}

/// `DiscShot extends Projectile` (C017_DiscShot_C, made by cannon_aim.rs's fireDisc): BeginPlay (@596-656) reads the
/// owner's facing: facing left, its ParticleSystem moves to the relative location (40, 0, 0). `owner_facing`: that
/// read (-1 left, 1 right), for the renderer.
#[derive(State, Debug)]
pub struct DiscShot {
    /// The owner's facing at the shot's BeginPlay (@596): -1 or 1.
    pub owner_facing: Real,
}

impl Default for DiscShot {
    /// The field starts at 1.
    fn default() -> Self {
        DiscShot { owner_facing: 1.0 }
    }
}

/// `Bomb extends Projectile` (C017_Bomb_C: ParentProjectilePhysics with its own graph). ReceiveBeginPlay (@3198):
/// the parent's, the cast sound, ProjectileBounce bound to Bounce. Bounce (@3623), HitNonPlayer (@3193) and the
/// HitEnemyPlayer override (@2558, no parent call) all run StartExplosionMulticast (@3757, once: @2011): MaxSpeed
/// 0, Velocity 0, StartExpansion (@2593: the Capsule's relative scale = HitStartScale 2, ShouldExpand, the
/// expanding sound), a one-shot timer EndExpantion(ExpansionTime 1.5) and a looping one
/// TickDamage(RepeatedDamageTime 0.2). ReceiveTick (@2838) while ShouldExpand: TimeExpanding += dt, Capsule scale =
/// min(Lerp(2, 4, TimeExpanding / ExpansionTime), 4). TickDamage (@2563) and EndExpantion (@2568) read the HitBox's
/// overlapping fighters. The actor's rotation does not follow its velocity (bRotationFollowsVelocity false): a
/// facing hit keeps the spawn side. The HitBox sphere (radius 100) deals, not HitBox2D (42): the class has no
/// Supports2DHitboxes.
// Deviation: an explosion started by a hit counts its timers and TimeExpanding from the next tick; the
// HitNonPlayer start (a clank, a destructible) is not wired.
#[derive(State, Debug, Default)]
pub struct Bomb {
    pub exploding: bool,
    /// age at StartExplosionMulticast.
    pub t0: i32,
    /// TimeExpanding.
    pub expand_t: Real,
    /// The looping DamageTickTimer's next expiry, seconds after t0.
    pub next_tick: Real,
    pub sphere_r: Real,
}

/// What one tick of an explosion does: a TickDamage expiry or EndExpantion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Expansion {
    Tick,
    End,
}

impl Bomb {
    /// The constructor's own part: the HitBox sphere's radius. `components`: the class data.
    pub fn new(p: &crate::actors::Projectile, components: &Props) -> Self {
        Bomb {
            sphere_r: if p.sphere_3d {
                obj(components, "HitBox").and_then(|c| num(c, "SphereRadius")).unwrap_or(0.0)
            } else {
                p.hit_radius / p.scale
            },
            ..Bomb::default()
        }
    }
}

/// `Bomb.resize`.
fn bomb_resize(p: &mut crate::actors::Projectile, sphere_r: Real, s: Real) {
    p.scale = s;
    p.hit_radius = sphere_r * s;
}

/// `Bomb.explode`: false when it was exploding already. `class`: the class data's RepeatedDamageTime and
/// HitStartScale.
pub fn bomb_explode(p: &mut crate::actors::Projectile, repeated: Real, start_scale: Real) -> bool {
    let age = p.age;
    let crate::actors::projectile::ProjectileExt::Bomb(b) = &mut p.ext else { return false };
    if b.exploding {
        return false;
    }
    b.exploding = true;
    b.t0 = age;
    b.next_tick = repeated;
    let sphere_r = b.sphere_r;
    p.max_speed = 0.0;
    p.vx = 0.0;
    p.vy = 0.0;
    bomb_resize(p, sphere_r, start_scale);
    true
}

/// `Bomb.expand`: one tick of the explosion: [`Expansion::Tick`] for a TickDamage expiry, [`Expansion::End`] for
/// EndExpantion.
pub fn bomb_expand(p: &mut crate::actors::Projectile, total: Real, repeated: Real) -> Option<Expansion> {
    let age = p.age;
    let crate::actors::projectile::ProjectileExt::Bomb(b) = &mut p.ext else { return None };
    b.expand_t += DELTA_SECONDS;
    let a = b.expand_t / total;
    let sphere_r = b.sphere_r;
    let t = elapsed(age - b.t0);
    let (tick_due, end) = (t > f64::from(b.next_tick), t > f64::from(total));
    if !end && tick_due {
        b.next_tick += repeated;
    }
    bomb_resize(p, sphere_r, js::min(2.0 + (4.0 - 2.0) * a, 4.0));
    if end {
        Some(Expansion::End)
    } else if tick_due {
        Some(Expansion::Tick)
    } else {
        None
    }
}

/// The bolts' place (their `x`, `y`).
pub fn antena_place(w: &World, a: &WorldActor, s: &AntenaBolts) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The stomp wave's place (its `x`, `y`).
pub fn stomp_place(w: &World, a: &WorldActor, s: &StompWave) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The down thruster's place (its `x`, `y`).
pub fn thruster_place(w: &World, a: &WorldActor, s: &DownThruster) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

/// The bolts as a dealer (`DealerActor`): their place, facing and AttackID.
pub fn antena_dealer_parts(w: &World, a: &WorldActor, s: &AntenaBolts) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The stomp wave as a dealer.
pub fn stomp_dealer_parts(w: &World, a: &WorldActor, s: &StompWave) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The down thruster as a dealer.
pub fn thruster_dealer_parts(w: &World, a: &WorldActor, s: &DownThruster) -> (Real, Real, Real, i32) {
    let _ = (w, a);
    (s.x, s.y, s.facing, s.attack_id)
}

/// The bolts' HitBoxDamage packet (`hit`).
pub fn antena_hit(s: &AntenaBolts) -> &HitPacket {
    &s.hit
}

/// The stomp wave's packet.
pub fn stomp_hit(s: &StompWave) -> &HitPacket {
    &s.hit
}

/// The down thruster's packet.
pub fn thruster_hit(s: &DownThruster) -> &HitPacket {
    &s.hit
}
