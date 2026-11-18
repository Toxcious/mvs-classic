//! The desync hash: [`StateHash`], one walk of a type's rollback state, written by `#[derive(State)]` for the sim's
//! own types and by hand below for the leaves. It is the desync hash's job (one number per tick that two
//! peers compare), not a hash other implementations share: the Rust peers compare with each other only.
//!
//! A float is hashed by its bits (the sim is deterministic to the bit, so two peers in step hash the same bits; a
//! NaN's payload and a -0 count). A sequence is hashed with its length first, an `Option` with its tag.

use indexmap::{IndexMap, IndexSet};
use std::hash::Hash;

/// FNV-1a over 64 bits, fed whole words: cheap, no allocation, the same on every machine.
#[derive(Clone, Debug)]
pub struct StateHasher(u64);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

impl Default for StateHasher {
    fn default() -> Self {
        StateHasher(FNV_OFFSET)
    }
}

impl StateHasher {
    #[inline]
    pub fn write_u64(&mut self, v: u64) {
        self.0 = (self.0 ^ v).wrapping_mul(FNV_PRIME);
    }
    #[inline]
    pub fn write_u32(&mut self, v: u32) {
        self.write_u64(u64::from(v));
    }
    #[inline]
    pub fn write_len(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
    pub fn write_bytes(&mut self, b: &[u8]) {
        self.write_len(b.len());
        for c in b.chunks(8) {
            let mut w = [0u8; 8];
            w[..c.len()].copy_from_slice(c);
            self.write_u64(u64::from_le_bytes(w));
        }
    }
    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// One value's part of the desync hash.
pub trait StateHash {
    fn hash_state(&self, h: &mut StateHasher);
}

/// The hash of one value from a fresh hasher.
pub fn hash_of<T: StateHash + ?Sized>(v: &T) -> u64 {
    let mut h = StateHasher::default();
    v.hash_state(&mut h);
    h.finish()
}

macro_rules! by_u64 {
    ($($t:ty),*) => {$(
        impl StateHash for $t {
            #[inline]
            #[allow(
                clippy::cast_sign_loss,
                clippy::cast_possible_wrap,
                clippy::cast_possible_truncation,
                reason = "the hash reads the value's bits, sign-extended to 64"
            )]
            fn hash_state(&self, h: &mut StateHasher) {
                h.write_u64(*self as u64);
            }
        }
    )*};
}
by_u64!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize, bool);

impl StateHash for f64 {
    #[inline]
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_u64(self.to_bits());
    }
}
impl StateHash for f32 {
    #[inline]
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_u32(self.to_bits());
    }
}
impl StateHash for () {
    fn hash_state(&self, _: &mut StateHasher) {}
}
impl StateHash for str {
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_bytes(self.as_bytes());
    }
}
impl StateHash for String {
    fn hash_state(&self, h: &mut StateHasher) {
        self.as_str().hash_state(h);
    }
}
/// A code literal held as state (a bound function's `"<Class>.<Function>"`).
impl StateHash for &'static str {
    fn hash_state(&self, h: &mut StateHasher) {
        (**self).hash_state(h);
    }
}
impl<T: StateHash + ?Sized> StateHash for Box<T> {
    #[inline]
    fn hash_state(&self, h: &mut StateHasher) {
        (**self).hash_state(h);
    }
}
impl<T: StateHash> StateHash for Option<T> {
    #[inline]
    fn hash_state(&self, h: &mut StateHasher) {
        match self {
            None => h.write_u32(0),
            Some(v) => {
                h.write_u32(1);
                v.hash_state(h);
            }
        }
    }
}
impl<T: StateHash> StateHash for [T] {
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_len(self.len());
        for v in self {
            v.hash_state(h);
        }
    }
}
impl<T: StateHash, const N: usize> StateHash for [T; N] {
    fn hash_state(&self, h: &mut StateHasher) {
        self.as_slice().hash_state(h);
    }
}
impl<T: StateHash> StateHash for Vec<T> {
    #[inline]
    fn hash_state(&self, h: &mut StateHasher) {
        self.as_slice().hash_state(h);
    }
}
impl<A: StateHash, B: StateHash> StateHash for (A, B) {
    fn hash_state(&self, h: &mut StateHasher) {
        self.0.hash_state(h);
        self.1.hash_state(h);
    }
}
impl<A: StateHash, B: StateHash, C: StateHash> StateHash for (A, B, C) {
    fn hash_state(&self, h: &mut StateHasher) {
        self.0.hash_state(h);
        self.1.hash_state(h);
        self.2.hash_state(h);
    }
}
/// In insertion order: the order the step iterates them in, which is part of the state.
impl<K: StateHash + Hash + Eq, V: StateHash, S> StateHash for IndexMap<K, V, S> {
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_len(self.len());
        for (k, v) in self {
            k.hash_state(h);
            v.hash_state(h);
        }
    }
}
impl<T: StateHash + Hash + Eq, S> StateHash for IndexSet<T, S> {
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_len(self.len());
        for v in self {
            v.hash_state(h);
        }
    }
}

impl StateHash for mvs_collision::Ground {
    fn hash_state(&self, h: &mut StateHasher) {
        (self.left, self.right, (self.bottom, self.top)).hash_state(h);
    }
}
impl StateHash for mvs_anim::montage::Started {
    fn hash_state(&self, h: &mut StateHasher) {
        h.write_u32(*self as u32);
    }
}
impl<K: StateHash> StateHash for mvs_anim::montage::MontageState<K> {
    fn hash_state(&self, h: &mut StateHasher) {
        let mvs_anim::montage::MontageState {
            montage,
            pos,
            ended,
            active,
            in_advance,
            terminated,
            blend_out_fired,
            proxy,
            interrupt_fired,
            play_rate,
            walking,
            started,
            age,
            at_end,
            blend_left,
            blend_time,
        } = self;
        montage.hash_state(h);
        pos.hash_state(h);
        ended.hash_state(h);
        active.hash_state(h);
        in_advance.hash_state(h);
        terminated.hash_state(h);
        blend_out_fired.hash_state(h);
        proxy.hash_state(h);
        interrupt_fired.hash_state(h);
        play_rate.hash_state(h);
        walking.hash_state(h);
        started.hash_state(h);
        age.hash_state(h);
        at_end.hash_state(h);
        blend_left.hash_state(h);
        blend_time.hash_state(h);
    }
}

// The data's branch rows a fighter's state holds (a queued taunt, an override pack's rows).
impl StateHash for mvs_data::fighter::Branch {
    fn hash_state(&self, h: &mut StateHasher) {
        self.attack.hash_state(h);
        self.button.hash_state(h);
        self.direction.hash_state(h);
        self.conditions.hash_state(h);
        self.priority.hash_state(h);
        self.ability_slot.hash_state(h);
        self.allow_pre_attack_rotation.hash_state(h);
        self.display_name.hash_state(h);
        self.unique_id.hash_state(h);
    }
}
impl StateHash for mvs_data::fighter::AttackEntry {
    fn hash_state(&self, h: &mut StateHasher) {
        self.required_button_input.hash_state(h);
        self.required_direction_input.hash_state(h);
        self.attack_montage.hash_state(h);
        self.ability_slot.hash_state(h);
        self.display_name.hash_state(h);
        self.conditions.hash_state(h);
        self.allow_pre_attack_rotation.hash_state(h);
    }
}

impl<T: StateHash + ?Sized> StateHash for std::sync::Arc<T> {
    #[inline]
    fn hash_state(&self, h: &mut StateHasher) {
        (**self).hash_state(h);
    }
}

/// A data value an actor holds as its own (a class graph's write over its class defaults): its variant, then its
/// contents.
impl StateHash for mvs_data::value::Value {
    fn hash_state(&self, h: &mut StateHasher) {
        use mvs_data::value::Value;
        match self {
            Value::Null => h.write_u32(0),
            Value::Bool(b) => {
                h.write_u32(1);
                b.hash_state(h);
            }
            Value::Num(n) => {
                h.write_u32(2);
                n.hash_state(h);
            }
            Value::Str(s) => {
                h.write_u32(3);
                s.hash_state(h);
            }
            Value::Arr(a) => {
                h.write_u32(4);
                a.hash_state(h);
            }
            Value::Obj(o) => {
                h.write_u32(5);
                o.hash_state(h);
            }
            Value::F32s(a) => {
                h.write_u32(6);
                a.hash_state(h);
            }
        }
    }
}
