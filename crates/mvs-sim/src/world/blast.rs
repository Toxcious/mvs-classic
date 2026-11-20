//! The blast zone: its box and the FreeForAll shrink, the rescue and the ring-outs, a
//! spawned actor's blast-box reads (SpawnedActorParent's BeginPlay loop) and the
//! ring-out effect's asset.

use crate::Real;
use crate::State;
use crate::combat::{buffs, holds, perks};
use crate::consts as C;
use crate::events::Event;
use crate::fighter::{self, FighterId, anim, match_side};
use crate::math;
use crate::registry::signals::{BuffSignal, BuffSignalArg, FighterSignal};
use crate::world::{World, flow, items, pawns, stage};

/// A box's bounds in sim x (`-Y`) and y (`Z`): `blast`, `blastOuter`; an actor's stored BlastBoxBoundsMin / Max
/// (`BlastBounds`: `l`, `r`, `b`, `t`).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct BlastBounds {
    pub left: Real,
    pub right: Real,
    pub bottom: Real,
    pub top: Real,
}

/// `koBox` / `koBox0`: the blast box's CollisionComp bounds (UE centre, Y range, Z range); `half_extent` is the
/// original's alone (`koBox0.halfExtent`).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct KoBox {
    pub center: [Real; 3],
    pub half_extent: [Real; 3],
    pub y: [Real; 2],
    pub z: [Real; 2],
}

/// `shrunk`: BlastBoxPlayerExit's shrunk CollisionComp.
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct Shrunk {
    pub center: [Real; 3],
    pub half_extent: [Real; 3],
    pub jump_out_buffer_top: Real,
}

/// BlastBoxPlayerExit MulticastShrinkTimeline (@7550): the ShrinkTimeline (1.0 long, Alpha linear 0 to 1) played
/// from the start at rate 1 / time; its update (@7751) sets ShrinkPercent, the actor location (VLerp of
/// OriginalPosition and ShrunkPosition), the box extent (VLerp of OriginalBoxExtent and ShrunkBoxExtent) and
/// BlastBoxJumpOutBufferTop (Lerp of 300 and ShrunkJumpOutBuffer), then the corners and the jump-out collision
/// from the new bounds (AdjustJumpOutCollision @4488, the padded box here). It runs on after the match is over.
/// Unverified: the timeline is taken to advance one frame's share per game tick from the frame after the timer,
/// before the fighters move (the actor's tick order against the pawns is not read). ShrunkJumpOutBuffer is in
/// neither the class defaults nor a map's instance: 0 (the stage file's jumpOutBufferTop).
/// The length is BPI_ShrinkBlastBox's time, given by the caller ([`start_blast_shrink`]): FFAGameMode's
/// ShrinkBlastBoxTime 30, Mutator_ShrinkBlastBoxLastPoint's 15. A second call plays the timeline from the start
/// again at its own rate.
pub fn shrink_tick(w: &mut World) {
    let fl = &w.flow;
    if fl.blast_shrink_tick < 0 {
        return;
    }
    let n = math::round(fl.blast_shrink_time.unwrap_or(0.0) * 60.0);
    let k = crate::real(fl.tick - fl.blast_shrink_tick);
    if k > n {
        return;
    }
    set_blast_shrink(w, if n > 0.0 { math::min(k / n, 1.0) } else { 1.0 });
}

/// BPI_ShrinkBlastBox(time) on every blast box and the CameraCenter's AdjustCameraForShrinkingBlastBox(time), by
/// the rule named in `by` (FFAGameMode ShrinkBlastBox, Mutator_ShrinkBlastBoxLastPoint): the view reads
/// `blast_shrink_tick`, `blast_shrink` and `blast_shrink_by`.
pub fn start_blast_shrink(w: &mut World, time: Real, by: crate::names::Name) {
    w.flow.blast_shrink_tick = w.flow.tick;
    w.flow.blast_shrink_time = Some(time);
    w.flow.blast_shrink_by = Some(by);
}

/// The blast box corners `blastCorners` gives: BPI_GetTopLeftCorner / BPI_GetBottomRightCorner as they are now
/// (world Y, Z).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct BlastCorners {
    pub top_left: [Real; 2],
    pub bottom_right: [Real; 2],
}

/// `blastCorners`: AdjustCornerPositions (BlastBoxPlayerExit @8704 -> @3870) rewrites the corners on every update
/// of the shrink from the CollisionComp bounds: the corners keep their buffers (the stage file's corners are the
/// unshrunk bounds less BlastBoxCornerHorizontalBuffer / TopBuffer / BottomBuffer). Derived from the box, so no
/// state of its own.
pub fn blast_corners(w: &World) -> BlastCorners {
    let c0 = &w.data.stage.blast_box_corners;
    let (o, k) = (&w.stage.ko_box0, &w.stage.ko_box);
    BlastCorners {
        top_left: [
            k.y[1] - (o.y[1] - crate::math::narrow(c0.top_left[0])),
            k.z[1] - (o.z[1] - crate::math::narrow(c0.top_left[1])),
        ],
        bottom_right: [
            k.y[0] + crate::math::narrow(c0.bottom_right[0] - f64::from(o.y[0])),
            k.z[0] + crate::math::narrow(c0.bottom_right[1] - f64::from(o.z[0])),
        ],
    }
}

/// `setBlastShrink`: the boxes at shrink `a` (0..1).
pub fn set_blast_shrink(w: &mut World, a: Real) {
    w.flow.blast_shrink = a;
    let st = &mut w.stage;
    let o = st.ko_box0;
    let s = st.shrunk.map_or((o.center, o.half_extent), |s| (s.center, s.half_extent));
    let k = &mut st.ko_box;
    if a == 0.0 {
        k.center = o.center;
        k.y = o.y;
        k.z = o.z;
    } else {
        for i in 0..3 {
            k.center[i] = o.center[i] + a * (s.0[i] - o.center[i]);
        }
        let ey = o.half_extent[1] + a * (s.1[1] - o.half_extent[1]);
        let ez = o.half_extent[2] + a * (s.1[2] - o.half_extent[2]);
        k.y = [k.center[1] - ey, k.center[1] + ey];
        k.z = [k.center[2] - ez, k.center[2] + ez];
    }
    let b = BlastBounds { left: -k.y[1], right: -k.y[0], bottom: k.z[0], top: k.z[1] };
    st.blast = b;
    let jump_top = st.shrunk.map_or(0.0, |s| s.jump_out_buffer_top);
    st.blast_outer = BlastBounds {
        left: b.left - 2.0 * C::BLAST_PAD_SIDE,
        right: b.right + 2.0 * C::BLAST_PAD_SIDE,
        bottom: b.bottom - 2.0 * C::BLAST_PAD_BOTTOM,
        top: b.top + 2.0 * (C::BLAST_PAD_TOP + a * (jump_top - C::BLAST_PAD_TOP)),
    };
}

/// `Math.max(a, 0, b)`.
fn max3(a: Real, b: Real, c: Real) -> Real {
    math::max(math::max(a, b), c)
}

/// Buff_OnExitedArenaTeleportToAlly's sensors: four SphereComponents (radius 32) on the buff actor, which spawns
/// with its owner's transform and is attached to the capsule: at Z -100, Z 150 and local X -100 / 100 (the
/// fighter's facing axis) of the capsule's centre. A sphere's EndOverlap with the BlastBoxPlayerExit actor, when
/// CheckIfComponentOverlaps(CollisionComponent, sphere) (the sphere's location inside the box's bounds) fails and
/// MoveToAllyAvailable, runs StartMoveToAlly once. Then the move of Buff_StunnedMoveToPositionThroughGeo.
/// Unverified: the spheres are taken to overlap the blast box (collision profiles not read) and only its
/// CollisionComponent's end counts (an end with a smaller box of the actor leaves the centre inside); taken after
/// the frame's movement, ahead of the ring-out test; the timeline's update likewise.
pub fn rescue_tick(w: &mut World, f: FighterId) {
    if w.fighter(f).life.dead != 0.0 {
        return;
    }
    holds::pull_tick(w, f);
    let by = fighter::rescue_by(w, f);
    if by < 0 || w.fighter(f).moves.pull.is_some() {
        return;
    }
    let Some(o) = w.by_id(by) else { return };
    let b = w.stage.blast;
    let (x, y) = (w.fighter(f).body.x, fighter::actor_y(w, f));
    let out = |px: Real, py: Real| {
        math::ue::size2d([max3(b.left - px, 0.0, px - b.right), max3(b.bottom - py, 0.0, py - b.top)]) > 32.0
    };
    if out(x, y - 100.0) || out(x, y + 150.0) || out(x - 100.0, y) || out(x + 100.0, y) {
        let (ox, oy) = (w.fighter(o).body.x, fighter::actor_y(w, o));
        holds::rescue_start(w, f, ox, oy);
    }
}

/// The capsule against a box: its segment and radius in the play plane.
fn capsule_over(b: &BlastBounds, x: Real, y: Real, r: Real, s: Real) -> bool {
    math::ue::size2d([max3(b.left - x, 0.0, x - b.right), max3(b.bottom - (y + s), 0.0, y - s - b.top)]) <= r
}

/// The fighter's capsule against BlastBoxPlayerExit's two boxes: bit 1 CollisionComponent (the blast box), bit 2
/// BlastBoxJumpOutArea (the box padded by 2 x side 100, 2 x top 300, 2 x bottom 20: ubergraph 4488-5183). Both are
/// of profile Trigger and overlap the Pawn channel; HandleBlastBoxExit takes the pawn's root capsule alone
/// (BPI_GetRootCapsule == OtherComp, 326-410). No overlap with the collision off (the recall's capsule, the rescue
/// pull's SetActorEnableCollision(false)). The capsule is its segment and radius in the play plane.
/// The two inner boxes are kept the other way round: bit 4 is set while the capsule is clear of
/// CollisionCompWarningBox, bit 8 while it is clear of CollisionCompProjectileStopBox (both overlap the Pawn
/// channel: DetectPawn and Trigger profiles), so a capsule with no collision has both set.
pub fn blast_overlaps(w: &World, f: FighterId) -> i32 {
    if fighter::no_collision(w, f) {
        return 4 | 8;
    }
    let (x, y, r) = (w.fighter(f).body.x, fighter::actor_y(w, f), fighter::cap_radius(w, f));
    let s = math::max(fighter::capsule_half(w, f) - r, 0.0);
    let over = |b: Option<BlastBounds>| b.is_some_and(|b| capsule_over(&b, x, y, r, s));
    i32::from(over(Some(w.stage.blast)))
        | (i32::from(over(Some(w.stage.blast_outer))) << 1)
        | if over(inner_box(w, WARNING)) { 0 } else { 4 }
        | if over(inner_box(w, STOP)) { 0 } else { 8 }
}

const WARNING: &str = "CollisionCompWarningBox";
const STOP: &str = "CollisionCompProjectileStopBox";

/// The warning box and the projectile-stop box as they are now, or `None` where the stage's box actor has no such
/// component. UserConstructionScript @0-610 and the shrink's update @8052-8662 set each to the CollisionComponent's
/// bounds less WarningThreshold (1200) / ProjectileStopThreshold (300) on Y and Z (SetWorldScale3D(1) first, X
/// kept): the threshold is the stage file's saved extents' difference from the CollisionComp's, and the box
/// follows the CollisionComp through the shrink.
/// Unverified: an extent the shrink takes below the threshold is clamped at 0 (UE's handling of a negative box
/// extent not read).
fn inner_box(w: &World, name: &str) -> Option<BlastBounds> {
    let st = w.data.stage.blast_boxes.get(name)?;
    let (o, k) = (&w.stage.ko_box0, &w.stage.ko_box);
    let ty = o.half_extent[1] - crate::math::narrow(st.half_extent[1]);
    let tz = o.half_extent[2] - crate::math::narrow(st.half_extent[2]);
    let cy = (k.y[0] + k.y[1]) / 2.0;
    let cz = (k.z[0] + k.z[1]) / 2.0;
    let ey = math::max((k.y[1] - k.y[0]) / 2.0 - ty, 0.0);
    let ez = math::max((k.z[1] - k.z[0]) / 2.0 - tz, 0.0);
    Some(BlastBounds { left: -(cy + ey), right: -(cy - ey), bottom: cz - ez, top: cz + ez })
}

/// The loser's death sounds (BPI_DeathMulticast @705 -> SFX_Death: stop WindRushID, VO_Death at the fighter's
/// location; mix_stop_all_character_sfx @952; InterruptVO(10) @1170), as the ring-out effects and a minion's
/// ring-out post them.
fn death_sounds(w: &mut World, f: FighterId) {
    match_side::stop_wind_for_death(w, f);
    let fr = w.fighter(f);
    let (id, x, y) = (fr.id, fr.body.x, fr.body.y);
    let vo = w.char_data(f).def.sfx.vo_death.as_deref().map(|s| w.lit(s));
    if let Some(ev) = vo {
        w.events.push(Event::Sound { fighter: id, ev, x: Some(x), y: Some(y), hit: None, pooled: false, key: None });
    }
    let ev = crate::name!("mix_stop_all_character_sfx");
    w.events.push(Event::Sound { fighter: id, ev, x: None, y: None, hit: None, pooled: false, key: None });
    match_side::interrupt_vo(w, f, C::VO_INTERRUPT_FADE);
}

/// BlastBoxDeathFXMulticast -> HandleBlastBoxDeathFX (PandaGameMode RoundEnd @24325, TutorialGameMode RoundEnd
/// @3662): Actor_BlastBoxExit and the RingOutVfxAnchor (@1774-2491), the blast box sound, then the death sounds.
/// The data (@1475): the attacker's RingOutVFX when PlayerDataManager.GetByPlayerIndex(AttackerIndex) succeeds
/// (PandaGameMode RoundEnd's Get Recent Attacker Info: MostRecentAttacker's player index, else -1, which never
/// succeeds), else GetRingOutVfxDataToPlay(loser): the loser's own. Each player's is its MatchPlayerData RingOutVFX
/// (ROV_Default unless set). VFX TESTING is false on the CDO.
/// The RingOutVfxAnchor (@2356-2491) is registered on the camera until its timer (Init_RingOutAnchor, the entry's
/// CameraHoldTime), which goes ahead of the timers due in its frame (removeCameraFocus). Init_RingOutAnchor @29
/// compares MapsToIgnore with GetCurrentLevelName(true), the persistent world's map: the Lab's is
/// TrainingContainer, which streams the stage in (MapData LabLevelsToLoad, OnMapLoadRequested @3435), so no stage
/// is ignored there; a match (`setup.rules`) opens MapData.Level itself as the persistent level.
pub fn blast_box_death_fx(w: &mut World, f: FighterId) {
    let att = w.fighter(f).attackers.most_recent_attacker;
    // (a minion attacker has no PlayerData: GetByPlayerIndex fails and the loser's own plays)
    let by = flow::player_attacker(w, att).unwrap_or(f);
    let slot = pawns::slot_of(w, by);
    let ring_out = usize::try_from(slot).ok().and_then(|i| w.setup.players.get(i)).and_then(|p| p.ring_out.as_deref());
    let asset = ring_out_asset_name(w, ring_out);
    let level = if w.setup.rules.is_some() { w.data.stage.name.as_str() } else { "TrainingContainer" };
    let hold = anchor_seconds(w, asset, level);
    let asset = if asset == DEFAULT_RING_OUT { crate::name!("ROV_Default") } else { w.lit(asset) };
    let (p, tl, tr, bl, center) = ko_fx_inputs(w, f);
    let id = w.fighter(f).id;
    w.events.push(Event::KoVfx { fighter: id, asset, hold, p, tl, tr, bl, center });
    w.flow.step_ring_outs += 1;
    let at = w.flow.tick + js_ticks(hold);
    w.flow.timers.insert(0, flow::Timer { at, cmd: flow::TimerCmd::RingOutAnchorEnd, key: None });
    death_sounds(w, f);
}

/// `Math.round(seconds * 60)` as a tick count.
pub(crate) fn js_ticks(seconds: Real) -> i32 {
    math::to_int32(math::round(seconds * 60.0))
}

/// Ring-out (BlastBoxPlayerExit). It comes from the end of an overlap, not from where the fighter is:
/// - CollisionComponent EndOverlap (ubergraph 2478): HandleBlastBoxExit(CheckHitStun true,
///   HandleNonOverlappingActors false). Left alone while the pawn's Comp_Actor_Destructible
///   GetCanSurviveBlastboxJumpOutArea (HandleBlastBoxExit 14-186; a counter only Buff_StunMoveToPosition and
///   Buff_StunnedMoveToPositionThroughGeo raise: the rescue pull here). Then CheckIfPositionOverlaps(the box, the
///   actor) (424: the actor's location within the box's bounds, edges included): inside, nothing; outside,
///   HandlePlayerKnockout when Comp_Pawn_HitstunHandler.CheckHitstunActive (675-847: hitstun or buff stun
///   sources), else nothing: the fighter is then in the band between the boxes and a hit there knocks nobody out.
/// - BlastBoxJumpOutArea EndOverlap (7484 -> 1195): HandleBlastBoxExit(false, true). The location inside the area's
///   bounds (the capsule's collision was switched off): AddUnique to NonOverlappingActors (479-640); outside:
///   HandlePlayerKnockout (875), hitstun or not.
/// - CollisionComponent BeginOverlap (7489): the actor leaves NonOverlappingActors.
/// - ReceiveTick (3607 -> 2197, body 1443-2418), per NonOverlappingActors entry: its location within the
///   CollisionComponent's bounds, nothing; else, unless GetCanSurviveBlastboxJumpOutArea (the entry stays),
///   HandlePlayerKnockout and the entry is removed (2336-2418 -> 2008). So a pull that ends outside the blast box
///   (collision back on at Buff_StunnedMoveToPositionThroughGeo 1123, the counter down at 1341, no begin overlap
///   with the CollisionComponent) is a ring-out at the box's next tick, and a recall's entry goes when the path
///   leaves the box (HandlePlayerKnockout stopped by CantDieList).
///
/// HandlePlayerKnockout (34-479): nothing for an actor on CantDieList (`cant_die`), being destroyed, or BPI_IsDead.
/// The boxes move in the FFA shrink (K2_SetActorLocation and SetBoxExtent(extent, true) on the CollisionComponent
/// 7833 / 7928, the jump-out area's SetBoxExtent(extent, false) and K2_SetWorldLocation 4860 / 5183): a box that
/// leaves a capsule ends the overlap as the capsule leaving the box does (stock UpdateOverlaps of the moved
/// component fires the end on both components), so the same handlers run.
// Deviation: the overlaps are compared once a frame, after every actor's movement, and the box's tick is taken after
// them in the same frame: the hitstun read is the end of the frame's (a hit later in the frame than the capsule's
// move counts), an exit and return within one frame is not seen, and the jump-out area's overlaps are not held back
// to its next move (its extent is set without an overlap update); stock 4.26 overlap rules, not read in the exe.
pub fn check_blast(w: &mut World, f: FighterId) {
    // A dead pawn: HandlePlayerKnockout returns at BPI_IsDead and the box's tick drops its entry; the overlaps
    // begin again at the respawn (respawn).
    if w.fighter(f).life.dead != 0.0 {
        return;
    }
    let was = w.fighter(f).life.blast_in;
    let now = blast_overlaps(w, f);
    w.fighter_mut(f).life.blast_in = now;
    let (x, y) = (w.fighter(f).body.x, fighter::actor_y(w, f));
    let within = |b: &BlastBounds| x >= b.left && x <= b.right && y >= b.bottom && y <= b.top;
    let can_survive = w.fighter(f).moves.pull.is_some() || buffs::survives_blast_jump_out(w, f);
    arena_warning(w, f, was, now);
    let (blast, outer) = (w.stage.blast, w.stage.blast_outer);
    let mut out = false;
    if now & 1 & !was != 0 {
        w.fighter_mut(f).life.blast_listed = false;
    }
    if was & 1 & !now != 0 && !can_survive && !within(&blast) && fighter::in_hitstun(w, f) {
        out = true;
    }
    if was & 2 & !now != 0 {
        if within(&outer) {
            w.fighter_mut(f).life.blast_listed = true;
        } else {
            out = true;
        }
    }
    if w.fighter(f).life.blast_listed && !within(&blast) && !can_survive {
        w.fighter_mut(f).life.blast_listed = false;
        out = true;
    }
    if out && !w.fighter(f).life.cant_die {
        ring_out(w, f);
    }
}

/// HandlePlayerKnockout past its gates (`checkBlast`'s `out`).
fn ring_out(w: &mut World, f: FighterId) {
    w.fighter_mut(f).life.blast_listed = false;
    // HandlePlayerKnockout @493: the fighter's OnExitedArena is broadcast first, its bound handlers in bind order
    // over a copy (ProcessMulticastDelegate 14079aa20), with the loser alive and its buffs on: the team's
    // Comp_C017_Hover_Rage OnAllyDeathRage (@1049, bound at its BeginPlay + 2 s), Buff_OnExitedArenaTeleportToAlly's
    // InstigatorDeath on the others, the fighter's own buffs that bound it (Buff_DelayKnockUp @1237-1260 -> @5295;
    // BuffSignal ExitedArena). Two more bound handlers are run elsewhere: Buff_HeldByEnemy's "Ringed Out" @7586 on
    // the holds the fighter instigated (at the respawn) and Arya's knife @13155 on its target (it reads the target's
    // `dead` in its next step).
    // Unverified: the handlers here are taken in that order (the rage binds ahead of any buff spawned after the
    // first 2 s of the Giant's life; the buffs among themselves by spawn order is not kept): they touch no common
    // state, only the order of their sound events in the frame would differ.
    // (each loop reads the list again, as `w.all` is: a handler may change it)
    let all = pawns::all_list(w);
    for &o in &all {
        if o != f
            && w.fighter(o).team.team == w.fighter(f).team.team
            && let Some(h) = w.fighter(o).hooks.char.ally_exited_arena
        {
            h(w, o, f);
        }
    }
    pawns::put_fighters(w, all);
    // The handlers a pawn's component bound on the fighter at run time (Comp_C023B_TargetTracker's
    // OnTargetExitArena @557), after the ones bound at BeginPlay.
    let all = pawns::all_list(w);
    for &o in &all {
        if o != f
            && let Some(h) = w.fighter(o).hooks.char.fighter_exited_arena
        {
            h(w, o, f);
        }
    }
    pawns::put_fighters(w, all);
    let id = w.fighter(f).id;
    let all = pawns::all_list(w);
    for &o in &all {
        if o != f {
            holds::end_rescue_by(w, o, id);
        }
    }
    pawns::put_fighters(w, all);
    holds::hold_exited_arena(w, f); // the holds whose child bound it in its BeginPlay (Buff_JakEat's JakeDeath)
    buffs::buff_signal(w, f, BuffSignal::ExitedArena, BuffSignalArg::default());
    fighter::signal_bound(w, f, FighterSignal::ExitedArena, false, 0.0, crate::names::Name::NONE, None);
    flow::broadcast_pawn(w, f, flow::PawnDelegate::ExitedArena); // a run-time binder's (a tutorial minigame's)
    // A mode World (`setup.mode`): its game mode's RoundEnd (TutorialGameMode RoundEnd @3502 overrides
    // PandaGameMode's for every loser) in place of the score and defeat path, then @704 as below.
    if let Some(mode) = w.mode {
        // (the port's own Waves mode runs a minion's ring-out as a match does; its mode hook takes the fighters')
        if w.waves.is_some() && w.fighter(f).life.is_minion {
            minion_ring_out(w, f);
            return;
        }
        (mode.round_end)(w, f);
        perks::check_for_blast_box_bounce(w, f);
        return;
    }
    if w.fighter(f).life.is_minion {
        minion_ring_out(w, f);
        return;
    }
    // @634 BPI_RoundEnd -> PandaGameMode RoundEnd @24209 (the Lab's TrainGameMode_C is its child and overrides none
    // of RoundEnd, BPI_OnPlayerRingout, OnPlayerRingoutImpl): BlastBoxDeathFXMulticast @24325.
    blast_box_death_fx(w, f);
    // PandaGameMode BPI_OnPlayerRingout @24379 -> OnPlayerRingoutImpl broadcasts PlayerKnockedOut (@248), to the
    // pawns that bound it (Pawn_Arya @289) and the FreeForAll Buff_FFABonusObjectiveTrackers: ahead of its
    // AlreadyRungOut test @341, the scores @513-537 and KillPlayerFighter @561, so with the loser still alive and
    // its buffs on. The Lab runs the same.
    // The buffs that bound it (BuffSignal PlayerKnockedOut: Buff_RefreshingKnockOut, Buff_InMemoriam) follow each
    // pawn's: a pawn binds at its BeginPlay, its perk buffs at its possession (ahead of Comp_Actor_Destructible's
    // Delay(0) bind, which the port has not).
    // Deviation: a respawned fighter's perk buffs bind again at its reset, behind every pawn's and every first-life
    // buff's; they are taken here in fighter order all the same (no binder reads what another one writes).
    player_knocked_out(w, f);
    if w.stage_run.is_some() {
        stage::stage_ring_out(w, f); // StageHooks.ringOut
    }
    let rules = w.setup.rules;
    if rules.is_some() && w.setup.team_style == crate::setup::TeamStyle::FreeForAll && w.flow.stocks.is_none() {
        flow::bonus_knocked_out(w, f);
    }
    if rules.is_some() {
        flow::score_ring_out(w, f);
    }
    // KillPlayerFighter @70: KillBaseFighter @159028 -> the defeat tail @18358.
    buffs::defeat_buffs(w, f); // the defeat tail's first step (BaseFighter 18380-19022)
    // @19155 StopAnimMontage(null): the current active montage alone, a grouped instance or a plain (dash, wall
    // jump) one included.
    anim::stop_anim_montage(w, f);
    match_side::pawn_death(w, f);
    // KillBaseFighter broadcasts OnBFDefeated -> Comp_ItemManager.OnOwnerDefeated: a held item is dropped where it
    // is (ThrowItem with a zero direction). Its blast-box timer runs again.
    match_side::throw_item(w, f, 0.0, 0.0, None);
    items::release_items(w, f);
    // ... and to the buffs that bound it in their BeginPlay (BuffSignal BfDefeated), behind the item manager's:
    // the ones the defeat tail destroyed above are gone.
    buffs::buff_signal(w, f, BuffSignal::BfDefeated, BuffSignalArg::default());
    let fr = w.fighter_mut(f);
    fr.life.dead = math::round(C::RESPAWN_DELAY * 60.0);
    if rules.is_none() {
        fr.stats.kos += 1;
    }
    ko_hitstun(fr);
    let id = fr.id;
    w.events.push(Event::Ko { fighter: id });
    flow::remove_camera_focus(w, f);
    // The winning ring-out spawns no RespawnTimerActor: the fighter stays out.
    // After HandleWinLoss a RespawnTimerActor (2.0 s) is still spawned, but BaseFighter BPI_RespawnFighter (@26510)
    // returns unless MatchStarted?, which MatchEndImpl clears 2.0 s after HandleWinLoss: before any such timer
    // ends. The fighter stays out.
    if rules.is_some() && (!flow::kill_player_fighter(w, f) || w.flow.match_over) {
        w.fighter_mut(f).life.dead = Real::INFINITY;
    }
    // KillBaseFighter -> BFCameraShake(CameraShakeMedium).
    let cls = crate::name!("CameraShakeMedium_C");
    w.events.push(Event::CameraShake { cls });
    // BlastBoxPlayerExit HandlePlayerKnockout @704 CheckForBlastBoxBounce(loser): after the whole ring-out, ahead of
    // PlayerKnockoutDispatcher @727.
    perks::check_for_blast_box_bounce(w, f);
}

/// The HitstunTimer at the ring-out: the ticks left, one more for a timer still to start and one for a paused
/// one's unpause (the handler ticks on: `hitstunHandlerTick`); then the hit state cleared.
fn ko_hitstun(fr: &mut crate::Fighter) {
    let h = &mut fr.hit;
    fr.life.ko_hitstun = if h.hitstun > 0.0 {
        h.hitstun
            + if h.hitstun_paused {
                2.0
            } else if h.hitstun_pending {
                1.0
            } else {
                0.0
            }
    } else {
        0.0
    };
    h.hitpause = 0.0;
    h.hitstun = 0.0;
    h.hitstun_paused = false;
    h.hitstun_pending = false;
    fr.body.vx = 0.0;
    fr.body.vy = 0.0;
}

/// The two inner boxes' end overlaps with a BP_FighterCharacter (`checkBlast`, ahead of the ring-out test), each
/// broadcasting its OnExitArenaWarning (`holdArenaWarning`):
/// - CollisionCompWarningBox EndOverlap (ubergraph 2585-3050): with the component's GetGenerateOverlapEvents, the
///   actor not being destroyed, GetActorEnableCollision, and CheckIfPositionOverlaps(the box, the actor) false (the
///   actor's location out of the box's bounds, edges counted inside).
/// - CollisionCompProjectileStopBox EndOverlap (3051-3479, authority): no collision or position gate; a
///   SpawnedActorParent gets ExitedArenaWarning instead (the actors' side is not modelled here).
///
/// Unverified: the root capsule alone is taken to end the overlaps (a pawn's other Pawn-channel components, the
/// mesh's bodies among them, not read), at the frame-end compare as the other boxes are, the warning box's first.
// Deviation: an end that comes from the actor's collision being switched off (SetActorEnableCollision(false) ends
// every overlap inside the call) is not broadcast for the projectile-stop box: the port sees the disable at the
// frame's end, after the handlers its caller binds behind it (Buff_EatenByC015 @421 before @1393), which the build's
// broadcast inside the disable does not reach.
fn arena_warning(w: &mut World, f: FighterId, was: i32, now: i32) {
    if now & 4 & !was != 0
        && let Some(b) = inner_box(w, WARNING)
    {
        let (x, y) = (w.fighter(f).body.x, fighter::actor_y(w, f));
        if !(x >= b.left && x <= b.right && y >= b.bottom && y <= b.top) {
            holds::hold_arena_warning(w, f, false);
        }
    }
    if now & 8 & !was != 0 && !fighter::actor_collision_off(w, f) {
        holds::hold_arena_warning(w, f, false);
    }
}

/// A minion's ring-out (`checkBlast`, behind its OnExitedArena). RoundEnd -> HandleBlastBoxDeathFX with
/// NotPlayerCharacter (@199) skips the RingOutVFX branch (@1232): no Actor_BlastBoxExit and no RingOutVfxAnchor on
/// the camera, and posts the default blast box sound (`Event::BlastBoxSound`: sfx_blastbox_default_ally / _enemy by
/// colour index, the view's pick) at the loser's place; then BPI_DeathMulticast, the character SFX mixer's stop and
/// InterruptVO as a player's.
/// OnPlayerRingoutImpl broadcasts PlayerKnockedOut(-1) (@197-248: no PlayerData for it), which every receiver of the
/// build drops on that index (the FFA trackers @4637, Pawn_Arya @9164, Buff_InMemoriam, Buff_RefreshingKnockOut,
/// Comp_Actor_Destructible @1276): none is called, and the stage's ringOut hook neither. Then IsMinion @443 ->
/// DestroyMinion @479 -> KillBaseFighter @65: the defeat tail and OnBFDefeated, no score, no KillPlayerFighter, no
/// respawn. The defeat tail with NotPlayerCharacter (BaseFighter @19063) skips ShowOrHideUI and still sets the
/// RemoveCameraFocus timer. The actor is not destroyed here: the creator's handler does it (CharHooks
/// minionDefeated -> `destroy_minion`).
/// OnPlayerRingoutImpl's PlayerKnockedOut broadcast (@248): each pawn's bound handler, then the buffs that bound it,
/// in fighter order, with `f` the loser (`ring_out`; the port's own Waves mode's ring-out, `modes::waves`).
pub fn player_knocked_out(w: &mut World, f: FighterId) {
    let all = pawns::all_list(w);
    for &o in &all {
        if let Some(h) = w.fighter(o).hooks.char.player_knocked_out {
            h(w, o, f);
        }
        buffs::buff_signal(
            w,
            o,
            BuffSignal::PlayerKnockedOut,
            BuffSignalArg { victim: Some(f), ..BuffSignalArg::default() },
        );
    }
    pawns::put_fighters(w, all);
}

fn minion_ring_out(w: &mut World, f: FighterId) {
    let fr = w.fighter(f);
    let (fighter, p) = (fr.id, [-fr.body.x, fr.body.y]);
    w.events.push(Event::BlastBoxSound { fighter, p });
    death_sounds(w, f);
    buffs::defeat_buffs(w, f);
    anim::stop_anim_montage(w, f);
    match_side::pawn_death(w, f);
    // OnBFDefeated: Comp_ItemManager.OnOwnerDefeated (bound at the spawn), the creator's handler (bound in the
    // set-up), then the buffs that bound it in their BeginPlay (all behind the set-up).
    match_side::throw_item(w, f, 0.0, 0.0, None);
    items::release_items(w, f);
    if let Some(by) = w.fighter(f).life.minion_creator
        && let Some(h) = w.fighter(by).hooks.char.minion_defeated
    {
        h(w, by, f);
    }
    buffs::buff_signal(w, f, BuffSignal::BfDefeated, BuffSignalArg::default());
    let fr = w.fighter_mut(f);
    fr.life.dead = Real::INFINITY;
    ko_hitstun(fr);
    flow::remove_camera_focus(w, f);
    let cls = crate::name!("CameraShakeMedium_C"); // KillBaseFighter -> BFCameraShake(CameraShakeMedium)
    w.events.push(Event::CameraShake { cls });
    perks::check_for_blast_box_bounce(w, f); // HandlePlayerKnockout @704, behind RoundEnd
}

/// BlastBoxDeathFX GetLocationRotationScale's inputs, in UE coordinates with the actor at (0, -x, y): the actor
/// location and the BPI_GetTopLeft / TopRight / BottomLeft corners (the CollisionComp bounds inset by Horizontal 30
/// / Top 250 / Bottom 0) and the actor's own location (the box centre). The transform itself is the view's.
#[allow(clippy::type_complexity, reason = "the event's five points")]
fn ko_fx_inputs(w: &World, f: FighterId) -> ([Real; 2], [Real; 2], [Real; 2], [Real; 2], [Real; 2]) {
    let bb = &w.stage.ko_box;
    let ([min_y, max_y], [min_z, max_z]) = (bb.y, bb.z);
    let fr = w.fighter(f);
    (
        [-fr.body.x, fr.body.y],
        [max_y - 30.0, max_z - 250.0],
        [min_y + 30.0, max_z - 250.0],
        [max_y - 30.0, min_z],
        [bb.center[1], bb.center[2]],
    )
}

// ---- the ring-out effects' data as the match reads it

/// HandleBlastBoxDeathFX's fallbacks: GetRingOutVfxDataToPlay @301 and GetEmitterDataToPlay @0.
pub const DEFAULT_RING_OUT: &str = "ROV_Default";

/// `ringOutAssetName`: a MatchPlayer.ringOut as the match takes it: an asset of the table, else ROV_Default.
/// MatchPlayerData.Initialize writes RingOutVFX only when the argument IsValid (@221-264), so a player whose name
/// resolves to no asset keeps the CDO's ROV_Default.
pub fn ring_out_asset_name<'a>(w: &'a World, name: Option<&'a str>) -> &'a str {
    match name {
        Some(n) if !n.is_empty() && w.data.ring_outs.assets.contains_key(n) => n,
        Some(n) if !n.is_empty() => {
            crate::fighter::common::warn_text(w, &format!("ring-out: no RingOutVfxData {n}: ROV_Default plays"));
            DEFAULT_RING_OUT
        }
        _ => DEFAULT_RING_OUT,
    }
}

/// `anchorSeconds`: RingOutVfxAnchor Init_RingOutAnchor (@29-226): K2_SetTimerDelegate(DestroyActorEvent) of
/// 0.01 s when GetCurrentLevelName(true) is in MapsToIgnore (an FString array: Array_Contains compares FStrings,
/// case-insensitive), else FMax(CameraHoldTime, 0.01). `level`: the persistent level's name.
pub fn anchor_seconds(w: &World, asset: &str, level: &str) -> Real {
    let t = &w.data.ring_outs;
    if t.maps.iter().any(|m| m.eq_ignore_ascii_case(level)) {
        return 0.01;
    }
    math::max(t.assets.get(asset).map_or(0.0, |a| crate::math::narrow(a.hold)), 0.01)
}

// ---- SpawnedActorParent's blast-box reads (its BeginPlay loop @9247-15091)
//
// Only SpawnedActorParent sets `Use Jump Buffer For Blast Box Checks` (true) and no class of the data sets it false,
// so for every spawned actor BPI_GetBlastBoxBounds and BPI_CheckIfPositionOverlaps (BlastBoxPlayerExit @0-239 /
// @0-101) read the jump-out area, `blast_outer`, never the collision box (`blast`). No class overrides
// CheckForBlastBox or CheckBlastBoxExit (SpawnedActorParent defines both, nothing else does).

/// `SpawnBox`: the blast box as one run of the loop reads it, at that moment (`set_blast_shrink` moves it): the
/// jump-out area (the overlap test, edges in) and the box the clamp uses, its corner and that corner's mirror (no 0
/// merge).
#[derive(State, Copy, PartialEq, Debug, Default)]
pub struct SpawnBox {
    pub left: Real,
    pub right: Real,
    pub bottom: Real,
    pub top: Real,
    pub hi_x: Real,
    pub lo_y: Real,
}

/// `storedBlastBounds`: the stored BlastBoxBoundsMin / Max with the jump buffer (@9720-10573), in sim axes, as one
/// BeginPlay leaves them on a fresh actor. BPI_GetBlastBoxBounds(true) is the jump-out area's max corner (UE Y max =
/// sim left, Z max = top); the min is that corner mirrored about the blast box actor's location (@10242-10288: 2 x
/// location - bounds), so the bottom is the box's bottom - 600 where the area itself (shifted up by Top - Bottom,
/// BlastBoxPlayerExit @4994-5183) ends 40 below it. Max is floored and min capped at 0 (the CDO's (0, 0, 0) merged
/// in). Unverified: the actor's location is taken as its CollisionComponent's centre (`blast`'s). `hi_x` / `lo_y`
/// are the mirrored corner before the cap.
pub fn stored_blast_bounds(w: &World) -> (BlastBounds, Real, Real) {
    let (o, b) = (&w.stage.blast_outer, &w.stage.blast);
    let cx = (b.left + b.right) / 2.0;
    let cz = (b.bottom + b.top) / 2.0;
    let hi_x = 2.0 * cx - o.left;
    let lo_y = 2.0 * cz - o.top;
    let s = BlastBounds {
        left: math::min(0.0, o.left),
        right: math::max(0.0, hi_x),
        bottom: math::min(0.0, lo_y),
        top: math::max(0.0, o.top),
    };
    (s, hi_x, lo_y)
}

/// `spawnBox`.
pub fn spawn_box(w: &World) -> SpawnBox {
    let o = w.stage.blast_outer;
    let (_, hi_x, lo_y) = stored_blast_bounds(w);
    SpawnBox { left: o.left, right: o.right, bottom: o.bottom, top: o.top, hi_x, lo_y }
}

/// `newBlastBounds`: an actor's own BlastBoxBoundsMin / Max, the class default (0, 0, 0), merged by every run of
/// the BeginPlay loop on it. Part of the actor's state.
pub fn new_blast_bounds() -> BlastBounds {
    BlastBounds::default()
}

/// `storeBlastBounds` (@9720-10573): Max = FMax(bounds, Max), Min = FMin(2 x boxLoc - bounds, Min), per axis.
pub fn store_blast_bounds(sb: &SpawnBox, s: &mut BlastBounds) {
    s.left = math::min(s.left, sb.left);
    s.right = math::max(s.right, sb.hi_x);
    s.bottom = math::min(s.bottom, sb.lo_y);
    s.top = math::max(s.top, sb.top);
}

/// `blastBoxLoop`: the BeginPlay loop (@9247-10808, @14195-15091), which CheckForBlastBox (@24026 = goto 9247) runs
/// again: the bounds stored, then BPI_CheckIfPositionOverlaps (the jump-out area, edges inclusive); outside it,
/// with ForceSpawnPositionInsideBlastBox (`force_inside`), @14347-14934 FClamp per axis between the corner and its
/// mirror and @14989 K2_SetActorLocation (no sweep, teleport), else @15066 with DestroyOnExitedArena?
/// DestroySpawnedActor(true): true returned, the caller destroys it. `destroy_on_exit`: the actor's
/// DestroyOnExitedArena? at that moment. A teleport moves the previous place too (`prev`, where the actor has one).
#[allow(clippy::too_many_arguments, reason = "the actor's fields the loop reads and writes")]
pub fn blast_box_loop(
    sb: &SpawnBox,
    x: &mut Real,
    y: &mut Real,
    prev: Option<(&mut Real, &mut Real)>,
    force_inside: bool,
    bounds: &mut BlastBounds,
    destroy_on_exit: bool,
) -> bool {
    store_blast_bounds(sb, bounds);
    if !force_inside {
        return destroy_on_exit && !inside_area(sb, *x, *y);
    }
    if force_spawn_inside(sb, x, y)
        && let Some((px, py)) = prev
    {
        *px = *x;
        *py = *y;
    }
    false
}

fn inside_area(sb: &SpawnBox, x: Real, y: Real) -> bool {
    x >= sb.left && x <= sb.right && y >= sb.bottom && y <= sb.top
}

/// `forceSpawnInside`: the clamp alone (@14347-14989), for a class whose bounds nothing reads (Morty's ship). True:
/// it moved.
pub fn force_spawn_inside(sb: &SpawnBox, x: &mut Real, y: &mut Real) -> bool {
    if inside_area(sb, *x, *y) {
        return false;
    }
    *x = math::min(math::max(*x, sb.left), sb.hi_x);
    *y = math::min(math::max(*y, sb.lo_y), sb.top);
    true
}

/// `checkBlastBoxExit`: CheckBlastBoxExit @20219-20685 (the 0.1 s looping CheckBlastBoxTimer, set where
/// DestroyOnExitedArena? is true): InRange(location, the actor's stored Min, Max, inclusive) per axis; true:
/// outside, DestroySpawnedActor(true). The timers stay with each class (`blast_timer_exit` for those that take it
/// whole).
pub fn check_blast_box_exit(x: Real, y: Real, s: &BlastBounds) -> bool {
    x < s.left || x > s.right || y < s.bottom || y > s.top
}

/// `blastTimerExit`: CheckBlastBoxTimer for an actor whose DestroyOnExitedArena? is its class's
/// (`destroy_on_exit`): `blast_at` its next expiry in seconds of counted deltas (F(0.1) at the spawn), `frames` the
/// ticks since the spawn's (the timer does not count the spawn frame), (x, y) the actor's location. True: it
/// expired outside the stored bounds, DestroySpawnedActor(true).
pub fn blast_timer_exit(
    destroy_on_exit: bool,
    blast_at: &mut crate::Clock,
    s: &BlastBounds,
    frames: Real,
    x: Real,
    y: Real,
) -> bool {
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the test !(a > b) is kept: NaN reads as not yet")]
    if !destroy_on_exit || !(crate::consts::clock(frames) > *blast_at) {
        return false;
    }
    *blast_at += f64::from(0.1_f32);
    check_blast_box_exit(x, y, s)
}

/// [`blast_timer_exit`] for the World actors whose own code stores the next expiry rounded, `blastAt = F(blastAt
/// + 0.1)` (the Finn perk blast, BMO's and the shockwave's), not the sum of a float32 and a double.
pub fn blast_timer_exit_rounded(
    destroy_on_exit: bool,
    blast_at: &mut Real,
    s: &BlastBounds,
    frames: Real,
    x: Real,
    y: Real,
) -> bool {
    #[allow(clippy::neg_cmp_op_on_partial_ord, reason = "the test !(a > b) is kept: NaN reads as not yet")]
    if !destroy_on_exit || !(crate::consts::clock(frames) > f64::from(*blast_at)) {
        return false;
    }
    *blast_at += 0.1;
    check_blast_box_exit(x, y, s)
}
