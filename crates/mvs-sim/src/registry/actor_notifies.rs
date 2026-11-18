//! The actor branches of the shared anim notify classes: their calls when
//! MeshComp's owner is a spawned actor (Ak_OneShot, Ak_Continuous, Ak_SurfaceEffect, AnimNotify_PlayVFX,
//! AnimNotify_Hitframe on an actor's mesh). [`super::notifies`] enters them as the classes' `actor_*` hooks.

use crate::Real;
use crate::actors::ActorRef;
use crate::actors::actor_montage::{actor_notify_data, mesh_actor};
use crate::combat::HitPacket;
use crate::consts::SURFACE_LINE_LENGTH;
use crate::events::{Event, NotifyRef, SoundKey};
use crate::fighter::common::warn_once;
use crate::fighter::match_side::ground_trace;
use crate::name;
use crate::names::Name;
use crate::registry::props::{arr, flag, name, num, num_in, obj, text};
use crate::world::World;
use mvs_data::fighter::Notify;
use mvs_data::value::{Props, Value};

/// What the notifies read of a mesh actor (`MeshActor`: a projectile or a world actor with a skeletal mesh).
#[derive(Clone, Copy, Debug)]
pub struct MeshActorView {
    pub owner: crate::fighter::FighterId,
    /// The owner's id.
    pub owner_id: i32,
    /// The actor's world count (`a.id`).
    pub seq: i32,
    pub cls: Name,
    pub x: Real,
    pub y: Real,
    /// `a.scale` where the actor has one.
    pub scale: Option<Real>,
    /// The montage its mesh plays (`a.anim.m?.name`).
    pub montage: Option<Name>,
}

/// The actor's root AkComponent as the voices' key (`actorAkKey`: `'ak' + a.id`).
fn ak_key(a: &MeshActorView) -> SoundKey {
    SoundKey { tag: name!("ak"), n: a.seq }
}

fn view(w: &World, a: ActorRef) -> Option<MeshActorView> {
    mesh_actor(w, a)
}

/// Ak_OneShot Received_Notify: with Follow (every actor use of the data) the event on the owner's root's AkComponent
/// (GetAkComponent @329, PF_Post_Event_On_AkComponent @434); without it at MeshComp.GetSocketLocation(Attach Name)
/// @724, the owner cast to Pawn (none for an actor) @829.
// Deviation: posted at the actor's place at the post for the owner's object; the AkComponent's following of the root
// afterwards is not kept, and the socket of a Follow-less use is the actor's place (the sim has no actor pose).
pub fn actor_one_shot(w: &mut World, a: ActorRef, n: NotifyRef) {
    let Some(v) = view(w, a) else { return };
    let data = w.data.clone();
    let p = &actor_notify_data(w, &data, a, n).p;
    post_on_actor(w, &v, p);
}

fn post_on_actor(w: &mut World, v: &MeshActorView, p: &Props) {
    if !flag(p, "AkEvent") {
        return;
    }
    let ev = text(p, "AkEvent").map_or(Name::NONE, |s| name(w, s));
    let key = ak_key(v);
    w.events.push(Event::Sound {
        fighter: v.owner_id,
        ev,
        x: Some(v.x),
        y: Some(v.y),
        hit: None,
        pooled: false,
        key: Some(key),
    });
}

/// Ak_Continuous on an actor's mesh: the owner has no Comp_Actor_Audio_Notify_Container, so Received_NotifyBegin
/// @266 -> @556-702 posts AkEvent on the owner's root's AkComponent with no stop handle, and Received_NotifyEnd @194 ->
/// @517-706 posts AkEvent_End there (the class default mix_stop_all_fade_eighthofsecond). That event's one action is a
/// Stop with no target (scope GlobalGameObject): every voice of the game object it is posted on stops.
// Unverified: the action's scope GlobalGameObject is read as "every sound of the object it is posted on" (Wwise's Stop
// All on a game object); "every sound of every object" was not ruled out in the exe. As a post with no handle it is a
// single shot for the net layer: a rolled-back begin is not undone.
pub fn actor_continuous_begin(w: &mut World, a: ActorRef, n: NotifyRef, _idx: u32) {
    actor_one_shot(w, a, n);
}

pub fn actor_continuous_end(w: &mut World, a: ActorRef, n: NotifyRef, _idx: u32) {
    let Some(v) = view(w, a) else { return };
    let data = w.data.clone();
    let p = &actor_notify_data(w, &data, a, n).p;
    let ev = match p.get("AkEvent_End") {
        None => name!("mix_stop_all_fade_eighthofsecond"),
        Some(Value::Str(s)) if !s.is_empty() && s != "None" => name(w, s),
        Some(_) => return,
    };
    let key = ak_key(&v);
    w.events.push(Event::SoundStop { fighter: v.owner_id, key, fade: None, ev: Some(ev), curve: None, cut: false });
}

/// Ak_SurfaceEffect on an actor's mesh: PF_Surface_Effect_Trace(AkEvent, MeshComp.GetSocketLocation(TraceFrom), the
/// owner cast to Pawn: none, line): the line 200 down, then PF_Surface_Effect: the physmat switch,
/// cha_foley_land_intensity 0 (the cast to BPI_Base_Fighter fails) and the event at the location.
// Deviation: the socket (fml_un_L_foot in every actor use) is taken as the mesh component's origin (the sim keeps no
// actor pose); the switch and the RTPC are set on the owner's object, where the build's spawned AkComponent at the
// location has its own.
pub fn actor_surface(w: &mut World, a: ActorRef, n: NotifyRef) {
    let Some(v) = view(w, a) else { return };
    let data = w.data.clone();
    let p = &actor_notify_data(w, &data, a, n).p;
    if !flag(p, "AkEvent") {
        return;
    }
    let rel_z = crate::actors::actor_montage::mesh_component_of(w, a).and_then(|c| num_in(c, "RelativeLocation", "Z"));
    let oy = v.y + rel_z.unwrap_or(0.0) * v.scale.unwrap_or(1.0);
    let hit = ground_trace(w, v.owner, v.x, oy, false, SURFACE_LINE_LENGTH);
    let fighter = v.owner_id;
    let (group, value) = (name!("physmat"), if hit { name!("flesh") } else { name!("rock") });
    w.events.push(Event::Switch { fighter, group, value });
    let rtpc = name!("cha_foley_land_intensity");
    w.events.push(Event::Rtpc { fighter, name: rtpc, value: 0.0, interp_ms: 0.0 });
    let ev = text(p, "AkEvent").map_or(Name::NONE, |s| name(w, s));
    w.events.push(Event::Sound { fighter, ev, x: Some(v.x), y: Some(oy), hit: None, pooled: false, key: None });
}

/// AnimNotify_PlayVFX SpawnVFX on a non-fighter owner: SpawnEmitterAttached(ParticleTeam0, MeshComp, SocketName, the
/// Location / Rotation / VFX Scale, keep relative) (socket None: SpawnEmitterAtLocation); NotifyEnd with
/// KillOnNotifyEnd deactivates MeshComp's particle children. The drawing is the view's.
pub fn actor_play_vfx_begin(w: &mut World, a: ActorRef, n: NotifyRef, _idx: u32) {
    let Some(v) = view(w, a) else { return };
    w.events.push(Event::ActorVfx { fighter: v.owner_id, actor: v.seq, n, on: true, x: v.x, y: v.y });
}

pub fn actor_play_vfx_end(w: &mut World, a: ActorRef, n: NotifyRef, _idx: u32) {
    let Some(v) = view(w, a) else { return };
    let data = w.data.clone();
    if flag(&actor_notify_data(w, &data, a, n).p, "KillOnNotifyEnd") {
        w.events.push(Event::ActorVfx { fighter: v.owner_id, actor: v.seq, n, on: false, x: v.x, y: v.y });
    }
}

/// The notify's members renamed to the Struct_HitBoxDamage's.
const HITFRAME_RENAME: [(&str, &str); 5] = [
    ("KnockBackDirection", "KnockBackDirectionVector"),
    ("OnHitSound", "OnHitAudio"),
    ("QueueHitPauseUntilPose", "QueueHitPauseUntilPoses"),
    ("Account for Velocity with Vacuum Knockback", "AccountForVelocityWithVacuumKnockback"),
    ("Direct Knockback Toward Socket", "DirectKnockbackTowardsSocket"),
];
/// The notify's own members, not the packet's.
const HITFRAME_OWN: [&str; 5] = ["HitBoxTags", "HitBoxScale", "ElementalBuffs", "OnHitBuffs", "ChargeHitPauseMult"];

/// AnimNotify_Hitframe on an actor's mesh: the cast to BP_FighterCharacter fails (@371-436), so RecentAttackID 0
/// (@1086) and TotalChargedTime 0 (@1109); HitframeHitboxesOn (@913) on the actor's own Comp_Pawn_HitBoxHandler with
/// the Struct_HitBoxDamage it builds (@158-1362): the notify's values under the struct's names, HitPauseTime =
/// ChargeTime (0) x ChargeHitPauseMult + HitPauseTime, AttackTag the notify's display name, AttackID 0,
/// PlayOnHitEffects and ApplyHitPauseToDamageDealer true. AttackAnimation (@614) = Animation, the montage the notify
/// is in.
// Unverified: the notify's values the data leaves at the class defaults are absent from the packet (read with the
// consumer's own defaults, as the fighter's Hitframe is); the display name is taken as the notify object's name.
// Deviation: ElementalBuffs / OnHitBuffs go to HitBoxesOn beside the packet (@1398) and are not passed (every actor
// Hitframe of the data has both lists empty).
pub fn actor_hit_packet(n: &Notify, montage: Option<&str>) -> Props {
    let mut out = Props::new();
    for (k, v) in &n.p {
        if HITFRAME_OWN.contains(&k.as_str()) {
            continue;
        }
        let key = HITFRAME_RENAME.iter().find(|(a, _)| a == k).map_or(k.as_str(), |(_, b)| b);
        out.insert(key.to_owned(), v.clone());
    }
    // + TotalChargedTime 0 x ChargeHitPauseMult
    out.insert("HitPauseTime".into(), Value::Num(f64::from(num(&n.p, "HitPauseTime").unwrap_or(0.0))));
    out.insert("AttackTag".into(), n.inst.clone().map_or(Value::Null, Value::Str));
    out.insert("AttackAnimation".into(), montage.map_or(Value::Null, |m| Value::Str(m.to_owned())));
    out.insert("AttackID".into(), Value::Num(0.0));
    out.insert("PlayOnHitEffects".into(), Value::Bool(true));
    out.insert("ApplyHitPauseToDamageDealer".into(), Value::Bool(true));
    out
}

fn hit_box_tags(w: &World, p: &Props) -> Vec<Name> {
    arr(p, "HitBoxTags").iter().filter_map(Value::as_str).map(|s| name(w, s)).collect()
}

fn mesh_hooks(w: &World, cls: Name) -> Option<crate::registry::entries::MeshHooks> {
    w.registry.actors.get(w.names.str(cls)).and_then(|e| e.mesh)
}

/// The class's MeshHooks take the calls (no default: one without them is warned). NotifyEnd: HitBoxesOff(HitBoxTags).
pub fn actor_hitframe_begin(w: &mut World, a: ActorRef, n: NotifyRef, idx: u32) {
    let Some(v) = view(w, a) else { return };
    let Some(h) = mesh_hooks(w, v.cls).and_then(|m| m.hit_boxes_on) else {
        warn_once(w, name!("Hitframe on an actor: its hit box handler is not ported"));
        return;
    };
    let data = w.data.clone();
    let nd = actor_notify_data(w, &data, a, n);
    let montage = v.montage.map(|m| w.names.str(m).to_owned());
    // actorHitPacket is an object literal: the members the notify leaves out are absent, not null.
    let packet = HitPacket {
        sparse: true,
        ..HitPacket::from_props(&w.names, &actor_hit_packet(nd, montage.as_deref()), Some(n))
    };
    let s = obj(&nd.p, "HitBoxScale");
    // `n.p.HitBoxScale ?? { X: 1, Y: 1, Z: 1 }`: a member the data leaves out reads undefined (NaN in the math).
    let scale = [
        s.map_or(1.0, |s| num(s, "X").unwrap_or(Real::NAN)),
        s.map_or(1.0, |s| num(s, "Y").unwrap_or(Real::NAN)),
        s.map_or(1.0, |s| num(s, "Z").unwrap_or(Real::NAN)),
    ];
    let tags = hit_box_tags(w, &nd.p);
    h(w, a, &tags, &packet, scale, idx);
}

pub fn actor_hitframe_end(w: &mut World, a: ActorRef, n: NotifyRef, idx: u32) {
    let Some(v) = view(w, a) else { return };
    let Some(h) = mesh_hooks(w, v.cls).and_then(|m| m.hit_boxes_off) else { return };
    let data = w.data.clone();
    let tags = hit_box_tags(w, &actor_notify_data(w, &data, a, n).p);
    h(w, a, &tags, idx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mvs_data::fighter::NotifyTick;

    #[test]
    fn hit_packet_renames_and_sets_defaults() {
        let mut p = Props::new();
        p.insert("OnHitSound".into(), Value::Str("ev".into()));
        p.insert("HitBoxTags".into(), Value::Arr(vec![]));
        p.insert("Damage".into(), Value::Num(3.0));
        let n = Notify {
            cls: "AnimNotify_Hitframe_C".into(),
            inst: Some("Hitframe_1".into()),
            t: 0.0,
            dur: 0.1,
            state: true,
            tick: NotifyTick::Queued,
            track: 0,
            p,
            seq: None,
            slot: None,
        };
        let out = actor_hit_packet(&n, Some("M"));
        let keys: Vec<&str> = out.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "OnHitAudio",
                "Damage",
                "HitPauseTime",
                "AttackTag",
                "AttackAnimation",
                "AttackID",
                "PlayOnHitEffects",
                "ApplyHitPauseToDamageDealer"
            ]
        );
        assert_eq!(out.get("AttackTag"), Some(&Value::Str("Hitframe_1".into())));
        assert_eq!(num(&out, "HitPauseTime"), Some(0.0));
    }
}
