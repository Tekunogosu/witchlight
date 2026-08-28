//! Encoded tiles, kept until the room runs out.
//!
//! This used to be a map that only ever grew: nothing left it but a tile that
//! changed, so a service left running while people explored held every tile
//! anyone had ever looked at, at every level, at about a hundred kilobytes each.
//! That is a leak with a slow fuse rather than a tuning problem.

use std::collections::HashMap;

/// Which tile, at which level.
pub type At = (u32, i32, i32);

pub struct Cache {
    held: HashMap<At, (Vec<u8>, u64)>,
    bytes: usize,
    budget: usize,
    clock: u64,
}

impl Cache {
    #[must_use]
    pub fn new(budget: usize) -> Self {
        Self { held: HashMap::new(), bytes: 0, budget, clock: 0 }
    }

    pub fn get(&mut self, at: &At) -> Option<Vec<u8>> {
        self.clock += 1;
        let clock = self.clock;
        let (bytes, used) = self.held.get_mut(at)?;
        *used = clock;
        Some(bytes.clone())
    }

    pub fn insert(&mut self, at: At, bytes: Vec<u8>) {
        self.clock += 1;
        self.bytes += bytes.len();
        if let Some((old, _)) = self.held.insert(at, (bytes, self.clock)) {
            self.bytes -= old.len();
        }
        self.evict();
    }

    /// Forgets everything, for a palette that has recoloured every tile there is.
    pub fn clear(&mut self) {
        self.held.clear();
        self.bytes = 0;
    }

    pub fn remove(&mut self, at: &At) {
        if let Some((old, _)) = self.held.remove(at) {
            self.bytes -= old.len();
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

        let mut by_age: Vec<(At, u64)> =
            self.held.iter().map(|(at, (_, used))| (*at, *used)).collect();
        by_age.sort_unstable_by_key(|(_, used)| *used);

        for (at, _) in by_age {
            if self.bytes <= self.budget {
                break;
            }
            self.remove(&at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(bytes: usize) -> Vec<u8> {
        vec![7u8; bytes]
    }

    #[test]
    fn a_tile_comes_back_out_the_way_it_went_in() {
        let mut cache = Cache::new(1024);
        cache.insert((0, 1, 2), tile(10));
        assert_eq!(cache.get(&(0, 1, 2)), Some(tile(10)));
        assert_eq!(cache.get(&(0, 9, 9)), None, "one that was never put in");
    }

    #[test]
    fn levels_are_separate_tiles() {
        let mut cache = Cache::new(1024);
        cache.insert((0, 1, 1), tile(10));
        cache.insert((3, 1, 1), tile(20));
        assert_eq!(cache.get(&(0, 1, 1)).map(|t| t.len()), Some(10));
        assert_eq!(cache.get(&(3, 1, 1)).map(|t| t.len()), Some(20));
    }

    #[test]
    fn it_stays_inside_its_budget() {
        let mut cache = Cache::new(100);
        for at in 0..50 {
            cache.insert((0, at, 0), tile(30));
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
        cache.insert((0, 1, 0), tile(10));
        cache.insert((0, 2, 0), tile(10));
        cache.insert((0, 3, 0), tile(10));

        assert!(cache.get(&(0, 1, 0)).is_some(), "used again, so most recent");

        cache.insert((0, 4, 0), tile(10));
        assert!(cache.get(&(0, 1, 0)).is_some(), "kept, because it was used");
        assert!(cache.get(&(0, 2, 0)).is_none(), "dropped, because it was not");
        assert!(cache.get(&(0, 3, 0)).is_some());
        assert!(cache.get(&(0, 4, 0)).is_some());
    }

    #[test]
    fn replacing_a_tile_does_not_count_it_twice() {
        let mut cache = Cache::new(1000);
        cache.insert((0, 1, 0), tile(100));
        cache.insert((0, 1, 0), tile(40));
        assert_eq!(cache.bytes, 40, "the tile it replaced must stop counting");
        assert_eq!(cache.held.len(), 1);
    }

    #[test]
    fn removing_and_clearing_give_the_room_back() {
        let mut cache = Cache::new(1000);
        cache.insert((0, 1, 0), tile(100));
        cache.insert((0, 2, 0), tile(100));
        cache.remove(&(0, 1, 0));
        assert_eq!(cache.bytes, 100);
        cache.clear();
        assert_eq!(cache.bytes, 0);
        assert!(cache.held.is_empty());
    }

    #[test]
    fn a_tile_larger_than_the_whole_budget_does_not_wedge_it() {
        let mut cache = Cache::new(50);
        cache.insert((0, 1, 0), tile(500));
        // Nothing is left to evict, so it is over budget with one tile — but it
        // must not spin trying, and the next insert must still work.
        cache.insert((0, 2, 0), tile(10));
        assert!(cache.held.contains_key(&(0, 2, 0)));
    }
}
