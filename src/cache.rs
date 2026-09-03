//! Encoded tiles, kept until the room runs out.
//!
//! This used to be a map that only ever grew: nothing left it but a tile that
//! changed, so a service left running while people explored held every tile
//! anyone had ever looked at, at every level, at about a hundred kilobytes each.
//! That is a leak with a slow fuse rather than a tuning problem.

use std::collections::HashMap;
use std::sync::Arc;

/// Which tile, at which level.
pub type At = (u32, i32, i32);

/// Whose tile: empty for the one everybody shares, and a key naming whose view
/// it was composed for otherwise. What the key says is the caller's business;
/// what matters here is that two readers' tiles never answer for each other.
pub type Whose = String;

pub struct Cache {
    /// Shared rather than copied out: a hit hands the caller the same bytes
    /// the cache holds, so serving a tile costs no copy of it.
    held: HashMap<(Whose, At), (Arc<[u8]>, u64)>,
    bytes: usize,
    budget: usize,
    clock: u64,
}

impl Cache {
    #[must_use]
    pub fn new(budget: usize) -> Self {
        Self { held: HashMap::new(), bytes: 0, budget, clock: 0 }
    }

    pub fn get(&mut self, whose: &str, at: &At) -> Option<Arc<[u8]>> {
        self.clock += 1;
        let clock = self.clock;
        let (bytes, used) = self.held.get_mut(&(whose.to_owned(), *at))?;
        *used = clock;
        Some(Arc::clone(bytes))
    }

    pub fn insert(&mut self, whose: Whose, at: At, bytes: Arc<[u8]>) {
        self.clock += 1;
        self.bytes += bytes.len();
        if let Some((old, _)) = self.held.insert((whose, at), (bytes, self.clock)) {
            self.bytes -= old.len();
        }
        self.evict();
    }

    /// Forgets everything, for a palette that has recoloured every tile there is.
    pub fn clear(&mut self) {
        self.held.clear();
        self.bytes = 0;
    }

    /// Forgets one tile, whoever it was drawn for: the ground under it moved,
    /// and every picture of that ground with it.
    pub fn remove(&mut self, at: &At) {
        self.remove_where(|_, held| held == at);
    }

    /// Forgets every tile the predicate names.
    pub fn remove_where(&mut self, mut gone: impl FnMut(&str, &At) -> bool) {
        let keys: Vec<(Whose, At)> =
            self.held.keys().filter(|(whose, at)| gone(whose, at)).cloned().collect();
        for key in keys {
            if let Some((old, _)) = self.held.remove(&key) {
                self.bytes -= old.len();
            }
        }
    }

    /// Drops the tiles nobody has asked for in longest, until there is room.
    ///
    /// Least recently used rather than oldest: a map has a few squares everyone
    /// looks at and a long tail nobody returns to, and evicting by age alone
    /// would throw away the ones being used.
    fn evict(&mut self) {
        if self.bytes <= self.budget {
            return;
        }

        let mut by_age: Vec<((Whose, At), u64)> =
            self.held.iter().map(|(key, (_, used))| (key.clone(), *used)).collect();
        by_age.sort_unstable_by_key(|(_, used)| *used);

        for (key, _) in by_age {
            if self.bytes <= self.budget {
                break;
            }
            if let Some((old, _)) = self.held.remove(&key) {
                self.bytes -= old.len();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(bytes: usize) -> Arc<[u8]> {
        vec![7u8; bytes].into()
    }

    #[test]
    fn a_tile_comes_back_out_the_way_it_went_in() {
        let mut cache = Cache::new(1024);
        cache.insert(String::new(), (0, 1, 2), tile(10));
        assert_eq!(cache.get("", &(0, 1, 2)), Some(tile(10)));
        assert_eq!(cache.get("", &(0, 9, 9)), None, "one that was never put in");
    }

    #[test]
    fn whose_tile_it_is_is_part_of_the_key_and_a_tile_gone_is_gone_for_everybody() {
        let mut cache = Cache::new(1024);
        cache.insert(String::new(), (0, 1, 1), tile(10));
        cache.insert("m:ada".to_owned(), (0, 1, 1), tile(20));
        cache.insert("m:ada".to_owned(), (0, 2, 2), tile(30));
        assert_eq!(cache.get("", &(0, 1, 1)).map(|t| t.len()), Some(10));
        assert_eq!(cache.get("m:ada", &(0, 1, 1)).map(|t| t.len()), Some(20));
        assert_eq!(cache.get("m:bob", &(0, 1, 1)), None, "Bob was never drawn one");

        cache.remove(&(0, 1, 1));
        assert_eq!(cache.get("", &(0, 1, 1)), None);
        assert_eq!(cache.get("m:ada", &(0, 1, 1)), None, "Ada's copy of moved ground goes too");
        assert!(cache.get("m:ada", &(0, 2, 2)).is_some(), "and her other tile stays");

        cache.remove_where(|whose, _| whose.contains("ada"));
        assert_eq!(cache.get("m:ada", &(0, 2, 2)), None);
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn levels_are_separate_tiles() {
        let mut cache = Cache::new(1024);
        cache.insert(String::new(), (0, 1, 1), tile(10));
        cache.insert(String::new(), (3, 1, 1), tile(20));
        assert_eq!(cache.get("", &(0, 1, 1)).map(|t| t.len()), Some(10));
        assert_eq!(cache.get("", &(3, 1, 1)).map(|t| t.len()), Some(20));
    }

    #[test]
    fn it_stays_inside_its_budget() {
        let mut cache = Cache::new(100);
        for at in 0..50 {
            cache.insert(String::new(), (0, at, 0), tile(30));
            assert!(
                cache.bytes <= 100,
                "held {} bytes after {} tiles, budget is 100",
                cache.bytes,
                at + 1
            );
        }
        assert!(cache.held.len() < 50, "something must have been dropped");
    }

    #[test]
    fn what_is_dropped_is_what_nobody_asked_for() {
        // Room for three. The first is used again, so the second should go before
        // it — dropping by age alone would take the one still being looked at.
        let mut cache = Cache::new(30);
        cache.insert(String::new(), (0, 1, 0), tile(10));
        cache.insert(String::new(), (0, 2, 0), tile(10));
        cache.insert(String::new(), (0, 3, 0), tile(10));

        assert!(cache.get("", &(0, 1, 0)).is_some(), "used again, so most recent");

        cache.insert(String::new(), (0, 4, 0), tile(10));
        assert!(cache.get("", &(0, 1, 0)).is_some(), "kept, because it was used");
        assert!(cache.get("", &(0, 2, 0)).is_none(), "dropped, because it was not");
        assert!(cache.get("", &(0, 3, 0)).is_some());
        assert!(cache.get("", &(0, 4, 0)).is_some());
    }

    #[test]
    fn replacing_a_tile_does_not_count_it_twice() {
        let mut cache = Cache::new(1000);
        cache.insert(String::new(), (0, 1, 0), tile(100));
        cache.insert(String::new(), (0, 1, 0), tile(40));
        assert_eq!(cache.bytes, 40, "the tile it replaced must stop counting");
        assert_eq!(cache.held.len(), 1);
    }

    #[test]
    fn removing_and_clearing_give_the_room_back() {
        let mut cache = Cache::new(1000);
        cache.insert(String::new(), (0, 1, 0), tile(100));
        cache.insert(String::new(), (0, 2, 0), tile(100));
        cache.remove(&(0, 1, 0));
        assert_eq!(cache.bytes, 100);
        cache.clear();
        assert_eq!(cache.bytes, 0);
        assert!(cache.held.is_empty());
    }

    #[test]
    fn a_tile_larger_than_the_whole_budget_does_not_wedge_it() {
        let mut cache = Cache::new(50);
        cache.insert(String::new(), (0, 1, 0), tile(500));
        // Nothing is left to evict, so it is over budget with one tile — but it
        // must not spin trying, and the next insert must still work.
        cache.insert(String::new(), (0, 2, 0), tile(10));
        assert!(cache.held.contains_key(&(String::new(), (0, 2, 0))));
    }
}
