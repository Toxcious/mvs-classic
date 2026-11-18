//! The World's actors as the record writes them: a projectile (`ProjectileRecord`),
//! an item or another actor (`ActorRecord`, ACTOR_ROWS), each with its `rest`; and the arenas rebuilt in the
//! record's order on a load.

use mvs_data::value::{Props, Value as DataValue};
use mvs_sim::World;
use mvs_sim::actors::item::{DefSrc, ItemExt};
use mvs_sim::actors::projectile::OwnerOverride;
use mvs_sim::actors::projectile::ProjectileExt;
use mvs_sim::actors::smoke_bomb::{SmokeGrenade, SmokeZone};
use mvs_sim::actors::{
    ActorRef, Item, ItemClass, Projectile, ProjectileClass, WorldActor, WorldActorClass, WorldActorState,
};
use mvs_sim::chars::batman::batarang::BatarangState;
use mvs_sim::chars::batman::grapple::Grapple;
use mvs_sim::chars::batman::perks_grapple::PerkGrapple;
use mvs_sim::chars::shaggy::Shockwave;
use mvs_sim::chars::shaggy::shockwave::Then;
use mvs_sim::combat::BuffClass;
use mvs_sim::combat::elem::{ElemBuff, ElemExt, ElemState};
use mvs_sim::combat::perks::PerkActorExt;
use mvs_sim::world::BlastBounds;
use mvs_trace::{ActorRecord, ProjectileRecord, Value};

use crate::conv::{
    Plain, Rec, Unset, as_obj, check_keys, int, named_enum, num, obj, opaque, plain_keys, plain_struct, via_default,
};
use crate::cx::{Lx, Wx};
use crate::parts::data_put;

/// `{static: "object"}`: the class's parameters (`p`), static data the oracle writes by its kind alone.
pub(crate) fn static_object() -> Value {
    obj(vec![("static", Value::Str("object".to_owned()))])
}

pub(crate) fn hit_actors_put(s: &indexmap::IndexSet<ActorRef>, x: &Wx) -> Vec<Value> {
    s.iter().map(|a| a.to_value(x)).collect()
}

pub(crate) fn hit_actors_load(s: &mut indexmap::IndexSet<ActorRef>, r: &[Value], l: &mut Lx) {
    s.clear();
    for v in r {
        if let Some(a) = ActorRef::from_value(v, l) {
            s.insert(a);
        }
    }
}

fn fid_or(f: Option<mvs_sim::fighter::FighterId>, x: &Wx) -> f64 {
    f.map_or(-1.0, |f| f64::from(x.fid(f)))
}

// ------------------------------------------------------------------ the small values of the actors

// An actor's blast bounds (`blastBounds`: `{l, r, b, t}`, the World's are `{left, right, bottom, top}`).
plain_struct!(BlastBounds { "b" = bottom, "l" = left, "r" = right, "t" = top });

plain_struct!(mvs_collision::Ground { "bottom" = bottom, "left" = left, "right" = right, "top" = top });

named_enum!(Then, "shockwave step", [Then::Stage => "stage", Then::Destroy => "destroy"]);

/// `platResponse`: ECollisionResponse's value.
impl Plain for mvs_sim::actors::projectile::CollisionResponse {
    fn to_value(&self, _: &Wx) -> Value {
        use mvs_sim::actors::projectile::CollisionResponse as C;
        Value::Num(match self {
            C::Ignore => 0.0,
            C::Overlap => 1.0,
            C::Block => 2.0,
        })
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(c) = Self::from_value(v, l) {
            *self = c;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        use mvs_sim::actors::projectile::CollisionResponse as C;
        match int(num(v, l)?, l)? {
            0 => Some(C::Ignore),
            1 => Some(C::Overlap),
            2 => Some(C::Block),
            n => {
                l.miss(format_args!("{n} is not a collision response"));
                None
            }
        }
    }
}

/// A projectile by reference (`aimAt`).
impl Plain for mvs_sim::actors::ProjectileId {
    fn to_value(&self, x: &Wx) -> Value {
        ActorRef::Projectile(*self).to_value(x)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(p) = Self::from_value(v, l) {
            *self = p;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        match ActorRef::from_value(v, l)? {
            ActorRef::Projectile(p) => Some(p),
            _ => {
                l.miss("an actor where a projectile is held");
                None
            }
        }
    }
}

/// `instOverride` / `listOwner`: undefined (left out), null, or a fighter.
impl Unset for OwnerOverride {
    fn unset(&self) -> bool {
        *self == OwnerOverride::Unset
    }
    fn clear(&mut self) {
        *self = OwnerOverride::Unset;
    }
}

impl Plain for OwnerOverride {
    fn to_value(&self, x: &Wx) -> Value {
        match self {
            OwnerOverride::Unset | OwnerOverride::None => Value::Null,
            OwnerOverride::Fighter(f) => f.to_value(x),
        }
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(o) = Self::from_value(v, l) {
            *self = o;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        match v {
            Value::Null => Some(OwnerOverride::None),
            v => mvs_sim::fighter::FighterId::from_value(v, l).map(OwnerOverride::Fighter),
        }
    }
}

/// `BuffHome`: `{actor, fighter, mag}`, the target in `fighter` when it is a fighter, else in
/// `actor`, the other null.
impl Plain for mvs_sim::actors::projectile::BuffHome {
    fn to_value(&self, x: &Wx) -> Value {
        let (fighter, actor) = match self.to {
            ActorRef::Fighter(_) => (self.to.to_value(x), Value::Null),
            _ => (Value::Null, self.to.to_value(x)),
        };
        obj(vec![("actor", actor), ("fighter", fighter), ("mag", Value::Num(f64::from(self.mag)))])
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        if let Some(h) = Self::from_value(v, l) {
            *self = h;
        }
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let o = as_obj(v, l)?;
        check_keys(o, &["actor", "fighter", "mag"], l);
        let to = match (o.get("fighter"), o.get("actor")) {
            (Some(f), _) if !matches!(f, Value::Null) => ActorRef::from_value(f, l)?,
            (_, Some(a)) if !matches!(a, Value::Null) => ActorRef::from_value(a, l)?,
            _ => {
                l.miss("a buffHome with no target");
                return None;
            }
        };
        let mag = num(o.get("mag").unwrap_or(&Value::Null), l)?;
        Some(mvs_sim::actors::projectile::BuffHome { to, mag: mvs_sim::math::narrow(mag) })
    }
}
plain_struct!(mvs_sim::combat::ChargeVals { "buff" = buff, "dmg" = dmg, "kb" = kb, "pct" = pct });

/// The keys of the grapple hook's class, over a struct holding it as `g`.
macro_rules! grapple_keys {
    ($($pre:tt)*) => {
        plain_keys!($($pre)* "queued" = queued, "queuedAlly" = queued_ally, "closed" = closed, "stuck" = stuck, "target" = target, "reeling" = reeling, "reelAt" = reel_at, "activateAt" = activate_at, "chargeEndAt" = charge_end_at, "destroyAt" = destroy_at, "kbAt" = kb_at, "evadeAt" = evade_at, "jumpAt" = jump_at, "ignored" = ignored, "allyIn" = ally_in,)
    };
}

/// The keys GrappleHookProjectile_Perk_C adds.
macro_rules! perk_grapple_keys {
    ($($pre:tt)*) => {
        plain_keys!($($pre)* "bound" = bound, "boomHit" = boom_hit, "boomDue" = boom_due,)
    };
}

/// The keys of FireWallHazard_C's class; `spawnYaw` is written once a world spawn
/// states it, `fxTeam` once a graph writes it.
macro_rules! fire_wall_keys {
    ($($pre:tt)*) => {
        plain_keys!($($pre)* "boxZ" = box_z, "boxHx" = box_hx, "boxHy" = box_hy, "hurtZ" = hurt_z, "hurtHalf" = hurt_half, "hadFire" = had_fire, "hadIce" = had_ice, "inProj" = in_proj, "inItems" = in_items, "spawnYaw" ~ spawn_yaw, "fxTeam" ~ fx_team,)
    };
}

/// The class's own keys of a projectile's `ext`.
fn ext_put(p: &Projectile, x: &Wx, o: &mut Vec<(String, Value)>) {
    match &p.ext {
        ProjectileExt::Plain => {}
        ProjectileExt::Grapple(g) => {
            grapple_keys!(put g, x, o;);
        }
        ProjectileExt::PerkGrapple(s) => {
            let g = &s.g;
            grapple_keys!(put g, x, o;);
            perk_grapple_keys!(put s, x, o;);
        }
        ProjectileExt::FireWall(s) => {
            fire_wall_keys!(put s, x, o;);
        }
        ProjectileExt::IcePellet(s) => crate::superman::ice::pellet_put(s, x, o),
        ProjectileExt::Ball(s) => crate::lebron::ball::put(s, x, o),
        ProjectileExt::SassBubble(s) => crate::lebron::bubble::put(s, x, o),
        ProjectileExt::Knife(s) => crate::arya::knife::put(s, x, o),
        ProjectileExt::Firebolt(s) => crate::creature::fire::put(s, x, o),
        ProjectileExt::WreckingBall(s) => crate::creature::perks_ball::put(s, x, o),
        ProjectileExt::SeismicReactor(s) => crate::creature::seismic::put(s, x, o),
        ProjectileExt::TetherProjectile(s) => crate::creature::tether::put(s, x, o),
        ProjectileExt::Chain(s) => crate::black_adam::chain::put(s, x, o),
        ProjectileExt::DoubleStrike(s) => crate::black_adam::double_strike::put(s, x, o),
        ProjectileExt::HomingBall(s) => crate::black_adam::projectiles::put(s, x, o),
        ProjectileExt::CookGrenade(s) => crate::morty::projectiles::grenade_put(s, x, o),
        ProjectileExt::BracerClash(s) => crate::wonder_woman::actors::clash_put(s, x, o),
        ProjectileExt::Lasso(s) => crate::wonder_woman::actors::lasso_put(s, x, o),
        ProjectileExt::Bomb(s) => crate::iron_giant::actor_state::bomb_put(s, x, o),
        ProjectileExt::DiscShot(s) => crate::iron_giant::actor_state::disc_put(s, x, o),
        ProjectileExt::Missile(s) => crate::iron_giant::actor_state::missile_put(s, x, o),
        ProjectileExt::ThrownScrap(s) => crate::iron_giant::actors::scrap_put(s, x, o),
        ProjectileExt::HealPickup(s) => crate::taz::chicken::heal_pickup_put(s, x, o),
        ProjectileExt::StewSpitAir(s) => crate::taz::projectiles::spit_air_put(s, x, o),
        ProjectileExt::BubbleCharge(s) => crate::steven::bubble::charge_put(s, x, o),
        ProjectileExt::StevenTether(s) => crate::steven::tether::tether_put(s, x, o),
        ProjectileExt::HealGrenade(s) => crate::steven::watermelon::grenade_put(s, x, o),
        ProjectileExt::GarnetGauntlets(s) => crate::garnet::gauntlets::gauntlets_put(s, x, o),
        ProjectileExt::GarnetMarker2(s) => crate::garnet::marker::marker_put(s, x, o),
        ProjectileExt::GarnetBolt(s) => crate::garnet::zone::bolt_put(s, x, o),
        ProjectileExt::StillActor(s) => crate::bugs::perks::still_put(s, x, o),
        ProjectileExt::BugsShockwave(s) => crate::bugs::perks::shockwave_put(s, x, o),
        ProjectileExt::Rocket(s) => crate::bugs::rockets::rocket_put(s, x, o),
        ProjectileExt::BugsBunnySafe(s) => crate::bugs::safe::safe_put(s, x, o),
        ProjectileExt::SafeDrop(s) => crate::bugs::safe::drop_put(s, x, o),
        ProjectileExt::MeeseeksRun(s) => crate::items::meeseeks::run_put(s, x, o),
        ProjectileExt::FartBomb(s) => crate::rick::fart_bomb::fart_bomb_put(s, x, o),
        ProjectileExt::MeeseeksSummon(s) => crate::rick::meeseeks::summon_put(s, x, o),
        ProjectileExt::FinnTreasure(s) => crate::finn::coins::treasure_put(s, x, o),
        ProjectileExt::Mark(s) => crate::velma::mark::mark_put(s, x, o),
        ProjectileExt::AimProjectile(s) => crate::velma::mark::aim_put(s, x, o),
        ProjectileExt::FollowUp(s) => crate::velma::mark::follow_put(s, x, o),
        ProjectileExt::VelmaBook(s) => crate::velma::gadgets::book_put(s, x, o),
        ProjectileExt::VelmaBurst(s) => crate::velma::gadgets::burst_put(s, x, o),
        ProjectileExt::VelmaSassBubble(s) => crate::velma::gadgets::bubble_put(s, x, o),
        ProjectileExt::Saw(s) => crate::stripe::saw::saw_put(s, x, o),
        ProjectileExt::GunShot(s) => crate::stripe::gun::gun_shot_put(s, x, o),
        ProjectileExt::BoomBox(s) => crate::stripe::dash::boombox_put(s, x, o),
        ProjectileExt::JerryCork(s) => crate::tom_and_jerry::champagne::cork_put(s, x, o),
        ProjectileExt::Dynamite(s) => crate::tom_and_jerry::dynamite::dynamite_put(s, x, o),
        ProjectileExt::Lure(s) => crate::tom_and_jerry::fishing::lure_put(s, x, o),
        ProjectileExt::AimRocket(s) => crate::tom_and_jerry::kicked::aim_rocket_put(s, x, o),
        ProjectileExt::KickedRocket(s) => crate::tom_and_jerry::kicked::kicked_put(s, x, o),
        ProjectileExt::JerryRocket(s) => crate::tom_and_jerry::rocket::rocket_put(s, x, o),
        ProjectileExt::TennisBall(s) => crate::tom_and_jerry::tennis::ball_put(s, x, o),
        ProjectileExt::Blaster(s) => crate::marvin::blaster::blaster_put(s, x, o),
        ProjectileExt::BubbleProj(s) => crate::marvin::bubble::bubble_put(s, x, o),
        ProjectileExt::C018Flag(s) => crate::marvin::flag::flag_put(s, x, o),
        ProjectileExt::Arrow(s) => crate::gizmo::bow::arrow_put(s, x, o),
        ProjectileExt::GizmoCar(s) => crate::gizmo::car::gizmo_car_put(s, x, o),
        ProjectileExt::MusicPickup(s) => crate::gizmo::music::pickup_put(s, x, o),
        ProjectileExt::MusicProjectile(s) => crate::gizmo::music::music_projectile_put(s, x, o),
        ProjectileExt::PianoNote(s) => crate::gizmo::music::piano_note_put(s, x, o),
        ProjectileExt::Popcorn(s) => crate::gizmo::popcorn::popcorn_put(s, x, o),
        ProjectileExt::ItemCar(s) => crate::items::car::item_car_put(s, x, o),
        ProjectileExt::BatMobileCar(s) => crate::items::car::batmobile_put(s, x, o),
    }
}

/// [`ext_put`]'s inverse; the keys it knows go into `known`.
fn ext_load(p: &mut Projectile, ob: &mvs_trace::Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    match &mut p.ext {
        ProjectileExt::Plain => {}
        ProjectileExt::Grapple(g) => {
            grapple_keys!(load g, ob, l;);
            known.extend(grapple_keys!(names));
        }
        ProjectileExt::PerkGrapple(s) => {
            let s: &mut PerkGrapple = s;
            let g = &mut s.g;
            grapple_keys!(load g, ob, l;);
            perk_grapple_keys!(load s, ob, l;);
            known.extend(grapple_keys!(names));
            known.extend(perk_grapple_keys!(names));
        }
        ProjectileExt::FireWall(s) => {
            fire_wall_keys!(load s, ob, l;);
            known.extend(fire_wall_keys!(names));
        }
        ProjectileExt::IcePellet(s) => crate::superman::ice::pellet_load(s, ob, l, known),
        ProjectileExt::Ball(s) => crate::lebron::ball::load(s, ob, l, known),
        ProjectileExt::SassBubble(s) => crate::lebron::bubble::load(s, ob, l, known),
        ProjectileExt::Knife(s) => crate::arya::knife::load(s, ob, l, known),
        ProjectileExt::Firebolt(s) => crate::creature::fire::load(s, ob, l, known),
        ProjectileExt::WreckingBall(s) => crate::creature::perks_ball::load(s, ob, l, known),
        ProjectileExt::SeismicReactor(s) => crate::creature::seismic::load(s, ob, l, known),
        ProjectileExt::TetherProjectile(s) => crate::creature::tether::load(s, ob, l, known),
        ProjectileExt::Chain(s) => crate::black_adam::chain::load(s, ob, l, known),
        ProjectileExt::DoubleStrike(s) => crate::black_adam::double_strike::load(s, ob, l, known),
        ProjectileExt::HomingBall(s) => crate::black_adam::projectiles::load(s, ob, l, known),
        ProjectileExt::CookGrenade(s) => crate::morty::projectiles::grenade_load(s, ob, l, known),
        ProjectileExt::BracerClash(s) => crate::wonder_woman::actors::clash_load(s, ob, l, known),
        ProjectileExt::Lasso(s) => crate::wonder_woman::actors::lasso_load(s, ob, l, known),
        ProjectileExt::Bomb(s) => crate::iron_giant::actor_state::bomb_load(s, ob, l, known),
        ProjectileExt::DiscShot(s) => crate::iron_giant::actor_state::disc_load(s, ob, l, known),
        ProjectileExt::Missile(s) => crate::iron_giant::actor_state::missile_load(s, ob, l, known),
        ProjectileExt::ThrownScrap(s) => crate::iron_giant::actors::scrap_load(s, ob, l, known),
        ProjectileExt::HealPickup(s) => crate::taz::chicken::heal_pickup_load(s, ob, l, known),
        ProjectileExt::StewSpitAir(s) => crate::taz::projectiles::spit_air_load(s, ob, l, known),
        ProjectileExt::BubbleCharge(s) => crate::steven::bubble::charge_load(s, ob, l, known),
        ProjectileExt::StevenTether(s) => crate::steven::tether::tether_load(s, ob, l, known),
        ProjectileExt::HealGrenade(s) => crate::steven::watermelon::grenade_load(s, ob, l, known),
        ProjectileExt::GarnetGauntlets(s) => crate::garnet::gauntlets::gauntlets_load(s, ob, l, known),
        ProjectileExt::GarnetMarker2(s) => crate::garnet::marker::marker_load(s, ob, l, known),
        ProjectileExt::GarnetBolt(s) => crate::garnet::zone::bolt_load(s, ob, l, known),
        ProjectileExt::StillActor(s) => crate::bugs::perks::still_load(s, ob, l, known),
        ProjectileExt::BugsShockwave(s) => crate::bugs::perks::shockwave_load(s, ob, l, known),
        ProjectileExt::Rocket(s) => crate::bugs::rockets::rocket_load(s, ob, l, known),
        ProjectileExt::BugsBunnySafe(s) => crate::bugs::safe::safe_load(s, ob, l, known),
        ProjectileExt::SafeDrop(s) => crate::bugs::safe::drop_load(s, ob, l, known),
        ProjectileExt::MeeseeksRun(s) => crate::items::meeseeks::run_load(s, ob, l, known),
        ProjectileExt::FartBomb(s) => crate::rick::fart_bomb::fart_bomb_load(s, ob, l, known),
        ProjectileExt::MeeseeksSummon(s) => crate::rick::meeseeks::summon_load(s, ob, l, known),
        ProjectileExt::FinnTreasure(s) => crate::finn::coins::treasure_load(s, ob, l, known),
        ProjectileExt::Mark(s) => crate::velma::mark::mark_load(s, ob, l, known),
        ProjectileExt::AimProjectile(s) => crate::velma::mark::aim_load(s, ob, l, known),
        ProjectileExt::FollowUp(s) => crate::velma::mark::follow_load(s, ob, l, known),
        ProjectileExt::VelmaBook(s) => crate::velma::gadgets::book_load(s, ob, l, known),
        ProjectileExt::VelmaBurst(s) => crate::velma::gadgets::burst_load(s, ob, l, known),
        ProjectileExt::VelmaSassBubble(s) => crate::velma::gadgets::bubble_load(s, ob, l, known),
        ProjectileExt::Saw(s) => crate::stripe::saw::saw_load(s, ob, l, known),
        ProjectileExt::GunShot(s) => crate::stripe::gun::gun_shot_load(s, ob, l, known),
        ProjectileExt::BoomBox(s) => crate::stripe::dash::boombox_load(s, ob, l, known),
        ProjectileExt::JerryCork(s) => crate::tom_and_jerry::champagne::cork_load(s, ob, l, known),
        ProjectileExt::Dynamite(s) => crate::tom_and_jerry::dynamite::dynamite_load(s, ob, l, known),
        ProjectileExt::Lure(s) => crate::tom_and_jerry::fishing::lure_load(s, ob, l, known),
        ProjectileExt::AimRocket(s) => crate::tom_and_jerry::kicked::aim_rocket_load(s, ob, l, known),
        ProjectileExt::KickedRocket(s) => crate::tom_and_jerry::kicked::kicked_load(s, ob, l, known),
        ProjectileExt::JerryRocket(s) => crate::tom_and_jerry::rocket::rocket_load(s, ob, l, known),
        ProjectileExt::TennisBall(s) => crate::tom_and_jerry::tennis::ball_load(s, ob, l, known),
        ProjectileExt::Blaster(s) => crate::marvin::blaster::blaster_load(s, ob, l, known),
        ProjectileExt::BubbleProj(s) => crate::marvin::bubble::bubble_load(s, ob, l, known),
        ProjectileExt::C018Flag(s) => crate::marvin::flag::flag_load(s, ob, l, known),
        ProjectileExt::Arrow(s) => crate::gizmo::bow::arrow_load(s, ob, l, known),
        ProjectileExt::GizmoCar(s) => crate::gizmo::car::gizmo_car_load(s, ob, l, known),
        ProjectileExt::MusicPickup(s) => crate::gizmo::music::pickup_load(s, ob, l, known),
        ProjectileExt::MusicProjectile(s) => crate::gizmo::music::music_projectile_load(s, ob, l, known),
        ProjectileExt::PianoNote(s) => crate::gizmo::music::piano_note_load(s, ob, l, known),
        ProjectileExt::Popcorn(s) => crate::gizmo::popcorn::popcorn_load(s, ob, l, known),
        ProjectileExt::ItemCar(s) => crate::items::car::item_car_load(s, ob, l, known),
        ProjectileExt::BatMobileCar(s) => crate::items::car::batmobile_load(s, ob, l, known),
    }
}

// ------------------------------------------------------------------ the actors' elemental buffs

/// `ElemState`: `{box, overlap, pooled, buffs}` and `sphere` where true.
impl Plain for ElemState {
    fn to_value(&self, x: &Wx) -> Value {
        let mut o = vec![
            ("box", Value::Bool(self.bx)),
            ("overlap", Value::Bool(self.overlap)),
            ("pooled", Value::Bool(self.pooled)),
            ("buffs", Value::List(self.buffs.iter().map(|b| elem_buff_put(b, x)).collect())),
        ];
        if self.sphere {
            o.push(("sphere", Value::Bool(true)));
        }
        obj(o)
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(ob) = as_obj(v, l) else { return };
        for (k, set) in [("box", &mut self.bx), ("overlap", &mut self.overlap), ("pooled", &mut self.pooled)] {
            match ob.get(k) {
                Some(v) => l.at(k, |l| set.load_value(v, l)),
                None => l.miss(format_args!("key {k} absent")),
            }
        }
        self.sphere = matches!(ob.get("sphere"), Some(Value::Bool(true)));
        match ob.get("buffs") {
            Some(Value::List(list)) => l.at("buffs", |l| elem_buffs_load(self, list, l)),
            _ => l.miss("key buffs absent or not a list"),
        }
        check_keys(ob, &["box", "buffs", "overlap", "pooled", "sphere"], l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        let mut s = ElemState::default();
        s.load_value(v, l);
        Some(s)
    }
}

/// The keys of an `ElemBuff` but `cls` and `ext`, each left out while undefined (`passive`: true or undefined).
macro_rules! elem_buff_keys {
    ($($pre:tt)*) => {
        plain_keys!($($pre)*
            "stacks" = stacks, "timer" ~ timer, "passive" ~ passive, "mod" ~ speed_mod, "life" ~ life,
            "stack" ~ stack, "gate" ~ gate, "addAt" ~ add_at,
        )
    };
}

fn elem_buff_put(b: &ElemBuff, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = vec![("cls".to_owned(), Value::Str(b.class.name(&x.w.names).to_owned()))];
    elem_buff_keys!(put b, x, o;);
    if let Some(e) = elem_ext_put(b.ext, x) {
        o.push(("ext".to_owned(), e));
    }
    Value::Obj(mvs_trace::Obj::new(o))
}

/// The list in the record's order; an entry of the same class at the same place keeps its serial (what a caller
/// holds across ticks), any other is a new entry with the list's next serial.
fn elem_buffs_load(s: &mut ElemState, list: &[Value], l: &mut Lx) {
    let mut out = Vec::with_capacity(list.len());
    for (i, v) in list.iter().enumerate() {
        l.at(&format!("[{i}]"), |l| {
            let Some(ob) = as_obj(v, l) else { return };
            let Some(Value::Str(cls)) = ob.get("cls") else {
                l.miss("key cls absent");
                return;
            };
            let class = crate::parts::buff_class(cls, l);
            let Some(class) = class else { return };
            let serial = match s.buffs.get(i) {
                Some(b) if b.class == class => b.serial,
                _ => {
                    let n = s.next_serial;
                    s.next_serial = s.next_serial.wrapping_add(1);
                    n
                }
            };
            let mut b = ElemBuff {
                serial,
                class,
                stacks: 0,
                timer: None,
                passive: false,
                speed_mod: None,
                life: None,
                stack: None,
                gate: None,
                add_at: None,
                ext: ElemExt::None,
            };
            elem_buff_keys!(load b, ob, l;);
            b.ext = match ob.get("ext") {
                Some(e) => l.at("ext", |l| elem_ext_load(class, e, l)),
                None => ElemExt::None,
            };
            let mut known = vec!["cls", "ext"];
            known.extend(elem_buff_keys!(names));
            check_keys(ob, &known, l);
            out.push(b);
        });
    }
    s.buffs = out;
}

/// `ElemBuff.ext` as the classes write it (`None`: undefined).
fn elem_ext_put(e: ElemExt, x: &Wx) -> Option<Value> {
    let b = Value::Bool;
    let n = Value::Num;
    Some(match e {
        ElemExt::None => return None,
        // (`scale` with EffectGravity alone)
        ElemExt::Slow30 { bound, scale } => {
            let mut o = vec![("bound", b(bound))];
            if let Some(s) = scale {
                o.push(("scale", n(f64::from(s))));
            }
            obj(o)
        }
        ElemExt::HeightChange { off } => obj(vec![("off", n(f64::from(off)))]),
        ElemExt::DestroyAfterApplied(d) => obj(vec![("DestroyAfterApplied", b(d))]),
        ElemExt::ApplyBuff { bound, active } | ElemExt::Perk(PerkActorExt::Gray { bound, active }) => {
            obj(vec![("active", b(active)), ("bound", b(bound))])
        }
        ElemExt::Perk(PerkActorExt::Electric { by, team }) => {
            obj(vec![("by", n(f64::from(by))), ("team", n(f64::from(team)))])
        }
        // (`by` the Instigator's id)
        ElemExt::ChainVictim { at, by } => obj(vec![("at", n(f64::from(at))), ("by", n(f64::from(by)))]),
        ElemExt::SlowReflect { r, refl } => obj(vec![("r", n(f64::from(r))), ("refl", b(refl))]),
        ElemExt::Follower { hidden, bound, delay } => {
            obj(vec![("bound", b(bound)), ("delay", n(f64::from(delay))), ("follower", b(true)), ("hidden", b(hidden))])
        }
        ElemExt::SnowballHead { x, y, scale } => {
            obj(vec![("x", n(f64::from(x))), ("y", n(f64::from(y))), ("scale", n(f64::from(scale)))])
        }
        ElemExt::GarnetStopProjectile(e) => crate::garnet::stop_projectile::stop_put(&e, x),
        ElemExt::C015HitProjectile(e) => crate::taz::projectile_eat::hit_projectile_put(&e, x),
        ElemExt::ApplyTasty(e) => crate::taz::tasty::apply_tasty_actor_put(&e, x),
        ElemExt::Tasty(e) => crate::taz::tasty::tasty_actor_put(&e, x),
        ElemExt::TastyOnHit(e) => crate::taz::tasty::tasty_on_hit_actor_put(&e, x),
        ElemExt::ThroughTunnel(e) => crate::bugs::tunnel::through_tunnel_put(&e, x),
        ElemExt::C020Silenced(e) => crate::rick::silenced::silenced_actor_put(&e, x),
        ElemExt::C018Bubbled(e) => crate::marvin::bubble::bubbled_put(&e, x),
        ElemExt::C018HomingProjectile(e) => crate::marvin::homing::homing_projectile_put(&e, x),
        ElemExt::C018ReverseProjectile(e) => crate::marvin::reverse::reverse_put(&e, x),
        ElemExt::C018PerkProjectileSpeed(e) => crate::marvin::perks::speed_put(&e, x),
        ElemExt::GrowProjectile { size, damage } => {
            crate::mutators::grow_projectile_put(f64::from(size), f64::from(damage), x)
        }
    })
}

/// An `ext` object to its variant: the classes of a module of their own by their class, the rest by its keys,
/// Buff_ProjectileGrayHealth's `{active, bound}` by its class.
fn elem_ext_load(class: BuffClass, v: &Value, l: &mut Lx) -> ElemExt {
    match class {
        BuffClass::BuffGarnetStopProjectile => {
            return ElemExt::GarnetStopProjectile(crate::garnet::stop_projectile::stop_load(v, l));
        }
        BuffClass::BuffC015HitProjectile => {
            return ElemExt::C015HitProjectile(crate::taz::projectile_eat::hit_projectile_load(v, l));
        }
        BuffClass::BuffApplyTasty => return ElemExt::ApplyTasty(crate::taz::tasty::apply_tasty_actor_load(v, l)),
        BuffClass::BuffTasty => return ElemExt::Tasty(crate::taz::tasty::tasty_actor_load(v, l)),
        BuffClass::BuffTastyOnHit => return ElemExt::TastyOnHit(crate::taz::tasty::tasty_on_hit_actor_load(v, l)),
        BuffClass::BuffProjectileThroughTunnel => {
            return ElemExt::ThroughTunnel(crate::bugs::tunnel::through_tunnel_load(v, l));
        }
        BuffClass::C020BuffSilenced => return ElemExt::C020Silenced(crate::rick::silenced::silenced_actor_load(v, l)),
        BuffClass::BuffC018BubbledProjectile => {
            return ElemExt::C018Bubbled(crate::marvin::bubble::bubbled_load(v, l));
        }
        BuffClass::BuffC018HomingProjectile | BuffClass::BuffC018PerkHomingProjectile => {
            return ElemExt::C018HomingProjectile(crate::marvin::homing::homing_projectile_load(v, l));
        }
        BuffClass::BuffC018ReverseProjectile => {
            return ElemExt::C018ReverseProjectile(crate::marvin::reverse::reverse_load(v, l));
        }
        BuffClass::BuffC018PerkProjectileSpeed => {
            return ElemExt::C018PerkProjectileSpeed(crate::marvin::perks::speed_load(v, l));
        }
        BuffClass::BuffGrowWithDamageProjectile => return crate::mutators::grow_projectile_load(v, l),
        _ => {}
    }
    let Some(o) = as_obj(v, l) else { return ElemExt::None };
    let flag = |k: &str, l: &mut Lx| match o.get(k) {
        Some(Value::Bool(b)) => *b,
        _ => {
            l.miss(format_args!("key {k} absent or not a boolean"));
            false
        }
    };
    let (e, keys): (ElemExt, &[&str]) = if o.get("follower").is_some() {
        let mut delay = 0.0;
        if let Some(v) = o.get("delay") {
            l.at("delay", |l| delay.load_value(v, l));
        }
        let e = ElemExt::Follower { hidden: flag("hidden", l), bound: flag("bound", l), delay };
        (e, &["bound", "delay", "follower", "hidden"])
    } else if let Some(v) = o.get("DestroyAfterApplied") {
        let mut d = false;
        l.at("DestroyAfterApplied", |l| d.load_value(v, l));
        (ElemExt::DestroyAfterApplied(d), &["DestroyAfterApplied"])
    } else if let Some(v) = o.get("off") {
        let mut off = 0.0;
        l.at("off", |l| off.load_value(v, l));
        (ElemExt::HeightChange { off }, &["off"])
    } else if o.get("at").is_some() {
        let mut at = 0.0;
        let mut by = -1;
        match o.get("at") {
            Some(v) => l.at("at", |l| at.load_value(v, l)),
            None => l.miss("key at absent"),
        }
        match o.get("by") {
            Some(v) => l.at("by", |l| by.load_value(v, l)),
            None => l.miss("key by absent"),
        }
        (ElemExt::ChainVictim { at, by }, &["at", "by"])
    } else if o.get("x").is_some() {
        let (mut sx, mut sy, mut scale) = (0.0, 0.0, 1.0);
        for (k, set) in [("x", &mut sx), ("y", &mut sy), ("scale", &mut scale)] {
            match o.get(k) {
                Some(v) => l.at(k, |l| set.load_value(v, l)),
                None => l.miss(format_args!("key {k} absent")),
            }
        }
        (ElemExt::SnowballHead { x: sx, y: sy, scale }, &["scale", "x", "y"])
    } else if o.get("refl").is_some() {
        let mut r = 0.0;
        match o.get("r") {
            Some(v) => l.at("r", |l| r.load_value(v, l)),
            None => l.miss("key r absent"),
        }
        (ElemExt::SlowReflect { r, refl: flag("refl", l) }, &["r", "refl"])
    } else if o.get("by").is_some() {
        let mut by = -1;
        let mut team = -1;
        for (k, set) in [("by", &mut by), ("team", &mut team)] {
            match o.get(k) {
                Some(v) => l.at(k, |l| set.load_value(v, l)),
                None => l.miss(format_args!("key {k} absent")),
            }
        }
        (ElemExt::Perk(PerkActorExt::Electric { by, team }), &["by", "team"])
    } else if o.get("active").is_some() {
        let (bound, active) = (flag("bound", l), flag("active", l));
        let e = if class == BuffClass::BuffProjectileGrayHealth {
            ElemExt::Perk(PerkActorExt::Gray { bound, active })
        } else {
            ElemExt::ApplyBuff { bound, active }
        };
        (e, &["active", "bound"])
    } else {
        let mut scale = None;
        if let Some(v) = o.get("scale") {
            l.at("scale", |l| scale.load_value(v, l));
        }
        (ElemExt::Slow30 { bound: flag("bound", l), scale }, &["bound", "scale"])
    };
    check_keys(o, keys, l);
    e
}

plain_struct!(mvs_sim::actors::projectile::LifetimePause { "count" = count, "left" = left });

plain_struct!(mvs_sim::actors::projectile::ReflectState {
    "boxes" = boxes, "left" = left, "lifeSpanAt" = life_span_at, "pause" = pause, "unlisted" = unlisted
});

opaque!(mvs_sim::combat::BuffRef, mvs_sim::fighter::StagePlatform,);

// A ProjectileBox: the PlatformBox's members and the ones the box adds, `actor` the
// projectile by reference (the box does not know it: `projectile_rest` adds it). The template and `TURNS` are
// off the object the snapshot walks (a WeakMap and a WeakSet), so not written; a load sets them from
// the projectile's class (`projectile_load`).
macro_rules! box_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "bottom" = bottom, "dead" = dead, "det" = det, "dx" = dx, "dy" = dy, "ex" = ex, "ey" = ey, "hx" = hx,
            "hy" = hy, "ignored" = ignored, "ignoredMinions" ~ ignored_minions, "item" = item, "left" = left,
            "nav" = nav, "on" = on, "ox" = ox, "oz" = oz, "placed" = placed, "right" = right, "top" = top,
            "trace" = trace, "vx" = vx, "vy" = vy, "wall" = wall, "x" = x, "y" = y,
        )
    };
}

impl Plain for mvs_sim::actors::projectile::ProjectileBox {
    fn to_value(&self, x: &Wx) -> Value {
        let mut o: Vec<(String, Value)> = Vec::new();
        box_keys!(plain_keys put self, x, o;);
        Value::Obj(mvs_trace::Obj::new(o))
    }
    fn load_value(&mut self, v: &Value, l: &mut Lx) {
        let Some(ob) = as_obj(v, l) else { return };
        box_keys!(plain_keys load self, ob, l;);
        let mut names: Vec<&str> = vec!["actor"];
        names.extend_from_slice(box_keys!(plain_keys names));
        check_keys(ob, &names, l);
    }
    fn from_value(v: &Value, l: &mut Lx) -> Option<Self> {
        via_default(v, l, <Self as Plain>::load_value)
    }
}

/// A box of a projectile as the oracle writes it: with its `actor`, the projectile by reference.
pub(crate) fn solid_put(b: &mvs_sim::actors::projectile::ProjectileBox, seq: i32, x: &Wx) -> Value {
    let Value::Obj(mut ob) = b.to_value(x) else { return Value::Null };
    ob.insert(
        "actor".to_owned(),
        obj(vec![("id", Value::Num(f64::from(seq))), ("ref", Value::Str("projectile".into()))]),
    );
    Value::Obj(ob)
}

/// The boxes of a projectile as the oracle writes them.
fn solids_put(bs: &[mvs_sim::actors::projectile::ProjectileBox], seq: i32, x: &Wx) -> Value {
    Value::List(bs.iter().map(|b| solid_put(b, seq, x)).collect())
}

// The tether family's values, the class kind's plain object.
plain_struct!(mvs_sim::actors::projectile::TetherParams {
    "travel" = travel, "returnSpeed" = return_speed, "returnMaxSpeed" ~ return_max_speed, "socket" ~ socket,
    "hitSphere" ~ hit_sphere, "snap" = snap,
});

// ------------------------------------------------------------------ projectiles

pub fn projectile_put(p: &Projectile, x: &Wx) -> ProjectileRecord {
    ProjectileRecord {
        id: f64::from(p.seq),
        class: x.str(p.cls).to_owned(),
        owner: fid_or(p.owner, x),
        x: f64::from(p.x),
        y: f64::from(p.y),
        vx: f64::from(p.vx),
        vy: f64::from(p.vy),
        facing: f64::from(p.facing),
        age: f64::from(p.age),
        dead: p.dead,
        hit_actors: hit_actors_put(&p.hit_actors, x),
        rest: projectile_rest(p, x),
    }
}

fn projectile_rest(p: &Projectile, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    // A shot's held buff as the oracle writes `{ vic, b }`: the victim's reference and the whole entry, written from
    // the victim's list through the reference (`hold_buff_load`).
    let hold = match p.hold_buff {
        Some((vic, r)) => Value::Obj(mvs_trace::Obj::new(vec![
            ("vic".to_owned(), vic.to_value(x)),
            ("b".to_owned(), crate::black_adam::chain::buff_entry_put(Some(r), x)),
        ])),
        None => Value::Null,
    };
    o.push(("holdBuff".to_owned(), hold));
    plain_keys!(put p, x, o;
        "attackId" = attack_id, "prevX" = prev_x, "prevY" = prev_y, "pitch" ^ pitch, "scale" = scale, "hitRadius" = hit_radius, "hitOn" = hit_on, "hit" = hit, "hitSet" = hit_set, "dodge" ~ dodge, "charge" ~ charge, "elem" ~ elem, "lifetimePause" ~ lifetime_pause, "rotX" = rot_x, "follows" = follows, "maxSpeed" = max_speed, "gravScale" = grav_scale, "gravTick" = grav_tick, "parentVx" = parent_vx, "parentVy" = parent_vy, "compV" ~ comp_v, "moving" = moving, "lookYaw" = look_yaw, "lookPitch" ^ look_pitch, "rotateGate" = rotate_gate, "travelX" = travel_x, "travelY" = travel_y, "distToTarget" = dist_to_target, "tlPos" = tl_pos, "tlRate" = tl_rate, "tlPlaying" = tl_playing, "attached" = attached, "socket" ~ socket, "buffHome" ~ buff_home, "spedUp" = sped_up, "mobility" ~ mobility, "stationaryCount" ~ stationary_count, "aimAt" = aim_at, "aimBound" = aim_bound, "capR" = cap_r, "capH" = cap_h, "bounce" = bounce, "friction" = friction, "stopSpeed" = stop_speed, "angleFriction" = angle_friction, "minFriction" = min_friction, "prevHitTime" = prev_hit_time, "prevNx" = prev_nx, "prevNy" = prev_ny, "sliding" = sliding, "stopped" = stopped, "hitT" = hit_t, "hitNx" = hit_nx, "hitNy" = hit_ny, "hitPen" = hit_pen, "hitDepth" = hit_depth, "noDispatch" = no_dispatch, "mtdNx" = mtd_nx, "mtdNy" = mtd_ny, "mtdDepth" = mtd_depth, "avx" = avx, "avy" = avy, "platResponse" = plat_response, "capCollides" = cap_collides, "capWritten" ~ cap_written, "noCollision" ~ no_collision, "solids" ~ solids, "bounceCount" = bounce_count, "bounced" = bounced, "dieOnBounce" = die_on_bounce, "kbOffOnBounce" = kb_off_on_bounce, "impactVx" = impact_vx, "impactVy" = impact_vy, "impactX" = impact_x, "impactY" = impact_y, "hitVar" = hit_var, "hasHitAFighter" ~ has_hit_a_fighter, "sphere3D" = sphere_3d, "healedAllies" = healed_allies, "hurtRadius" = hurt_radius, "hurtOffY" = hurt_off_y, "damageDisabled" ~ damage_disabled, "attackType" = attack_type, "health" = health, "clanking" = clanking, "queueDestroy" = queue_destroy, "blockedDestroy" = blocked_destroy, "overlapping" = overlapping, "allyOver" ~ ally_over, "canTurnOnHitBox" = can_turn_on_hit_box, "hitGateTick" = hit_gate_tick, "restoreTick" = restore_tick, "baseHit" = base_hit, "baseHurt" = base_hurt, "destroyOnEnemyHit" = destroy_on_enemy_hit, "reflect" ~ reflect, "instOverride" ~ inst_override, "listOwner" ~ list_owner, "parentFighter" ~ parent_fighter, "lookupFighter" ~ lookup_fighter, "doom" ~ doom, "hitPause" ~ hit_pause, "destroyAfterPause" ~ destroy_after_pause, "sizeScale" ~ size_scale, "destroyFxBlocked" ~ destroy_fx_blocked, "destroySpawnBlocked" ~ destroy_spawn_blocked, "lifeTicks" = life_ticks, "blastAt" = blast_at, "blastBounds" = blast_bounds, "activateTick" = activate_tick, "activated" = activated, "splitTick" = split_tick, "slowTick" = slow_tick, "ended" = ended, "disarmed" = disarmed, "fuseTicks" = fuse_ticks, "tellTick" = tell_tick, "loopTick" = loop_tick, "detonated" = detonated, "grenade" = grenade, "chargeShot" = charge_shot, "bombCount" = bomb_count, "bombVisual" = bomb_visual, "plasma" = plasma, "whip" = whip, "tether" = tether, "returnSocket" ~ return_socket, "canControl" = can_control, "initialSpeed" = initial_speed, "snapTick" = snap_tick, "returnTick" = return_tick, "endTick" = end_tick, "returnStarted" = return_started, "homing" = homing, "returning" = returning, "snapped" = snapped, "tipperDue" = tipper_due, "controlled" = controlled, "kbSeq" = kb_seq,
    );
    if let (Some(bs), Some(e)) = (&p.solids, o.iter_mut().find(|e| e.0 == "solids")) {
        e.1 = solids_put(bs, p.seq, x);
    }
    ext_put(p, x, &mut o);
    o.push(("rel".into(), obj(vec![("Y", Value::Num(f64::from(p.rel.0))), ("Z", Value::Num(f64::from(p.rel.1)))])));
    // The instance's own object once a graph wrote its variables (`setInstanceVars`), written whole as the oracle
    // writes a new object; else the class's (static).
    let own = p.own_p.as_deref().map(|v| data_put(&DataValue::Obj(v.clone())));
    o.push(("p".into(), own.unwrap_or_else(static_object)));
    Value::Obj(mvs_trace::Obj::new(o))
}

pub fn projectile_load(p: &mut Projectile, r: &ProjectileRecord, l: &mut Lx) {
    if l.str(p.cls) != r.class {
        let m = format!("{} loaded into a {}", r.class, l.str(p.cls));
        l.at("class", |l| l.miss(m));
    }
    l.at("owner", |l| p.owner.load(&r.owner, l));
    l.at("x", |l| p.x.load(&r.x, l));
    l.at("y", |l| p.y.load(&r.y, l));
    l.at("vx", |l| p.vx.load(&r.vx, l));
    l.at("vy", |l| p.vy.load(&r.vy, l));
    l.at("facing", |l| p.facing.load(&r.facing, l));
    l.at("age", |l| p.age.load(&r.age, l));
    p.dead = r.dead;
    l.at("hit_actors", |l| hit_actors_load(&mut p.hit_actors, &r.hit_actors, l));
    l.at("rest", |l| {
        let Some(ob) = as_obj(&r.rest, l) else { return };
        plain_keys!(load p, ob, l;
            "attackId" = attack_id, "prevX" = prev_x, "prevY" = prev_y, "pitch" ^ pitch, "scale" = scale, "hitRadius" = hit_radius, "hitOn" = hit_on, "hit" = hit, "hitSet" = hit_set, "dodge" ~ dodge, "charge" ~ charge, "elem" ~ elem, "lifetimePause" ~ lifetime_pause, "rotX" = rot_x, "follows" = follows, "maxSpeed" = max_speed, "gravScale" = grav_scale, "gravTick" = grav_tick, "parentVx" = parent_vx, "parentVy" = parent_vy, "compV" ~ comp_v, "moving" = moving, "lookYaw" = look_yaw, "lookPitch" ^ look_pitch, "rotateGate" = rotate_gate, "travelX" = travel_x, "travelY" = travel_y, "distToTarget" = dist_to_target, "tlPos" = tl_pos, "tlRate" = tl_rate, "tlPlaying" = tl_playing, "attached" = attached, "socket" ~ socket, "buffHome" ~ buff_home, "spedUp" = sped_up, "mobility" ~ mobility, "stationaryCount" ~ stationary_count, "aimAt" = aim_at, "aimBound" = aim_bound, "capR" = cap_r, "capH" = cap_h, "bounce" = bounce, "friction" = friction, "stopSpeed" = stop_speed, "angleFriction" = angle_friction, "minFriction" = min_friction, "prevHitTime" = prev_hit_time, "prevNx" = prev_nx, "prevNy" = prev_ny, "sliding" = sliding, "stopped" = stopped, "hitT" = hit_t, "hitNx" = hit_nx, "hitNy" = hit_ny, "hitPen" = hit_pen, "hitDepth" = hit_depth, "noDispatch" = no_dispatch, "mtdNx" = mtd_nx, "mtdNy" = mtd_ny, "mtdDepth" = mtd_depth, "avx" = avx, "avy" = avy, "platResponse" = plat_response, "capCollides" = cap_collides, "capWritten" ~ cap_written, "noCollision" ~ no_collision, "solids" ~ solids, "bounceCount" = bounce_count, "bounced" = bounced, "dieOnBounce" = die_on_bounce, "kbOffOnBounce" = kb_off_on_bounce, "impactVx" = impact_vx, "impactVy" = impact_vy, "impactX" = impact_x, "impactY" = impact_y, "hitVar" = hit_var, "hasHitAFighter" ~ has_hit_a_fighter, "sphere3D" = sphere_3d, "healedAllies" = healed_allies, "hurtRadius" = hurt_radius, "hurtOffY" = hurt_off_y, "damageDisabled" ~ damage_disabled, "attackType" = attack_type, "health" = health, "clanking" = clanking, "queueDestroy" = queue_destroy, "blockedDestroy" = blocked_destroy, "overlapping" = overlapping, "allyOver" ~ ally_over, "canTurnOnHitBox" = can_turn_on_hit_box, "hitGateTick" = hit_gate_tick, "restoreTick" = restore_tick, "baseHit" = base_hit, "baseHurt" = base_hurt, "destroyOnEnemyHit" = destroy_on_enemy_hit, "reflect" ~ reflect, "instOverride" ~ inst_override, "listOwner" ~ list_owner, "parentFighter" ~ parent_fighter, "lookupFighter" ~ lookup_fighter, "doom" ~ doom, "hitPause" ~ hit_pause, "destroyAfterPause" ~ destroy_after_pause, "sizeScale" ~ size_scale, "destroyFxBlocked" ~ destroy_fx_blocked, "destroySpawnBlocked" ~ destroy_spawn_blocked, "lifeTicks" = life_ticks, "blastAt" = blast_at, "blastBounds" = blast_bounds, "activateTick" = activate_tick, "activated" = activated, "splitTick" = split_tick, "slowTick" = slow_tick, "ended" = ended, "disarmed" = disarmed, "fuseTicks" = fuse_ticks, "tellTick" = tell_tick, "loopTick" = loop_tick, "detonated" = detonated, "grenade" = grenade, "chargeShot" = charge_shot, "bombCount" = bomb_count, "bombVisual" = bomb_visual, "plasma" = plasma, "whip" = whip, "tether" = tether, "returnSocket" ~ return_socket, "canControl" = can_control, "initialSpeed" = initial_speed, "snapTick" = snap_tick, "returnTick" = return_tick, "endTick" = end_tick, "returnStarted" = return_started, "homing" = homing, "returning" = returning, "snapped" = snapped, "tipperDue" = tipper_due, "controlled" = controlled, "kbSeq" = kb_seq,
        );
        match ob.get("rel").and_then(|v| as_obj(v, l)) {
            Some(rel) => {
                if let Some(v) = rel.get("Y") {
                    l.at("rel.Y", |l| p.rel.0.load_value(v, l));
                }
                if let Some(v) = rel.get("Z") {
                    l.at("rel.Z", |l| p.rel.1.load_value(v, l));
                }
            }
            None => l.miss("rel absent"),
        }
        let mut known: Vec<&str> = plain_keys!(names
            "attackId" = attack_id, "prevX" = prev_x, "prevY" = prev_y, "pitch" ^ pitch, "scale" = scale, "hitRadius" = hit_radius, "hitOn" = hit_on, "hit" = hit, "hitSet" = hit_set, "dodge" ~ dodge, "charge" ~ charge, "elem" ~ elem, "lifetimePause" ~ lifetime_pause, "rotX" = rot_x, "follows" = follows, "maxSpeed" = max_speed, "gravScale" = grav_scale, "gravTick" = grav_tick, "parentVx" = parent_vx, "parentVy" = parent_vy, "compV" ~ comp_v, "moving" = moving, "lookYaw" = look_yaw, "lookPitch" ^ look_pitch, "rotateGate" = rotate_gate, "travelX" = travel_x, "travelY" = travel_y, "distToTarget" = dist_to_target, "tlPos" = tl_pos, "tlRate" = tl_rate, "tlPlaying" = tl_playing, "attached" = attached, "socket" ~ socket, "buffHome" ~ buff_home, "spedUp" = sped_up, "mobility" ~ mobility, "stationaryCount" ~ stationary_count, "aimAt" = aim_at, "aimBound" = aim_bound, "capR" = cap_r, "capH" = cap_h, "bounce" = bounce, "friction" = friction, "stopSpeed" = stop_speed, "angleFriction" = angle_friction, "minFriction" = min_friction, "prevHitTime" = prev_hit_time, "prevNx" = prev_nx, "prevNy" = prev_ny, "sliding" = sliding, "stopped" = stopped, "hitT" = hit_t, "hitNx" = hit_nx, "hitNy" = hit_ny, "hitPen" = hit_pen, "hitDepth" = hit_depth, "noDispatch" = no_dispatch, "mtdNx" = mtd_nx, "mtdNy" = mtd_ny, "mtdDepth" = mtd_depth, "avx" = avx, "avy" = avy, "platResponse" = plat_response, "capCollides" = cap_collides, "capWritten" ~ cap_written, "noCollision" ~ no_collision, "solids" ~ solids, "bounceCount" = bounce_count, "bounced" = bounced, "dieOnBounce" = die_on_bounce, "kbOffOnBounce" = kb_off_on_bounce, "impactVx" = impact_vx, "impactVy" = impact_vy, "impactX" = impact_x, "impactY" = impact_y, "hitVar" = hit_var, "hasHitAFighter" ~ has_hit_a_fighter, "sphere3D" = sphere_3d, "healedAllies" = healed_allies, "hurtRadius" = hurt_radius, "hurtOffY" = hurt_off_y, "damageDisabled" ~ damage_disabled, "attackType" = attack_type, "health" = health, "clanking" = clanking, "queueDestroy" = queue_destroy, "blockedDestroy" = blocked_destroy, "overlapping" = overlapping, "allyOver" ~ ally_over, "canTurnOnHitBox" = can_turn_on_hit_box, "hitGateTick" = hit_gate_tick, "restoreTick" = restore_tick, "baseHit" = base_hit, "baseHurt" = base_hurt, "destroyOnEnemyHit" = destroy_on_enemy_hit, "reflect" ~ reflect, "instOverride" ~ inst_override, "listOwner" ~ list_owner, "parentFighter" ~ parent_fighter, "lookupFighter" ~ lookup_fighter, "doom" ~ doom, "hitPause" ~ hit_pause, "destroyAfterPause" ~ destroy_after_pause, "sizeScale" ~ size_scale, "destroyFxBlocked" ~ destroy_fx_blocked, "destroySpawnBlocked" ~ destroy_spawn_blocked, "lifeTicks" = life_ticks, "blastAt" = blast_at, "blastBounds" = blast_bounds, "activateTick" = activate_tick, "activated" = activated, "splitTick" = split_tick, "slowTick" = slow_tick, "ended" = ended, "disarmed" = disarmed, "fuseTicks" = fuse_ticks, "tellTick" = tell_tick, "loopTick" = loop_tick, "detonated" = detonated, "grenade" = grenade, "chargeShot" = charge_shot, "bombCount" = bomb_count, "bombVisual" = bomb_visual, "plasma" = plasma, "whip" = whip, "tether" = tether, "returnSocket" ~ return_socket, "canControl" = can_control, "initialSpeed" = initial_speed, "snapTick" = snap_tick, "returnTick" = return_tick, "endTick" = end_tick, "returnStarted" = return_started, "homing" = homing, "returning" = returning, "snapped" = snapped, "tipperDue" = tipper_due, "controlled" = controlled, "kbSeq" = kb_seq,
        )
        .to_vec();
        // A box's template and `TURNS` are off the record (`box_keys`): a box the load made takes its class's.
        if let (Some(bs), Some((comp, turns))) = (p.solids.as_mut(), mvs_sim::chars::bugs::rockets::box_kind(p.class)) {
            let template = l.name(comp);
            for b in bs {
                b.template = template;
                b.turns = turns;
            }
        }
        known.extend(["rel", "p", "holdBuff"]);
        l.at("holdBuff", |l| p.hold_buff = hold_buff_load(ob.get("holdBuff"), l));
        p.own_p = match ob.get("p") {
            Some(v @ Value::Obj(o)) if o.get("static").is_none() => match crate::parts::data_load(v) {
                DataValue::Obj(o) => Some(std::sync::Arc::new(o)),
                _ => None,
            },
            _ => None,
        };
        ext_load(p, ob, l, &mut known);
        check_keys(ob, &known, l);
    });
}

/// The serial a loaded `holdBuff` waits with until [`resolve_hold_buffs`] (never handed out: serials count up from 0).
const PENDING_HELD: u32 = u32::MAX - 5;
/// The serial of a held buff its victim's list no longer holds (swept): its entry is kept whole for the writer.
const GONE_HELD: u32 = u32::MAX - 10;
const HELD: &str = "Buff_HeldByPlasmaBall_C";

/// A shot's held buff (`holdBuff`, `{ vic, b }`): the victim's reference and the whole entry; it loads as a marker
/// [`resolve_hold_buffs`] turns into the victim's entry once every fighter has its buffs (an entry gone from the list,
/// `swept`, is kept whole for the writer).
fn hold_buff_load(v: Option<&Value>, l: &mut Lx) -> Option<(mvs_sim::fighter::FighterId, mvs_sim::combat::BuffRef)> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::Obj(o)) => {
            let (Some(vv), Some(b @ Value::Obj(bo))) = (o.get("vic"), o.get("b")) else {
                l.miss("a held buff without its victim and entry");
                return None;
            };
            if !matches!(bo.get("cls"), Some(Value::Str(c)) if c == HELD) {
                l.miss("a held buff of another class");
                return None;
            }
            let mut vic: Option<mvs_sim::fighter::FighterId> = None;
            vic.load_value(vv, l);
            let vic = vic?;
            if matches!(bo.get("swept"), Some(Value::Bool(true))) {
                return Some((vic, crate::black_adam::chain::buff_entry_gone_load(b, vic, GONE_HELD, l)));
            }
            Some((vic, mvs_sim::combat::BuffRef { fighter: vic, serial: PENDING_HELD }))
        }
        Some(_) => {
            l.miss("a held buff that is no object");
            None
        }
    }
}

/// Turns the markers [`hold_buff_load`] left into the victims' entries: the first Buff_HeldByPlasmaBall_C of the list.
// Unverified: the record names no entry, so a victim held by two shots at once would resolve both to the first.
pub fn resolve_hold_buffs(w: &mut World, l: &mut Lx) {
    let mut fix: Vec<(mvs_sim::actors::ProjectileId, Option<mvs_sim::combat::BuffRef>)> = Vec::new();
    for (id, p) in w.actors.projectiles.iter() {
        let Some((vic, r)) = p.hold_buff else { continue };
        if r.serial != PENDING_HELD {
            continue;
        }
        match w.fighter(vic).buffs.buffs.iter().find(|b| w.names.str(b.cls) == HELD) {
            Some(b) => fix.push((id, Some(mvs_sim::combat::BuffRef { fighter: vic, serial: b.serial }))),
            None => {
                l.miss(format_args!("a shot's held buff {HELD} is in no list of its victim"));
                fix.push((id, None));
            }
        }
    }
    for (id, r) in fix {
        if let Some(p) = w.actors.projectiles.get_mut(id) {
            p.hold_buff = r.map(|r| (r.fighter, r));
        }
    }
}

fn projectile_make(r: &ProjectileRecord, l: &mut Lx) -> Option<Projectile> {
    let Some(class) = ProjectileClass::ALL.iter().copied().find(|c| c.name() == r.class) else {
        l.miss(format_args!("no projectile class {}", r.class));
        return None;
    };
    let owner = l.fighter(r.owner);
    let attack_id = match as_obj(&r.rest, l).and_then(|o| o.get("attackId")) {
        Some(v) => i32::from_value(v, l)?,
        None => 0,
    };
    let owner_id = int(r.owner, l)?;
    let mut p = Projectile::new(l.name(&r.class), class, owner, owner_id, attack_id);
    p.seq = int(r.id, l)?;
    // The class's own state (its kind's `make`), filled by the load.
    p.ext = match class {
        ProjectileClass::GrappleHookProjectileV2 => ProjectileExt::Grapple(Grapple::default()),
        ProjectileClass::GrappleHookProjectilePerk => ProjectileExt::PerkGrapple(PerkGrapple::default()),
        // (FireStormHazard_C enters the wall's parts: its entry is the wall's with its own class)
        ProjectileClass::FireWallHazard | ProjectileClass::FireStormHazard => {
            ProjectileExt::FireWall(Default::default())
        }
        ProjectileClass::SupermanIceBreath | ProjectileClass::SupermanIceBreathEducated => {
            ProjectileExt::IcePellet(Default::default())
        }
        ProjectileClass::BasketBallProjectile
        | ProjectileClass::BasketBallProjectileShot
        | ProjectileClass::BasketBallProjectilePass
        | ProjectileClass::BasketBallProjectileAllyPass => ProjectileExt::Ball(Default::default()),
        ProjectileClass::C016SassBubble => ProjectileExt::SassBubble(Default::default()),
        ProjectileClass::MarkTeleportProjectile => ProjectileExt::Knife(Default::default()),
        ProjectileClass::Firebolt | ProjectileClass::FireboltChild => ProjectileExt::Firebolt(Default::default()),
        ProjectileClass::CreatureWreckingBall => ProjectileExt::WreckingBall(Default::default()),
        ProjectileClass::SeismicReactor => ProjectileExt::SeismicReactor(Default::default()),
        // (SuperSaveTetherProjectile_C's kind has no make: a plain projectile)
        ProjectileClass::TetherProjectile => ProjectileExt::TetherProjectile(Default::default()),
        ProjectileClass::C021ElectricChain | ProjectileClass::C021ElectricChainStarter => {
            ProjectileExt::Chain(Default::default())
        }
        ProjectileClass::C021HomingProjectile => ProjectileExt::HomingBall(Default::default()),
        ProjectileClass::C021DoubleStrikeProjectile => ProjectileExt::DoubleStrike(Default::default()),
        ProjectileClass::C019CookGrenade
        | ProjectileClass::C019CookGrenadeNoGrav
        | ProjectileClass::C019CookGrenadeSplit => ProjectileExt::CookGrenade(Default::default()),
        ProjectileClass::WonderWomanMeterBracerClash => ProjectileExt::BracerClash(Default::default()),
        ProjectileClass::LassoProjectileFastV3 => ProjectileExt::Lasso(Default::default()),
        ProjectileClass::C017Bomb => ProjectileExt::Bomb(Default::default()),
        ProjectileClass::C017DiscShot => ProjectileExt::DiscShot(Default::default()),
        ProjectileClass::WeaponModeMissile => ProjectileExt::Missile(Default::default()),
        ProjectileClass::C017ThrownScrap => ProjectileExt::ThrownScrap(Default::default()),
        ProjectileClass::C015HealPickup => ProjectileExt::HealPickup(Default::default()),
        ProjectileClass::C015StewSpitAir => ProjectileExt::StewSpitAir(Default::default()),
        ProjectileClass::StevenBubbleCharge => ProjectileExt::BubbleCharge(Default::default()),
        ProjectileClass::StevenTetherStunProjectile => ProjectileExt::StevenTether(Default::default()),
        ProjectileClass::HealGrenade => ProjectileExt::HealGrenade(Default::default()),
        ProjectileClass::GarnetGauntletsProjectile | ProjectileClass::GarnetGauntletsProjectileAir => {
            ProjectileExt::GarnetGauntlets(Default::default())
        }
        ProjectileClass::GarnetMarker2 => ProjectileExt::GarnetMarker2(Default::default()),
        ProjectileClass::GarnetLightningBolt => ProjectileExt::GarnetBolt(Default::default()),
        ProjectileClass::LingeringLoveProjectile => ProjectileExt::StillActor(Default::default()),
        ProjectileClass::TunnelShockwave => ProjectileExt::BugsShockwave(Default::default()),
        ProjectileClass::BugsBunnyRocketFProjectile | ProjectileClass::BugsBunnyRocketUpProjectile => {
            ProjectileExt::Rocket(Default::default())
        }
        ProjectileClass::BugsBunnySafe | ProjectileClass::BugsBunnyNeutralSafe => {
            ProjectileExt::BugsBunnySafe(Default::default())
        }
        ProjectileClass::BugsBunnySafeDrop => ProjectileExt::SafeDrop(Default::default()),
        ProjectileClass::SelfActivateMeeseeksRun => ProjectileExt::MeeseeksRun(Default::default()),
        ProjectileClass::FartBombSpawnedActor => ProjectileExt::FartBomb(Default::default()),
        ProjectileClass::MeeseeksSpawnedActor => ProjectileExt::MeeseeksSummon(Default::default()),
        ProjectileClass::FinnTreasure => ProjectileExt::FinnTreasure(Default::default()),
        ProjectileClass::VelmaMarkProjectile => ProjectileExt::Mark(Default::default()),
        ProjectileClass::VelmaAimProjectile => ProjectileExt::AimProjectile(Default::default()),
        ProjectileClass::VelmamSassFollowUpProjectile => ProjectileExt::FollowUp(Default::default()),
        ProjectileClass::VelmaBookProjectile => ProjectileExt::VelmaBook(Default::default()),
        ProjectileClass::VelmaSPAIRCameraFlash | ProjectileClass::VelmaAirSideSpBookBlast => {
            ProjectileExt::VelmaBurst(Default::default())
        }
        ProjectileClass::VelmaSassBubblev2 => ProjectileExt::VelmaSassBubble(Default::default()),
        ProjectileClass::C023BSawBladeEdgeTrace
        | ProjectileClass::C023BSawBladeEdgeTraceStartOnGround
        | ProjectileClass::C023BSawBladeFromAllyEdgeTrace => ProjectileExt::Saw(Default::default()),
        ProjectileClass::C023BSpawnedGunShot => ProjectileExt::GunShot(Default::default()),
        ProjectileClass::BoomBoxSpawnedActor => ProjectileExt::BoomBox(Default::default()),
        ProjectileClass::JerryCork => ProjectileExt::JerryCork(Default::default()),
        ProjectileClass::DynamiteSpawnedActor | ProjectileClass::CantSplitDynamiteSpawnedActor => {
            ProjectileExt::Dynamite(Default::default())
        }
        ProjectileClass::FishingLure => ProjectileExt::Lure(Default::default()),
        ProjectileClass::TomAimRocket1 => ProjectileExt::AimRocket(Default::default()),
        ProjectileClass::JerryKickedRocket => ProjectileExt::KickedRocket(Default::default()),
        ProjectileClass::JerryRocket => ProjectileExt::JerryRocket(Default::default()),
        ProjectileClass::TomAndJerryTennisBall | ProjectileClass::TomAndJerryTennisBallReflected => {
            ProjectileExt::TennisBall(Default::default())
        }
        ProjectileClass::C018BubbleProj => ProjectileExt::BubbleProj(Default::default()),
        ProjectileClass::C018UpBlasterProj
        | ProjectileClass::C018UpBlasterProjAir
        | ProjectileClass::C018ComboBlasterProjVersion2 => ProjectileExt::Blaster(Default::default()),
        ProjectileClass::C018Flag | ProjectileClass::C018AirFlag => ProjectileExt::C018Flag(Default::default()),
        ProjectileClass::C023AArrowProjectile => ProjectileExt::Arrow(Default::default()),
        ProjectileClass::C023ACarSpawnedActor => ProjectileExt::GizmoCar(Default::default()),
        ProjectileClass::C023AMusicPickup => ProjectileExt::MusicPickup(Default::default()),
        ProjectileClass::C023AMusicProjectile => ProjectileExt::MusicProjectile(Default::default()),
        ProjectileClass::C023APianoNote => ProjectileExt::PianoNote(Default::default()),
        ProjectileClass::C023APopCorn => ProjectileExt::Popcorn(Default::default()),
        ProjectileClass::ParentCarSpawnedActor | ProjectileClass::MysteryMachineSpawnedCar => {
            ProjectileExt::ItemCar(Default::default())
        }
        ProjectileClass::BatMobileSpawnedActorChild => ProjectileExt::BatMobileCar(Default::default()),
        _ => ProjectileExt::Plain,
    };
    projectile_load(&mut p, r, l);
    Some(p)
}

// ------------------------------------------------------------------ items

pub fn item_put(i: &Item, x: &Wx) -> ActorRecord {
    ActorRecord {
        family: "items".to_owned(),
        id: f64::from(i.seq),
        class: x.str(i.cls).to_owned(),
        owner: fid_or(i.owner, x),
        holder: Some(f64::from(i.holder)),
        x: Some(f64::from(i.x)),
        y: Some(f64::from(i.y)),
        vx: Some(f64::from(i.vx)),
        vy: Some(f64::from(i.vy)),
        dead: i.dead,
        hit_actors: hit_actors_put(&i.hit_actors, x),
        rest: item_rest(i, x),
    }
}

/// An item gone from its list as the oracle writes the object whole (a Finn aimer's TeleportActor still holds it): the
/// record's keys and its `rest` in one object, its owner by reference, its hit set as a Set.
// The load: [`item_whole_load`].
pub fn item_whole(i: &Item, x: &Wx) -> Value {
    let r = item_put(i, x);
    let mut o: Vec<(String, Value)> = match r.rest {
        Value::Obj(ob) => ob.entries().to_vec(),
        _ => Vec::new(),
    };
    let owner = i.owner.map_or(Value::Null, |f| f.to_value(x));
    o.extend([
        ("cls".to_owned(), Value::Str(r.class)),
        ("dead".to_owned(), Value::Bool(r.dead)),
        ("id".to_owned(), Value::Num(r.id)),
        ("owner".to_owned(), owner),
        ("holder".to_owned(), Value::Num(r.holder.unwrap_or(-1.0))),
        ("hitActors".to_owned(), obj(vec![("set", Value::List(r.hit_actors))])),
    ]);
    for (k, v) in [("x", r.x), ("y", r.y), ("vx", r.vx), ("vy", r.vy)] {
        if let Some(v) = v {
            o.push((k.to_owned(), Value::Num(v)));
        }
    }
    Value::Obj(mvs_trace::Obj::new(o))
}

/// [`item_whole`]'s inverse: the gone item a Finn aimer names. It is made once per frame (by its id), kept in
/// `Lx::gone_items` for the World's `gone_items`, and named by an id of no live slot of the arena.
pub fn item_whole_load(o: &mvs_trace::Obj, l: &mut Lx) -> Option<mvs_sim::actors::ItemId> {
    let Some(Value::Num(id)) = o.get("id") else {
        l.miss("a gone item with no id");
        return None;
    };
    let seq = int(*id, l)?;
    if let Some((_, k, _)) = l.gone_items.iter().find(|(s, _, _)| *s == seq) {
        return Some(*k);
    }
    let Some(Value::Str(class)) = o.get("cls") else {
        l.miss("a gone item with no class");
        return None;
    };
    let num_of = |k: &str| match o.get(k) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    };
    let owner = match o.get("owner") {
        Some(Value::Obj(r)) => match r.get("id") {
            Some(Value::Num(n)) => *n,
            _ => -1.0,
        },
        _ => -1.0,
    };
    let hit_actors = match o.get("hitActors") {
        Some(Value::Obj(s)) => match s.get("set") {
            Some(Value::List(v)) => v.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    const OWN: [&str; 10] = ["cls", "dead", "id", "owner", "holder", "hitActors", "vx", "vy", "x", "y"];
    let rest = o.entries().iter().filter(|(k, _)| !OWN.contains(&k.as_str())).cloned().collect();
    let r = ActorRecord {
        family: "items".to_owned(),
        id: *id,
        class: class.clone(),
        owner,
        holder: num_of("holder"),
        x: num_of("x"),
        y: num_of("y"),
        vx: num_of("vx"),
        vy: num_of("vy"),
        dead: matches!(o.get("dead"), Some(Value::Bool(true))),
        hit_actors,
        rest: Value::Obj(mvs_trace::Obj::new(rest)),
    };
    let i = item_make(&r, l)?;
    // (an id of no slot: past the arena's, never handed out)
    let k = mvs_sim::actors::ItemId::new(u32::MAX - u32::try_from(l.gone_items.len()).ok()?, 0);
    l.gone_items.push((seq, k, i));
    Some(k)
}

fn item_rest(i: &Item, x: &Wx) -> Value {
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put i, x, o;
        "prevX" = prev_x, "prevY" = prev_y, "age" = age, "held" = held, "pooled" = pooled, "flying" = flying, "thrown" = thrown, "hitOn" = hit_on, "hitSet" = hit_set, "hit" = hit, "attackId" = attack_id, "scale" = scale, "spawner" = spawner, "dodge" ~ dodge, "charge" ~ charge, "elem" ~ elem, "lifetimePause" ~ lifetime_pause, "pitch" ^ pitch, "facing" = facing, "late" = late, "inputUi" = input_ui, "touching" = touching, "hasHitAFighter" ~ has_hit_a_fighter, "durability" = durability, "sphereOn" = sphere_on, "projFlag" ~ proj_flag, "pickR" ~ pick_r, "health" = health, "clanking" = clanking, "queueDestroy" = queue_destroy, "mobility" ~ mobility, "stationaryCount" ~ stationary_count, "reflect" ~ reflect, "noCollision" ~ no_collision, "doom" ~ doom, "buffHome" ~ buff_home, "overlapping" = overlapping, "overlappedBy" = overlapped_by, "bigGeo" = big_geo, "ammo" ~ ammo, "maxSpeed" = max_speed, "sizeScale" ~ size_scale, "lookupFighter" ~ lookup_fighter, "actAt" = act_at, "pawnAt" = pawn_at, "pawnOf" = pawn_of, "capR" = cap_r, "capH" = cap_h, "linDamp" = lin_damp, "lifeOff" = life_off, "unheldAt" = unheld_at, "gravScale" = grav_scale, "hitSphere" = hit_sphere, "hurtSphere" = hurt_sphere, "reach" = reach, "idleTicks" = idle_ticks, "blink" = blink, "blastAt" = blast_at, "blastBounds" = blast_bounds,
    );
    let p = match &i.ext {
        ItemExt::Batarang(b) => {
            batarang_put(b, x, &mut o);
            batarang_p(i.def(&x.w.data).p, b)
        }
        ItemExt::Fastball(s) => {
            crate::creature::fastball::put(s, x, &mut o);
            None
        }
        ItemExt::Anvil(s) => {
            crate::taz::perks::anvil_put(s, x, &mut o);
            None
        }
        ItemExt::Dynamite(s) => {
            crate::items::dynamite::dynamite_put(s, x, &mut o);
            None
        }
        ItemExt::HelperItem(s) => {
            crate::items::helper::helper_put(s, x, &mut o);
            None
        }
        ItemExt::MeeseekItem(s) => {
            crate::items::meeseeks::meeseek_put(s, x, &mut o);
            None
        }
        ItemExt::Basketball(s) => {
            crate::items::basketball::basketball_put(s, x, &mut o);
            None
        }
        ItemExt::Gem(s) => {
            crate::finn::gem::gem_put(s, x, &mut o);
            None
        }
        ItemExt::DroppedBomb(s) => {
            crate::stripe::bomb::bomb_put(s, x, &mut o);
            None
        }
        ItemExt::BouncePadItem(s) => {
            crate::items::bounce_pad::bounce_pad_item_put(s, x, &mut o);
            None
        }
        ItemExt::MysteryItem(s) => {
            crate::items::mystery::mystery_item_put(s, x, &mut o);
            None
        }
        ItemExt::ProximityMine(s) => {
            crate::items::proximity_mine::proximity_mine_put(s, x, &mut o);
            None
        }
        ItemExt::SafeItem(s) => {
            crate::items::safe::safe_item_put(s, x, &mut o);
            None
        }
        ItemExt::Snowball(s) => {
            crate::items::snowball::snowball_put(s, x, &mut o);
            None
        }
        _ => None,
    };
    o.push(("p".into(), p.unwrap_or_else(static_object)));
    Value::Obj(mvs_trace::Obj::new(o))
}

/// A batarang's `p` once its graph wrote its own variables (`setInstanceVars`: IsBouncerang at the spawn, the
/// StartReturn row), written whole as the oracle writes an instance's own object; `None`: the class's (static).
fn batarang_p(class: &Props, b: &BatarangState) -> Option<Value> {
    if !b.is_bouncerang && !b.weakened_rows {
        return None;
    }
    let mut p = class.clone();
    if b.is_bouncerang {
        p.insert("IsBouncerang".into(), DataValue::Bool(true));
    }
    if b.weakened_rows {
        let stacks = p.get("BouncerangWeakenedStacks").and_then(DataValue::as_f64).unwrap_or(0.0);
        let row: Props = [
            ("AppliedBuffs", DataValue::Str("Buff_Weakened_C".into())),
            ("ApplyBuffSelf?", DataValue::Bool(false)),
            ("ApplyBuffHit?", DataValue::Bool(true)),
            ("ApplyBuffAlly?", DataValue::Bool(false)),
            ("ApplyBuffEnemy?", DataValue::Bool(true)),
            ("RequireHitToApplyBuffToSelf", DataValue::Bool(false)),
            ("StackCount", DataValue::Num(stacks)),
            ("BuffDuration", DataValue::Num(0.0)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        let mut o = match p.get("On Hit Buffs") {
            Some(DataValue::Obj(o)) => o.clone(),
            _ => Props::new(),
        };
        o.insert("OnHitBuffs".into(), DataValue::Arr(vec![DataValue::Obj(row)]));
        p.insert("On Hit Buffs".into(), DataValue::Obj(o));
    }
    Some(data_put(&DataValue::Obj(p)))
}

/// A loaded batarang's own variables from its `p`: `{static}` is the class's.
fn batarang_p_load(b: &mut BatarangState, p: Option<&Value>) {
    let o = match p {
        Some(Value::Obj(o)) if o.get("static").is_none() => o,
        _ => {
            b.is_bouncerang = false;
            b.weakened_rows = false;
            return;
        }
    };
    b.is_bouncerang = matches!(o.get("IsBouncerang"), Some(Value::Bool(true)));
    b.weakened_rows = match o.get("On Hit Buffs") {
        Some(Value::Obj(h)) => match h.get("OnHitBuffs") {
            Some(Value::List(rows)) => rows.iter().any(|r| match r {
                Value::Obj(r) => matches!(r.get("AppliedBuffs"), Some(Value::Str(s)) if s == "Buff_Weakened_C"),
                _ => false,
            }),
            _ => false,
        },
        _ => false,
    };
}

/// The keys `Batarang` adds. `hitKind` is 0 outside its spend (the trace never sees
/// another value); IsBouncerang and the StartReturn rows are its `p` ([`batarang_p`]).
macro_rules! batarang_keys {
    ($mode:ident $($a:ident),*) => {
        plain_keys!($mode $($a),*;
            "team" = team, "creatorOv" ~ creator_ov, "canControl" = can_control, "canBeStopped" = can_be_stopped, "idle" = idle, "thrownAlready" = thrown_already, "bomb" = bomb, "looping" = looping, "homing" = homing, "homeMag" = home_mag, "initialSpeed" = initial_speed, "origScale" = orig_scale, "returnLeft" = return_left, "areaOn" = area_on, "retGen" = ret_gen, "inRet" = in_ret, "inArea" = in_area, "collected" = collected, "parentVx" = parent_vx, "parentVy" = parent_vy, "trail" = trail, "trailShown" = trail_shown, "pickupVfx" = pickup_vfx, "pickupAt" = pickup_at,
        )
    };
}

fn batarang_put(b: &BatarangState, x: &Wx, o: &mut Vec<(String, Value)>) {
    batarang_keys!(put b, x, o);
    o.push(("hitKind".into(), Value::Num(0.0)));
}

fn batarang_load(b: &mut BatarangState, ob: &mvs_trace::Obj, l: &mut Lx) {
    batarang_keys!(load b, ob, l);
    // (absent: never written by a reflect)
    if ob.get("creatorOv").is_none() {
        b.creator_ov = None;
    }
}

const BATARANG_KEYS: &[&str] = &[
    "team",
    "creatorOv",
    "canControl",
    "canBeStopped",
    "idle",
    "thrownAlready",
    "bomb",
    "looping",
    "homing",
    "homeMag",
    "initialSpeed",
    "origScale",
    "returnLeft",
    "areaOn",
    "retGen",
    "inRet",
    "inArea",
    "collected",
    "parentVx",
    "parentVy",
    "trail",
    "trailShown",
    "pickupVfx",
    "pickupAt",
    "hitKind",
];

pub(crate) fn opt_load(v: &mut mvs_sim::Real, r: Option<f64>, k: &str, l: &mut Lx) {
    match r {
        Some(r) => *v = mvs_sim::math::narrow(r),
        None => l.at(k, |l| l.miss("undefined (the Rust actor has it)")),
    }
}

pub fn item_load(i: &mut Item, r: &ActorRecord, l: &mut Lx) {
    if l.str(i.cls) != r.class {
        let m = format!("{} loaded into a {}", r.class, l.str(i.cls));
        l.at("class", |l| l.miss(m));
    }
    l.at("owner", |l| i.owner.load(&r.owner, l));
    match r.holder {
        Some(h) => l.at("holder", |l| i.holder.load(&h, l)),
        None => l.at("holder", |l| l.miss("undefined")),
    }
    opt_load(&mut i.x, r.x, "x", l);
    opt_load(&mut i.y, r.y, "y", l);
    opt_load(&mut i.vx, r.vx, "vx", l);
    opt_load(&mut i.vy, r.vy, "vy", l);
    i.dead = r.dead;
    l.at("hit_actors", |l| hit_actors_load(&mut i.hit_actors, &r.hit_actors, l));
    l.at("rest", |l| {
        let Some(ob) = as_obj(&r.rest, l) else { return };
        plain_keys!(load i, ob, l;
            "prevX" = prev_x, "prevY" = prev_y, "age" = age, "held" = held, "pooled" = pooled, "flying" = flying, "thrown" = thrown, "hitOn" = hit_on, "hitSet" = hit_set, "hit" = hit, "attackId" = attack_id, "scale" = scale, "spawner" = spawner, "dodge" ~ dodge, "charge" ~ charge, "elem" ~ elem, "lifetimePause" ~ lifetime_pause, "pitch" ^ pitch, "facing" = facing, "late" = late, "inputUi" = input_ui, "touching" = touching, "hasHitAFighter" ~ has_hit_a_fighter, "durability" = durability, "sphereOn" = sphere_on, "projFlag" ~ proj_flag, "pickR" ~ pick_r, "health" = health, "clanking" = clanking, "queueDestroy" = queue_destroy, "mobility" ~ mobility, "stationaryCount" ~ stationary_count, "reflect" ~ reflect, "noCollision" ~ no_collision, "doom" ~ doom, "buffHome" ~ buff_home, "overlapping" = overlapping, "overlappedBy" = overlapped_by, "bigGeo" = big_geo, "ammo" ~ ammo, "maxSpeed" = max_speed, "sizeScale" ~ size_scale, "lookupFighter" ~ lookup_fighter, "actAt" = act_at, "pawnAt" = pawn_at, "pawnOf" = pawn_of, "capR" = cap_r, "capH" = cap_h, "linDamp" = lin_damp, "lifeOff" = life_off, "unheldAt" = unheld_at, "gravScale" = grav_scale, "hitSphere" = hit_sphere, "hurtSphere" = hurt_sphere, "reach" = reach, "idleTicks" = idle_ticks, "blink" = blink, "blastAt" = blast_at, "blastBounds" = blast_bounds,
        );
        let mut known: Vec<&str> = plain_keys!(names
            "prevX" = prev_x, "prevY" = prev_y, "age" = age, "held" = held, "pooled" = pooled, "flying" = flying, "thrown" = thrown, "hitOn" = hit_on, "hitSet" = hit_set, "hit" = hit, "attackId" = attack_id, "scale" = scale, "spawner" = spawner, "dodge" ~ dodge, "charge" ~ charge, "elem" ~ elem, "lifetimePause" ~ lifetime_pause, "pitch" ^ pitch, "facing" = facing, "late" = late, "inputUi" = input_ui, "touching" = touching, "hasHitAFighter" ~ has_hit_a_fighter, "durability" = durability, "sphereOn" = sphere_on, "projFlag" ~ proj_flag, "pickR" ~ pick_r, "health" = health, "clanking" = clanking, "queueDestroy" = queue_destroy, "mobility" ~ mobility, "stationaryCount" ~ stationary_count, "reflect" ~ reflect, "noCollision" ~ no_collision, "doom" ~ doom, "buffHome" ~ buff_home, "overlapping" = overlapping, "overlappedBy" = overlapped_by, "bigGeo" = big_geo, "ammo" ~ ammo, "maxSpeed" = max_speed, "sizeScale" ~ size_scale, "lookupFighter" ~ lookup_fighter, "actAt" = act_at, "pawnAt" = pawn_at, "pawnOf" = pawn_of, "capR" = cap_r, "capH" = cap_h, "linDamp" = lin_damp, "lifeOff" = life_off, "unheldAt" = unheld_at, "gravScale" = grav_scale, "hitSphere" = hit_sphere, "hurtSphere" = hurt_sphere, "reach" = reach, "idleTicks" = idle_ticks, "blink" = blink, "blastAt" = blast_at, "blastBounds" = blast_bounds,
        )
        .to_vec();
        known.push("p");
        if let ItemExt::Batarang(b) = &mut i.ext {
            batarang_load(b, ob, l);
            batarang_p_load(b, ob.get("p"));
            known.extend(BATARANG_KEYS);
        }
        if let ItemExt::Fastball(s) = &mut i.ext {
            crate::creature::fastball::load(s, ob, l, &mut known);
        }
        if let ItemExt::Anvil(s) = &mut i.ext {
            crate::taz::perks::anvil_load(s, ob, l, &mut known);
        }
        if let ItemExt::Dynamite(s) = &mut i.ext {
            crate::items::dynamite::dynamite_load(s, ob, l, &mut known);
        }
        if let ItemExt::HelperItem(s) = &mut i.ext {
            crate::items::helper::helper_load(s, ob, l, &mut known);
        }
        if let ItemExt::MeeseekItem(s) = &mut i.ext {
            crate::items::meeseeks::meeseek_load(s, ob, l, &mut known);
        }
        if let ItemExt::Basketball(s) = &mut i.ext {
            crate::items::basketball::basketball_load(s, ob, l, &mut known);
        }
        if let ItemExt::Gem(s) = &mut i.ext {
            crate::finn::gem::gem_load(s, ob, l, &mut known);
        }
        if let ItemExt::DroppedBomb(s) = &mut i.ext {
            crate::stripe::bomb::bomb_load(s, ob, l, &mut known);
        }
        if let ItemExt::BouncePadItem(s) = &mut i.ext {
            crate::items::bounce_pad::bounce_pad_item_load(s, ob, l, &mut known);
        }
        if let ItemExt::MysteryItem(s) = &mut i.ext {
            crate::items::mystery::mystery_item_load(s, ob, l, &mut known);
        }
        if let ItemExt::ProximityMine(s) = &mut i.ext {
            crate::items::proximity_mine::proximity_mine_load(s, ob, l, &mut known);
        }
        if let ItemExt::SafeItem(s) = &mut i.ext {
            crate::items::safe::safe_item_load(s, ob, l, &mut known);
        }
        if let ItemExt::Snowball(s) = &mut i.ext {
            crate::items::snowball::snowball_load(s, ob, l, &mut known);
        }
        check_keys(ob, &known, l);
    });
}

fn item_make(r: &ActorRecord, l: &mut Lx) -> Option<Item> {
    let Some(class) = ItemClass::ALL.iter().copied().find(|c| c.name() == r.class) else {
        l.miss(format_args!("no item class {}", r.class));
        return None;
    };
    let rest = as_obj(&r.rest, l)?;
    let get = |k: &str| match rest.get(k) {
        Some(Value::Num(n)) => *n,
        _ => 0.0,
    };
    let late = matches!(rest.get("late"), Some(Value::Bool(true)));
    let owner = l.fighter(r.owner);
    let mut i = Item::new(
        l.name(&r.class),
        class,
        owner,
        crate::conv::deg_of(get("pitch")),
        mvs_sim::math::narrow(get("facing")),
        mvs_sim::math::narrow(r.vx?),
        mvs_sim::math::narrow(r.vy?),
        late,
    );
    i.seq = int(r.id, l)?;
    // the class's own variables (its subclass)
    if let Some(sub) = mvs_sim::actors::item::item_hooks(mvs_sim::registry::registry(), class).and_then(|h| h.sub) {
        i.ext = sub();
    }
    item_load(&mut i, r, l);
    Some(i)
}

// ------------------------------------------------------------------ the other actors

pub fn actor_put(a: &WorldActor, x: &Wx) -> ActorRecord {
    let mut r = ActorRecord {
        family: "actors".to_owned(),
        id: f64::from(a.seq),
        class: x.str(a.cls).to_owned(),
        owner: f64::from(x.fid(a.owner)),
        dead: a.dead,
        ..ActorRecord::default()
    };
    let mut o: Vec<(String, Value)> = Vec::new();
    plain_keys!(put a, x, o;
        "charge" ~ charge, "elem" ~ elem, "lifetimePause" ~ lifetime_pause, "sizeScale" ~ size_scale, "dodge" ~ dodge, "doom" ~ doom, "mobility" ~ mobility, "stationaryCount" ~ stationary_count,
    );
    match &a.state {
        WorldActorState::Shockwave(s) => {
            r.x = Some(f64::from(s.x));
            r.y = Some(f64::from(s.y));
            // (ShaggyShockwaveActor's vx / vy: readonly 0)
            r.vx = Some(0.0);
            r.vy = Some(0.0);
            plain_keys!(put s, x, o;
                "attackId" = attack_id, "facing" = facing, "hit" = hit, "placed" = placed, "height" = height, "stage" = stage, "wait" = wait, "then" = then, "decalAlpha" = decal_alpha, "decalT" = decal_t, "ground" = ground,
            );
            o.push(("spheres".into(), spheres_put(&s.spheres)));
            // (the stage's pass-through platforms the shockwave's trace is blocked by: the World's)
            o.push(("platforms".into(), x.w.stage.stage_platforms.to_value(x)));
            o.push(("p".into(), static_object()));
        }
        WorldActorState::SmokeGrenade(s) => {
            r.x = Some(f64::from(s.x));
            r.y = Some(f64::from(s.y));
            r.vx = Some(f64::from(s.vx));
            r.vy = Some(f64::from(s.vy));
            plain_keys!(put s, x, o; "angle" = angle, "attackId" = attack_id, "blastBounds" = blast_bounds, "left" = left, "pvx" = pvx, "pvy" = pvy, "radius" = radius, "spin" = spin, "wa" = wa, "wb" = wb, "wc" = wc,);
            o.push(("p".into(), static_object()));
        }
        WorldActorState::SmokeZone(s) => {
            r.x = Some(f64::from(s.x));
            r.y = Some(f64::from(s.y));
            plain_keys!(put s, x, o; "activate" = activate, "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds, "buffed" = buffed, "destroy" = destroy, "fogActivated" = fog_activated, "fogEnd" = fog_end, "inside" = inside, "looping" = looping, "radius" = radius, "visuals" = visuals, "chimney" ~ chimney, "blockedDestroy" ~ blocked_destroy,);
            o.push(("p".into(), static_object()));
        }
        WorldActorState::FlightActor(s) => crate::superman::flight::put(s, a.owner, x, &mut r, &mut o),
        WorldActorState::LaserSweep(s) => crate::superman::laser::sweep_put(s, x, &mut r, &mut o),
        WorldActorState::LaserDamage(s) => crate::superman::laser::damage_put(s, x, &mut r, &mut o),
        WorldActorState::IceStream(s) => crate::superman::ice::stream_put(s, x, &mut r, &mut o),
        WorldActorState::AimMarker(s) => crate::superman::aim_punch::put(s, x, &mut r, &mut o),
        WorldActorState::DashToBasketball(s) => crate::lebron::later::put(s, x, &mut r, &mut o),
        WorldActorState::DisguiseShock(s) => crate::arya::actors::shock_put(s, x, &mut r, &mut o),
        WorldActorState::KillPie(s) => crate::arya::actors::pie_put(s, x, &mut r, &mut o),
        WorldActorState::Bolt(s) => crate::black_adam::bolt::bolt_put(s, x, &mut r, &mut o),
        WorldActorState::SmashIn(s) => crate::black_adam::bolt::smash_put(s, x, &mut r, &mut o),
        WorldActorState::ForceField(s) => crate::black_adam::field::field_put(s, x, &mut r, &mut o),
        WorldActorState::PerkForceField(s) => crate::black_adam::field::perk_put(s, x, &mut r, &mut o),
        WorldActorState::GrabHit(s) => crate::black_adam::grab_hit::put(s, a.owner, x, &mut r, &mut o),
        WorldActorState::ProjectileBarrier(s) => crate::barrier::put(s, x, &mut r, &mut o),
        WorldActorState::ShieldActor(s) => crate::shield_actor::shield_put(a, s, x, &mut r, &mut o),
        WorldActorState::EvadeShield(s) => crate::wonder_woman::perks::evade_put(a, s, x, &mut r, &mut o),
        WorldActorState::BracerShield(s) => crate::wonder_woman::actors::bracer_put(s, x, &mut r, &mut o),
        WorldActorState::ScrubShield(s) => crate::morty::scrub::scrub_put(s, x, &mut r, &mut o),
        WorldActorState::HarleyBomb(s) => crate::harley::bomb::bomb_put(s, x, &mut r, &mut o),
        WorldActorState::JackBox(s) => crate::harley::actors::jack_put(s, x, &mut r, &mut o),
        WorldActorState::SnackBox(s) => crate::shaggy::snack_put(s, x, &mut r, &mut o),
        WorldActorState::AntenaBolts(s) => crate::iron_giant::actor_state::antena_put(s, x, &mut r, &mut o),
        WorldActorState::CannonBall(s) => crate::iron_giant::actor_state::ball_put(s, a, x, &mut r, &mut o),
        WorldActorState::TurretAim(s) => crate::iron_giant::actor_state::turret_aim_put(s, x, &mut r, &mut o),
        WorldActorState::StompWave(s) => crate::iron_giant::actor_state::stomp_put(s, x, &mut r, &mut o),
        WorldActorState::DownThruster(s) => crate::iron_giant::actor_state::thruster_put(s, x, &mut r, &mut o),
        WorldActorState::CannonAim(s) => crate::iron_giant::cannon_aim::cannon_aim_put(s, x, &mut r, &mut o),
        WorldActorState::DiscAim(s) => crate::iron_giant::cannon_aim::disc_aim_put(s, a.owner, x, &mut r, &mut o),
        WorldActorState::PlasmaAim(s) => crate::iron_giant::cannon_aim::plasma_aim_put(s, a.owner, x, &mut r, &mut o),
        WorldActorState::CannonBurst(s) => crate::iron_giant::cannon_aim::burst_put(s, x, &mut r, &mut o),
        WorldActorState::PilotReticle(s) => crate::iron_giant::turret_actors::reticle_put(s, x, &mut r, &mut o),
        WorldActorState::TurretBomb(s) => crate::iron_giant::turret_actors::bomb_put(s, x, &mut r, &mut o),
        WorldActorState::WallPlatform(s) => crate::iron_giant::wall::wall_put(a, s, x, &mut r, &mut o),
        WorldActorState::Dogpile(s) => crate::taz::dogpile::dogpile_put(s, x, &mut r, &mut o),
        WorldActorState::StevenShield(s) => crate::steven::barrier::shield_put(a, s, x, &mut r, &mut o),
        WorldActorState::StevenBlock(s) => crate::steven::shield::block_put(a, s, x, &mut r, &mut o),
        WorldActorState::HealZone(s) => crate::steven::watermelon::heal_zone_put(s, x, &mut r, &mut o),
        WorldActorState::ShockwaveHitBox(s) => crate::garnet::shockwave::hit_box_put(s, x, &mut r, &mut o),
        WorldActorState::ElectricZone(s) => crate::garnet::zone::zone_put(s, x, &mut r, &mut o),
        WorldActorState::Tunnel(s) => crate::bugs::tunnel::tunnel_put(s, x, &mut r, &mut o),
        WorldActorState::Dispenser(s) => crate::bugs::dispenser::dispenser_put(s, x, &mut r, &mut o),
        WorldActorState::Nepter(s) => crate::items::nepter::nepter_put(s, x, &mut r, &mut o),
        WorldActorState::BananaGuard(s) => crate::items::banana_guard::banana_guard_put(s, x, &mut r, &mut o),
        WorldActorState::BouncePad(s) => crate::items::bounce_pad::bounce_pad_put(s, x, &mut r, &mut o),
        WorldActorState::PortalActor(s) => crate::rick::portal::portal_put(s, x, &mut r, &mut o),
        WorldActorState::PortalAimer(s) => crate::rick::aimer::aimer_put(s, x, &mut r, &mut o),
        WorldActorState::AirFlap(s) => crate::rick::air_flap::air_flap_put(s, x, &mut r, &mut o),
        WorldActorState::PolymorphGrenade(s) => crate::rick::grenade::grenade_put(s, x, &mut r, &mut o),
        WorldActorState::FinnAimer(s) => crate::finn::aimer::aimer_put(s, x, &mut r, &mut o),
        WorldActorState::GemBoostBlast(s) => crate::finn::perks::blast_put(s, x, &mut r, &mut o),
        WorldActorState::FinnShop(s) => crate::finn::shop::shop_put(s, x, &mut r, &mut o),
        WorldActorState::BmoActor(s) => crate::finn::bmo::bmo_put(s, x, &mut r, &mut o),
        WorldActorState::FinnListener(s) => crate::finn::high_five::listener_put(s, x, &mut r, &mut o),
        WorldActorState::HighFiveShockwave(s) => crate::finn::high_five::shockwave_put(s, x, &mut r, &mut o),
        WorldActorState::FinnShockwave(s) => crate::finn::shockwave::shockwave_put(s, x, &mut r, &mut o),
        WorldActorState::Beam(s) => crate::velma::beam::beam_put(s, x, &mut r, &mut o),
        WorldActorState::VelmaCarryRun(s) => crate::velma::carry::run_put(s, x, &mut r, &mut o),
        WorldActorState::Evidence(s) => crate::velma::evidence::evidence_put(s, x, &mut r, &mut o),
        WorldActorState::SlowToStunGrenade(s) => crate::velma::gadgets::grenade_put(s, x, &mut r, &mut o),
        WorldActorState::VelmaVial(s) => crate::velma::gadgets::vial_put(s, x, &mut r, &mut o),
        WorldActorState::JenkinsPin(s) => crate::velma::police::pin_put(s, x, &mut r, &mut o),
        WorldActorState::AllySaw(s) => crate::stripe::saw::ally_saw_put(s, x, &mut r, &mut o),
        WorldActorState::StripeListener(s) => crate::stripe::dash::listener_put(s, x, &mut r, &mut o),
        WorldActorState::JakeAimPunchEatListener(s) => crate::jake::aim::listener_put(s, x, &mut r, &mut o),
        WorldActorState::JakeAimer(s) => crate::jake::aim::aimer_put(s, x, &mut r, &mut o),
        WorldActorState::JakeEatAllyAim(s) => crate::jake::ally_spit::aim_put(s, x, &mut r, &mut o),
        WorldActorState::JakeSpitAllyHitBox(s) => crate::jake::ally_spit::hit_box_put(s, x, &mut r, &mut o),
        WorldActorState::JakeHorse(s) => crate::jake::horse::horse_put(s, x, &mut r, &mut o),
        WorldActorState::JakeHorsePlatform(s) => crate::jake::horse::platform_put(a, s, x, &mut r, &mut o),
        WorldActorState::JakeHouse(s) => crate::jake::house::house_put(s, x, &mut r, &mut o),
        WorldActorState::SpitAimer(s) => crate::jake::spit::aimer_put(s, x, &mut r, &mut o),
        WorldActorState::SpitHitBox(s) => crate::jake::spit::hit_box_put(s, x, &mut r, &mut o),
        WorldActorState::Stretch(s) => crate::jake::stretch::stretch_put(s, x, &mut r, &mut o),
        WorldActorState::JerryActor(s) => crate::tom_and_jerry::jerry::jerry_put(s, x, &mut r, &mut o),
        WorldActorState::SlingAim(s) => crate::tom_and_jerry::slingshot::sling_aim_put(s, x, &mut r, &mut o),
        WorldActorState::NoJerryAim(s) => crate::tom_and_jerry::tennis::no_jerry_aim_put(s, x, &mut r, &mut o),
        WorldActorState::MouseTrap(s) => crate::tom_and_jerry::trap::trap_put(s, x, &mut r, &mut o),
        WorldActorState::Ray(s) => crate::marvin::ray::ray_put(s, x, &mut r, &mut o),
        WorldActorState::RocketExplosion(s) => crate::marvin::ufo::rocket_explosion_put(s, x, &mut r, &mut o),
        WorldActorState::Ufo(s) => crate::marvin::ufo::ufo_put(a, s, x, &mut r, &mut o),
    }
    r.rest = Value::Obj(mvs_trace::Obj::new(o));
    r
}

/// `spheres[]`: the shockwave's hit spheres, `{fwd, up, r}` (Rust `[forward, up, radius]`).
pub(crate) fn spheres_put(s: &[[mvs_sim::Real; 3]]) -> Value {
    Value::List(
        s.iter()
            .map(|&[fwd, up, r]| {
                obj(vec![
                    ("fwd", Value::Num(f64::from(fwd))),
                    ("r", Value::Num(f64::from(r))),
                    ("up", Value::Num(f64::from(up))),
                ])
            })
            .collect(),
    )
}

pub(crate) fn spheres_load(s: &mut Vec<[mvs_sim::Real; 3]>, v: &Value, l: &mut Lx) {
    let Value::List(a) = v else {
        l.miss(format_args!("{v:?} is not a list"));
        return;
    };
    s.clear();
    for e in a {
        let Some(o) = as_obj(e, l) else { return };
        let g = |k: &str| match o.get(k) {
            Some(Value::Num(n)) => Some(*n),
            _ => None,
        };
        match (g("fwd"), g("up"), g("r")) {
            (Some(f), Some(u), Some(r)) => s.push([f, u, r].map(mvs_sim::math::narrow)),
            _ => l.miss("a sphere that is not {fwd, up, r}"),
        }
    }
}

pub fn actor_load(a: &mut WorldActor, r: &ActorRecord, l: &mut Lx) {
    if l.str(a.cls) != r.class {
        let m = format!("{} loaded into a {}", r.class, l.str(a.cls));
        l.at("class", |l| l.miss(m));
    }
    l.at("owner", |l| match l.fighter(r.owner) {
        Some(f) => a.owner = f,
        None => l.miss("no owner (a Rust world actor has one)"),
    });
    a.dead = r.dead;
    // (a BananaGuard's own set: its load reads it)
    if !r.hit_actors.is_empty() && !matches!(a.state, WorldActorState::BananaGuard(_)) {
        l.at("hit_actors", |l| l.miss("hit actors (a Rust world actor holds none but a BananaGuard)"));
    }
    let Some(ob) = l.at("rest", |l| as_obj(&r.rest, l).cloned()) else { return };
    l.at("rest", |l| {
        plain_keys!(load a, ob, l;
            "charge" ~ charge, "elem" ~ elem, "lifetimePause" ~ lifetime_pause, "sizeScale" ~ size_scale, "dodge" ~ dodge, "doom" ~ doom, "mobility" ~ mobility, "stationaryCount" ~ stationary_count,
        );
    });
    let mut known: Vec<&str> =
        vec!["charge", "elem", "lifetimePause", "sizeScale", "dodge", "doom", "mobility", "stationaryCount", "p"];
    match &mut a.state {
        WorldActorState::Shockwave(s) => {
            opt_load(&mut s.x, r.x, "x", l);
            opt_load(&mut s.y, r.y, "y", l);
            l.at("rest", |l| {
                plain_keys!(load s, ob, l;
                    "attackId" = attack_id, "facing" = facing, "hit" = hit, "placed" = placed, "height" = height, "stage" = stage, "wait" = wait, "then" = then, "decalAlpha" = decal_alpha, "decalT" = decal_t, "ground" = ground,
                );
                match ob.get("spheres") {
                    Some(v) => l.at("spheres", |l| spheres_load(&mut s.spheres, v, l)),
                    None => l.miss("key spheres absent"),
                }
            });
            known.extend(plain_keys!(names
                "attackId" = attack_id, "facing" = facing, "hit" = hit, "placed" = placed, "height" = height, "stage" = stage, "wait" = wait, "then" = then, "decalAlpha" = decal_alpha, "decalT" = decal_t, "ground" = ground,
            ));
            known.extend(["spheres", "platforms"]);
        }
        WorldActorState::SmokeGrenade(s) => {
            opt_load(&mut s.x, r.x, "x", l);
            opt_load(&mut s.y, r.y, "y", l);
            opt_load(&mut s.vx, r.vx, "vx", l);
            opt_load(&mut s.vy, r.vy, "vy", l);
            l.at("rest", |l| {
                plain_keys!(load s, ob, l; "angle" = angle, "attackId" = attack_id, "blastBounds" = blast_bounds, "left" = left, "pvx" = pvx, "pvy" = pvy, "radius" = radius, "spin" = spin, "wa" = wa, "wb" = wb, "wc" = wc,);
            });
            known.extend(plain_keys!(names "angle" = angle, "attackId" = attack_id, "blastBounds" = blast_bounds, "left" = left, "pvx" = pvx, "pvy" = pvy, "radius" = radius, "spin" = spin, "wa" = wa, "wb" = wb, "wc" = wc,));
        }
        WorldActorState::SmokeZone(s) => {
            opt_load(&mut s.x, r.x, "x", l);
            opt_load(&mut s.y, r.y, "y", l);
            l.at("rest", |l| {
                plain_keys!(load s, ob, l; "activate" = activate, "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds, "buffed" = buffed, "destroy" = destroy, "fogActivated" = fog_activated, "fogEnd" = fog_end, "inside" = inside, "looping" = looping, "radius" = radius, "visuals" = visuals, "chimney" ~ chimney, "blockedDestroy" ~ blocked_destroy,);
            });
            known.extend(plain_keys!(names "activate" = activate, "age" = age, "attackId" = attack_id, "blastAt" = blast_at, "blastBounds" = blast_bounds, "buffed" = buffed, "destroy" = destroy, "fogActivated" = fog_activated, "fogEnd" = fog_end, "inside" = inside, "looping" = looping, "radius" = radius, "visuals" = visuals, "chimney" ~ chimney, "blockedDestroy" ~ blocked_destroy,));
        }
        WorldActorState::FlightActor(s) => crate::superman::flight::load(s, r, &ob, l, &mut known),
        WorldActorState::LaserSweep(s) => crate::superman::laser::sweep_load(s, r, &ob, l, &mut known),
        WorldActorState::LaserDamage(s) => crate::superman::laser::damage_load(s, r, &ob, l, &mut known),
        WorldActorState::IceStream(s) => crate::superman::ice::stream_load(s, r, &ob, l, &mut known),
        WorldActorState::AimMarker(s) => crate::superman::aim_punch::load(s, r, &ob, l, &mut known),
        WorldActorState::DashToBasketball(s) => crate::lebron::later::load(s, r, &ob, l, &mut known),
        WorldActorState::DisguiseShock(s) => crate::arya::actors::shock_load(s, r, &ob, l, &mut known),
        WorldActorState::KillPie(s) => crate::arya::actors::pie_load(s, r, &ob, l, &mut known),
        WorldActorState::Bolt(s) => crate::black_adam::bolt::bolt_load(s, r, &ob, l, &mut known),
        WorldActorState::SmashIn(s) => crate::black_adam::bolt::smash_load(s, r, &ob, l, &mut known),
        WorldActorState::ForceField(s) => crate::black_adam::field::field_load(s, r, &ob, l, &mut known),
        WorldActorState::PerkForceField(s) => crate::black_adam::field::perk_load(s, r, &ob, l, &mut known),
        WorldActorState::GrabHit(s) => crate::black_adam::grab_hit::load(s, r, &ob, l, &mut known),
        WorldActorState::ProjectileBarrier(s) => crate::barrier::load(s, r, &ob, l, &mut known),
        WorldActorState::ShieldActor(s) => crate::shield_actor::shield_load(s, &mut a.plat, r, &ob, l, &mut known),
        WorldActorState::EvadeShield(s) => {
            crate::wonder_woman::perks::evade_load(s, &mut a.plat, r, &ob, l, &mut known)
        }
        WorldActorState::BracerShield(s) => crate::wonder_woman::actors::bracer_load(s, r, &ob, l, &mut known),
        WorldActorState::ScrubShield(s) => crate::morty::scrub::scrub_load(s, r, &ob, l, &mut known),
        WorldActorState::HarleyBomb(s) => crate::harley::bomb::bomb_load(s, r, &ob, l, &mut known),
        WorldActorState::JackBox(s) => crate::harley::actors::jack_load(s, r, &ob, l, &mut known),
        WorldActorState::SnackBox(s) => crate::shaggy::snack_load(s, r, &ob, l, &mut known),
        WorldActorState::AntenaBolts(s) => crate::iron_giant::actor_state::antena_load(s, r, &ob, l, &mut known),
        WorldActorState::CannonBall(s) => crate::iron_giant::actor_state::ball_load(s, r, &ob, l, &mut known),
        WorldActorState::TurretAim(s) => crate::iron_giant::actor_state::turret_aim_load(s, r, &ob, l, &mut known),
        WorldActorState::StompWave(s) => crate::iron_giant::actor_state::stomp_load(s, r, &ob, l, &mut known),
        WorldActorState::DownThruster(s) => crate::iron_giant::actor_state::thruster_load(s, r, &ob, l, &mut known),
        WorldActorState::CannonAim(s) => crate::iron_giant::cannon_aim::cannon_aim_load(s, r, &ob, l, &mut known),
        WorldActorState::DiscAim(s) => crate::iron_giant::cannon_aim::disc_aim_load(s, r, &ob, l, &mut known),
        WorldActorState::PlasmaAim(s) => crate::iron_giant::cannon_aim::plasma_aim_load(s, r, &ob, l, &mut known),
        WorldActorState::CannonBurst(s) => crate::iron_giant::cannon_aim::burst_load(s, r, &ob, l, &mut known),
        WorldActorState::PilotReticle(s) => crate::iron_giant::turret_actors::reticle_load(s, r, &ob, l, &mut known),
        WorldActorState::TurretBomb(s) => crate::iron_giant::turret_actors::bomb_load(s, r, &ob, l, &mut known),
        WorldActorState::WallPlatform(s) => crate::iron_giant::wall::wall_load(s, &mut a.plat, r, &ob, l, &mut known),
        WorldActorState::Dogpile(s) => crate::taz::dogpile::dogpile_load(s, r, &ob, l, &mut known),
        WorldActorState::StevenShield(s) => crate::steven::barrier::shield_load(s, &mut a.plat, r, &ob, l, &mut known),
        WorldActorState::StevenBlock(s) => crate::steven::shield::block_load(s, &mut a.plat, r, &ob, l, &mut known),
        WorldActorState::HealZone(s) => crate::steven::watermelon::heal_zone_load(s, r, &ob, l, &mut known),
        WorldActorState::ShockwaveHitBox(s) => crate::garnet::shockwave::hit_box_load(s, r, &ob, l, &mut known),
        WorldActorState::ElectricZone(s) => crate::garnet::zone::zone_load(s, r, &ob, l, &mut known),
        WorldActorState::Tunnel(s) => crate::bugs::tunnel::tunnel_load(s, r, &ob, l, &mut known),
        WorldActorState::Dispenser(s) => crate::bugs::dispenser::dispenser_load(s, r, &ob, l, &mut known),
        WorldActorState::Nepter(s) => crate::items::nepter::nepter_load(s, r, &ob, l, &mut known),
        WorldActorState::BananaGuard(s) => crate::items::banana_guard::banana_guard_load(s, r, &ob, l, &mut known),
        WorldActorState::BouncePad(s) => crate::items::bounce_pad::bounce_pad_load(s, r, &ob, l, &mut known),
        WorldActorState::PortalActor(s) => crate::rick::portal::portal_load(s, r, &ob, l, &mut known),
        WorldActorState::PortalAimer(s) => crate::rick::aimer::aimer_load(s, r, &ob, l, &mut known),
        WorldActorState::AirFlap(s) => crate::rick::air_flap::air_flap_load(s, r, &ob, l, &mut known),
        WorldActorState::PolymorphGrenade(s) => crate::rick::grenade::grenade_load(s, r, &ob, l, &mut known),
        WorldActorState::FinnAimer(s) => crate::finn::aimer::aimer_load(s, r, &ob, l, &mut known),
        WorldActorState::GemBoostBlast(s) => crate::finn::perks::blast_load(s, r, &ob, l, &mut known),
        WorldActorState::FinnShop(s) => crate::finn::shop::shop_load(s, r, &ob, l, &mut known),
        WorldActorState::BmoActor(s) => crate::finn::bmo::bmo_load(s, r, &ob, l, &mut known),
        WorldActorState::FinnListener(s) => crate::finn::high_five::listener_load(s, r, &ob, l, &mut known),
        WorldActorState::HighFiveShockwave(s) => crate::finn::high_five::shockwave_load(s, r, &ob, l, &mut known),
        WorldActorState::FinnShockwave(s) => crate::finn::shockwave::shockwave_load(s, r, &ob, l, &mut known),
        WorldActorState::Beam(s) => crate::velma::beam::beam_load(s, r, &ob, l, &mut known),
        WorldActorState::VelmaCarryRun(s) => crate::velma::carry::run_load(s, r, &ob, l, &mut known),
        WorldActorState::Evidence(s) => crate::velma::evidence::evidence_load(s, r, &ob, l, &mut known),
        WorldActorState::SlowToStunGrenade(s) => crate::velma::gadgets::grenade_load(s, r, &ob, l, &mut known),
        WorldActorState::VelmaVial(s) => crate::velma::gadgets::vial_load(s, r, &ob, l, &mut known),
        WorldActorState::JenkinsPin(s) => crate::velma::police::pin_load(s, r, &ob, l, &mut known),
        WorldActorState::AllySaw(s) => crate::stripe::saw::ally_saw_load(s, r, &ob, l, &mut known),
        WorldActorState::StripeListener(s) => crate::stripe::dash::listener_load(s, r, &ob, l, &mut known),
        WorldActorState::JakeAimPunchEatListener(s) => crate::jake::aim::listener_load(s, r, &ob, l, &mut known),
        WorldActorState::JakeAimer(s) => crate::jake::aim::aimer_load(s, r, &ob, l, &mut known),
        WorldActorState::JakeEatAllyAim(s) => crate::jake::ally_spit::aim_load(s, r, &ob, l, &mut known),
        WorldActorState::JakeSpitAllyHitBox(s) => crate::jake::ally_spit::hit_box_load(s, r, &ob, l, &mut known),
        WorldActorState::JakeHorse(s) => crate::jake::horse::horse_load(s, r, &ob, l, &mut known),
        WorldActorState::JakeHorsePlatform(s) => {
            crate::jake::horse::platform_load(s, &mut a.plat, r, &ob, l, &mut known)
        }
        WorldActorState::JakeHouse(s) => crate::jake::house::house_load(s, r, &ob, l, &mut known),
        WorldActorState::SpitAimer(s) => crate::jake::spit::aimer_load(s, r, &ob, l, &mut known),
        WorldActorState::SpitHitBox(s) => crate::jake::spit::hit_box_load(s, r, &ob, l, &mut known),
        WorldActorState::Stretch(s) => crate::jake::stretch::stretch_load(s, r, &ob, l, &mut known),
        WorldActorState::JerryActor(s) => crate::tom_and_jerry::jerry::jerry_load(s, r, &ob, l, &mut known),
        WorldActorState::SlingAim(s) => crate::tom_and_jerry::slingshot::sling_aim_load(s, r, &ob, l, &mut known),
        WorldActorState::NoJerryAim(s) => crate::tom_and_jerry::tennis::no_jerry_aim_load(s, r, &ob, l, &mut known),
        WorldActorState::MouseTrap(s) => crate::tom_and_jerry::trap::trap_load(s, r, &ob, l, &mut known),
        WorldActorState::Ray(s) => crate::marvin::ray::ray_load(s, r, &ob, l, &mut known),
        WorldActorState::RocketExplosion(s) => crate::marvin::ufo::rocket_explosion_load(s, r, &ob, l, &mut known),
        WorldActorState::Ufo(s) => crate::marvin::ufo::ufo_load(s, &mut a.plat, r, &ob, l, &mut known),
    }
    l.at("rest", |l| check_keys(&ob, &known, l));
}

pub(crate) fn actor_make(r: &ActorRecord, l: &mut Lx) -> Option<WorldActor> {
    let Some(class) = WorldActorClass::ALL.iter().copied().find(|c| c.name() == r.class) else {
        l.miss(format_args!("no world actor class {}", r.class));
        return None;
    };
    let state = match class {
        WorldActorClass::ShaggyShockwaveActor => WorldActorState::Shockwave(Shockwave::default()),
        WorldActorClass::SmokeGrenade => WorldActorState::SmokeGrenade(SmokeGrenade::default()),
        WorldActorClass::SmokeGrenadeOcclusionBP => WorldActorState::SmokeZone(SmokeZone::default()),
        WorldActorClass::SupermanFlightHit | WorldActorClass::SupermanGrabListener => {
            WorldActorState::FlightActor(Default::default())
        }
        WorldActorClass::SupermanGroundLazer | WorldActorClass::SupermanGroundLazerAir => {
            WorldActorState::LaserSweep(Default::default())
        }
        WorldActorClass::SupermanGroundLazerDamage => WorldActorState::LaserDamage(Default::default()),
        WorldActorClass::SupermanIceBreathStream => WorldActorState::IceStream(Default::default()),
        WorldActorClass::SupermanAimPunch => WorldActorState::AimMarker(Default::default()),
        WorldActorClass::DashToBasketball => WorldActorState::DashToBasketball(Default::default()),
        WorldActorClass::AryaDisguiseShock => WorldActorState::DisguiseShock(Default::default()),
        WorldActorClass::AryaKillPie => WorldActorState::KillPie(Default::default()),
        WorldActorClass::C021Bolt => WorldActorState::Bolt(Default::default()),
        WorldActorClass::C021EarthSmashIn => WorldActorState::SmashIn(Default::default()),
        WorldActorClass::C021ForceField => WorldActorState::ForceField(Default::default()),
        WorldActorClass::C021ForceFieldPerk => WorldActorState::PerkForceField(Default::default()),
        WorldActorClass::C021GrabHit => WorldActorState::GrabHit(Default::default()),
        WorldActorClass::ProjectileBarrier
        | WorldActorClass::C016ProjectileBarrier
        | WorldActorClass::C023AProjectileBarrier => WorldActorState::ProjectileBarrier(Default::default()),
        WorldActorClass::WWShieldActor
        | WorldActorClass::WWShieldActorUSpecial
        | WorldActorClass::VelmaProjectileShieldActor => WorldActorState::ShieldActor(Default::default()),
        WorldActorClass::WWEvadeProjectileBlock => WorldActorState::EvadeShield(Default::default()),
        WorldActorClass::WWBracerShield => WorldActorState::BracerShield(Default::default()),
        WorldActorClass::C019ScrubShield => WorldActorState::ScrubShield(Default::default()),
        WorldActorClass::HarleyBomb => WorldActorState::HarleyBomb(Default::default()),
        WorldActorClass::HarleyJackInTheBox => WorldActorState::JackBox(Default::default()),
        WorldActorClass::ScoobySnackBox => WorldActorState::SnackBox(Default::default()),
        WorldActorClass::C017AntenaBolts => WorldActorState::AntenaBolts(Default::default()),
        WorldActorClass::C017CannonBall => WorldActorState::CannonBall(Default::default()),
        WorldActorClass::C017CannonAim => WorldActorState::TurretAim(Default::default()),
        WorldActorClass::C017ShockwaveActor => WorldActorState::StompWave(Default::default()),
        WorldActorClass::C017DownThrusterAttack => WorldActorState::DownThruster(Default::default()),
        WorldActorClass::C017CannonShotAim => WorldActorState::CannonAim(Default::default()),
        WorldActorClass::C017EnergyDiscAimer => WorldActorState::DiscAim(Default::default()),
        WorldActorClass::C017PlasmaAimer => WorldActorState::PlasmaAim(Default::default()),
        WorldActorClass::C017CannonExplotion => WorldActorState::CannonBurst(Default::default()),
        WorldActorClass::C017PilotReticle => WorldActorState::PilotReticle(Default::default()),
        WorldActorClass::C017AllyTurretBomb => WorldActorState::TurretBomb(Default::default()),
        WorldActorClass::SpawnedPlatformC017Wall => WorldActorState::WallPlatform(Default::default()),
        WorldActorClass::C015Dogpile => WorldActorState::Dogpile(Default::default()),
        WorldActorClass::StevenShieldActor | WorldActorClass::StevenShieldActorUp => {
            WorldActorState::StevenShield(Default::default())
        }
        // (a WW_ShieldActor child: the shield kind's state)
        WorldActorClass::StevenShieldActorUSpecialPlatform => WorldActorState::ShieldActor(Default::default()),
        WorldActorClass::StevenCollisionBlock => WorldActorState::StevenBlock(Default::default()),
        WorldActorClass::HealOverTimeZoneBP => WorldActorState::HealZone(Default::default()),
        WorldActorClass::GarnetShockwaveHitBox
        | WorldActorClass::GarnetShockwaveHitBoxLeft
        | WorldActorClass::GarnetShockwaveHitBoxRight => WorldActorState::ShockwaveHitBox(Default::default()),
        WorldActorClass::GarnetElectricZone => WorldActorState::ElectricZone(Default::default()),
        WorldActorClass::BugsBunnyTunnelParent => WorldActorState::Tunnel(Default::default()),
        WorldActorClass::ItemDispenserBugs => WorldActorState::Dispenser(Default::default()),
        WorldActorClass::ItemHelperNepter => WorldActorState::Nepter(Default::default()),
        WorldActorClass::ItemHelperBananaGuard => WorldActorState::BananaGuard(Default::default()),
        WorldActorClass::BouncePadSpawnedActor => WorldActorState::BouncePad(Default::default()),
        WorldActorClass::AutoTeleportPortal | WorldActorClass::VisualOnlyPortal | WorldActorClass::ExitPortalParent => {
            WorldActorState::PortalActor(Default::default())
        }
        WorldActorClass::C020PortalAimer => WorldActorState::PortalAimer(Default::default()),
        WorldActorClass::MeeSeeksAirFlap => WorldActorState::AirFlap(Default::default()),
        WorldActorClass::PolymorphGrenade => WorldActorState::PolymorphGrenade(Default::default()),
        WorldActorClass::FinnDashAimerExperiment => WorldActorState::FinnAimer(Default::default()),
        WorldActorClass::FinnGemBoostBlast => WorldActorState::GemBoostBlast(Default::default()),
        WorldActorClass::FinnShop => WorldActorState::FinnShop(Default::default()),
        WorldActorClass::BMOFallowActor => WorldActorState::BmoActor(Default::default()),
        WorldActorClass::FinnHighFiveListener | WorldActorClass::FinnKickListener => {
            WorldActorState::FinnListener(Default::default())
        }
        WorldActorClass::FinnHighFiveShockwave => WorldActorState::HighFiveShockwave(Default::default()),
        WorldActorClass::FinnShockwaveActor => WorldActorState::FinnShockwave(Default::default()),
        WorldActorClass::VelmaHealBeam | WorldActorClass::VelmaChildBeam => WorldActorState::Beam(Default::default()),
        WorldActorClass::VelmaCarryRunActor => WorldActorState::VelmaCarryRun(Default::default()),
        WorldActorClass::VelmaEvidence | WorldActorClass::VelmaEvidenceNoGrav => {
            WorldActorState::Evidence(Default::default())
        }
        WorldActorClass::SlowtoStunGrenade => WorldActorState::SlowToStunGrenade(Default::default()),
        WorldActorClass::VelmaVialAttack => WorldActorState::VelmaVial(Default::default()),
        WorldActorClass::VelmaJenkinsPin => WorldActorState::JenkinsPin(Default::default()),
        WorldActorClass::C023BAllySaw => WorldActorState::AllySaw(Default::default()),
        WorldActorClass::C023BDashListener => WorldActorState::StripeListener(Default::default()),
        WorldActorClass::JakeAimPunchEatListener => WorldActorState::JakeAimPunchEatListener(Default::default()),
        WorldActorClass::JakeAimer => WorldActorState::JakeAimer(Default::default()),
        WorldActorClass::JakeEatAllyAim => WorldActorState::JakeEatAllyAim(Default::default()),
        WorldActorClass::JakeSpitAllyHitBox => WorldActorState::JakeSpitAllyHitBox(Default::default()),
        WorldActorClass::JakeHorseActor3 => WorldActorState::JakeHorse(Default::default()),
        WorldActorClass::JakeHorsePlatform => WorldActorState::JakeHorsePlatform(Default::default()),
        WorldActorClass::JakeHouseActor => WorldActorState::JakeHouse(Default::default()),
        WorldActorClass::ProjectileAimerSpitJake => WorldActorState::SpitAimer(Default::default()),
        WorldActorClass::JakeSpitCharacterHitBox => WorldActorState::SpitHitBox(Default::default()),
        WorldActorClass::JakeStretch => WorldActorState::Stretch(Default::default()),
        WorldActorClass::JerrySpawnedActor | WorldActorClass::JerrySpawnedActorTempInvincibility => {
            WorldActorState::JerryActor(Default::default())
        }
        WorldActorClass::TomAndJerrySlingShotAim02 => WorldActorState::SlingAim(Default::default()),
        WorldActorClass::TomAndJerryNoJerrySlingShotAim => WorldActorState::NoJerryAim(Default::default()),
        WorldActorClass::MouseTrapSpawnedActor => WorldActorState::MouseTrap(Default::default()),
        WorldActorClass::C018Platform | WorldActorClass::C018PerkPlatform => WorldActorState::Ufo(Default::default()),
        WorldActorClass::C018RocketExplosion | WorldActorClass::C018PerkRocketExplosion => {
            WorldActorState::RocketExplosion(Default::default())
        }
        WorldActorClass::C018DisintegrationRay | WorldActorClass::C018DisintegrationRayDown => {
            WorldActorState::Ray(Default::default())
        }
        _ => {
            l.miss(format_args!("no rule to make a {}", r.class));
            return None;
        }
    };
    let owner = l.fighter(r.owner)?;
    let mut a = WorldActor {
        cls: l.name(&r.class),
        class,
        seq: int(r.id, l)?,
        dead: false,
        owner,
        owner_id: int(r.owner, l)?,
        charge: None,
        elem: None,
        lifetime_pause: None,
        size_scale: None,
        plat: None,
        dodge: None,
        doom: None,
        mobility: None,
        stationary_count: None,
        state,
    };
    actor_load(&mut a, r, l);
    Some(a)
}

// ------------------------------------------------------------------ the arenas

/// One arena loaded from its records in order: an object of the record's id loaded in place, one the World lacks
/// made, one the record lacks removed. The arena keeps its own order; a record that orders them otherwise is named.
fn load_arena<T, R>(
    arena: &mut mvs_sim::Arena<T>,
    recs: &[&R],
    id_of: impl Fn(&T) -> (i32, mvs_sim::names::Name),
    rec_id: impl Fn(&R) -> (f64, &str),
    mut load: impl FnMut(&mut T, &R, &mut Lx),
    make: impl Fn(&R, &mut Lx) -> Option<T>,
    l: &mut Lx,
) {
    let wanted: Vec<f64> = recs.iter().map(|r| rec_id(r).0).collect();
    // (an actor of another class at a record's id is another actor: dropped, the record's made)
    arena.retain(|_, t| {
        let (id, cls) = id_of(t);
        recs.iter().any(|r| rec_id(r) == (f64::from(id), l.str(cls)))
    });
    for (i, r) in recs.iter().enumerate() {
        l.at(&format!("[{i}]"), |l| {
            let id = rec_id(r).0;
            let found = arena.iter().find(|(_, t)| f64::from(id_of(t).0) == id).map(|(k, _)| k);
            match found {
                Some(k) => load(arena.get_mut(k).expect("a live id"), r, l),
                None => {
                    if let Some(t) = make(r, l) {
                        arena.insert(t);
                    }
                }
            }
        });
    }
    let order: Vec<f64> = arena.iter().map(|(_, t)| f64::from(id_of(t).0)).collect();
    if order != wanted {
        l.miss(format_args!("the arena's order {order:?} is not the record's {wanted:?}"));
    }
}

pub fn put_all(x: &Wx) -> (Vec<ProjectileRecord>, Vec<ActorRecord>) {
    let a = &x.w.actors;
    let projectiles = a.projectiles.iter().map(|(_, p)| projectile_put(p, x)).collect();
    let mut actors: Vec<ActorRecord> = a.actors.iter().map(|(_, w)| actor_put(w, x)).collect();
    actors.extend(a.items.iter().map(|(_, i)| item_put(i, x)));
    // (Morty's typed lists, in the record's family order)
    actors.extend(a.markers.iter().map(|(_, m)| crate::morty::marker::marker_put(m, x)));
    actors.extend(a.ships.iter().map(|(_, s)| crate::morty::ship::ship_put(s, x)));
    actors.extend(a.mortars.iter().map(|(_, m)| crate::morty::mortar::mortar_put(m, x)));
    actors.extend(a.aim_punches.iter().map(|(_, p)| crate::morty::aim_punch::aim_punch_put(p, x)));
    actors.extend(a.accelerators.iter().map(|(_, c)| crate::morty::accelerator::accelerator_put(c, x)));
    (projectiles, actors)
}

/// The record's actor families (the oracle World's lists FAMILIES).
const FAMILIES: [&str; 7] = ["actors", "items", "markers", "ships", "mortars", "aimPunches", "accelerators"];

/// Morty's typed lists from their records (`chars::morty`'s files).
fn load_lists<'r>(w: &mut World, of: &impl Fn(&str) -> Vec<&'r ActorRecord>, l: &mut Lx) {
    use crate::morty::{accelerator, aim_punch, marker, mortar, ship};
    fn id(r: &ActorRecord) -> (f64, &str) {
        (r.id, &r.class)
    }
    let recs = of("markers");
    l.at("markers", |l| {
        load_arena(&mut w.actors.markers, &recs, |m| (m.seq, m.cls), id, marker::marker_load, marker::marker_make, l)
    });
    let recs = of("ships");
    l.at("ships", |l| {
        load_arena(&mut w.actors.ships, &recs, |s| (s.seq, s.cls), id, ship::ship_load, ship::ship_make, l)
    });
    let recs = of("mortars");
    l.at("mortars", |l| {
        load_arena(&mut w.actors.mortars, &recs, |m| (m.seq, m.cls), id, mortar::mortar_load, mortar::mortar_make, l)
    });
    let recs = of("aimPunches");
    l.at("aimPunches", |l| {
        let (load, make) = (aim_punch::aim_punch_load, aim_punch::aim_punch_make);
        load_arena(&mut w.actors.aim_punches, &recs, |p| (p.seq, p.cls), id, load, make, l)
    });
    let recs = of("accelerators");
    l.at("accelerators", |l| {
        let (load, make) = (accelerator::accelerator_load, accelerator::accelerator_make);
        load_arena(&mut w.actors.accelerators, &recs, |c| (c.seq, c.cls), id, load, make, l)
    });
    l.index(w);
}

/// The actor lists from their records: a first pass makes the objects the World lacks (its misses dropped), so a
/// reference to an object later in a list resolves in the second.
/// A world actor gone from its list as the oracle writes the object whole (a projectile's hit set still holds it): the
/// record's keys and its `rest` in one object, its owner by reference.
// The load: [`actor_whole_load`].
pub fn actor_whole(a: &WorldActor, x: &Wx) -> Value {
    let r = actor_put(a, x);
    let mut o: Vec<(String, Value)> = match r.rest {
        Value::Obj(ob) => ob.entries().to_vec(),
        _ => Vec::new(),
    };
    o.extend([
        ("cls".to_owned(), Value::Str(r.class)),
        ("dead".to_owned(), Value::Bool(r.dead)),
        ("id".to_owned(), Value::Num(r.id)),
        ("owner".to_owned(), a.owner.to_value(x)),
    ]);
    for (k, v) in [("x", r.x), ("y", r.y), ("vx", r.vx), ("vy", r.vy)] {
        if let Some(v) = v {
            o.push((k.to_owned(), Value::Num(v)));
        }
    }
    Value::Obj(mvs_trace::Obj::new(o))
}

/// [`actor_whole`]'s inverse: the gone world actor a hit set names. It is made once per frame (by its id), kept in
/// `Lx::gone_actors` for the World's `gone_actors`, and named by an id of no live slot of the arena.
pub fn actor_whole_load(o: &mvs_trace::Obj, l: &mut Lx) -> Option<mvs_sim::actors::ActorId> {
    let Some(Value::Num(id)) = o.get("id") else {
        l.miss("a gone world actor with no id");
        return None;
    };
    let seq = int(*id, l)?;
    if let Some((_, k, _)) = l.gone_actors.iter().find(|(s, _, _)| *s == seq) {
        return Some(*k);
    }
    let Some(Value::Str(class)) = o.get("cls") else {
        l.miss("a gone world actor with no class");
        return None;
    };
    let num_of = |k: &str| match o.get(k) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    };
    let owner = match o.get("owner") {
        Some(Value::Obj(r)) => match r.get("id") {
            Some(Value::Num(n)) => *n,
            _ => -1.0,
        },
        _ => -1.0,
    };
    const OWN: [&str; 8] = ["cls", "dead", "id", "owner", "vx", "vy", "x", "y"];
    let rest = o.entries().iter().filter(|(k, _)| !OWN.contains(&k.as_str())).cloned().collect();
    let r = ActorRecord {
        family: "actors".to_owned(),
        id: *id,
        class: class.clone(),
        owner,
        x: num_of("x"),
        y: num_of("y"),
        vx: num_of("vx"),
        vy: num_of("vy"),
        dead: matches!(o.get("dead"), Some(Value::Bool(true))),
        rest: Value::Obj(mvs_trace::Obj::new(rest)),
        ..ActorRecord::default()
    };
    let mut a = actor_make(&r, l)?;
    actor_load(&mut a, &r, l);
    // (an id of no slot: past the arena's, never handed out)
    let k = mvs_sim::actors::ActorId::new(u32::MAX - u32::try_from(l.gone_actors.len()).ok()?, 0);
    l.gone_actors.push((seq, k, a));
    Some(k)
}

pub fn load_all(w: &mut World, projectiles: &[ProjectileRecord], actors: &[ActorRecord], l: &mut Lx) {
    let n = l.misses.len();
    load_pass(w, projectiles, actors, l);
    l.misses.truncate(n);
    load_pass(w, projectiles, actors, l);
    w.actors.gone_mortars = l.gone_mortars.drain(..).map(|(_, k, a)| (k, a)).collect();
    w.actors.gone_items = l.gone_items.drain(..).map(|(_, k, i)| (k, i)).collect();
    w.actors.gone_actors = l.gone_actors.drain(..).map(|(_, k, a)| (k, a)).collect();
    // A Bugs rocket's geometry is of its class data, not in the record: a rocket the load made has none.
    let ids: Vec<_> = w.actors.projectiles.ids().collect();
    for id in ids {
        let Some(p) = w.actors.projectiles.get(id) else { continue };
        if !matches!(p.ext, ProjectileExt::Rocket(_)) {
            continue;
        }
        let geo = mvs_sim::chars::bugs::rockets::geo_of(w, p);
        if let Some(ProjectileExt::Rocket(r)) = w.actors.projectiles.get_mut(id).map(|p| &mut p.ext) {
            r.geo = geo;
        }
    }
}

fn load_pass(w: &mut World, projectiles: &[ProjectileRecord], actors: &[ActorRecord], l: &mut Lx) {
    let ps: Vec<&ProjectileRecord> = projectiles.iter().collect();
    l.at("projectiles", |l| {
        load_arena(
            &mut w.actors.projectiles,
            &ps,
            |p| (p.seq, p.cls),
            |r| (r.id, &r.class),
            projectile_load,
            projectile_make,
            l,
        );
    });
    l.index(w);
    let of = |fam: &str| actors.iter().filter(|r| r.family == fam).collect::<Vec<_>>();
    let (world_actors, items) = (of("actors"), of("items"));
    for r in actors {
        if !FAMILIES.contains(&r.family.as_str()) {
            l.miss(format_args!("an actor of the World's {} (no rule to load it)", r.family));
        }
    }
    l.at("actors", |l| {
        load_arena(
            &mut w.actors.actors,
            &world_actors,
            |a| (a.seq, a.cls),
            |r| (r.id, &r.class),
            actor_load,
            actor_make,
            l,
        );
    });
    l.index(w);
    l.at("items", |l| {
        load_arena(&mut w.actors.items, &items, |i| (i.seq, i.cls), |r| (r.id, &r.class), item_load, item_make, l)
    });
    load_lists(w, &of, l);
    // A made item's class data, which the record does not hold: the table entry its spawn took, once, from its
    // spawner (`actor` of the spawner's actorDef). An item with no spawner is the stage's or the
    // items part's (the item area spawns from the items part, world/items.rs).
    let made: Vec<_> =
        w.actors.items.iter().filter(|(_, i)| i.src == DefSrc::None).map(|(k, i)| (k, i.spawner, i.cls)).collect();
    for (k, spawner, cls) in made {
        let cls_s = l.str(cls).to_owned();
        let src = match spawner {
            Some(_) => mvs_sim::world::items::resolve_def(w, spawner, &cls_s),
            None => {
                let items = w.data.items.as_ref().and_then(|d| d.actors.get_index_of(&cls_s));
                let stage = w.data.stage.actors.as_ref().and_then(|m| m.get_index_of(&cls_s));
                if items.is_some() && stage.is_some() {
                    let m = "with no spawner, in the stage's and the items part's tables (the items part's taken)";
                    l.miss(format_args!("item {cls_s} {m}"));
                }
                let at = |i: usize| u32::try_from(i).expect("a table index");
                items.map(|i| DefSrc::Items(at(i))).or(stage.map(|i| DefSrc::Stage(at(i))))
            }
        };
        match src {
            Some(src) => w.actors.items.get_mut(k).expect("a live item").src = src,
            None => l.miss(format_args!("item {cls_s}: no class data for its spawner")),
        }
    }
    l.index(w);
}
