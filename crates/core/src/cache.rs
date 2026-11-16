//! Tiny bounded in-memory cache (oldest-inserted evicted first).
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

pub struct Cache<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    cap: usize,
}

impl<K: Hash + Eq + Clone, V> Cache<K, V> {
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.map.get(k)
    }

    pub fn insert(&mut self, k: K, v: V) {
        if self.map.insert(k.clone(), v).is_none() {
            self.order.push_back(k);
            if self.map.len() > self.cap
                && let Some(old) = self.order.pop_front()
            {
                self.map.remove(&old);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_oldest_and_updates_in_place() {
        let mut c = Cache::new(2);
        c.insert("a", 1);
        c.insert("b", 2);
        c.insert("a", 10); // update, not a new entry
        assert_eq!(c.get(&"a"), Some(&10));
        c.insert("c", 3); // evicts "a" (oldest inserted)
        assert_eq!(c.get(&"a"), None);
        assert_eq!(c.get(&"b"), Some(&2));
        assert_eq!(c.get(&"c"), Some(&3));
    }
}
