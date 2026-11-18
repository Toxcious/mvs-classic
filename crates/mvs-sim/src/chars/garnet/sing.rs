//! The neutral special: SetSinging @7911 -> @7786,
//! MulticastIsSigning @18948, ApplySongBuffToArea @19780 with its five Delay(0) retries (resume @538 -> 15) and Apply
//! Buff For Fighter, the Sing Sphere's begin overlap @13142 and end @13578 -> @8835 with OverlappingSingActors,
//! AnimNotify_GarnetStartSinging / _StartSingBuff, Buff_GarnetStackingSinging ([`SingExt`]; TimerExists, native) and
//! its part of the joined timerTick. The Sing Sphere is the pawn's component (`Sing Sphere_GEN_VARIABLE`).
//! Buff_GarnetStopProjectile is stop_projectile.rs's, given here by its class name through the actor buff route
//! (`combat::elem` spawn_actor_buff). BPI_SetSinging's one caller is AnimNotify_GarnetStartSinging, entered here: the
//! notify calls the body directly.
//!
//! What the sing does: BPI_IsSinging / BPI_IsSingReady are literal false (the release always cancels), so the whole
//! neutral special is the notify's SetSinging(true) at its begin and (false) at its end: the music stacks on Garnet
//! and every ally (any distance), and Buff_GarnetStopProjectile on the enemy spawned actors in the sphere.

use mvs_data::value::{Props, Value};

use crate::Real;
use crate::State;
use crate::actors::ActorRef;
use crate::actors::world_actor::{actor_cls, actor_dead, actor_elem, warn};
use crate::combat::BuffClass;
use crate::combat::boxes::{PawnHitSphere, pawn_hit_spheres, pawn_sphere_place};
use crate::combat::buffs::{
    BuffExt, BuffRef, buff, buff_mut, buff_of, inst_def_of, set_buff_values, spawn_buff, stack_buff, update_states,
};
use crate::combat::elem::{ElemExt, spawn_actor_buff};
use crate::combat::hit::timer_ticks;
use crate::events::{Event, NotifyRef, UiStateOp, UiStateParams};
use crate::fighter::common::{actor_is_a, buff_def, data_of, owner_team, peers, timer_frames};
use crate::fighter::traces::sphere_overlap_actors;
use crate::fighter::{FighterId, actor_collision_off, actor_y, mesh_wx, mesh_wy};
use crate::math::max;
use crate::name;
use crate::registry::entries::{BuffHooks, NotifyHandler};
use crate::registry::props::num;
use crate::registry::{BuffEntry, Registry, RegistryError};
use crate::world::World;

use super::state::{garnet, garnet_mut, garnet_of, garnet_w};

/// The class's name.
pub const STACKING_SINGING: &str = "Buff_GarnetStackingSinging_C";
const SPHERE: &str = "Sing Sphere_GEN_VARIABLE";

// ---------------------------------------------------------------- BPI_SetSinging @7911 -> @7786

/// Authority; acts only when IsActiveSinging differs from the input @7816: MulticastIsSigning(Singing) @7864, then
/// ApplySongBuffToArea(Singing) @7887.
fn set_singing(w: &mut World, f: FighterId, singing: bool) {
    if garnet(w, f).is_active_singing == singing {
        return;
    }
    multicast_is_signing(w, f, singing);
    apply_song_buff_to_area(w, f, singing);
}

/// MulticastIsSigning @18948: IsActiveSinging = IsSinging. True: SingingVFX (PS_Garnet_SPN_Singing_Activate attached
/// to the root, the Team parameter by BPI_GetColorValue) when it is not valid already @18981-19327, the view's.
/// False: a valid SingingVFX is taken off the visibility manager and destroyed, then SFX Buff Select @19503 -> @19850
/// posts sfx_cha_garnet_attack_special_n_select on the pawn; none valid: nothing @19415.
fn multicast_is_signing(w: &mut World, f: FighterId, singing: bool) {
    let g = garnet_w(w, f);
    g.is_active_singing = singing;
    if singing {
        g.sing.singing_vfx = true;
        return;
    }
    if !g.sing.singing_vfx {
        return;
    }
    g.sing.singing_vfx = false;
    let (id, x, y) = (w.fighter(f).id, w.fighter(f).body.x, actor_y(w, f));
    let ev = name!("sfx_cha_garnet_attack_special_n_select");
    let e = Event::Sound { fighter: id, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None };
    w.fighter_mut(f).events.push(e);
}

// ---------------------------------------------------------------- ApplySongBuffToArea @19780

/// Array_Clear(BuffedAllies) @19780 (nothing adds to it: not kept), Apply Buff For Fighter(self, Entering) @19821,
/// then @15 ([`song_area`]).
fn apply_song_buff_to_area(w: &mut World, f: FighterId, entering: bool) {
    apply_buff_for_fighter(w, f, f, entering);
    garnet_w(w, f).sing.song_entering = entering;
    song_area(w, f);
}

/// @15: AreAlliesAssigned @15-60. Assigned: Apply Buff For Fighter(ally, Entering) over GetAllies @74-399, then over
/// OverlappingSingActors @593-1193 (a SpawnedActorParent: SpawnBuffActor(Buff_GarnetStopProjectile, it, self, 0
/// stacks, 0.0 s entering @926 or 3.0 s leaving @1025, BreakIfBlocked true)). The loop reads the array's length at
/// each step @639 and Array_Get's by index: an actor the buff's BeginPlay takes out of the array (its collision off
/// ends the overlap: [`sing_sphere_end_overlap`]) moves the next one into its index, which the loop then skips, as
/// here. Not assigned @400: the counter below 5 @425 -> +1 and Delay(0) @538, which resumes @15 with the Entering of
/// the last call (the ubergraph's own variable); at 5 nothing more (the counter is set to 0 once in the pawn's life,
/// @1272-1322). Entering is passed to the fighters and the actors as it is (leaving gives the actors the 3 s buff).
fn song_area(w: &mut World, f: FighterId) {
    let s = &garnet(w, f).sing;
    let entering = s.song_entering;
    if !allies_assigned(w, f) {
        let s = &mut garnet_w(w, f).sing;
        if s.song_retries < 5.0 {
            s.song_retries += 1.0;
            s.song_retry_pending = true;
        }
        return;
    }
    let team = w.fighter(f).team.team;
    let allies: Vec<FighterId> = peers(w)
        .iter()
        .copied()
        .filter(|&o| {
            let x = w.fighter(o);
            o != f && x.team.team == team && x.life.dead <= 0.0 && !x.life.is_minion
        })
        .collect();
    for o in allies {
        apply_buff_for_fighter(w, f, o, entering);
    }
    let mut i = 0;
    while let Some(&a) = garnet(w, f).sing.overlapping_sing_actors.get(i) {
        i += 1;
        if !actor_dead(w, a) {
            give_stop(w, f, a, if entering { 0.0 } else { 3.0 });
        }
    }
}

/// Comp_Actor_Destructible.AreAlliesAssigned: set by the component's first pass after its BeginPlay's Delay(0)
/// (Comp_Actor_Destructible.GetAllies), so from a pawn's second frame on. The port's allies are the fighters of the
/// team from the spawn (GetAllies: the team mates in play, not minions), and no montage reaches SetSinging in a
/// pawn's first frame: always true here, so the retry above never runs.
fn allies_assigned(_w: &World, _f: FighterId) -> bool {
    true
}

/// Apply Buff For Fighter(Fighter, Entering) (function, @0-712): the fighter's Comp_State_Manager (every fighter has
/// one). CheckHasBuff(Buff_GarnetStackingSinging) @145: found (the cast succeeds) -> SetActiveServer(Entering) @405;
/// none and Entering -> SpawnBuffActor(it, Fighter, Instigator self, 1 stack, 0.0 s, BreakIfBlocked true,
/// SpawnOnAllies false) @520 and SetActiveServer(true) @675 on the new one; none and leaving: nothing.
fn apply_buff_for_fighter(w: &mut World, g: FighterId, o: FighterId, entering: bool) {
    let cls = name!("Buff_GarnetStackingSinging_C");
    if let Some(b) = buff_of(w, o, cls) {
        set_active_server(w, b, entering);
        return;
    }
    if !entering {
        return;
    }
    let nb = spawn_buff(w, o, cls, 0.0, Some(1), Some(g), true, None);
    if let Some(nb) = nb
        && buff(w, nb).is_some_and(|x| x.frames > 0.0)
    {
        set_active_server(w, nb, true);
    }
}

/// SpawnBuffActor(Buff_GarnetStopProjectile, actor, Instigator self, 0 stacks, duration, true, false) on the actor's
/// Comp_State_Manager. An item nobody owns (a stage item, its team -1) has no owner whose data the route reads the
/// class from.
// Deviation: an item nobody owns gets no buff (warned).
fn give_stop(w: &mut World, f: FighterId, a: ActorRef, duration: Real) {
    if let ActorRef::Item(id) = a
        && w.actors.items.get(id).is_some_and(|i| i.owner.is_none())
    {
        warn("Buff_GarnetStopProjectile_C on a stage item nobody owns (Pawn_Garnet's sing): not given");
        return;
    }
    spawn_actor_buff(w, a, name!("Buff_GarnetStopProjectile_C"), 0, duration, true, Some(f));
}

// ---------------------------------------------------------------- the Sing Sphere
//
// SphereComponent 'Sing Sphere' (radius 550 at (0, 0, 169.52382) under CharacterMesh0: Pawn_Garnet SCS_Node_24
// ParentComponentOrVariableName; object type ECC_GameTraceChannel3, the HitBox channel; it overlaps the types its
// ResponseArray does not Ignore: Pawn, PhysicsBody, DamageDestructible, ProjectileDestructible, ItemDestructible).
// Its world place is the mesh chain's (`combat::boxes` pawn_sphere_place); a size buff's SetActorScale3D scales the
// mesh and its children. BeginPlay's SingCollisionMaxScale = its RelativeScale3D @12826 is stored and read by nothing
// in the build's reach. The component is `combat::boxes`'s (a pawn's always-on HitBox sphere: the melee hitbox sphere
// test meets it too).

fn sphere_def(w: &World, f: FighterId) -> Option<PawnHitSphere> {
    let d = pawn_hit_spheres(w, f).into_iter().find(|x| x.name == SPHERE);
    if d.is_none() {
        warn("garnet: Sing Sphere_GEN_VARIABLE not in the pawn's components (its overlaps are not run)");
    }
    d
}

fn sphere_place(w: &World, f: FighterId, d: &PawnHitSphere) -> [Real; 3] {
    let fm = w.fighter(f);
    let (xy, sz) = match fm.body.actor_scale {
        Some(a) => (a[0], a[1]),
        None => (fm.body.size_scale, fm.body.size_scale),
    };
    let ix = &data_of(w, f).boxes;
    pawn_sphere_place(
        mesh_wx(w, f),
        mesh_wy(w, f),
        xy,
        sz,
        crate::math::narrow(ix.mesh_scale),
        crate::math::narrow(ix.mesh_z),
        d,
    )
}

/// The spawned actor of an overlap: a projectile, an item or a world actor of the SpawnedActorParent chain (the
/// begin handler's cast @13212); `None` for a fighter and anything else.
fn spawned_of(w: &World, f: FighterId, a: ActorRef) -> Option<ActorRef> {
    match a {
        ActorRef::Projectile(_) | ActorRef::Item(_) => Some(a),
        ActorRef::Fighter(_) | ActorRef::Stage(_) | ActorRef::Buff(_) => None,
        _ => {
            let cls = actor_cls(w, a)?;
            let parent = name!("SpawnedActorParent_C");
            (actor_is_a(w, ActorRef::Fighter(f), cls, parent) == Some(true)).then_some(a)
        }
    }
}

/// The actor's Comp_Actor_Destructible TeamIndex: an item's (-1 for an unowned stage item), else its owner's (a
/// reflect sets both).
fn team_of(w: &World, a: ActorRef) -> i32 {
    match a {
        ActorRef::Item(id) => w.actors.items.get(id).map_or(-1, |i| i.destructible_team(w)),
        _ => owner_team(w, a),
    }
}

fn no_actors(w: &World) -> bool {
    let a = &w.actors;
    a.projectiles.is_empty()
        && a.items.is_empty()
        && a.actors.is_empty()
        && a.markers.is_empty()
        && a.ships.is_empty()
        && a.mortars.is_empty()
        && a.aim_punches.is_empty()
        && a.accelerators.is_empty()
}

/// The sphere's overlaps, once a frame in the leftover latent pass (timer_tick): the ends first, then the begins in
/// the gather's order. The bodies met are the trace gather's (`fighter::traces` sphere_overlap_actors, responder
/// HitBox): an actor Buff_GarnetStopProjectile has turned the collision of is not met (its end came through
/// [`sing_sphere_end_overlap`]).
// Deviation: the engine fires them inside the moves (the pawn's, the actor's) and at an actor's spawn; here at the
// frame's end, where every move of the frame is done: a projectile spawned inside the sphere while she sings moves
// (and may hit) once before its StopProjectile. SetActorEnableCollision(false) on Garnet (actor_collision_off) ends
// them all.
fn sphere_overlaps(w: &mut World, f: FighterId) {
    if garnet(w, f).sing.inside.is_empty() && no_actors(w) {
        return;
    }
    let Some(d) = sphere_def(w, f) else { return };
    let [x, y, r] = sphere_place(w, f, &d);
    let now = if actor_collision_off(w, f) {
        Vec::new()
    } else {
        let types: Vec<Real> = d.types.iter().map(|&t| crate::real(t)).collect();
        let hit_box = crate::real(crate::combat::boxes::obj::HIT_BOX);
        sphere_overlap_actors(w, x, y, r, &types, &[Some(ActorRef::Fighter(f))], hit_box)
    };
    let mut i = garnet(w, f).sing.inside.len();
    while i > 0 {
        i -= 1;
        let Some(&a) = garnet(w, f).sing.inside.get(i) else { continue };
        if !actor_dead(w, a) && now.contains(&a) {
            continue;
        }
        garnet_w(w, f).sing.inside.remove(i);
        end_overlap(w, f, a);
    }
    for o in now {
        let Some(a) = spawned_of(w, f, o) else { continue };
        if actor_dead(w, a) || garnet(w, f).sing.inside.contains(&a) {
            continue;
        }
        garnet_w(w, f).sing.inside.push(a);
        begin_overlap(w, f, a);
    }
}

/// ComponentBeginOverlap @13142 (authority): OtherActor != self, a SpawnedActorParent of another team than hers
/// @13287 -> AddUnique to OverlappingSingActors @13401; singing @13469 -> SpawnBuffActor(Buff_GarnetStopProjectile,
/// 0 stacks, 0.0 s) @13479. No fighter branch (the cast fails on one).
fn begin_overlap(w: &mut World, f: FighterId, a: ActorRef) {
    if team_of(w, a) == w.fighter(f).team.team {
        return;
    }
    let g = garnet_w(w, f);
    if !g.sing.overlapping_sing_actors.contains(&a) {
        g.sing.overlapping_sing_actors.push(a);
    }
    if g.is_active_singing {
        give_stop(w, f, a, 0.0);
    }
}

/// ComponentEndOverlap @13578 -> @8835 (authority): only an actor in OverlappingSingActors @8865, which leaves it
/// @8935; a SpawnedActorParent (every entry is) with a Buff_GarnetStopProjectile whose Instigator is self
/// (CheckHasBuff @9074, GetInstigator @9271) -> SpawnBuffActor(it, 0 stacks, 3.0 s) @9353. The cast-failure branch
/// @9452 (a pawn with a hit box handler, singing: Apply Buff For Fighter(it, false)) needs a member that is no
/// SpawnedActorParent: none can be. A dead actor (destroyed, its overlaps ended by its components' destruction) only
/// leaves the list here.
// Unverified: the build runs the handler on the dying actor as well (its StopProjectile already among the children it
// destroyed or about to be: not read); the port gives a dead actor nothing.
fn end_overlap(w: &mut World, f: FighterId, a: ActorRef) {
    let list = &mut garnet_w(w, f).sing.overlapping_sing_actors;
    let Some(i) = list.iter().position(|&x| x == a) else { return };
    list.remove(i);
    if actor_dead(w, a) {
        return;
    }
    // The buff's Instigator: stop_projectile.rs keeps it as the ext's instigator (the fighter's id).
    let inst = actor_elem(w, a)
        .and_then(Option::as_ref)
        .and_then(|e| e.buffs.iter().find(|b| b.class == BuffClass::BuffGarnetStopProjectile))
        .map(|b| match b.ext {
            ElemExt::GarnetStopProjectile(e) => Some(e.instigator),
            _ => None,
        });
    let Some(Some(inst)) = inst else { return };
    if inst != w.fighter(f).id {
        return;
    }
    give_stop(w, f, a, 3.0);
}

/// `singSphereEndOverlap`: the Sing Sphere's ComponentEndOverlap for a spawned actor leaving it outside the frame's
/// sphere pass: stop_projectile.rs calls it when Buff_GarnetStopProjectile turns its owner's collision off
/// (SetActorEnableCollision(false) ends the overlap inside the call), with the Garnet whose sphere held the actor.
pub fn sing_sphere_end_overlap(w: &mut World, f: FighterId, actor: ActorRef) {
    let Some(s) = garnet_mut(w, f).map(|g| &mut g.sing) else { return };
    let Some(i) = s.inside.iter().position(|&x| x == actor) else { return };
    s.inside.remove(i);
    end_overlap(w, f, actor);
}

/// sing.rs's part of the joined timerTick (after zone.rs's), the leftover latent pass: the Delay(0) of
/// ApplySongBuffToArea resumes @15 (a Delay made in the pawn's tick counts that frame: it resumes in the frame it
/// was made; one made inside the resumed link counts from the next), then the sphere's overlaps.
pub fn timer_tick(w: &mut World, f: FighterId) {
    let s = &mut garnet_w(w, f).sing;
    if s.song_retry_pending {
        s.song_retry_pending = false;
        song_area(w, f);
    }
    sphere_overlaps(w, f);
}

// ---------------------------------------------------------------- notifies

/// AnimNotify_GarnetStartSinging (Garnet_Attack_Special_N_Montage 0.2194 s, _Air_N_Montage1 0.1969 s, to the end of
/// the montage's Sing section): Received_NotifyBegin -> BPI_SetSinging(true), Received_NotifyEnd ->
/// BPI_SetSinging(false), on an owner with BPI_Garnet (a failed cast does nothing).
fn start_singing_begin(w: &mut World, f: FighterId, _n: NotifyRef, _k: i32) {
    if garnet_of(w, f).is_some() {
        set_singing(w, f, true);
    }
}

fn start_singing_end(w: &mut World, f: FighterId, _n: NotifyRef, _k: i32) {
    if garnet_of(w, f).is_some() {
        set_singing(w, f, false);
    }
}

/// AnimNotify_GarnetStartSingBuff (the same montages, 3.4168 s and 4.7501 s): Received_NotifyBegin ->
/// BPI_ActivateSingBuff @6423, which is a bare pop_flow: nothing.
fn start_sing_buff_begin(_w: &mut World, _f: FighterId, _n: NotifyRef, _k: i32) {}

// ---------------------------------------------------------------- Buff_GarnetStackingSinging
//
// A BuffParentActor child: CanStack, MaxStacks 3, StackDecayRate 5, SpeedStack 2, ArmorBreakStack 3, SpeedMod 1.1 at
// SpeedModType DoNothing, StackIncreaseTime 0.5. Its variables are [`SingExt`].

/// Buff_GarnetStackingSinging_C's extension: ActiveCount, the Increase Stack Timer (a one-shot world timer: ticks
/// left, paused, or none), SpeedBoosted, ArmorBreaker, PreviousCountValue.
#[derive(State, Debug)]
pub struct SingExt {
    /// ActiveCount: the Garnets singing on it.
    pub active: Real,
    /// Increase Stack Timer: timer ticks left, -1 for none (never set, or fired and not set again).
    pub inc: Real,
    /// K2_PauseTimerHandle'd (the record kept: K2_TimerExistsHandle true, N1).
    pub paused: bool,
    /// Set or unpaused before the frame's timer tick: started by it without counting the frame.
    pub pending: bool,
    /// SpeedBoosted.
    pub boosted: bool,
    /// ArmorBreaker.
    pub armor_breaker: bool,
    /// PreviousCountValue.
    pub prev: Real,
}

/// The CDO values until the graph writes them.
impl Default for SingExt {
    fn default() -> Self {
        SingExt {
            active: 0.0,
            inc: -1.0,
            paused: false,
            pending: false,
            boosted: false,
            armor_breaker: false,
            prev: 0.0,
        }
    }
}

/// The buff's ext, made with the CDO values at the first read (BeginPlay's status update reads one first).
fn sing_ext(w: &mut World, b: BuffRef) -> Option<&mut SingExt> {
    let x = buff_mut(w, b)?;
    if !matches!(x.ext, BuffExt::StackingSinging(_)) {
        x.ext = BuffExt::StackingSinging(SingExt::default());
    }
    match &mut x.ext {
        BuffExt::StackingSinging(e) => Some(e),
        _ => None,
    }
}

fn def(w: &World, b: BuffRef) -> Props {
    inst_def_of(w, b).map(std::borrow::Cow::into_owned).unwrap_or_default()
}

/// SetActiveServer(Active) @2515 (reliable server): ActiveCount = Max(ActiveCount + (Active ? 1 : -1), 0), then @15:
/// - above 0: PauseStackLifeTimer @63 (BuffParentActor @5612: K2_PauseTimerHandle(StackLifeTimer); no graph unpauses
///   it: the next AddStack's SetStackLifeTimer sets a new one), then K2_TimerExistsHandle(Increase Stack Timer) @77:
///   false -> StartStackIncreaseTimer @150, true -> K2_UnPauseTimerHandle @209 (N1: a paused timer exists, so a sing
///   after a pause goes on with the time it had left; on a running timer the unpause does nothing);
/// - 0: SetStackLifeTimer(StackDecayRate) @165 (a new 5 s timer, BuffParentActor @5368), K2_PauseTimerHandle(Increase
///   Stack Timer) @188.
///
/// Its callers are the notify's SetSinging (the tick groups) and the Delay's resume (the latent pass): outside the
/// timer tick, so a timer set here is pending until that frame's timer tick ([`start_increase`]).
fn set_active_server(w: &mut World, b: BuffRef, active: bool) {
    let d = def(w, b);
    let Some(e) = sing_ext(w, b) else { return };
    e.active = max(e.active + if active { 1.0 } else { -1.0 }, 0.0);
    if e.active > 0.0 {
        if e.inc < 0.0 {
            start_increase(e, &d, false);
        } else if e.paused {
            e.paused = false;
            e.pending = true; // Timer pause / elapsed: back on the pending list with the time left
        }
        if let Some(x) = buff_mut(w, b) {
            x.stack_frames = Real::INFINITY;
        }
        return;
    }
    if e.inc >= 0.0 {
        e.paused = true;
    }
    let rate = num(&d, "StackDecayRate");
    if let Some(r) = rate.filter(|&r| r != 0.0 && !r.is_nan())
        && let Some(x) = buff_mut(w, b)
    {
        x.stack_frames = timer_frames(Some(r));
    }
}

/// StartStackIncreaseTimer @2833: K2_SetTimerDelegate(IncreaseStackFromTimerr, StackIncreaseTime, looping false).
/// `in_timer`: set inside the timer tick (active at once), else pending until it: the ticks are the Rate's either way
/// and the pending pass is not counted, so a timer paused before its first pass (Timer pause / elapsed: a pending one
/// keeps its Rate as the time left) and unpaused later fires after the Rate's ticks, as an unpaused active one does
/// after the ticks it had left.
fn start_increase(e: &mut SingExt, d: &Props, in_timer: bool) {
    e.inc = timer_ticks(num(d, "StackIncreaseTime"));
    e.paused = false;
    e.pending = !in_timer;
}

/// CalculateStackChanges (authority; function @0-479): speed, then armor, then UpdatePlayerStatus(None) when the
/// speed changed (LocalShouldCalculateBuffState: an armor change alone does not ask for it).
/// - CurrentStackAmount >= SpeedStack and not SpeedBoosted: SpeedModType = 1 (Multiply: SpeedMod 1.1), SpeedBoosted;
///   below it and SpeedBoosted: SpeedModType = 0 (DoNothing), not SpeedBoosted.
/// - CurrentStackAmount >= ArmorBreakStack and not ArmorBreaker: DealtDamageMods.ArmorBreak = true (the dealt mods read it whatever the
///   mod types), ArmorBreaker; below it and ArmorBreaker: both false.
fn calculate_stack_changes(w: &mut World, f: FighterId, b: BuffRef) {
    let d = def(w, b);
    let Some(stacks) = buff(w, b).map(|x| x.stacks) else { return };
    let Some(e) = sing_ext(w, b) else { return };
    let (boosted, breaker) = (e.boosted, e.armor_breaker);
    let mut recompute = false;
    let speed = |t: Real| Props::from_iter([("SpeedModType".to_owned(), Value::Num(f64::from(t)))]);
    if stacks >= num(&d, "SpeedStack").unwrap_or(2.0) {
        if !boosted {
            set_buff_values(w, b, &speed(1.0));
            set_flags(w, b, Some(true), None);
            recompute = true;
        }
    } else if boosted {
        set_buff_values(w, b, &speed(0.0));
        set_flags(w, b, Some(false), None);
        recompute = true;
    }
    let mods = |on: bool| {
        let mut m = d.get("DealtDamageMods").and_then(Value::as_obj).cloned().unwrap_or_default();
        m.insert("ArmorBreak".to_owned(), Value::Bool(on));
        Props::from_iter([("DealtDamageMods".to_owned(), Value::Obj(m))])
    };
    if stacks >= num(&d, "ArmorBreakStack").unwrap_or(3.0) {
        if !breaker {
            set_buff_values(w, b, &mods(true));
            set_flags(w, b, None, Some(true));
        }
    } else if breaker {
        set_buff_values(w, b, &mods(false));
        set_flags(w, b, None, Some(false));
    }
    if recompute {
        update_states(w, f);
    }
}

fn set_flags(w: &mut World, b: BuffRef, boosted: Option<bool>, breaker: Option<bool>) {
    if let Some(e) = sing_ext(w, b) {
        if let Some(v) = boosted {
            e.boosted = v;
        }
        if let Some(v) = breaker {
            e.armor_breaker = v;
        }
    }
}

/// ReceiveBeginPlay @3110 -> @3095: the parent's, then (authority) CalculateStackChanges @2794.
fn sing_begin(w: &mut World, f: FighterId, b: BuffRef, _from: Option<FighterId>, _duration: Option<Real>) {
    calculate_stack_changes(w, f, b);
}

/// AddStack @2809 (the parent's, then CalculateStackChanges @2764); RemoveStack @2682 (the parent's, then
/// CurrentStackAmount == 1 -> K2_DestroyActor @2749, else CalculateStackChanges).
fn sing_stacks(w: &mut World, f: FighterId, b: BuffRef, added: bool) -> bool {
    if !added && buff(w, b).is_some_and(|x| x.stacks == 1.0) {
        return true;
    }
    calculate_stack_changes(w, f, b);
    false
}

/// IncreaseStackFromTimerr @2941 (the Increase Stack Timer, one-shot: the record goes once it fired, N1): AddStack(0)
/// (the parent's: 1), then CurrentStackAmount <= MaxStacks and ActiveCount > 0 @2960-3070 -> StartStackIncreaseTimer
/// @3080 (always while she sings: the count never passes MaxStacks; at the cap AddStack only restarts the stack life
/// timer, so the stacks do not decay while anyone sings).
fn sing_timer(w: &mut World, f: FighterId, b: BuffRef) -> bool {
    let Some(e) = sing_ext(w, b) else { return false };
    if e.inc < 0.0 || e.paused {
        return false;
    }
    if e.pending {
        e.pending = false;
        return false;
    }
    e.inc -= 1.0;
    if e.inc > 0.0 {
        return false;
    }
    e.inc = -1.0;
    stack_buff(w, f, b, 0.0);
    let Some(stacks) = buff(w, b).filter(|x| x.frames > 0.0).map(|x| x.stacks) else { return false };
    let d = def(w, b);
    let Some(e) = sing_ext(w, b) else { return false };
    if stacks <= num(&d, "MaxStacks").unwrap_or(1.0) && e.active > 0.0 {
        start_increase(e, &d, true);
    }
    false
}

/// BPI After Status Update Multicast @230 (the owner's UI broker; a fighter has one): removed, or NewStackCount <= 0,
/// or 1 while going down (below PreviousCountValue) -> RemoveStateForPlayer(Sing); else PreviousCountValue = it and
/// AddStateForPlayer(Sing: StateType 1, DisplayType 0, NewStackCount - 1 of MaxStacks - 1, Timer -1, MI_State_Music,
/// active, PopText '', AutoRemoveTime -1). The hud draws the ops (the icon). Then @1300, whatever the broker did:
/// NewStackCount >= ArmorBreakStack -> PS_SharpenBuff_Loop on both hands (the view's) and
/// PF_Post_Event(sfx_cha_garnet_attack_special_n_gauntlets_glow, null, self) @2434, which posts nothing (a null
/// actor); below -> RemoveVFX (the view's).
fn sing_status(w: &mut World, f: FighterId, _class: BuffClass, n: Real, _applied: bool, removed: bool) {
    let cls = name!("Buff_GarnetStackingSinging_C");
    let live = w
        .fighter(f)
        .buffs
        .buffs
        .iter()
        .find(|x| x.cls == cls && x.frames > 0.0)
        .map(|x| BuffRef { fighter: f, serial: x.serial });
    let prev = match live {
        Some(b) => sing_ext(w, b).map_or(0.0, |e| e.prev),
        None => 0.0,
    };
    if removed || n <= 0.0 || (n == 1.0 && n < prev) {
        w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Remove(name!("Sing")));
        return;
    }
    if let Some(b) = live
        && let Some(e) = sing_ext(w, b)
    {
        e.prev = n;
    }
    let max_stacks = buff_def(w, ActorRef::Fighter(f), cls).and_then(|d| num(d, "MaxStacks")).unwrap_or(1.0);
    w.fighter_mut(f).controls.ui_state_ops.push(UiStateOp::Add(UiStateParams {
        id: name!("Sing"),
        state_type: 1,
        display_type: 0,
        current_value: n - 1.0,
        max_value: max_stacks - 1.0,
        timer: -1.0,
        icon_material: name!("MI_State_Music"),
        active: true,
        pop_text: crate::names::Name::NONE,
        auto_remove_time: -1.0,
        subicon: None,
    }));
}

/// This file's registry entries: the two notifies, then Buff_GarnetStackingSinging_C (sing.rs's order). Its
/// ReceiveEndPlay @3115 (the parent's, then RemoveVFX) is the view's; its timer goes with the actor.
pub fn register(r: &mut Registry) -> Result<(), RegistryError> {
    r.notifies.enter(
        "AnimNotify_GarnetStartSinging_C",
        NotifyHandler { begin: Some(start_singing_begin), end: Some(start_singing_end), ..NotifyHandler::default() },
    )?;
    r.notifies.enter(
        "AnimNotify_GarnetStartSingBuff_C",
        NotifyHandler { begin: Some(start_sing_buff_begin), ..NotifyHandler::default() },
    )?;
    r.buffs.enter(
        STACKING_SINGING,
        BuffEntry::new(
            BuffClass::BuffGarnetStackingSinging,
            BuffHooks {
                begin: Some(sing_begin),
                stacks: Some(sing_stacks),
                timer: Some(sing_timer),
                status: Some(sing_status),
                ..BuffHooks::default()
            },
        ),
    )
}
