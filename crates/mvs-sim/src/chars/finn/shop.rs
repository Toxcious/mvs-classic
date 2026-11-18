//! Finn's shop: Finn_Shop ([`FinnShop`], a world actor), Finn_Shop_Parent_Buff,
//! Finn_Shop_Boots_Buff and _Ally ([`BootsExt`]), Finn_Shop_ArmorBuns_Buff ([`BunsExt`]); joined to chargeStarted and
//! chargeEnd.
//!
//! OpenBag's ChargingAttack (Out) and ActivateSpawned are the shared notifies' (the montage holds its Loop section
//! while the button is held; ActivateSpawned at 0.5834 is the purchase, the Spawner's DestroyAllActorsOnNotifyEnd at
//! 0.717 the close). Finn_Shop_Parent_Buff is abstract (no shop spawns it; its graph is ported in its children's
//! entries). FinnStoreState holds nothing: the shop's state is the actor's, the buffs' their ext.
//!
//! The shop (Finn_Shop_C):
//! - ReceiveBeginPlay @5223: BootsStacks, OwningFighter, OnFireSale, the wallet read, the Price colour and text, the UI
//!   loop; Delay(0.066); sfx_cha_finn_backpack_poof on the owner (@7364). The parent's BeginPlay comes first: its
//!   blast-box loop destroys a shop spawned outside the area once BeginPlay returns.
//! - ReceiveTick @7598: MoveX from BPI_GetMoveAxisValues. At 0.5 or more: nothing while MoveRightTimer is active, else
//!   MoveSelection(false), MoveLeftTimer cleared, MoveRightTimer = SelectionTimeDelay (0.2 s) and the select sound on
//!   the owner (posted whether or not the selection moved). At -0.5 or less the mirror; else both timers cleared. Then
//!   the latent Delay (AActor::Tick runs ReceiveTick ahead of the actor's latent actions).
//! - ActivateObject @8703 (authority): the wallet (read now), its stacks at least the selected price and a selection
//!   other than 0, else K2_DestroyActor (@11091). Then RemoveStack(price) on the wallet (at 0 it destroys itself),
//!   SpawnBuffActor on the owner's Comp_State_Manager (Boots x1, ArmorBuns x3, Bmo_Chop x1; target and instigator the
//!   owner, duration 0, BreakIfBlocked true). Boots alone goes on (@10872): each ally gets Boots_Ally (@4869), then
//!   K2_DestroyActor (@5134). ArmorBuns and BMO leave the shop up until the Spawner's notify end.
//! - ReceiveEndPlay @13145: sfx_cha_finn_backpack_poof at ChooseGoose's location + (0, 50, 100) world (UE Y 50 = sim x
//!   -50). ChooseGoose sits at ChooseGooseLocation (150, -140, -275) relative, X mirrored for a left-facing owner,
//!   under the actor's yaw -90: sim x + X, y + Z.
#![allow(clippy::neg_cmp_op_on_partial_ord, reason = "`!(a < b)` is true for a NaN")]
// Deviation: before the Delay's end the actor keeps the Spawner's rotation and the goose its unmirrored X; taken as
// after it (the place of a sound only).
//
// Finn_Shop_Parent_Buff (both Boots' parent): BeginPlay binds the Instigator's OnDestroyed -> K2_DestroyActor, which
// a match never fires: no graph destroys an eliminated pawn. PandaGameMode SpawnAndInitPlayer's two K2_DestroyActor
// calls replace an old pawn: @2973 when the player's selected class is no child of the old pawn's (@654
// ClassIsChildOf false, a new class spawned in its place), @6479 on the Respawn false branch (a player's first
// spawn); a respawn of the same class goes through RespawnCharacter, which reuses the pawn. So an ally's Boots_Ally
// keeps its 5 s life span after Finn is out. SetUiServer's SetUIMulticast does nothing for an
// Icon of null; EndPlay removes the UI state FinnShopBuff, which no class adds. BuffParentActor's BeginPlay posts
// SFX_Loop_Start, the buy sound, here.
// Unverified: no native path destroys an eliminated pawn either: AActor::FellOutOfWorld (KillZ) destroying a pawn
// that falls below the world's KillZ, and any native elimination in the game mode, are not read.
// Finn_Shop_Boots_Buff BeginPlay @1654: the parent's, then (authority) Delay(0.1) @1564 resuming @15: PopTextMulticast
// ("Speed Up x{max(CurrentStackAmount, 1)}", on the owner) and PF_Post_VO_Event(owner, ability_boots, 1.0) @1033.
// AddStack @1669: the parent's, then SpeedMod = n <= 1 ? 1.1 : n == 2 ? 1.15 : 1.15 + 0.03 (n - 2), UpdatePlayerStatus
// and the same Delay node (one pending at a time: a second call while it runs is dropped).
// Finn_Shop_Boots_Buff_Ally (a child of Boots): InitialLifeSpan 5 (the actor's life span: destroyed 5 s after its
// spawn); BeginPlay the parent's, then BPI_IncrementCoopActions on the instigator's player state (a stat: not ported).
// Unverified: Boots (BuffDuration 0) keeps until the owner's death, as every buff of duration 0.
//
// Finn_Shop_ArmorBuns_Buff. BeginPlay @3606: the parent's; binds the owner's Comp_Pawn_ChargeAttack ChargeStarted ->
// StartProjectileArmor and ChargeEnd -> StopProjectileArmor (CharHooks charge_started / charge_end) and its
// DamageManager HitByEnemy (an empty handler, @4162); PF_Post_VO_Event(Instigator, ability_buns, 1.0) @2786;
// (authority) UpdateStackUi @2884; GetAllies, each ally into the Allies map with CurrentStackAmount @3368.
// UpdateStackUi @3502: CurrentStackAmount > 0 -> StackArmorBuns(count): AddStateForPlayer {ArmorBuns, StateType 2,
// DisplayType 1, CurrentValue count, MaxValue 0, Timer -1, MI_State_ProjectileArmor, active, no pop text}.
// AddStack / RemoveStack @5032 / @5066: the parent's, then UpdateStackUi.
// StartProjectileArmor @4077: nothing while Charging; Charging = true; Delay(StartupDelay 0.1) @4103 resuming @1393:
// Charging and ProjectileBlockStacksOnApply == -1 -> the owner's SpawnBuffActor(Buff_ProjectileBlock, owner, owner,
// CurrentStackAmount, 0.0) @1628, its ConditionalBuffUpdate bound to ConditionalUpdateStack @1716 (a stacked spawn
// returns no NewBuff, nothing bound), ProjectileBlockStacksOnApply = CurrentStackAmount, then each ally with a count
// above 0 its own (instigator the owner, the ally's count; @2573, nothing bound).
// StopProjectileArmor @4051: nothing unless Charging; Charging = false; @317: with an apply, each ally's
// HandleStopMomentum and its count less the stacks it lost, then the owner's; ProjectileBlockStacksOnApply = -1.
// ConditionalUpdateStack @2736: RemoveStack(1). EndPlay @4027 -> @15: RemoveStateForPlayer(ArmorBuns), then @317.

use super::coins::wallet_stacks;
use super::perks::is_sale_active;
use super::state::finn_mut;
use crate::Real;
use crate::State;
use crate::actors::world_actor::{WorldActorState, actor_p};
use crate::actors::{ActorClass, ActorId, ActorRef, WorldActor, WorldActorClass};
use crate::combat::BuffClass;
use crate::combat::buffs::{
    BuffExt, BuffRef, buff_of, inst_view, remove_stack, set_buff_values, spawn_buff, update_states,
};
use crate::events::{Event, NotifyRef, PopKind, UiStateOp, UiStateParams};
use crate::fighter::common::{actor_data, latent_frames, peers, timer_frames};
use crate::fighter::match_side::{SpawnRequest, post_vo};
use crate::fighter::{FighterId, actor_y, facing};
use crate::math::to_int32;
use crate::name;
use crate::names::Name;
use crate::registry::entries::{ActorEntry, ActorHooks, BuffEntry, BuffHooks, SizeHooks, WorldActorHooks};
use crate::registry::props::{buff, buff_mut, flag, num, obj};
use crate::registry::signals::{BuffSignal, BuffSignalArg};
use crate::registry::{CharHooks, Registry, RegistryError};
use crate::world::World;
use crate::world::blast::{BlastBounds, blast_box_loop, blast_timer_exit, new_blast_bounds, spawn_box};
use crate::world::spawn_spot::spawn_place;
use crate::world::spawning::request_props;
use mvs_data::value::{Props, Value};

/// The shop's class.
pub const SHOP: &str = "Finn_Shop_C";
const BOOTS: Name = name!("Finn_Shop_Boots_Buff_C");
const BOOTS_ALLY: Name = name!("Finn_Shop_Boots_Buff_Ally_C");
const ARMOR_BUNS: Name = name!("Finn_Shop_ArmorBuns_Buff_C");
/// bmo.rs's, spawned here by name through the buff registry.
const BMO_CHOP: Name = name!("Bmo_Chop_Buff_C");
/// the dodge perk's entry.
const PROJECTILE_BLOCK: Name = name!("Buff_ProjectileBlock_C");
const TREASURE: Name = name!("FinnTreasureBuff_C");
const ARMOR_UI: Name = name!("ArmorBuns");

/// `FinnShop` (Finn_Shop_C): what the view reads of it (its place, the selection, the prices, what the wallet covers,
/// whether the widgets show and which way the goose faces) and its own timers.
#[derive(State, Debug)]
pub struct FinnShop {
    /// Its place: the Spawner's (socket None, Translation Z 200), then attached to the owner.
    pub x: Real,
    pub y: Real,
    /// CurrentSelection @11106: 0 Cancel, 1 Boots, 2 ArmorBuns, 3 BMO; the class's is 1.
    pub selection: i32,
    /// Per slot 0-3 the price (0 for Cancel; 1: BootStackCost x BootsStacks + ShopCostOption1, 2 / 3: ShopCostOption2 /
    /// 3, each less DiscountAmount on a fire sale), fixed at BeginPlay.
    pub prices: [Real; 4],
    /// Per slot whether the wallet read at the last UI pass covers its price.
    pub affordable: [bool; 4],
    /// The widgets, the price and ChooseGoose shown: after BeginPlay's Delay(0.066).
    pub visible: bool,
    /// ChooseGoose's relative X mirrored (the owner faced left at that Delay's end).
    pub goose_left: bool,
    /// BootsStacks: the owner's Boots buff stacks at BeginPlay.
    pub boots_stacks: Real,
    /// OnFireSale: Buff_Finn_GoingOutOfBusiness IsSaleActive at BeginPlay.
    pub on_fire_sale: bool,
    /// Frames left of BeginPlay's Delay(0.066).
    pub attach_in: Real,
    pub attached: bool,
    /// The offset K2_AttachToActor(owner, KeepWorld) kept.
    pub dx: Real,
    pub dy: Real,
    /// MoveRightTimer / MoveLeftTimer: frames left, 0 = not active.
    pub right_timer: Real,
    pub left_timer: Real,
    /// The timer passes it has had.
    pub age: i32,
    /// SpawnedActorParent's CheckBlastBoxTimer: its next expiry.
    pub blast_at: crate::Clock,
    pub blast_bounds: BlastBounds,
}

impl Default for FinnShop {
    fn default() -> Self {
        FinnShop {
            x: 0.0,
            y: 0.0,
            selection: 1,
            prices: [0.0; 4],
            affordable: [false; 4],
            visible: false,
            goose_left: false,
            boots_stacks: 0.0,
            on_fire_sale: false,
            attach_in: 0.0,
            attached: false,
            dx: 0.0,
            dy: 0.0,
            right_timer: 0.0,
            left_timer: 0.0,
            age: 0,
            blast_at: f64::from(0.1_f32),
            blast_bounds: new_blast_bounds(),
        }
    }
}

/// Finn_Shop_Boots_Buff_C's and Finn_Shop_Boots_Buff_Ally_C's extension (`BootsExt`).
#[derive(State, Debug, Default)]
pub struct BootsExt {
    /// Frames left of Delay(0.1), 0 = none pending.
    pub delay: Real,
    /// Frames left of InitialLifeSpan, 0 = none.
    pub life: Real,
}

/// Finn_Shop_ArmorBuns_Buff_C's extension (`BunsExt`).
#[derive(State, Debug)]
pub struct BunsExt {
    /// Charging.
    pub charging: bool,
    /// ProjectileBlockStacksOnApply (-1: none applied).
    pub applied: Real,
    /// Frames left of Delay(StartupDelay), 0 = none pending.
    pub delay: Real,
    /// The Allies map's keys, in insertion order.
    pub allies: Vec<FighterId>,
    /// Its values.
    pub counts: Vec<Real>,
    /// The Buff_ProjectileBlock actors whose ConditionalBuffUpdate is bound.
    pub bound: Vec<BuffRef>,
}

impl Default for BunsExt {
    fn default() -> Self {
        BunsExt {
            charging: false,
            applied: -1.0,
            delay: 0.0,
            allies: Vec::new(),
            counts: Vec::new(),
            bound: Vec::new(),
        }
    }
}

/// The Finn_Shop_C's place (its `x`, `y`).
pub fn place(w: &World, a: &WorldActor, s: &FinnShop) -> Option<(Real, Real)> {
    let _ = (w, a);
    Some((s.x, s.y))
}

fn shop_of(w: &World, id: ActorId) -> Option<&FinnShop> {
    match &w.actors.actors.get(id)?.state {
        WorldActorState::FinnShop(s) => Some(s),
        _ => None,
    }
}

fn shop_mut(w: &mut World, id: ActorId) -> Option<&mut FinnShop> {
    match &mut w.actors.actors.get_mut(id)?.state {
        WorldActorState::FinnShop(s) => Some(s),
        _ => None,
    }
}

fn sound(w: &mut World, f: FighterId, ev: Name, at: Option<(Real, Real)>) {
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::Sound {
        fighter,
        ev,
        x: at.map(|a| a.0),
        y: at.map(|a| a.1),
        hit: None,
        pooled: false,
        key: None,
    });
}

/// The price switch of slot `sel` (@1521-1916, @6348-6743, @9043-9438: the same terms everywhere). Int math.
fn price(p: &Props, on_fire_sale: bool, boots_stacks: Real, sel: i32) -> Real {
    let off = if on_fire_sale { num(p, "DiscountAmount").unwrap_or(2.0) } else { 0.0 };
    let v = match sel {
        1 => num(p, "BootStackCost").unwrap_or(3.0) * boots_stacks + (num(p, "ShopCostOption1").unwrap_or(5.0) - off),
        2 => num(p, "ShopCostOption2").unwrap_or(6.0) - off,
        3 => num(p, "ShopCostOption3").unwrap_or(10.0) - off,
        _ => 0.0,
    };
    crate::real(to_int32(v))
}

/// The UI loop over the slots and the Price colour, from one wallet read: CheckHasBuff(FinnTreasureBuff)'s BuffStacks.
fn paint(w: &mut World, id: ActorId, f: FighterId) {
    let wallet = wallet_stacks(w, f);
    if let Some(s) = shop_mut(w, id) {
        for i in 0..4 {
            s.affordable[i] = wallet >= s.prices[i];
        }
    }
}

/// MoveSelection(Left) @0-339: down to 0 / up to ItemAmount (3); a change calls UpdateSelectionServer ->
/// UpdateSelectionMulticast @11106: CurrentSelection, the Price text and colour, the UI loop.
fn move_selection(w: &mut World, id: ActorId, f: FighterId, left: bool) {
    let items = num(actor_p(w, ActorRef::Actor(id)), "ItemAmount").unwrap_or(3.0);
    let Some(s) = shop_mut(w, id) else { return };
    if left {
        if s.selection == 0 {
            return;
        }
        s.selection -= 1;
    } else {
        if !(crate::real(s.selection) < items) {
            return;
        }
        s.selection += 1;
    }
    paint(w, id, f);
}

/// GetAllies (Comp_Actor_Destructible): the other players of the owner's team not knocked out, in the sim's order.
fn allies_of(w: &World, f: FighterId) -> Vec<FighterId> {
    let team = w.fighter(f).team.team;
    peers(w)
        .iter()
        .copied()
        .filter(|&o| {
            let x = w.fighter(o);
            o != f && x.team.team == team && x.life.dead <= 0.0 && !x.life.is_minion
        })
        .collect()
}

/// The kind's `spawn`: the place, the blast-box loop, BootsStacks, OnFireSale, the prices, the first UI pass.
fn shop_spawn(w: &mut World, f: FighterId, r: &SpawnRequest) -> (WorldActorState, bool) {
    let data = w.data.clone();
    let cls = w.names.str(r.cls);
    let a = actor_data(w, &data, ActorRef::Fighter(f), r.cls)
        .unwrap_or_else(|| panic!("{cls}: no actor data for its spawn (the importer gives every spawned class)"));
    let np = request_props(w, f, r);
    let at = spawn_place(w, f, &np);
    let mut s = FinnShop {
        x: at.x,
        y: at.y,
        selection: num(&a.p, "CurrentSelection").map_or(1, to_int32),
        ..FinnShop::default()
    };
    let sb = spawn_box(w);
    let dead = blast_box_loop(
        &sb,
        &mut s.x,
        &mut s.y,
        None,
        flag(&a.p, "ForceSpawnPositionInsideBlastBox"),
        &mut s.blast_bounds,
        flag(&a.p, "DestroyOnExitedArena?"),
    );
    if let Some(b) = buff_of(w, f, BOOTS) {
        s.boots_stacks = buff(w, b).map_or(0.0, |x| x.stacks);
    }
    s.on_fire_sale = is_sale_active(w, f);
    for (i, slot) in (0..4_i32).zip(s.prices.iter_mut()) {
        *slot = price(&a.p, s.on_fire_sale, s.boots_stacks, i);
    }
    let wallet = wallet_stacks(w, f);
    for i in 0..4 {
        s.affordable[i] = wallet >= s.prices[i];
    }
    s.attach_in = latent_frames(0.066);
    sound(w, f, name!("sfx_cha_finn_backpack_poof"), None);
    (WorldActorState::FinnShop(s), dead)
}

/// `finn(f).shop = s`, once the actor has its id.
fn shop_spawned(w: &mut World, id: ActorId) {
    let Some(owner) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    if let Some(st) = finn_mut(w, owner) {
        st.shop = Some(ActorRef::Actor(id));
    }
}

fn shop_step(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let (fx, fy) = (w.fighter(f).body.x, actor_y(w, f));
    let delay = num(actor_p(w, ActorRef::Actor(id)), "SelectionTimeDelay").unwrap_or(0.2);
    if let Some(s) = shop_mut(w, id)
        && s.attached
    {
        s.x = fx + s.dx;
        s.y = fy + s.dy;
    }
    let mx = w.fighter(f).controls.inp.x;
    let select = name!("sfx_cha_finn_backpack_buff_select");
    if mx >= 0.5 {
        if shop_of(w, id).is_some_and(|s| s.right_timer <= 0.0) {
            move_selection(w, id, f, false);
            if let Some(s) = shop_mut(w, id) {
                s.left_timer = 0.0;
                s.right_timer = timer_frames(Some(delay));
            }
            sound(w, f, select, None);
        }
    } else if mx <= -0.5 {
        if shop_of(w, id).is_some_and(|s| s.left_timer <= 0.0) {
            move_selection(w, id, f, true);
            if let Some(s) = shop_mut(w, id) {
                s.right_timer = 0.0;
                s.left_timer = timer_frames(Some(delay));
            }
            sound(w, f, select, None);
        }
    } else if let Some(s) = shop_mut(w, id) {
        s.left_timer = 0.0;
        s.right_timer = 0.0;
    }
    // Delay(0.066) resumes @50: K2_AttachToActor(owner, KeepWorld x3), the widgets shown, ChooseGoose's X mirrored when
    // the owner faces left.
    let left = facing(w, f) < 0.0;
    if let Some(s) = shop_mut(w, id)
        && s.attach_in > 0.0
    {
        s.attach_in -= 1.0;
        if s.attach_in == 0.0 {
            s.attached = true;
            s.dx = s.x - fx;
            s.dy = s.y - fy;
            s.visible = true;
            s.goose_left = left;
        }
    }
}

/// The frame's timer pass (after the actor ticks): CheckBlastBoxTimer (set first, by the parent's BeginPlay), then the
/// selection timers.
fn shop_late(w: &mut World, id: ActorId) {
    let doe = flag(actor_p(w, ActorRef::Actor(id)), "DestroyOnExitedArena?");
    let Some(s) = shop_mut(w, id) else { return };
    let frames = crate::real(s.age);
    s.age += 1;
    if blast_timer_exit(doe, &mut s.blast_at, &s.blast_bounds, frames, s.x, s.y) {
        if let Some(a) = w.actors.actors.get_mut(id) {
            a.dead = true;
        }
        return;
    }
    if s.right_timer > 0.0 {
        s.right_timer -= 1.0;
    }
    if s.left_timer > 0.0 {
        s.left_timer -= 1.0;
    }
}

fn kill(w: &mut World, id: ActorId) {
    if let Some(a) = w.actors.actors.get_mut(id) {
        a.dead = true;
    }
}

/// ActivateObject @8703.
fn shop_activate(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let Some((sel, cost)) = shop_of(w, id).map(|s| {
        let p = actor_p(w, ActorRef::Actor(id));
        (s.selection, price(p, s.on_fire_sale, s.boots_stacks, s.selection))
    }) else {
        return;
    };
    let wallet = buff_of(w, f, TREASURE);
    let covered = wallet.and_then(|b| buff(w, b)).is_some_and(|x| x.stacks >= cost);
    let Some(wallet) = wallet.filter(|_| covered && sel != 0) else {
        kill(w, id);
        return;
    };
    remove_stack(w, f, wallet, to_int32(cost));
    let class = match sel {
        1 => Some((BOOTS, 1)),
        2 => Some((ARMOR_BUNS, 3)),
        3 => Some((BMO_CHOP, 1)),
        _ => None,
    };
    if let Some((cls, n)) = class {
        spawn_buff(w, f, cls, 0.0, Some(n), Some(f), true, Some(f));
    }
    if sel != 1 {
        return;
    }
    for o in allies_of(w, f) {
        spawn_buff(w, o, BOOTS_ALLY, 0.0, Some(1), Some(f), true, Some(o));
    }
    kill(w, id);
}

/// ReceiveEndPlay @13145: the poof at the goose, the owner's `shop` cleared.
fn shop_end(w: &mut World, id: ActorId) {
    let Some(f) = w.actors.actors.get(id).map(|a| a.owner) else { return };
    let Some((x, y, left)) = shop_of(w, id).map(|s| (s.x, s.y, s.goose_left)) else { return };
    let (gx, gz) = match obj(actor_p(w, ActorRef::Actor(id)), "ChooseGooseLocation") {
        Some(g) => (num(g, "X").unwrap_or(0.0), num(g, "Z").unwrap_or(0.0)),
        None => (150.0, -275.0),
    };
    let gx = if left { -gx } else { gx };
    sound(w, f, name!("sfx_cha_finn_backpack_poof"), Some((x + gx - 50.0, y + gz + 100.0)));
    if let Some(st) = finn_mut(w, f)
        && st.shop == Some(ActorRef::Actor(id))
    {
        st.shop = None;
    }
}

// ---------------------------------------------------------------- Boots, Boots_Ally

fn boots_speed(n: Real) -> Real {
    if !(n > 1.0) {
        return 1.1;
    }
    if !(n > 2.0) {
        return 1.15;
    }
    1.15 + ((n - 2.0) * 0.03)
}

fn boots_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _d: Option<Real>) {
    let (start, life) = {
        let d = inst_view(w, b);
        (
            d.get("SFX_Loop_Start")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|s| crate::registry::props::name(w, s)),
            d.num("InitialLifeSpan").filter(|l| *l != 0.0),
        )
    };
    if let Some(ev) = start {
        sound(w, f, ev, None);
    }
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::ShopBoots(BootsExt {
            delay: latent_frames(0.1),
            life: life.map_or(0.0, |l| timer_frames(Some(l))),
        });
    }
}

fn boots_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let stacks = x.stacks;
    let BuffExt::ShopBoots(e) = &mut x.ext else { return false };
    if e.delay <= 0.0 {
        return false;
    }
    e.delay -= 1.0;
    if e.delay > 0.0 {
        return false;
    }
    let fighter = w.fighter(f).id;
    w.fighter_mut(f).events.push(Event::PopText {
        fighter,
        kind: PopKind::FinnBoots,
        stacks: crate::math::max(stacks, 1.0),
        gain: None,
        params: None,
    });
    post_vo(w, f, name!("ability_boots"), None, 1.0);
    false
}

fn boots_stacks_hook(w: &mut World, f: FighterId, b: BuffRef, added: bool) -> bool {
    if !added {
        return false;
    }
    let n = buff(w, b).map_or(0.0, |x| x.stacks);
    let mut v = Props::new();
    v.insert("SpeedMod".into(), Value::Num(f64::from(boots_speed(n))));
    set_buff_values(w, b, &v);
    update_states(w, f);
    if let Some(x) = buff_mut(w, b)
        && let BuffExt::ShopBoots(e) = &mut x.ext
        && e.delay <= 0.0
    {
        e.delay = latent_frames(0.1);
    }
    false
}

/// Boots_Ally's life span (a world timer): K2_DestroyActor at its end.
fn boots_timer(w: &mut World, _f: FighterId, b: BuffRef) -> bool {
    let Some(x) = buff_mut(w, b) else { return false };
    let BuffExt::ShopBoots(e) = &mut x.ext else { return false };
    if e.life > 0.0 {
        e.life -= 1.0;
        return e.life == 0.0;
    }
    false
}

// ---------------------------------------------------------------- ArmorBuns

fn buns(w: &World, b: BuffRef) -> Option<&BunsExt> {
    match &buff(w, b)?.ext {
        BuffExt::ArmorBuns(e) => Some(e),
        _ => None,
    }
}

fn buns_mut(w: &mut World, b: BuffRef) -> Option<&mut BunsExt> {
    match &mut buff_mut(w, b)?.ext {
        BuffExt::ArmorBuns(e) => Some(e),
        _ => None,
    }
}

fn update_stack_ui(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(stacks) = buff(w, b).map(|x| x.stacks).filter(|s| *s > 0.0) else { return };
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: ARMOR_UI,
        state_type: 2,
        display_type: 1,
        current_value: stacks,
        max_value: 0.0,
        timer: -1.0,
        icon_material: name!("MI_State_ProjectileArmor"),
        active: true,
        pop_text: Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

/// HandleStopMomentum(Fighter, StacksStarting) -> StacksLost @0-1083: 0 unless an apply stands; no block on the
/// fighter: all lost; fewer started than it holds: RemoveStack(started), none lost; else RemoveStack(what it holds)
/// (it ends) and the difference lost.
fn handle_stop(w: &mut World, applied: Real, o: FighterId, starting: Real) -> Real {
    if !(applied > 0.0) {
        return 0.0;
    }
    let Some(blk) = buff_of(w, o, PROJECTILE_BLOCK) else { return starting };
    let held = buff(w, blk).map_or(0.0, |x| x.stacks);
    if starting < held {
        remove_stack(w, o, blk, to_int32(starting));
        return 0.0;
    }
    let diff = starting - held;
    if diff >= starting {
        remove_stack(w, o, blk, to_int32(starting));
        return starting;
    }
    remove_stack(w, o, blk, to_int32(starting - diff));
    diff
}

fn stop_apply(w: &mut World, f: FighterId, b: BuffRef) {
    let Some(e) = buns(w, b) else { return };
    if e.applied == -1.0 {
        return;
    }
    let (applied, n_allies) = (e.applied, e.allies.len());
    for i in 0..n_allies {
        let Some(e) = buns(w, b) else { return };
        let (Some(&o), Some(&n)) = (e.allies.get(i), e.counts.get(i)) else { break };
        if !(n > 0.0) {
            continue;
        }
        let lost = handle_stop(w, applied, o, n);
        if let Some(c) = buns_mut(w, b).and_then(|e| e.counts.get_mut(i)) {
            *c = n - lost;
        }
    }
    handle_stop(w, applied, f, applied);
    if let Some(e) = buns_mut(w, b) {
        e.applied = -1.0;
    }
}

fn buns_begin(w: &mut World, f: FighterId, b: BuffRef, from: Option<FighterId>, _d: Option<Real>) {
    let (start, applied) = {
        let d = inst_view(w, b);
        (
            d.get("SFX_Loop_Start")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|s| crate::registry::props::name(w, s)),
            d.num("ProjectileBlockStacksOnApply").unwrap_or(-1.0),
        )
    };
    if let Some(ev) = start {
        sound(w, f, ev, None);
    }
    let stacks = buff(w, b).map_or(0.0, |x| x.stacks);
    if let Some(x) = buff_mut(w, b) {
        x.ext = BuffExt::ArmorBuns(BunsExt { applied, ..BunsExt::default() });
    }
    post_vo(w, from.unwrap_or(f), name!("ability_buns"), None, 1.0);
    update_stack_ui(w, f, b);
    for o in allies_of(w, f) {
        let Some(e) = buns_mut(w, b) else { return };
        match e.allies.iter().position(|&a| a == o) {
            None => {
                e.allies.push(o);
                e.counts.push(stacks);
            }
            Some(i) => e.counts[i] = stacks,
        }
    }
}

fn buns_tick(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = buns_mut(w, b) else { return false };
    if e.delay <= 0.0 {
        return false;
    }
    e.delay -= 1.0;
    if e.delay > 0.0 || !e.charging || e.applied != -1.0 {
        return false;
    }
    let stacks = buff(w, b).map_or(0.0, |x| x.stacks);
    let nb = spawn_buff(w, f, PROJECTILE_BLOCK, 0.0, Some(to_int32(stacks)), Some(f), true, Some(f));
    let Some(e) = buns_mut(w, b) else { return false };
    if let Some(nb) = nb
        && !e.bound.contains(&nb)
    {
        e.bound.push(nb); // AddMulticastDelegate: AddUnique
    }
    e.applied = stacks;
    let (allies, counts) = (e.allies.clone(), e.counts.clone());
    for (o, n) in allies.into_iter().zip(counts) {
        if n > 0.0 {
            spawn_buff(w, o, PROJECTILE_BLOCK, 0.0, Some(to_int32(n)), Some(f), true, Some(o));
        }
    }
    false
}

fn buns_on(w: &mut World, f: FighterId, b: BuffRef, s: BuffSignal, a: &BuffSignalArg) -> bool {
    if s != BuffSignal::ConditionalBuffUpdate {
        return false;
    }
    let Some(src) = a.source else { return false };
    if buns(w, b).is_some_and(|e| e.bound.contains(&src)) {
        remove_stack(w, f, b, 1);
    }
    false
}

fn buns_stacks(w: &mut World, f: FighterId, b: BuffRef, _added: bool) -> bool {
    update_stack_ui(w, f, b);
    false
}

fn buns_end(w: &mut World, f: FighterId, b: BuffRef) {
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(ARMOR_UI));
    stop_apply(w, f, b);
}

fn charge_started(w: &mut World, f: FighterId, _n: NotifyRef) {
    let Some(b) = buff_of(w, f, ARMOR_BUNS) else { return };
    let startup = inst_view(w, b).num("StartupDelay").unwrap_or(0.1);
    let Some(e) = buns_mut(w, b) else { return };
    if e.charging {
        return;
    }
    e.charging = true;
    if e.delay <= 0.0 {
        e.delay = latent_frames(startup);
    }
}

fn charge_end(w: &mut World, f: FighterId, _n: NotifyRef) {
    let Some(b) = buff_of(w, f, ARMOR_BUNS) else { return };
    let Some(e) = buns_mut(w, b) else { return };
    if !e.charging {
        return;
    }
    e.charging = false;
    stop_apply(w, f, b);
}

/// The hooks this file joins in Finn's CharHooks (`charge_started`, `charge_end`).
pub fn hooks(h: &mut CharHooks) {
    h.charge_started = Some(charge_started);
    h.charge_end = Some(charge_end);
}

/// This file's registry entries (Finn_Shop_C and the shop buffs). `size`: no shape of the sim's (its Capsule is a nav
/// obstacle no class traces); the widgets' view reads it. No `bodies` either.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    let hooks = WorldActorHooks {
        spawn: Some(shop_spawn),
        spawned: Some(shop_spawned),
        step: Some(shop_step),
        late: Some(shop_late),
        activate: Some(shop_activate),
        end: Some(shop_end),
        size: Some(SizeHooks::default()),
        bodies: Some(|_, _, _| {}),
        ..WorldActorHooks::default()
    };
    r.actors.enter(SHOP, ActorEntry::new(ActorClass::World(WorldActorClass::FinnShop), ActorHooks::World(hooks)))?;
    let boots = BuffHooks {
        begin: Some(boots_begin),
        tick: Some(boots_tick),
        stacks: Some(boots_stacks_hook),
        timer: Some(boots_timer),
        ..BuffHooks::default()
    };
    r.buffs.enter("Finn_Shop_Boots_Buff_C", BuffEntry::new(BuffClass::FinnShopBootsBuff, boots))?;
    r.buffs.enter("Finn_Shop_Boots_Buff_Ally_C", BuffEntry::new(BuffClass::FinnShopBootsBuffAlly, boots))?;
    r.buffs.enter(
        "Finn_Shop_ArmorBuns_Buff_C",
        BuffEntry::new(
            BuffClass::FinnShopArmorBunsBuff,
            BuffHooks {
                begin: Some(buns_begin),
                tick: Some(buns_tick),
                on: Some(buns_on),
                stacks: Some(buns_stacks),
                end: Some(buns_end),
                ..BuffHooks::default()
            },
        ),
    )
}
