//! Harley Quinn's effects (the CharFx hooks; her meshes
//! and pose are mvs-view chars/harley.rs).
//!
//! What a match draws for her beside the meshes:
//!   BombAuraVFX: Pawn_Harley's BatBomb component's child (PS_HarleyBatDollAura, bVisible false). Nothing sets its
//!     visibility alone: it shows with BatBomb through the propagation of SetSubMeshVisibility (tag SatchelCharge).
//!     OnMeshActivated / SpawnInvulnerability set its Team (BombAuraVFX.SetFloatParameter @1154) to SelectFloat(0.6,
//!     ColorIndex, IsLocalOnlinePlayer). It rides BatBomb's socket (OnFacingChange @1228..1309: fml_un_L_PropOffset
//!     facing left, the node's fml_un_R_PropOffset facing right) at BatBomb's RelativeScale3D.X.
//!     Unverified: started when BatBomb is shown and killed when it is hidden or the facing changes (whether the
//!     hidden component, whose tick starts off, simulates is not read; as Item_Sandwich's trail).
//!   HarleyBomb's particle components PS_Explosion_SatchelAura (PS_HarleyBatDollAura) and PS_Explosion_SatchelAura1
//!     (PS_Harley_BatbombTimer) under the mesh, Team = SelectFloat(0.6, the colour index, local) @3035 / @3257: on the
//!     mesh's place of the latest tick (the view enters it under the component's name in its bones), hidden with the
//!     mesh (the carrier's InvisibilityCount), gone with the actor.
//!   HarleyJackInTheBox BalloonSpawn @5784: SpawnEmitterAttached(PS_HarleyBalloons, SkeletalMesh, socket Box) with
//!     Team @6129. Box: neither the mesh nor its skeleton has a socket of that name, so it is the bone "box" (names
//!     compare without case), read from the last drawn pose, with the bone's location alone (the system's rotation
//!     and scale are taken as the world's).
//! Not drawn here: the jack-in-the-box's jump input label and "Jumps Reset!" pop text (the HUD's).

use std::collections::HashMap;

use mvs_sim::World;
use mvs_sim::actors::ActorRef;
use mvs_sim::actors::world_actor::WorldActorState;
use mvs_sim::fighter::FighterId;
use mvs_vfx::SystemId;
use mvs_vfx::math::{IDENTITY, Rows};

use super::{CharFxHooks, ProjRule};
use crate::fx::Fx;
use crate::fx::attach::{att, warn_once};

const AURA_OF: &str = "BatBomb_GEN_VARIABLE";
const AURA: &str = "BombAuraVFX_GEN_VARIABLE";
/// OnFacingChange's literal: BatBomb's socket facing left.
const AURA_LEFT_SOCKET: &str = "fml_un_L_PropOffset";

/// The bomb's mesh component: its place is in the view's bones under this name (mvs-view chars/harley.rs BOMB_MESH).
const BOMB_MESH: &str = "HarleyQuinn_BatBomb_SkelMesh";
const BOMB_FX: [&str; 2] = ["PS_Explosion_SatchelAura", "PS_Explosion_SatchelAura1"];
const BALLOONS: &str = "PS_HarleyBalloons";
const BOX_BONE: &str = "box";

/// A Harley pawn's aura (`PawnComps.aura`).
#[derive(Debug)]
struct Aura {
    ps: String,
    tag: String,
    scale: f64,
    /// BatBomb's socket facing right, left.
    sockets: [String; 2],
    left: Option<bool>,
    h: Option<SystemId>,
}

/// What Harley Quinn's effects keep between steps.
#[derive(Debug, Default)]
pub(crate) struct Harley {
    /// Per Harley pawn (by pawn id): her aura, None where it is not drawn.
    auras: HashMap<i32, Option<Aura>>,
    /// Per bomb actor: its particle components.
    bombs: HashMap<ActorRef, Vec<SystemId>>,
    /// Per box actor whose BalloonSpawn ran: its balloons (None: not drawn).
    boxes: HashMap<ActorRef, Option<SystemId>>,
}

/// The aura's set-up for a Harley pawn (`pawnComps`' aura part).
fn aura_of(w: &World, f: FighterId) -> Option<Aura> {
    let pawn = &w.char_data(f).fighter.components.pawn;
    let bat = pawn.get(AURA_OF)?;
    let p = bat.p.as_obj()?;
    let socket = bat.socket.clone()?;
    let tag = p.get("ComponentTags")?.as_arr()?.first()?.as_str()?.to_lowercase();
    let scale = p.get("RelativeScale3D").and_then(|v| v.as_obj()).and_then(|o| o.get("X")).and_then(|v| v.as_f64());
    let ps = pawn.get(AURA)?.p.as_obj()?.get("Template")?.as_str()?.to_owned();
    Some(Aura {
        ps,
        tag,
        scale: scale.unwrap_or(1.0),
        sockets: [socket, AURA_LEFT_SOCKET.to_owned()],
        left: None,
        h: None,
    })
}

impl Harley {
    /// `placePawnComps`' aura: once a presentation tick.
    fn place_auras(&mut self, fx: &mut Fx, w: &World) {
        for fid in w.all_ids() {
            let fi = w.fighter(fid);
            if w.names.str(fi.ch) != mvs_sim::chars::harley::KEY {
                continue;
            }
            let id = fi.id;
            let a = self.auras.entry(id).or_insert_with(|| {
                let a = aura_of(w, fid);
                if a.is_none() {
                    eprintln!("warning: Harley: BombAuraVFX not drawn (BatBomb's or its template is not in the data)");
                }
                a
            });
            let Some(a) = a else { continue };
            let left = fi.body.facing_dir < 0.0;
            if a.left != Some(left) {
                a.left = Some(left);
                // (the aura rides BatBomb: the other socket's follow)
                if let Some(h) = a.h.take() {
                    fx.player.kill(h);
                }
            }
            let shown = w.char_data(fid).def.sub_meshes.contains(&a.tag)
                && !w.names.get(&a.tag).is_some_and(|n| fi.pose.sub_hidden.contains(&n));
            if fi.life.dead != 0.0 || !shown {
                if let Some(h) = a.h.take() {
                    fx.player.kill(h);
                }
                continue;
            }
            if a.h.is_none_or(|h| !fx.player.alive(h)) {
                let socket = a.sockets[usize::from(left)].clone();
                let team = fx.team(id);
                let s = a.scale;
                a.h = fx.attach_socket(w, id, &a.ps, &socket, [0.0; 3], &IDENTITY, [s; 3], &[("Team", team)]);
            }
            if let Some(h) = a.h {
                fx.player.set_visible(h, !fi.vis.invisible);
            }
        }
    }

    /// The bombs' particle components and the boxes' balloons (`harleyActors.place`'s effects).
    fn place_actors(&mut self, fx: &mut Fx, w: &World) {
        for (id, x) in w.actors.actors.iter() {
            if x.dead {
                continue;
            }
            let a = ActorRef::Actor(id);
            match &x.state {
                WorldActorState::HarleyBomb(b) => {
                    let Some(at) = bone_of(fx, a, BOMB_MESH).map(|(loc, rot, scale)| att(loc, &rot, Some(scale)))
                    else {
                        continue;
                    };
                    let hs = self.bombs.entry(a).or_insert_with(|| {
                        let team = fx.team(x.owner_id);
                        let from = mvs_sim::fighter::common::actor_def_owner(w, x.owner, x.cls);
                        let def = mvs_sim::fighter::common::actor_def(w, ActorRef::Fighter(from), x.cls);
                        let mut hs = Vec::new();
                        for n in BOMB_FX {
                            let c = def.and_then(|d| d.components.get(n)).and_then(|c| c.as_obj());
                            let Some(t) = c.and_then(|c| c.get("Template")).and_then(|t| t.as_str()) else {
                                eprintln!("warning: Harley: the bomb's {n} is not drawn (no template)");
                                continue;
                            };
                            let c = c.expect("a template's component");
                            if ["RelativeLocation", "RelativeRotation", "RelativeScale3D"]
                                .iter()
                                .any(|k| c.contains_key(*k))
                            {
                                eprintln!("warning: Harley: the bomb's {n} has a relative transform");
                            }
                            match fx.player.spawn_attached(t, &at, 1.0, &[("Team", team)]) {
                                Some(h) => hs.push(h),
                                None => {
                                    eprintln!("warning: Harley: the bomb's {n} is not drawn (its system is not loaded)")
                                }
                            }
                        }
                        hs
                    });
                    // (hidden with the mesh: the carrier's InvisibilityCount)
                    let carrier = mvs_sim::chars::harley::bomb::attached_to(w, x, b);
                    let shown = carrier.is_none_or(|c| !w.fighter(c).vis.invisible);
                    for &h in hs.iter() {
                        fx.player.attach(h, &at);
                        fx.player.set_visible(h, shown);
                    }
                }
                WorldActorState::JackBox(j) => {
                    if !j.balloons {
                        continue;
                    }
                    // the box's bone, else the actor's place
                    let loc = match bone_of(fx, a, BOX_BONE) {
                        Some((loc, _, _)) => loc,
                        None => {
                            warn_once(format!(
                                "Harley: the box mesh has no bone {BOX_BONE}: the balloons sit at the actor"
                            ));
                            [0.0, -j.x, j.y].map(f64::from)
                        }
                    };
                    let at = att(loc, &IDENTITY, None);
                    // (hadBalloons: the system is asked for once; an entry of None is one not drawn)
                    match self.boxes.get(&a) {
                        Some(&Some(h)) => fx.player.attach(h, &at),
                        Some(None) => {}
                        None => {
                            let team = fx.team(x.owner_id);
                            let h = fx.player.spawn_attached(BALLOONS, &at, 1.0, &[("Team", team)]);
                            if h.is_none() {
                                eprintln!("warning: Harley: {BALLOONS} is not loaded");
                            }
                            self.boxes.insert(a, h);
                        }
                    }
                }
                _ => {}
            }
        }
        // K2_DestroyActor takes the actor's components with it.
        let live = |a: &ActorRef| matches!(*a, ActorRef::Actor(id) if w.actors.actors.get(id).is_some_and(|x| !x.dead));
        let player = &mut fx.player;
        self.bombs.retain(|a, hs| {
            let keep = live(a);
            if !keep {
                for &h in hs.iter() {
                    player.kill(h);
                }
            }
            keep
        });
        self.boxes.retain(|a, h| {
            let keep = live(a);
            if !keep && let Some(h) = *h {
                player.kill(h);
            }
            keep
        });
    }

    fn clear_actors(&mut self, fx: &mut Fx) {
        for h in self.bombs.drain().flat_map(|(_, hs)| hs) {
            fx.player.kill(h);
        }
        for h in self.boxes.drain().filter_map(|(_, h)| h) {
            fx.player.kill(h);
        }
    }
}

/// A bone (or the bomb's mesh component) of an actor's view as last drawn, by name in any case: location, rows,
/// scale.
fn bone_of(fx: &Fx, a: ActorRef, name: &str) -> Option<([f64; 3], Rows, [f64; 3])> {
    let (_, x) = fx.actor_bones.get(&a)?.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))?;
    Some((x.loc, x.rot, x.scale))
}

impl CharFxHooks for Harley {
    fn actors(&mut self, fx: &mut Fx, w: &World) {
        self.place_actors(fx, w);
    }

    fn place(&mut self, fx: &mut Fx, w: &World) {
        self.place_auras(fx, w);
    }

    /// The Lab's reset: the world's actors are gone (the player's systems cleared).
    fn reset(&mut self, fx: &mut Fx) {
        self.clear_actors(fx);
        for a in self.auras.values_mut().flatten() {
            a.h = None;
        }
    }

    fn end(&mut self, fx: &mut Fx) {
        self.clear_actors(fx);
        for a in self.auras.values_mut().flatten() {
            if let Some(h) = a.h.take() {
                fx.player.kill(h);
            }
        }
    }
}

/// The classes' own effects, by class name.
pub(crate) const RULES: &[(&str, ProjRule)] = &[];
