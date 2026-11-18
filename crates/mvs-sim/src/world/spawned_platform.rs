//! The spawned platforms: the boxes of SpawnedPlatformParent_C (SpawnedPlatformParent)
//! children and of projectiles. The parent's variables and parts are a [`SpawnedPlatform`] its child's actor holds
//! (`WorldActor::plat`); a projectile's boxes are its `solids`.

use crate::Real;
use crate::State;
use crate::actors::lists::Ship;
use crate::actors::projectile::ProjectileBox;
use crate::actors::{ActorClass, ActorId, ActorRef, Projectile, ProjectileId, WorldActor, WorldActorClass};
use crate::chars::morty::ship;
use crate::fighter::FighterId;
use crate::fighter::body::{Plat, PlatformRef, Span};
use crate::math::{max, min};
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, WorldActorHooks};
use crate::registry::{Registry, RegistryError};
use crate::world::World;
use mvs_collision::Ground;
use mvs_data::value::{Props, Value};

/// `BlockKind`: the query a box blocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockKind {
    /// ProjectileGroundTrace.
    Trace,
    /// PhysicsBody.
    Nav,
    Item,
    /// WorldStaticDetection.
    Detect,
    Dynamic,
}

/// What a box of [`crate::world::World::blockers_marked_into`] belongs to: the marks of a box
/// (`ShipBox.ship`, `PlatformBox.actor`, `SolidBox.stageSolid`), which a `Ground` does not carry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoxMark {
    /// A stage platform or a run-time solid.
    Plain,
    /// A piece of a ship; the ship's owner.
    Ship(crate::fighter::FighterId),
    /// A box of a spawned platform or a projectile.
    Actor(ActorRef),
    /// A stage solid's box; the solid's data id (`StageSolidData.id`).
    Solid(i32),
}

// ---- the call surface of the spawned platforms

/// The query channel of a block kind (`BLOCK_CHANNEL`).
const fn block_channel(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Trace => "ProjectileGroundTrace",
        BlockKind::Nav => "PhysicsBody",
        BlockKind::Item => "Item",
        BlockKind::Detect => "WorldStaticDetection",
        BlockKind::Dynamic => "WorldDynamic",
    }
}

/// A channel's response where a template's ResponseArray leaves it out (`CHANNEL_DEFAULT`: DefaultEngine.ini).
fn channel_default(ch: &str) -> &'static str {
    match ch {
        "PhysicsBody" | "WorldDynamic" => "ECR_Block",
        "Item" => "ECR_Overlap",
        _ => "ECR_Ignore",
    }
}

/// A component template's response to the channel `ch` (its BodyInstance's ResponseArray entry), if it has one.
fn response<'a>(comp: &'a Props, ch: &str) -> Option<&'a str> {
    let list = comp
        .get("BodyInstance")
        .and_then(Value::as_obj)?
        .get("CollisionResponses")
        .and_then(Value::as_obj)?
        .get("ResponseArray")
        .and_then(Value::as_arr)?;
    list.iter()
        .filter_map(Value::as_obj)
        .find(|r| r.get("Channel").and_then(Value::as_str) == Some(ch))
        .and_then(|r| r.get("Response").and_then(Value::as_str))
}

/// `blocks`: whether a box component (its template's values) blocks a query of the
/// kind.
pub fn blocks(comp: &Props, kind: BlockKind) -> bool {
    let ch = block_channel(kind);
    response(comp, ch).unwrap_or_else(|| channel_default(ch)) == "ECR_Block"
}

/// `blocksWallJump`: whether a box component blocks the WallJump trace channel
/// (type 2: the wall slide's BoxTrace, the wall jump's and MoveCommand's line traces): its ResponseArray entry, else
/// the channel's default, Ignore (DefaultEngine.ini DefaultChannelResponses; the same for a Custom profile).
pub fn blocks_wall_jump(comp: Option<&Props>) -> bool {
    comp.is_some_and(|c| response(c, "WallJump").unwrap_or("ECR_Ignore") == "ECR_Block")
}

/// `canPassThrough`: CanPassThrough(TeamIndex) (0-195): for the team of the
/// actor's Comp_Actor_Destructible (`own`), IgnoreAllies || AlliesDropThrough, for another team IgnoreEnemies ||
/// EnemiesDropThrough (`p`: the class defaults). BaseFighter's jump press reads it (ubergraph 76807-77060): pushing
/// down on the ground with a SpawnedPlatformParent as the movement base's owner and CanPassThrough, the press sets
/// isDroppingDown, runs HandlePlatformCollision, clears FastFallTriggered and the crouch, and does not jump.
pub fn can_pass_through(p: &Props, own: Real, team: i32) -> bool {
    if crate::real(team) == own {
        truthy(p, "IgnoreAllies") || truthy(p, "AlliesDropThrough")
    } else {
        truthy(p, "IgnoreEnemies") || truthy(p, "EnemiesDropThrough")
    }
}

/// What `canPassThrough` reads of a SpawnedPlatformParent child gone from its arena (`Actors::gone_plats`): the
/// actor, its `plat.team` and its class data's drop-through answers for an ally and for an enemy, and its parts as
/// they ended (the box a fighter's base still holds; kept so the per-tick trace's record writes it).
#[derive(State, Debug)]
pub struct GonePlat {
    pub id: ActorId,
    pub team: i32,
    pub allies: bool,
    pub enemies: bool,
    pub parts: Vec<PlatformPart>,
}

/// The list pass's keep of `Actors::gone_plats`, ahead of the removal of the dead world actors: the entries a
/// fighter's base still names stay, and each dead child with a SpawnedPlatform a fighter stands on joins them.
pub fn keep_gone_plats(w: &mut World) {
    let based = |w: &World, id: ActorId| {
        w.fighters
            .iter()
            .any(|(_, f)| matches!(f.platforms.base, Some(PlatformRef::Spawned(ActorRef::Actor(a), _)) if a == id))
    };
    let mut gone = std::mem::take(&mut w.actors.gone_plats);
    gone.retain(|g| based(w, g.id));
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        let Some(a) = w.actors.actors.get(x).filter(|a| a.dead) else { continue };
        let Some(pl) = &a.plat else { continue };
        if !based(w, x) {
            continue;
        }
        let p = crate::actors::world_actor::actor_p(w, ActorRef::Actor(x));
        let plain = plain_box(a);
        gone.push(GonePlat {
            id: x,
            team: pl.team,
            allies: !plain && (truthy(p, "IgnoreAllies") || truthy(p, "AlliesDropThrough")),
            enemies: !plain && (truthy(p, "IgnoreEnemies") || truthy(p, "EnemiesDropThrough")),
            parts: pl.parts.clone(),
        });
    }
    w.actors.gone_plats = gone;
}

/// `canStepUpOn`: whether a walking capsule may step up onto a box component: its
/// CanCharacterStepUpOn (UPrimitiveComponent +0x219; CMC::CanStepUp 142923400 asks the component, then the hit
/// actor's CanBeBaseForCharacter). A template that leaves the field out keeps the primitive's default, ECB_Yes (what
/// the serializer omits is the archetype's value; the ACharacter ctor sets its own capsule to No, which the
/// serialized No of BaseFighter's and the sensors' templates repeat). ECB_Owner asks the owner's
/// CanBeBaseForCharacter, which no box of the port's actors uses: warned and taken as No.
/// Unverified: the primitive default Yes and AActor::CanBeBaseForCharacter = true for the boxes' actors are the
/// 4.26 source, not read in the exe.
pub fn can_step_up_on(w: &World, comp: Option<&Props>, who: Name) -> bool {
    let Some(comp) = comp else { return false };
    let v = comp.get("CanCharacterStepUpOn").and_then(Value::as_str).unwrap_or("ECB_Yes");
    if v == "ECB_Yes" {
        return true;
    }
    if v != "ECB_No" {
        crate::fighter::common::warn_parts(
            w,
            &["CanCharacterStepUpOn ", v, " not ported (taken as No): ", w.names.str(who)],
        );
    }
    false
}

/// `truthy`: a JavaScript truthy read of the field `k`.
fn truthy(p: &Props, k: &str) -> bool {
    match p.get(k) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

// ---- the parent's own graph
//
// The class has two solid boxes on its root: Collision (the platform the fighters stand on and are stopped by) and
// WallbounceCollision (a wall for the fighters in hitstun). Each is a PlatformPart: the fighters' movement meets it
// through `PlatformRef::Spawned` (the actor's parts of the frame beside the ships), the other queries through
// `World::blockers`.
// UserConstructionScript @0-123: with DelayCollisionEnabled Collision.SetCollisionEnabled(NoCollision) @30; without
// UseWallbounceCollision WallbounceCollision.SetCollisionEnabled(NoCollision) @84.
// ReceiveBeginPlay @54, after SpawnedActorParent's: the game state's BPI_AddIgnorablePlatform(self) @169 (the list
// BaseFighter's HandlePlatformCollision walks); with DelayCollisionEnabled Delay(0.2) @217 ->
// Collision.SetCollisionEnabled(QueryAndPhysics) @15; with UseWallbounceCollision BPI_AddWallbounceCollision(self)
// @382 (the list Comp_Pawn_HitstunHandler's tick walks) and WallbounceCollision.IgnoreActorWhenMoving(GetOwner())
// @440 (the box's own moves: an attached box is never swept, so nothing in the port).
// ReceiveEndPlay @483: BPI_RemoveIgnorablePlatform @603 and BPI_RemoveWallbounceCollision @748 (`platform_end`).
// BPI_IsMovingPlatform answers false and BPI_GetWallBounceMultiplier 1.0 (BaseFighter's capsule hit @128863-129102
// passes it to BFBounce: the bounce off such a box is the stage's).
// A child with its own BPI_IgnorePlatformCollision or BPI_IgnoreWallbounceCollision enters that function under its
// class (`WorldActorHooks`); a class that has a part and no entry for the call is reported, not given these.
// Deviation: a part is an axis-aligned box in the play plane, placed by its child class once a tick (where the child's
// actor tick stands in the World's step: after the fighters' moves of the frame, so a fighter meets a box that follows
// another fighter where that one stood at the end of the frame before); the capsule's move-ignore list is written
// where the port makes the two calls (see `ignore_platform_collision`, `wallbounce_tick`), and a part that turned
// solid inside a capsule is resolved as the movement's collidePlatforms does for a ship's piece.

/// `PlatformPart` (its `PlatformBox` part): a box of a SpawnedPlatformParent child (the class's Collision or
/// WallbounceCollision, or a child's own box) the fighters' movement meets as a platform.
#[derive(State, Debug)]
pub struct PlatformPart {
    /// The component of the class data the box is (the traces name it; its template).
    pub comp: Name,
    /// WallbounceCollision (else a platform box).
    pub wall: bool,
    /// The box takes BaseFighter HandlePlatformCollision's call (the class's BPI_IgnorePlatformCollision): false for
    /// a child's box whose class override skips it (C018_Platform's Mid boxes, the Steven barrier's WallBox).
    pub takes_ignore: bool,
    pub dead: bool,
    /// Collision enabled.
    pub on: bool,
    /// The box's centre and sides.
    pub x: Real,
    pub y: Real,
    pub left: Real,
    pub right: Real,
    pub top: Real,
    pub bottom: Real,
    /// Its move since the last placing.
    pub dx: Real,
    pub dy: Real,
    /// The movement base's velocity (MovementBaseUtility::GetMovementBaseVelocity: the component's, else its
    /// actor's): taken as 0 for a box that only follows its attach parent.
    // Unverified: stock 4.26 (AActor::GetVelocity is the root's ComponentVelocity), not read in the exe;
    // WW_ShieldActor's class has EnableProjectileMovement true, and a movement component writes its
    // UpdatedComponent's velocity.
    pub vx: Real,
    pub vy: Real,
    /// The fighters whose capsule ignores the box when moving (IgnoreComponentWhenMoving), a bit per player id.
    pub ignored: i32,
    /// The minions' ids (from 100) it ignores, in the order set, `None` while empty.
    pub ignored_minions: Option<Vec<i32>>,
    pub placed: bool,
    /// The half extents.
    pub hx: Real,
    pub hy: Real,
    /// It blocks the ProjectileGroundTrace, PhysicsBody and Item queries (`BlockKind`; detect and dynamic are read
    /// from the template when asked).
    pub trace: bool,
    pub nav: bool,
    pub item: bool,
}

impl PlatformPart {
    /// A part of component `comp` (its template `c`) with the half extents (hx, hy), collision off and unplaced.
    pub fn new(comp: Name, wall: bool, hx: Real, hy: Real, c: &Props) -> PlatformPart {
        PlatformPart {
            comp,
            wall,
            takes_ignore: true,
            dead: false,
            on: false,
            x: 0.0,
            y: 0.0,
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
            dx: 0.0,
            dy: 0.0,
            vx: 0.0,
            vy: 0.0,
            ignored: 0,
            ignored_minions: None,
            placed: false,
            hx,
            hy,
            trace: blocks(c, BlockKind::Trace),
            nav: blocks(c, BlockKind::Nav),
            item: blocks(c, BlockKind::Item),
        }
    }

    /// `place`: the box centred on (x, y) with its half extents; dx / dy its move since the last placing (0 at the
    /// first).
    pub fn place(&mut self, x: Real, y: Real) {
        self.dx = if self.placed { x - self.x } else { 0.0 };
        self.dy = if self.placed { y - self.y } else { 0.0 };
        self.placed = true;
        self.x = x;
        self.y = y;
        self.left = x - self.hx;
        self.right = x + self.hx;
        self.top = y + self.hy;
        self.bottom = y - self.hy;
    }

    /// `ignores`: whether the capsule of the fighter with id `id` ignores the box (all of them while its collision
    /// is off).
    pub fn ignores(&self, id: i32) -> bool {
        !self.on || ignores_id(self.ignored, self.ignored_minions.as_deref(), id)
    }

    /// `setIgnored`: the fighter with id `id` ignores the box when moving, or no longer.
    pub fn set_ignored(&mut self, id: i32, v: bool) {
        if id < 32 {
            let bit = 1i32.wrapping_shl(id.cast_unsigned());
            self.ignored = if v { self.ignored | bit } else { self.ignored & !bit };
            return;
        }
        // A minion (ids from MINION_ID_BASE): a shift would wrap at 32 onto a player's bit.
        let k = self.ignored_minions.as_ref().and_then(|l| l.iter().position(|&m| m == id));
        match k {
            None if v => self.ignored_minions.get_or_insert_with(Vec::new).push(id),
            Some(k) if !v => {
                let l = self.ignored_minions.as_mut().expect("the list it was found in");
                l.remove(k);
                if l.is_empty() {
                    self.ignored_minions = None;
                }
            }
            _ => {}
        }
    }
}

/// The ignore list's test: a bit per player id under 32, else the minions' list.
fn ignores_id(bits: i32, minions: Option<&[i32]>, id: i32) -> bool {
    if id < 32 { (bits.wrapping_shr(id.cast_unsigned()) & 1) == 1 } else { minions.is_some_and(|l| l.contains(&id)) }
}

/// `enabled`: a component template whose CollisionEnabled is not NoCollision.
fn enabled(c: Option<&Props>) -> bool {
    c.is_some_and(|c| {
        !c.get("BodyInstance")
            .and_then(Value::as_obj)
            .and_then(|b| b.get("CollisionEnabled"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .ends_with("NoCollision")
    })
}

/// `SpawnedPlatform`: the parent's variables and its parts, held by the child's actor (`WorldActor::plat`).
/// `parts` is the actor's `solids`: the parent's Collision and WallbounceCollision (`collision`, `wallbounce`: their
/// places in it) and the boxes a child adds of its own, in the order the child lists them.
#[derive(State, Debug)]
pub struct SpawnedPlatform {
    pub age: i32,
    /// Comp_Actor_Destructible.TeamIndex, taken as the owner's at the spawn.
    pub team: i32,
    pub collision: Option<u8>,
    pub wallbounce: Option<u8>,
    pub parts: Vec<PlatformPart>,
    /// Whether the actor lists its parts (`solids`): false while a child holds them out (WW_ShieldActor's socket
    /// boxes not upright: no parts).
    pub listed: bool,
    /// The tick DelayCollisionEnabled's Delay(0.2) runs out at, -1 for none.
    pub collision_at: Real,
    /// Comp_EdgeTrace's BeginPlay place of its actor (edgeBeginPlay); `None` where the level
    /// has no generator.
    pub edge_at: Option<(Real, Real)>,
}

impl SpawnedPlatform {
    /// `new SpawnedPlatform(actor, a, collision, wallbounce)`: the parts of the class `cls` of the owner's data,
    /// `collision` / `wallbounce` the half extents of the two boxes in the play plane (`None` for one the child
    /// cannot place).
    pub fn new(
        w: &World,
        owner: FighterId,
        cls: Name,
        collision: Option<(Real, Real)>,
        wallbounce: Option<(Real, Real)>,
    ) -> SpawnedPlatform {
        let mut pl = SpawnedPlatform {
            age: 0,
            team: w.fighter(owner).team.team,
            collision: None,
            wallbounce: None,
            parts: Vec::new(),
            listed: true,
            collision_at: -1.0,
            edge_at: None,
        };
        let Some(def) = crate::fighter::common::actor_class_def(w, owner, cls) else {
            panic!("SpawnedPlatform of {}: no class data in the owner's data", w.names.str(cls))
        };
        let p = &def.p;
        let comp = |k: &str| def.components.get(k).and_then(Value::as_obj);
        let (c, wb) = (comp("Collision"), comp("WallbounceCollision"));
        let delay = truthy(p, "DelayCollisionEnabled");
        if let (Some((hx, hy)), Some(c)) = (collision, c)
            && (enabled(Some(c)) || delay)
        {
            let mut pt = PlatformPart::new(name!("Collision"), false, hx, hy, c);
            pt.on = !delay;
            pl.collision = Some(pl.push(pt));
            if delay {
                pl.collision_at = crate::fighter::common::latent_frames(0.2);
            }
        }
        if let (Some((hx, hy)), Some(wb)) = (wallbounce, wb)
            && truthy(p, "UseWallbounceCollision")
            && enabled(Some(wb))
        {
            let mut pt = PlatformPart::new(name!("WallbounceCollision"), true, hx, hy, wb);
            pt.on = true;
            pl.wallbounce = Some(pl.push(pt));
        }
        pl
    }

    /// A part added to the list; its place in it.
    pub fn push(&mut self, pt: PlatformPart) -> u8 {
        self.parts.push(pt);
        u8::try_from(self.parts.len() - 1).expect("fewer than 256 parts")
    }

    /// The parent's own parts (`SpawnedPlatform.solids`): Collision, then WallbounceCollision.
    fn own(&self) -> impl Iterator<Item = usize> + use<> {
        [self.collision, self.wallbounce].into_iter().flatten().map(usize::from)
    }

    /// The parent's Collision part.
    pub fn collision_part(&mut self) -> Option<&mut PlatformPart> {
        self.collision.map(|i| &mut self.parts[usize::from(i)])
    }

    /// The parent's WallbounceCollision part.
    pub fn wallbounce_part(&mut self) -> Option<&mut PlatformPart> {
        self.wallbounce.map(|i| &mut self.parts[usize::from(i)])
    }

    /// `platformStep`: the actor's tick: the Delay(0.2) of DelayCollisionEnabled. The child places the parts.
    // Unverified: the Delay counts the spawn frame (the actor ticks as newly spawned), as the port's other latent
    // Delays.
    pub fn step(&mut self) {
        self.age = self.age.wrapping_add(1);
        if crate::real(self.age) == self.collision_at
            && let Some(c) = self.collision_part()
        {
            c.on = true;
        }
    }

    /// `scalePlatform`: Comp_SizeManager.SetSizeScale on a child whose boxes are its root's children with no scale
    /// of their own: each of the parent's boxes' half extents times r, the new scale over the old (SetActorScale3D,
    /// absolute). The child places the boxes again. No sweep: a fighter a grown box now overlaps is resolved as for
    /// a box that turned solid inside him (the movement's collidePlatforms).
    pub fn scale(&mut self, r: Real) {
        for i in self.own() {
            let s = &mut self.parts[i];
            s.hx *= r;
            s.hy *= r;
        }
    }

    /// `platformEnd`: ReceiveEndPlay: the actor leaves the game state's two lists.
    pub fn end(&mut self) {
        for i in self.own() {
            self.parts[i].dead = true;
        }
    }

    /// The parts the actor lists (`solids`).
    pub fn solids(&self) -> &[PlatformPart] {
        if self.listed { &self.parts } else { &[] }
    }
}

/// A world actor's SpawnedPlatform, `None` for one of a class without it or gone.
pub fn plat_of(w: &World, id: ActorId) -> Option<&SpawnedPlatform> {
    w.actors.actors.get(id).and_then(|a| a.plat.as_ref())
}

/// `plat_of`, to write.
pub fn plat_mut(w: &mut World, id: ActorId) -> Option<&mut SpawnedPlatform> {
    w.actors.actors.get_mut(id).and_then(|a| a.plat.as_mut())
}

/// `platformStep` of a world actor (the parent's tick).
pub fn platform_step(w: &mut World, id: ActorId) {
    if let Some(pl) = plat_mut(w, id) {
        pl.step();
    }
}

/// `platformEnd` of a world actor.
pub fn platform_end(w: &mut World, id: ActorId) {
    if let Some(pl) = plat_mut(w, id) {
        pl.end();
    }
}

/// `scalePlatform` of a world actor: the new scale over the old.
pub fn scale_platform(w: &mut World, id: ActorId, r: Real) {
    if let Some(pl) = plat_mut(w, id) {
        pl.scale(r);
    }
}

/// `ignorePlatformCollision`: BPI_IgnorePlatformCollision(Object, Ignore) @787-2680, the parent's: Object a Character
/// @787. With Ignore the capsule and Collision ignore each other when moving @876-940. Else, for a Character with a
/// Comp_Actor_Destructible (every fighter): of the platform's team with IgnoreAllies, or of another team with
/// IgnoreEnemies, the same @1099-1279. Else the two ignore each other when the capsule's bottom + 10 is at or under
/// the top of Collision's bounds, or the Character's Velocity.Z > 0 @1284-2608 (a platform passed from below and
/// from the side, solid for a capsule that is over it and not rising).
/// The caller is BaseFighter's HandlePlatformCollision with its IgnorePlatforms (every pawn tick and the jump and
/// evade drops), vy the velocity it passes there.
pub fn ignore_platform_collision(w: &mut World, id: ActorId, f: FighterId, ignore: bool, vy: Real) {
    let Some(pl) = plat_of(w, id) else { return };
    let Some(c) = pl.collision else { return };
    let (team, top) = (pl.team, pl.parts[usize::from(c)].top);
    let p = crate::actors::world_actor::actor_p(w, ActorRef::Actor(id));
    let fm = w.fighter(f);
    let v = ignore
        || (if fm.team.team == team { truthy(p, "IgnoreAllies") } else { truthy(p, "IgnoreEnemies") })
        || fm.body.y - crate::fighter::body::cap_half(w, f) + 10.0 <= top
        || vy > 0.0;
    let fid = fm.id;
    if let Some(c) = plat_mut(w, id).and_then(SpawnedPlatform::collision_part) {
        c.set_ignored(fid, v);
    }
}

/// `ignoreWallbounceCollision`: BPI_IgnoreWallbounceCollision(Object, Ignore) @2681-3723, the parent's: for an actor
/// with a Comp_Actor_Destructible and a primitive root @2756-2953, WallbounceCollision and that root ignore each
/// other when moving by: of the platform's team, !WallbounceAllies || Ignore; of another team, Ignore ||
/// !WallbounceEnemies @3014-3629.
/// The caller is Comp_Pawn_HitstunHandler's ReceiveTick @8431-9098: every tick, for every actor of the game state's
/// BPI_GetWallbounceCollisions, BPI_IgnoreWallbounceCollision(the handler's owner, !CheckHitstunActive): the box is a
/// wall for the fighters in hitstun alone.
pub fn ignore_wallbounce_collision(w: &mut World, id: ActorId, f: FighterId, ignore: bool) {
    let Some(pl) = plat_of(w, id) else { return };
    if pl.wallbounce.is_none() {
        return;
    }
    let team = pl.team;
    let p = crate::actors::world_actor::actor_p(w, ActorRef::Actor(id));
    let fm = w.fighter(f);
    let v = if fm.team.team == team {
        !truthy(p, "WallbounceAllies") || ignore
    } else {
        ignore || !truthy(p, "WallbounceEnemies")
    };
    let fid = fm.id;
    if let Some(c) = plat_mut(w, id).and_then(SpawnedPlatform::wallbounce_part) {
        c.set_ignored(fid, v);
    }
}

/// `wallbounceTick`: the hitstun handlers' calls of the frame (`ignore_wallbounce_collision`'s caller), for every
/// fighter and every live actor with a WallbounceCollision part.
// Unverified: the calls are made at the head of the frame, behind the fighters' frame-boundary timers and ahead of
// every move of the frame (the handler is a component of each fighter; where its tick stands among the frame's
// ticks is not read for this call): CheckHitstunActive is the fighter's state there.
pub fn wallbounce_tick(w: &mut World) {
    let mut i = 0;
    while let Some(x) = w.actors.actors.nth(i) {
        i += 1;
        let a = w.actors.actors.get(x).expect("a live actor id");
        let Some(pl) = &a.plat else { continue };
        if a.dead || !pl.solids().iter().any(|s| s.wall) {
            continue;
        }
        let cls = a.cls;
        let Some(k) = crate::combat::spawned_hits::world_hooks(w, cls).and_then(|k| k.ignore_wallbounce_collision)
        else {
            crate::fighter::common::warn_parts(
                w,
                &[w.names.str(cls), ": BPI_IgnoreWallbounceCollision is not entered"],
            );
            continue;
        };
        let mut j = 0;
        while let Some(&f) = w.pawns.as_ref().unwrap_or(&w.players).get(j) {
            j += 1;
            let hit = crate::fighter::in_hitstun(w, f);
            k(w, x, f, !hit);
        }
    }
}

/// The parent's own entry (`spawnedPlatformParent`): what a child that overrides neither function takes by naming
/// it.
pub fn parent_hooks() -> WorldActorHooks {
    WorldActorHooks {
        step: Some(platform_step),
        end: Some(platform_end),
        ignore_platform_collision: Some(ignore_platform_collision),
        ignore_wallbounce_collision: Some(ignore_wallbounce_collision),
        ..WorldActorHooks::default()
    }
}

/// `actors({ SpawnedPlatformParent_C: spawnedPlatformParent })`.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let class = WorldActorClass::SpawnedPlatformParent;
    r.actors.enter(class.name(), ActorEntry::new(ActorClass::World(class), ActorHooks::World(parent_hooks())))
}

/// `platformBlockers`: the parts of the World.actors entries and the boxes of the live projectiles with their
/// collision on that block a query of `kind`, added to `out` (made from `base` at the first); a projectile with its
/// collision off (`no_collision`) is left out.
pub fn platform_blockers(w: &World, kind: BlockKind, base: &[Ground], out: Option<Vec<Ground>>) -> Option<Vec<Ground>> {
    platform_blockers_marked(w, kind, base, out, None)
}

/// [`platform_blockers`] that also pushes each box's [`BoxMark`] to `marks`.
pub fn platform_blockers_marked(
    w: &World,
    kind: BlockKind,
    base: &[Ground],
    mut out: Option<Vec<Ground>>,
    mut marks: Option<&mut Vec<BoxMark>>,
) -> Option<Vec<Ground>> {
    for (id, a) in w.actors.actors.iter() {
        let Some(pl) = &a.plat else { continue };
        if a.dead {
            continue;
        }
        for s in pl.solids() {
            if !s.on {
                continue;
            }
            let k = match kind {
                BlockKind::Trace => s.trace,
                BlockKind::Nav => s.nav,
                BlockKind::Item => s.item,
                // (`detect` / `dynamic`: from the box's template)
                BlockKind::Detect | BlockKind::Dynamic => {
                    part_template(w, a.owner, a.cls, s.comp).is_some_and(|c| blocks(c, kind))
                }
            };
            if !k {
                continue;
            }
            out.get_or_insert_with(|| base.to_vec()).push(Ground {
                left: f64::from(s.left),
                right: f64::from(s.right),
                bottom: f64::from(s.bottom),
                top: f64::from(s.top),
            });
            if let Some(m) = marks.as_deref_mut() {
                m.push(BoxMark::Actor(ActorRef::Actor(id)));
            }
        }
    }
    for (id, pr) in w.actors.projectiles.iter() {
        let Some(solids) = &pr.solids else { continue };
        if pr.dead || pr.no_collision {
            continue;
        }
        for s in solids {
            if !s.on {
                continue;
            }
            let k = match kind {
                BlockKind::Trace => s.trace,
                BlockKind::Nav => s.nav,
                BlockKind::Item => s.item,
                BlockKind::Detect => s.det,
                // (`dynamic`: from the box's template)
                BlockKind::Dynamic => box_template(w, pr, s).is_some_and(|c| blocks(c, kind)),
            };
            if !k {
                continue;
            }
            out.get_or_insert_with(|| base.to_vec()).push(Ground {
                left: f64::from(s.left),
                right: f64::from(s.right),
                bottom: f64::from(s.bottom),
                top: f64::from(s.top),
            });
            if let Some(m) = marks.as_deref_mut() {
                m.push(BoxMark::Actor(ActorRef::Projectile(id)));
            }
        }
    }
    out
}

/// The live actors' parts of the frame (`platformsOfFrame`'s `x.solids` loop), pushed to `out`.
pub fn actor_platforms(w: &World, out: &mut Vec<PlatformRef>) {
    for (id, a) in w.actors.actors.iter() {
        let Some(pl) = &a.plat else { continue };
        if a.dead {
            continue;
        }
        let n = pl.solids().len();
        out.extend((0..n).map(|j| PlatformRef::Spawned(ActorRef::Actor(id), u32::try_from(j).expect("a part index"))));
    }
}

// ---- the projectiles' boxes
//
// A box of the Platform object type (ECC_GameTraceChannel7) on a projectile, attached to its root Capsule:
// BugsBunnyRocketFProjectile's Collision (Custom profile, KnockBack and ProjectileGroundTrace Block, Item, WorldStatic,
// WorldDynamic and Platform Ignore, NoCollision in the template, RelativeLocation (-20, 0, 68.86), RelativeScale3D
// (7.25, 2.75, .25), no BoxExtent: the class default 32). The class's kind makes it (`projectile_box`) and turns it on
// and off (`on`: its SetCollisionEnabled); the world places it at each frame's head where its projectile stands
// (`platforms_of_frame`: dx / dy are the projectile's move of the frame before) and ends it with the projectile.
// Based movement (stock and read in the exe): a fighter that stands on it is carried by its moves (baseUpdate, a swept
// follow); a jump or a walk off its end adds its velocity (vx, vy: the projectile's); one whose projectile was
// destroyed loses the base with no impart and falls with his own.
// A box of another object type joins the same way when the fighters' capsule blocks it: BugsBunnyRocketUpProjectile's
// Box (Custom profile, no ObjectType serialized: WorldDynamic, the UShapeComponent default; Pawn not listed: Block, and
// the Pawn profile blocks WorldDynamic). It blocks the WallJump traces too (blocksWallJump).
// It has no CanPassThrough and no BPI_IgnorePlatformCollision: those are SpawnedPlatformParent's functions, and
// BaseFighter casts the movement base's owner to that class before it calls the first (ubergraph 47704,
// 76807-77060; a failed cast is the plain jump or evade), while only that class's BeginPlay puts an actor in the game
// state's list HandlePlatformCollision walks (@169). So it is solid from every side for every fighter, and a landing
// on it is on solid ground (OnLanded resets the wall fatigue).
// The imparted velocity (vx, vy) is the projectile's at the placing, so of the frame before; its angular part is 0
// (GetImpartedMovementBaseVelocity's angular term reads the base body's physics angular velocity, and the box does
// not simulate). GetMovementBaseVelocity 1428ffc40: the box's own ComponentVelocity is 0, so its owner's GetVelocity,
// the ProjectileMovement's.
// Deviation: the build ticks the base's actor ahead of the based fighter's CMC (MovementBaseUtility::
// AddTickDependency, 4.26 source); the port moves the projectiles after the fighters' CMC ticks, so a fighter follows
// the projectile's move of the frame before and drops a frame after its death.

/// `PlatformBox.place` of a projectile's box.
fn place(b: &mut ProjectileBox, x: Real, y: Real) {
    b.dx = if b.placed { x - b.x } else { 0.0 };
    b.dy = if b.placed { y - b.y } else { 0.0 };
    b.placed = true;
    b.x = x;
    b.y = y;
    b.left = x - b.hx;
    b.right = x + b.hx;
    b.top = y + b.hy;
    b.bottom = y - b.hy;
}

/// `ProjectileBox.follow`: box `i` of the projectile placed where it stands now: the root's place, scale (the
/// root's, which its children inherit), pitch and the side its yaw faces; its velocity the projectile's.
// Deviation: axis-aligned in the play plane: the centre turns with the pitch and the yaw side, the extents do not
// (bRotationFollowsVelocity tilts the build's box with the velocity), but for a box its kind asks to turn (RocketUp's,
// whose flight is straight up or down), whose extents lie along x under a pitch of 45 degrees and along y over it
// (exact at pitch 0 and +-90); a rider follows the centre's translation alone, not the base's turn
// (UpdateBasedMovement also turns the capsule's place about a base that rotated: a reflected RocketF's yaw flip).
pub fn projectile_box_follow(w: &mut World, pr: ProjectileId, i: usize) {
    let p = w.actors.projectiles.get(pr).expect("a live projectile id");
    let (k, side) = (p.scale, crate::actors::projectile::yaw_x(p));
    let [cp, sp] = crate::math::ue::turn_by_pitch(p.pitch, [1.0, 0.0]);
    let pitch = p.pitch;
    let (px, py, vx, vy) = (p.x, p.y, p.vx, p.vy);
    let b = &mut w.actors.projectiles.get_mut(pr).expect("a live projectile id").solids.as_mut().expect("its boxes")[i];
    let (ox, oz) = (b.ox * k, b.oz * k);
    let up = b.turns && sp.abs() > cp.abs();
    b.hx = (if up { b.ey } else { b.ex }) * k;
    b.hy = (if up { b.ex } else { b.ey }) * k;
    let [tx, tz] = crate::math::ue::turn_by_pitch(pitch, [ox, oz]);
    place(b, px + side * tx, py + tz);
    b.vx = vx;
    b.vy = vy;
}

/// A number of a template's object field (`o.k ?? d`).
fn num_or(o: Option<&Props>, k: &str, d: Real) -> Real {
    crate::math::narrow(o.and_then(|o| o.get(k)).and_then(Value::as_f64).unwrap_or(f64::from(d)))
}

/// `projectileBox`: a projectile class's kind asks for its box that the fighters' capsule blocks (its `make` or
/// `begin`): `comp` names the component in the class data, which must be a box of the Platform object type, or a box
/// with no ObjectType serialized (WorldDynamic) whose response to Pawn is Block (unlisted in a Custom profile: Block).
/// The box joins the projectile's solids, placed where the projectile stands at the call (a kind that moves it later,
/// the spawn's push-out, calls it from `begin`, or the next frame head's placing counts that move as the box's).
/// `turn`: its extents turn with the pitch (`projectile_box_follow`). Its place in the solids, `None` for a component
/// that is no such box (warned).
pub fn projectile_box(w: &mut World, pr: ProjectileId, comp: &str, turn: bool) -> Option<usize> {
    let p = w.actors.projectiles.get(pr).expect("a live projectile id");
    let c = crate::actors::projectile::class_def(w, p).and_then(|d| d.components.get(comp)).and_then(Value::as_obj);
    let b0 = c.and_then(|c| c.get("BodyInstance")).and_then(Value::as_obj);
    let pawn = response(c.unwrap_or(&NO_PROPS), "Pawn").unwrap_or("ECR_Block");
    let solid = b0.is_some_and(|b| {
        b.get("ObjectType").and_then(Value::as_str) == Some("ECC_GameTraceChannel7")
            || (b.get("ObjectType").is_none()
                && b.get("CollisionProfileName").and_then(Value::as_str) == Some("Custom")
                && c.and_then(|c| c.get("BoxExtent")).is_some_and(|e| !matches!(e, Value::Null))
                && pawn == "ECR_Block")
    });
    let (Some(c), true) = (c, solid) else {
        crate::fighter::common::warn_parts(
            w,
            &[
                w.names.str(p.cls),
                ": ",
                comp,
                " is no box of object type Platform or a Custom WorldDynamic box that blocks Pawn in the class data",
            ],
        );
        return None;
    };
    let obj = |k: &str| c.get(k).and_then(Value::as_obj);
    let (e, s, l) = (obj("BoxExtent"), obj("RelativeScale3D"), obj("RelativeLocation"));
    // Local X is the actor's forward (sim x at the spawn yaw +-90), local Z up; local Y is the depth.
    let b = ProjectileBox {
        on: enabled(Some(c)),
        trace: blocks(c, BlockKind::Trace),
        nav: blocks(c, BlockKind::Nav),
        item: blocks(c, BlockKind::Item),
        det: blocks(c, BlockKind::Detect),
        ex: num_or(e, "X", 32.0) * num_or(s, "X", 1.0),
        ey: num_or(e, "Z", 32.0) * num_or(s, "Z", 1.0),
        ox: num_or(l, "X", 0.0),
        oz: num_or(l, "Z", 0.0),
        turns: turn,
        ..ProjectileBox::default()
    };
    let template = w.lit(comp);
    let p = w.actors.projectiles.get_mut(pr).expect("a live projectile id");
    let solids = p.solids.get_or_insert_with(Vec::new);
    solids.push(ProjectileBox { template, ..b });
    let i = solids.len() - 1;
    projectile_box_follow(w, pr, i); // placed at once: the World's blockers meet it before the next frame's head
    Some(i)
}

static NO_PROPS: std::sync::LazyLock<Props> = std::sync::LazyLock::new(Props::default);

/// A projectile box's template (`BOX_TEMPLATE`): its component in the projectile's class data.
pub(crate) fn box_template<'a>(w: &'a World, pr: &Projectile, b: &ProjectileBox) -> Option<&'a Props> {
    crate::actors::projectile::class_def(w, pr)?.components.get(w.names.str(b.template)).and_then(Value::as_obj)
}

/// A part's template (`PlatformBox.template`): its component in the class data of the actor's owner.
pub(crate) fn part_template(w: &World, owner: FighterId, cls: Name, comp: Name) -> Option<&Props> {
    crate::fighter::common::actor_components(w, ActorRef::Fighter(owner), cls)?
        .get(w.names.str(comp))
        .and_then(Value::as_obj)
}

// ---- the boxes as the movement meets them (`PlatformRef::Spawned`)

/// A spawned box resolved from its actor and place.
enum BoxAt<'a> {
    Part(&'a WorldActor, &'a PlatformPart),
    Projectile(&'a Projectile, &'a ProjectileBox),
    /// A C019 ship (`chars::morty::ship`): its pieces are one platform.
    Ship(&'a Ship),
    /// Its actor is gone from its arena.
    Gone,
}

/// Unverified: StevenCollisionBlock's Box is kept as a part here but treated as a plain PlatformBox
/// (BlockBox), not a PlatformPart: no ignorePlatformCollision, canPassThrough or wallBounceMultiplier.
fn plain_box(x: &WorldActor) -> bool {
    matches!(x.state, crate::actors::world_actor::WorldActorState::StevenBlock(_))
}

fn box_at(w: &World, a: ActorRef, i: u32) -> BoxAt<'_> {
    let i = i as usize;
    match a {
        ActorRef::Actor(id) => match w.actors.actors.get(id) {
            Some(x) => {
                let pl = x.plat.as_ref().expect("a world actor platform of a SpawnedPlatform");
                BoxAt::Part(x, &pl.parts[i])
            }
            None => BoxAt::Gone,
        },
        ActorRef::Projectile(id) => match w.actors.projectiles.get(id) {
            Some(p) => BoxAt::Projectile(p, &p.solids.as_ref().expect("a projectile platform with boxes")[i]),
            None => BoxAt::Gone,
        },
        ActorRef::Ship(id) => w.actors.ships.get(id).map_or(BoxAt::Gone, BoxAt::Ship),
        _ => panic!("a spawned platform of an actor of no platform family: {a:?}"),
    }
}

/// `trace`: whether the box blocks the trace of type 3 (ProjectileGroundTrace); the ship has no such field (unset,
/// not false), a gone box keeps none either.
pub fn spawned_trace(w: &World, a: ActorRef, i: u32) -> bool {
    match box_at(w, a, i) {
        BoxAt::Part(_, s) => s.trace,
        BoxAt::Projectile(_, b) => b.trace,
        BoxAt::Ship(_) | BoxAt::Gone => true,
    }
}

/// The box's sides, `None` for one gone.
fn sides(w: &World, a: ActorRef, i: u32) -> Option<(Real, Real)> {
    match box_at(w, a, i) {
        BoxAt::Part(_, s) => Some((s.left, s.right)),
        BoxAt::Projectile(_, b) => Some((b.left, b.right)),
        BoxAt::Ship(_) | BoxAt::Gone => None,
    }
}

/// A SpawnedPlatform's part or a projectile's box `i` of actor `a` as the movement's platform view.
// Deviation: a box whose actor left its arena reads as dead at (0, 0) (its last place is not kept); the
// readers of a dead base drop it first.
pub fn spawned_plat(w: &World, a: ActorRef, i: u32) -> Plat {
    let mut v = Plat {
        r: PlatformRef::Spawned(a, i),
        dead: true,
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
        dx: 0.0,
        dy: 0.0,
        vx: 0.0,
        vy: 0.0,
        stage_index: None,
        stage_solid: false,
        poly: false,
        fresh: false,
        wall: false,
        ignore_hook: false,
        last_move_z: None,
    };
    match box_at(w, a, i) {
        BoxAt::Part(x, s) => {
            (v.dead, v.left, v.right, v.top, v.bottom) = (s.dead, s.left, s.right, s.top, s.bottom);
            (v.dx, v.dy, v.vx, v.vy) = (s.dx, s.dy, s.vx, s.vy);
            v.wall = s.wall;
            v.ignore_hook = !plain_box(x);
        }
        BoxAt::Projectile(_, b) => {
            (v.dead, v.left, v.right, v.top, v.bottom) = (b.dead, b.left, b.right, b.top, b.bottom);
            (v.dx, v.dy, v.vx, v.vy) = (b.dx, b.dy, b.vx, b.vy);
            v.wall = b.wall;
        }
        // The ship has no left / right (NaN here); its readers take its spans.
        BoxAt::Ship(s) => {
            (v.dead, v.left, v.right, v.top, v.bottom) = (s.dead, Real::NAN, Real::NAN, ship::top(s), ship::bottom(s));
            (v.dx, v.dy, v.vx, v.vy) = (s.dx, s.dy, s.vx, s.vy);
        }
        BoxAt::Gone => {}
    }
    v
}

/// `spanAt`: the box itself under x.
pub fn spawned_span_at(w: &World, a: ActorRef, i: u32, x: Real) -> Option<Span> {
    if let BoxAt::Ship(s) = box_at(w, a, i) {
        return ship::span_at(s, x);
    }
    let (left, right) = sides(w, a, i)?;
    (x >= left && x <= right).then_some(Span { left, right })
}

/// `spanNear`: the box within r of x.
pub fn spawned_span_near(w: &World, a: ActorRef, i: u32, x: Real, r: Real) -> Option<Span> {
    if let BoxAt::Ship(s) = box_at(w, a, i) {
        return ship::span_near(s, x, r);
    }
    let (left, right) = sides(w, a, i)?;
    (x > left - r && x < right + r).then_some(Span { left, right })
}

/// `clampX`.
pub fn spawned_clamp_x(w: &World, a: ActorRef, i: u32, x: Real) -> Real {
    if let BoxAt::Ship(s) = box_at(w, a, i) {
        return ship::clamp_x(s, x);
    }
    let (left, right) = sides(w, a, i).unwrap_or((0.0, 0.0));
    max(left, min(right, x))
}

/// `canStepUpOn`: whether a walker may step up onto the box (its template's CanCharacterStepUpOn); `None` for a
/// box with no answer (one gone).
pub fn spawned_can_step_up_on(w: &World, a: ActorRef, i: u32, x: Real, r: Real) -> Option<bool> {
    match box_at(w, a, i) {
        BoxAt::Part(x, s) => Some(can_step_up_on(w, part_template(w, x.owner, x.cls, s.comp), x.cls)),
        BoxAt::Projectile(p, b) => Some(can_step_up_on(w, box_template(w, p, b), p.cls)),
        BoxAt::Ship(s) => Some(ship::ship_can_step_up_on(w, s, x, r)),
        BoxAt::Gone => None,
    }
}

/// `wallBounceMultiplier`: BaseFighter's capsule hit on the box (ubergraph @128863-129041): the actor's
/// BPI_GetWallBounceMultiplier(f), the class's own (`WorldActorHooks::wall_bounce_multiplier`), else the parent's 1.0.
/// `None` for a projectile's box (no such answer).
pub fn spawned_wall_bounce_multiplier(w: &mut World, a: ActorRef, i: u32, f: FighterId) -> Option<Real> {
    match box_at(w, a, i) {
        BoxAt::Part(x, _) if plain_box(x) => None,
        BoxAt::Part(x, _) => {
            let ActorRef::Actor(id) = a else { unreachable!("a part of a world actor") };
            let k = crate::combat::spawned_hits::world_hooks(w, x.cls).and_then(|k| k.wall_bounce_multiplier);
            Some(k.map_or(1.0, |k| k(w, id, i, f)))
        }
        BoxAt::Projectile(..) | BoxAt::Ship(_) => None,
        // (a gone actor keeps its method)
        BoxAt::Gone => matches!(a, ActorRef::Actor(_)).then_some(1.0),
    }
}

/// `blocksWallJump`: whether the box, with its collision on, blocks the WallJump traces (the fighter's wall slide
/// and wall jump traces meet it). From the template in the class data. A projectile's box: none while its
/// projectile's collision is off (SetActorEnableCollision(false)).
pub fn spawned_blocks_wall_jump(w: &World, a: ActorRef, i: u32) -> Option<bool> {
    match box_at(w, a, i) {
        BoxAt::Part(x, s) => Some(s.on && blocks_wall_jump(part_template(w, x.owner, x.cls, s.comp))),
        BoxAt::Projectile(p, b) => Some(!p.no_collision && b.on && blocks_wall_jump(box_template(w, p, b))),
        // (the ship has no such answer, gone or not)
        BoxAt::Ship(_) => None,
        BoxAt::Gone => (!matches!(a, ActorRef::Ship(_))).then_some(false),
    }
}

/// `ignores`: whether the fighter's capsule ignores the box: all of them while its collision is off. A projectile's
/// box: SetActorEnableCollision(false) on the projectile (`no_collision`: the buffs that carry or eat it,
/// Buff_ProjectileThroughTunnel @189, Buff_C015HitProjectile @1289) turns off every component of the actor, the box
/// among them, whatever its own SetCollisionEnabled says: no floor, no blocker, no WallJump trace body.
pub fn spawned_ignores(w: &World, a: ActorRef, i: u32, f: FighterId) -> bool {
    let id = w.fighter(f).id;
    match box_at(w, a, i) {
        BoxAt::Part(_, s) => s.ignores(id),
        BoxAt::Projectile(p, b) => p.no_collision || !b.on || ignores_id(b.ignored, b.ignored_minions.as_deref(), id),
        // (the ship has no `ignores`, gone or not)
        BoxAt::Ship(_) => false,
        BoxAt::Gone => !matches!(a, ActorRef::Ship(_)),
    }
}

/// `PlatformPart.ignorePlatformCollision`: BaseFighter HandlePlatformCollision's call on a platform of the game
/// state's list (the movement's handlePlatformCollision): the actor's class answers
/// (`WorldActorHooks::ignore_platform_collision`); a WallbounceCollision part and a child's box its class skips take
/// none.
pub fn spawned_ignore_platform_collision(w: &mut World, p: PlatformRef, f: FighterId, ignore: bool, vy: Real) {
    let PlatformRef::Spawned(a, i) = p else { return };
    let BoxAt::Part(x, s) = box_at(w, a, i) else { return };
    let ActorRef::Actor(id) = a else { return };
    if s.wall || !s.takes_ignore {
        return;
    }
    let cls = x.cls;
    match crate::combat::spawned_hits::world_hooks(w, cls).and_then(|k| k.ignore_platform_collision) {
        Some(k) => k(w, id, f, ignore, vy),
        None => {
            crate::fighter::common::warn_parts(w, &[w.names.str(cls), ": BPI_IgnorePlatformCollision is not entered"]);
        }
    }
}

/// Whether the box answers `canPassThrough` (a SpawnedPlatformParent's part and the ship; a projectile's box does not).
pub fn spawned_has_pass_through(w: &World, a: ActorRef, i: u32) -> bool {
    let _ = (w, i);
    matches!(a, ActorRef::Actor(_) | ActorRef::Ship(_))
}

/// A world actor's SpawnedPlatform parts as the traces gather them (`a.solids`): false for an actor without them,
/// else true with each part's collision on and component name written to `out` (cleared first; its box:
/// `spawned_plat`).
pub fn spawned_solids(w: &World, a: ActorRef, out: &mut Vec<(bool, Name)>) -> bool {
    out.clear();
    let ActorRef::Actor(id) = a else { return false };
    let Some(pl) = plat_of(w, id) else { return false };
    out.extend(pl.solids().iter().map(|s| (s.on, s.comp)));
    true
}

/// A world actor's SpawnedPlatform Comp_EdgeTrace place (`plat.edgeAt`, holderOf): `None` for an
/// actor with no SpawnedPlatform, `Some(None)` before its BeginPlay placed it.
pub fn spawned_edge_at(w: &World, a: ActorId) -> Option<Option<(Real, Real)>> {
    plat_of(w, a).map(|pl| pl.edge_at)
}

/// `f.base?.canPassThrough?.(team) === true`: whether a fighter of `team` may drop through the platform `base`;
/// false for a platform without the method (a projectile's box, a stage solid).
pub fn platform_can_pass_through(w: &World, base: PlatformRef, team: i32) -> bool {
    let PlatformRef::Spawned(a, _) = base else { return false };
    match a {
        ActorRef::Actor(id) => {
            let Some(x) = w.actors.actors.get(id) else {
                // (the part of an actor gone from its arena answers from its last state)
                let g = w.actors.gone_plats.iter().find(|g| g.id == id);
                let g = g.expect("a fighter's base of a gone actor is kept in gone_plats");
                return if team == g.team { g.allies } else { g.enemies };
            };
            if plain_box(x) {
                return false;
            }
            let pl = x.plat.as_ref().expect("a world actor platform of a SpawnedPlatform");
            can_pass_through(crate::actors::world_actor::actor_p(w, a), crate::real(pl.team), team)
        }
        // Deviation: a ship gone from its arena answers false (its last state is not kept); the readers
        // of a dead base drop it first.
        ActorRef::Ship(id) => w.actors.ships.get(id).is_some_and(|s| ship::can_pass_through(w, s, team)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part() -> PlatformPart {
        let mut p = PlatformPart::new(name!("Collision"), false, 10.0, 5.0, &Props::default());
        p.on = true;
        p
    }

    #[test]
    fn ignore_list_takes_players_by_bit_and_minions_by_id() {
        let mut p = part();
        p.set_ignored(31, true);
        p.set_ignored(100, true);
        assert!(p.ignores(31) && p.ignores(100) && !p.ignores(0) && !p.ignores(132));
        assert_eq!(p.ignored, i32::MIN);
        p.set_ignored(31, false);
        p.set_ignored(100, false);
        assert_eq!((p.ignored, p.ignored_minions.is_none()), (0, true));
        p.on = false;
        assert!(p.ignores(0));
    }

    #[test]
    fn place_counts_the_move_from_the_second_placing() {
        let mut p = part();
        p.place(3.0, 4.0);
        assert_eq!((p.dx, p.dy, p.left, p.right, p.top, p.bottom), (0.0, 0.0, -7.0, 13.0, 9.0, -1.0));
        p.place(5.0, 1.0);
        assert_eq!((p.dx, p.dy), (2.0, -3.0));
    }

    #[test]
    fn delayed_collision_turns_on_at_its_tick_and_scale_and_end_reach_the_parents_parts() {
        let mut pl = SpawnedPlatform {
            age: 0,
            team: 0,
            collision: None,
            wallbounce: None,
            parts: Vec::new(),
            listed: true,
            collision_at: 3.0,
            edge_at: None,
        };
        let mut own = part();
        own.on = false;
        pl.collision = Some(pl.push(own));
        pl.push(part()); // a child's own box
        for _ in 0..2 {
            pl.step();
        }
        assert!(!pl.parts[0].on);
        pl.step();
        assert!(pl.parts[0].on);
        pl.scale(2.0);
        assert_eq!((pl.parts[0].hx, pl.parts[1].hx), (20.0, 10.0));
        pl.end();
        assert!(pl.parts[0].dead && !pl.parts[1].dead);
    }
}
