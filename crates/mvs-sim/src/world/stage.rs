//! The stage's own sim: the calls the World makes to the stage's entry and the state they keep:
//! the level's world-clock Delays and timers, the solids and their collision, the moving pieces (the movers,
//! and the component moves), the placed actors' solid hits and the solids a sim
//! object spawns mid-match.
//!
//! A World whose stage has none of these (an empty entry, no solids, no placed actors: Sky Arena and the Lab) holds no
//! `World::stage_run` at all, so its saved state and hash are those of a World before the stage sim existed.

use crate::Real;
use crate::State;
use crate::actors::ItemId;
use crate::consts::DELTA_SECONDS;
use crate::fighter::FighterId;
use crate::fighter::StagePlatform;
use crate::fighter::common::warn_text;
use crate::math::{max, to_int32};
use crate::registry::entries::StageEntry;
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use crate::world::spawned_platform::BlockKind;
use crate::world::stage_actors::{StageActor, StageActorOwn};
use mvs_collision::Ground;
use mvs_collision::solids::{SolidShape, section};
use mvs_data::stage::{SolidKind, StageSolidData};

// `StageSolidData.flags`.
/// A fighter stands on its top (FindFloor walkable).
pub const SOLID_FLOOR: i32 = 1;
/// Its sides block the capsule (and the wall traces).
pub const SOLID_WALL: i32 = 2;
/// A pass-through platform's rule (ShouldIgnoreCollision), not a solid all round.
pub const SOLID_ONE_WAY: i32 = 4;
// The other channels the component blocks (the build's profiles; Pawn gives FLOOR | WALL): the queries of
// `World::blockers` (`BlockKind`) and the fighter's traces.
/// ProjectileGroundTrace (`BlockKind::Trace`).
pub const SOLID_TRACE: i32 = 8;
/// PhysicsBody (`BlockKind::Nav`: the ProjectileNavPlatform capsules).
pub const SOLID_NAV: i32 = 16;
/// Item (`BlockKind::Item`).
pub const SOLID_ITEM: i32 = 32;
/// WorldStaticDetection (`BlockKind::Detect`).
pub const SOLID_DETECT: i32 = 64;
/// WallJump (the wall slide and wall jump traces).
pub const SOLID_WALLJUMP: i32 = 128;
/// ProjectileExplode.
pub const SOLID_EXPLODE: i32 = 256;
/// ProjectileGroundNoPlatform.
pub const SOLID_NOPLATFORM: i32 = 512;
/// CanCharacterStepUpOn ECB_Yes.
pub const SOLID_STEP: i32 = 1024;

/// The Floor profile's `StageSolidData.flags` with `SOLID_STEP`: Pawn (FLOOR | WALL), TRACE, NAV, ITEM, DETECT,
/// WALLJUMP, EXPLODE, NOPLATFORM, STEP.
pub const FLOOR_FLAGS: i32 = 2043;

/// `StageTimer`: a world-clock Delay or timer of the stage's actors or level script, by world tick. `owner`: the
/// stage's number of the object it belongs to (a placed actor's `StageData.placed` index; a level script instance, a
/// stage's own number for each: Throne Room's three Map_M008_C each run BeginPlay). Latent actions are per object and
/// node, timer handles per object and function: two pillars of one class each have their own.
///
/// A Delay (`delay`) is the FDelayAction of one Delay / RetriggerableDelay node: `key` is the node (its latent UUID:
/// the stage's own number for it), so a Delay node reached again while its object's action runs starts nothing and a
/// RetriggerableDelay one restarts the count. Its owner's latent pass updates it: in the owner's own actor tick when
/// it ticks (`StageEntry::tickers` / `pre_tick`: AActor::Tick runs ProcessLatentActions after ReceiveTick), else in
/// UWorld::Tick's leftover pass (`stage_latent`).
///
/// A timer (not `delay`) is a K2_SetTimer(Delegate) handle: `key` is the delegate (the stage's number for its event);
/// setting it again replaces the running one. It fires by FTimerManager::Tick's rule: pulse n at the first timer tick
/// after `from` whose counted deltas exceed n x rate (a loop re-arms by ExpireTime += Rate; unverified); several
/// pulses due in one tick are called in a row.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct StageTimer {
    /// The world tick it is due at.
    pub at: i32,
    /// The stage's own number of the event it runs (`StageEntry::timer`).
    pub ev: i32,
    pub owner: i32,
    pub key: i32,
    /// A latent Delay (true) or a timer.
    pub delay: bool,
    /// `loop` (a timer): bLooping.
    pub looping: bool,
    /// (A timer) the world tick whose timer tick started it (the frame it was set in, or 1 at create).
    pub from: i32,
    /// (A timer) its Rate, float32 seconds.
    pub rate: Real,
    /// (A timer) the pulse due at `at` (1 for the first).
    pub n: i32,
}

/// `StageMoved`: the move state of a piece (a [`StageSolid`], a moving platform's
/// [`PlatformRun`]).
#[derive(State, Debug, Default)]
pub struct StageMoved {
    /// Where it has moved from its data place, sim space: the component's relative location less the level's.
    pub dx: Real,
    pub dy: Real,
    /// Its running MoveComponentTo.
    pub act: Option<ComponentMove>,
    /// That action's object (the latent action's callback target, in `StageTimer::owner`'s numbering) and its link
    /// (what `StageEntry::timer` gets at its end, with that owner; -1 for none).
    pub owner: i32,
    pub ev: i32,
    /// The move of the last leftover latent pass: what a fighter based on it follows in his next CMC tick
    /// (`Platform.dx`).
    pub mx: Real,
    pub my: Real,
    /// The part of `mx` / `my` that teleports wrote (`stage_teleport`), `None` while none: a teleport gives the PhysX
    /// body no kinematic velocity, so the simulated bodies' contacts leave it out
    /// (kinematicOf).
    pub tpx: Option<Real>,
    pub tpy: Option<Real>,
    /// Its body velocity (units / s) as the next CMC tick reads it: the move of the pass before the last / dt.
    pub vx: Real,
    pub vy: Real,
}

impl StageMoved {
    /// The kinematic move a simulated body's contact with it takes (kinematicOf over the
    /// piece's mark): the pass's move less its teleports.
    pub fn kinematic(&self) -> crate::fighter::traces::KinematicMove {
        crate::fighter::traces::KinematicMove { mx: self.mx, my: self.my, tpx: self.tpx, tpy: self.tpy }
    }
}

/// `StageSolid`: a solid's state in the match, by its index in `StageData.solids`, with its move.
#[derive(State, Debug, Default)]
pub struct StageSolid {
    /// `StageSolidData.id`; -1 for a run-time solid's.
    pub id: i32,
    /// Its collision on (SetCollisionEnabled QueryAndPhysics / NoCollision: `stage_solid_enable`).
    pub enabled: bool,
    /// Its collision came on since the frame's CMCs (`SolidBox::fresh`, `stage_solids_moved`).
    pub fresh: bool,
    pub m: StageMoved,
    /// `OWNERS`: the stage actor (by its place in `StageState::actors`) whose
    /// `solid_hit` hook takes its hits.
    pub hit_owner: Option<u32>,
    /// `STATIC`: its static mark (a run-time Floor box's).
    pub is_static: bool,
}

/// `ComponentMove`: UKismetSystemLibrary::MoveComponentTo (142b2a360) and its
/// latent action's update (142b335e0) for one component: its relative location in the play plane.
///
/// `call`: no action yet and MoveAction 0 (Move) makes one: TotalTime = OverTime, TimeElapsed 0, bInterpolating, the
/// eases, Target = the asked relative location, Initial = the component's relative location at the call (no world
/// space anywhere: a caller that passes a world location moves the component to it relative to its parent). On a
/// running action Move sets TotalTime, Target and Initial = the location at the call and restarts the time (the eases
/// stay those of the first call); Stop (1) clears bInterpolating; Return (2) sets TotalTime = TimeElapsed, Target =
/// the old Initial, Initial = the location at the call, and restarts.
///
/// `update`: TimeElapsed += the object's DeltaTime (float32); complete = TimeElapsed >= TotalTime; while interpolating
/// the location is Target when complete, else Lerp(Initial, Target, blend): blend = t / TotalTime, squared with ease
/// in alone, its square root with ease out alone, InterpEaseInOut(0, 1, t, 2) with both. SetRelativeLocation without a
/// sweep: nothing blocks it. The action ends (its output pin fires) when complete or not interpolating: a stopped one
/// ends at its next update without moving.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct ComponentMove {
    pub total: Real,
    pub elapsed: Real,
    pub interpolating: bool,
    pub ix: Real,
    pub iy: Real,
    pub tx: Real,
    pub ty: Real,
    pub x: Real,
    pub y: Real,
    pub ease_out: bool,
    pub ease_in: bool,
}

impl ComponentMove {
    fn new(x: Real, y: Real, ease_out: bool, ease_in: bool) -> Self {
        ComponentMove { interpolating: true, x, y, ease_out, ease_in, ..ComponentMove::default() }
    }

    /// The call on this component: `cur` is its running action or `None`. Returns the action to keep.
    #[allow(clippy::too_many_arguments, reason = "the node's pins")]
    pub fn call(
        cur: Option<ComponentMove>,
        x: Real,
        y: Real,
        tx: Real,
        ty: Real,
        ease_out: bool,
        ease_in: bool,
        over_time: Real,
        action: i32,
    ) -> Option<ComponentMove> {
        let mut c = match cur {
            None => {
                if action != 0 {
                    return None;
                }
                let mut c = ComponentMove::new(x, y, ease_out, ease_in);
                c.total = over_time;
                c.tx = tx;
                c.ty = ty;
                c
            }
            Some(mut c) => match action {
                0 => {
                    c.total = over_time;
                    c.tx = tx;
                    c.ty = ty;
                    c
                }
                1 => {
                    c.interpolating = false;
                    return Some(c);
                }
                2 => {
                    c.total = c.elapsed;
                    c.tx = c.ix;
                    c.ty = c.iy;
                    c
                }
                _ => return Some(c),
            },
        };
        c.x = x;
        c.ix = x;
        c.y = y;
        c.iy = y;
        c.elapsed = 0.0;
        Some(c)
    }

    /// One update; true when the action has ended.
    pub fn update(&mut self, dt: Real) -> bool {
        self.elapsed += dt;
        let complete = self.elapsed >= self.total;
        if self.interpolating {
            if complete {
                self.x = self.tx;
                self.y = self.ty;
            } else {
                let t = self.elapsed / self.total;
                let mut b = t;
                if self.ease_in && self.ease_out {
                    b = if t < 0.5 {
                        2.0 * t * (2.0 * t) * 0.5
                    } else {
                        (1.0 - (2.0 - 2.0 * t) * (2.0 - 2.0 * t)) * 0.5 + 0.5
                    };
                } else if self.ease_in {
                    b = t * t;
                } else if self.ease_out {
                    b = t.sqrt();
                }
                self.x = self.ix + (self.tx - self.ix) * b;
                self.y = self.iy + (self.ty - self.iy) * b;
            }
        }
        complete || !self.interpolating
    }
}

/// `SolidBox`: a solid's box as the fighters' movement meets it (`World.platformsOfFrame`): the
/// box-kind solids that block a capsule (`SOLID_FLOOR` / `SOLID_WALL`), where their state (`s`: `solid`, by its index in
/// `StageState::solids`) puts them: the data place plus the solid's move. A switched-off one is in the list and
/// ignored by every capsule. Its kinematic move for the simulated bodies is its solid's ([`StageMoved::kinematic`]).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct SolidBox {
    pub dead: bool,
    pub solid: u32,
    pub flags: i32,
    pub l0: Real,
    pub r0: Real,
    pub t0: Real,
    pub b0: Real,
}

impl SolidBox {
    fn s<'a>(&self, run: &'a StageState) -> &'a StageSolid {
        &run.solids[self.solid as usize]
    }
    /// Its box now (`left`, `right`, `top`, `bottom`).
    pub fn ground(&self, run: &StageState) -> Ground {
        let m = &self.s(run).m;
        Ground {
            left: f64::from(self.l0 + m.dx),
            right: f64::from(self.r0 + m.dx),
            top: f64::from(self.t0 + m.dy),
            bottom: f64::from(self.b0 + m.dy),
        }
    }
    /// `dx` / `dy` / `vx` / `vy`: its solid's move and velocity.
    pub fn moved<'a>(&self, run: &'a StageState) -> &'a StageMoved {
        &self.s(run).m
    }
    pub fn fresh(&self, run: &StageState) -> bool {
        self.s(run).fresh
    }
    /// `trace`: false while switched off (no trace of any channel meets it); on, as its profile blocks
    /// ProjectileGroundTrace (Floor, BlockAll).
    pub fn trace(&self, run: &StageState) -> bool {
        self.s(run).enabled && (self.flags & SOLID_TRACE) != 0
    }
    pub fn on(&self, run: &StageState) -> bool {
        self.s(run).enabled
    }
    /// `objectType`: its component's object type (`StageSolidData.objectType`). A run-time Floor box, WorldStatic
    /// (`RuntimeBox.objectType`), is no `SolidBox` here.
    pub fn object_type<'a>(&self, w: &'a World, run: &StageState) -> Option<&'a str> {
        let id = self.s(run).id;
        let d = w.data.stage.solids.as_ref()?.get(usize::try_from(id).ok()?)?;
        d.object_type.as_deref()
    }
    pub fn blocks_wall_jump(&self, run: &StageState) -> bool {
        self.s(run).enabled && (self.flags & SOLID_WALLJUMP) != 0
    }
    pub fn can_step_up_on(&self) -> bool {
        (self.flags & SOLID_STEP) != 0
    }
    /// `ignores(f)`: every capsule while switched off.
    pub fn ignores(&self, run: &StageState) -> bool {
        !self.s(run).enabled
    }
    pub fn span_at(&self, run: &StageState, x: Real) -> bool {
        let g = self.ground(run);
        x >= crate::math::narrow(g.left) && x <= crate::math::narrow(g.right)
    }
    pub fn span_near(&self, run: &StageState, x: Real, r: Real) -> bool {
        let g = self.ground(run);
        // the bounds in float, as the build holds the box
        x > crate::math::narrow(g.left) - r && x < crate::math::narrow(g.right) + r
    }
    pub fn clamp_x(&self, run: &StageState, x: Real) -> Real {
        let g = self.ground(run);
        max(crate::math::narrow(g.left), crate::math::min(crate::math::narrow(g.right), x))
    }
}

/// `PlatformRun`: a moving platform's state (`StagePlatform.run`, kept here by the platform's
/// index in `Arena2d::stage_platforms` and `StageData.platforms`): BasePlatformBP's CurrentCurve and LastMove.Z and
/// the move. `d`: its data, `StageData.platforms[index].moving`.
#[derive(State, Debug, Default)]
pub struct PlatformRun {
    pub index: u32,
    /// The Box at its data place (sim space; the Scene root's location is the Box's centre: the stage import asserts
    /// the Parent and the Box carry no relative transform).
    pub l0: Real,
    pub r0: Real,
    pub t0: Real,
    pub b0: Real,
    /// The Scene root at its data place (sim x, y).
    pub cx: Real,
    pub cy: Real,
    /// CurrentCurve (class default 0).
    pub curve: i32,
    /// LastMove.Z (class default 0): (Start + Distance).Z / (Duration x 60) of the last Move Platform, the absolute
    /// destination over nominal frames as the graph writes it (@878-1143); only the one-way gate reads it. Its X and Y
    /// are read nowhere.
    pub last_move_z: Real,
    pub m: StageMoved,
}

/// The stage entry's own state (`StageHooks.create`'s result), a variant per stage: they come with the stages' ports.
#[derive(State, Debug, Default)]
pub enum StageOwn {
    #[default]
    None,
    TreeFort(crate::stages::tree_fort::TreeFortOwn),
    Cromulons(crate::stages::cromulons::CromulonsOwn),
    Mansion(crate::stages::scoobys_haunted_mansion::MansionOwn),
    Batcave(crate::stages::batcave::BatcaveOwn),
    ThroneRoom(crate::stages::throne_room::ThroneRoomOwn),
    Court(crate::stages::the_court::CourtOwn),
}

/// `StageRun`: the stage's sim state.
#[derive(State, Debug, Default)]
pub struct StageState {
    pub s: StageOwn,
    pub timers: Vec<StageTimer>,
    pub solids: Vec<StageSolid>,
    /// The box solids the fighters' movement meets (`World.platformsOfFrame`), over `solids`.
    pub boxes: Vec<SolidBox>,
    /// `actors`: the stage's placed actors, by their place in the stage's list, then the run-time ones.
    pub actors: Vec<StageActor>,
    /// The moving platforms' states (`StagePlatform.run`), in the platforms' order.
    pub platforms: Vec<PlatformRun>,
    /// Where in the frame a stage call is made (`phase`): module values, not state: they only carry within one step
    /// or the World's construction; create and a reset set `IDLE`, every step's `stage_pre_tick` starts the frame
    /// afresh. They decide a new Delay's first counted frame and a new timer's start.
    #[state(skip)]
    pub phase: u8,
    /// `marked`: the owners processed with an entry this frame (the "processed this frame" mark of an object that had
    /// actions).
    #[state(skip)]
    pub marked: Vec<i32>,
    /// `links`: `stage_moves`' buffer of the ended actions' (owner, link), empty between its calls.
    #[state(skip)]
    pub links: Vec<(i32, i32)>,
}

/// The World's construction (create: BeginPlay), a Lab reset.
const IDLE: u8 = 0;
/// A ticking level actor's ReceiveTick (`StageEntry::pre_tick`), TG_PrePhysics ahead of the pawns.
const PRE: u8 = 1;
/// The rest of the tick groups (a fighter's tick, a hit, an overlap), and the frame's tail.
const GROUPS: u8 = 2;
/// The leftover latent pass (`StageEntry::tick` and the Delay links run there).
const LATENT: u8 = 3;
/// The timer pass (a timer's event).
const TIMER: u8 = 4;
/// No owner.
const NONE: i32 = -0x7fff_ffff;

/// The owner numbers of the moving platforms' own latent actions (BasePlatformBP's Delay and MoveComponentTo, whose
/// callback target is the platform actor): `PLATFORM_OWNER` - its index. A stage numbers its own objects (placed
/// actors, level scripts) above this range.
pub const PLATFORM_OWNER: i32 = -0x10000;

pub fn is_platform_owner(o: i32) -> bool {
    o <= PLATFORM_OWNER && o > PLATFORM_OWNER - 0x10000
}

// BasePlatformBP's latent nodes (ExecuteUbergraph_BasePlatformBP): the BeginPlay Delay @196 resuming @251, Move
// Platform's MoveComponentTo @1348 resuming @312.
const DELAY_NODE: i32 = 196;
const DELAY_LINK: i32 = 251;
const MOVE_LINK: i32 = 312;

/// `RuntimeSolid`: a solid a sim object spawned mid-match (a tutorial's walls and
/// platforms), with its own [`StageSolid`] state (no stage data id: -1) and its box. It is no part of the stage's
/// state: a World with no stage state keeps none.
#[derive(State, Debug, Default)]
pub struct RuntimeSolid {
    /// The spec it was made from (`RuntimeSolidSpec`), by the spawner's own numbering.
    pub spec: i32,
    /// `spec.component`: its collision component's name (a trace's hit component).
    pub component: crate::names::Name,
    /// `box`: a Floor box (its place: [`RuntimeSolid::ground`]) or a pass-through platform (in `Arena2d`'s).
    pub solid: RuntimeBox,
    /// `state`: its move, collision switch and mark.
    pub state: StageSolid,
    /// The Floor box's flags (`FLOOR_FLAGS`; 0 for a platform).
    pub flags: i32,
    pub destroyed: bool,
    /// The place at the pass's start (`passStart`).
    pub x0: Real,
    pub y0: Real,
    pub l0: Real,
    pub r0: Real,
    pub t0: Real,
    pub b0: Real,
}

/// `RuntimeSolid.box`.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum RuntimeBox {
    #[default]
    None,
    /// A Floor box (`RuntimeBox extends SolidBox`), its place and state the solid's own.
    Floor,
    /// A pass-through platform, by its index in `Arena2d::stage_platforms`.
    Platform(u32),
}

/// `RuntimeSolidSpec`: what a run-time solid is made from.
#[derive(Clone, Debug)]
pub struct RuntimeSolidSpec {
    /// The actor class (TutorialWall_C ...), for the drawing side and the logs.
    pub cls: String,
    /// Floor (true) or Platform.
    pub floor: bool,
    /// The collision box: its world centre (UE cm), the scaled half extent and the world rotation (pitch, yaw, roll in
    /// degrees).
    pub center: [Real; 3],
    pub half_extent: [Real; 3],
    pub rot: Option<[Real; 3]>,
    /// The collision component's name (a trace's hit component).
    pub component: crate::names::Name,
    /// A Floor box's flags where they are not the class's (`FLOOR_FLAGS`): a solid of the port's own that
    /// items pass. `None` in every solid of the build.
    pub flags: Option<i32>,
}

/// A piece a stage's code moves: a solid of `StageState::solids` or a moving pass-through platform (by its index in
/// `Arena2d::stage_platforms`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StagePiece {
    Solid(u32),
    Platform(u32),
}

fn entry(w: &World) -> Option<StageEntry> {
    w.registry.stages.get(&w.data.stage.name).copied()
}

fn run(w: &World) -> &StageState {
    match &w.stage_run {
        Some(r) => r,
        None => panic!("stage timer call on a World with no stage state ({})", w.data.stage.name),
    }
}

fn run_mut(w: &mut World) -> &mut StageState {
    match w.stage_run {
        Some(ref mut r) => r,
        None => panic!("stage timer call on a World with no stage state"),
    }
}

fn v3(v: &[f64]) -> [Real; 3] {
    [0, 1, 2].map(|i| crate::math::narrow(v.get(i).copied().unwrap_or(0.0)))
}

/// A solid's shape as `section` reads it.
pub(crate) fn shape_of(d: &StageSolidData) -> SolidShape {
    let (c, e) = (v3(&d.center), v3(&d.half_extent));
    match d.kind {
        SolidKind::Box => SolidShape::Box { c: c.map(f64::from), e: e.map(f64::from) },
        SolidKind::Obox => SolidShape::OBox {
            c: c.map(f64::from),
            e: e.map(f64::from),
            rot: v3(d.rot.as_deref().unwrap_or(&[])).map(f64::from),
        },
        _ => SolidShape::Points {
            c: c.map(f64::from),
            e: e.map(f64::from),
            points: d.points.clone().unwrap_or_default(),
        },
    }
}

/// The stages' entries (`STAGE_HOOKS`, `STAGE_ACTORS`: `crate::stages`). The item classes' entries go in with them
/// (`actors::item::register`), then those of the items a character's code drops (`crate::items`).
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    crate::stages::register(r)?;
    crate::actors::item::register(r)?;
    crate::items::register(r)?;
    crate::modes::tutorial::advanced::meeseeks::register(r)
}

// ---- the World's stage phases

/// `stageMeshTicks`, TG_PrePhysics ahead of the spawned actors'.
pub fn mesh_ticks(w: &mut World) {
    if w.stage_run.is_some() {
        crate::world::stage_actors::stage_mesh_ticks(w);
    }
}

/// `stagePreTick`.
pub fn pre_tick(w: &mut World) {
    if w.stage_run.is_some() {
        stage_pre_tick(w);
    }
}

/// `stageSolidsMoved` and `runtimeSolidsMoved`, after the fighters' CMCs.
pub fn solids_moved(w: &mut World) {
    if let Some(r) = w.stage_run.as_mut() {
        stage_solids_moved(r);
    }
    runtime_solids_moved(w);
}

/// `stageTimers`.
pub fn timers(w: &mut World) {
    if w.stage_run.is_some() {
        stage_timers(w);
    }
}

// ---- the solids' hits

/// `stageSolidHit`: a blocking hit of f's movement on a solid (its box or polygon: the
/// solid's index in `StageState::solids`; `None`, the stage block or a spawned box, is passed by). (ox, oy): the way
/// out of the solid at the contact, the fighter's own ImpactNormal (sim space); the actor's event gets it reversed
/// (`StageActorEntry::solid_hit`). A dead actor's (destroyed: its components gone) is not called.
pub fn stage_solid_hit(w: &mut World, f: FighterId, solid: Option<u32>, ox: Real, oy: Real) {
    stage_solid_hit_by(w, crate::actors::ActorRef::Fighter(f), solid, ox, oy);
}

/// [`stage_solid_hit`] of any mover: a fighter's movement, or a spawned actor's move (a projectile's, an item's) whose
/// blocking hit met the solid.
pub fn stage_solid_hit_by(w: &mut World, by: crate::actors::ActorRef, solid: Option<u32>, ox: Real, oy: Real) {
    let Some(i) = solid else { return };
    let Some(r) = w.stage_run.as_ref() else { return };
    let Some(s) = r.solids.get(i as usize) else { return };
    if !s.enabled {
        return;
    }
    let Some(o) = s.hit_owner else { return };
    let a = &r.actors[o as usize];
    if a.dead {
        return;
    }
    let id = u32::try_from(s.id).expect("a stage solid's id");
    let hook = w.registry.stage_actors.get(a.cls.name()).and_then(|k| k.solid_hit);
    if let Some(h) = hook {
        h(w, o, id, by, -ox, -oy);
    }
}

// ---- the stage's run

/// `makeStageRun`: at the World's construction, ahead of its first reset: the stage's state in
/// `World::stage_run`, left `None` when it has none.
/// Unverified: the World holds the state from the placed actors' creates on, so a
/// create can start its Delays.
pub fn make_stage_run(w: &mut World) {
    let sd = &w.data.stage;
    let h = entry(w);
    if h.is_none() {
        warn_text(w, &format!("stage not entered: {} (sim/stages: no stage sim)", sd.name));
    }
    let placed_n = sd.placed.as_ref().map_or(0, Vec::len);
    let moving = sd.platforms.as_ref().is_some_and(|p| p.iter().any(|p| p.moving.is_some()));
    let hooked = h.is_some_and(|h| {
        h.create.is_some()
            || h.tickers.is_some()
            || h.pre_tick.is_some()
            || h.tick.is_some()
            || h.timer.is_some()
            || h.ring_out.is_some()
            || h.reset.is_some()
    });
    if !hooked && sd.solids.as_ref().is_none_or(Vec::is_empty) && placed_n == 0 && !moving {
        return;
    }
    let solids: Vec<StageSolid> = sd.solids.iter().flatten().map(make_solid).collect();
    let mut st = StageState { solids, ..StageState::default() };
    st.boxes = solid_boxes(w);
    st.phase = IDLE;
    w.stage_run = Some(st);
    let data = std::sync::Arc::clone(&w.data);
    for (i, d) in data.stage.placed.iter().flatten().enumerate() {
        let k = w.registry.stage_actors.get(&d.cls).copied();
        let Some((k, create)) = k.and_then(|k| k.create.map(|c| (k, c))) else {
            warn_text(w, &format!("stage actor not implemented ({} {})", d.cls, d.name));
            continue;
        };
        let loc = v3(&d.loc);
        let slot = idx(run(w).actors.len());
        let a = StageActor {
            cls: k.class,
            index: idx(i),
            team: -1,
            dead: false,
            x: 0.0 - loc[1],
            y: loc[2],
            own: StageActorOwn::None,
        };
        run_mut(w).actors.push(a);
        create(w, slot);
        // its solids' OnComponentHit (`StageActorEntry::solid_hit`; solid hits)
        if k.solid_hit.is_some() {
            let r = run_mut(w);
            for &id in d.solids.iter().flatten() {
                if let Some(s) = usize::try_from(id).ok().and_then(|id| r.solids.get_mut(id)) {
                    s.hit_owner = Some(slot);
                }
            }
        }
    }
    run_mut(w).phase = IDLE;
    // the moving platforms' state and their BeginPlay Delay(2.0) (BasePlatformBP @656: a level actor's BeginPlay,
    // ahead of the level script's). Unverified: their order against the placed actors' and the level script's
    // BeginPlay is not read.
    if moving {
        make_moving_platforms(w);
    }
    if let Some(c) = h.and_then(|h| h.create) {
        c(w);
    }
}

fn idx(i: usize) -> u32 {
    u32::try_from(i).expect("a list index")
}

/// `nextStageActorIndex`: the index a run-time actor of `StageState::actors` takes: past every placed actor's and
/// every earlier one's, so that its hit-once key (STAGE_HIT_KEY - index) is its own.
pub fn next_stage_actor_index(w: &World) -> u32 {
    let mut n = idx(w.data.stage.placed.as_ref().map_or(0, Vec::len));
    for a in w.stage_run.iter().flat_map(|r| r.actors.iter()) {
        if a.index >= n {
            n = a.index + 1;
        }
    }
    n
}

/// `spawnStageActor`: an actor a mode's graph spawns at run time with a placed actor's hit path (a
/// `StageActorEntry` with no create): it joins `StageState::actors` behind the placed ones (its index from
/// `next_stage_actor_index`) and stays there once dead, as a placed one does. A World with no stage state gets an
/// empty one here (no hook, no solid, no timer: its stage calls then run over the empty lists).
pub fn spawn_stage_actor(w: &mut World, a: StageActor) {
    let name = a.cls.name();
    assert!(w.registry.stage_actors.get(name).is_some(), "spawnStageActor: {name} is not entered");
    assert!(a.index == next_stage_actor_index(w), "spawnStageActor: {name} index {} is not the next one", a.index);
    w.stage_run.get_or_insert_with(StageState::default).actors.push(a);
}

/// `stagePreTick`: TG_PrePhysics, ahead of the fighters' controllers: a level actor registers
/// its tick function at the level load, before the pawns, and the tick queue keeps registration order. Per ticking
/// actor (`StageEntry::tickers`, in their order): its ReceiveTick (`pre_tick`), then its own latent pass (AActor::Tick:
/// ProcessLatentActions after ReceiveTick): its due Delays. From here on the frame is in the tick groups.
/// Unverified: the place against the blast box shrink's timeline (also TG_PrePhysics) and the Morty ships' ticks
/// (ahead of the fighters) is not read; it runs behind both, after the fighters' frame-boundary timers.
pub fn stage_pre_tick(w: &mut World) {
    let h = entry(w);
    run_mut(w).marked.clear();
    let tickers: Option<Vec<i32>> = h.and_then(|h| h.tickers).map(|t| t(w).to_vec());
    if let Some(tickers) = tickers {
        for o in tickers {
            run_mut(w).phase = PRE;
            if let Some(p) = h.and_then(|h| h.pre_tick) {
                p(w, o);
            }
            let r = run_mut(w);
            r.phase = GROUPS;
            // ProcessLatentActions(this): an entry (any action of the object) is ticked and marked processed
            if r.timers.iter().any(|e| e.delay && e.owner == o) {
                r.marked.push(o);
                due_delays(w, h, o);
            }
        }
    }
    run_mut(w).phase = GROUPS;
}

/// `stageLatent`: the leftover latent pass (`StageEntry::tick`): the pieces' MoveComponentTo
/// actions and their links (`stage_moves`), then the due Delays of every object not processed this frame, in start
/// order, then the entry's tick.
/// Deviation: the build walks object by object (each one's moves and Delays updated, then that object's links); the
/// port updates every move, runs their links, then the Delays. Deviation: the build walks the objects in the slot
/// order of their entries (a history of freed slots the data does not give); the port takes the order the Delays
/// were started in. A Delay of a ticking actor that is due here is one its tick could not count (one started in the
/// tick groups after that tick, its object then with no entry).
pub fn stage_latent(w: &mut World) {
    let h = entry(w);
    run_mut(w).phase = LATENT;
    // an object with a running MoveComponentTo has an entry: processed here (a Delay it starts later in the frame
    // counts from the next one, `delay_at`)
    move_owners(w);
    let r = run_mut(w);
    let owners: Vec<i32> = r.timers.iter().filter(|e| e.delay).map(|e| e.owner).collect();
    for o in owners {
        mark(r, o);
    }
    stage_moves(w, h);
    if !run(w).timers.is_empty() {
        due_delays(w, h, NONE);
    }
    if let Some(t) = h.and_then(|h| h.tick) {
        t(w);
    }
}

fn mark(r: &mut StageState, o: i32) {
    if !r.marked.contains(&o) {
        r.marked.push(o);
    }
}

/// `stageTimers`: the timer pass: the due K2_SetTimer entries. After it the frame's tail (the
/// ring-out check, the items' and hits' leftovers: in the build those run inside the tick groups) counts as the tick
/// groups.
pub fn stage_timers(w: &mut World) {
    run_mut(w).phase = TIMER;
    if !run(w).timers.is_empty() {
        let h = entry(w);
        due_timers(w, h);
    }
    run_mut(w).phase = GROUPS;
}

fn fire(w: &mut World, h: Option<StageEntry>, ev: i32, owner: i32) {
    if let Some(t) = h.and_then(|h| h.timer) {
        t(w, ev, owner);
    }
}

/// TickLatentActionForObject: the due actions of one object (`owner`; `NONE`: every object) in list order (their
/// start order; unverified: the build's slot reuse can swap two that started after a freed slot), all removed before
/// any link runs.
fn due_delays(w: &mut World, h: Option<StageEntry>, owner: i32) {
    let t = w.flow.tick;
    let hit = |e: &StageTimer| e.delay && e.at <= t && (owner == NONE || e.owner == owner);
    let r = run_mut(w);
    if !r.timers.iter().any(hit) {
        return;
    }
    let due: Vec<StageTimer> = r.timers.iter().copied().filter(hit).collect();
    r.timers.retain(|e| !hit(e));
    for e in due {
        // a moving platform's own Delay (BasePlatformBP BeginPlay @196 -> @251): its class's, no stage entry's
        if is_platform_owner(e.owner) {
            platform_delayed(w, e.owner, e.ev);
        } else {
            fire(w, h, e.ev, e.owner);
        }
    }
}

/// FTimerManager::Tick: the due timers by expire time (from x dt + n x rate; Unverified: equal times in the order
/// they were set, the heap's order of equal keys is not read). A one-shot timer is gone before its event runs
/// (setting its key again in the event starts a new one); a loop runs each due pulse and is re-armed after them, if
/// its event left it set. A timer cleared or replaced by an earlier event of the pass does not fire. A timer is found
/// again by its value: a new one of the same key has another `from` or `at`.
fn due_timers(w: &mut World, h: Option<StageEntry>) {
    let t = w.flow.tick;
    let r = run(w);
    if !r.timers.iter().any(|e| !e.delay && e.at <= t) {
        return;
    }
    let exp = |e: &StageTimer| crate::consts::clock(crate::real(e.from)) + f64::from(e.n) * f64::from(e.rate);
    let mut due: Vec<StageTimer> = r.timers.iter().copied().filter(|e| !e.delay && e.at <= t).collect();
    due.sort_by(|a, b| exp(a).total_cmp(&exp(b)));
    let has = |w: &World, e: &StageTimer| run(w).timers.iter().position(|x| x == e);
    for e in due {
        let Some(k) = has(w, &e) else { continue };
        if !e.looping {
            run_mut(w).timers.remove(k);
            fire(w, h, e.ev, e.owner);
            continue;
        }
        let mut n = e.n;
        while pulse_at(e.from, n, e.rate) <= t && has(w, &e).is_some() {
            fire(w, h, e.ev, e.owner);
            n += 1;
        }
        // (the entry is written back even when an event cleared it: a cleared one is out of the list)
        if let Some(k) = has(w, &e) {
            let x = &mut run_mut(w).timers[k];
            x.n = n;
            x.at = pulse_at(e.from, n, e.rate);
        }
    }
}

/// The world tick of a timer's pulse n: the first timer tick after `from` whose counted deltas exceed n x rate
/// (strict; the manager's InternalTime is a double sum of DeltaSeconds). Pulse 1 is `timer_ticks`.
#[allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "the negated test is kept: a NaN rate loops as it does there (native.md Numerics)"
)]
fn pulse_at(from: i32, n: i32, rate: Real) -> i32 {
    // (doubles: the expire time n x rate and the tick count, as the manager's clock)
    let x: crate::Clock = f64::from(n) * f64::from(rate);
    let d = f64::from(DELTA_SECONDS);
    let mut k = crate::math::dbl::max(1.0, (x / d).floor());
    while !(k * d > x) {
        k += 1.0;
    }
    while k > 1.0 && (k - 1.0) * d > x {
        k -= 1.0;
    }
    from + crate::math::dbl::to_int32(k)
}

/// The world tick a Delay of `frames` updates (`latent_frames`) of `owner` started now fires at. The build's rule: an
/// action is first updated by its object's latent pass of this frame when that pass is still to come, else in the
/// next frame. Its object's pass is past when the object was processed this frame with an entry (marked: a ticking
/// actor whose tick found an action, or any object at the leftover pass); an object with no entry gets a new,
/// unmarked one, which the leftover pass counts this frame. So:
/// - IDLE (create = BeginPlay at the load, before tick 1; a Lab reset): from the next step;
/// - PRE (a ticking actor's ReceiveTick) and GROUPS (the tick groups, the frame's tail): this frame unless marked;
/// - LATENT (the leftover pass's links and the stage's tick hook): from the next frame. Deviation: an object with no
///   entry that gets one during the walk is updated in the same walk when its slot lies after the current one; the
///   port takes the next frame;
/// - TIMER (the timer pass, after the leftover pass): from the next frame.
///
/// In the frame's tail (after the timer pass) a Delay counted "this frame" with one update (Delay(0)) fires in the next
/// frame's leftover pass, one late: the port's leftover pass of the frame is past there (a deviation).
fn delay_at(w: &World, frames: Real, owner: i32) -> i32 {
    let r = run(w);
    let now = (r.phase == PRE || r.phase == GROUPS) && !r.marked.contains(&owner);
    w.flow.tick + to_int32(frames) - i32::from(now)
}

/// `stageDelay`: Delay(s) at node `key` of the object `owner` (execDelay 142f9baa0: a call while that object's action
/// of that node exists starts nothing; latent actions are per object and UUID). `ev`: what the stage's timer hook gets
/// when it fires. True: started.
pub fn stage_delay(w: &mut World, owner: i32, key: i32, s: Real, ev: i32) -> bool {
    if run(w).timers.iter().any(|e| e.delay && e.owner == owner && e.key == key) {
        return false;
    }
    let at = delay_at(w, crate::fighter::common::latent_frames(s), owner);
    let e = StageTimer { at, ev, owner, key, delay: true, looping: false, from: 0, rate: 0.0, n: 0 };
    run_mut(w).timers.push(e);
    true
}

/// `stageRetriggerDelay`: RetriggerableDelay(s) at node `key` of `owner` (execRetriggerableDelay 142fa6110): a running
/// action counts its TimeRemaining again from s (its place in the list kept); none: a new one.
pub fn stage_retrigger_delay(w: &mut World, owner: i32, key: i32, s: Real, ev: i32) {
    let Some(k) = run(w).timers.iter().position(|x| x.delay && x.owner == owner && x.key == key) else {
        stage_delay(w, owner, key, s, ev);
        return;
    };
    let at = delay_at(w, crate::fighter::common::latent_frames(s), owner);
    let e = &mut run_mut(w).timers[k];
    e.at = at;
    e.ev = ev;
}

/// `stageSetTimer`: K2_SetTimer / K2_SetTimerDelegate(key, s, loop) of `owner` (a handle per object and function): a
/// running timer of the key is cleared and a new one started; a Rate <= 0 only clears it (InternalSetTimer
/// 142e3de10). Set inside a step it fires `timer_ticks` timer ticks after this frame's (pending until this frame's
/// timer tick, which does not count the frame; set in the timer pass, armed at its end; set after it, active at once
/// with this frame's delta counted: the same tick); set outside a step (create) the first step's.
pub fn stage_set_timer(w: &mut World, owner: i32, key: i32, s: Real, looping: bool, ev: i32) {
    stage_clear_timer(w, owner, key);
    let rate = s;
    if rate.is_nan() || rate <= 0.0 {
        return;
    }
    let from = w.flow.tick + i32::from(run(w).phase == IDLE);
    let at = from + to_int32(crate::combat::hit::timer_ticks(Some(rate)));
    run_mut(w).timers.push(StageTimer { at, ev, owner, key, delay: false, looping, from, rate, n: 1 });
}

/// `stageClearTimer`: K2_ClearTimer(Handle) of the key of `owner`: the timer does not fire again (a Delay is not a
/// timer: left alone).
pub fn stage_clear_timer(w: &mut World, owner: i32, key: i32) {
    let r = run_mut(w);
    if let Some(i) = r.timers.iter().position(|e| !e.delay && e.owner == owner && e.key == key) {
        r.timers.remove(i);
    }
}

/// `stageTimerActive`: whether the key's timer of `owner` is set (K2_IsTimerActive(Handle): pending or active).
pub fn stage_timer_active(w: &World, owner: i32, key: i32) -> bool {
    run(w).timers.iter().any(|e| !e.delay && e.owner == owner && e.key == key)
}

/// `stageRandomRange`: KismetMathLibrary RandomFloatInRange(min, max) of a stage graph: Min + (Max - Min) x FRand, one
/// draw of the world's stream (as the spawned actors' draws), a float32 as the Blueprint pin holds it.
/// Deviation: the build's FRand stream and the order of its draws across the frame are not modelled: the port's waits
/// are deterministic per seed, not the build's values.
pub fn stage_random_range(w: &mut World, min: Real, max: Real) -> Real {
    let a = min;
    let r = w.random_float();
    a + ((max - a) * r)
}

/// `stageItemMoved`: a free item's move (`StageEntry::item_moved`), in the tick groups.
pub fn stage_item_moved(w: &mut World, it: ItemId) {
    if let Some(m) = entry(w).and_then(|h| h.item_moved) {
        m(w, it);
    }
}

/// `stageRingOut`.
pub fn stage_ring_out(w: &mut World, f: FighterId) {
    if let Some(m) = entry(w).and_then(|h| h.ring_out) {
        m(w, f);
    }
}

/// `stageReset`: World.reset, at its end: the placed actors' kinds, then the entry.
pub fn stage_reset(w: &mut World) {
    let p = run(w).phase;
    run_mut(w).phase = IDLE;
    let n = run(w).actors.len();
    for i in 0..n {
        let cls = run(w).actors[i].cls;
        if let Some(r) = w.registry.stage_actors.get(cls.name()).and_then(|k| k.reset) {
            r(w, idx(i));
        }
    }
    if let Some(r) = entry(w).and_then(|h| h.reset) {
        r(w);
    }
    run_mut(w).phase = p;
}

// ---- the movers

/// `makeSolid`: at `make_stage_run`, the move state of a solid.
fn make_solid(d: &StageSolidData) -> StageSolid {
    StageSolid {
        id: d.id,
        enabled: d.enabled != Some(false),
        fresh: false,
        m: StageMoved { ev: -1, ..StageMoved::default() },
        hit_owner: None,
        is_static: false,
    }
}

/// `makeMovingPlatforms` (`make_stage_run`, at the load: `IDLE`): their state, and BasePlatformBP BeginPlay @656 ->
/// @15: with Moving (authority; every peer here), Delay(2.0) @196, whose link @251 is Move Platform(ActorLocation,
/// CurrentCurve) (`platform_delayed`). No match-start or player gate precedes it.
fn make_moving_platforms(w: &mut World) {
    let data = std::sync::Arc::clone(&w.data);
    for (i, d) in data.stage.platforms.iter().flatten().enumerate() {
        let Some(mv) = &d.moving else { continue };
        if mv.curves.is_empty() {
            warn_text(w, &format!("moving platform with no PlatformCurves: {}", d.name));
            continue;
        }
        let p = &mut w.stage.stage_platforms[i];
        p.moving = Some(true);
        let c = v3(&d.center);
        let owner = PLATFORM_OWNER - i32::try_from(i).expect("a platform index");
        let r = PlatformRun {
            index: idx(i),
            l0: p.left,
            r0: p.right,
            t0: p.top,
            b0: p.bottom,
            cx: -c[1],
            cy: c[2],
            curve: 0,
            last_move_z: 0.0,
            m: StageMoved { owner, ev: -1, ..StageMoved::default() },
        };
        run_mut(w).platforms.push(r);
        stage_delay(w, owner, DELAY_NODE, 2.0, DELAY_LINK);
    }
}

fn platform_of(r: &mut StageState, i: u32) -> Option<&mut PlatformRun> {
    r.platforms.iter_mut().find(|p| p.index == i)
}

fn owner_platform(owner: i32) -> u32 {
    u32::try_from(PLATFORM_OWNER - owner).expect("a platform owner")
}

/// `platformDelayed`: a due Delay of a moving platform (`due_delays`): BeginPlay's, @251 Move Platform from where it
/// is.
fn platform_delayed(w: &mut World, owner: i32, ev: i32) {
    let i = owner_platform(owner);
    if ev != DELAY_LINK || platform_of(run_mut(w), i).is_none() {
        warn_text(w, &format!("moving platform: a Delay link it does not have ({owner}, {ev})"));
        return;
    }
    base_platform_start(w, i);
}

/// `moveOwners`: the objects with a running MoveComponentTo (`stage_latent`: processed with an entry this frame).
fn move_owners(w: &mut World) {
    let r = run_mut(w);
    let owners: Vec<i32> = r
        .solids
        .iter()
        .map(|s| &s.m)
        .chain(r.platforms.iter().map(|p| &p.m))
        .filter(|m| m.act.is_some())
        .map(|m| m.owner)
        .collect();
    for o in owners {
        mark(r, o);
    }
}

fn moved_of(r: &mut StageState, p: StagePiece) -> Option<&mut StageMoved> {
    match p {
        StagePiece::Solid(i) => r.solids.get_mut(i as usize).map(|s| &mut s.m),
        StagePiece::Platform(i) => platform_of(r, i).map(|p| &mut p.m),
    }
}

/// `stageMoveTo`: UKismetSystemLibrary::MoveComponentTo on the piece (its root: every component of the actor moves
/// with it), the target given as the piece's offset from its data place (a caller's world target less the root's
/// level location), action 0 Move / 1 Stop / 2 Return (`ComponentMove::call`). `owner`: the calling object (the
/// action's callback target); `ev`: the node's link, run when the action ends (`StageEntry::timer(w, ev, owner)`; a
/// moving platform's: its class's).
/// Deviation: the action updates in the leftover pass whatever its owner; an owner that ticks would update it in its
/// own tick (AActor::Tick's ProcessLatentActions). No decoded mover ticks.
/// Deviation: one action per piece; the build keys an action by its node (callback target and UUID), so a second node
/// moving the same component while the first runs would make a second action. A call from another link than the
/// running action's is warned.
#[allow(clippy::too_many_arguments, reason = "the node's pins")]
pub fn stage_move_to(
    w: &mut World,
    p: StagePiece,
    tdx: Real,
    tdy: Real,
    over_time: Real,
    owner: i32,
    ev: i32,
    action: i32,
    ease_out: bool,
    ease_in: bool,
) {
    let Some(o) = moved_of(run_mut(w), p) else {
        warn_text(w, "MoveComponentTo on a stage platform that is not moving");
        return;
    };
    if o.act.is_some() && (o.ev != ev || o.owner != owner) {
        warn_text(w, "MoveComponentTo: a second node moves a stage piece while one runs (one action kept)");
    }
    let Some(o) = moved_of(run_mut(w), p) else { return };
    o.act = ComponentMove::call(o.act, o.dx, o.dy, tdx, tdy, ease_out, ease_in, over_time, action);
    if o.act.is_some() {
        o.owner = owner;
        o.ev = ev;
    }
}

/// `stageTeleport`: K2_SetWorldLocation(teleport true) on the piece's root (BasePlatformBP Move Platform's first
/// write): no sweep. A fighter based on it follows the move in his next CMC tick, as any base move.
/// Unverified: a teleport sets the PhysX global pose rather than a kinematic target; whether the next step's body
/// velocity reads it is not read (Move Platform's own call writes the place the move just reached, a move of 0).
pub fn stage_teleport(w: &mut World, p: StagePiece, dx: Real, dy: Real) {
    let Some(o) = moved_of(run_mut(w), p) else { return };
    if o.dx == dx && o.dy == dy {
        return;
    }
    o.mx += dx - o.dx;
    o.my += dy - o.dy;
    o.tpx = Some(o.tpx.unwrap_or(0.0) + (dx - o.dx));
    o.tpy = Some(o.tpy.unwrap_or(0.0) + (dy - o.dy));
    o.dx = dx;
    o.dy = dy;
    if let StagePiece::Platform(i) = p {
        place_platform(w, i);
    }
}

/// `stageSolidEnable`: SetCollisionEnabled on a solid's component: QueryAndPhysics (true) or NoCollision. A fighter
/// standing on it when it goes off has no floor in his next CMC tick: PhysWalking's FindFloor misses it,
/// StartFalling with the base still set, so he takes its velocity (the ignored-base path). One inside it when it
/// comes on starts his next move in penetration, which the movement resolves as for a box that rose into him
/// (`SolidBox::fresh`).
pub fn stage_solid_enable(w: &mut World, solid: u32, on: bool) {
    let s = &mut run_mut(w).solids[solid as usize];
    if s.enabled == on {
        return;
    }
    s.enabled = on;
    if on {
        s.fresh = true;
    }
}

/// `stageSolidsMoved`: behind the fighters' CMC ticks: a solid switched on before them (in
/// the last frame's leftover pass, its timer pass or ring-out, or this frame's input events) has been met by every
/// capsule's move; one switched on later in the frame (a pawn tick, a hit) stays fresh for the next frame's.
/// Deviation: one bit for every fighter, cleared behind all the CMCs (the fighters tick their CMCs in one loop here).
pub fn stage_solids_moved(run: &mut StageState) {
    for s in &mut run.solids {
        s.fresh = false;
    }
}

/// `stageMoves`: the leftover latent pass's moves (`stage_latent`, ahead of the due Delays and the stage's tick):
/// every piece's action updated with the world delta, then the links of those that ended: a moving platform's to its
/// class (`base_platform_next`), the others' to the stage's timer hook with the action's owner.
fn stage_moves(w: &mut World, h: Option<StageEntry>) {
    let dt = DELTA_SECONDS;
    let r = run_mut(w);
    let mut links = std::mem::take(&mut r.links);
    for s in &mut r.solids {
        if update(&mut s.m, dt) {
            links.push((s.m.owner, s.m.ev));
        }
    }
    for k in 0..r.platforms.len() {
        let p = &mut run_mut(w).platforms[k];
        if update(&mut p.m, dt) {
            links.push((p.m.owner, p.m.ev));
        }
        let i = p.index;
        place_platform(w, i);
    }
    for &(owner, ev) in &links {
        if ev < 0 {
            continue;
        }
        if is_platform_owner(owner) && ev == MOVE_LINK {
            base_platform_next(w, owner_platform(owner));
        } else {
            fire(w, h, ev, owner);
        }
    }
    links.clear();
    if let Some(r) = w.stage_run.as_mut() {
        r.links = links;
    }
}

/// One piece's pass: true when its action ended (it is gone by then: a call at the link makes a new one).
fn update(o: &mut StageMoved, dt: Real) -> bool {
    // this frame's physics step consumed the last pass's target: the next CMC reads that move's velocity
    o.vx = o.mx / dt;
    o.vy = o.my / dt;
    let (x0, y0) = (o.dx, o.dy);
    if o.tpx.is_some() {
        // this pass's move is the action's alone
        o.tpx = None;
        o.tpy = None;
    }
    let mut done = false;
    if let Some(a) = o.act.as_mut() {
        done = a.update(dt);
        o.dx = a.x;
        o.dy = a.y;
        if done {
            o.act = None;
        }
    }
    o.mx = o.dx - x0;
    o.my = o.dy - y0;
    done
}

/// `placePlatform`: a moving platform's box, move and velocity where its state says (the Platform fields the
/// fighters read).
fn place_platform(w: &mut World, i: u32) {
    let Some(r) = w.stage_run.as_ref().and_then(|r| r.platforms.iter().find(|p| p.index == i)) else { return };
    let (l, rt, t, b, m) =
        (r.l0 + r.m.dx, r.r0 + r.m.dx, r.t0 + r.m.dy, r.b0 + r.m.dy, (r.m.mx, r.m.my, r.m.vx, r.m.vy));
    let p = &mut w.stage.stage_platforms[i as usize];
    p.left = l;
    p.right = rt;
    p.top = t;
    p.bottom = b;
    p.dx = m.0;
    p.dy = m.1;
    p.vx = m.2;
    p.vy = m.3;
}

/// `movePlatform`: BasePlatformBP Move Platform(Start, CurveIndex) (@799-1348, a reliable multicast: every peer runs
/// it): Scene to the world start S (teleport), CurrentCurve = the index, LastMove = (S + Distance) / (Duration x 60)
/// in float32, then MoveComponentTo(Scene, S + Distance, Duration), linear, action Move. sx, sy: S in sim space (x =
/// -UE Y, y = UE Z; the depth X is not modelled). Its link @312 is `base_platform_next` (`stage_moves`).
fn move_platform(w: &mut World, i: u32, sx: Real, sy: Real, curve: i32) {
    let data = std::sync::Arc::clone(&w.data);
    let Some(r) = platform_of(run_mut(w), i) else {
        warn_text(w, "Move Platform on a platform that is not moving");
        return;
    };
    let (cx, cy, owner) = (r.cx, r.cy, r.m.owner);
    let mv = data.stage.platforms.as_ref().and_then(|p| p.get(i as usize)).and_then(|p| p.moving.as_ref());
    let c = &mv.expect("a moving platform's data").curves[usize::try_from(curve).expect("a curve index")];
    let d = v3(&c.d);
    stage_teleport(w, StagePiece::Platform(i), sx - cx, sy - cy);
    let r = platform_of(run_mut(w), i).expect("the platform");
    r.curve = curve;
    r.last_move_z = (sy + d[2]) / (crate::math::narrow(c.duration) * 60.0);
    stage_move_to(
        w,
        StagePiece::Platform(i),
        sx - d[1] - cx,
        sy + d[2] - cy,
        crate::math::narrow(c.duration),
        owner,
        MOVE_LINK,
        0,
        false,
        false,
    );
}

/// `basePlatformNext`: the link of Move Platform's MoveComponentTo (@312): CurrentCurve = (CurrentCurve + 1) %
/// len(PlatformCurves), then (authority) Move Platform again from the actor's location at that moment.
fn base_platform_next(w: &mut World, i: u32) {
    let n = w.data.stage.platforms.as_ref().and_then(|p| p.get(i as usize)).and_then(|p| p.moving.as_ref());
    let n = i32::try_from(n.map_or(0, |m| m.curves.len())).expect("a short list");
    let Some(r) = platform_of(run_mut(w), i) else {
        warn_text(w, "Move Platform on a platform that is not moving");
        return;
    };
    r.curve = (r.curve + 1) % n;
    let (x, y, c) = (r.cx + r.m.dx, r.cy + r.m.dy, r.curve);
    move_platform(w, i, x, y, c);
}

/// `basePlatformStart`: BeginPlay's first call (after its Delay(2.0) @196 -> @251): Move Platform from where it is,
/// with CurrentCurve.
fn base_platform_start(w: &mut World, i: u32) {
    let Some(r) = platform_of(run_mut(w), i) else {
        warn_text(w, "Move Platform on a platform that is not moving");
        return;
    };
    let (x, y, c) = (r.cx + r.m.dx, r.cy + r.m.dy, r.curve);
    move_platform(w, i, x, y, c);
}

/// `solidBoxes`: the solids' boxes the fighters meet. A solid whose section by the play plane is a rectangle on the
/// axes (a box, an obox whose axes lie on the world's) is one; any other section is a polygon of
/// the solid list. A solid that blocks no capsule (a hit box: PhysicsBody alone) is not in the list.
fn solid_boxes(w: &World) -> Vec<SolidBox> {
    let mut out = Vec::new();
    for d in w.data.stage.solids.iter().flatten() {
        if d.flags & (SOLID_FLOOR | SOLID_WALL) == 0 {
            continue; // neither floor nor wall: no capsule meets it
        }
        if d.flags & SOLID_ONE_WAY != 0 {
            warn_text(
                w,
                &format!(
                    "stage solid {}: one-way (object type Platform) solids are not ported for the fighters (a BasePlatformBP child the stage's platforms leave out)",
                    d.name
                ),
            );
            continue;
        }
        let Some(b) = section(&shape_of(d)).and_then(|x| x.box_) else { continue };
        let solid = u32::try_from(d.id).expect("a stage solid's id");
        out.push(SolidBox {
            dead: false,
            solid,
            flags: d.flags,
            l0: crate::math::narrow(b[0]),
            r0: crate::math::narrow(b[1]),
            t0: crate::math::narrow(b[3]),
            b0: crate::math::narrow(b[2]),
        });
    }
    out
}

// ---- the run-time solids

/// `addRuntimeSolid`: SpawnActor of the class: its box in the world from the next query on. False where its section
/// is no rectangle on the axes or misses the play plane (warned). The solid keeps its state itself, out of the stage's
/// (the stage pass neither moves nor lists it: `runtime_solids_pass` moves it).
/// Deviation: the section by the play plane is taken at the spawn; a rotation that does not leave a rectangle on the
/// axes (a tilted box) is refused with a warning, as the stage's polygons are not made at run time.
pub fn add_runtime_solid(w: &mut World, spec: i32, d: &RuntimeSolidSpec) -> bool {
    let rot = d.rot.filter(|r| r.iter().any(|&v| v != 0.0));
    let shape = match rot {
        Some(rot) => {
            SolidShape::OBox { c: d.center.map(f64::from), e: d.half_extent.map(f64::from), rot: rot.map(f64::from) }
        }
        None => SolidShape::Box { c: d.center.map(f64::from), e: d.half_extent.map(f64::from) },
    };
    let Some([l, r, b, t]) = section(&shape).and_then(|x| x.box_) else {
        warn_text(
            w,
            &format!(
                "{}: a run-time solid whose section is no rectangle on the axes (or misses the play plane) is not made",
                d.cls
            ),
        );
        return false;
    };
    // markStaticSolid (a Floor box) or markKinematic (a pass-through platform, over the same state)
    let state = StageSolid {
        id: -1,
        enabled: true,
        fresh: true,
        m: StageMoved { ev: -1, ..StageMoved::default() },
        hit_owner: None,
        is_static: d.floor,
    };
    let solid = if d.floor {
        RuntimeBox::Floor
    } else {
        let k = idx(w.stage.stage_platforms.len());
        let p = StagePlatform {
            stage_index: i32::try_from(k).expect("a platform index"),
            left: crate::math::narrow(l),
            right: crate::math::narrow(r),
            top: crate::math::narrow(t),
            bottom: crate::math::narrow(b),
            ..StagePlatform::default()
        };
        w.stage.stage_platforms.push(p);
        // (`World.stagePlatforms` is one array every fighter holds: the new platform is in each one's list)
        let ids: Vec<_> = w.fighters.ids().collect();
        for f in ids {
            w.fighter_mut(f).platforms.stage_platforms.push(k);
        }
        RuntimeBox::Platform(k)
    };
    let flags = if d.floor { d.flags.unwrap_or(FLOOR_FLAGS) } else { 0 };
    let s = RuntimeSolid {
        spec,
        component: d.component,
        solid,
        state,
        flags,
        destroyed: false,
        x0: 0.0,
        y0: 0.0,
        l0: crate::math::narrow(l),
        r0: crate::math::narrow(r),
        t0: crate::math::narrow(t),
        b0: crate::math::narrow(b),
    };
    w.runtime_solids.get_or_insert_with(Vec::new).push(s);
    true
}

impl RuntimeSolid {
    /// Its box now (`box.left` ...): the spawn place plus its move.
    pub fn ground(&self) -> Ground {
        let m = &self.state.m;
        Ground {
            left: f64::from(self.l0 + m.dx),
            right: f64::from(self.r0 + m.dx),
            top: f64::from(self.t0 + m.dy),
            bottom: f64::from(self.b0 + m.dy),
        }
    }
    /// `x` / `y`: MoveTarget's root place, sim space (the box centre).
    pub fn at(&self) -> (Real, Real) {
        let m = &self.state.m;
        ((self.l0 + self.r0) / 2.0 + m.dx, (self.t0 + self.b0) / 2.0 + m.dy)
    }
}

/// `RuntimeSolid.x` / `y` of the live solid of `spec`.
pub fn runtime_solid_at(w: &World, spec: i32) -> Option<(Real, Real)> {
    w.runtime_solids.as_ref()?.iter().find(|s| s.spec == spec).map(RuntimeSolid::at)
}

/// `RuntimeSolid.place`: a write of the root's place (no sweep): the box follows.
pub fn runtime_solid_place(w: &mut World, spec: i32, x: Real, y: Real) {
    let Some(k) = w.runtime_solids.as_ref().and_then(|l| l.iter().position(|s| s.spec == spec)) else { return };
    let s = &mut w.runtime_solids.as_mut().expect("the list")[k];
    let (cx, cy) = ((s.l0 + s.r0) / 2.0, (s.t0 + s.b0) / 2.0);
    s.state.m.dx = x - cx;
    s.state.m.dy = y - cy;
    runtime_sync(w, k);
}

fn runtime_sync(w: &mut World, k: usize) {
    let s = &w.runtime_solids.as_ref().expect("the list")[k];
    let RuntimeBox::Platform(i) = s.solid else { return };
    let g = s.ground();
    let m = &s.state.m;
    let (mx, my, vx, vy) = (m.mx, m.my, m.vx, m.vy);
    let b = &mut w.stage.stage_platforms[i as usize];
    b.left = crate::math::narrow(g.left);
    b.right = crate::math::narrow(g.right);
    b.top = crate::math::narrow(g.top);
    b.bottom = crate::math::narrow(g.bottom);
    b.dx = mx;
    b.dy = my;
    b.vx = vx;
    b.vy = vy;
}

/// `removeRuntimeSolid`: K2_DestroyActor of the solid's actor: out of the world. A fighter based on it loses the base
/// with no impart (a pending-kill base). A pass-through platform leaves `Arena2d::stage_platforms`, the ones behind it
/// moving up one place.
pub fn remove_runtime_solid(w: &mut World, spec: i32) {
    let Some(list) = w.runtime_solids.as_mut() else { return };
    let Some(k) = list.iter().position(|s| s.spec == spec && !s.destroyed) else { return };
    let s = list.remove(k);
    if list.is_empty() {
        w.runtime_solids = None;
    }
    let RuntimeBox::Platform(i) = s.solid else { return };
    let i = i as usize;
    if i >= w.stage.stage_platforms.len() {
        return;
    }
    w.stage.stage_platforms.remove(i);
    // (the fighters' lists are the same array: the entry goes, the ones behind it move up one place)
    let own = u32::try_from(i).expect("a platform index");
    let ids: Vec<_> = w.fighters.ids().collect();
    for f in ids {
        let l = &mut w.fighter_mut(f).platforms.stage_platforms;
        l.retain(|&j| j != own);
        for j in l.iter_mut().filter(|j| **j > own) {
            *j -= 1;
        }
    }
    for (k, p) in w.stage.stage_platforms.iter_mut().enumerate().skip(i) {
        p.stage_index = i32::try_from(k).expect("a platform index");
    }
    for o in w.runtime_solids.iter_mut().flatten() {
        if let RuntimeBox::Platform(j) = o.solid
            && j as usize > i
        {
            o.solid = RuntimeBox::Platform(j - 1);
        }
    }
}

/// `runtimeFloorBoxes`: the Floor solids (the fighters' platforms of the frame, `World::blockers`, the stage block's
/// readers), in spawn order.
pub fn runtime_floor_boxes(w: &World) -> impl Iterator<Item = &RuntimeSolid> {
    w.runtime_solids.iter().flatten().filter(|s| s.solid == RuntimeBox::Floor)
}

/// `runtimeSolidsPass`: the leftover latent pass that moves them (TutorialGameMode's latent): `run` between the two
/// halves of each solid's bookkeeping: ahead of the moves, the velocity of the last pass's move; after them, this
/// pass's move.
pub fn runtime_solids_pass(w: &mut World, run: impl FnOnce(&mut World)) {
    for s in w.runtime_solids.iter_mut().flatten() {
        let m = &mut s.state.m;
        m.vx = m.mx / DELTA_SECONDS;
        m.vy = m.my / DELTA_SECONDS;
        s.x0 = m.dx;
        s.y0 = m.dy;
    }
    run(w);
    let n = w.runtime_solids.as_ref().map_or(0, Vec::len);
    for k in 0..n {
        let s = &mut w.runtime_solids.as_mut().expect("the list")[k];
        let (x0, y0) = (s.x0, s.y0);
        let m = &mut s.state.m;
        m.mx = m.dx - x0;
        m.my = m.dy - y0;
        runtime_sync(w, k);
    }
}

/// `runtimeSolidsMoved`: behind the fighters' CMC ticks (`stage_solids_moved`): a
/// solid spawned before them has been met by every capsule's move.
pub fn runtime_solids_moved(w: &mut World) {
    for s in w.runtime_solids.iter_mut().flatten() {
        s.state.fresh = false;
    }
}

/// `runtimeBlockers`: `World::blockers`' boxes of the run-time Floor solids that
/// block a query of the kind (the stage solids' rule: the flag of the kind; WorldDynamic has none, a WorldStatic
/// solid is met there as the stage block is, through `static_solids`). `out`: the list so far, `None` while it is
/// `base`.
pub fn runtime_blockers(w: &World, kind: BlockKind, base: &[Ground], out: Option<Vec<Ground>>) -> Option<Vec<Ground>> {
    let bit = match kind {
        BlockKind::Trace => SOLID_TRACE,
        BlockKind::Nav => SOLID_NAV,
        BlockKind::Item => SOLID_ITEM,
        BlockKind::Detect => SOLID_DETECT,
        BlockKind::Dynamic => 0,
    };
    let mut out = out;
    if bit == 0 {
        return out;
    }
    for s in runtime_floor_boxes(w) {
        if s.flags & bit == 0 || !s.state.enabled {
            continue;
        }
        out.get_or_insert_with(|| base.to_vec()).push(s.ground());
    }
    out
}

/// `stageSolidHit` on a stage solid's box (a `StageState::boxes` index): its solid's.
pub fn stage_solid_box_hit(w: &mut World, f: FighterId, b: u32, ox: Real, oy: Real) {
    let Some(s) = w.stage_run.as_ref().and_then(|r| r.boxes.get(b as usize)).map(|b| b.solid) else { return };
    stage_solid_hit(w, f, Some(s), ox, oy);
}

/// `onPlatformFloor`'s test of a stage solid's box: its `objectType` (the data's) is none
/// or Platform.
pub fn solid_object_type_is_platform(w: &World, b: u32) -> bool {
    let Some(s) = w.stage_run.as_ref().and_then(|r| r.boxes.get(b as usize)).map(|b| b.solid) else { return true };
    crate::fighter::traces::solid_object_type(w, s).is_none_or(|t| t == "Platform")
}

/// A run-time solid's collision component's name (`RuntimeSolidSpec.component`: a
/// trace's hit component), by `RuntimeSolid::spec`; none for a solid out of the world.
pub fn runtime_solid_component(w: &World, spec: i32) -> crate::names::Name {
    w.runtime_solids.iter().flatten().find(|s| s.spec == spec).map_or(crate::names::Name::NONE, |s| s.component)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_move_linear_and_stop() {
        let mut a = ComponentMove::call(None, 0.0, 0.0, 10.0, 0.0, false, false, 0.5, 0).expect("a move");
        assert!(!a.update(0.25));
        assert_eq!(a.x, 5.0);
        assert!(a.update(0.25));
        assert_eq!(a.x, 10.0);
        assert!(ComponentMove::call(None, 0.0, 0.0, 1.0, 1.0, false, false, 1.0, 1).is_none());
        let s = ComponentMove::call(Some(a), 3.0, 0.0, 0.0, 0.0, false, false, 0.0, 1).expect("kept");
        assert!(!s.interpolating);
    }

    #[test]
    fn pulse_at_counts_strictly() {
        // pulse 1 of a 1 s timer: the first tick whose counted deltas exceed 1 s
        let k = pulse_at(0, 1, 1.0);
        assert!(crate::consts::clock(crate::real(k)) > 1.0);
        assert!(crate::consts::clock(crate::real(k - 1)) <= 1.0);
        assert_eq!(pulse_at(10, 2, 0.5) - 10, k);
    }

    #[test]
    fn platform_owners() {
        assert!(is_platform_owner(PLATFORM_OWNER));
        assert!(is_platform_owner(PLATFORM_OWNER - 3));
        assert!(!is_platform_owner(0));
        assert_eq!(owner_platform(PLATFORM_OWNER - 3), 3);
    }
}
