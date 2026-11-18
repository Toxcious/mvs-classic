//! The shared ice actors' particle side (sim combat/ice.rs, chars/superman/ice.rs):
//! IceBlockItem's block, which is a particle component, not a mesh.
//!
//! IceBlockItem has no mesh component: the block is its PS_SupermanIceBreath_IceCube component (auto activated, on
//! the Capsule at a relative scale of 0.3; the Capsule has bAbsoluteScale, so the item's own root scale carries it).
//! SetTeamColor @113: its Team parameter = SelectFloat(0.6, the colour value, IsLocalOnline) of the owner; an owner
//! that is no BPI_BaseFighter gives ColorValue 0 and not local (@123-425). The inherited ParticleSystem component
//! (PS_ShaggySandwichProjectile, bVisible false: the throw's trail) is the shared path's `item_trail`. The death blink
//! hides the actor, the component with it.
//! Unverified: Team is the owner's at the block's first sight (when SetTeamColor runs again after a pickup is not
//! read); held, the block is drawn at the place the view hands over (the holder's mesh origin for a pawn without a
//! hold socket rule), with the item's own rotation. An ownerless block (a random item, which reads its owner only
//! once picked up) has no owner's colour: its LocalColorIndex default is taken, as for the thrown trail.

use indexmap::IndexMap;
use mvs_data::value::Value;
use mvs_sim::World;
use mvs_sim::actors::{ActorRef, ItemId};
use mvs_vfx::SystemId;
use mvs_vfx::math::IDENTITY;
use mvs_view::actors::actor_place;

use crate::fx::attach::text;
use crate::fx::chars::{CharFxHooks, ProjRule};
use crate::fx::{Follow, Fx};

const BLOCK: &str = "IceBlockItem_C";
const CUBE: &str = "PS_SupermanIceBreath_IceCube";

/// One record per live block, by item.
#[derive(Debug)]
struct Block {
    h: Option<SystemId>,
    seen: u64,
}

/// What its effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Ice {
    /// In the order the blocks were first seen (the actor meshes' order: the kills run in it).
    blocks: IndexMap<ItemId, Block>,
    tick: u64,
}

impl Ice {
    fn clear(&mut self, fx: &mut Fx) {
        for b in self.blocks.drain(..).map(|(_, b)| b) {
            if let Some(h) = b.h {
                fx.player.kill(h);
            }
        }
    }
}

impl CharFxHooks for Ice {
    /// Once per presentation tick: the cube of each block there, following the item tick by tick (in a hand, the
    /// holder's mesh origin: actor_place), shown with the item, gone with it.
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.tick += 1;
        for (id, it) in w.actors.items.iter() {
            if it.dead || w.names.str(it.cls) != BLOCK {
                continue;
            }
            let b = match self.blocks.get_mut(&id) {
                Some(b) => b,
                None => {
                    let def = it.def(&w.data);
                    let c = def.components.get(CUBE).and_then(Value::as_obj);
                    let h = c.and_then(|c| {
                        let template = text(c, "Template")?;
                        let rs = c
                            .get("RelativeScale3D")
                            .and_then(Value::as_obj)
                            .and_then(|s| s.get("X"))
                            .and_then(Value::as_f64)
                            .unwrap_or(1.0);
                        let team = it.owner.map_or(0.0, |o| fx.team(w.fighter(o).id));
                        let f = Follow::Actor {
                            turned: true,
                            a: ActorRef::Item(id),
                            rel: [0.0; 3],
                            rel_rot: IDENTITY,
                            scale: [rs; 3],
                        };
                        fx.spawn_follow(w, template, f, 1.0, &[("Team", team)])
                    });
                    self.blocks.entry(id).or_insert(Block { h, seen: 0 })
                }
            };
            if let Some(h) = b.h {
                let hidden = it.hidden() || actor_place(w, ActorRef::Item(id)).is_none_or(|p| p.hidden);
                fx.player.set_visible(h, !hidden);
            }
            b.seen = self.tick;
        }
        let tick = self.tick;
        let player = &mut fx.player;
        self.blocks.retain(|_, b| {
            let keep = b.seen == tick;
            if !keep && let Some(h) = b.h {
                player.kill(h);
            }
            keep
        });
    }

    fn reset(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear(fx);
    }
}

/// Its classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
