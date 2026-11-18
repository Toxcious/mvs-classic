//! The two sides of a frame's conversion: [`Wx`], what the writer reads beside the World (the montages of the data,
//! the oracle ids of the World's objects), and [`Lx`], what the loader carries while it writes a World (the names it
//! interns, the World's objects by oracle id, what it could not set).

use std::cell::RefCell;
use std::fmt::Write as _;

use indexmap::IndexMap;
use mvs_data::GameData;
use mvs_data::fighter::{Montage, Notify};
use mvs_sim::World;
use mvs_sim::actors::{ActorId, ActorRef, ItemId, ProjectileId};
use mvs_sim::events::NotifyRef;
use mvs_sim::fighter::FighterId;
use mvs_sim::names::{Name, Names};

/// A montage of the data as the record needs it: its blend-in and its sections, for `MontageRecord.section` and
/// `weight` (`MontagePlayer.sectionIndexAt`, `weight`).
pub struct MontageInfo {
    pub blend_in: f64,
    /// Each section's name and start (seconds).
    pub sections: Vec<(String, f64)>,
}

/// The data the trace names by identity: the montages by name and every notify by its identity (`notifyId`: class,
/// inst, t, track, seq, slot), to its montage (or sequence) and index. Built once per match from its data.
#[derive(Default)]
pub struct Book {
    pub montages: IndexMap<String, MontageInfo>,
    /// The notify lists by montage or sequence name: a `NotifyRef`'s targets.
    pub notifies: IndexMap<String, Vec<NotifyId>>,
    /// Each notify identity to the first (montage, index) that has it.
    by_id: IndexMap<NotifyId, (String, u32)>,
    /// The identities more than one notify of the data has: the loader takes the first (or the one already there).
    pub ambiguous: usize,
    /// The notifies that make a ProjectileAimer (a Spawner notify with AimOn, AnimNotify_AimItemThrow) by (montage,
    /// index), with the aimer's `p` as the record writes it: how an aimer's notify is found again from its `p`.
    pub aims: IndexMap<(String, u32), mvs_trace::Value>,
    /// The AnimNotify_C015SpitFighter_C notifies by (montage, index), with the Hit TazState.spitHit holds, as the
    /// record writes it: a `spit_hit` is a notify of the data, found again from this value.
    pub spit_hits: IndexMap<(String, u32), mvs_trace::Value>,
    /// The AnimNotify_PopText notifies by (montage, index), with their properties as a `popText` event writes them
    /// (`PopParams::Notify`): how the event's notify is found again from its `params`.
    pub pop_texts: IndexMap<(String, u32), mvs_trace::Value>,
    /// Each character's montages and sequences by name (an aimer's notify is one of its fighter's own).
    pub char_owners: IndexMap<String, indexmap::IndexSet<String>>,
    /// Each projectile the writer has seen live, to its oracle id: a reference to one gone from the list (a live one's
    /// `hitActors` can still hold it) is written by that id, as the oracle writes the dead object's.
    pub projectile_seqs: RefCell<IndexMap<ProjectileId, i32>>,
    /// The same for items and world actors (a fighter's `recentHitActor` can hold one gone).
    pub item_seqs: RefCell<IndexMap<ItemId, i32>>,
    pub actor_seqs: RefCell<IndexMap<ActorId, i32>>,
    /// The last value each buff entry a state holds whole was written with, by (fighter, serial): the oracle keeps the
    /// object after its end, so one gone from its fighter's list is written from it, ended (frames 0, swept).
    pub held_buffs: RefCell<IndexMap<(FighterId, u32), mvs_trace::Value>>,
}

/// AnimNotify_AimItemThrow_C: its aimer's `p` is the AimInfo its begin builds, not the notify's properties.
pub const AIM_THROW: &str = "AnimNotify_AimItemThrow_C";

/// The `p` of the aimer notify `n` makes, as the record writes it (`None`: no aimer notify).
fn aim_p(n: &Notify) -> Option<mvs_trace::Value> {
    use mvs_sim::registry::props::{flag, num_or};
    if n.cls == AIM_THROW {
        let p = &n.p;
        let info = mvs_sim::fighter::match_side::ThrowAimInfo {
            max: num_or(p, "Aim Angle Max", 45.0),
            min: num_or(p, "Aim Angle Min", -45.0),
            speed: num_or(p, "Aim Speed", 1000.0),
            input_rotation: num_or(p, "Input Rotation Offset", 0.0),
        };
        return Some(throw_aim_p(&info));
    }
    flag(&n.p, "AimOn").then(|| crate::parts::static_put(&n.p))
}

/// A throw aimer's `p`: `{AimInfo}`.
pub fn throw_aim_p(i: &mvs_sim::fighter::match_side::ThrowAimInfo) -> mvs_trace::Value {
    use mvs_trace::Value::Num;
    crate::conv::obj(vec![(
        "AimInfo",
        crate::conv::obj(vec![
            ("AimAngleMax", Num(f64::from(i.max))),
            ("AimAngleMin", Num(f64::from(i.min))),
            ("AimSpeed", Num(f64::from(i.speed))),
            ("InputRotationOffset", Num(f64::from(i.input_rotation))),
        ]),
    )])
}

/// A notify's identity as the trace writes it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NotifyId {
    pub cls: String,
    pub inst: Option<String>,
    /// `t`'s bits.
    pub t: u64,
    pub track: i32,
    pub seq: Option<String>,
    pub slot: Option<String>,
}

impl NotifyId {
    fn of(n: &Notify) -> Self {
        NotifyId {
            cls: n.cls.clone(),
            inst: n.inst.clone(),
            t: n.t.to_bits(),
            track: n.track,
            seq: n.seq.clone(),
            slot: n.slot.clone(),
        }
    }
}

impl Book {
    pub fn new(data: &GameData) -> Self {
        let mut b = Book::default();
        let add_montage = |b: &mut Book, m: &Montage| {
            if b.montages.contains_key(&m.name) {
                return;
            }
            let sections = m.sections.iter().map(|s| (s.name.clone(), s.t)).collect();
            b.montages.insert(m.name.clone(), MontageInfo { blend_in: m.blend_in, sections });
            b.add_notifies(&m.name, &m.notifies);
        };
        for (key, ch) in &data.chars {
            let f = &ch.fighter;
            let owners = f
                .montages
                .keys()
                .chain(f.actor_montages.iter().flat_map(|a| a.keys()))
                .chain(f.seq_notifies.iter().flat_map(|a| a.keys()))
                .cloned()
                .collect();
            b.char_owners.insert(key.clone(), owners);
            for m in f.montages.values() {
                add_montage(&mut b, m);
            }
            for m in f.actor_montages.iter().flat_map(|a| a.values()) {
                add_montage(&mut b, m);
            }
            for (k, list) in f.seq_notifies.iter().flat_map(|a| a.iter()) {
                if !b.notifies.contains_key(k) {
                    b.add_notifies(k, list);
                }
            }
        }
        // The items part's montages (the drumstick chicken's, a polymorph with no Taz in the match).
        for m in data.items.iter().flat_map(|i| i.montages.values().chain(i.actor_montages.values())) {
            add_montage(&mut b, m);
        }
        // Stripe's body hit boxes (HitBoxesOn of the bomb component) hold a notify of no montage: the oracle builds one
        // of the class Comp_C023B_Bomb_C, the port's reference names it as an owner.
        let id = NotifyId {
            cls: "Comp_C023B_Bomb_C".to_owned(),
            inst: None,
            t: 0f64.to_bits(),
            track: 0,
            seq: None,
            slot: None,
        };
        b.by_id.insert(id.clone(), ("Comp_C023B_Bomb_C".to_owned(), 0));
        b.notifies.insert("Comp_C023B_Bomb_C".to_owned(), vec![id]);
        b
    }

    fn add_notifies(&mut self, owner: &str, list: &[Notify]) {
        for (i, n) in list.iter().enumerate() {
            if let Some(p) = aim_p(n) {
                self.aims.insert((owner.to_owned(), u32::try_from(i).expect("a notify index")), p);
            }
            if n.cls == "AnimNotify_PopText_C" {
                let p = crate::parts::data_put(&mvs_data::value::Value::Obj(n.p.clone()));
                self.pop_texts.insert((owner.to_owned(), u32::try_from(i).expect("a notify index")), p);
            }
            if n.cls == "AnimNotify_C015SpitFighter_C" {
                self.spit_hits.insert(
                    (owner.to_owned(), u32::try_from(i).expect("a notify index")),
                    crate::taz::state::spit_hit_value(n),
                );
            }
        }
        let ids: Vec<NotifyId> = list.iter().map(NotifyId::of).collect();
        for (i, id) in ids.iter().enumerate() {
            if self.by_id.contains_key(id) {
                self.ambiguous += 1;
            } else {
                self.by_id.insert(id.clone(), (owner.to_owned(), u32::try_from(i).expect("a notify index")));
            }
        }
        self.notifies.insert(owner.to_owned(), ids);
    }

    /// The (montage, index) of the first notify of the data with this identity.
    pub fn find(&self, id: &NotifyId) -> Option<(&str, u32)> {
        self.by_id.get(id).map(|(m, i)| (m.as_str(), *i))
    }

    /// The identity of the notify `index` of `owner`.
    pub fn notify(&self, owner: &str, index: u32) -> Option<&NotifyId> {
        self.notifies.get(owner)?.get(index as usize)
    }
}

/// What the writer reads: the World and the book. `unmapped` collects the values with a place in the record that the
/// writer has no rule for (written as null or a stand-in: the frame is not what the oracle would write); `unrecorded`
/// the Rust-only state the record has no place for.
pub struct Wx<'a> {
    pub w: &'a World,
    pub book: &'a Book,
    pub unmapped: RefCell<Vec<String>>,
    pub unrecorded: RefCell<Vec<String>>,
    /// The gone actors being written whole: one its own hit set reaches again (two gone items that hit each other)
    /// is written there by reference.
    pub whole: RefCell<Vec<mvs_sim::actors::ActorRef>>,
}

impl<'a> Wx<'a> {
    pub fn new(w: &'a World, book: &'a Book) -> Self {
        let mut seqs = book.projectile_seqs.borrow_mut();
        // The live actors, and the ones the step removed (a projectile spawned and removed within one step is never
        // live at a frame's end, but a sibling's `hit_actors` can still name it).
        for (id, p) in w.actors.projectiles.iter() {
            seqs.insert(id, p.seq);
        }
        for g in &w.actors.gone_motion {
            seqs.entry(g.id).or_insert(g.seq);
        }
        drop(seqs);
        let mut seqs = book.item_seqs.borrow_mut();
        for (id, i) in w.actors.items.iter() {
            seqs.insert(id, i.seq);
        }
        for (id, i) in &w.actors.gone_items {
            seqs.entry(*id).or_insert(i.seq);
        }
        drop(seqs);
        let mut seqs = book.actor_seqs.borrow_mut();
        for (id, a) in w.actors.actors.iter() {
            seqs.insert(id, a.seq);
        }
        for (id, a) in &w.actors.gone_actors {
            seqs.entry(*id).or_insert(a.seq);
        }
        drop(seqs);
        Wx {
            w,
            book,
            unmapped: RefCell::new(Vec::new()),
            unrecorded: RefCell::new(Vec::new()),
            whole: RefCell::new(Vec::new()),
        }
    }

    pub fn str(&self, n: Name) -> &'a str {
        self.w.names.str(n)
    }

    /// A fighter's oracle id (`Fighter.id`), -1 for a stale one.
    pub fn fid(&self, id: FighterId) -> i32 {
        self.w.fighters.get(id).map_or(-1, |f| f.id)
    }

    /// An actor's family (the oracle World list `setTick` finds it in) and its oracle id (`seq`, a fighter's `id`).
    pub fn aref(&self, r: ActorRef) -> (&'static str, f64) {
        let a = &self.w.actors;
        match r {
            ActorRef::Fighter(id) => ("fighter", f64::from(self.fid(id))),
            ActorRef::Projectile(id) => match a.projectiles.get(id) {
                Some(p) => ("projectile", f64::from(p.seq)),
                None => match self.book.projectile_seqs.borrow().get(&id) {
                    Some(&seq) => ("spawned", f64::from(seq)),
                    None => self.stale("a projectile"),
                },
            },
            ActorRef::Item(id) => match a.items.get(id) {
                Some(i) => ("item", f64::from(i.seq)),
                None => match self.book.item_seqs.borrow().get(&id) {
                    Some(&seq) => ("spawned", f64::from(seq)),
                    None => self.stale("an item"),
                },
            },
            ActorRef::Actor(id) => match a.actors.get(id) {
                Some(x) => ("actors", f64::from(x.seq)),
                None => match self.book.actor_seqs.borrow().get(&id) {
                    Some(&seq) => ("spawned", f64::from(seq)),
                    None => self.stale("an actor"),
                },
            },
            ActorRef::Marker(id) => match a.markers.get(id) {
                Some(m) => ("markers", f64::from(m.seq)),
                None => self.stale("a marker"),
            },
            ActorRef::Ship(id) => match a.ships.get(id) {
                Some(s) => ("ships", f64::from(s.seq)),
                None => self.stale("a ship"),
            },
            ActorRef::Mortar(id) => match a.mortars.get(id) {
                Some(m) => ("mortars", f64::from(m.seq)),
                None => self.stale("a mortar aim"),
            },
            ActorRef::AimPunch(id) => match a.aim_punches.get(id) {
                Some(p) => ("aimPunches", f64::from(p.seq)),
                None => self.stale("an aim punch"),
            },
            ActorRef::Accelerator(id) => match a.accelerators.get(id) {
                Some(c) => ("accelerators", f64::from(c.seq)),
                None => self.stale("a portal"),
            },
            // A stage actor by its index (`StageActor.index`, its place in `StageState::actors`).
            ActorRef::Stage(i) => ("stage", f64::from(i)),
            // A buff actor (a PreviousAttacker the oracle never writes): the port's own key, -2 - (its wearer's oracle
            // id x 1000 + its index in the wearer's buffs), below every actor id and the -1 of none; an ended one is
            // -1, as the build's reference to a destroyed actor reads invalid.
            ActorRef::Buff(b) => {
                let Some(f) = self.w.fighters.get(b.fighter) else { return ("buff", -1.0) };
                let Some(i) = f.buffs.buffs.iter().position(|x| x.serial == b.serial) else { return ("buff", -1.0) };
                #[allow(clippy::cast_precision_loss, reason = "a list index")]
                let k = f64::from(f.id) * 1000.0 + i as f64;
                ("buff", -2.0 - k)
            }
        }
    }

    // Deviation: an actor gone from its list is the oracle 'spawned' family with its id; the Rust reference keeps no id
    // once the slot is freed, so one the writer never saw live or in the World's gone lists (a projectile, item or
    // world actor is found in `Book.projectile_seqs`, `item_seqs`, `actor_seqs`) is written -1 and named.
    fn stale(&self, what: &str) -> (&'static str, f64) {
        self.unmapped(&format!("a reference to {what} no longer in its list"));
        ("spawned", -1.0)
    }

    pub fn unrecorded(&self, what: &str) {
        let mut u = self.unrecorded.borrow_mut();
        if !u.iter().any(|x| x == what) {
            u.push(what.to_owned());
        }
    }

    pub fn unmapped(&self, what: &str) {
        let mut u = self.unmapped.borrow_mut();
        if !u.iter().any(|x| x == what) {
            u.push(what.to_owned());
        }
    }

    /// A notify reference's identity, or `None` (named) where the book has no such notify.
    pub fn notify(&self, n: NotifyRef) -> Option<&'a NotifyId> {
        let owner = self.str(n.montage);
        let id = self.book.notify(owner, n.index);
        if id.is_none() {
            self.unmapped(&format!("a notify of the data {owner}[{}]", n.index));
        }
        id
    }
}

/// The object of oracle id `id` in an id list.
fn find_id<T: Copy>(list: &[(i32, T)], id: f64) -> Option<T> {
    list.iter().find(|(t, _)| f64::from(*t) == id).map(|(_, x)| *x)
}

/// What the loader carries: the World's names (shared, copied on the first name it adds, put back at the end), the
/// objects by oracle id, the path it is at and what it could not set.
pub struct Lx<'a> {
    pub names: std::sync::Arc<Names>,
    pub book: &'a Book,
    pub fighters: Vec<(i32, FighterId)>,
    pub projectiles: Vec<(i32, ProjectileId)>,
    pub items: Vec<(i32, ItemId)>,
    pub actors: Vec<(i32, ActorId)>,
    /// Morty's typed lists by oracle id.
    pub markers: Vec<(i32, mvs_sim::actors::MarkerId)>,
    pub ships: Vec<(i32, mvs_sim::actors::ShipId)>,
    pub mortars: Vec<(i32, mvs_sim::actors::MortarId)>,
    pub aim_punches: Vec<(i32, mvs_sim::actors::AimPunchId)>,
    pub accelerators: Vec<(i32, mvs_sim::actors::AcceleratorId)>,
    /// The mortar aims gone from their list that a hit list names, loaded whole: oracle id, the id given, the aim.
    pub gone_mortars: Vec<(i32, mvs_sim::actors::MortarId, mvs_sim::actors::lists::MortarAim)>,
    /// The items gone from their list that a Finn aimer names, loaded whole: oracle id, the id given, the item.
    pub gone_items: Vec<(i32, mvs_sim::actors::ItemId, mvs_sim::actors::item::Item)>,
    pub gone_actors: Vec<(i32, mvs_sim::actors::ActorId, mvs_sim::actors::world_actor::WorldActor)>,
    /// The stage's actors (`StageState::actors`) the World holds: a "stage" reference's index is below it.
    pub stage_actors: usize,
    /// The frame's `stageRun.boxes` as written: a fighter's base on a stage solid's box is the box of this list it
    /// equals (its place is the box's in `StageState::boxes`).
    pub stage_boxes: Vec<mvs_trace::Value>,
    /// The frame's `runtimeSolids` as written, each one's `[l0, r0, t0, b0]`: a held solid is the entry of this list
    /// whose box it equals, its spec number the place's (`tutorial::spec_at`).
    pub runtime_places: Vec<[f64; 4]>,
    /// The boxes of the projectiles gone from their list that a fighter's base names, loaded whole (the World's
    /// `Actors::gone_boxes`): the id given, the oracle id, the boxes by index.
    pub gone_boxes: Vec<(ProjectileId, i32, Vec<mvs_sim::actors::projectile::ProjectileBox>)>,
    /// The boxes of the world actors gone from their list that a fighter's base names, loaded whole (the World's
    /// `Actors::gone_plats`): the actor's id, the parts by index.
    pub gone_plats: Vec<(ActorId, Vec<mvs_sim::world::spawned_platform::PlatformPart>)>,
    /// A live world actor's box a fighter's platform reference names, by its flags (wall, ignored, trace, nav, item):
    /// the part is found once the actors are loaded (`fighter::resolve_actor_plats`).
    pub pending_plats: Vec<(bool, i32, bool, bool, bool)>,
    pub path: String,
    pub misses: Vec<String>,
    /// The character of the fighter being loaded (its data key), while its record loads.
    pub ch: Option<String>,
}

impl<'a> Lx<'a> {
    pub fn new(w: &World, book: &'a Book) -> Self {
        let mut c = Lx {
            // (a copy of the table per load was most of a run's time: an input's load makes one per slot and tick)
            names: std::sync::Arc::clone(&w.names),
            book,
            fighters: Vec::new(),
            projectiles: Vec::new(),
            items: Vec::new(),
            actors: Vec::new(),
            markers: Vec::new(),
            ships: Vec::new(),
            mortars: Vec::new(),
            aim_punches: Vec::new(),
            accelerators: Vec::new(),
            gone_mortars: Vec::new(),
            gone_items: Vec::new(),
            gone_actors: Vec::new(),
            stage_actors: w.stage_run.as_ref().map_or(0, |r| r.actors.len()),
            stage_boxes: Vec::new(),
            runtime_places: Vec::new(),
            gone_boxes: Vec::new(),
            gone_plats: Vec::new(),
            pending_plats: Vec::new(),
            path: String::new(),
            misses: Vec::new(),
            ch: None,
        };
        c.index(w);
        c
    }

    /// The World's objects by oracle id, again after the lists changed.
    pub fn index(&mut self, w: &World) {
        self.fighters = w.fighters.iter().map(|(id, f)| (f.id, id)).collect();
        self.projectiles = w.actors.projectiles.iter().map(|(id, p)| (p.seq, id)).collect();
        self.items = w.actors.items.iter().map(|(id, i)| (i.seq, id)).collect();
        self.actors = w.actors.actors.iter().map(|(id, a)| (a.seq, id)).collect();
        self.markers = w.actors.markers.iter().map(|(id, m)| (m.seq, id)).collect();
        self.ships = w.actors.ships.iter().map(|(id, s)| (s.seq, id)).collect();
        self.mortars = w.actors.mortars.iter().map(|(id, m)| (m.seq, id)).collect();
        self.aim_punches = w.actors.aim_punches.iter().map(|(id, p)| (p.seq, id)).collect();
        self.accelerators = w.actors.accelerators.iter().map(|(id, c)| (c.seq, id)).collect();
    }

    pub fn name(&mut self, s: &str) -> Name {
        match self.names.get(s) {
            Some(n) => n,
            None => std::sync::Arc::make_mut(&mut self.names).intern(s),
        }
    }

    pub fn str(&self, n: Name) -> &str {
        self.names.str(n)
    }

    pub fn miss(&mut self, what: impl std::fmt::Display) {
        let mut m = String::new();
        let _ = write!(m, "{}: {what}", if self.path.is_empty() { "frame" } else { &self.path });
        if !self.misses.contains(&m) {
            self.misses.push(m);
        }
    }

    /// Runs `f` with `part` pushed on the path.
    pub fn at<T>(&mut self, part: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        let at = self.path.len();
        if !self.path.is_empty() && !part.starts_with('[') {
            self.path.push('.');
        }
        self.path.push_str(part);
        let r = f(self);
        self.path.truncate(at);
        r
    }

    pub fn fighter(&mut self, id: f64) -> Option<FighterId> {
        let found = self.fighters.iter().find(|(t, _)| f64::from(*t) == id).map(|(_, f)| *f);
        if found.is_none() && id >= 0.0 {
            self.miss(format_args!("no fighter of id {id}"));
        }
        found
    }

    /// An actor reference by its family and oracle id.
    pub fn aref(&mut self, family: &str, id: f64) -> Option<ActorRef> {
        let r = match family {
            "fighter" => find_id(&self.fighters, id).map(ActorRef::Fighter),
            "projectile" => find_id(&self.projectiles, id).map(ActorRef::Projectile),
            "item" => find_id(&self.items, id).map(ActorRef::Item),
            "actors" => find_id(&self.actors, id).map(ActorRef::Actor),
            "markers" => find_id(&self.markers, id).map(ActorRef::Marker),
            "ships" => find_id(&self.ships, id).map(ActorRef::Ship),
            "mortars" => find_id(&self.mortars, id).map(ActorRef::Mortar),
            "aimPunches" => find_id(&self.aim_punches, id).map(ActorRef::AimPunch),
            "accelerators" => find_id(&self.accelerators, id).map(ActorRef::Accelerator),
            "stage" => {
                let n = self.stage_actors;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let i = id as u32;
                (id >= 0.0 && f64::from(i) == id && (i as usize) < n).then_some(ActorRef::Stage(i))
            }
            // A projectile, item or world actor gone from its list: an id of it the writer saw live and no longer is
            // (a stale id; the three share the oracle's actorSeq).
            "spawned" => {
                let p = self
                    .book
                    .projectile_seqs
                    .borrow()
                    .iter()
                    .find(|&(p, &seq)| f64::from(seq) == id && !self.projectiles.iter().any(|(_, q)| q == p))
                    .map(|(&p, _)| ActorRef::Projectile(p));
                let iseqs = self.book.item_seqs.borrow();
                let i = || {
                    iseqs
                        .iter()
                        .find(|&(i, &seq)| f64::from(seq) == id && !self.items.iter().any(|(_, q)| q == i))
                        .map(|(&i, _)| ActorRef::Item(i))
                };
                let aseqs = self.book.actor_seqs.borrow();
                let a = || {
                    aseqs
                        .iter()
                        .find(|&(a, &seq)| f64::from(seq) == id && !self.actors.iter().any(|(_, q)| q == a))
                        .map(|(&a, _)| ActorRef::Actor(a))
                };
                let found = p.or_else(i).or_else(a);
                drop(iseqs);
                drop(aseqs);
                found.or_else(|| self.mint_gone(id))
            }
            _ => None,
        };
        if r.is_none() {
            self.miss(format_args!("no {family} of id {id}"));
        }
        r
    }

    /// An id for a projectile, item or world actor gone from its list that the book never saw (one spawned and
    /// removed within a step the World was not stepped through): a projectile id no arena slot holds, its oracle id in
    /// `Book.projectile_seqs`, the same id for the same oracle id on every later load.
    // Unverified: the record names no family for a gone actor; it is taken as a projectile. Nothing in the sim reads
    // a gone actor's family but its being gone.
    fn mint_gone(&self, id: f64) -> Option<ActorRef> {
        #[allow(clippy::cast_possible_truncation)]
        let seq = id as i32;
        if f64::from(seq) != id || seq < 0 {
            return None;
        }
        let mut seqs = self.book.projectile_seqs.borrow_mut();
        let n = u32::try_from(seqs.len()).ok()?;
        let p = ProjectileId::new(u32::MAX - n, u32::MAX);
        seqs.insert(p, seq);
        Some(ActorRef::Projectile(p))
    }

    /// A notify identity to its reference: `keep` where it already names it (the data can hold one identity twice).
    pub fn notify(&mut self, id: &NotifyId, keep: Option<NotifyRef>) -> Option<NotifyRef> {
        if let Some(k) = keep {
            let owner = self.names.str(k.montage).to_owned();
            if self.book.notify(&owner, k.index) == Some(id) {
                return Some(k);
            }
        }
        let book = self.book;
        match book.find(id) {
            Some((m, index)) => Some(NotifyRef { montage: self.name(m), index }),
            None => {
                self.miss(format_args!("no notify {} {:?} of the data", id.cls, id.inst));
                None
            }
        }
    }
}
