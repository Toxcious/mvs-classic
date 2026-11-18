//! Cromulons' speakers: M009_DestructiblePlatformBP_C (Schwifty_DestructiblePlatformBP,
//! the left one) and its child M009_DestructiblePlatformBP_Child_C (Schwifty_DestructiblePlatformBP_Child_2, the right
//! one: BeginPlay forwarding only, M009_DestructiblePlatformBP_Child), on WallBP_C (`world::wall_bp`). Offsets are the
//! speaker's ubergraph's.
//! - BeginPlay @2819: WallBP's (InitStage(0): HP 1), then three dynamic materials (the drawing's).
//! - ReceiveAnyDamage @4170: WallBP's first with every argument (HP, BPI_DamageVisuals: the montage stored BEFORE this
//!   update, InitStage), then DamageCauser = the event's @4216, then on CurrentStage (@4235):
//!   1: a DoOnce @2638 -> @2511 UpdateMesh Multicast(SpeakerSkelMesh_DamageState1, speaker_platform_LightDamage_Mat,
//!   false), PS_SchwiftyDamageSparks shown @2544, DamageMontage = Speaker_Hit_Anim_DamageState1_Montage @2578;
//!   2: a DoOnce @2781 -> @2674 UpdateMesh Multicast(SpeakerSkelMesh_DamageState3, speaker_platform_HeavyDamage_Mat,
//!   true), DamageMontage = Speaker_Hit_Anim_DamageState2_Montage @2707, SpeakerDamageState2Multicast @2726
//!   (PS_SpeakerDamage2 attached at Root @4932: the drawing's).
//!   UpdateMesh Multicast @4332: SetSkeletalMesh, the materials, then @1063 the sparks shown and PS_SpeakerDamage1 at
//!   the mesh's root socket (the drawing's), and a DoOnce of its own: Stage2 false sfx_map_cromulon_speaker_destroy_a
//!   @1466, true sfx_map_cromulon_speaker_destroy_b @1756, at GetSocketLocation(root) with the DamageCauser cast to
//!   Pawn as Instigator (the current hit's: it was stored at @4216).
//! - DestroyDestructible @3253 (BPI_Destroy's): WallBP's first (Destroyed_0, the 0.1 s Delay), then both thrusters and
//!   the sparks hidden @3263-3331, sfx_map_cromulon_speaker_destroy_c @3480 at the sparks component's place with the
//!   DamageCauser cast to Pawn (the PREVIOUS hit's: this runs inside WallBP's damage, ahead of @4216), then
//!   Comp_EdgeTrace.On Destroyed(null) @15. A repeated call (a hit with HP <= 0 past the last stage) skips WallBP's
//!   part and repeats the rest.
//! - ReceiveTick @3571: the music analyser RTPCs into the materials' Scroll: the drawing's. The speakers tick
//!   (PrimaryActorTick bCanEverTick): their latent pass follows their tick (`super::tickers`).
//! - SFX @4467 (the sparks' particle events): the drawing's particle events, not the sim's.
//! - AllowHazards: read by neither class, WallBP, DestructibleObjectBP nor the level script (Map_M009): the speakers
//!   stand whatever the match config says.
//! - Comp_Pawn_DamageManager OnHitSoundOverride: read by no graph (xref reads 0) and its name is no string of the exe:
//!   it does nothing.
//!
//! The fields the drawing reads: hp, stage, destroyed, mesh, hit_at, hit_montage, broken_at, off_at, thrusters,
//! sparks, is_child; also mesh_at (the tick of the last UpdateMesh: PS_SpeakerDamage1 at the root socket, and with mesh
//! 2 PS_SpeakerDamage2 attached at Root) and destroy_montage.

use crate::Real;
use crate::State;
use crate::actors::{ActorRef, StageActorClass};
use crate::combat::boxes::CH_DAMAGE;
use crate::events::Event;
use crate::fighter::traces::edges::edges_removed;
use crate::name;
use crate::names::Name;
use crate::registry::entries::StageActorEntry;
use crate::world::World;
use crate::world::stage_actors::StageActorOwn;
use crate::world::wall_bp::{
    WallActor, WallPart, wall, wall_any_damage, wall_create, wall_destroy_destructible, wall_hurt, wall_mut,
};

/// `Speaker`: its own state, its `StageActorOwn` variant (a WallBP child).
#[derive(State, Debug, Default)]
pub struct Speaker {
    /// Its `WallActor` part (`world::wall_bp`).
    pub wall: WallPart,
    pub is_child: bool,
    /// 0 SpeakerSkelMesh, 1 SpeakerSkelMesh_DamageState1, 2 SpeakerSkelMesh_DamageState3.
    pub mesh: i32,
    /// The world tick of the last UpdateMesh Multicast, -1 none.
    pub mesh_at: i32,
    /// PS_SchwiftyThruster / PS_SchwiftyThruster1 shown.
    pub thrusters: bool,
    /// PS_SchwiftyDamageSparks shown (bVisible false in the template).
    pub sparks: bool,
    /// DamageCauser cast to Pawn: the fighter's id, -1 none (a projectile, an item: no Pawn).
    pub causer: i32,
    /// The stage 1 DoOnce closed (Temp_bool_IsClosed_Variable_2).
    pub once1: bool,
    /// The stage 2 DoOnce closed (Temp_bool_IsClosed_Variable_3).
    pub once2: bool,
    /// UpdateMesh's destroy_a DoOnce closed (Temp_bool_IsClosed_Variable).
    pub once_a: bool,
    /// UpdateMesh's destroy_b DoOnce closed (Temp_bool_IsClosed_Variable_4).
    pub once_b: bool,
    /// Comp_EdgeTrace.Enabled: On Destroyed removes the polygon from the EdgeTraceGenerator and sets it false; the
    /// riders bound to it detach (`edges_removed`).
    pub edges: bool,
    /// The places the sounds are posted at, sim (made at create from the data). Not state.
    #[state(skip)]
    pub places: Places,
}

/// The places the sounds are posted at, sim.
#[derive(Clone, Copy, Debug, Default)]
pub struct Places {
    /// GetSocketLocation(root) of the SkeletalMesh.
    root: (Real, Real),
    /// PS_SchwiftyDamageSparks' K2_GetComponentLocation.
    sparks: (Real, Real),
}

// The class's component templates (M009_DestructiblePlatformBP _GEN_VARIABLE; the child overrides none of these), UE,
// actor space before the root's scale: SkeletalMesh RelativeLocation; CollisionBox RelativeLocation and scale,
// PS_SchwiftyDamageSparks (attached to CollisionBox) RelativeLocation; DamageAudio (WallBP: no RelativeLocation, at the
// root Box).
const MESH_REL: [Real; 3] = [-36.3891, -0.000_488_281_25, -179.739_62];
const BOX_REL: [Real; 3] = [0.0, 0.0, 42.0];
const BOX_SCALE: [Real; 3] = [0.71, 0.89, 2.4];
const SPARKS_REL: [Real; 3] = [176.1142, 0.0, -493.720_92];
const DAMAGE_AUDIO_REL: [Real; 3] = [0.0, 0.0, 0.0];

const DS1_MONTAGE: &str = "Speaker_Hit_Anim_DamageState1_Montage";
const DS2_MONTAGE: &str = "Speaker_Hit_Anim_DamageState2_Montage";
const SFX_A: Name = name!("sfx_map_cromulon_speaker_destroy_a");
const SFX_B: Name = name!("sfx_map_cromulon_speaker_destroy_b");
const SFX_C: Name = name!("sfx_map_cromulon_speaker_destroy_c");

fn speaker(w: &World, a: WallActor) -> &Speaker {
    match &w.stage_run.as_ref().expect("a stage run").actors[a.0 as usize].own {
        StageActorOwn::Speaker(s) => s,
        o => panic!("stage actor {} is no Speaker: {o:?}", a.0),
    }
}

fn speaker_mut(w: &mut World, a: WallActor) -> &mut Speaker {
    match &mut w.stage_run.as_mut().expect("a stage run").actors[a.0 as usize].own {
        StageActorOwn::Speaker(s) => s,
        o => panic!("stage actor {} is no Speaker: {o:?}", a.0),
    }
}

/// PF_Post_Event_At_Location(event, place, Instigator): the pooled AkComponent at the place.
// Deviation: the Instigator (the DamageCauser cast to Pawn) is not carried: the port's pooled path sets
// cha_locally_controlled 1 and posts the name as it is (Lib_Audio with an Instigator reads its local control and skin).
fn post(w: &mut World, ev: Name, at: (Real, Real)) {
    w.events.push(Event::Sound { fighter: -1, ev, x: Some(at.0), y: Some(at.1), hit: None, pooled: true, key: None });
}

/// UpdateMesh Multicast(NewMesh, NewMaterial, Stage2) @4332: the mesh, then @1063 the sparks shown and the DoOnce sound.
fn update_mesh(w: &mut World, a: WallActor, mesh: i32, stage2: bool) {
    let tick = w.flow.tick;
    let s = speaker_mut(w, a);
    s.mesh = mesh;
    s.mesh_at = tick;
    s.sparks = true;
    let root = s.places.root;
    if !stage2 && !s.once_a {
        s.once_a = true;
        post(w, SFX_A, root);
    } else if stage2 && !s.once_b {
        s.once_b = true;
        post(w, SFX_B, root);
    }
}

/// DestroyDestructible (@3253 -> @15).
fn speaker_destroy(w: &mut World, a: WallActor) {
    wall_destroy_destructible(w, a);
    let s = speaker_mut(w, a);
    s.thrusters = false;
    s.sparks = false;
    let at = s.places.sparks;
    post(w, SFX_C, at);
    // Comp_EdgeTrace.On Destroyed @15: Remove Traceable Element, On Edges Removed, Enabled false
    speaker_mut(w, a).edges = false;
    let placed = w.stage_run.as_ref().expect("a stage run").actors[a.0 as usize].index;
    edges_removed(w, placed);
}

/// The DamageCauser cast to Pawn: a fighter (melee: the fighter; a projectile or an item: no Pawn).
// Unverified: a World.actors pawn as the causer (a Banana Guard's spear) is taken as no Pawn.
fn pawn_of(w: &World, c: ActorRef) -> i32 {
    match c {
        ActorRef::Fighter(f) => w.fighter(f).id,
        _ => -1,
    }
}

fn create_left(w: &mut World, slot: u32) {
    make(w, slot, false);
}

fn create_child(w: &mut World, slot: u32) {
    make(w, slot, true);
}

fn make(w: &mut World, slot: u32, child: bool) {
    let wall_part = wall_create(w, WallActor(slot), DAMAGE_AUDIO_REL);
    let index = w.stage_run.as_ref().expect("a stage run").actors[slot as usize].index;
    let data = std::sync::Arc::clone(&w.data);
    let d = &data.stage.placed.as_ref().expect("placed actors")[index as usize];
    let (sc, l) = (&d.scale, &d.loc);
    let at = |rel: [Real; 3]| -> (Real, Real) {
        (
            crate::math::narrow(-(l[1] + sc[1] * f64::from(rel[1]))),
            crate::math::narrow(l[2] + sc[2] * f64::from(rel[2])),
        )
    };
    // Unverified: the root bone's place is taken as the component's (SpeakerSkelMesh's reference pose is not read; the
    // AnimBP's audio-driven bones do not move root)
    let sparks_rel = [
        BOX_REL[0] + BOX_SCALE[0] * SPARKS_REL[0],
        BOX_REL[1] + BOX_SCALE[1] * SPARKS_REL[1],
        BOX_REL[2] + BOX_SCALE[2] * SPARKS_REL[2],
    ];
    let s = Speaker {
        wall: wall_part,
        is_child: child,
        mesh: 0,
        mesh_at: -1,
        thrusters: true,
        sparks: false,
        causer: -1,
        once1: false,
        once2: false,
        once_a: false,
        once_b: false,
        edges: true,
        places: Places { root: at(MESH_REL), sparks: at(sparks_rel) },
    };
    w.stage_run.as_mut().expect("a stage run").actors[slot as usize].own = StageActorOwn::Speaker(s);
}

fn any_damage(w: &mut World, slot: u32, dmg: Real, causer: ActorRef) {
    let a = WallActor(slot);
    wall_any_damage(w, a, dmg, speaker_destroy, None);
    let c = pawn_of(w, causer);
    speaker_mut(w, a).causer = c;
    let stage = wall(w, a).stage;
    if stage == 1 && !speaker(w, a).once1 {
        speaker_mut(w, a).once1 = true;
        update_mesh(w, a, 1, false);
        speaker_mut(w, a).sparks = true;
        wall_mut(w, a).damage_montage = DS1_MONTAGE.to_owned();
    } else if stage == 2 && !speaker(w, a).once2 {
        speaker_mut(w, a).once2 = true;
        update_mesh(w, a, 2, true);
        wall_mut(w, a).damage_montage = DS2_MONTAGE.to_owned();
    }
}

/// The kind of the speaker (`child` false: PM_Rock) and its child (PM_Metal_Solid).
pub fn entry(class: StageActorClass, child: bool) -> StageActorEntry {
    StageActorEntry {
        class,
        hurt_type: Some(i32::from(CH_DAMAGE)),
        // Comp_Actor_Destructible FriendlyFire true (M009_DestructiblePlatformBP; the child inherits it)
        friendly_fire: true,
        can_take_damage: true,
        // Comp_Pawn_DamageHandler HitPauseMultiplier 0.5 on both placed speakers of both levels (Map_M009,
        // Map_M009_small instance values); no Comp_Actor_Damage / Comp_HitPause: no victim pause, no tier
        hit_pause_multiplier: Some(0.5),
        // the DamageBox's PhysMaterialOverride: PM_Rock (SurfaceType1) on the speaker, PM_Metal_Solid (SurfaceType4) on
        // the child (M009_DestructiblePlatformBP_Child DamageBox_GEN_VARIABLE)
        surface: Some(if child { 4 } else { 1 }),
        create: Some(if child { create_child } else { create_left }),
        any_damage: Some(any_damage),
        hit_by_enemy: None,
        hit_by_ally: None,
        knocked_back: None,
        hurt: Some(|w, i, out| wall_hurt(w, WallActor(i), out)),
        sounds: &[
            "sfx_map_cromulon_speaker_destroy_a",
            "sfx_map_cromulon_speaker_destroy_b",
            "sfx_map_cromulon_speaker_destroy_c",
        ],
        damage_disabled: None,
        invulnerable: None,
        current_damage: None,
        victim_pause: None,
        mesh: None,
        reset: None,
        solid_hit: None,
    }
}
