//! The advanced tutorials' recovery and elements family (`elements.rs`, `elements/{common,downspike,
//! fire}.rs`): the minigames TutorialMinigame_DownSpike, TutorialMinigame_Elements_ExtinguishFire and
//! TutorialMinigame_Elements_Ignite, the hazards they spawn, and the demo controllers
//! TutorialController_KillBotOffPlatform and TutorialController_Elements_*.

use crate::Real;
use crate::State;
use crate::actors::projectile::MakeOpts;
use crate::actors::{ActorRef, ProjectileClass};
use crate::ai::bt::BotController;
use crate::ai::pad::Btn as BotBtn;
use crate::combat::BuffClass;
use crate::consts::DELTA_SECONDS;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::common::{actor_def, actor_is_a, lookup_owner, warn_once};
use crate::fighter::match_side::TutorialEvent;
use crate::fighter::moves::V2;
use crate::input::{Btn, Dir};
use crate::math::{max, min, narrow, to_int32};
use crate::name;
use crate::names::Name;
use crate::world::World;
use crate::world::flow::{ActorList, DelegateCall, DelegateList, PawnList};
use crate::world::pawns;
use mvs_data::value::{Props, Value};

use super::super::classes::{self, Call, Fired, RunList, demo_mut, demo_of, minigame_mut, minigame_of};
use super::super::demo::{DemoPad, DemoState, Place, active, callable, get_pawn, place_of, teleport};
use super::super::minigame::{self, MinigameState};
use super::super::pawn::{self, BagSpawn};
use super::super::run;
use super::super::{TutorialObj, latent};

/// TutorialBotClass: LoadClassAsset_Blocking(Pawn_WonderWomanV3) (DownSpike @8228, KillBotOffPlatform @4034).
const BOT: Name = name!("wonderwoman");
const OFF_LEDGE: &str = "OffLedge";
/// EMovementMode Walking.
const MOVEMENT_WALKING: i32 = 1;
/// OffLedgeCoopMinimumTime (the class default).
const MINIMUM_TIME: Real = 50.0;
const SPIT: Name = name!("CreatureSpit_C");
const FIRE_WALL: Name = name!("FireWallHazard_C");
const FIREBOLT: Name = name!("Firebolt_C");

// ---- the classes' state (the record writes them under these names)

/// TutorialController_KillBotOffPlatform.
#[derive(State, Debug, Default)]
pub struct KillBotOffPlatform {
    /// `destroyed` (the controller's DelegateOwner flag).
    pub destroyed: bool,
    /// KillBotOffPlatformComplete.
    pub complete: bool,
    pub at: Option<Place>,
    /// K2Node_DynamicCast_AsBase_Fighter_1 (the spawn chain's).
    pub cast: Option<FighterId>,
    /// K2Node_DynamicCast_AsBase_Fighter (Attack Loop's).
    pub loop_cast: Option<FighterId>,
    /// CallFunc_FinishSpawningActor_ReturnValue_1.
    pub bot: Option<FighterId>,
    /// The `world` (the cleanup's: TutorialDemoComplete passes none): the record writes it null until
    /// ReceivePossess.
    pub world_set: bool,
    /// ReceivePossess's pawn (the Delay's capture).
    pub possessed: Option<FighterId>,
}

/// TutorialController_Elements_ExtinguishFire.
#[derive(State, Debug, Default)]
pub struct ExtinguishFireDemo {
    pub fighter: Option<FighterId>,
    pub bag: Option<FighterId>,
    /// ReceivePossess's pawn (the Delay's capture).
    pub possessed: Option<FighterId>,
}

/// TutorialController_Elements_Ignite.
#[derive(State, Debug, Default)]
pub struct IgniteDemo {
    pub fighter: Option<FighterId>,
    pub bag: Option<FighterId>,
    pub possessed: Option<FighterId>,
}

/// TutorialMinigame_DownSpike.
#[derive(State, Debug, Default)]
pub struct DownSpike {
    /// The world tick of the BeginPlay.
    pub start_tick: i32,
    /// CallFunc_FinishSpawningActor_ReturnValue_3: the latest Wonder Woman spawned (every read of the graph takes it,
    /// whichever bot the event came from).
    pub bot: Option<FighterId>,
    /// The DoOnce at @8495 (Temp_bool_IsClosed_Variable).
    pub time_limit_set: bool,
}

/// TutorialMinigame_Elements_ExtinguishFire: no fields.
#[derive(State, Debug, Default)]
pub struct ExtinguishFire {}

/// TutorialMinigame_Elements_Ignite: no fields.
#[derive(State, Debug, Default)]
pub struct Ignite {}

// ---- the family's classes (`classes.rs` asks every family; a class entered in two is an error)

/// The demo controller of a DemoAIController class of this family (its `DemoState`), `None` for a class it lacks.
pub fn make_demo(cls: &str) -> Option<(&'static str, DemoState)> {
    use DemoState as D;
    Some(match cls {
        "TutorialController_KillBotOffPlatform" => {
            ("TutorialController_KillBotOffPlatform", D::KillBotOffPlatform(KillBotOffPlatform::default()))
        }
        "TutorialController_Elements_ExtinguishFire" => {
            ("TutorialController_Elements_ExtinguishFire", D::ExtinguishFire(ExtinguishFireDemo::default()))
        }
        "TutorialController_Elements_Ignite" => {
            ("TutorialController_Elements_Ignite", D::Ignite(IgniteDemo::default()))
        }
        _ => return None,
    })
}

/// The minigame actor of a CustomTutorialActors class of this family, `None` for a class it lacks.
pub fn make_minigame(cls: &str) -> Option<(&'static str, MinigameState)> {
    use MinigameState as M;
    Some(match cls {
        "TutorialMinigame_DownSpike" => ("TutorialMinigame_DownSpike", M::DownSpike(DownSpike::default())),
        "TutorialMinigame_Elements_ExtinguishFire" => {
            ("TutorialMinigame_Elements_ExtinguishFire", M::ExtinguishFire(ExtinguishFire::default()))
        }
        "TutorialMinigame_Elements_Ignite" => ("TutorialMinigame_Elements_Ignite", M::Ignite(Ignite::default())),
        _ => return None,
    })
}

/// A TutorialActor child class's own ReceiveBeginPlay, after the parent's: nothing for a class this family lacks.
pub fn actor_begin_play(w: &mut World, cls: &str) {
    let _ = (w, cls);
}

/// The characters a class of this family is drawn with (`TutorialClasses.drawnWith`): none (the family has no
/// entries).
pub fn drawn_with(cls: &str) -> &'static [&'static str] {
    let _ = cls;
    &[]
}

// ---- what the classes share

/// The state of the demo class of `id`, by its variant.
macro_rules! dst {
    ($w:expr, $id:expr, $v:ident) => {
        match &mut demo_mut($w, $id).state {
            DemoState::$v(s) => s,
            _ => unreachable!(),
        }
    };
}

/// The state of the live minigame of `id`, by its variant.
macro_rules! mst {
    ($w:expr, $id:expr, $v:ident) => {
        match &mut minigame_mut($w, $id).expect("a live minigame").state {
            MinigameState::$v(s) => s,
            _ => unreachable!(),
        }
    };
}

fn pad(w: &mut World, id: u32) -> &mut DemoPad {
    &mut demo_mut(w, id).pad
}

fn delay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Demo(id), node, s);
}

fn mdelay(w: &mut World, id: u32, node: &'static str, s: Real) {
    latent::delay(w, TutorialObj::Minigame(id), node, s);
}

fn set_timer(w: &mut World, id: u32, name: &'static str, s: Real) {
    latent::set_timer(w, TutorialObj::Demo(id), name, s, true);
}

fn clear_timer(w: &mut World, id: u32, name: &'static str) {
    latent::clear_timer(w, TutorialObj::Demo(id), name);
}

fn first_possess(w: &World, id: u32, node: &'static str, f: FighterId, cur: Option<FighterId>) -> Option<FighterId> {
    if latent::delay_pending(w, TutorialObj::Demo(id), node) { cur } else { Some(f) }
}

fn unhandled(w: &World, id: u32, c: Call<'_>) -> ! {
    panic!("tutorial: {} has no handler for {c:?}", demo_of(w, id).cls);
}

fn unhandled_minigame(w: &World, id: u32, c: Call<'_>) -> ! {
    panic!("tutorial: {} has no handler for {c:?}", minigame_of(w, id).map_or("a destroyed minigame", |m| m.cls));
}

/// SpawnEmitterAtLocation(PS_MeeseeksSpawn, the pawn's location, rotation 0, scale 1) by a tutorial object, raised as
/// a located effect with no instigator (its Team the no-instigator 0.6).
/// Unverified: the build's emitter keeps the system's own Team default unless the graph sets it (the DownSpike cleanup's
/// SetFloatParameter(Team, character == Reindog) @4265: 0 for her); the drawn Team is 0.6 either way.
fn meeseeks_spawn_fx(w: &mut World, cls: Name, f: FighterId) {
    let (x, y) = (w.fighter(f).body.x, crate::fighter::actor_y(w, f));
    w.events.push(Event::ProjectileDeath {
        cls,
        fighter: -1,
        vfx: Some(name!("PS_MeeseeksSpawn")),
        x,
        y,
        ox: 0.0,
        rot: [0.0, 0.0, 0.0],
        scale: 1.0,
        tint: None,
        to: None,
    });
}

/// The poof: `meeseeksSpawnFx` and the pooled sfx_map_tutorial_meeseeks_poof at the pawn's place.
fn poof(w: &mut World, cls: Name, f: FighterId) {
    meeseeks_spawn_fx(w, cls, f);
    let (x, y) = (w.fighter(f).body.x, crate::fighter::actor_y(w, f));
    pawn::sound_at(w, name!("sfx_map_tutorial_meeseeks_poof"), x, y);
}

/// The full teardown a minigame gives its bot (TutorialMinigame_DownSpike @611-923, @10463-10736): PlayerState
/// K2_DestroyActor, the AI controller's SetLifeSpan(0), its BrainComponent destroyed, UnPossess, the pawn's
/// K2_DestroyActor (`despawn_pawn` with the teardown).
fn tear_down(w: &mut World, f: Option<FighterId>) {
    if let (true, Some(f)) = (callable(w, f), f) {
        pawns::despawn_pawn(w, f, true);
    }
}

/// The OffLedge points (Actor_RecoveryBotSpawn and Actor_RecoveryBotSpawn2), in the order GetAllActorsWithTag gives.
fn off_ledge(w: &World) -> Vec<Vec<f64>> {
    pawn::actors_with_tag(&w.data.stage, OFF_LEDGE).into_iter().map(|a| a.loc.clone()).collect()
}

fn is_character(w: &World, f: FighterId, key: &str) -> bool {
    w.names.str(w.fighter(f).ch) == key
}

fn managed(w: &World) -> Option<FighterId> {
    w.tutorial.as_ref().and_then(|t| t.actor.as_ref()).and_then(|a| a.managed)
}

// ---- the dispatch

/// A call on one of this family's demo controllers (`TutorialRun::demos`, by id).
pub fn demo(w: &mut World, id: u32, c: Call<'_>) {
    match &demo_of(w, id).state {
        DemoState::KillBotOffPlatform(_) => kill_bot(w, id, c),
        DemoState::ExtinguishFire(_) => extinguish_fire_demo(w, id, c),
        DemoState::Ignite(_) => ignite_demo(w, id, c),
        _ => unreachable!("not an elements demo controller"),
    }
}

/// A call on one of this family's minigame actors (`TutorialActor::minigames`, by id).
pub fn minigame(w: &mut World, id: u32, c: Call<'_>) {
    match &minigame_of(w, id).expect("a live minigame").state {
        MinigameState::DownSpike(_) => down_spike(w, id, c),
        MinigameState::ExtinguishFire(_) => extinguish_fire(w, id, c),
        MinigameState::Ignite(_) => ignite(w, id, c),
        _ => unreachable!("not an elements minigame"),
    }
}

// ---- TutorialMinigame_DownSpike

/// TutorialMinigame_DownSpike.
/// ReceiveBeginPlay @8851: OffLedgeCoopStartTime = GetGameTimeInSeconds; the TutorialActor's TutorialStepComplete to
/// CleanUpOffLedgeCoop and TutorialStepCleanup to OffLedgeMinigameCleanup (@7702-8153); TutorialBotClass (@8228); a
/// Comp_Actor_KeepOnCamera added to each OffLedge actor (@5994); Delay 0.5 (@6141 -> @1652): a looping 5.0 s timer
/// (RecoveryFighterSpawnTimer) to Spawn Off Ledge Fighter, and one call now.
/// Not ported: SpawnFriendlyReinDog's branch (@6196-7066: a Reindog at the origin with an Idle controller, whose down
/// special the spawn chain presses); the flag's default is false and nothing writes it, so FinishSpawningActor_1 (the
/// Reindog) is never valid and every IsValid on it (@3342, @3531, @9119, @9624) takes its false pin.
/// Not ported: the KeepOnCamera components (@5994) and their Unregister at the cleanup (@7346-7546): the port's camera
/// has no route for a placed actor.
/// The minigame never destroys itself but in its cleanup (@5327).
fn down_spike(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Minigame(id);
    let cls = name!("TutorialMinigame_DownSpike");
    match c {
        Call::BeginPlay => {
            minigame::bind_steps(w, id, true, true);
            mst!(w, id, DownSpike).start_tick = w.flow.tick;
            mdelay(w, id, "@6141", 0.5);
        }
        Call::Fired(Fired::Delay("@6141")) => {
            latent::set_timer(w, me, "RecoveryFighterSpawnTimer", 5.0, true);
            spawn_off_ledge_fighter(w, id);
        }
        Call::Fired(Fired::Timer("RecoveryFighterSpawnTimer")) => spawn_off_ledge_fighter(w, id),
        // CleanUpOffLedgeCoop (@10773): both timers cleared.
        Call::Delegate { handler: "onStepComplete", .. } => {
            latent::clear_timer(w, me, "OffLedgeCoopTimeLimitTimer");
            latent::clear_timer(w, me, "RecoveryFighterSpawnTimer");
        }
        // OffLedgeMinigameCleanup (@10955 -> @10059): for every BP_FighterCharacter whose BPI_GetFighterCharacter is 11
        // (Wonder Woman) or 6 (Reindog) (@3863-4009): PS_MeeseeksSpawn at it with Team = (character == 6) (@4128-4265),
        // the poof, the full teardown; then K2_DestroyActor on itself (@5327).
        // Unverified: GetAllActorsOfClass's order is taken as the match's pawn order (World.pawns).
        Call::Delegate { handler: "onStepCleanup", .. } => {
            for f in w.pawns.clone().unwrap_or_default() {
                if !callable(w, Some(f)) || !(is_character(w, f, "wonderwoman") || is_character(w, f, "creature")) {
                    continue;
                }
                poof(w, cls, f);
                tear_down(w, Some(f));
            }
            minigame::destroy(w, id);
        }
        // Spawn Off Ledge Fighter (@9119): the Reindog invalid, Delay 2.0 (@9413 -> @3342), the Reindog invalid, Delay
        // 0.5 (@3476 -> @1770): the spawn.
        Call::Fired(Fired::Delay("@9413")) => mdelay(w, id, "@3476", 0.5),
        Call::Fired(Fired::Delay("@3476")) => spawn_bot(w, id),
        Call::Fired(Fired::Delay("@3172")) => possess_bot(w, id),
        Call::Fired(Fired::Delay("@1597")) => {
            let b = mst!(w, id, DownSpike).bot;
            let (true, Some(b)) = (callable(w, b), b) else { return };
            poof(w, cls, b);
            tear_down(w, Some(b));
        }
        Call::Delegate {
            handler: "shaggyWon",
            src: Some(ActorRef::Fighter(f)),
            call: DelegateCall::MovementModeChanged { mode, .. },
        } => {
            shaggy_won(w, cls, f, *mode);
        }
        Call::Delegate { handler: "hit", call: DelegateCall::HitByEnemy { dir_x, dir_y, .. }, .. } => {
            shaggy_hit(w, id, *dir_x, *dir_y);
        }
        Call::Fired(Fired::Delay("@9791")) => mdelay(w, id, "@3665", 0.5),
        Call::Fired(Fired::Delay("@3665")) => {
            let b = mst!(w, id, DownSpike).bot;
            tear_down(w, b);
        }
        // End Time (@9104 -> OffLedgeCoopProceed @10950 -> @10814): GameState.BPI_ProcessTutorialEvent(28
        // CustomActorEvent) (@10910).
        Call::Fired(Fired::Timer("OffLedgeCoopTimeLimitTimer")) => {
            minigame::process_event(w, TutorialEvent::CustomActorEvent);
        }
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

fn spawn_off_ledge_fighter(w: &mut World, id: u32) {
    mdelay(w, id, "@9413", 2.0);
}

/// @1770-3172: RandomInteger(the OffLedge count) for BeginDeferredActorSpawnFromClass's transform and a second
/// RandomInteger for FinishSpawningActor's, which places the pawn (the first draw's transform is replaced); FacingLeft
/// true; SpawnInvulnerabilityTime 0; EnableInvulnerability(true); PlayerDataReadyServer(1); PS_MeeseeksSpawn and
/// sfx_cha_tutorial_meeseeks_cast at its location; the team of player data 1; CurrentDamage 200; Delay 0.5 (@3172 ->
/// @960).
/// Unverified: the yaw of the point's transform (180) is left to FacingLeft (the pawn's facing is its FacingLeft).
fn spawn_bot(w: &mut World, id: u32) {
    let pts = off_ledge(w);
    if pts.is_empty() {
        warn_once(w, name!("TutorialMinigame_DownSpike: no actor tagged OffLedge"));
        return;
    }
    let n = narrow(pts.len() as f64);
    w.random_int(n); // (Begin's transform)
    let i = usize::try_from(to_int32(w.random_int(n))).expect("a draw below the count");
    let (x, y) = pawn::place_of_loc(&pts[i]);
    let team = run::data_team(w, 1);
    let f = run::spawn_fighter(w, BOT, x, y, true, 1, team);
    mst!(w, id, DownSpike).bot = Some(f);
    pawn::set_spawn_invulnerability_time(w, f, 0.0);
    crate::fighter::control::enable_invulnerability(w, f, true, false);
    meeseeks_spawn_fx(w, name!("TutorialMinigame_DownSpike"), f);
    let (fx, fy) = (w.fighter(f).body.x, crate::fighter::actor_y(w, f));
    pawn::sound_at(w, name!("sfx_cha_tutorial_meeseeks_cast"), fx, fy);
    pawn::set_current_damage(w, f, 200.0);
    mdelay(w, id, "@3172", 0.5);
}

/// @960: TutorialRecoverOnly_AIController possesses the latest bot; EnableInvulnerability(false); SetRotation(false);
/// MovementModeChangedDelegate to Shaggy Won, HitByEnemy to Shaggy Hit; Delay 3.0 (@1597 -> @15): the latest bot
/// still valid: PS_MeeseeksSpawn, the poof, the full teardown.
/// Unverified: Possess, EnableInvulnerability and the binds on a bot destroyed in the 0.5 s (its ring-out) are taken to
/// do nothing (the pawn is pending kill).
fn possess_bot(w: &mut World, id: u32) {
    let me = TutorialObj::Minigame(id);
    let f = mst!(w, id, DownSpike).bot;
    if let (true, Some(f)) = (callable(w, f), f) {
        run::possess_bot(w, f, BotController::TutorialRecoverOnly);
        crate::fighter::control::enable_invulnerability(w, f, false, false);
        pawn::set_rotation(w, f, false);
        classes::bind(w, DelegateList::Pawn(f, PawnList::MovementModeChanged), me, "shaggyWon");
        classes::bind(w, DelegateList::Pawn(f, PawnList::HitByEnemy), me, "hit");
    }
    mdelay(w, id, "@1597", 3.0);
}

/// Shaggy Won (@10131): the character's MovementMode Walking: PS_MeeseeksSpawn, the poof, the full teardown of that
/// character.
fn shaggy_won(w: &mut World, cls: Name, f: FighterId, mode: i32) {
    if mode != MOVEMENT_WALKING || !callable(w, Some(f)) {
        return;
    }
    poof(w, cls, f);
    tear_down(w, Some(f));
}

/// Shaggy Hit (@9468): DotProduct2D(Normal2D(KnockBackDirectionVector), (0, -1)) >= 0.9 (a spike: the knockback
/// within about 26 degrees of straight down). Then, in the order the pushed flows run: Delay 0.5 (@9791 -> @3531),
/// the Reindog invalid, Delay 0.5 (@3665 -> @611): the full teardown of the latest bot, no effect; HitByEnemy of the
/// latest bot unbound from Shaggy Hit (@5446, once per TutorialActor: the one); IncrementMinigameCounter(true)
/// (@10043); the DoOnce (@8813 -> @8495): a one-shot timer of FClamp(OffLedgeCoopMinimumTime - (now -
/// OffLedgeCoopStartTime), 1.5, OffLedgeCoopMinimumTime) (OffLedgeCoopTimeLimitTimer) to End Time (@9104 ->
/// OffLedgeCoopProceed @10950 -> @10814): GameState.BPI_ProcessTutorialEvent(28 CustomActorEvent) (@10910).
/// So the task (CustomActorEvent once) completes 50 s after the start, or 1.5 s after the first spike when that comes
/// later; the spikes until then are the counter (the saved score).
/// GetGameTimeInSeconds' difference is taken as the frames since the BeginPlay times the world delta.
fn shaggy_hit(w: &mut World, id: u32, dx: Real, dy: Real) {
    let me = TutorialObj::Minigame(id);
    let l = (dx * dx + dy * dy).sqrt();
    let dot = if l > 0.0 { -dy / l } else { 0.0 };
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "negated test: a NaN dot returns")]
    if !(dot >= 0.9) {
        return;
    }
    mdelay(w, id, "@9791", 0.5);
    let bot = mst!(w, id, DownSpike).bot;
    if let Some(b) = bot {
        classes::unbind(w, DelegateList::Pawn(b, PawnList::HitByEnemy), me, Some("hit"));
    }
    minigame::increment_counter(w, id, true);
    if mst!(w, id, DownSpike).time_limit_set {
        return;
    }
    mst!(w, id, DownSpike).time_limit_set = true;
    let start = mst!(w, id, DownSpike).start_tick;
    let elapsed = narrow(f64::from(w.flow.tick - start) * f64::from(DELTA_SECONDS));
    let t = min(max(MINIMUM_TIME - elapsed, 1.5), MINIMUM_TIME);
    latent::set_timer(w, me, "OffLedgeCoopTimeLimitTimer", t, false);
}

// ---- TutorialController_KillBotOffPlatform

/// TutorialController_KillBotOffPlatform.
/// ReceiveBeginPlay (@4426 -> @4034): TutorialBotClass; the TutorialActor's TutorialDemoComplete to
/// KillBotOffPlatformCleanup (@1812), bound here at the controller's making (its BeginPlay).
/// ReceivePossess: Delay 0.5 (@2250 -> @460): InitialLocation = the possessed pawn's location; a looping 3.0 s timer
/// (KillBotOffLedgeTimer) to Attack Loop; Delay 0.6 (@640 -> @695); Delay 1.0 (@695 -> @750): Spawn Recovery Bot;
/// then K2_GetPawn as a BaseFighter, ActiveInTutorialIntro: MoveX(-1); Delay 0.5; JumpPressed; Delay 0.05;
/// JumpReleased; Delay 0.0; NormalAttackPressed(1 Down) and MoveX(1); Delay 0.5; MoveX(0).
/// Attack Loop (@2113): K2_GetPawn as a BaseFighter: SetRotation(false); MoveComponentTo(its root, InitialLocation,
/// its rotation, ease out, ease in, 0.6) (@1557 -> @1383): SetRotation(true), then the Delay 1.0 node @695 (a call
/// while it runs starts nothing).
/// Not ported: the KeepOnCamera the minigame's points get (none here: the demo adds none).
/// Deviation: the TutorialDemoComplete bind is made at the first ReceivePossess, not at the controller's making
/// (`classes::make_demo` returns the state alone): the controller is made and possessed in one call chain, so the
/// run's lists read the same at every tick end.
fn kill_bot(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Demo(id);
    match c {
        Call::Possessed(f) => {
            classes::bind_run(w, RunList::DemoComplete, me, "cleanup");
            dst!(w, id, KillBotOffPlatform).world_set = true;
            // (the possess Delay's capture: the first call's pawn while it is pending)
            let cur = demo_possessed(w, id);
            let p = first_possess(w, id, "@2250", f, cur);
            set_demo_possessed(w, id, p);
            delay(w, id, "@2250", 0.5);
        }
        Call::Fired(Fired::Delay("@2250")) => {
            let f = demo_possessed(w, id);
            if let (true, Some(f)) = (callable(w, f), f) {
                let at = place_of(w, f);
                dst!(w, id, KillBotOffPlatform).at = Some(at);
            }
            set_timer(w, id, "KillBotOffLedgeTimer", 3.0);
            delay(w, id, "@640", 0.6);
        }
        Call::Fired(Fired::Timer("KillBotOffLedgeTimer")) => {
            if dst!(w, id, KillBotOffPlatform).destroyed {
                return;
            }
            attack_loop(w, id);
        }
        Call::Fired(Fired::Delay("@640")) => kill_bot_attack(w, id),
        Call::Fired(Fired::Delay("@695")) => {
            spawn_recovery_bot(w, id);
            let p = get_pawn(w, id);
            dst!(w, id, KillBotOffPlatform).cast = p;
            if !active(w, p) {
                return;
            }
            pad(w, id).move_x(-1.0);
            delay(w, id, "@932", 0.5);
        }
        Call::Fired(Fired::Delay("@932")) => {
            let cast = dst!(w, id, KillBotOffPlatform).cast;
            if callable(w, cast) {
                pad(w, id).press(Btn::Jump, None);
            }
            delay(w, id, "@1023", 0.05);
        }
        Call::Fired(Fired::Delay("@1023")) => {
            let cast = dst!(w, id, KillBotOffPlatform).cast;
            if callable(w, cast) {
                pad(w, id).release(Btn::Jump);
            }
            delay(w, id, "@1114", 0.0);
        }
        Call::Fired(Fired::Delay("@1114")) => {
            let cast = dst!(w, id, KillBotOffPlatform).cast;
            if callable(w, cast) {
                let pd = pad(w, id);
                pd.press(Btn::Attack, Some(Dir::Down));
                pd.move_x(1.0);
            }
            delay(w, id, "@1249", 0.5);
        }
        Call::Fired(Fired::Delay("@1249")) => {
            let cast = dst!(w, id, KillBotOffPlatform).cast;
            if callable(w, cast) {
                pad(w, id).move_x(0.0);
            }
        }
        Call::Fired(Fired::Move("@1557")) => {
            let lc = dst!(w, id, KillBotOffPlatform).loop_cast;
            if let (true, Some(lc)) = (callable(w, lc), lc) {
                pawn::set_rotation(w, lc, true);
            }
            kill_bot_attack(w, id);
        }
        Call::Fired(Fired::Delay("@3610")) => {
            let bot = dst!(w, id, KillBotOffPlatform).bot;
            if let (true, Some(b)) = (callable(w, bot), bot) {
                run::despawn(w, b);
            }
        }
        Call::Delegate { handler: "hit", .. } => {
            if dst!(w, id, KillBotOffPlatform).destroyed {
                return;
            }
            // Shaggy Hit: Delay 1.0 (@3610 -> @1346): K2_DestroyActor of the latest bot (any hit).
            delay(w, id, "@3610", 1.0);
        }
        Call::Delegate {
            handler: "shaggyWon",
            src: Some(ActorRef::Fighter(f)),
            call: DelegateCall::MovementModeChanged { mode, .. },
        } => {
            if dst!(w, id, KillBotOffPlatform).destroyed {
                return;
            }
            // Shaggy Won (@3665): the character's MovementMode Walking: PS_MeeseeksSpawn, the poof, its K2_DestroyActor.
            if *mode != MOVEMENT_WALKING || !callable(w, Some(f)) {
                return;
            }
            poof(w, name!("TutorialController_KillBotOffPlatform"), f);
            run::despawn(w, f);
        }
        Call::Delegate { handler: "cleanup", .. } => kill_bot_cleanup(w, id),
        Call::Unpossessed { .. } => {}
        _ => unhandled(w, id, c),
    }
}

/// The pawn ReceivePossess's Delay captured.
fn demo_possessed(w: &mut World, id: u32) -> Option<FighterId> {
    dst!(w, id, KillBotOffPlatform).possessed
}

fn set_demo_possessed(w: &mut World, id: u32, f: Option<FighterId>) {
    dst!(w, id, KillBotOffPlatform).possessed = f;
}

/// @695: Delay 1.0, then @750.
fn kill_bot_attack(w: &mut World, id: u32) {
    delay(w, id, "@695", 1.0);
}

fn attack_loop(w: &mut World, id: u32) {
    let p = get_pawn(w, id);
    let Some(p) = p else { return };
    dst!(w, id, KillBotOffPlatform).loop_cast = Some(p);
    if callable(w, Some(p)) {
        pawn::set_rotation(w, p, false);
    }
    let at = dst!(w, id, KillBotOffPlatform).at;
    let (true, Some(at)) = (callable(w, Some(p)), at) else { return };
    latent::move_to(w, TutorialObj::Demo(id), "@1557", latent::MoveTarget::Fighter(p), at.x, at.y, 0.6, true, true);
}

/// Spawn Recovery Bot (@2305): unless KillBotOffPlatformComplete: a deferred TutorialBotClass at OffLedge[0]'s
/// transform (no FacingLeft written), SpawnInvulnerabilityTime 0, PlayerDataReadyServer(1), the team of player data
/// 1, CurrentDamage 200; a TutorialRecoverOnly_AIController possesses it; HitByEnemy to Shaggy Hit and
/// MovementModeChangedDelegate to Shaggy Won.
/// Unverified: the pawn's facing is its class's FacingLeft default (false: facing right, toward the stage); the yaw
/// 180 of the point's transform is not read as a facing.
fn spawn_recovery_bot(w: &mut World, id: u32) {
    if dst!(w, id, KillBotOffPlatform).complete {
        return;
    }
    let pts = off_ledge(w);
    let Some(pt) = pts.first() else {
        warn_once(w, name!("TutorialController_KillBotOffPlatform: no actor tagged OffLedge"));
        return;
    };
    let (x, y) = pawn::place_of_loc(pt);
    let team = run::data_team(w, 1);
    let f = run::spawn_fighter(w, BOT, x, y, false, 1, team);
    dst!(w, id, KillBotOffPlatform).bot = Some(f);
    pawn::set_spawn_invulnerability_time(w, f, 0.0);
    pawn::set_current_damage(w, f, 200.0);
    run::possess_bot(w, f, BotController::TutorialRecoverOnly);
    let me = TutorialObj::Demo(id);
    classes::bind(w, DelegateList::Pawn(f, PawnList::HitByEnemy), me, "hit");
    classes::bind(w, DelegateList::Pawn(f, PawnList::MovementModeChanged), me, "shaggyWon");
}

/// KillBotOffPlatformCleanup (@15): KillBotOffPlatformComplete = true, the timer cleared, K2_DestroyActor on every
/// actor of TutorialBotClass (every Wonder Woman of the match), then on itself (its latent actions and timers go).
/// Unverified: GetAllActorsOfClass's order is taken as the match's pawn order (World.pawns).
fn kill_bot_cleanup(w: &mut World, id: u32) {
    // a destroyed controller's binding is skipped (a Multicast drops a destroyed owner's entry)
    if dst!(w, id, KillBotOffPlatform).destroyed {
        return;
    }
    let world_set = dst!(w, id, KillBotOffPlatform).world_set;
    dst!(w, id, KillBotOffPlatform).complete = true;
    clear_timer(w, id, "KillBotOffLedgeTimer");
    if world_set {
        for f in w.pawns.clone().unwrap_or_default() {
            if callable(w, Some(f)) && is_character(w, f, "wonderwoman") {
                run::despawn(w, f);
            }
        }
    }
    dst!(w, id, KillBotOffPlatform).destroyed = true;
    latent::drop_owner(w, TutorialObj::Demo(id));
}

// ---- the Elements tutorial

/// TutorialController_Elements_ExtinguishFire / _Ignite SpawnPunchingBag(SpawnTag, 1, PawnClass) (@0-1271, the same
/// function in both): SpawnInvulnerabilityTime 0, the team of player data 1, CurrentDamage 0.
fn elements_bag(w: &mut World, tag: &str, character: Name) -> Option<FighterId> {
    let team = run::data_team(w, 1);
    pawn::spawn_punching_bag(
        w,
        BagSpawn { tag, character, slot: 1, team, damage: 0.0, spawn_invulnerability_time: Some(0.0) },
    )
}

/// TutorialMinigame_Elements_ExtinguishFire.
/// ReceiveBeginPlay @1536: Player = GetPlayerPawn(0) as a BaseFighter (no pawn: nothing more); TutorialStepComplete to
/// OnTutorialComplete (@1891: nothing) and TutorialStepCleanup to CleanUpSpawnedActors (@1886 -> @15: K2_DestroyActor
/// on PunchingBag, a variable nothing writes: nothing), so neither is bound here. Then for each actor tagged FireWall
/// (@492; Actor_Firewall .. Actor_Firewall4) a deferred FireWallHazard_C at its transform with owner None, AttackID 2
/// (@861), FinishSpawningActor; then on it a Comp_Actor_KeepOnCamera added and registered (@1045), its ParticleSystem's
/// Team 0.0 (@1175), its Decal's dynamic material's Team 0.0 (@1251) and OnDestroyed to OnFireExtinguished (@1346).
/// OnFireExtinguished: GameState.BPI_ProcessTutorialEvent(74 UniqueCharacterMechanic2) (@1988), for every destroy of
/// a wall: an ice breath's (its Buff_Elemental_Fire ended) and its MaxLifetime's (8 s) alike.
/// The minigame never destroys itself, nor its walls: a wall still standing when the step completes counts its destroy
/// into the next step's task when that task counts the same event.
/// Not ported: the KeepOnCamera components (the port's camera has no route for a projectile). The Team 0.0 of the
/// wall's particle system and decal is the drawing side's: the sim keeps it as the wall's `fx_team`.
/// The class data the walls are made from is the managed pawn's (`actor_def`: Superman's perk parts carry
/// FireWallHazard_C); the world spawn (`spawn_world_projectile`) runs the BeginPlay.
fn extinguish_fire(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Minigame(id);
    match c {
        Call::BeginPlay => {
            let Some(m) = managed(w) else { return };
            let Some(a) = actor_def(w, ActorRef::Fighter(m), FIRE_WALL) else {
                warn_once(
                    w,
                    name!(
                        "TutorialMinigame_Elements_ExtinguishFire: no FireWallHazard_C in the pawn's data (no walls)"
                    ),
                );
                return;
            };
            let pm = a.components.get("ProjectileMovement").and_then(Value::as_obj);
            let num = |k: &str| pm.and_then(|p| p.get(k)).and_then(Value::as_f64).unwrap_or(0.0);
            let (speed, max_speed) = (num("InitialSpeed"), num("MaxSpeed"));
            let tags: Vec<(Vec<f64>, Vec<f64>)> = pawn::actors_with_tag(&w.data.stage, "FireWall")
                .into_iter()
                .map(|t| (t.loc.clone(), t.rot.clone()))
                .collect();
            for (loc, rot) in tags {
                // No BPI_SetInitialMovement on a graph's own spawn: the template's speeds stand.
                let rot_at = |i: usize| rot.get(i).copied().unwrap_or(0.0);
                let mut notify = Props::new();
                let dir: Props = ["X", "Y", "Z"].into_iter().map(|k| (k.to_owned(), Value::Num(0.0))).collect();
                notify.insert("InitialSpeed".to_owned(), Value::Num(speed));
                notify.insert("MaxSpeed".to_owned(), Value::Num(max_speed));
                notify.insert("InitialMovementDirection".to_owned(), Value::Obj(dir));
                let r: Props = [("Pitch", rot_at(0)), ("Yaw", rot_at(1)), ("Roll", rot_at(2))]
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), Value::Num(v)))
                    .collect();
                let tr: Props = std::iter::once(("Rotation".to_owned(), Value::Obj(r))).collect();
                notify.insert("Transforms".to_owned(), Value::Obj(tr));
                let (x, y) = pawn::place_of_loc(&loc);
                let opts = MakeOpts { lookup: Some(m), ..MakeOpts::default() };
                let mut p = crate::actors::projectile::make_projectile(
                    w,
                    FIRE_WALL,
                    ProjectileClass::FireWallHazard,
                    None,
                    2,
                    &notify,
                    Some(V2 { x, y }),
                    opts,
                );
                crate::actors::firewall::make_world_wall(w, &mut p, narrow(rot_at(1)));
                let pid = w.actors.projectiles.insert(p);
                crate::world::spawning::spawn_world_projectile(w, pid);
                if let Some(crate::actors::projectile::ProjectileExt::FireWall(fw)) =
                    w.actors.projectiles.get_mut(pid).map(|p| &mut p.ext)
                {
                    fw.fx_team = Some(0.0);
                }
                classes::bind(
                    w,
                    DelegateList::Actor(ActorRef::Projectile(pid), ActorList::Destroyed),
                    me,
                    "extinguished",
                );
            }
        }
        Call::Delegate { handler: "extinguished", .. } => {
            minigame::process_event(w, TutorialEvent::UniqueCharacterMechanic2);
        }
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

/// TutorialMinigame_Elements_Ignite.
/// ReceiveBeginPlay @1000: Player = GetPlayerPawn(0) as a BaseFighter (none: nothing more); TutorialStepComplete and
/// TutorialStepCleanup to handlers that do nothing (@1328 / @1327: not bound here); the game mode's PlayerRespawned to
/// Set Up Player Bindings (@898); the Player's Comp_Spawner.AfterSpawnedActor to On Projectile Spawned (@257).
/// Set Up Player Bindings(index) @15: index 0: Player = GetPlayerPawn(index), its AfterSpawnedActor bound the same.
/// On Projectile Spawned @1329: a CreatureSpit_C: its Comp_State_Manager.NewBuffSpawned to On Ptoo Buffed (@1427).
/// On Ptoo Buffed @1491: IsSoftRef(the buff class, Buff_Elemental_Fire_C): BPI_ProcessTutorialEvent(74) (@1740): the
/// spit passed through a fire wall (the elemental overlap gives it the wall's fire).
/// The minigame never destroys itself.
fn ignite(w: &mut World, id: u32, c: Call<'_>) {
    let me = TutorialObj::Minigame(id);
    match c {
        Call::BeginPlay => {
            let Some(m) = managed(w) else { return };
            classes::bind_run(w, RunList::PlayerRespawned, me, "setUpPlayerBindings");
            classes::bind(w, DelegateList::Pawn(m, PawnList::AfterSpawnedActor), me, "spawned");
        }
        Call::Delegate { handler: "setUpPlayerBindings", call: DelegateCall::PlayerRespawned { index }, .. } => {
            if *index != 0 {
                return;
            }
            if let Some(m) = managed(w) {
                classes::bind(w, DelegateList::Pawn(m, PawnList::AfterSpawnedActor), me, "spawned");
            }
        }
        Call::Delegate { handler: "spawned", call: DelegateCall::AfterSpawnedActor { actor }, .. } => {
            // (only a projectile has a class and an owner to cast: an item's is no CreatureSpit_C)
            let ActorRef::Projectile(pid) = *actor else { return };
            let Some(p) = w.actors.projectiles.get(pid) else { return };
            let pcls = p.cls;
            let own = ActorRef::Fighter(lookup_owner(w, *actor));
            if pcls != SPIT && actor_is_a(w, own, pcls, SPIT) != Some(true) {
                return;
            }
            classes::bind(w, DelegateList::Actor(*actor, ActorList::NewBuffSpawned), me, "buffed");
        }
        Call::Delegate { handler: "buffed", call: DelegateCall::NewBuffSpawned { cls }, .. } => {
            if *cls == BuffClass::BuffElementalFire {
                minigame::process_event(w, TutorialEvent::UniqueCharacterMechanic2);
            }
        }
        Call::Destroyed => {}
        _ => unhandled_minigame(w, id, c),
    }
}

/// TutorialController_Elements_ExtinguishFire.
/// ReceivePossess: Delay 0.1 (@1539 -> @15): DemoFighter = the possessed pawn (a BaseFighter); its own
/// SpawnPunchingBag(PunchingBag, 1, Pawn_Reindog); the bag's DisableCD = true; the bag K2_TeleportTo(its
/// location + (0, -50, 0)) (sim x + 50); a looping 10.0 s timer (ExtinguishFireDemoTimer) to ExtinguishFireDemo, and
/// one call now.
/// ExtinguishFireDemo (@1695): the bag's BPI_SpecialAttackPressed(1 Down) and BPI_SpecialAttackReleased (a fire wall);
/// Delay 4.0 (@1849 -> @929): DemoFighter's BPI_SpecialAttackPressed(4 Neutral) (the ice breath); Delay 2.0 (@967 ->
/// @1022): BPI_SpecialAttackReleased.
/// ReceiveUnPossess (@1594): K2_DestroyActor on the bag, the timer cleared, then K2_DestroyActor on every Firebolt_C,
/// then on every FireWallHazard_C.
fn extinguish_fire_demo(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, ExtinguishFire).possessed;
            let p = first_possess(w, id, "@1539", f, cur);
            dst!(w, id, ExtinguishFire).possessed = p;
            delay(w, id, "@1539", 0.1);
        }
        Call::Fired(Fired::Delay("@1539")) => {
            let f = dst!(w, id, ExtinguishFire).possessed;
            dst!(w, id, ExtinguishFire).fighter = f;
            let bag = elements_bag(w, "PunchingBag", name!("creature"));
            dst!(w, id, ExtinguishFire).bag = bag;
            if let Some(b) = bag {
                pawn::set_disable_cd(w, b, true);
                let at = place_of(w, b);
                teleport(w, Some(b), Some(Place { x: at.x + 50.0, y: at.y }));
            }
            set_timer(w, id, "ExtinguishFireDemoTimer", 10.0);
            extinguish_demo(w, id);
        }
        Call::Fired(Fired::Timer("ExtinguishFireDemoTimer")) => extinguish_demo(w, id),
        Call::Fired(Fired::Delay("@1849")) => {
            let f = dst!(w, id, ExtinguishFire).fighter;
            if callable(w, f) {
                pad(w, id).press(Btn::Special, Some(Dir::Neutral));
            }
            delay(w, id, "@967", 2.0);
        }
        Call::Fired(Fired::Delay("@967")) => {
            let f = dst!(w, id, ExtinguishFire).fighter;
            if callable(w, f) {
                pad(w, id).release(Btn::Special);
            }
        }
        Call::Unpossessed { .. } => {
            let bag = dst!(w, id, ExtinguishFire).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                run::despawn(w, b);
            }
            clear_timer(w, id, "ExtinguishFireDemoTimer");
            // (@1135-1275: the FireWallHazard loop is inside the Firebolt ForEach's body, so the walls go only when a
            // Firebolt is left)
            if destroy_all_of(w, FIREBOLT) {
                destroy_all_of(w, FIRE_WALL);
            }
        }
        _ => unhandled(w, id, c),
    }
}

fn extinguish_demo(w: &mut World, id: u32) {
    let bag = dst!(w, id, ExtinguishFire).bag;
    if let (true, Some(b)) = (callable(w, bag), bag) {
        pawn::bot_press(w, b, BotBtn::Special, Some(Dir::Down), false);
        pawn::bot_release(w, b, BotBtn::Special);
    }
    delay(w, id, "@1849", 4.0);
}

/// GetAllActorsOfClass(cls) and K2_DestroyActor on each (the world's projectiles of the class or a child).
/// Unverified: the order is taken as the world's projectile list's.
/// Returns whether it found one.
fn destroy_all_of(w: &mut World, cls: Name) -> bool {
    let mut any = false;
    let ids: Vec<_> = w.actors.projectiles.iter().map(|(id, _)| id).collect();
    for pid in ids {
        let Some(p) = w.actors.projectiles.get(pid) else { continue };
        let (dead, pcls) = (p.dead, p.cls);
        let own = ActorRef::Fighter(lookup_owner(w, ActorRef::Projectile(pid)));
        if !dead && (pcls == cls || actor_is_a(w, own, pcls, cls) == Some(true)) {
            any = true;
            crate::world::spawning::destroy_projectile(w, pid, false);
        }
    }
    any
}

/// TutorialController_Elements_Ignite.
/// ReceivePossess: Delay 0.1 (@347 -> @30): DemoFighter = the possessed pawn (a BaseFighter); a looping 9.0 s timer
/// (IgniteDemoTimer) to IgniteDemo, and one call now.
/// IgniteDemo (@459): PunchingBag = SpawnPunchingBag(PunchingBagFar, 1, Pawn_Superman): a new Superman
/// each loop, the one before kept (only the variable is overwritten); DemoFighter's BPI_SpecialAttackPressed(1 Down)
/// and BPI_SpecialAttackReleased (a fire wall); Delay 0.35 (@682 -> @232): BPI_NormalAttackPressed(4 Neutral).
/// ReceiveUnPossess (@402): K2_DestroyActor on PunchingBag (the latest Superman), the timer cleared.
fn ignite_demo(w: &mut World, id: u32, c: Call<'_>) {
    match c {
        Call::Possessed(f) => {
            let cur = dst!(w, id, Ignite).possessed;
            let p = first_possess(w, id, "@347", f, cur);
            dst!(w, id, Ignite).possessed = p;
            delay(w, id, "@347", 0.1);
        }
        Call::Fired(Fired::Delay("@347")) => {
            let f = dst!(w, id, Ignite).possessed;
            dst!(w, id, Ignite).fighter = f;
            set_timer(w, id, "IgniteDemoTimer", 9.0);
            ignite_demo_loop(w, id);
        }
        Call::Fired(Fired::Timer("IgniteDemoTimer")) => ignite_demo_loop(w, id),
        Call::Fired(Fired::Delay("@682")) => {
            let f = dst!(w, id, Ignite).fighter;
            if callable(w, f) {
                pad(w, id).press(Btn::Attack, Some(Dir::Neutral));
            }
        }
        Call::Unpossessed { .. } => {
            let bag = dst!(w, id, Ignite).bag;
            if let (true, Some(b)) = (callable(w, bag), bag) {
                run::despawn(w, b);
            }
            clear_timer(w, id, "IgniteDemoTimer");
        }
        _ => unhandled(w, id, c),
    }
}

fn ignite_demo_loop(w: &mut World, id: u32) {
    let bag = elements_bag(w, "PunchingBagFar", name!("superman"));
    dst!(w, id, Ignite).bag = bag;
    let f = dst!(w, id, Ignite).fighter;
    if callable(w, f) {
        let pd = pad(w, id);
        pd.press(Btn::Special, Some(Dir::Down));
        pd.release(Btn::Special);
    }
    delay(w, id, "@682", 0.35);
}
