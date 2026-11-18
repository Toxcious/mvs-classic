//! What the game builds from the store's entries, kept once per process: a typed value per key, shared by every
//! holder (`Arc`), built once even when two threads ask for it together, and let go least recently used first when
//! the cache is over its budget and nobody else holds the value.
//!
//! A value is immutable once built. Its weight (bytes, as the caller counts them) is what a budget counts; a value
//! of weight 0 is never let go by a trim.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type Value = Arc<dyn Any + Send + Sync>;

/// A key's slot: locked while its value is built, so a second asker waits for the first's value.
#[derive(Default)]
struct Slot {
    value: Mutex<Option<(Value, usize)>>,
    /// The cache's clock at the last ask.
    used: AtomicU64,
}

/// A value's key: its type and the caller's key.
type SlotKey = (TypeId, Box<str>);

/// A cache of values by (type, key).
#[derive(Default)]
pub struct Cache {
    slots: Mutex<HashMap<SlotKey, Arc<Slot>>>,
    clock: AtomicU64,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.stats();
        f.debug_struct("Cache").field("entries", &s.entries).field("bytes", &s.bytes).finish()
    }
}

/// What a cache holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: usize,
    /// Of `bytes`, the values nobody but the cache holds (what a trim may let go).
    pub unheld_bytes: usize,
}

impl Cache {
    pub fn new() -> Cache {
        Cache::default()
    }

    fn slot(&self, key: (TypeId, &str)) -> Arc<Slot> {
        let mut m = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(s) = m.get(&(key.0, Box::from(key.1))) {
            return s.clone();
        }
        let s = Arc::new(Slot::default());
        m.insert((key.0, Box::from(key.1)), s.clone());
        s
    }

    /// The value of `key`, built by `make` on the first ask (weight 0: never let go by a trim).
    pub fn get_or<T, E>(&self, key: &str, make: impl FnOnce() -> Result<T, E>) -> Result<Arc<T>, E>
    where
        T: Any + Send + Sync,
    {
        self.get_or_weighed(key, make, |_| 0)
    }

    /// The value of `key`, built by `make` on the first ask; `weight` its bytes for the budget. A failed build
    /// leaves no value: the next ask builds again.
    pub fn get_or_weighed<T, E>(
        &self,
        key: &str,
        make: impl FnOnce() -> Result<T, E>,
        weight: impl FnOnce(&T) -> usize,
    ) -> Result<Arc<T>, E>
    where
        T: Any + Send + Sync,
    {
        let slot = self.slot((TypeId::of::<T>(), key));
        slot.used.store(self.clock.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
        let mut v = slot.value.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((value, _)) = &*v {
            return Ok(value.clone().downcast::<T>().expect("a cache slot holds its key's type"));
        }
        let made = Arc::new(make()?);
        let w = weight(&made);
        *v = Some((made.clone(), w));
        Ok(made)
    }

    /// The value of `key` if it is built, without building it.
    pub fn peek<T: Any + Send + Sync>(&self, key: &str) -> Option<Arc<T>> {
        let m = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        let slot = m.get(&(TypeId::of::<T>(), Box::from(key)))?.clone();
        drop(m);
        let v = slot.value.try_lock().ok()?;
        v.as_ref().map(|(value, _)| value.clone().downcast::<T>().expect("a cache slot holds its key's type"))
    }

    /// What the cache holds now.
    pub fn stats(&self) -> CacheStats {
        let m = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        let mut s = CacheStats::default();
        for slot in m.values() {
            let Ok(v) = slot.value.try_lock() else { continue };
            if let Some((value, w)) = &*v {
                s.entries += 1;
                s.bytes += w;
                if Arc::strong_count(value) == 1 {
                    s.unheld_bytes += w;
                }
            }
        }
        s
    }

    /// Lets values go, least recently asked first, until the cache's weight is at most `budget` bytes; a value
    /// someone else holds stays (its memory would not be freed). The bytes let go.
    pub fn trim(&self, budget: usize) -> usize {
        let mut m = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        let mut total = 0;
        let mut unheld: Vec<(u64, SlotKey, usize)> = Vec::new();
        for (k, slot) in m.iter() {
            let Ok(v) = slot.value.try_lock() else { continue };
            if let Some((value, w)) = &*v {
                total += w;
                if *w > 0 && Arc::strong_count(value) == 1 {
                    unheld.push((slot.used.load(Ordering::Relaxed), k.clone(), *w));
                }
            }
        }
        unheld.sort_by_key(|u| u.0);
        let mut freed = 0;
        for (_, k, w) in unheld {
            if total <= budget {
                break;
            }
            m.remove(&k);
            total -= w;
            freed += w;
        }
        freed
    }

    /// Lets every value go (the holders keep theirs).
    pub fn clear(&self) {
        self.slots.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_once_and_trimmed_least_recent_first() {
        let c = Cache::new();
        let mut built = 0;
        let a = c
            .get_or_weighed::<Vec<u8>, ()>(
                "a",
                || {
                    built += 1;
                    Ok(vec![1; 10])
                },
                Vec::len,
            )
            .unwrap();
        let a2 = c.get_or_weighed::<Vec<u8>, ()>("a", || unreachable!(), Vec::len).unwrap();
        assert!(Arc::ptr_eq(&a, &a2));
        assert_eq!(built, 1);
        // (another type under the same key is another value)
        assert_eq!(*c.get_or::<u32, ()>("a", || Ok(7)).unwrap(), 7);
        c.get_or_weighed::<Vec<u8>, ()>("b", || Ok(vec![2; 20]), Vec::len).unwrap();
        c.get_or_weighed::<Vec<u8>, ()>("c", || Ok(vec![3; 30]), Vec::len).unwrap();
        assert_eq!(c.stats(), CacheStats { entries: 4, bytes: 60, unheld_bytes: 50 });
        // "a" is held: "b" goes first, then "c"
        assert_eq!(c.trim(40), 20);
        assert!(c.peek::<Vec<u8>>("b").is_none() && c.peek::<Vec<u8>>("c").is_some());
        drop((a, a2));
        assert_eq!(c.trim(0), 40);
        assert_eq!(c.stats().entries, 1, "the weightless value stays");
        assert!(c.get_or::<u8, &str>("x", || Err("no")).is_err());
        assert_eq!(*c.get_or::<u8, &str>("x", || Ok(1)).unwrap(), 1, "a failed build leaves no value");
    }

    #[test]
    fn two_threads_build_once() {
        let c = Arc::new(Cache::new());
        let n = Arc::new(AtomicU64::new(0));
        let t: Vec<_> = (0..4)
            .map(|_| {
                let (c, n) = (c.clone(), n.clone());
                std::thread::spawn(move || {
                    c.get_or::<u64, ()>("k", || {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        Ok(n.fetch_add(1, Ordering::SeqCst))
                    })
                    .unwrap()
                })
            })
            .collect();
        let got: Vec<u64> = t.into_iter().map(|h| *h.join().unwrap()).collect();
        assert_eq!(got, [0, 0, 0, 0]);
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }
}
