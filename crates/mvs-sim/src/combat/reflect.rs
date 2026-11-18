//! Reflecting projectiles and actors: Comp_ReflectProjectile (offsets are its
//! ReflectProjectile's unless named) and the SpawnedActorParent side it calls (BPI_Reflected ubergraph @4677-5100,
//! MulticastReflect @23793-23966 with its pause @18965-19561 -> @1782 -> @3067 -> @285-1708).
//!
//! A shared component: every fighter has one (BaseFighter's SCS Comp_ReflectProjectile_GEN_VARIABLE; Pawn_Jake and
//! Pawn_TomAndJerry serialize their own values) and the callers reach it through a fighter: an actor's or a buff's
//! graph calls Fighter.Comp_ReflectProjectile.ReflectProjectile(the spawned actor, a Struct_HitBoxDamage) and reads
//! Success. The component's BeginPlay @88-371 takes InstigatingFighter = GetOwner as BP_FighterCharacter (else
//! GetOwner().GetOwner()): the fighter itself for every instance of this build.
//!
//! ReflectProjectile(Projectile, Hit), in the order it runs:
//! - LocalShouldPause true, IsAlly false @5-16; BPI_CanBeReflected false (or the cast fails) -> Success false @436.
//! - IsAlly = (the actor's Comp_Actor_Destructible TeamIndex == the component owner's) @196-400.
//! - BPI_ReflectionInfo -> ReflectAim?, AimAngleMax, AimAngleMin @531-641 (false, 0, 0 when the cast fails @807);
//!   Projectile.GetOwner().Comp_Spawner.RemoveActorListServer(Projectile) @668-761.
//! - The actor's Comp_Pawn_HitBoxHandler.ClearAllHitActors @869-920.
//! - Not an ally @956 -> @2034-2462: the handler's Instigator and the actor's Instigator = the component's owner.
//! - Both @970-2017: LocalShouldPause = the owner fighter's TeamIndex != the actor's old GetOwner's @970-1322;
//!   SetOwner(InstigatingFighter) @1385; Comp_Actor_Child.UpdateParent @1481 (nothing binds LostChild); the actor's
//!   TeamIndex = InstigatingFighter's team @1596-1913; SetLifeSpan(InitialLifeSpan) @1950.
//! - @2467-5840: MoveX / MoveY of InstigatingFighter @2546; the owner's Comp_Pawn_ChargeAttack GetChargeValue(4), (0),
//!   (1) @2744-2980; the launch vector @3027-4048, the stick's aim @4185-5182, MaxSpeed @5233-5371,
//!   SetVelocityServer @5439 (-> SetVelocityMulticast -> ubergraph @376: MaxSpeed and Velocity, then @10
//!   PF_Post_Event(sfx_shared_combat_reflect, InstigatingFighter)); BPI_Reflected(InstigatingFighter, damage mult,
//!   knockback mult, LocalShouldPause) @5480-5767.
//! - broadcast ReflectedProjectile() @5892 (nothing binds it); Success true @6110.
//!
//! The actor's GetOwner and TeamIndex are one field (`owner`: the team is the owner's) and its Instigator is the same
//! fighter unless `Projectile::inst_override` names another. A reflect by an enemy sets the owner to the reflecting
//! fighter and clears the override (@2105 / @2277).
// Deviation: a reflect by an ally (IsAlly: the same team) changes GetOwner (@1385) but keeps the Instigator (@956
// skips @2034); the port keeps the owner, the Instigator's side: the team is the same either way, and the spawner
// lists are left by `unlisted`.
// Deviation: the timing depends on where the caller runs in the frame. The pause's Delays count down in the actor's
// own tick (`reflect_tick`, from the next one it runs), the SetLifeSpan timer from the actor's age at the call. A
// caller ahead of the actor's tick: both Delays exact, the SetLifeSpan end one frame early. A caller behind it: the
// Delays exact only when the actor had a pending latent action at its tick; without one the build's Delay(0.0) fires
// in that frame's leftover latent pass and the port's one frame late.

use crate::Real;
use crate::actors::ActorId;
use crate::actors::ActorRef;
use crate::actors::projectile::ReflectState;
use crate::events::Event;
use crate::fighter::FighterId;
use crate::fighter::moves::V2;
use crate::math;
use crate::registry::entries::ReflectHooks;
use crate::world::World;
use mvs_data::value::{Props, Value};

/// The component's values on a fighter: its pawn's instance over BaseFighter's (the SCS template carries the CDO's:
/// MinReflectSpeed 2000, ChargeModSpeed? true, ChagreSpeedMaxAddition 500, UseAttacksChargeDamage? /
/// UseAttacksChargeKnockBack? true, ChargeDamageMult / ChargeKnockBackMult 1).
// It is read again per reflect, not cached per character data (a reflect is rare).
struct Cfg {
    min_speed: Real,
    charge_mod_speed: bool,
    addition: Real,
    use_dmg: bool,
    dmg_mult: Real,
    use_kb: bool,
    kb_mult: Real,
}

const COMP: &str = "Comp_ReflectProjectile_GEN_VARIABLE";

fn truthy(v: Option<&Value>) -> bool {
    crate::actors::projectile::truthy(v)
}

fn make_cfg(own: Option<&Props>, base: Option<&Props>) -> Option<Cfg> {
    if own.is_none() && base.is_none() {
        return None;
    }
    let v = |k: &str| own.and_then(|p| p.get(k)).or_else(|| base.and_then(|p| p.get(k)));
    let n = |k: &str| crate::math::narrow(v(k).and_then(Value::as_f64).unwrap_or(0.0));
    Some(Cfg {
        min_speed: n("MinReflectSpeed"),
        charge_mod_speed: truthy(v("ChargeModSpeed?")),
        addition: n("ChagreSpeedMaxAddition"),
        use_dmg: truthy(v("UseAttacksChargeDamage?")),
        dmg_mult: n("ChargeDamageMult"),
        use_kb: truthy(v("UseAttacksChargeKnockBack?")),
        kb_mult: n("ChargeKnockBackMult"),
    })
}

/// The fighter's data's component of the name, base (BaseFighter's template) or pawn.
fn fighter_comp(w: &World, f: FighterId, pawn: bool) -> Option<&Props> {
    let ch = w.names.str(w.fighter(f).ch);
    let c = &w.data.chars.get(ch)?.fighter.components;
    let m = if pawn { &c.pawn } else { &c.base };
    m.get(COMP).and_then(|c| c.p.as_obj())
}

/// `cfgOf`: the fighter's own component.
fn cfg_of(w: &World, f: FighterId) -> Option<Cfg> {
    make_cfg(fighter_comp(w, f, true), fighter_comp(w, f, false))
}

/// A spawned actor's own Comp_ReflectProjectile (its class data's), `None` for a class without one.
fn dealer_comp(w: &World, a: ActorRef) -> Option<&Props> {
    let cls = crate::actors::world_actor::actor_cls(w, a)?;
    let o = crate::fighter::common::lookup_owner(w, a);
    let d = crate::fighter::common::actor_class_def(w, o, cls)?;
    d.components.get("Comp_ReflectProjectile").and_then(Value::as_obj)
}

/// The classes whose graph overrides Reflected or MulticastReflect (besides SpawnedActorParent's own), lower case with
/// the data's _C. A class that is one of them, or has one among its parents, is reflected only with its own entry
/// (`ReflectHooks` on its kind).
const OVERRIDES: [&str; 22] = [
    "basketballprojectile_c",
    "basketball_projectilev2_c",
    "c018swapprojectile_c",
    "c019_splitterfollowup_c",
    "c019_splitterprojectile_c",
    "meeseeks_chargeuppercut_c",
    "c023b_sawblade_edgetrace_c",
    "item_batarangv2_c",
    "bugsbunnysafe_c",
    "creaturespit_c",
    "dynamite_spawnedactor_c",
    "jerryrocket_c",
    "tom_and_jerry_tennisball_c",
    "meeseeks_run_c",
    "meeseeks_spawnedactor_c",
    "velmaaimprojectile_c",
    "c019blasterchargeprojectile_c",
    "c020_blasterprojectileparent_c",
    "firebolt_c",
    "jerrycork_c",
    "jerrykickedrocket_c",
    "tomaimrocket1_c",
];

fn is_override(s: &str) -> bool {
    OVERRIDES.iter().any(|o| o.eq_ignore_ascii_case(s))
}

/// A class whose parents the data does not give is refused too (its overrides are not known).
fn overrides(w: &World, t: ActorRef) -> bool {
    let Some(cls) = crate::actors::world_actor::actor_cls(w, t) else { return true };
    if is_override(w.names.str(cls)) {
        return true;
    }
    let o = crate::fighter::common::lookup_owner(w, t);
    let Some(supers) = crate::fighter::common::actor_class_def(w, o, cls).and_then(|d| d.supers.as_deref()) else {
        return true;
    };
    supers.split(' ').any(is_override)
}

/// `canBeReflected`: BPI_CanBeReflected, SpawnedActorParent's CanBeReflected: the class
/// default unless the class's graph writes it (`ReflectHooks::can_be_reflected`, a projectile's: Meeseeks_Base
/// ActivateObject @296).
pub fn can_be_reflected(w: &World, t: ActorRef) -> bool {
    if let ActorRef::Projectile(_) = t
        && let Some(own) = hooks_of(w, t).can_be_reflected
    {
        return own(w, t);
    }
    truthy(crate::actors::world_actor::actor_p(w, t).get("CanBeReflected"))
}

/// The target's reflect overrides (`None`: its kind has no entry, or no reflect hooks).
fn hooks_of(w: &World, t: ActorRef) -> ReflectHooks {
    use crate::registry::ActorHooks;
    match (t, crate::actors::world_actor::actor_entry(w, t).map(|e| e.hooks)) {
        (ActorRef::Projectile(_), Some(ActorHooks::Projectile(h))) => h.reflect,
        (ActorRef::Item(_), Some(ActorHooks::Item(h))) => h.reflect,
        _ => ReflectHooks::default(),
    }
}

fn has_hooks(h: &ReflectHooks) -> bool {
    h.reflected.is_some()
        || h.multicast.is_some()
        || h.multicast_pause.is_some()
        || h.can_be_reflected.is_some()
        || h.hit_box_damage.is_some()
}

/// UKismetMathLibrary::DegAtan2 (FMath::Atan2 x 180 / pi in one multiply).
fn deg_atan2(y: Real, x: Real) -> Real {
    math::ue::deg_atan2(y, x)
}

fn sgn(v: Real, pos: Real) -> Real {
    if v == 0.0 {
        0.0
    } else if v > 0.0 {
        pos
    } else {
        -pos
    }
}

/// FindNearAngleDirection(A1, A2) (Comp_ReflectProjectile's function): a = (A1 + 180) - (A2 + 180) (@0-84, each
/// sum rounded to float32 before the difference) and a -/+ 360, the one of least size; Positive = it < 0,
/// AngleDifrence = Round(it) (the Round node's RoundToInt). ReturnMult is not read.
fn near_angle(a1: Real, a2: Real) -> (bool, Real) {
    let a = (a1 + 180.0) - (a2 + 180.0);
    let b = a - 360.0;
    let c = a + 360.0;
    let s0 = if a.abs() < b.abs() { a } else { b };
    let s1 = if s0.abs() < c.abs() { s0 } else { c };
    (s1 < 0.0, crate::real(math::ue::round_to_int(s1)))
}

/// `pmcActive`: the movement component flies: a projectile always, an item in flight (lying,
/// its ProjectileMovement is off and the body moves it).
pub fn pmc_active(w: &World, t: ActorRef) -> bool {
    match t {
        ActorRef::Projectile(_) => true,
        ActorRef::Item(id) => w.actors.items.get(id).is_some_and(|it| it.flying),
        _ => false,
    }
}

/// The fields a reflect reads and writes, of a projectile or an item.
fn reflect_mut(w: &mut World, t: ActorRef) -> Option<&mut Option<ReflectState>> {
    match t {
        ActorRef::Projectile(id) => w.actors.projectiles.get_mut(id).map(|p| &mut p.reflect),
        ActorRef::Item(id) => w.actors.items.get_mut(id).map(|it| &mut it.reflect),
        _ => None,
    }
}

fn reflect_of(w: &World, t: ActorRef) -> Option<&ReflectState> {
    match t {
        ActorRef::Projectile(id) => w.actors.projectiles.get(id)?.reflect.as_ref(),
        ActorRef::Item(id) => w.actors.items.get(id)?.reflect.as_ref(),
        _ => None,
    }
}

/// `reflectProjectile`: fighter f's Comp_ReflectProjectile.ReflectProjectile(t, Hit), Hit's
/// KnockBackDirectionVector as the caller made it (`kb`: the struct's X, Y). Returns Success. `dealer`: the spawned
/// actor whose own component deals it (pfgDoDamage @15181: Meeseeks_SpawnedActor_C alone of the data has one): its
/// class's values over the CDO's and its own charge reads (its copy from its instigator, `combat::elem`); f is its
/// owner (the stick, the team, the new owner, the sound). A dealer whose class has no component reflects with f's.
pub fn reflect_projectile(w: &mut World, f: FighterId, t: ActorRef, kb: V2, dealer: Option<ActorRef>) -> bool {
    let dealer = dealer.filter(|&d| dealer_comp(w, d).is_some());
    let cfg = match dealer {
        Some(d) => make_cfg(dealer_comp(w, d), fighter_comp(w, f, false)),
        None => cfg_of(w, f),
    };
    let Some(cfg) = cfg else {
        crate::actors::world_actor::warn("reflect: no Comp_ReflectProjectile in the fighter's data");
        return false;
    };
    let reflectable = match t {
        ActorRef::Projectile(_) => true,
        ActorRef::Item(id) => crate::actors::item::is_owned(w, id),
        _ => false,
    };
    if crate::actors::world_actor::actor_dead(w, t) || !can_be_reflected(w, t) || !reflectable {
        return false;
    }
    let hooks = hooks_of(w, t);
    if !has_hooks(&hooks) && overrides(w, t) {
        crate::actors::world_actor::warn(
            "reflect: the class overrides Reflected / MulticastReflect (or its parents are not in the data) and has no reflect entry: refused",
        );
        return false;
    }
    let old = crate::actors::world_actor::actor_owner(w, t);
    let old_team = old.map_or(-1, |o| w.fighter(o).team.team);
    let team = w.fighter(f).team.team;
    let ally = old_team == team;
    // @452-761: BPI_ReflectionInfo, RemoveActorListServer on the old owner's spawner.
    let p = crate::actors::world_actor::actor_p(w, t);
    let can_aim = truthy(p.get("CanAimReflect"));
    let aim_max = crate::math::narrow(p.get("MaxReflectAngle").and_then(Value::as_f64).unwrap_or(0.0));
    let aim_min = crate::math::narrow(p.get("MinReflectAngle").and_then(Value::as_f64).unwrap_or(0.0));
    let life = crate::math::narrow(p.get("InitialLifeSpan").and_then(Value::as_f64).unwrap_or(0.0));
    let mut listed = false;
    if let Some(o) = old {
        let l = &mut w.fighter_mut(o).items.spawned_actors;
        if let Some(i) = l.iter().position(|&x| x == t) {
            l.remove(i);
            listed = true;
        }
    }
    let is_proj = matches!(t, ActorRef::Projectile(_));
    if let Some(st) = reflect_mut(w, t) {
        let st = st.get_or_insert(ReflectState { unlisted: false, pause: 0, left: 0, boxes: false, life_span_at: -1 });
        if listed || is_proj {
            st.unlisted = true;
        }
    }
    // @970-1322 (the old owner's team), @1385 SetOwner, @2034-2462 / @5927 the Instigator.
    let should_pause = team != old_team;
    let (mx, my) = {
        let inp = &w.fighter(f).controls.inp;
        (inp.x, inp.y)
    };
    // @2744-2980: GetChargeValue(4), (0), (1) of the component owner (1, 1, 0 off a charge's montage).
    let ch = match dealer {
        Some(d) => crate::actors::world_actor::actor_charge(w, d).copied().flatten(),
        None => crate::combat::elem::instigator_charge(w, f),
    };
    let pct = ch.map_or(0.0, |c| c.pct);
    let c_dmg = ch.map_or(1.0, |c| c.dmg);
    let c_kb = ch.map_or(1.0, |c| c.kb);
    // @3027-4048: LaunchBaseVector = (0, side, up) x speed, from the hit's direction: the larger part's sign reversed
    // (Y > 0 -> -1, X > 0 -> 1 on UE Y: towards sim -x), the other part 0 (ties go to the X part).
    let (kx, ky) = (kb.x, kb.y);
    let y_wins = ky.abs() > kx.abs();
    let up = if y_wins { sgn(ky, -1.0) } else { 0.0 };
    let side = if y_wins { 0.0 } else { sgn(kx, 1.0) };
    let (vx, vy, max_speed) = match t {
        ActorRef::Projectile(id) => {
            let Some(pr) = w.actors.projectiles.get_mut(id) else { return false };
            pr.hit_set.clear();
            pr.hit_actors.clear();
            if !ally {
                pr.owner = Some(f);
                // @2105 the handler's Instigator, @2277 the actor's: the reflecting pawn (an ally pass's goes)
                if matches!(pr.inst_override, crate::actors::projectile::OwnerOverride::Fighter(_)) {
                    pr.inst_override = crate::actors::projectile::OwnerOverride::None;
                }
            }
            if let Some(st) = pr.reflect.as_mut() {
                st.life_span_at = if life > 0.0 {
                    math::to_int32(crate::real(pr.age) + crate::combat::hit::timer_ticks(Some(life)))
                } else {
                    -1
                };
            }
            (pr.vx, pr.vy, pr.max_speed)
        }
        ActorRef::Item(id) => {
            let Some(it) = w.actors.items.get_mut(id) else { return false };
            it.hit_set.clear();
            it.hit_actors.clear();
            if !ally {
                it.owner = Some(f);
            }
            if life > 0.0 {
                crate::actors::world_actor::warn("reflect: an item's InitialLifeSpan is not ported");
            }
            (it.vx, it.vy, it.max_speed)
        }
        _ => return false,
    };
    let ue_y = -vx;
    let ue_z = vy;
    let vsize = math::ue::size2d([ue_y, ue_z]); // VSize(ProjectileMovement.Velocity)
    let floor = math::max(vsize, cfg.min_speed);
    let charged = floor + (cfg.addition * pct);
    let speed = if cfg.charge_mod_speed { charged } else { floor };
    let lb_y = side * speed;
    let lb_z = up * speed;
    // @4185-5182: the stick's aim. Signs (MoveY > 0 -> -1, MoveX > 0 -> 1), DegAtan2(x part, y part) against
    // DegAtan2(LB.Y, LB.Z); RotateAngleAxis(LB, Positive ? AimAngleMax : AimAngleMin, (1, 0, 0)); LB itself when
    // ReflectAim? and (|AngleDifrence| >= 179 or the stick at rest), else the rotated vector; X zeroed.
    let (sy, sx) = (sgn(my, -1.0), sgn(mx, 1.0));
    let (positive, diff) = near_angle(deg_atan2(lb_y, lb_z), deg_atan2(sx, sy));
    let ang = if positive { aim_max } else { aim_min };
    let keep = can_aim && (diff.abs() >= 179.0 || (mx == 0.0 && my == 0.0));
    // The node's axis (1, 0, 0) is a unit vector already (GetSafeNormal returns it as it is).
    let r = math::ue::rotate_angle_axis([0.0, lb_y, lb_z], ang, [1.0, 0.0, 0.0]);
    let out_y = if keep { lb_y } else { r[1] };
    let out_z = if keep { lb_z } else { r[2] };
    // @5233-5439: MaxSpeed = FMax(its MaxSpeed, FMax(VSize, MinReflectSpeed) + ChagreSpeedMaxAddition x pct) (the
    // addition whatever ChargeModSpeed? is); SetVelocityMulticast @376: MaxSpeed, Velocity, the sound.
    let ms = math::max(max_speed, charged);
    let active = pmc_active(w, t);
    let nvx = if out_y == 0.0 { 0.0 } else { -out_y };
    match t {
        ActorRef::Projectile(id) => {
            if let Some(pr) = w.actors.projectiles.get_mut(id) {
                pr.max_speed = ms;
                pr.vx = nvx;
                pr.vy = out_z;
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id) {
                it.max_speed = ms;
                if active {
                    it.vx = nvx;
                    it.vy = out_z;
                }
            }
            if !active {
                // Deviation: an item lying on its body (its ProjectileMovement off): the movement's Velocity is
                // written and the body's velocity is not read here; the port's one velocity is the body's, left as
                // it is.
                crate::actors::world_actor::warn(
                    "reflect: reflected while its ProjectileMovement is off (the body's velocity kept)",
                );
            }
        }
        _ => {}
    }
    let fid = w.fighter(f).id;
    w.events.push(Event::Sound {
        fighter: fid,
        ev: crate::name!("sfx_shared_combat_reflect"),
        x: None,
        y: None,
        hit: None,
        pooled: false,
        key: None,
    });
    // @5480-5767: BPI_Reflected(InstigatingFighter, damage, knockback, LocalShouldPause).
    let dmg_mult = if cfg.use_dmg { c_dmg } else { 1.0 + (cfg.dmg_mult - 1.0) * pct };
    let kb_mult = if cfg.use_kb { c_kb } else { 1.0 + (cfg.kb_mult - 1.0) * pct };
    bpi_reflected(w, t, f, dmg_mult, kb_mult, should_pause, hooks);
    // @5892 ReflectedProjectile(): bound by nothing.
    true
}

/// `reflectWorldActor`: ReflectProjectile(t, Hit) of a fighter's component on an actor of
/// the World's own list (a SpawnedActorParent kind that is no projectile or item). BPI_CanBeReflected @5-436 is its
/// class default there: no such class of the data sets it, and only Meeseeks_Base's graph writes it. So Success is
/// false and nothing is done.
// Not ported: such a class with CanBeReflected true (none) is warned, not reflected.
pub fn reflect_world_actor(w: &mut World, t: ActorId) -> bool {
    let a = ActorRef::Actor(t);
    if !crate::actors::world_actor::actor_dead(w, a) && can_be_reflected(w, a) {
        crate::actors::world_actor::warn(
            "reflect: an actor of the World's list can be reflected; its reflect is not ported",
        );
    }
    false
}

/// SpawnedActorParent BPI_Reflected (ubergraph @4677, authority): HitBoxDamage.BaseKnockBack x knockback @4749,
/// Damage x damage @4895, the handler's UpdateHitBoxDamage @4931 (`activate_boxes`), Reflected() @5017 (a class's
/// override is its hook), MulticastReflect(NewOwner, ShouldPause) @5031, then with ShouldPause
/// Comp_State_Manager.UpdateBuffsOnReflect(NewOwner) @5073.
fn bpi_reflected(w: &mut World, t: ActorRef, by: FighterId, dmg: Real, kb: Real, pause: bool, hooks: ReflectHooks) {
    if let Some(own) = hooks.hit_box_damage {
        let h = own(w, t).clone(); // the class's own variable
        set_hit(w, t, h);
    }
    if dmg != 1.0 || kb != 1.0 {
        let hit = match t {
            ActorRef::Projectile(id) => w.actors.projectiles.get_mut(id).map(|p| &mut p.hit),
            ActorRef::Item(id) => w.actors.items.get_mut(id).map(|it| &mut it.hit),
            _ => None,
        };
        if let Some(h) = hit {
            h.base_knock_back = Some(h.base_knock_back.unwrap_or(0.0) * kb);
            h.damage = Some(h.damage.unwrap_or(0.0) * dmg);
        }
    }
    activate_boxes(w, t);
    if let Some(f) = hooks.reflected {
        f(w, t);
    }
    let p = hooks.multicast_pause.map_or(pause, |f| f(w, t, by, pause));
    multicast_reflect(w, t, by, p);
    if let Some(f) = hooks.multicast {
        f(w, t, by, pause);
    }
    if pause {
        crate::combat::elem::update_buffs_on_reflect(w, t, by);
    }
}

fn set_hit(w: &mut World, t: ActorRef, h: crate::combat::HitPacket) {
    match t {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                p.hit = h;
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id) {
                it.hit = h;
            }
        }
        _ => {}
    }
}

/// The handler's UpdateHitBoxDamage(HitBoxTags, HitBoxDamage, ElementalBuffs, (1, 1, 1), On Hit Buffs, false) @4931
/// (Comp_Pawn_HitBoxHandler ubergraph @10842-11867): the boxes on with the packet, SetRelativeScale3D(1, 1, 1),
/// whatever HitBoxOn? is (the variable is not written); HasHitAFighter is not touched. A projectile's boxes come on
/// unless the HitOnlyOneFighter refusal holds (`reflect_boxes_on`); an item's on is kept beside its HitBoxOn?
/// (`ReflectState::boxes`).
// Unverified: an actor with HitBoxOn? false hits with the box alone; ItemActorParent HitEnemyPlayer @7207 has no
// HitBoxOn? test, the other classes' readers of the variable are not checked.
// Not ported: a HitBoxScale other than (1, 1, 1) (no reflectable class of the data has one) is warned and its box
// keeps its size.
fn activate_boxes(w: &mut World, t: ActorRef) {
    let s = crate::actors::world_actor::actor_p(w, t).get("HitBoxScale").and_then(Value::as_obj);
    if let Some(s) = s {
        let c = |k: &str| s.get(k).and_then(Value::as_f64).unwrap_or(1.0) != 1.0;
        if c("X") || c("Y") || c("Z") {
            crate::actors::world_actor::warn("reflect: a HitBoxScale; the reflect's scale 1 is not ported");
        }
    }
    if crate::actors::world_actor::actor_dead(w, t) {
        return;
    }
    match t {
        ActorRef::Projectile(id) => {
            if let Some(p) = w.actors.projectiles.get_mut(id) {
                crate::actors::projectile::reflect_boxes_on(p);
            }
        }
        ActorRef::Item(id) => {
            if let Some(it) = w.actors.items.get_mut(id)
                && !it.hit_on
                && let Some(st) = it.reflect.as_mut()
            {
                st.boxes = true;
            }
        }
        _ => {}
    }
}

/// SpawnedActorParent MulticastReflect @23793: a homing movement stops homing, SetOwner(NewOwner), broadcast
/// ReflectedDispatch @23918, SetTeamColor (drawn from the owner), and with ShouldPause the pause @18965, once while its
/// gate is closed: the MaxLifetimeTimer cleared @19027, PS_Reflect at the root with the new owner's colour
/// @19131-19503 (the ReflectFx event), Delay(0.0) @19561.
fn multicast_reflect(w: &mut World, t: ActorRef, by: FighterId, pause: bool) {
    crate::combat::elem::clear_homing(w, t);
    // ReflectedDispatch: Buff_SlowProjectile's family (OnReflected @2562), then the buffs of the actor-buff path that
    // bind it.
    // Unverified: the bindings' order is not read.
    if crate::actors::world_actor::actor_elem(w, t).is_some_and(|e| e.is_some()) {
        crate::combat::elem::slow_hit_anything(w, t);
        crate::combat::elem::actor_buff_signal(w, t, crate::registry::signals::ActorBuffSignal::Reflected, None);
    }
    if !pause || reflect_of(w, t).is_none_or(|st| st.pause != 0) {
        return;
    }
    let left = math::to_int32(crate::fighter::common::latent_frames(0.0));
    let (seq, item) = match t {
        ActorRef::Projectile(id) => {
            let Some(p) = w.actors.projectiles.get_mut(id) else { return };
            crate::actors::projectile::clear_max_lifetime(p);
            (p.seq, false)
        }
        ActorRef::Item(id) => (w.actors.items.get(id).map_or(-1, |it| it.seq), true),
        _ => return,
    };
    if let Some(Some(st)) = reflect_mut(w, t) {
        st.pause = 1;
        st.left = left;
    }
    let owner = w.fighter(by).id;
    w.events.push(Event::ReflectFx { actor: seq, item, owner });
}

/// `reflectTick`: the pause's latent actions, in the actor's tick. Delay(0.0) @1782:
/// BPI_SetMobilityToStationary(true), CurrentVFXTimeDilation 0.5 (drawn while the pause is 2) and
/// Montage_SetPlayRate(the active montage, 0.5), then Delay(0.375) @3067. That one @285: the dilation back to 1, on
/// authority with MaxLifetime > 0 the MaxLifetimeTimer set again @342-470, SetMobilityToStationary(false) @497, the
/// rate back to 1, the gate open again @1685.
pub fn reflect_tick(w: &mut World, t: ActorRef) {
    let Some(Some(st)) = reflect_mut(w, t) else { return };
    if st.pause == 0 {
        return;
    }
    st.left -= 1;
    if st.left > 0 {
        return;
    }
    if st.pause == 1 {
        st.pause = 2;
        st.left = math::to_int32(crate::fighter::common::latent_frames(0.375));
        crate::combat::holds::set_mobility_to_stationary(w, t, true);
        if crate::actors::actor_montage::has_anim(w, t) {
            crate::actors::actor_montage::montage_set_play_rate(w, t, 0.5);
        }
        return;
    }
    st.pause = 0;
    st.left = 0;
    if let ActorRef::Projectile(id) = t
        && let Some(p) = w.actors.projectiles.get_mut(id)
    {
        crate::actors::projectile::restart_max_lifetime(p);
    }
    crate::combat::holds::set_mobility_to_stationary(w, t, false);
    if crate::actors::actor_montage::has_anim(w, t) {
        crate::actors::actor_montage::montage_set_play_rate(w, t, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_angle_takes_the_least_turn() {
        assert_eq!(near_angle(10.0, 20.0), (true, -10.0));
        assert_eq!(near_angle(350.0, 10.0), (true, -20.0));
        assert_eq!(near_angle(10.0, 350.0), (false, 20.0));
        // a tie of 180 and -180 keeps a - 360's side
        assert_eq!(near_angle(90.0, -90.0), (true, -180.0));
        assert_eq!(near_angle(0.0, 0.0), (false, 0.0));
    }

    #[test]
    fn overrides_match_without_case() {
        assert!(is_override("BasketBallProjectile_C"));
        assert!(is_override("C020_BlasterProjectileParent_C"));
        assert!(!is_override("ParentProjectilePhysics_C"));
    }
}
