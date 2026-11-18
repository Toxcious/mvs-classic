//! Rick's portals as the record writes them.

use mvs_sim::chars::rick::portal::{Pending, PortalActor};
use mvs_trace::{ActorRecord, Obj, Value};

use crate::actors::{opt_load, static_object};
use crate::conv::plain_keys;
use crate::conv::plain_struct;
use crate::cx::{Lx, Wx};

plain_struct!(Pending { "k" = k, "left" = left });

macro_rules! portal_keys {
    ($mac:ident $($pre:tt)*) => {
        $mac!($($pre)*
            "allowOne" = allow_one, "attackId" = attack_id, "beingTeleported" = being_teleported,
            "blastBounds" = blast_bounds, "collision" = collision, "ended" = ended, "facingLeft" = facing_left,
            "fightersTeleported" = fighters_teleported, "hit" = hit, "hitstunned" = hitstunned,
            "inEndPlay" ~ in_end_play, "inside" = inside, "labelFor" = label_for, "lat" = lat, "lifeAt" = life_at,
            "meshAt" = mesh_at, "onlyVisuals" = only_visuals, "onlyVisualsTick" = only_visuals_tick,
            "outroTick" = outro_tick, "outroVfx" = outro_vfx, "portalCount" = portal_count, "queued" = queued,
            "spawnTick" = spawn_tick, "team" = team, "teleported" = teleported, "throughIn" = through_in,
            "throughTick" = through_tick, "transitioningOut" = transitioning_out, "visualsDue" = visuals_due,
        )
    };
}

/// A portal of Rick as the record writes it: the place (vx and vy readonly 0), its own keys and the constant
/// `portalParent`.
pub fn portal_put(s: &PortalActor, x: &Wx, r: &mut ActorRecord, o: &mut Vec<(String, Value)>) {
    r.x = Some(f64::from(s.x));
    r.y = Some(f64::from(s.y));
    r.vx = Some(0.0);
    r.vy = Some(0.0);
    portal_keys!(plain_keys put s, x, o;);
    o.push(("portalParent".into(), Value::Bool(true)));
    o.push(("p".into(), static_object()));
}

/// [`portal_put`]'s inverse; the keys it knows go into `known`.
pub fn portal_load(s: &mut PortalActor, r: &ActorRecord, ob: &Obj, l: &mut Lx, known: &mut Vec<&'static str>) {
    opt_load(&mut s.x, r.x, "x", l);
    opt_load(&mut s.y, r.y, "y", l);
    l.at("rest", |l| {
        portal_keys!(plain_keys load s, ob, l;);
    });
    known.extend(portal_keys!(plain_keys names));
    known.push("portalParent");
}
