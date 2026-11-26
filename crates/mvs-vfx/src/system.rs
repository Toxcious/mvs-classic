//! A spawned UParticleSystemComponent: its emitter instances, transform,
//! parameters, activation and completion.

use std::sync::Arc;

use mvs_math::js::{fround, max, min, sign};
use mvs_math::libm::{asin, atan2};

use crate::distribution::{Params, sample, sample1};
use crate::emitter::{EmitterInstance, Others};
use crate::math::{Rows, V3, ZERO3, normalize_axis, rotation_rows, rotator_rows};
use crate::random::RandomSource;
use crate::template::{EmitterTemplate, EventType};

/// A horizontal surface the Collision module's traces meet: the
/// plane Z and its world XY rectangle [x0, x1, y0, y1] (unbounded when None), UE units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub z: f64,
    pub rect: Option<[f64; 4]>,
}

/// The view the Collision module's distance gate reads (VfxPlayer.collisionView): the camera location of the last
/// drawn frame and its LOD distance factor (the horizontal FOV / 90).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionView {
    pub loc: V3,
    pub lod: f64,
}

/// An FBox (UE world unless said).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub min: V3,
    pub max: V3,
}

/// A loaded UParticleSystem.
#[derive(Debug)]
pub struct SystemTemplate {
    pub name: Arc<str>,
    pub emitters: Vec<Arc<EmitterTemplate>>,
    /// bUseFixedRelativeBoundingBox's FixedRelativeBoundingBox.
    pub fixed_box: Option<Box3>,
    /// WarmupTime (+0x38).
    pub warmup: f64,
    /// WarmupTickRate (+0x3c).
    pub warmup_rate: f64,
    /// bOrientZAxisTowardCamera (+0xb0 bit 0).
    pub orient_z: bool,
}

/// An emitter event of a tick.
#[derive(Clone, Debug)]
pub struct SystemEvent {
    pub name: Arc<str>,
    pub ty: EventType,
    pub loc: V3,
    pub vel: V3,
    pub time: f64,
}

/// What the emitters read and write of their component: the transform, parameters, activation, clock and the
/// tick's events.
#[derive(Debug)]
pub struct SysState {
    /// ComponentToWorld translation (UE world).
    pub loc: V3,
    /// The rotation's rows: the X, Y and Z axes.
    pub rot: Rows,
    /// ComponentToWorld Scale3D (signed: FTransform keeps a negative relative scale's sign).
    pub scale: V3,
    /// The emitters' OldLocation: the component location at the previous tick.
    pub old_loc: V3,
    /// UParticleSystemComponent OldPosition (+0x4cc): (0, 0, 0) at ActivateSystem (142be17a0), the location after
    /// each tick (FinalizeTickComponent 142be9cd0). Read by SourceMovement alone.
    pub old_pos: V3,
    pub params: Params,
    pub active: bool,
    /// The template has a fixed relative box (the emitters keep no box of their own).
    pub fixed_box: bool,
    /// World TimeSeconds for this tick (the player's game clock).
    pub world_time: f64,
    pub events: Vec<SystemEvent>,
    /// What the Collision modules meet: the stage's horizontal surfaces (None outside a stage).
    pub surfaces: Option<Arc<[Surface]>>,
    /// The Collision modules' view (None before a frame is drawn).
    pub collision_view: Option<CollisionView>,
    /// Beam emitters: UserSetSourceArray per emitter (the system's emitter index; SetBeamSourcePoint 142bf87c0).
    pub beam_sources: Vec<Vec<V3>>,
    /// UserSetTargetArray per emitter (SetBeamTargetPoint).
    pub beam_targets: Vec<Vec<V3>>,
    /// The actor instance parameters a beam end with method Actor reads (SetActorParameter): the place of the
    /// actor's root component as its owner set it last, None for no actor.
    pub beam_actors: Vec<(String, Option<V3>)>,
}

impl SysState {
    /// An emitter's event of this tick.
    pub fn push_event(&mut self, name: &Arc<str>, ty: EventType, loc: V3, vel: V3, time: f64) {
        self.events.push(SystemEvent { name: name.clone(), ty, loc, vel, time });
    }

    /// ComponentToWorld.TransformVector (rotation and scale).
    pub fn transform_vector(&self, v: V3) -> V3 {
        let (r, s) = (&self.rot, self.scale);
        let mut o = [0.0; 3];
        for c in 0..3 {
            o[c] = s[0] * v[0] * r[c] + s[1] * v[1] * r[3 + c] + s[2] * v[2] * r[6 + c];
        }
        o
    }

    /// EmitterToSimulation.TransformVector for a world-space emitter: FParticleEmitterInstance::UpdateTransforms
    /// builds it from ComponentToWorld without scale (read in the game: the instance's EmitterToSimulation, +0x60,
    /// has unit rows).
    pub fn sim_vector(&self, v: V3) -> V3 {
        let r = &self.rot;
        let mut o = [0.0; 3];
        for c in 0..3 {
            o[c] = v[0] * r[c] + v[1] * r[3 + c] + v[2] * r[6 + c];
        }
        o
    }

    /// ComponentToWorld.InverseTransformVector: unrotate, then x the safe scale reciprocal (0 where |Scale3D| <=
    /// 1e-8, the SMALL_NUMBER vector 1450937d0).
    pub fn inverse_transform_vector(&self, v: V3) -> V3 {
        let (r, s) = (&self.rot, self.scale);
        let mut o = [0.0; 3];
        for k in 0..3 {
            o[k] = if s[k].abs() <= 1e-8 {
                0.0
            } else {
                (v[0] * r[3 * k] + v[1] * r[3 * k + 1] + v[2] * r[3 * k + 2]) / s[k]
            };
        }
        o
    }

    /// A local emitter's point in the world (TransformVector + the location); a world one's unchanged.
    pub fn to_world(&self, local: bool, v: V3) -> V3 {
        if !local {
            return v;
        }
        let (r, s, l) = (&self.rot, self.scale, self.loc);
        let mut o = [0.0; 3];
        for c in 0..3 {
            o[c] = s[0] * v[0] * r[c] + s[1] * v[1] * r[3 + c] + s[2] * v[2] * r[6 + c] + l[c];
        }
        o
    }

    /// FBox::TransformBy(ComponentToWorld): the centre through the transform, the extent through |rotation x
    /// scale| (summed from 0 over k in turn).
    pub fn transform_box(&self, b: &Box3) -> Box3 {
        let mut c = [0.0; 3];
        let mut h = [0.0; 3];
        for k in 0..3 {
            c[k] = (b.min[k] + b.max[k]) / 2.0;
            h[k] = (b.max[k] - b.min[k]) / 2.0;
        }
        let c = self.to_world(true, c);
        let mut out = Box3 { min: ZERO3, max: ZERO3 };
        for j in 0..3 {
            let mut sum = 0.0;
            for k in 0..3 {
                sum += (h[k] * self.scale[k] * self.rot[3 * k + j]).abs();
            }
            out.min[j] = c[j] - sum;
            out.max[j] = c[j] + sum;
        }
        out
    }

    /// A signed unit axis (1 X, 2 Y, 3 Z, negative flipped) of the component (local) or the world:
    /// LocalToWorld.GetUnitAxis (140ce6530), so a negative scale flips it and one under SMALL_NUMBER gives 0.
    pub fn axis(&self, local: bool, a: i32) -> V3 {
        let k = (a.unsigned_abs() - 1) as usize;
        let sg = f64::from(a.signum());
        let mut o = [0.0; 3];
        if !local {
            o[k] = sg;
            return o;
        }
        let sc = self.scale[k];
        if sc * sc < 1e-8 {
            return o;
        }
        for c in 0..3 {
            o[c] = sg * sign(sc) * self.rot[3 * k + c];
        }
        o
    }
}

/// FTransform::Equals per component: every |a - b| within SMALL_NUMBER.
fn near(a: &[f64], b: &[f64]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-8)
}

/// A point strictly inside the box of origin o and extent e.
fn inside(o: V3, e: V3, v: V3) -> bool {
    (0..3).all(|c| o[c] - e[c] < v[c] && v[c] < o[c] + e[c])
}

/// USceneComponent Bounds (UE world): Origin, BoxExtent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub o: V3,
    pub e: V3,
}

/// A spawned UParticleSystemComponent.
#[derive(Debug)]
pub struct SystemInstance {
    pub t: Arc<SystemTemplate>,
    pub emitters: Vec<EmitterInstance>,
    pub sys: SysState,
    pub bounds: Bounds,
    /// TimeSinceLastForceUpdateTransform (+0x4f4).
    bounds_time: f64,
    /// The component's RelativeRotation (+0x128) as the rotator it is kept as, pitch / yaw / roll in degrees,
    /// summed and never normalised (bOrientZAxisTowardCamera).
    rel: V3,
    /// The attach parent's world rotation rows as the latest attachment gave them.
    parent: Option<Rows>,
    complete: bool,
    pub killed: bool,
    pub persistent: bool,
    pub hidden: bool,
    /// (the port's own Ghosts) Not drawn while its fighter is hidden from this screen, whatever `hidden` says.
    pub ghost: bool,
    /// UParticleSystemComponent::CustomTimeDilation.
    pub time_dilation: f64,
    /// Registered with the world manager but not in a tick list that still runs this frame.
    pub waiting: bool,
    /// TranslucencySortPriority.
    pub sort_priority: i32,
    /// The component's dynamic instance scalars, set on each emitter's material (a menu banner element's).
    pub material_scalars: Vec<(std::sync::Arc<str>, f32)>,
    /// The attach parent's transform the next tick takes (SpawnEmitterAttached).
    pub follow: Option<(V3, Rows, Option<V3>)>,
    /// Where the component was before the latest tick (None before the first): the draws between ticks.
    pub draw_from: Option<V3>,
    /// A level's own system (Player::spawn_level): Player::clear keeps it.
    pub level: bool,
    /// The owner reads its OnParticleSpawn: `spawned` keeps the latest tick's spawn events.
    pub on_spawn: bool,
    /// The latest tick's spawn events (event name, location) when `on_spawn`.
    pub spawned: Vec<(Arc<str>, V3)>,
}

impl SystemInstance {
    /// `rotator`: pitch, yaw, roll in degrees.
    pub fn new(
        t: Arc<SystemTemplate>,
        loc: V3,
        rotator: V3,
        scale: f64,
        params: Params,
        persistent: bool,
        rng: &mut dyn RandomSource,
    ) -> SystemInstance {
        let d = std::f64::consts::PI / 180.0;
        let emitters = t.emitters.iter().map(|e| EmitterInstance::new(e.clone(), rng)).collect();
        let sys = SysState {
            loc,
            rot: rotation_rows(rotator[0] * d, rotator[1] * d, rotator[2] * d),
            scale: [scale; 3],
            old_loc: loc,
            old_pos: ZERO3,
            params,
            active: true,
            fixed_box: t.fixed_box.is_some(),
            world_time: 0.0,
            events: Vec::new(),
            surfaces: None,
            collision_view: None,
            beam_sources: Vec::new(),
            beam_targets: Vec::new(),
            beam_actors: Vec::new(),
        };
        // The rotator here is the degrees themselves (a UE rotator is in degrees), not the radians it was given
        // (x 180 / PI).
        let rel = if t.orient_z { rotator } else { ZERO3 };
        let mut s = SystemInstance {
            t,
            emitters,
            sys,
            bounds: Bounds::default(),
            bounds_time: 0.0,
            rel,
            parent: None,
            complete: false,
            killed: false,
            persistent,
            hidden: false,
            ghost: false,
            time_dilation: 1.0,
            waiting: false,
            sort_priority: 0,
            material_scalars: Vec::new(),
            follow: None,
            draw_from: None,
            level: false,
            on_spawn: false,
            spawned: Vec::new(),
        };
        s.calc_bounds();
        s
    }

    /// A new transform. When it moved (FTransform::Equals within SMALL_NUMBER fails), UpdateComponentToWorld's
    /// PropagateTransformUpdate updates the bounds at once.
    pub fn set_transform(&mut self, loc: V3, rot: Rows, scale: Option<V3>) {
        let s = &mut self.sys;
        let moved = !near(&s.loc, &loc) || !near(&s.rot, &rot) || scale.is_some_and(|sc| !near(&s.scale, &sc));
        s.loc = loc;
        s.rot = rot;
        if let Some(sc) = scale {
            s.scale = sc;
        }
        if moved {
            self.calc_bounds();
        }
    }

    /// UParticleSystemComponent::CalcBounds (142be3ba0): a fixed relative box through ComponentToWorld; else the
    /// union of the boxes of the emitters with particles, each side pushed out by 0.05 of its size; with none, a
    /// point at the component location.
    pub fn calc_bounds(&mut self) {
        if let Some(f) = &self.t.fixed_box {
            let b = self.sys.transform_box(f);
            for c in 0..3 {
                self.bounds.o[c] = (b.min[c] + b.max[c]) / 2.0;
                self.bounds.e[c] = (b.max[c] - b.min[c]) / 2.0;
            }
            return;
        }
        let Some(u) = self.union_boxes(true) else {
            self.bounds = Bounds { o: self.sys.loc, e: ZERO3 };
            return;
        };
        for c in 0..3 {
            let d = (u.max[c] - u.min[c]) * 0.05;
            let (lo, hi) = (u.min[c] - d, u.max[c] + d);
            self.bounds.o[c] = (lo + hi) / 2.0;
            self.bounds.e[c] = (hi - lo) / 2.0;
        }
    }

    /// FBox += FBox over the live emitters' valid boxes (only those with particles with `with_particles`).
    fn union_boxes(&self, with_particles: bool) -> Option<Box3> {
        let mut u: Option<Box3> = None;
        for e in &self.emitters {
            let Some(b) = e.bbox else { continue };
            if e.dead || (with_particles && e.particle_count() == 0) {
                continue;
            }
            match &mut u {
                None => u = Some(b),
                Some(u) => {
                    for c in 0..3 {
                        u.min[c] = min(u.min[c], b.min[c]);
                        u.max[c] = max(u.max[c], b.max[c]);
                    }
                }
            }
        }
        u
    }

    /// FinalizeTickComponent (142be9cd0), after HasCompleted, while not complete and without a fixed box: past
    /// MaxTimeBeforeForceUpdateTransform (5) or when the live emitters' union (the zero vector when none) is not
    /// strictly inside the bounds, CalcBounds and the timer restarts.
    fn finalize_bounds(&mut self, dt: f64) {
        if self.complete || self.t.fixed_box.is_some() {
            return;
        }
        self.bounds_time += dt;
        let mut dirty = self.bounds_time > 5.0;
        if !dirty {
            let Bounds { o, e } = self.bounds;
            dirty = !match self.union_boxes(false) {
                Some(u) => inside(o, e, u.min) && inside(o, e, u.max),
                None => inside(o, e, ZERO3),
            };
        }
        if dirty {
            self.calc_bounds();
            self.bounds_time = 0.0;
        }
    }

    /// Every emitter's tick, then FinalizeTickComponent (142be9cd0): each live emitter's ProcessParticleEvents
    /// (142c1a0a0) gives this tick's events to its EventReceiverSpawn modules. ProcessParticleEvent (142c545d0): the
    /// names match; Count = RoundToInt(SpawnCount at the event's EmitterTime); the event's location or with
    /// bUsePSysLocation the component's; velocity 0 or InheritVelocityScale x the event's; ForceSpawn (142c0fa70) of
    /// Count particles with SpawnTime = DeltaTime, Interp 1 and no event payload. `view`: the camera location
    /// OrientZAxisTowardCamera reads.
    pub fn tick(&mut self, dt: f64, view: Option<V3>, rng: &mut dyn RandomSource) {
        self.draw_from = Some(self.sys.loc);
        self.spawned.clear();
        if let Some((loc, rot, scale)) = self.follow {
            if self.t.orient_z {
                self.parent = Some(rot);
                let r = self.relative_rows();
                self.set_transform(loc, r, scale);
            } else {
                self.set_transform(loc, rot, scale);
            }
        }
        let turned = self.t.orient_z && view.is_some_and(|v| self.orient_z_toward_camera(v));
        for i in 0..self.emitters.len() {
            let (before, rest) = self.emitters.split_at_mut(i);
            let (e, after) = rest.split_first_mut().unwrap();
            e.tick(dt, &mut self.sys, Others { before, after }, rng);
        }
        for i in 0..self.emitters.len() {
            let (before, rest) = self.emitters.split_at_mut(i);
            let (e, after) = rest.split_first_mut().unwrap();
            if e.dead {
                continue;
            }
            let t = e.t.clone();
            for r in &t.receive {
                let mut q = 0;
                while q < self.sys.events.len() {
                    let ev = &self.sys.events[q];
                    q += 1;
                    if *r.name != *ev.name || r.ty.is_some_and(|ty| ty != ev.ty) {
                        continue;
                    }
                    let (time, ev_loc, ev_vel) = (ev.time, ev.loc, ev.vel);
                    let n = (sample1(&r.count, time, &self.sys.params, rng) + 0.5).floor();
                    if n <= 0.0 {
                        continue;
                    }
                    let loc = if r.psys_loc { self.sys.loc } else { ev_loc };
                    let mut vel = ZERO3;
                    if r.inherit {
                        let k = sample(&r.inherit_scale, time, &self.sys.params, rng);
                        for c in 0..3 {
                            vel[c] = k[c] * ev_vel[c];
                        }
                    }
                    let mut j = 0.0;
                    while j < n {
                        e.spawn_one(&mut self.sys, Others { before, after }, rng, dt, Some(loc), 1.0, Some(vel), false);
                        j += 1.0;
                    }
                }
            }
        }
        if self.on_spawn {
            for ev in &self.sys.events {
                if ev.ty == EventType::Spawn {
                    self.spawned.push((ev.name.clone(), ev.loc));
                }
            }
        }
        self.sys.events.clear();
        self.sys.old_loc = self.sys.loc;
        self.sys.old_pos = self.sys.loc;
        self.complete = self.has_completed();
        // FinalizeTickComponent's transform block: with bIsTransformDirty the bounds timer is skipped,
        // UpdateComponentToWorld takes the new RelativeRotation and the timer restarts.
        if turned {
            self.sys.rot = self.relative_rows();
            self.calc_bounds();
            self.bounds_time = 0.0;
        } else {
            self.finalize_bounds(dt);
        }
    }

    /// Quat(RelativeRotation) x the parent's rotation (relative first).
    fn relative_rows(&self) -> Rows {
        rotator_rows(self.rel, self.parent.as_ref())
    }

    /// UParticleSystemComponent::OrientZAxisTowardCamera (142bf1d80): RelativeRotation += the rotator of
    /// FQuat::FindBetweenNormals((0, 0, 1), the unrotated direction to the camera), each axis summed, never
    /// normalised; true when that rotator is not zero (each axis fmod 360). Unverified: the camera is the last
    /// drawn frame's; asin / atan2 / sqrt are not the build's polynomial versions (1412c8e50).
    fn orient_z_toward_camera(&mut self, view: V3) -> bool {
        let (m, l) = (&self.sys.rot, self.sys.loc);
        let (mut dx, mut dy, mut dz) = (view[0] - l[0], view[1] - l[1], view[2] - l[2]);
        let s = dx * dx + dy * dy + dz * dz;
        if s > 1e-8 {
            let k = 1.0 / s.sqrt();
            dx *= k;
            dy *= k;
            dz *= k;
        }
        let lx = dx * m[0] + dy * m[1] + dz * m[2];
        let ly = dx * m[3] + dy * m[4] + dz * m[5];
        let lz = dx * m[6] + dy * m[7] + dz * m[8];
        let (mut x, mut y, mut w) = (-ly, lx, 1.0 + lz);
        let z = 0.0;
        if w < 1e-6 {
            (x, y, w) = (0.0, -1.0, 0.0);
        }
        let n = x * x + y * y + w * w;
        if n < 1e-8 {
            (x, y, w) = (0.0, 0.0, 1.0);
        } else {
            let k = 1.0 / n.sqrt();
            x *= k;
            y *= k;
            w *= k;
        }
        // The quaternion-to-rotator's radians to degrees (143b7e634, a float).
        let k = fround(180.0 / std::f64::consts::PI);
        let st = z * x - w * y;
        let yaw = atan2(2.0 * (w * z + x * y), 1.0 - 2.0 * (y * y + z * z)) * k;
        let (pitch, roll);
        if st < -0.4999995 {
            pitch = -90.0;
            roll = normalize_axis(-yaw - 2.0 * atan2(x, w) * k);
        } else if st > 0.4999995 {
            pitch = 90.0;
            roll = normalize_axis(yaw - 2.0 * atan2(x, w) * k);
        } else {
            pitch = asin(2.0 * st) * k;
            roll = atan2(-2.0 * (w * x + y * z), 1.0 - 2.0 * (x * x + y * y)) * k;
        }
        self.rel[0] += pitch;
        self.rel[1] += yaw;
        self.rel[2] += roll;
        pitch % 360.0 != 0.0 || yaw % 360.0 != 0.0 || roll % 360.0 != 0.0
    }

    /// The warm-up of ActivateSystem (142be17a0): with WarmupTime not 0, every emitter's ResetBurstList, then
    /// TickComponent steps of 0.032 s (1442b5404, a float sum), or of a positive WarmupTickRate, until the summed
    /// steps reach WarmupTime. `now`: World TimeSeconds.
    pub fn warm_up(&mut self, now: f64, view: Option<V3>, rng: &mut dyn RandomSource) {
        let time = self.t.warmup;
        if time == 0.0 {
            return;
        }
        self.sys.world_time = now;
        for e in &mut self.emitters {
            if !e.dead {
                e.fired.fill(false);
            }
        }
        let mut step = fround(0.032);
        if self.t.warmup_rate > 0.0 {
            step = if time <= self.t.warmup_rate { time } else { self.t.warmup_rate };
        }
        let mut t = 0.0;
        while t < time {
            self.tick(step * self.time_dilation, view, rng);
            t = fround(t + step);
        }
    }

    /// UParticleSystemComponent::DeactivateSystem (142be6f20): spawning stops; every emitter with
    /// bKillOnDeactivate is deleted, the others get OnDeactivateSystem.
    /// Activate(false) (142be15c0) on a kept component: ShouldActivate (142bfafb0) passes an inactive one;
    /// ActivateSystem (142be17a0) sets it active and, once it has been activated before, runs InitializeSystem
    /// (142bf0c80: InitParticles, each emitter's reinit) and the warm-up. Particles already alive keep going.
    pub fn activate(&mut self, now: f64, view: Option<V3>, rng: &mut dyn RandomSource) {
        if self.sys.active {
            return;
        }
        self.sys.active = true;
        self.sys.old_loc = self.sys.loc;
        self.sys.old_pos = ZERO3;
        for e in &mut self.emitters {
            e.reinit(rng);
        }
        self.warm_up(now, view, rng);
    }

    pub fn deactivate(&mut self) {
        self.sys.active = false;
        for e in &mut self.emitters {
            if e.t.kill_on_deactivate {
                e.kill();
            } else if !e.dead {
                e.on_deactivate();
            }
        }
    }

    /// DestroyComponent: every particle gone at once.
    pub fn kill(&mut self) {
        self.killed = true;
        for e in &mut self.emitters {
            e.clear_particles();
        }
    }

    /// UParticleSystemComponent::HasCompleted (142bef830), after the tick. Once deactivated an emitter is complete
    /// without particles; else an EmitterLoops 0 one never is, and a looping one is when HasCompleted, which deletes
    /// it with bKillOnCompleted. When every live emitter has bEmitterIsDone the component deactivates itself
    /// (bAutoDeactivate).
    fn has_completed(&mut self) -> bool {
        let mut complete = true;
        let mut all_done = true;
        let active = self.sys.active;
        for e in &mut self.emitters {
            if e.dead {
                continue;
            }
            if !e.done {
                all_done = false;
            }
            if !active {
                if e.particle_count() > 0 {
                    complete = false;
                }
            } else if e.t.loops < 1.0 || !e.has_completed() {
                complete = false;
            } else if e.t.kill_on_completed {
                e.kill();
            }
        }
        if all_done && active {
            self.deactivate();
        }
        complete
    }

    pub fn done(&self) -> bool {
        self.killed || (!self.persistent && self.complete)
    }
}
