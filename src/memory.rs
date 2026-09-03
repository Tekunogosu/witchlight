//! What each person remembers of the map.
//!
//! Under a private map, nobody is shown the world: they are shown the world as
//! they last saw it. Two facts per person make that up, and both live here and
//! in the database behind [`crate::store`]:
//!
//! - **Discovered** chunks: everything that has ever been within sight of where
//!   they were standing. One bit per chunk, thirty-two bytes per region.
//! - **Divergences**: discovered chunks that changed while they were not there,
//!   and the version they last saw. A chunk they have discovered and have no
//!   divergence for is a chunk they see as it is now — because every change to
//!   a discovered chunk either happened in sight, or left a divergence.
//!
//! So a player leaving spawn keeps spawn as it was, and a player coming back
//! has the divergence cleared and sees it as it is. The old version stays in the
//! database as long as anybody points at it, and no longer.
//!
//! Sharing is a choice each person makes per group. A person who shares with a
//! group hands their memory to every member of it, and a member reads the union:
//! their own memory and everything shared with them, where a chunk anybody has
//! seen as it is now wins, and among remembered versions the newest does. Which
//! groups exist and who is in them is the mod's to say; who shares with which is
//! in each person's own settings.
//!
//! Everything here is keyed by uid and changes on two beats: the position report
//! every couple of seconds, which discovers; and terrain arriving, which
//! diverges. A change to one person's memory is announced under the map's own
//! generation clock, on the same beat as everything else — see
//! [`crate::state::State::announce`], which files it here with [`Memory::record`]
//! — so a browser polling with `since` learns which tiles its own view changed
//! in, and nobody else's.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::history::History;
use crate::log::warn;
use crate::store::{self, BITSET_BYTES, Divergence, Store, Stored, Version};

/// One player group, as the mod says it: what it is called and who is in it,
/// online or not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub members: HashSet<String>,
}

/// One region as a view remembers it — see [`Memory::shown_in`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegionMemory {
    /// Which chunks are discovered, or nothing where no source has been near.
    pub discovered: Option<[u8; BITSET_BYTES]>,
    /// The discovered chunks shown as a remembered version rather than as they
    /// are; every other discovered chunk is current.
    pub remembered: HashMap<(i32, i32), Version>,
}

/// Whose memory a reader is shown: their own, plus everyone who shares with a
/// group they are in. Empty for a reader with no session, whose map is the
/// spawn disc alone — see [`Memory::view`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub sources: Vec<String>,
}

impl View {
    /// Whether this view has any memory at all to draw from.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

#[derive(Default)]
struct Inner {
    /// Discovered chunks, by uid and then by region.
    discovered: HashMap<String, HashMap<(i32, i32), [u8; BITSET_BYTES]>>,
    /// Remembered versions, by uid and then by chunk.
    divergences: HashMap<String, HashMap<(i32, i32), Version>>,
    /// What each person can see right now, as of the last position report.
    in_sight: HashMap<String, HashSet<(i32, i32)>>,
    /// Every group, by the id the game gives it.
    groups: HashMap<i32, Group>,
    /// Which groups each person shares their memory with.
    shares: HashMap<String, HashSet<i32>>,
    /// Which regions each person's memory changed in, by generation.
    history: HashMap<String, History<Vec<(i32, i32)>>>,
}

pub struct Memory {
    store: Arc<Store>,
    inner: Mutex<Inner>,
}

impl Memory {
    /// Reads back everything a previous run kept.
    #[must_use]
    pub fn load(store: Arc<Store>) -> Self {
        let mut inner = Inner::default();

        match store.discovered() {
            Ok(rows) => {
                for row in rows {
                    inner.discovered.entry(row.uid).or_default().insert((row.rx, row.rz), row.bits);
                }
            }
            Err(error) => warn!("could not read what has been discovered: {error}"),
        }
        match store.divergences() {
            Ok(rows) => {
                for row in rows {
                    inner.divergences.entry(row.uid).or_default().insert((row.cx, row.cz), row.version);
                }
            }
            Err(error) => warn!("could not read the divergences: {error}"),
        }

        Self { store, inner: Mutex::new(inner) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Takes what somebody can see right now. Every chunk in sight is
    /// discovered, and any divergence for one is cleared: they are looking at
    /// it, so what they remember is what is there.
    ///
    /// Answers which regions this person's memory changed in, or nothing. The
    /// caller announces it — see [`record`](Self::record).
    pub fn saw(&self, uid: &str, sight: &[(i32, i32)]) -> Option<Vec<(i32, i32)>> {
        if uid.is_empty() {
            return None;
        }

        let mut inner = self.lock();
        inner.in_sight.insert(uid.to_owned(), sight.iter().copied().collect());

        let mut regions_changed: HashSet<(i32, i32)> = HashSet::new();
        let mut cleared: Vec<(i32, i32)> = Vec::new();

        let discovered = inner.discovered.entry(uid.to_owned()).or_default();
        for &(cx, cz) in sight {
            let region = store::region_of(cx, cz);
            let bits = discovered.entry(region).or_insert([0u8; BITSET_BYTES]);
            if store::set_bit(bits, store::slot_of(cx, cz)) {
                regions_changed.insert(region);
            }
        }

        if let Some(remembered) = inner.divergences.get_mut(uid) {
            for &chunk in sight {
                if remembered.remove(&chunk).is_some() {
                    cleared.push(chunk);
                    regions_changed.insert(store::region_of(chunk.0, chunk.1));
                }
            }
        }

        if regions_changed.is_empty() {
            return None;
        }

        // Written before it is announced, so a browser told to repaint a tile is
        // never shown one drawn from a memory the next start will not have.
        for &region in &regions_changed {
            if let Some(bits) = inner.discovered.get(uid).and_then(|held| held.get(&region))
                && let Err(error) = self.store.set_discovered(uid, region.0, region.1, bits)
            {
                warn!("could not record what {uid} discovered: {error}");
            }
        }
        if let Err(error) = self.store.clear_divergences(uid, &cleared) {
            warn!("could not clear what {uid} remembered: {error}");
        }

        let mut regions: Vec<(i32, i32)> = regions_changed.into_iter().collect();
        regions.sort_unstable();
        Some(regions)
    }

    /// Takes chunks that just changed. Everybody who has discovered one and is
    /// not looking at it keeps the version they last saw.
    ///
    /// Answers whose memory changed and in which regions, so the caller can
    /// say so to each of them.
    pub fn changed(&self, stored: &[Stored]) -> Vec<(String, Vec<(i32, i32)>)> {
        let mut inner = self.lock();
        let mut diverged: Vec<Divergence> = Vec::new();
        let mut by_uid: HashMap<String, HashSet<(i32, i32)>> = HashMap::new();

        for one in stored.iter().filter(|one| one.surface_moved()) {
            let Some(was) = one.was else { continue };
            let chunk = (one.cx, one.cz);
            let region = store::region_of(one.cx, one.cz);
            let slot = store::slot_of(one.cx, one.cz);

            let absent: Vec<String> = inner
                .discovered
                .iter()
                .filter(|(_, held)| held.get(&region).is_some_and(|bits| store::bit(bits, slot)))
                .filter(|(uid, _)| !inner.in_sight.get(*uid).is_some_and(|seen| seen.contains(&chunk)))
                .filter(|(uid, _)| !inner.divergences.get(*uid).is_some_and(|held| held.contains_key(&chunk)))
                .map(|(uid, _)| uid.clone())
                .collect();

            for uid in absent {
                inner.divergences.entry(uid.clone()).or_default().insert(chunk, was);
                by_uid.entry(uid.clone()).or_default().insert(region);
                diverged.push(Divergence { uid, cx: one.cx, cz: one.cz, version: was });
            }
        }

        if diverged.is_empty() {
            return Vec::new();
        }

        if let Err(error) = self.store.set_divergences(&diverged) {
            warn!("could not record {} divergences: {error}", diverged.len());
        }

        let mut whose = Vec::with_capacity(by_uid.len());
        for (uid, regions) in by_uid {
            let mut regions: Vec<(i32, i32)> = regions.into_iter().collect();
            regions.sort_unstable();
            whose.push((uid, regions));
        }
        whose
    }

    /// Files that one person's memory changed in these regions at
    /// `generation`, which is what [`changes_since`](Self::changes_since)
    /// answers from. Called by whoever turned the clock.
    pub fn record(&self, uid: &str, generation: u64, regions: Vec<(i32, i32)>) {
        let mut inner = self.lock();
        inner.history.entry(uid.to_owned()).or_default().record(generation, regions, std::time::Instant::now());
    }

    /// Takes the groups as the mod says them.
    pub fn set_groups(&self, groups: HashMap<i32, Group>) {
        self.lock().groups = groups;
    }

    /// Takes which groups one person shares their memory with.
    pub fn set_shares(&self, uid: &str, groups: impl IntoIterator<Item = i32>) {
        self.lock().shares.insert(uid.to_owned(), groups.into_iter().collect());
    }

    /// The groups one person is in, by id and name, for a settings form to offer.
    #[must_use]
    pub fn groups_of(&self, uid: &str) -> Vec<(i32, String)> {
        let inner = self.lock();
        let mut found: Vec<(i32, String)> = inner
            .groups
            .iter()
            .filter(|(_, group)| group.members.contains(uid))
            .map(|(&id, group)| (id, group.name.clone()))
            .collect();
        found.sort();
        found
    }

    /// Whose memory a reader is shown: their own, and that of everyone who
    /// shares with a group the reader is in. Nobody's, for a reader with no
    /// session.
    #[must_use]
    pub fn view(&self, uid: Option<&str>) -> View {
        let Some(uid) = uid.filter(|uid| !uid.is_empty()) else {
            return View::default();
        };

        let inner = self.lock();
        let mut sources = vec![uid.to_owned()];
        for (sharer, groups) in &inner.shares {
            if sharer == uid {
                continue;
            }
            let shared_with_me = groups
                .iter()
                .any(|group| inner.groups.get(group).is_some_and(|group| group.members.contains(uid)));
            if shared_with_me {
                sources.push(sharer.clone());
            }
        }
        sources.sort();
        View { sources }
    }

    /// Which chunks of one region a view has discovered, as the union of its
    /// sources, or nothing where none of them has been near it.
    #[must_use]
    pub fn discovered_in(&self, view: &View, region: (i32, i32)) -> Option<[u8; BITSET_BYTES]> {
        let inner = self.lock();
        let mut union: Option<[u8; BITSET_BYTES]> = None;
        for source in &view.sources {
            if let Some(bits) = inner.discovered.get(source).and_then(|held| held.get(&region)) {
                let into = union.get_or_insert([0u8; BITSET_BYTES]);
                for (byte, other) in into.iter_mut().zip(bits) {
                    *byte |= other;
                }
            }
        }
        union
    }

    /// Every region a view has anything discovered in.
    #[must_use]
    pub fn regions_of(&self, view: &View) -> HashSet<(i32, i32)> {
        let inner = self.lock();
        view.sources
            .iter()
            .filter_map(|source| inner.discovered.get(source))
            .flat_map(|held| held.keys().copied())
            .collect()
    }

    /// Which version of a chunk a view is shown, or nothing where it is shown
    /// the chunk as it is — one chunk of what [`shown_in`](Self::shown_in)
    /// answers for a region, and read from that so there is one reading of it.
    ///
    /// A chunk no source has discovered is also nothing here: it is not drawn
    /// at all, which [`discovered_in`](Self::discovered_in) decides.
    #[cfg(test)]
    pub fn remembered(&self, view: &View, chunk: (i32, i32)) -> Option<Version> {
        self.shown_in(view, store::region_of(chunk.0, chunk.1)).remembered.get(&chunk).copied()
    }

    /// What one whole region is to a view, in one reading: which of its chunks
    /// are discovered, and for each of those which version is shown, as
    /// [`remembered`](Self::remembered) would answer chunk by chunk. One lock
    /// for the region rather than one per chunk — a tile at the coarsest level
    /// holds tens of thousands of chunks, and asking about each in turn was
    /// most of what drawing one cost.
    #[must_use]
    pub fn shown_in(&self, view: &View, region: (i32, i32)) -> RegionMemory {
        let inner = self.lock();
        let mut discovered: Option<[u8; BITSET_BYTES]> = None;
        for source in &view.sources {
            if let Some(bits) = inner.discovered.get(source).and_then(|held| held.get(&region)) {
                let into = discovered.get_or_insert([0u8; BITSET_BYTES]);
                for (byte, other) in into.iter_mut().zip(bits) {
                    *byte |= other;
                }
            }
        }
        let Some(discovered) = discovered else {
            return RegionMemory::default();
        };

        let mut remembered = HashMap::new();
        for chunk in crate::columns::chunks_of(region) {
            let slot = store::slot_of(chunk.0, chunk.1);
            if !store::bit(&discovered, slot) {
                continue;
            }
            let mut newest: Option<Version> = None;
            let mut current = false;
            for source in &view.sources {
                let seen = inner
                    .discovered
                    .get(source)
                    .and_then(|held| held.get(&region))
                    .is_some_and(|bits| store::bit(bits, slot));
                if !seen {
                    continue;
                }
                match inner.divergences.get(source).and_then(|held| held.get(&chunk)) {
                    None => {
                        current = true;
                        break;
                    }
                    Some(&version) => newest = Some(newest.map_or(version, |held| held.max(version))),
                }
            }
            if !current && let Some(version) = newest {
                remembered.insert(chunk, version);
            }
        }
        RegionMemory { discovered: Some(discovered), remembered }
    }

    /// Which regions a view's memory changed in since generation `since`, or
    /// nothing where some source has fallen further behind than is remembered.
    #[must_use]
    pub fn changes_since(&self, view: &View, since: u64) -> Option<Vec<(i32, i32)>> {
        let inner = self.lock();
        let mut regions = Vec::new();
        for source in &view.sources {
            let Some(history) = inner.history.get(source) else { continue };
            for changed in history.since(since)? {
                regions.extend(changed.iter().copied());
            }
        }
        regions.sort_unstable();
        regions.dedup();
        Some(regions)
    }

    /// Drops every stored version nothing points at any more. On a slow beat,
    /// because a person walking back into changed ground clears divergences a
    /// few at a time and one pass a minute is plenty.
    pub fn collect(&self) {
        match self.store.collect_garbage() {
            Ok(0) | Err(_) => {}
            Ok(freed) => crate::log::say!("{freed} remembered chunk versions freed"),
        }
    }

    /// How many people have a memory, and how many divergences there are
    /// between them, for the log and for `witchlight status`.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        let inner = self.lock();
        (inner.discovered.len(), inner.divergences.values().map(HashMap::len).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Arrived;
    use std::time::SystemTime;

    fn record(edge: usize, block: u16) -> Vec<u8> {
        let mut record = Vec::with_capacity(edge * edge * 6);
        for index in 0..edge * edge {
            record.extend_from_slice(&block.to_le_bytes());
            record.extend_from_slice(&(index as i16).to_le_bytes());
            record.push(80);
            record.push(90);
        }
        record
    }

    /// A map with one chunk at spawn, and the memory over it.
    fn spawn() -> (Arc<Store>, Memory, Stored) {
        let store = Arc::new(Store::in_memory());
        let first = store
            .put_chunks(2, &[Arrived { cx: 0, cz: 0, season: 1, record: record(2, 11) }], SystemTime::now())
            .unwrap()[0];
        let memory = Memory::load(Arc::clone(&store));
        (store, memory, first)
    }

    /// Ground at spawn changes, and says what the map did.
    fn build(store: &Store, block: u16) -> Stored {
        store
            .put_chunks(2, &[Arrived { cx: 0, cz: 0, season: 1, record: record(2, block) }], SystemTime::now())
            .unwrap()[0]
    }

    fn only(uid: &str) -> View {
        View { sources: vec![uid.to_owned()] }
    }

    /// The story the design was told with. Ada and Bob stand at spawn, go
    /// their separate ways, Cass builds a house, and Ada comes back.
    #[test]
    fn spawn_is_remembered_as_it_was_until_somebody_returns() {
        let (store, memory, first) = spawn();
        let mut clock = 1;

        // Both at spawn: both discover it, and neither remembers anything else.
        assert_eq!(memory.saw("ada", &[(0, 0)]), Some(vec![(0, 0)]));
        assert!(memory.saw("bob", &[(0, 0)]).is_some());
        assert!(memory.saw("ada", &[(0, 0)]).is_none(), "seeing it again changes nothing");
        assert_eq!(memory.remembered(&only("ada"), (0, 0)), None, "Ada sees spawn as it is");

        // Ada goes west, Bob goes east.
        clock += 1;
        memory.saw("ada", &[(-9, 0)]);
        memory.saw("bob", &[(9, 0)]);

        // Cass logs in at spawn and builds a house.
        clock += 1;
        memory.saw("cass", &[(0, 0)]);
        let built = build(&store, 22);
        assert_eq!(built.was, Some(first.now));
        clock += 1;
        let mut whose: Vec<String> = memory.changed(&[built]).into_iter().map(|(uid, _)| uid).collect();
        whose.sort();
        assert_eq!(whose, vec!["ada", "bob"], "the two who left keep what they saw; Cass is looking at it");

        assert_eq!(memory.remembered(&only("ada"), (0, 0)), Some(first.now));
        assert_eq!(memory.remembered(&only("bob"), (0, 0)), Some(first.now));
        assert_eq!(memory.remembered(&only("cass"), (0, 0)), None);
        assert_eq!(memory.counts(), (3, 2));
        assert_eq!(store.counts().unwrap().versions, 2, "the old spawn is kept because two people point at it");

        // Cass builds again while everybody is where they were: Ada and Bob
        // still remember the first spawn, not the second.
        let again = build(&store, 33);
        clock += 1;
        assert!(memory.changed(&[again]).is_empty(), "nobody's memory moves twice");
        assert_eq!(memory.remembered(&only("ada"), (0, 0)), Some(first.now));

        // Ada walks back. Her memory is reconciled; Bob's is not.
        clock += 1;
        let told = memory.saw("ada", &[(0, 0)]).expect("her memory moved");
        memory.record("ada", clock, told);
        assert_eq!(memory.remembered(&only("ada"), (0, 0)), None, "Ada sees the house");
        assert_eq!(memory.remembered(&only("bob"), (0, 0)), Some(first.now), "Bob still does not");
        assert_eq!(memory.counts(), (3, 1));

        // Ada was told about her own change, under the clock it happened at.
        assert_eq!(memory.changes_since(&only("ada"), clock - 1), Some(vec![(0, 0)]));
        assert_eq!(memory.changes_since(&only("ada"), clock), Some(vec![]));

        // The first spawn stays while Bob remembers it; the middle one, which
        // nobody ever pointed at, goes. Everything but the current goes once
        // Bob returns.
        memory.collect();
        assert_eq!(store.counts().unwrap().versions, 2);
        memory.saw("bob", &[(0, 0)]);
        memory.collect();
        assert_eq!(store.counts().unwrap().versions, 1, "only the current spawn is left");
    }

    #[test]
    fn what_is_remembered_survives_a_restart() {
        let (store, memory, first) = spawn();
        memory.saw("ada", &[(0, 0), (1, 0)]);
        memory.saw("ada", &[(40, 40)]);
        let built = build(&store, 22);
        memory.changed(&[built]);

        let again = Memory::load(Arc::clone(&store));
        let view = only("ada");
        assert_eq!(again.remembered(&view, (0, 0)), Some(first.now));
        let bits = again.discovered_in(&view, (0, 0)).expect("the region is known");
        assert!(store::bit(&bits, store::slot_of(0, 0)));
        assert!(store::bit(&bits, store::slot_of(1, 0)));
        assert!(!store::bit(&bits, store::slot_of(2, 0)));
        assert!(again.discovered_in(&view, (2, 2)).is_some(), "chunk (40, 40) is in region (2, 2)");
        assert_eq!(again.regions_of(&view).len(), 2);
    }

    #[test]
    fn a_chunk_never_discovered_is_never_remembered() {
        let (store, memory, _) = spawn();
        memory.saw("ada", &[(5, 5)]);
        let built = build(&store, 22);
        assert!(memory.changed(&[built]).is_empty());
        assert_eq!(memory.remembered(&only("ada"), (0, 0)), None);
        assert!(memory.discovered_in(&only("ada"), (0, 0)).is_none_or(|bits| !store::bit(&bits, 0)));
    }

    /// Sharing: Bob shares with a group Ada is in. Ada is shown the union, and
    /// where the two disagree the fresher memory wins.
    #[test]
    fn a_shared_memory_is_read_as_one_and_the_fresher_wins() {
        let (store, memory, first) = spawn();
        let mut groups = HashMap::new();
        groups.insert(7, Group { name: "the guild".into(), members: ["ada", "bob"].map(String::from).into_iter().collect() });
        memory.set_groups(groups);

        // Bob has seen spawn and chunk (3, 3); Ada has seen nothing.
        memory.saw("bob", &[(0, 0), (3, 3)]);
        assert_eq!(memory.view(Some("ada")).sources, vec!["ada"], "nobody shares with Ada yet");

        memory.set_shares("bob", [7]);
        let ada = memory.view(Some("ada"));
        assert_eq!(ada.sources, vec!["ada", "bob"]);
        assert_eq!(memory.view(Some("bob")).sources, vec!["bob"], "sharing is one way");
        assert!(memory.view(None).is_empty(), "a stranger has nobody's memory");
        assert_eq!(memory.groups_of("ada"), vec![(7, "the guild".to_owned())]);

        let bits = memory.discovered_in(&ada, (0, 0)).expect("Bob's discoveries reach Ada");
        assert!(store::bit(&bits, store::slot_of(3, 3)));

        // Bob leaves, spawn changes: Bob remembers the old spawn, and so does
        // Ada through him.
        memory.saw("bob", &[(30, 30)]);
        let built = build(&store, 22);
        memory.changed(&[built]);
        assert_eq!(memory.remembered(&ada, (0, 0)), Some(first.now));

        // Ada walks to spawn herself: she has seen it as it is, and that beats
        // Bob's memory in her own view — and in nobody else's.
        memory.saw("ada", &[(0, 0)]);
        assert_eq!(memory.remembered(&ada, (0, 0)), None);
        assert_eq!(memory.remembered(&only("bob"), (0, 0)), Some(first.now));

        // Two remembered versions: the newer row wins.
        let second = build(&store, 33);
        memory.saw("ada", &[(30, 30)]);
        memory.changed(&[second]);
        assert_eq!(memory.remembered(&only("ada"), (0, 0)), Some(second.was.unwrap()));
        assert_eq!(memory.remembered(&ada, (0, 0)), Some(second.was.unwrap()), "Ada's own is the newer");
        assert!(second.was.unwrap() > first.now);
    }
}
