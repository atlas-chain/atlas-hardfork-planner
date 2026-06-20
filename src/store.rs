use std::collections::VecDeque;
use std::sync::Mutex;

use crate::model::{ScheduleDocument, ScheduleEntry, canonicalize, now_iso_second, select_active_entries};
use crate::validation::{self, ValidationFailure};

const HISTORY_LIMIT: usize = 64;

#[derive(Clone, Debug)]
pub struct ReleaseRecord {
    pub version: u64,
    pub chain_id: u64,
    pub current_block: Option<u64>,
    pub active_entries: usize,
    pub hash: String,
    pub installed_at: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub canonical: String,
    pub hash: String,
    pub version: u64,
    pub chain_id: u64,
    pub current_block: Option<u64>,
    pub active_entries: usize,
    pub retained_versions: usize,
}

#[derive(Debug)]
pub enum RemoveForkFailure {
    NotFound,
    Validation(ValidationFailure),
}

#[derive(Debug)]
struct Inner {
    current: ScheduleDocument,
    canonical: String,
    hash: String,
    last_version: u64,
    history: VecDeque<ReleaseRecord>,
}

#[derive(Debug)]
pub struct ScheduleStore {
    inner: Mutex<Inner>,
    expected_chain_id: Option<u64>,
}

impl ScheduleStore {
    pub fn new(
        doc: ScheduleDocument,
        expected_chain_id: Option<u64>,
    ) -> Result<Self, ValidationFailure> {
        validation::validate_document(&doc, expected_chain_id)?;

        let canonical = canonicalize(&doc);
        let hash = stable_hash(canonical.as_bytes());
        let last_version = doc.version;
        let record = record_for(&doc, &hash);

        let mut history = VecDeque::new();
        history.push_back(record);

        Ok(Self {
            inner: Mutex::new(Inner {
                current: doc,
                canonical,
                hash,
                last_version,
                history,
            }),
            expected_chain_id,
        })
    }

    pub fn install(&self, doc: ScheduleDocument) -> Result<(), ValidationFailure> {
        validation::validate_document(&doc, self.expected_chain_id)?;

        let mut inner = self.inner.lock().expect("schedule store lock poisoned");

        if doc.version < inner.last_version {
            return Err(ValidationFailure::VersionRegression {
                offered: doc.version,
                last: inner.last_version,
            });
        }

        let canonical = canonicalize(&doc);
        if doc.version == inner.last_version && canonical != inner.canonical {
            return Err(ValidationFailure::VersionNotIncreased {
                version: doc.version,
            });
        }

        let hash = stable_hash(canonical.as_bytes());
        let record = record_for(&doc, &hash);

        inner.last_version = doc.version;
        inner.current = doc;
        inner.canonical = canonical;
        inner.hash = hash;
        push_history(&mut inner.history, record);

        Ok(())
    }

    pub fn set_current_block(&self, block: u64) -> bool {
        let mut inner = self.inner.lock().expect("schedule store lock poisoned");

        if inner.current.current_block == Some(block) {
            return false;
        }

        inner.current.current_block = Some(block);
        apply_bumped_locked(&mut inner);
        true
    }

    pub fn add_fork(&self, entry: ScheduleEntry) -> Result<Snapshot, ValidationFailure> {
        let mut inner = self.inner.lock().expect("schedule store lock poisoned");

        let mut new_doc = inner.current.clone();
        new_doc.schedule.push(entry);
        new_doc.schedule.sort_by_key(|item| item.activation_block);

        validation::validate_document(&new_doc, self.expected_chain_id)?;
        inner.current = new_doc;
        apply_bumped_locked(&mut inner);
        Ok(snapshot_from(&inner))
    }

    pub fn remove_fork(&self, activation_block: u64) -> Result<Snapshot, RemoveForkFailure> {
        let mut inner = self.inner.lock().expect("schedule store lock poisoned");

        let mut new_doc = inner.current.clone();
        let before = new_doc.schedule.len();
        new_doc.schedule.retain(|item| item.activation_block != activation_block);

        if new_doc.schedule.len() == before {
            return Err(RemoveForkFailure::NotFound);
        }

        validation::validate_document(&new_doc, self.expected_chain_id)
            .map_err(RemoveForkFailure::Validation)?;

        inner.current = new_doc;
        apply_bumped_locked(&mut inner);
        Ok(snapshot_from(&inner))
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner.lock().expect("schedule store lock poisoned");
        snapshot_from(&inner)
    }

    pub fn history(&self) -> Vec<ReleaseRecord> {
        let inner = self.inner.lock().expect("schedule store lock poisoned");
        inner.history.iter().cloned().collect()
    }
}

fn apply_bumped_locked(inner: &mut Inner) {
    let next_version = inner
        .last_version
        .checked_add(1)
        .expect("schedule version does not overflow");
    inner.current.version = next_version;
    inner.last_version = next_version;
    inner.canonical = canonicalize(&inner.current);
    inner.hash = stable_hash(inner.canonical.as_bytes());
    let record = record_for(&inner.current, &inner.hash);
    push_history(&mut inner.history, record);
}

fn snapshot_from(inner: &Inner) -> Snapshot {
    Snapshot {
        canonical: inner.canonical.clone(),
        hash: inner.hash.clone(),
        version: inner.current.version,
        chain_id: inner.current.chain_id,
        current_block: inner.current.current_block,
        active_entries: select_active_entries(&inner.current).len(),
        retained_versions: inner.history.len(),
    }
}

fn record_for(doc: &ScheduleDocument, hash: &str) -> ReleaseRecord {
    ReleaseRecord {
        version: doc.version,
        chain_id: doc.chain_id,
        current_block: doc.current_block,
        active_entries: select_active_entries(doc).len(),
        hash: hash.to_string(),
        installed_at: now_iso_second(),
    }
}

fn push_history(history: &mut VecDeque<ReleaseRecord>, record: ReleaseRecord) {
    history.push_back(record);
    while history.len() > HISTORY_LIMIT {
        history.pop_front();
    }
}

fn stable_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScheduleEntry;

    fn entry(activation_block: u64) -> ScheduleEntry {
        ScheduleEntry {
            activation_block,
            min_base_fee_per_gas: "440000000".to_string(),
            elasticity_multiplier: 2,
            base_fee_max_change_denominator: 8,
            max_block_gas_limit: "30000000".to_string(),
        }
    }

    fn document(version: u64, current_block: Option<u64>) -> ScheduleDocument {
        ScheduleDocument {
            chain_id: 42069,
            version,
            current_block,
            schedule: vec![entry(0)],
        }
    }

    #[test]
    fn rejects_version_regression_on_install() {
        let store = ScheduleStore::new(document(5, None), None).unwrap();
        let result = store.install(document(4, None));
        assert_eq!(
            result,
            Err(ValidationFailure::VersionRegression {
                offered: 4,
                last: 5
            })
        );
    }

    #[test]
    fn rejects_content_change_without_version_increase() {
        let store = ScheduleStore::new(document(5, None), None).unwrap();
        let mut changed = document(5, None);
        changed.schedule[0].elasticity_multiplier = 4;
        let result = store.install(changed);
        assert_eq!(
            result,
            Err(ValidationFailure::VersionNotIncreased { version: 5 })
        );
    }

    #[test]
    fn accepts_higher_version_install() {
        let store = ScheduleStore::new(document(5, None), None).unwrap();
        let mut next = document(6, None);
        next.schedule[0].elasticity_multiplier = 4;
        assert!(store.install(next).is_ok());
        assert_eq!(store.snapshot().version, 6);
    }

    #[test]
    fn set_current_block_updates_version_and_hash() {
        let store = ScheduleStore::new(document(1, None), None).unwrap();
        let before = store.snapshot();

        assert!(store.set_current_block(100));
        let after = store.snapshot();

        assert_eq!(after.version, before.version + 1);
        assert_eq!(after.current_block, Some(100));
        assert_ne!(after.hash, before.hash);
        assert!(after.canonical.contains("currentBlock"));
    }

    #[test]
    fn set_current_block_is_idempotent() {
        let store = ScheduleStore::new(document(1, None), None).unwrap();
        assert!(store.set_current_block(100));
        assert!(!store.set_current_block(100));

        let snap = store.snapshot();
        assert_eq!(snap.version, 2);
    }

    #[test]
    fn history_caps_at_limit() {
        let store = ScheduleStore::new(document(1, None), None).unwrap();
        for block in 1..=(HISTORY_LIMIT as u64 + 10) {
            assert!(store.set_current_block(block));
        }
        let history = store.history();
        assert_eq!(history.len(), HISTORY_LIMIT);
        assert_eq!(history.first().unwrap().current_block, Some(11));
        assert_eq!(
            history.last().unwrap().current_block,
            Some(HISTORY_LIMIT as u64 + 10)
        );
    }

    #[test]
    fn snapshot_reflects_active_entries() {
        let mut doc = document(1, None);
        doc.schedule = vec![entry(0), entry(1_000)];
        let store = ScheduleStore::new(doc, None).unwrap();

        let snap = store.snapshot();
        assert_eq!(snap.active_entries, 2);

        store.set_current_block(50);
        let snap = store.snapshot();
        assert_eq!(snap.active_entries, 1);
    }

    #[test]
    fn add_fork_appends_and_bumps_version() {
        let store = ScheduleStore::new(document(1, None), None).unwrap();
        let snapshot = store.add_fork(entry(1_000)).unwrap();

        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.active_entries, 2);
        assert!(store
            .snapshot()
            .canonical
            .contains("\"activationBlock\": 1000"));
    }

    #[test]
    fn add_fork_inserts_in_order() {
        let store = ScheduleStore::new(document(1, None), None).unwrap();
        store.add_fork(entry(2_000)).unwrap();
        store.add_fork(entry(1_000)).unwrap();

        let canonical = store.snapshot().canonical;
        let position_zero = canonical.find("\"activationBlock\": 0").unwrap();
        let position_1000 = canonical.find("\"activationBlock\": 1000").unwrap();
        let position_2000 = canonical.find("\"activationBlock\": 2000").unwrap();
        assert!(position_zero < position_1000);
        assert!(position_1000 < position_2000);
    }

    #[test]
    fn add_fork_rejects_duplicate_activation_block() {
        let store = ScheduleStore::new(document(1, None), None).unwrap();
        let result = store.add_fork(entry(0));
        assert!(matches!(
            result,
            Err(ValidationFailure::NonIncreasingActivationBlocks { .. })
        ));
        assert_eq!(store.snapshot().version, 1);
    }

    #[test]
    fn remove_fork_removes_and_bumps_version() {
        let mut doc = document(1, None);
        doc.schedule = vec![entry(0), entry(1_000)];
        let store = ScheduleStore::new(doc, None).unwrap();

        let snapshot = store.remove_fork(1_000).unwrap();
        assert_eq!(snapshot.version, 2);
        assert_eq!(snapshot.active_entries, 1);
        assert!(!store.snapshot().canonical.contains("\"activationBlock\": 1000"));
    }

    #[test]
    fn remove_fork_returns_not_found_for_missing_block() {
        let store = ScheduleStore::new(document(1, None), None).unwrap();
        assert!(matches!(
            store.remove_fork(999),
            Err(RemoveForkFailure::NotFound)
        ));
    }

    #[test]
    fn remove_fork_rejects_removing_baseline() {
        let store = ScheduleStore::new(document(1, None), None).unwrap();
        let result = store.remove_fork(0);
        assert!(matches!(result, Err(RemoveForkFailure::Validation(_))));
        assert_eq!(store.snapshot().version, 1);
    }
}
