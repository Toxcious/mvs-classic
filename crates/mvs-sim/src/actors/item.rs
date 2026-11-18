//! Items (`Item`): an
//! ItemActorParent actor in the match, and the queues of its throw hits and uses that the world drains. The struct
//! holds every field of `Item`; [`ItemExt`] the fields a subclass adds. The class's data (`p`, the actor CDO, and
//! its components) is the bound data's, found through [`Item::src`].

use super::ItemClass;
use super::projectile::{BuffHome, LifetimePause, ReflectState};
use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::ItemId;
use crate::combat::{ChargeVals, ElemState, HitPacket};
use crate::consts::{DELTA_SECONDS, DT, GRAVITY_Z, ITEM_FADE_TIME, ITEM_IDLE_LIFE};
use crate::fighter::common::warn_text;
use crate::fighter::moves::V2;
use crate::fighter::traces::solids::PolyBlock;
use crate::fighter::{DodgeState, FighterId};
use crate::math::ue::size2d;
use crate::math::{max, min, sign, to_int32};
use crate::names::Name;
use crate::registry::entries::ItemHooks;
use crate::registry::{ActorHooks, Registry, RegistryError};
use crate::world::BlastBounds;
use crate::world::World;
use crate::world::blast::{SpawnBox, blast_box_loop, check_blast_box_exit};
use indexmap::IndexSet;
use mvs_collision::Ground;
use mvs_data::GameData;
use mvs_data::value::{Props, Value};
use std::sync::LazyLock;

/// Where an item's class data is: the entry of the actor table it was made from, by its place in that table.
#[derive(State, Copy, PartialEq, Eq, Debug, Default)]
pub enum DefSrc {
    /// No data (an item made by [`Item::new`] alone).
    #[default]
    None,
    /// A character's `FighterData.actors`: the character's place in `GameData.chars`, the entry's in its actors.
    Char(u32, u32),
    /// `StageData.actors`.
    Stage(u32),
    /// `ItemsData.actors`.
    Items(u32),
    /// A character's perk part (`PerkTable.actors`): the character's place, the entry's.
    Perk(u32, u32),
}

/// An actor table entry's two parts (`ActorDef`): the CDO's properties and its components' templates.
#[derive(Clone, Copy, Debug)]
pub struct ItemDef<'a> {
    pub p: &'a Props,
    pub components: &'a Props,
}

static EMPTY: LazyLock<Props> = LazyLock::new(Props::new);

impl<'a> ItemDef<'a> {
    /// The definition at `src` in `data`; an empty one for [`DefSrc::None`] or an entry that is not there.
    pub fn at(data: &'a GameData, src: DefSrc) -> ItemDef<'a> {
        let at = |i: u32| usize::try_from(i).expect("a table index");
        let found = match src {
            DefSrc::None => None,
            DefSrc::Char(c, a) => data
                .chars
                .get_index(at(c))
                .and_then(|(_, ch)| ch.fighter.actors.as_ref())
                .and_then(|m| m.get_index(at(a)))
                .map(|(_, d)| ItemDef { p: &d.p, components: &d.components }),
            DefSrc::Stage(a) => data
                .stage
                .actors
                .as_ref()
                .and_then(|m| m.get_index(at(a)))
                .map(|(_, d)| ItemDef { p: &d.p, components: &d.components }),
            DefSrc::Items(a) => data
                .items
                .as_ref()
                .and_then(|m| m.actors.get_index(at(a)))
                .map(|(_, d)| ItemDef { p: &d.p, components: &d.components }),
            DefSrc::Perk(c, a) => data
                .chars
                .get_index(at(c))
                .and_then(|(_, ch)| ch.perks.as_ref())
                .and_then(|t| t.actors.as_ref())
                .and_then(|m| m.get_index(at(a)))
                .map(|(_, e)| ItemDef { p: &e.p, components: &e.components }),
        };
        found.unwrap_or(ItemDef { p: &EMPTY, components: &EMPTY })
    }

    /// `components[name]`, the component's template.
    pub fn comp(&self, name: &str) -> Option<&'a Props> {
        self.components.get(name).and_then(Value::as_obj)
    }
}

/// `p[k]` as a number (`undefined` / `null` and a non-number are `None`).
pub fn num(p: &Props, k: &str) -> Option<Real> {
    p.get(k).and_then(Value::as_f64).map(crate::math::narrow)
}

/// `p[k]` as a boolean (`undefined` / `null` are `None`).
pub fn flag(p: &Props, k: &str) -> Option<bool> {
    p.get(k).and_then(Value::as_bool)
}

/// `!!p[k]`: JavaScript truthiness.
pub fn truthy(p: &Props, k: &str) -> bool {
    match p.get(k) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `p[k]` as a string.
pub fn text<'a>(p: &'a Props, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

/// `p[k].X` (a vector's X).
fn vec_x(p: &Props, k: &str) -> Option<Real> {
    p.get(k).and_then(Value::as_obj).and_then(|v| num(v, "X"))
}

/// The class's `ItemKind` hooks (`ACTORS.get(cls)?.item`).
pub fn item_hooks(r: &Registry, class: ItemClass) -> Option<&ItemHooks> {
    match r.actors.get(class.name()).map(|e| &e.hooks) {
        Some(ActorHooks::Item(h)) => Some(h),
        _ => None,
    }
}

/// DeathBlinkWarning's Blink track (constant keys, length 3 s = ItemFadeTime): the actor is hidden while the
/// value is below 0.5.
pub const BLINK_HIDDEN: [[Real; 2]; 7] = [
    [0.07063842, 0.24687612],
    [0.65320194, 0.8392309],
    [1.2406613, 1.475645],
    [1.8477024, 2.0092535],
    [2.2882967, 2.4988027],
    [2.6848316, 2.81701],
    [2.9736657, 3.0],
];

/// What ended an item in its step (`Item.step`'s answer).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemEnd {
    /// CheckBlastBoxExit.
    Blast,
    /// DeathBlinkWarning's Finished.
    Life,
    /// The blocking hit's tail spent the last Durability.
    Hit,
}

/// `Item.step`'s answer: why it was destroyed, if it was, and whether the step had a blocking hit (the root's
/// ReceiveHit, for the class's own ReceiveHit after the parent's: `ItemKind.receiveHit`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ItemStep {
    pub end: Option<ItemEnd>,
    pub blocked: bool,
}

/// A box an item's body meets (`World.blockers('item')`) with its marks: a stage
/// solid of object type WorldStatic (`isStaticSolid`, which a throw does not ignore) and the move a stage's code gave
/// it in its last pass (`kinematicOf`).
#[derive(Clone, Debug, Default)]
pub struct ItemBox {
    pub g: Ground,
    pub is_static: bool,
    pub kin: Option<V2>,
    /// The stage solid's data id (`StageSolidData.id`) when the box is one (`BoxMark::Solid`): the actor the hit's
    /// Other is, a placed stage actor's (`world::wall_bp::solid_destructible`).
    pub solid: Option<i32>,
    /// A polygon stage solid in place of the box (`g` unread): its nearest point and its top stand for the box's
    /// clamp and top.
    pub poly: Option<PolyBlock>,
}

impl ItemBox {
    /// The point of the shape nearest (x, y) ((x, y) inside it): the box's clamp.
    pub fn nearest(&self, x: Real, y: Real) -> (Real, Real) {
        match &self.poly {
            Some(q) => q.closest(x, y),
            None => {
                let b = &self.g;
                (
                    max(crate::math::narrow(b.left), min(x, crate::math::narrow(b.right))),
                    max(crate::math::narrow(b.bottom), min(y, crate::math::narrow(b.top))),
                )
            }
        }
    }

    /// The way up out of the shape of a point (x, y) inside it (a sample inside leaves through the top).
    pub fn below_top(&self, x: Real, y: Real) -> Real {
        match &self.poly {
            Some(q) => q.top_at(x) - y,
            None => crate::math::narrow(self.g.top - f64::from(y)),
        }
    }

    /// The least way out of the shape of the capsule at (x, y) (`capsule_mtd`'s rule).
    #[allow(clippy::too_many_arguments, reason = "a place and the capsule's shape")]
    pub fn mtd(&self, x: Real, y: Real, r: Real, h: Real, ux: Real, uy: Real) -> mvs_collision::capsule_mtd::Mtd {
        match &self.poly {
            Some(q) => q.mtd(x, y, r, h, ux, uy),
            None => mvs_collision::capsule_mtd::capsule_mtd(&self.g, x, y, r, h, ux, uy),
        }
    }
}

/// One blocking contact of the root Capsule in a step: the hit normal (out of the box, towards the item) and the box's
/// stage solid id ([`ItemBox::solid`]; `None` for the stage block and every other box).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct ItemContact {
    pub nx: Real,
    pub ny: Real,
    pub solid: Option<i32>,
}

/// A spawned item. Its data (`p`, the actor CDO) is the class's.
#[derive(State, Debug)]
pub struct Item {
    pub cls: Name,
    pub class: ItemClass,
    /// The class data it was made with (`p` and the constructor's `actor`).
    pub src: DefSrc,
    /// `id`: the world's item count at the spawn (`World.itemSeq`).
    pub seq: i32,
    pub x: Real,
    pub y: Real,
    pub prev_x: Real,
    pub prev_y: Real,
    /// Ticks since BeginPlay.
    pub age: i32,
    pub dead: bool,
    /// IsHeld: from the pickup until the Delay(0.1) after its release.
    pub held: bool,
    /// The holder's id, -1 none.
    pub holder: i32,
    /// Picked up by a Shaggy: dropped and pooled a frame later (MulticastPickedUpByShaggy).
    pub pooled: bool,
    /// A throw (Comp_ItemManager.ThrowItem -> BPI_ReleaseFromFighter): the ProjectileMovement is active (flying),
    /// IsThrown (trail, the Platform channel ignored), PickupArea at ThrownPickUpRadius, HitBoxOn? with the
    /// ThrowHitBox sphere.
    pub flying: bool,
    pub thrown: bool,
    pub hit_on: bool,
    /// TrackActorsHitByTag (HitEachActorOnlyOnce).
    pub hit_set: IndexSet<i32>,
    /// HitBoxDamage (a reflect replaces it with a scaled copy).
    pub hit: HitPacket,
    pub attack_id: i32,
    /// The root Capsule's world scale: the spawn transform's (USCS_Node::ExecuteNodeOnActor 142d4a790 gives an
    /// unattached root the transform whole), then a release's K2_SetActorTransform(..., scale (1, 1, 1)) and
    /// Comp_SizeManager.SetSizeScale's SetActorScale3D (`set_size_scale`).
    pub scale: Real,
    /// Whose Comp_Spawner lists the actor ('Die With Instigator'); `None`: a stage item.
    pub spawner: Option<FighterId>,
    /// Its Comp_Pawn_HitBoxHandler dodge variables.
    pub dodge: Option<DodgeState>,
    /// Its Comp_Pawn_ChargeAttack values, `None` at the defaults.
    pub charge: Option<ChargeVals>,
    /// The buff children of its Comp_State_Manager that the port holds.
    pub elem: Option<ElemState>,
    /// SpawnedActorParent LifetimePausedCount.
    pub lifetime_pause: Option<LifetimePause>,
    /// SpawnedActorParent DestroyVFXBlocked (BPI_SetDestroyEffectsBlocked @5202): no DeathVFX and no 'Ak Death
    /// Location' at the EndPlay (@15557).
    pub destroy_fx_blocked: bool,
    /// SpawnedActorParent DestroySpawnedActorBlocked (BPI_SetDestroySpawnedActorBlocked @5222): no SpawnActorAtDeath
    /// at the EndPlay (@16070); the port runs no SpawnActorAtDeath roll for an item, so nothing reads it yet.
    pub destroy_spawn_blocked: bool,
    /// The actor's owner / Instigator: the spawning fighter, then whoever picked it up last (LastHolder); `None`: a
    /// stage item no one has picked up yet.
    pub owner: Option<FighterId>,
    /// The root's pitch in degrees (FRotator's); spawned with the dying projectile's transform.
    pub pitch: Real,
    pub facing: Real,
    pub vx: Real,
    pub vy: Real,
    /// Spawned in the world's timer tick, after that frame's physics.
    pub late: bool,

    /// IsValid(InputUI): SetPickupIcon creates the widget once.
    pub input_ui: bool,
    /// The body touched the stage in its last physics step (a contact that persists reports none: `step`).
    pub touching: bool,
    /// Comp_Pawn_HitBoxHandler.HasHitAFighter, cleared by HitBoxesOff (`hit_box_off`).
    pub has_hit_a_fighter: bool,
    pub durability: Real,
    /// The ItemDestructible Sphere (overlaps the HitBox profile): off from the construction script without
    /// CanBeHitWhileIdle, QueryOnly from a throw (ubergraph 10146) until the hit tail (4941 -> 5313) turns it off
    /// again. A drop and a pickup leave it as it is (the pickup turns the whole actor's collision off).
    pub sphere_on: bool,
    /// Comp_Actor_Destructible.IsProjectile once a write changed it from the template's (`is_projectile`).
    pub proj_flag: Option<bool>,
    /// PickupArea.SphereRadius as a SetSphereRadius wrote it (`pickup_radius`): `None` until the first write.
    pub pick_r: Option<Real>,
    /// ItemHealth.
    pub health: Real,
    /// IsProjectileClanking and QueueDestroyAfterClank.
    pub clanking: bool,
    pub queue_destroy: bool,
    /// The spawned actors this throw's hitbox has hit (TrackActorsHitByTag).
    pub hit_actors: IndexSet<ActorRef>,
    /// The root's Mobility while not Movable.
    pub mobility: Option<u8>,
    pub stationary_count: Option<i32>,
    /// What a reflect left on it.
    pub reflect: Option<ReflectState>,
    /// SetActorEnableCollision(false) of Buff_ProjectileThroughTunnel.
    pub no_collision: Option<bool>,
    /// The frames left of a pending DestroySpawnedActor Delay.
    pub doom: Option<i32>,
    /// The ProjectileMovement's homing as a buff's graph wrote it.
    /// Unverified: a later throw keeps it (whether ItemActorParent's release or pooling resets the component's
    /// homing is not read).
    pub buff_home: Option<BuffHome>,
    /// The HurtBoxes / Spheres the ThrowHitBox is inside of and the hitboxes inside the Sphere.
    pub overlapping: IndexSet<ActorRef>,
    pub overlapped_by: IndexSet<ActorRef>,
    /// Item_Sandwich Thrown -> SetVisualSize: Geo.SetWorldScale3D(2), never set back.
    pub big_geo: bool,
    /// CurrentAmmo, StartingAmmo's copy at BeginPlay (@2221): `None` until a use spends one.
    pub ammo: Option<Real>,
    pub max_speed: Real,
    /// Comp_SizeManager.CurrentSizeScale once a buff's size mods wrote it; `None`: the component's default.
    pub size_scale: Option<Real>,
    /// With no owner: a fighter of the match the world spawned it in.
    pub lookup_fighter: Option<FighterId>,
    /// Latent actions about the item (the world tick at which each ends, -1: none): its own graph's and the
    /// pawn's that picked it up.
    pub act_at: i32,
    pub pawn_at: i32,
    pub pawn_of: Option<FighterId>,
    /// The root Capsule (SpawnedActorParent's 22 / 44 unless the class sets its own) and its body's linear damping
    /// (0.01 is the FBodyInstance default, 142c66290), which a class's graph may set (SetLinearDamping).
    pub cap_r: Real,
    pub cap_h: Real,
    pub lin_damp: Real,
    /// PickUp cleared LifeTimer; nothing starts it again.
    pub life_off: bool,
    /// The age at which the release's Delay(0.1) clears IsHeld.
    pub unheld_at: i32,
    /// ProjectileMovement.ProjectileGravityScale (a class's graph may write it).
    pub grav_scale: Real,
    pub hit_sphere: Real,
    pub hurt_sphere: Real,
    /// The fighters whose root capsule overlaps the PickupArea.
    pub reach: IndexSet<FighterId>,
    /// World timer ticks after the spawn frame until LifeTimer fires (-1: none).
    pub idle_ticks: i32,
    /// DeathBlinkWarning's position (float32), -1 while it is not playing.
    pub blink: Real,
    /// CheckBlastBoxTimer's next expiry, in seconds of counted deltas.
    pub blast_at: crate::Clock,
    /// Its BlastBoxBoundsMin / Max (SpawnedActorParent @9720-10573).
    pub blast_bounds: BlastBounds,
    pub ext: ItemExt,
}

impl Item {
    /// `new Item(cls, owner, pitch, facing, vx, vy, spawn, late)`'s field initialisers, with no class data; the
    /// constructor whole is [`Item::make`].
    #[allow(clippy::too_many_arguments, reason = "the constructor's parameters, in its order")]
    pub fn new(
        cls: Name,
        class: ItemClass,
        owner: Option<FighterId>,
        pitch: Real,
        facing: Real,
        vx: Real,
        vy: Real,
        late: bool,
    ) -> Self {
        Item {
            cls,
            class,
            src: DefSrc::None,
            seq: -1,
            x: 0.0,
            y: 0.0,
            prev_x: 0.0,
            prev_y: 0.0,
            age: 0,
            dead: false,
            held: false,
            holder: -1,
            pooled: false,
            flying: false,
            thrown: false,
            hit_on: false,
            hit_set: IndexSet::new(),
            hit: HitPacket::default(),
            attack_id: 0,
            scale: 1.0,
            spawner: owner,
            dodge: None,
            charge: None,
            elem: None,
            lifetime_pause: None,
            destroy_fx_blocked: false,
            destroy_spawn_blocked: false,
            owner,
            pitch,
            facing,
            vx,
            vy,
            late,
            input_ui: false,
            touching: false,
            has_hit_a_fighter: false,
            durability: 2.0,
            sphere_on: false,
            proj_flag: None,
            pick_r: None,
            health: 1.0,
            clanking: false,
            queue_destroy: false,
            hit_actors: IndexSet::new(),
            mobility: None,
            stationary_count: None,
            reflect: None,
            no_collision: None,
            doom: None,
            buff_home: None,
            overlapping: IndexSet::new(),
            overlapped_by: IndexSet::new(),
            big_geo: false,
            ammo: None,
            max_speed: 0.0,
            size_scale: None,
            lookup_fighter: None,
            act_at: -1,
            pawn_at: -1,
            pawn_of: None,
            cap_r: 22.0,
            cap_h: 44.0,
            lin_damp: 0.01,
            life_off: false,
            unheld_at: -1,
            grav_scale: 1.0,
            hit_sphere: 32.0,
            hurt_sphere: 130.0,
            reach: IndexSet::new(),
            idle_ticks: 0,
            blink: -1.0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: BlastBounds::default(),
            ext: ItemExt::default(),
        }
    }

    /// `new Item(cls, owner, actor, x, y, pitch, facing, vx, vy, spawn, late)`: the class data's set-up and the
    /// BeginPlay blast-box loop. Comp_Spawner.HandleActorPostSpawn without projectile movement: AddImpulse(direction
    /// x InitialSpeed + the spawning fighter's velocity / 2, bVelChange), which the caller passes as (vx, vy).
    #[allow(clippy::too_many_arguments, reason = "the constructor's parameters, in its order")]
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the negated test: a NaN life loops (native.md Numerics)")]
    pub fn make(
        w: &World,
        cls: Name,
        class: ItemClass,
        owner: Option<FighterId>,
        src: DefSrc,
        a: ItemDef,
        x: Real,
        y: Real,
        pitch: Real,
        facing: Real,
        vx: Real,
        vy: Real,
        spawn: &SpawnBox,
        late: bool,
    ) -> Self {
        let mut it = Item::new(cls, class, owner, pitch, facing, vx, vy, late);
        it.src = src;
        let p = a.p;
        it.hit = HitPacket::from_props(&w.names, p.get("HitBoxDamage").and_then(Value::as_obj).unwrap_or(&EMPTY), None);
        it.durability = num(p, "Durability").unwrap_or(2.0);
        // Every item spawn of the port has a transform at scale 1: the dispenser's MakeTransform((0, 0, 100), 0, 1)
        // (ItemDispenser_Bugs @1242), the safe's loot and a projectile's death item (SpawnedActorParent @16629: the
        // dying actor's GetTransform or SpawnedActorAtDeathTransform, both 1 for every class that has one), The
        // Court's ball (Map_M010 @15: MakeTransform((0, 0, 1500), 0, 1)) and a notify's spawn with no Scale3D of its
        // own. The templates' 1.25 (Item_Sandwich, Item_Meeseek, Item_Basketball) never apply.
        it.scale = 1.0;
        it.grav_scale = a.comp("ProjectileMovement").and_then(|c| num(c, "ProjectileGravityScale")).unwrap_or(1.0);
        it.hit_sphere = a.comp("ThrowHitBox").and_then(|c| num(c, "SphereRadius")).unwrap_or(32.0);
        it.hurt_sphere = a.comp("Sphere").and_then(|c| num(c, "SphereRadius")).unwrap_or(130.0);
        it.sphere_on = truthy(p, "CanBeHitWhileIdle");
        let name = class.name();
        // No Comp_Actor_Destructible: the class is not a Destructible, and nothing says how a hit on it goes. Warned,
        // and taken as a projectile (`is_projectile`).
        if a.comp("Comp_Actor_Destructible").is_none() {
            warn_text(w, &format!("item {name}: no Comp_Actor_Destructible (IsProjectile unknown, taken as true)"));
        }
        match a.comp("PickupArea") {
            None => warn_text(w, &format!("item {name}: no PickupArea template (taken as ItemActorParent's)")),
            Some(pa) => {
                // A turned, scaled or re-parented sphere is not read (no item class of the build has one).
                let parent = text(pa, "AttachParent");
                if truthy(pa, "RelativeScale3D")
                    || truthy(pa, "RelativeRotation")
                    || parent.is_some_and(|s| !s.is_empty() && s != "Capsule")
                {
                    warn_text(
                        w,
                        &format!("item {name}: PickupArea scaled, turned or not under the Capsule (not read)"),
                    );
                }
            }
        }
        it.health = num(p, "ItemHealth").unwrap_or(1.0);
        // UpdateTimer(true) in BeginPlay: K2_SetTimerDelegate(ItemIdleLife - ItemFadeTime). Set inside the spawn, the
        // timer starts counting after the spawn frame's timer tick (also when the spawn is inside that tick: a timer
        // set from a callback is pending and is activated at the end of the same tick) and fires at the first tick
        // with counted deltas > its rate. UpdateTimer (ubergraph 11645) sets no timer with ItemIdleLife 0
        // (Item_C017_Art).
        let idle = num(p, "ItemIdleLife").unwrap_or(ITEM_IDLE_LIFE);
        let life = idle - ITEM_FADE_TIME;
        let mut k = 0;
        while !(f64::from(k) * f64::from(DELTA_SECONDS) > f64::from(life)) {
            k += 1;
        }
        it.idle_ticks = if idle == 0.0 { -1 } else { k };
        let cap = a.comp("Capsule");
        it.cap_r = cap.and_then(|c| num(c, "CapsuleRadius")).unwrap_or(22.0);
        it.cap_h = cap.and_then(|c| num(c, "CapsuleHalfHeight")).unwrap_or(44.0);
        it.lin_damp = cap
            .and_then(|c| c.get("BodyInstance"))
            .and_then(Value::as_obj)
            .and_then(|b| num(b, "LinearDamping"))
            .unwrap_or(0.01);
        // SpawnedActorParent BeginPlay (@10738, @14195-14989): with ForceSpawnPositionInsideBlastBox (every ported
        // item class has it) a location outside the jump-out area is clamped into it.
        it.x = x;
        it.prev_x = x;
        it.y = y;
        it.prev_y = y;
        // Outside it without ForceSpawnPositionInsideBlastBox, @15066: DestroyOnExitedArena? destroys it
        // (`World.addItem`).
        // Deviation: the loop runs here, ahead of addItem's FindTeleportSpot (in the build it comes after it).
        let force = flag(p, "ForceSpawnPositionInsideBlastBox").unwrap_or(false);
        let destroy = flag(p, "DestroyOnExitedArena?").unwrap_or(true);
        let Item { x, y, prev_x, prev_y, blast_bounds, .. } = &mut it;
        let out = blast_box_loop(spawn, x, y, Some((prev_x, prev_y)), force, blast_bounds, destroy);
        if out {
            it.dead = true;
        }
        it
    }

    /// The class data it was made with (`p`, `components`).
    pub fn def<'a>(&self, data: &'a GameData) -> ItemDef<'a> {
        ItemDef::at(data, self.src)
    }

    /// `currentAmmo`.
    pub fn current_ammo(&self, p: &Props) -> Real {
        self.ammo.or_else(|| num(p, "StartingAmmo")).unwrap_or(0.0)
    }

    /// `destructibleTeam`: Comp_Actor_Destructible.TeamIndex, the spawner's (SetTeamIndexOnSpawn), then the last
    /// grabber's; on a stage item no one has picked up the component's default -1 (M010_Hoop_A_01_BP_C).
    pub fn destructible_team(&self, w: &World) -> i32 {
        self.owner.map_or(-1, |f| w.fighter(f).team.team)
    }

    /// `setSizeScale`: Comp_SizeManager SetSizeScale(s) with SizeCanScale: CurrentSizeScale = s and
    /// SetActorScale3D(s) @566, absolute. No sweep: the body's next contact pushes a grown capsule out of the floor.
    pub fn set_size_scale(&mut self, s: Real) {
        self.size_scale = Some(s);
        self.scale = s;
    }

    /// SpawnedActorParent's Capsule x the root scale.
    fn radius(&self) -> Real {
        self.cap_r * self.scale
    }

    fn half(&self) -> Real {
        max(self.cap_h - self.cap_r, 0.0) * self.scale
    }

    /// `capsuleRadius`: the Capsule's scaled radius.
    pub fn capsule_radius(&self) -> Real {
        self.cap_r * self.scale
    }

    /// `capsuleHalfHeight`.
    pub fn capsule_half_height(&self) -> Real {
        self.cap_h * self.scale
    }

    /// The PickupArea sphere's template (`PICKUP_TEMPLATE`): SphereRadius (ItemActorParent's 250 unless the class sets
    /// its own: Fastball 400, C015Anvil 200) and its RelativeLocation under the root Capsule, forward (X) and up (Z)
    /// (Item_Helper (0, 0, 40)).
    fn pickup_template(a: ItemDef) -> (Real, Real, Real) {
        let pa = a.comp("PickupArea");
        let l = pa.and_then(|c| c.get("RelativeLocation")).and_then(Value::as_obj);
        (
            pa.and_then(|c| num(c, "SphereRadius")).unwrap_or(250.0),
            l.and_then(|l| num(l, "X")).unwrap_or(0.0),
            l.and_then(|l| num(l, "Z")).unwrap_or(0.0),
        )
    }

    /// `pickupRadius`: PickupArea.SphereRadius x the root scale: the class's template until ItemActorParent's first
    /// SetSphereRadius (IdlePickUpRadius at ItemThrowHitMulticast @7694, ThrownPickUpRadius at
    /// ReleaseFromFighterMulticast's throw @10367).
    pub fn pickup_radius(&self, a: ItemDef) -> Real {
        self.pick_r.unwrap_or_else(|| Item::pickup_template(a).0) * self.scale
    }

    /// `pickupX`: the sphere's centre, the root plus the template's RelativeLocation through the root's turn (pitch,
    /// facing) and scale.
    pub fn pickup_x(&self, a: ItemDef) -> Real {
        let (_, fx, uz) = Item::pickup_template(a);
        if fx == 0.0 && uz == 0.0 {
            return self.x;
        }
        self.x + crate::math::ue::turn_by_pitch(self.pitch, [fx, uz])[0] * self.facing * self.scale
    }

    /// `pickupY`.
    pub fn pickup_y(&self, a: ItemDef) -> Real {
        let (_, fx, uz) = Item::pickup_template(a);
        if fx == 0.0 && uz == 0.0 {
            return self.y;
        }
        self.y + crate::math::ue::turn_by_pitch(self.pitch, [fx, uz])[1] * self.scale
    }

    /// `pickupOn`: the PickupArea's collision is on (a class's construction script may turn it off until its graph
    /// turns it on).
    pub fn pickup_on(&self) -> bool {
        match &self.ext {
            ItemExt::Batarang(b) => b.area_on,
            ItemExt::Gem(g) => crate::chars::finn::gem::pickup_on(g),
            // CanBePickedUp gates the PickupArea's begin overlap (Comp_ItemManager @3083). Unverified: taken as the
            // area's collision (as the Finn gem's).
            ItemExt::ProximityMine(m) => !m.no_pick,
            _ => true,
        }
    }

    /// `hitVelocity`: BPI_GetParentVelocity (SpawnedActorParentVelocity), what a VelocityInsteadOfFacing hit reads.
    pub fn hit_velocity(&self) -> V2 {
        match &self.ext {
            ItemExt::Batarang(b) => V2 { x: b.parent_vx, y: b.parent_vy },
            ItemExt::Dynamite(d) => d.boom_v.map_or(V2 { x: self.vx, y: self.vy }, |[x, y]| V2 { x, y }),
            _ => V2 { x: self.vx, y: self.vy },
        }
    }

    /// `hitRadius`.
    pub fn hit_radius(&self, p: &Props) -> Real {
        // C023B_DroppedBomb: the ExplosionHitBox sphere (300) once Detonate names it (Unverified: the 2D twin is the same
        // circle in the play plane)
        if let ItemExt::DroppedBomb(b) = &self.ext
            && b.explosion_box
        {
            return 300.0 * self.scale;
        }
        self.hit_sphere * self.scale * vec_x(p, "HitBoxScale").unwrap_or(1.0)
    }

    /// `hurtRadius`: the Sphere a hitbox can overlap (0 while it is off or the item is in a hand).
    pub fn hurt_radius(&self) -> Real {
        if self.sphere_on && self.holder < 0 && self.no_collision != Some(true) {
            self.hurt_sphere * self.scale
        } else {
            0.0
        }
    }

    /// `isProjectile`: Comp_Actor_Destructible.IsProjectile, the template's (Item_C017_Scrap / Art: false, the other
    /// ported classes: true) until a throw sets it (ubergraph 10289 / 10746) or the hit tail of a class with
    /// CanBeHitWhileIdle sets it to !CanBeMeleedWhileIdle (4941-4984).
    pub fn is_projectile(&self, a: ItemDef) -> bool {
        self.proj_flag.unwrap_or_else(|| a.comp("Comp_Actor_Destructible").is_none_or(|d| truthy(d, "IsProjectile")))
    }

    /// `setProjectile`: an own value only once a write changed it.
    fn set_projectile(&mut self, a: ItemDef, v: bool) {
        if self.proj_flag.is_some() || v != self.is_projectile(a) {
            self.proj_flag = Some(v);
        }
    }

    /// `attackType`: BPI_GetProjectileAttackType (the class default: Normal).
    pub fn attack_type(&self, p: &Props) -> i32 {
        num(p, "ProjectileAttackType").map_or(0, to_int32)
    }

    /// `canClank`: BPI_CanProjectileClank (HitBoxOn? AND CanProjectileClank).
    pub fn can_clank(&self, p: &Props) -> bool {
        self.hit_on && truthy(p, "CanProjectileClank")
    }

    /// `boxOn`: the hit box's collision: HitBoxOn?'s, or a reflect's UpdateHitBoxDamage with the variable off
    /// (activateBoxes) until the next HitBoxesOff.
    pub fn box_on(&self) -> bool {
        self.no_collision != Some(true) && (self.hit_on || self.reflect.as_ref().is_some_and(|r| r.boxes))
    }

    /// `hitBoxOff`: BPI_SetHitBoxOn(false) / HitBoxOn? = false with UpdateHitbox: the variable and every box off,
    /// and the handler's HitBoxesOff clears HasHitAFighter (@7250).
    fn hit_box_off(&mut self) {
        self.hit_on = false;
        self.has_hit_a_fighter = false;
        if let Some(r) = &mut self.reflect {
            r.boxes = false;
        }
    }

    /// `clankHit`: BPI_GetProjectileClankHitBox (HitBoxDamage).
    pub fn clank_hit(&self) -> &HitPacket {
        &self.hit
    }

    /// `pickUp`: Comp_ItemManager.EquipOnSpawn: IsHeld, the item's PickUp (ProjectileMovement.Deactivate, physics off,
    /// LifeTimer cleared, StopDeathBlink, the grabber becomes the HitBoxHandler's Instigator and the owner, actor
    /// collision off, the blast-box timer paused), then BPI_SetHitBoxOn(false) and the attachment to the grabber's
    /// hand socket. The actor does not tick its timers while it is attached here.
    pub fn pick_up(&mut self, f: FighterId, fid: i32) {
        self.held = true;
        self.holder = fid;
        self.owner = Some(f);
        self.blink = -1.0;
        self.life_off = true;
        self.unheld_at = -1;
        self.flying = false;
        self.thrown = false;
        self.hit_box_off();
        self.vx = 0.0;
        self.vy = 0.0;
    }

    /// `release`: BPI_ReleaseFromFighter: detached, K2_SetActorTransform((0, location), rotation 0, scale 1) as a
    /// teleport, collision on; IsHeld and the PickupArea come back after Delay(0.1). Thrown: SetMovementMulticast
    /// (the ProjectileMovement active with the velocity, MaxSpeed 3 x the launch speed), the hitbox on with the
    /// thrower's attack id. Dropped: physics on with the velocity.
    /// The Delay is created in the releasing fighter's mesh tick and updated by the item's own actor tick: 6 updates
    /// for 0.1 s, the first one in the release frame, so IsHeld clears in the item's tick 5 frames after the release.
    /// Unverified: that first update needs the item's actor tick to come after the fighter's mesh tick in
    /// TG_PrePhysics, which is captured for the sandwich projectile, not for the item.
    #[allow(clippy::too_many_arguments, reason = "the method's parameters, in its order")]
    pub fn release(
        &mut self,
        a: ItemDef,
        x: Real,
        y: Real,
        vx: Real,
        vy: Real,
        thrown: bool,
        attack_id: i32,
        max_speed: Option<Real>,
    ) {
        self.holder = -1;
        self.x = x;
        self.prev_x = x;
        self.y = y;
        self.prev_y = y;
        self.scale = 1.0;
        // Item_BatarangV2 Thrown @162 / Drop @7750: SetActorScale3D(Original Scale).
        if let ItemExt::Batarang(b) = &self.ext {
            self.scale = b.orig_scale;
        }
        self.pitch = 0.0;
        self.vx = vx;
        self.vy = vy;
        self.unheld_at = self.age + to_int32(crate::fighter::common::latent_frames(0.1)) - 1;
        if !thrown {
            return;
        }
        self.flying = true;
        self.thrown = true;
        self.hit_on = true;
        self.sphere_on = true;
        self.set_projectile(a, true); // ubergraph 10289
        self.pick_r = Some(num(a.p, "ThrownPickUpRadius").unwrap_or(130.0)); // @10367
        self.hit_set.clear();
        self.hit_actors.clear();
        self.attack_id = attack_id;
        // Deviation: Batarang.release drops the buffed MaxSpeed the base class takes: a
        // re-thrown batarang under a projectile-speed buff keeps 3 x its launch speed.
        let max_speed = if matches!(self.ext, ItemExt::Batarang(_)) { None } else { max_speed };
        self.max_speed = max_speed.unwrap_or_else(|| size2d([vx, vy]) * 3.0);
        self.follow();
    }

    /// `follow`: bRotationFollowsVelocity. The hit's side is read from facing: a velocity without a Y part has yaw 0.
    fn follow(&mut self) {
        self.facing = if self.vx > 0.0 { 1.0 } else { -1.0 };
        // Velocity.ToOrientationRotator(): FVector::Rotation's pitch.
        self.pitch = crate::math::ue::vector_rotation([0.0, self.vx.abs(), self.vy])[0];
    }

    /// `throwHit`: ItemThrowHit(HitPlayer, Enemy) -> ItemThrowHitMulticast: IsThrown off (the trail hidden, Platform
    /// blocking again, the pickup radius back to IdlePickUpRadius). A class that overrides ItemThrowHit has the call
    /// kept with the velocity of this moment for the world to run behind its caller.
    /// Deviation: the build runs the child's part inside the parent's tail, ahead of its Durability < 1 destroy; here
    /// it comes behind the tail. `q`: the World's queue (`Scratch::throw_hits`).
    pub fn throw_hit(&mut self, me: ItemId, p: &Props, hit_player: bool, enemy: bool, q: &mut Vec<ThrowHitCall>) {
        // Anvil's override: ItemThrowHitMulticast @7620 makes its capsule block the platforms.
        if let ItemExt::Anvil(a) = &mut self.ext {
            a.platforms_on = true;
        }
        // Item_BouncePad_C's HitAlly (World.itemHits' throwHit(true, false)): the deploy in place of the parent's
        // ItemThrowHit.
        if let ItemExt::BouncePadItem(b) = &mut self.ext
            && hit_player
            && !enemy
            && b.can_deploy
        {
            b.deploying = true;
            return;
        }
        self.thrown = false;
        self.pick_r = Some(num(p, "IdlePickUpRadius").unwrap_or(250.0)); // @7694
        if item_hooks(crate::registry::registry(), self.class).is_some_and(|h| h.throw_hit.is_some()) {
            q.push(ThrowHitCall { it: me, hit_player, enemy, vx: self.vx, vy: self.vy });
        }
    }

    /// `spend`: HitEnemyPlayer / HitNonPlayer / ReceiveHit with the hitbox on, then the common tail: Durability - 1,
    /// the hitbox off; below 1 the actor is destroyed (true), else the ProjectileMovement stops, physics comes on and
    /// ItemHitGroundImpulse is added as a velocity change: -velocity + (0, the side opposite the velocity's x 0.5, 1)
    /// x OnHitBounce. The tail starts (4941) with the Sphere: off again unless CanBeHitWhileIdle, which sets
    /// IsProjectile to !CanBeMeleedWhileIdle instead (4984). Below 1 while IsProjectileClanking (5073):
    /// QueueDestroyAfterClank instead.
    /// Unverified: the velocity the impulse reads is taken as the flight's (ItemHitGroundImpulse :938 reads
    /// GetVelocity() right after SetSimulatePhysics(true), which may be the body's).
    /// enemy: the hit was HitEnemyPlayer's.
    pub fn spend(&mut self, me: ItemId, a: ItemDef, enemy: bool, q: &mut Vec<ThrowHitCall>) -> bool {
        match &mut self.ext {
            // Item_BouncePad_C: with CanDeploy the hit asks for the deploy and the release (true: the item is gone).
            ItemExt::BouncePadItem(b) if b.can_deploy => {
                b.deploying = true;
                return true;
            }
            // Item_ProximityMine_C: with CanActivate the class's ReceiveHit replaces the parent's, so the flight's
            // blocking hit is not spent; the receive_hit hook arms the mine.
            ItemExt::ProximityMine(m) if m.in_step && m.can_activate => return false,
            _ => {}
        }
        self.durability -= 1.0;
        self.hit_box_off();
        // HitEnemyPlayer's (true, true); ReceiveHit's, ReceiveAnyDamage's, HitNonPlayer's (false, false)
        self.throw_hit(me, a.p, enemy, enemy, q);
        if !truthy(a.p, "CanBeHitWhileIdle") {
            self.sphere_on = false;
        } else {
            self.set_projectile(a, !truthy(a.p, "CanBeMeleedWhileIdle"));
        }
        if self.durability < 1.0 {
            return self.destroy_or_queue();
        }
        self.flying = false;
        let b = num(a.p, "OnHitBounce").unwrap_or(500.0);
        let side = if self.vx == 0.0 {
            0.0
        } else if self.vx < 0.0 {
            0.5
        } else {
            -0.5
        };
        self.vx = side * b;
        self.vy = b;
        false
    }

    /// `receiveDamage`: ReceiveAnyDamage (ubergraph 7785-8518): needs CanEnemyDestroy and a causer with a
    /// Comp_Actor_Destructible; a causer of the item's own team is dropped only with RestrictedToTeam. ItemHealth -
    /// Damage <= 0: DeathFromDamageVisuals and ReleaseToPool (true: destroyed). Else, with the hitbox on and Damage >
    /// 0, the hit is spent as a blocking hit is. While IsProjectileClanking the destroy is queued (8226).
    /// UGameplayStatics::ApplyDamage does not call it with a damage of 0.
    pub fn receive_damage(
        &mut self,
        me: ItemId,
        a: ItemDef,
        dmg: Real,
        same_team: bool,
        q: &mut Vec<ThrowHitCall>,
    ) -> bool {
        // Gem.receiveDamage: the class's own ReceiveAnyDamage (@8395, no parent call): a causer of another team destroys
        // it. The world's callers have already refused a causer of its team (the gem's component has no FriendlyFire).
        // Unverified: the causer's Comp_Actor_Destructible is taken as there (a fighter's is; a spawned causer's is not read).
        if matches!(self.ext, ItemExt::Gem(_)) {
            return dmg != 0.0;
        }
        if dmg == 0.0 || !flag(a.p, "CanEnemyDestroy").unwrap_or(true) || (same_team && truthy(a.p, "RestrictedToTeam"))
        {
            return false;
        }
        self.health -= dmg;
        if self.health <= 0.0 {
            return self.destroy_or_queue();
        }
        self.hit_on && dmg > 0.0 && self.spend(me, a, false, q)
    }

    /// `hitNonPlayer`: HitNonPlayer(HitActor) (ubergraph 9018): with the hitbox on and the other actor's attack type
    /// not Light, the hit is spent as in ReceiveHit.
    pub fn hit_non_player(&mut self, me: ItemId, a: ItemDef, other_type: i32, q: &mut Vec<ThrowHitCall>) -> bool {
        other_type != 2 && self.hit_on && self.spend(me, a, false, q)
    }

    fn destroy_or_queue(&mut self) -> bool {
        if !self.clanking {
            return true;
        }
        self.queue_destroy = true;
        false
    }

    /// `hidden`: DeathWarning: SetActorHiddenInGame(Blink < 0.5) while the timeline plays.
    pub fn hidden(&self) -> bool {
        if let ItemExt::Gem(g) = &self.ext
            && crate::chars::finn::gem::fade_hidden(g)
        {
            return true; // Gem.hidden: fadeHidden || super.hidden
        }
        let t = self.blink;
        t >= 0.0 && BLINK_HIDDEN.iter().any(|&[a, b]| t >= a && t < b)
    }

    /// `step`: one physics step plus the actor's timers; why it was destroyed, if it was. `platforms`: the boxes of
    /// `World.blockers('item')`. The capsule's profile Item blocks them from every side (the one-way rule is the
    /// fighters'); a throw sets the Platform response to Ignore until ItemThrowHitMulticast. `contacts`: cleared,
    /// then the root Capsule's blocking contacts of the step ([`ItemContact`]: the hit normal out of the box, towards
    /// the item, and the box's stage solid), for the class's own OnComponentHit (`ItemKind.componentHit`).
    /// `q`: the World's ItemThrowHit queue (`Scratch::throw_hits`). `accel`: the move's homing acceleration, a buff's
    /// or the class's own, as the world works it out ([`item_home_accel`]).
    /// Unverified: a body contact is one per box per physics step while it touches (PhysX's found and persisting touch
    /// pairs taken as UE 4.26's contact notify sends them, not read in the exe).
    #[allow(clippy::too_many_arguments, reason = "the step's parameters and the homing the world works out")]
    pub fn step(
        &mut self,
        me: ItemId,
        a: ItemDef,
        ground: &Ground,
        platforms: &[ItemBox],
        contacts: &mut Vec<ItemContact>,
        accel: Option<V2>,
        q: &mut Vec<ThrowHitCall>,
    ) -> ItemStep {
        // Item_ProximityMine_C's step override: InStep around the parent's, then an attached mine's velocity zeroed.
        if let ItemExt::ProximityMine(m) = &mut self.ext {
            m.in_step = true;
        }
        let out = self.step_parent(me, a, ground, platforms, contacts, accel, q);
        if let ItemExt::ProximityMine(m) = &mut self.ext {
            m.in_step = false;
            if m.attached {
                self.vx = 0.0;
                self.vy = 0.0;
            }
        }
        out
    }

    /// `Item.step` of the parent.
    #[allow(clippy::too_many_arguments, reason = "the step's parameters and the homing the world works out")]
    fn step_parent(
        &mut self,
        me: ItemId,
        a: ItemDef,
        ground: &Ground,
        platforms: &[ItemBox],
        contacts: &mut Vec<ItemContact>,
        accel: Option<V2>,
        q: &mut Vec<ThrowHitCall>,
    ) -> ItemStep {
        let mut out = ItemStep::default();
        contacts.clear();
        if self.dead {
            return out;
        }
        // Anvil's override: no platforms until its first ItemThrowHit.
        let platforms: &[ItemBox] = match &self.ext {
            ItemExt::Anvil(a) if !a.platforms_on => &[],
            _ => platforms,
        };
        self.prev_x = self.x;
        self.prev_y = self.y;
        let frame = self.age; // frames since the spawn frame, less the ones spent in a hand
        self.age += 1;
        if frame == self.unheld_at {
            self.held = false;
        }
        // The DeathBlinkWarning timeline ticks with the actor (TG_PrePhysics): position + delta in float32; past the
        // length (3 s) it is clamped, Update runs and Finished pools the actor.
        if self.blink >= 0.0 {
            self.blink += DELTA_SECONDS;
            if self.blink > ITEM_FADE_TIME {
                self.blink = ITEM_FADE_TIME;
                self.dead = true;
                out.end = Some(ItemEnd::Life);
                return out;
            }
        }
        // The physics scene steps between TG_StartPhysics and TG_EndPhysics. An item spawned in a tick group before it
        // is stepped in its spawn frame; one spawned in the timer tick is first stepped in the next frame.
        // Unverified: not captured.
        let ground = ItemBox { g: *ground, is_static: false, kin: None, solid: None, poly: None };
        if self.mobility.is_some() {
            // A root not Movable: the movement's tick skips and the body is held.
        } else if self.flying {
            self.touching = false;
            // Item_ProximityMine_C's fly override (SetCollisionResponseToChannel(20, Block) at Thrown): the Platform
            // boxes block the flight (IsThrown ignored them).
            let was_thrown = self.thrown;
            if matches!(&self.ext, ItemExt::ProximityMine(m) if m.can_activate) {
                self.thrown = false;
            }
            let blocked = self.fly(&ground, platforms, contacts, accel);
            self.thrown = was_thrown;
            if blocked {
                out.blocked = true;
                if self.spend(me, a, false, q) {
                    self.dead = true;
                    out.end = Some(ItemEnd::Hit);
                    return out;
                }
            }
        } else if !(self.late && frame == 0) {
            let dt = DT;
            self.vy -= GRAVITY_Z * dt;
            let damp = max(0.0, 1.0 - self.lin_damp * dt);
            self.vx *= damp;
            self.vy *= damp;
            self.x += self.vx * dt;
            self.y += self.vy * dt;
            if self.no_collision != Some(true) {
                self.collide(&ground, contacts);
                for p in platforms {
                    self.collide(p, contacts);
                }
                // The body's contact is a ReceiveHit too (bNotifyRigidBodyCollision on the Capsule): ItemActorParent
                // @4579-4915 spends it as the flight's with HitBoxOn?. The first contact spends it; the hit box is off
                // for the next ones.
                // Unverified: only a new contact (none in the last step) is spent: PhysX sends no report for a
                // resting, sleeping body's persisting contact; the sleep itself is not modelled.
                let fresh = !contacts.is_empty() && !self.touching;
                self.touching = !contacts.is_empty();
                if self.hit_on && fresh {
                    out.blocked = true;
                    if self.spend(me, a, false, q) {
                        self.dead = true;
                        out.end = Some(ItemEnd::Hit);
                        return out;
                    }
                }
            }
        }
        // The world's timer tick. LifeTimer -> DeathWarning: DeathBlinkWarning.PlayFromStart (position 0, the actor
        // shown); the timeline first advances in the next frame.
        if !self.life_off && frame == self.idle_ticks {
            self.blink = 0.0;
        }
        // CheckBlastBoxExit on the 0.1 s looping timer (DestroyOnExitedArena?), set in BeginPlay: it does not count
        // the spawn frame.
        let due = f64::from(frame) * f64::from(DELTA_SECONDS) > self.blast_at;
        if due {
            self.blast_at += f64::from(0.1_f32);
        }
        if due
            && flag(a.p, "DestroyOnExitedArena?").unwrap_or(true)
            && check_blast_box_exit(self.x, self.y, &self.blast_bounds)
        {
            self.dead = true;
            out.end = Some(ItemEnd::Blast);
        }
        out
    }

    /// `fly`: the thrown item's ProjectileMovement (ProjectileGravityScale 3, no bounce): ComputeVelocity (v + a dt,
    /// limited to MaxSpeed) and ComputeMoveDelta (v dt + (v' - v) dt / 2). The root capsule blocks the floor; the
    /// sweep is sampled every 10 u and bisected to the contact. True on a blocking hit (ReceiveHit). Objects of
    /// type Platform are ignored while IsThrown (ubergraph 10146) and block again from ItemThrowHitMulticast (7620);
    /// a move that starts inside one is a blocking hit at once. A class whose ProjectileMovement bounces
    /// (bShouldBounce) has its own flight (`actors::item_bounce`).
    fn fly(&mut self, b: &ItemBox, platforms: &[ItemBox], contacts: &mut Vec<ItemContact>, accel: Option<V2>) -> bool {
        if super::item_bounce::bounce_of(self).is_some() {
            return super::item_bounce::fly(self, b, platforms, contacts, accel);
        }
        let dt = 1.0 / 60.0;
        let mut nvx = self.vx;
        let mut nvy = self.vy - GRAVITY_Z * self.grav_scale * dt;
        // bIsHomingProjectile: ComputeAcceleration adds ComputeHomingAcceleration to the gravity. A buff's write of
        // the component's homing (buff_home) is the move's, ahead of the class's own; the world works it out (it reads
        // the target) and passes it in as accel ([`item_home_accel`]).
        if let Some(h) = accel {
            nvx += h.x * dt;
            nvy += h.y * dt;
        }
        (nvx, nvy) = super::projectile_move::limit_velocity(nvx, nvy, self.max_speed);
        let dx = self.vx * dt + (nvx - self.vx) * 0.5 * dt;
        let dy = self.vy * dt + (nvy - self.vy) * 0.5 * dt;
        self.vx = nvx;
        self.vy = nvy;
        self.follow();
        let (x0, y0) = (self.x, self.y);
        if self.blocked(b, platforms, x0, y0) {
            self.hit_normal(b, platforms, x0, y0, contacts);
            return true;
        }
        let n = max(1.0, (size2d([dx, dy]) / 10.0).ceil());
        let mut i = 1.0;
        while i <= n {
            if !self.blocked(b, platforms, x0 + (dx * i) / n, y0 + (dy * i) / n) {
                i += 1.0;
                continue;
            }
            let mut lo = (i - 1.0) / n;
            let mut hi = i / n;
            for _ in 0..12 {
                let m = (lo + hi) / 2.0;
                if self.blocked(b, platforms, x0 + dx * m, y0 + dy * m) {
                    hi = m;
                } else {
                    lo = m;
                }
            }
            self.x = x0 + dx * lo;
            self.y = y0 + dy * lo;
            self.hit_normal(b, platforms, x0 + dx * hi, y0 + dy * hi, contacts);
            return true;
        }
        self.x += dx;
        self.y += dy;
        false
    }

    /// `writeHoming`: ProjectileMovement.bIsHomingProjectile as a graph outside the class writes it. The base class
    /// keeps no flag of its own; Item_BatarangV2 keeps it (its OnReturn reads it).
    pub fn write_homing(&mut self, on: bool) {
        if let ItemExt::Batarang(b) = &mut self.ext {
            b.homing = on;
        }
    }

    /// `movementActive`: ProjectileMovement.IsActive() (the flight).
    pub fn movement_active(&self) -> bool {
        self.flying
    }

    /// `simulatingPhysics`: Capsule.IsSimulatingPhysics() (lying, not in a hand).
    pub fn simulating_physics(&self) -> bool {
        !self.flying && self.holder < 0
    }

    /// `gravityScale`.
    pub fn gravity_scale(&self) -> Real {
        self.grav_scale
    }

    /// `setGravityScale`.
    pub fn set_gravity_scale(&mut self, s: Real) {
        self.grav_scale = s;
    }

    /// (IsThrown ignores the Platform channel alone: a stage solid of object type WorldStatic blocks the throw as the
    /// block does.)
    fn blocked(&self, b: &ItemBox, platforms: &[ItemBox], x: Real, y: Real) -> bool {
        if self.no_collision == Some(true) {
            return false;
        }
        if self.touches(b, x, y) {
            return true;
        }
        platforms.iter().any(|p| !(self.thrown && !p.is_static) && self.touches(p, x, y))
    }

    /// `hitNormal`: the flight's blocking hit at (x, y): Hit.Normal as the normal of the deepest touching sample of
    /// the boxes the flight meets there, out of the box.
    fn hit_normal(&self, b: &ItemBox, platforms: &[ItemBox], x: Real, y: Real, contacts: &mut Vec<ItemContact>) {
        let mut best = [-1.0, 0.0, 0.0];
        let mut solid = b.solid;
        self.deepest(b, x, y, &mut best);
        for p in platforms {
            if self.thrown && !p.is_static {
                continue;
            }
            let was = best[0];
            self.deepest(p, x, y, &mut best);
            if best[0] > was {
                solid = p.solid;
            }
        }
        if best[0] > 0.0 {
            contacts.push(ItemContact { nx: best[1], ny: best[2], solid });
        }
    }

    /// `deepest`: the deepest sample of the capsule at (x, y) inside the box's reach, into out ([depth, nx, ny]) when
    /// deeper than out[0]; collide's rule (a sample inside the box leaves through the top).
    pub(crate) fn deepest(&self, b: &ItemBox, x: Real, y: Real, out: &mut [Real; 3]) {
        let [ux, uy] = crate::math::ue::turn_by_pitch(self.pitch, [0.0, 1.0]);
        let ux = ux * self.facing;
        let r = self.radius();
        for i in 0..=8 {
            let s = -1.0 + f64::from(i) / 4.0;
            let px = x + ux * self.half() * crate::math::narrow(s);
            let py = y + uy * self.half() * crate::math::narrow(s);
            let (cx, cy) = b.nearest(px, py);
            let (dx, dy) = (px - cx, py - cy);
            let d = size2d([dx, dy]);
            if d > r {
                continue;
            }
            let k = if d > 0.0 { r - d } else { r + b.below_top(px, py) };
            if k <= out[0] {
                continue;
            }
            out[0] = k;
            out[1] = if d > 0.0 { dx / d } else { 0.0 };
            out[2] = if d > 0.0 { dy / d } else { 1.0 };
        }
    }

    pub(crate) fn touches(&self, b: &ItemBox, x: Real, y: Real) -> bool {
        let [ux, uy] = crate::math::ue::turn_by_pitch(self.pitch, [0.0, 1.0]);
        let ux = ux * self.facing;
        let r = self.radius();
        for i in 0..=8 {
            let s = -1.0 + f64::from(i) / 4.0;
            let px = x + ux * self.half() * crate::math::narrow(s);
            let py = y + uy * self.half() * crate::math::narrow(s);
            let (cx, cy) = b.nearest(px, py);
            let (qx, qy) = (cx - px, cy - py);
            if qx * qx + qy * qy <= r * r {
                return true;
            }
        }
        false
    }

    /// `collide`: the capsule (axis tilted by the spawn pitch) against a box: push out along the deepest contact's
    /// normal, bounce above the threshold, Coulomb friction on the tangent. Restitution 0.3 and friction 0.7 are the
    /// engine default physical material (UPhysicalMaterial's constructor at 1421226aa), Average combine and the 200
    /// cm/s bounce threshold are in DefaultEngine.ini. A stage piece a stage's code moves is a kinematic body in this
    /// step, at its last pass's move / dt: the bounce and the friction act on the velocity relative to it.
    /// Deviation: the PhysX contact itself is approximated: the rotation is held at the spawn rotation
    /// (AngularDamping 600).
    /// Unverified: the box is met where the pass left it (the step's end pose), not where the step starts it.
    fn collide(&mut self, b: &ItemBox, contacts: &mut Vec<ItemContact>) {
        let [ux, uy] = crate::math::ue::turn_by_pitch(self.pitch, [0.0, 1.0]);
        let ux = ux * self.facing;
        let r = self.radius();
        let (mut pen, mut nx, mut ny) = (0.0, 0.0, 0.0);
        for i in 0..=8 {
            let s = -1.0 + f64::from(i) / 4.0;
            let px = self.x + ux * self.half() * crate::math::narrow(s);
            let py = self.y + uy * self.half() * crate::math::narrow(s);
            let (cx, cy) = b.nearest(px, py);
            let (dx, dy) = (px - cx, py - cy);
            let d = size2d([dx, dy]);
            if d >= r {
                continue;
            }
            // A sample inside the box leaves through the top.
            let k = if d > 0.0 { r - d } else { r + b.below_top(px, py) };
            if k <= pen {
                continue;
            }
            pen = k;
            nx = if d > 0.0 { dx / d } else { 0.0 };
            ny = if d > 0.0 { dy / d } else { 1.0 };
        }
        if pen <= 0.0 {
            return;
        }
        contacts.push(ItemContact { nx, ny, solid: b.solid }); // the body's contact (OnComponentHit)
        self.x += nx * pen;
        self.y += ny * pen;
        let (bvx, bvy) = b.kin.map_or((0.0, 0.0), |k| (k.x / DT, k.y / DT));
        let vn = (self.vx - bvx) * nx + (self.vy - bvy) * ny;
        if vn >= 0.0 {
            return;
        }
        let e = if -vn > 200.0 { 0.3 } else { 0.0 };
        let jn = -(1.0 + e) * vn;
        self.vx += nx * jn;
        self.vy += ny * jn;
        let (tx, ty) = (-ny, nx);
        let vt = (self.vx - bvx) * tx + (self.vy - bvy) * ty;
        let jt = min(vt.abs(), 0.7 * jn) * sign(vt);
        self.vx -= tx * jt;
        self.vy -= ty * jt;
    }

    /// `overlapsFighter`: the PickupArea (a sphere at pickupX / pickupY) against a fighter's root capsule (its x, its
    /// actor Y, its capsule's half height and radius).
    pub fn overlaps_fighter(&self, a: ItemDef, fx: Real, actor_y: Real, half: Real, rad: Real) -> bool {
        if self.no_collision == Some(true) {
            return false;
        }
        let x = self.pickup_x(a);
        let y = self.pickup_y(a);
        let seg = half - rad;
        let dy = y - max(actor_y - seg, min(y, actor_y + seg));
        let r = self.pickup_radius(a) + rad;
        (x - fx) * (x - fx) + dy * dy <= r * r
    }
}

/// What a subclass of `Item` adds, a variant per class. `Plain` is `Item` itself.
#[derive(State, Debug, Default)]
pub enum ItemExt {
    #[default]
    Plain,
    // BouncingItem's subclasses
    /// FinnDashThrownTarget_C.
    Gem(crate::chars::finn::gem::Gem),
    /// C023B_DroppedBomb_C.
    DroppedBomb(crate::chars::stripe::bomb::DroppedBomb),
    // chars/
    /// `Batarang`.
    Batarang(crate::chars::batman::batarang::BatarangState),
    /// `Fastball`.
    Fastball(crate::chars::creature::fastball::Fastball),
    /// `Anvil`.
    Anvil(crate::chars::taz::perks::Anvil),
    // items/
    /// Item_BouncePad_C.
    BouncePadItem(crate::items::bounce_pad::BouncePadItem),
    /// `Dynamite`: Item_Dynamite_C.
    Dynamite(crate::items::dynamite::Dynamite),
    /// Item_Helper_C.
    HelperItem(crate::items::helper::HelperItem),
    /// Item_Meeseek_C.
    MeeseekItem(crate::items::meeseeks::MeeseekItem),
    /// Item_Mystery_C.
    MysteryItem(crate::items::mystery::MysteryItem),
    /// Item_ProximityMine_C.
    ProximityMine(crate::items::proximity_mine::ProximityMine),
    /// Item_Safe_C.
    SafeItem(crate::items::safe::SafeItem),
    /// Item_Snowball_C and Item_SuperSnowball_C.
    Snowball(crate::items::snowball::Snowball),
    // stages/
    /// `Basketball`.
    Basketball(crate::stages::the_court::ball::Basketball),
}

/// `ThrowHitCall`: an ItemThrowHit call of the parent's graph on an item whose class overrides it, with
/// SpawnedActorParentVelocity at the call.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ThrowHitCall {
    pub it: ItemId,
    pub hit_player: bool,
    pub enemy: bool,
    pub vx: Real,
    pub vy: Real,
}

// ---- the call surface of the items

/// An item with an owner (one a fighter's Comp_Spawner spawned, or a stage item a
/// fighter has picked up since).
pub fn is_owned(w: &World, it: ItemId) -> bool {
    w.actors.items.get(it).is_some_and(|i| i.owner.is_some())
}

/// Queues `f`'s use of the item.
pub fn queue_item_use(w: &mut World, it: ItemId, f: FighterId) {
    w.scratch.item_uses.push((it, f));
}

/// The queued calls into `out`, the queue emptied.
pub fn take_throw_hits(w: &mut World, out: &mut Vec<ThrowHitCall>) {
    out.append(&mut w.scratch.throw_hits);
}

/// `f`'s queued uses (every one with `None`) into `out`, in their order.
pub fn take_item_uses(w: &mut World, f: Option<FighterId>, out: &mut Vec<(ItemId, FighterId)>) {
    w.scratch.item_uses.retain(|&u| {
        if f.is_some_and(|f| u.1 != f) {
            return true;
        }
        out.push(u);
        false
    });
}

/// Empties the throw-hit and use queues.
pub fn clear_item_queues(w: &mut World) {
    w.scratch.throw_hits.clear();
    w.scratch.item_uses.clear();
}

/// `PARENT_ACTIVATE`: the item classes that can be used and whose graph has no ActivateObject of its own (read in
/// their pseudo): ItemActorParent's runs for them.
pub const PARENT_ACTIVATE: [ItemClass; 2] = [ItemClass::ItemC017Scrap, ItemClass::ItemC017Art];

/// `itemActivateParent`: ItemActorParent ActivateObject (ubergraph 5425-6197), the parent's body a class's own
/// ActivateObject calls or the whole of it for a class without one:
/// - CanUseItem: StartingAmmo 0 -> UseItem (empty in ItemActorParent) @5497, then the durability: Durability > 0 ->
///   (CurrentAmmo 0 and FragileAfterSpent -> Durability = 1 @5613); else DestroyWhenSpent -> ReleaseToPool @5651,
///   else the FragileAfterSpent test as above. StartingAmmo not 0: the item's ActivationConditions empty or holding a
///   true one, and CurrentAmmo > 0 -> CurrentAmmo - 1, then the UseItem path above; else ItemUseFail @6031.
/// - Without CanUseItem: IsHeld -> LastHolder's Comp_ItemManager.ThrowItem((0, 0, 0), false, (0, 0)) @6197.
///
/// True when it destroyed the item.
pub fn item_activate_parent(w: &mut World, it: ItemId) -> bool {
    let data = std::sync::Arc::clone(&w.data);
    let Some(i) = w.actors.items.get_mut(it) else { return false };
    let p = i.def(&data).p;
    if !flag(p, "CanUseItem").unwrap_or(true) {
        // a held item has its holder for owner
        if let (true, Some(o)) = (i.held, i.owner) {
            crate::fighter::match_side::throw_item(w, o, 0.0, 0.0, None);
        }
        return false;
    }
    let start = num(p, "StartingAmmo").unwrap_or(0.0);
    if start != 0.0 {
        let conds = p.get("ActivationConditions").and_then(Value::as_arr).unwrap_or(&[]);
        let open = conds.is_empty() || conds.iter().any(|c| c.as_bool() == Some(true));
        if !(open && i.current_ammo(p) > 0.0) {
            return false; // ItemUseFail
        }
        i.ammo = Some(i.current_ammo(p) - 1.0);
    }
    // UseItem (empty).
    let fragile = flag(p, "FragileAfterSpent").unwrap_or(true);
    if i.durability > 0.0 {
        if i.current_ammo(p) == 0.0 && fragile {
            i.durability = 1.0;
        }
        return false;
    }
    if truthy(p, "DestroyWhenSpent") {
        crate::world::items::destroy_item(w, it);
        return true;
    }
    if fragile {
        i.durability = 1.0; // @5637 -> @5603: no ammo test on this path
    }
    false
}

/// Item_Sandwich_C's `thrown`: its own graph adds Thrown @1386 -> SetVisualSize @1340: the Geo
/// at world scale 2, never set back.
fn sandwich_thrown(w: &mut World, it: ItemId) {
    if let Some(i) = w.actors.items.get_mut(it) {
        i.big_geo = true;
    }
}

/// The `ACTORS` entries of the shared item classes.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    use crate::actors::ActorClass;
    use crate::registry::ActorEntry;
    r.actors.enter(
        ItemClass::ItemSandwich.name(),
        ActorEntry {
            class: ActorClass::Item(ItemClass::ItemSandwich),
            parents: &[],
            foreign_owner: false,
            ownerless: false,
            keep_at_lab_reset: false,
            spawned_meet_bodies: false,
            calls_parent_hit_anything: false,
            elemental: None,
            hooks: ActorHooks::Item(ItemHooks { thrown: Some(sandwich_thrown), ..ItemHooks::default() }),
            carry: None,
            mesh: None,
            hazard: None,
            follower: None,
        },
    )
}

/// Whether the item's ProjectileMovement is active.
pub fn item_movement_active(w: &World, it: ItemId) -> bool {
    w.actors.items.get(it).expect("a live item").movement_active()
}

/// Whether the item's Capsule simulates physics.
pub fn item_simulating_physics(w: &World, it: ItemId) -> bool {
    w.actors.items.get(it).expect("a live item").simulating_physics()
}

/// Sets the item's size scale.
pub fn item_set_size_scale(w: &mut World, it: ItemId, s: Real) {
    w.actors.items.get_mut(it).expect("a live item").set_size_scale(s);
}

/// Writes the item's ProjectileMovement homing flag.
pub fn item_write_homing(w: &mut World, it: ItemId, on: bool) {
    w.actors.items.get_mut(it).expect("a live item").write_homing(on);
}

/// the PickupArea sphere (`pickupX`, `pickupY`, `pickupRadius`) while `pickupOn`
/// (a class overrides it: the Batarang, Finn's gem, the proximity mine); `None` while off.
pub fn pickup_area(w: &World, it: ItemId) -> Option<(Real, Real, Real)> {
    let i = w.actors.items.get(it).expect("a live item");
    if !i.pickup_on() {
        return None;
    }
    let d = i.def(&w.data);
    Some((i.pickup_x(d), i.pickup_y(d), i.pickup_radius(d)))
}

/// The item's hit radius. A class's own (Stripe's bomb's explosion box) comes with its port.
pub fn item_hit_radius(w: &World, it: ItemId) -> Real {
    let i = w.actors.items.get(it).expect("a live item");
    i.hit_radius(i.def(&w.data).p)
}

/// The Sphere a hitbox can overlap (0 while it is off or held).
pub fn item_hurt_radius(w: &World, it: ItemId) -> Real {
    w.actors.items.get(it).expect("a live item").hurt_radius()
}

/// The hit box's collision.
pub fn item_box_on(w: &World, it: ItemId) -> bool {
    w.actors.items.get(it).expect("a live item").box_on()
}

/// Comp_Actor_Destructible.IsProjectile, the item's current one.
pub fn item_is_projectile(w: &World, it: ItemId) -> bool {
    let i = w.actors.items.get(it).expect("a live item");
    i.is_projectile(i.def(&w.data))
}

/// BPI_CanProjectileClank.
pub fn item_can_clank(w: &World, it: ItemId) -> bool {
    let i = w.actors.items.get(it).expect("a live item");
    i.can_clank(i.def(&w.data).p)
}

/// Comp_Actor_Destructible.TeamIndex.
pub fn item_destructible_team(w: &World, it: ItemId) -> i32 {
    w.actors.items.get(it).expect("a live item").destructible_team(w)
}

/// ReceiveAnyDamage; true is its destroy.
pub fn item_receive_damage(w: &mut World, it: ItemId, dmg: Real, same_team: bool) -> bool {
    let data = std::sync::Arc::clone(&w.data);
    let i = w.actors.items.get_mut(it).expect("a live item");
    let gone = i.receive_damage(it, i.def(&data), dmg, same_team, &mut w.scratch.throw_hits);
    // Item_Dynamite_C ReceiveAnyDamage @2271: the parent's, then ReleaseToPool @2317 for any damage (ApplyDamage makes no
    // call for 0).
    gone || (dmg != 0.0 && matches!(i.ext, ItemExt::Dynamite(_)))
}

/// The parent's HitNonPlayer; true is its destroy.
pub fn item_hit_non_player(w: &mut World, it: ItemId, other_type: i32) -> bool {
    let data = std::sync::Arc::clone(&w.data);
    let i = w.actors.items.get_mut(it).expect("a live item");
    i.hit_non_player(it, i.def(&data), other_type, &mut w.scratch.throw_hits)
}

/// ItemThrowHitMulticast.
pub fn item_throw_hit(w: &mut World, it: ItemId, hit_player: bool, enemy: bool) {
    let data = std::sync::Arc::clone(&w.data);
    let i = w.actors.items.get_mut(it).expect("a live item");
    i.throw_hit(it, i.def(&data).p, hit_player, enemy, &mut w.scratch.throw_hits);
}

/// A hit's use of the item; true is its destroy.
pub fn item_spend(w: &mut World, it: ItemId, enemy: bool) -> bool {
    let data = std::sync::Arc::clone(&w.data);
    let i = w.actors.items.get_mut(it).expect("a live item");
    // Snowball's override: HitEnemyPlayer spends nothing.
    if enemy && matches!(i.ext, ItemExt::Snowball(_)) {
        return false;
    }
    i.spend(it, i.def(&data), enemy, &mut w.scratch.throw_hits)
}

/// The item's ProjectileMovement gravity scale.
pub fn item_gravity_scale(w: &World, it: ItemId) -> Real {
    w.actors.items.get(it).expect("a live item").gravity_scale()
}

/// Sets the item's ProjectileMovement gravity scale.
pub fn item_set_gravity_scale(w: &mut World, it: ItemId, s: Real) {
    w.actors.items.get_mut(it).expect("a live item").set_gravity_scale(s);
}

/// BPI_GetProjectileAttackType, the class default.
pub fn item_attack_type(w: &World, it: ItemId) -> i32 {
    let i = w.actors.items.get(it).expect("a live item");
    i.attack_type(i.def(&w.data).p)
}

/// `homeAccel` of the item's move: a buff's homing (`home_toward` reads its target) ahead of the class's own
/// (`None`: bIsHomingProjectile off, as for every class but Item_BatarangV2).
pub fn item_home_accel(w: &World, it: ItemId) -> Option<V2> {
    let i = w.actors.items.get(it).expect("a live item");
    match (&i.buff_home, &i.ext) {
        (Some(h), _) => crate::actors::homing::home_toward(w, i.x, i.y, h),
        (None, ItemExt::Batarang(_)) => crate::chars::batman::batarang::home_accel(w, it),
        (None, _) => None,
    }
}

/// The item's own On Hit Buffs rows where its graph wrote them (Item_BatarangV2's StartReturn); `None`: the class's.
pub fn item_own_rows(w: &World, it: ItemId) -> Option<Vec<crate::combat::OnHitBuffRow>> {
    match &w.actors.items.get(it)?.ext {
        ItemExt::Batarang(_) => crate::chars::batman::batarang::own_rows(w, it),
        _ => None,
    }
}
