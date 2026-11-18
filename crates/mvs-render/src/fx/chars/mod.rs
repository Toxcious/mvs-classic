//! What the characters' and the shared classes' modules draw in a match: one [`CharFx`] per module, made for every
//! match in a fixed order (the characters' entries first, then the shared classes'), its hooks called by `Fx` at the
//! presented tick's points; and the classes' [`ProjRule`]s, each module's `RULES` resolved by class name once.
//!
//! A module is a file of its own (`<key>.rs` here, `../shared/<name>.rs`, `../items.rs`): its state type with
//! [`CharFxHooks`] (every hook a no-op until it is written) and its `RULES`. One with no rule of its own leaves the
//! class to the shared path. A rule or hook calls the shared path's helpers rather than copying them: `Fx::team`,
//! `Fx::socket_follow`, `Fx::spawn_follow`, `Fx::attach_to_mesh` / `attach_socket`, `Follow::Actor` with
//! `mvs_view::actors::actor_place`, `mvs_sim::fighter::common::actor_def`, `mvs_sim::actors::projectile::attach_parent`,
//! `mvs_view::skins::skin_of` and `FighterDraws::actor`.

use indexmap::IndexMap;
use mvs_data::value::Props;
use mvs_sim::World;
use mvs_sim::actors::ProjectileId;
use mvs_sim::actors::projectile::{AttachParentOut, Projectile};
use mvs_sim::events::Event;
use mvs_vfx::SystemId;
use mvs_vfx::math::V3;

use super::Fx;
use super::items;
use super::shared::{
    actor_vfx, air_superiority, ally_dash, arcade, barrier, disguise, educated, electric, elemental, firewall,
    heart_skin, ice, m010_pickups, polymorph, projectile_block, speed, tutorial,
};
use shaggy::shockwave;

pub(super) mod arya;
pub(super) mod batman;
pub(super) mod black_adam;
pub(super) mod bugs;
pub(super) mod creature;
pub(super) mod finn;
pub(super) mod garnet;
pub(super) mod gizmo;
pub(super) mod harley;
pub(super) mod iron_giant;
pub(super) mod jake;
pub(super) mod lebron;
pub(super) mod marvin;
pub(super) mod morty;
pub(super) mod rick;
pub(super) mod shaggy;
pub(super) mod steven;
pub(super) mod stripe;
pub(super) mod superman;
pub(super) mod taz;
pub(super) mod tom_and_jerry;
pub(super) mod velma;
pub(super) mod velma_police;
pub(super) mod wonder_woman;

/// A module's hooks, each called on every module in [`CharFx`]'s order; the default does
/// nothing. `fx` is the match's effects (the module's own state taken out of it for the call).
pub(crate) trait CharFxHooks {
    /// presentTick: its spawned actors of the World's lists, after the smoke trails.
    fn actors(&mut self, _fx: &mut Fx, _w: &World) {}
    /// presentTick: its pawns' and buffs' effects, after the montage meshes and the heal effects.
    fn place(&mut self, _fx: &mut Fx, _w: &World) {}
    /// presentTick: after the particle player's tick.
    fn after_tick(&mut self, _fx: &mut Fx, _w: &World) {}
    /// Each drawn frame, after the views are placed (`alpha`: the frame between the last two ticks).
    fn frame(&mut self, _fx: &mut Fx, _w: &World, _alpha: f64) {}
    /// A Lab reset (the World's actors are gone and the player's systems cleared).
    fn reset(&mut self, _fx: &mut Fx) {}
    /// The match's end.
    fn end(&mut self, _fx: &mut Fx) {}
    /// An event only this module's graphs raise (handleEvents asks before its own match): true when taken.
    fn event(&mut self, _fx: &mut Fx, _w: &World, _e: &Event) -> bool {
        false
    }
    /// The match's warm-up (Fx::warm): the component meshes it may add, with each material set they may take.
    fn warm(&mut self, _fx: &mut Fx, _w: &World, _out: &mut Vec<mvs_view::WarmMesh>) {}
    /// An event that names a minion (an id no pawn of the match has): true when this module draws that minion.
    fn minion_event(&mut self, _fx: &mut Fx, _w: &World, _e: &Event) -> bool {
        false
    }
}

/// An effects' component mesh's warm entry (crate::warm): a static mesh of the store (`mesh`, its object path) or,
/// None, a geometry of its own, drawn with the component material of key `key` (`<path>#local`, `#spline`) on
/// every slot (cmesh.rs).
pub(super) fn component_warm(mesh: Option<&str>, key: &str) -> mvs_view::WarmMesh {
    let material = crate::cmesh::component_material_path(key);
    mvs_view::WarmMesh {
        mesh: mesh.map(std::sync::Arc::from),
        materials: vec![Some(std::sync::Arc::from(material.as_str()))],
        every_slot: true,
        ..mvs_view::WarmMesh::default()
    }
}

/// The sound events character `key`'s drawing code posts by name (Arya's
/// DISGUISE_SOUNDS, the Iron Giant's GIANT_SOUNDS), for its match scope.
pub fn render_sounds(key: &str) -> &'static [&'static str] {
    match key {
        // (the buff's cast and end, ActivateDisguiseMulticast)
        "arya" => &["sfx_shared_class_assassin_clone_cast", "sfx_cha_batman_smokebomb_end"],
        // (BOLT_POP.audio, the "+1 Bolt" widget's Show)
        "irongiant" => &["sfx_cha_finn_backpack_treasure_collect"],
        _ => &[],
    }
}

/// A fighter of the match playing character `key` (the first), for a module's warm-up.
pub(super) fn fighter_of(w: &World, key: &str) -> Option<mvs_sim::fighter::FighterId> {
    w.all_ids().into_iter().find(|&f| w.names.str(w.fighter(f).ch) == key)
}

/// Each module's state, a variant per module, listed once with its rules table.
macro_rules! modules {
    ($($v:ident($m:ident::$t:ident)),* $(,)?) => {
        /// What one module draws in a match and keeps between steps.
        #[derive(Debug)]
        pub(crate) enum CharFx {
            $($v($m::$t),)*
        }

        impl CharFx {
            /// Every module's, in the fixed order (the characters', then the shared classes').
            pub(crate) fn all() -> Vec<CharFx> {
                vec![$(CharFx::$v($m::$t::default()),)*]
            }

            fn hooks(&mut self) -> &mut dyn CharFxHooks {
                match self {
                    $(CharFx::$v(s) => s,)*
                }
            }
        }

        /// Every module's projectile rules, in [`CharFx::all`]'s order.
        static MODULE_RULES: &[&[(&str, ProjRule)]] = &[$($m::RULES,)*];
    };
}

modules! {
    Shaggy(shaggy::Shaggy),
    Morty(morty::Morty),
    IronGiant(iron_giant::IronGiant),
    WonderWoman(wonder_woman::WonderWoman),
    Harley(harley::Harley),
    Superman(superman::Superman),
    Lebron(lebron::Lebron),
    Arya(arya::Arya),
    Batman(batman::Batman),
    Creature(creature::Creature),
    BlackAdam(black_adam::BlackAdam),
    Bugs(bugs::Bugs),
    Rick(rick::Rick),
    Taz(taz::Taz),
    Steven(steven::Steven),
    Garnet(garnet::Garnet),
    Finn(finn::Finn),
    Jake(jake::Jake),
    TomAndJerry(tom_and_jerry::TomAndJerry),
    VelmaPolice(velma_police::VelmaPolice),
    Velma(velma::Velma),
    Marvin(marvin::Marvin),
    Stripe(stripe::Stripe),
    Gizmo(gizmo::Gizmo),
    Shockwave(shockwave::Shockwave),
    Barrier(barrier::Barrier),
    Disguise(disguise::Disguise),
    Electric(electric::Electric),
    Polymorph(polymorph::Polymorph),
    ActorVfx(actor_vfx::ActorVfx),
    Elemental(elemental::Elemental),
    AirSuperiority(air_superiority::AirSuperiority),
    AllyDash(ally_dash::AllyDash),
    ProjectileBlock(projectile_block::ProjectileBlock),
    Firewall(firewall::Firewall),
    Ice(ice::Ice),
    HeartSkin(heart_skin::HeartSkin),
    M010Pickups(m010_pickups::M010Pickups),
    Tutorial(tutorial::Tutorial),
    Arcade(arcade::Arcade),
    Educated(educated::Educated),
    Speed(speed::Speed),
    ItemCars(items::ItemCars),
}

impl CharFx {
    pub(super) fn warm(&mut self, fx: &mut Fx, w: &World, out: &mut Vec<mvs_view::WarmMesh>) {
        self.hooks().warm(fx, w, out);
    }

    pub(super) fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.hooks().actors(fx, w);
    }

    pub(super) fn place(&mut self, fx: &mut Fx, w: &World) {
        self.hooks().place(fx, w);
    }

    pub(super) fn after_tick(&mut self, fx: &mut Fx, w: &World) {
        self.hooks().after_tick(fx, w);
    }

    pub(super) fn frame(&mut self, fx: &mut Fx, w: &World, alpha: f64) {
        self.hooks().frame(fx, w, alpha);
    }

    pub(super) fn reset(&mut self, fx: &mut Fx) {
        self.hooks().reset(fx);
    }

    pub(super) fn end(&mut self, fx: &mut Fx) {
        self.hooks().end(fx);
    }

    pub(super) fn event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        self.hooks().event(fx, w, e)
    }

    pub(super) fn minion_event(&mut self, fx: &mut Fx, w: &World, e: &Event) -> bool {
        self.hooks().minion_event(fx, w, e)
    }
}

// ---- projectile rules -------------------------------------------------------------------------------------------

/// A hook of a rule on one projectile: its module's state (the `CharFx` the rule's module entered), the match's
/// effects, the World and the projectile's record.
pub(crate) type ProjHook = fn(&mut CharFx, &mut Fx, &World, &mut ProjFxState);

/// ProjRule.parent: the attach parent's transform of a component (the class's components, the component's name).
pub(crate) type ParentOf = fn(&World, &Projectile, &Props, &str) -> AttachParentOut;

/// ProjRule.scale: the relative scale the graph set (the attach parent's scale given).
pub(crate) type ScaleOf = fn(&World, &Projectile, V3) -> Option<V3>;

/// ProjRule.own_emitter: draws the class's own emitter of a projectileDeath event (true: drawn).
pub(crate) type OwnEmitter = fn(&mut CharFx, &mut Fx, &World, &Event) -> bool;

/// What a class's graph does to its particle components on top of the component templates
/// (`rot` is mvs_view::actors::MeshRule's, which actor_rot reads for both crates). The shared path
/// (Fx::projectile_component, place_projectiles) applies world_scale, no_team, rel_loc, parent, shown, more, scale and
/// systems; a rule's hooks call `Fx::remake` for an Activate(true).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProjRule {
    /// The construction script's ParticleSystem.SetWorldScale3D (the relative scale is that over the parent's world
    /// scale then).
    pub(crate) world_scale: Option<f64>,
    /// No graph writes Team (`team: false`).
    pub(crate) no_team: bool,
    /// A relative location written at BeginPlay.
    pub(crate) rel_loc: Option<fn(&World, &Projectile) -> Option<V3>>,
    /// The attach parent's transform where a timeline moves it (the class's components and the component's name).
    pub(crate) parent: Option<ParentOf>,
    /// SetVisibility on the ParticleSystem component.
    pub(crate) shown: Option<fn(&World, &Projectile) -> bool>,
    /// The class's other particle components.
    pub(crate) more: &'static [&'static str],
    /// A relative scale the graph sets in place of the component's own (the attach parent's scale given); None while
    /// it has not.
    pub(crate) scale: Option<ScaleOf>,
    /// An emitter the class's graph spawns itself, which the sim raises as a projectileDeath event: true when the
    /// rule draws it (the shared spawn is left out).
    pub(crate) own_emitter: Option<OwnEmitter>,
    /// The particle components of the actor's own the rule keeps (a reflect's pause sets every ParticleSystem of
    /// the actor).
    pub(crate) systems: Option<fn(&CharFx, &ProjFxState) -> Vec<SystemId>>,
    /// Made once after the components; tick each step before the shared buff effects, after after them; end with
    /// the actor.
    pub(crate) made: Option<ProjHook>,
    pub(crate) tick: Option<ProjHook>,
    pub(crate) after: Option<ProjHook>,
    pub(crate) end: Option<ProjHook>,
}

impl ProjRule {
    /// A rule that adds nothing (`..ProjRule::NONE` fills a rule's other fields).
    pub(crate) const NONE: ProjRule = ProjRule {
        world_scale: None,
        no_team: false,
        rel_loc: None,
        parent: None,
        shown: None,
        more: &[],
        scale: None,
        own_emitter: None,
        systems: None,
        made: None,
        tick: None,
        after: None,
        end: None,
    };
}

/// A projectile's record: its first component, the handles of the rule's
/// `more`, the actor's place of the latest tick.
#[derive(Debug)]
pub(crate) struct ProjFxState {
    /// The actor's sequence number (the record's key) and its id now.
    pub(crate) seq: i32,
    pub(crate) id: ProjectileId,
    pub(crate) h: Option<SystemId>,
    pub(crate) more: Vec<Option<SystemId>>,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(super) seen: bool,
    /// The class's rule and the index of its module in [`CharFx::all`].
    pub(super) rule: Option<(usize, &'static ProjRule)>,
    /// The first component's name (remake's).
    #[allow(dead_code, reason = "remake's, which the character modules call")]
    pub(super) cn: &'static str,
    /// Hidden by a tunnel carry (carryFx).
    pub(super) hid: bool,
    /// The speed-up buff's component; the slow buff's and its class.
    pub(super) speed: Option<SystemId>,
    pub(super) slow: Option<SystemId>,
    pub(super) slow_cls: Option<mvs_sim::combat::buff_class::BuffClass>,
}

impl ProjFxState {
    pub(super) fn new(
        seq: i32,
        id: ProjectileId,
        h: Option<SystemId>,
        cn: &'static str,
        x: f64,
        y: f64,
        rule: Option<(usize, &'static ProjRule)>,
    ) -> ProjFxState {
        ProjFxState {
            seq,
            id,
            h,
            more: Vec::new(),
            x,
            y,
            seen: true,
            rule,
            cn,
            hid: false,
            speed: None,
            slow: None,
            slow_cls: None,
        }
    }
}

/// Every module's rules by class name, resolved once (a class entered twice stops here).
fn proj_rules() -> &'static IndexMap<&'static str, (usize, &'static ProjRule)> {
    static RULES: std::sync::OnceLock<IndexMap<&'static str, (usize, &'static ProjRule)>> = std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        let mut t = IndexMap::new();
        for (k, rules) in MODULE_RULES.iter().enumerate() {
            for (cls, rule) in rules.iter() {
                if t.insert(*cls, (k, rule)).is_some() {
                    panic!("projectile effect {cls} is registered twice");
                }
            }
        }
        t
    })
}

/// `projectileFxRule`: the class's rule and its module's index (None: the shared path's drawing).
pub(super) fn proj_rule(cls: &str) -> Option<(usize, &'static ProjRule)> {
    proj_rules().get(cls).copied()
}
