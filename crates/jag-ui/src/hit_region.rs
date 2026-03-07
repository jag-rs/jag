//! Standalone hit-region registry mapping opaque region IDs to [`FocusId`]s.
//!
//! Each focusable element is assigned a unique `u32` region identifier that
//! can be used for GPU-based hit-testing (e.g. color-picking).

use std::collections::HashMap;

use crate::focus::FocusId;

/// Registry that maps between [`FocusId`]s and opaque `u32` region IDs.
///
/// Region IDs start at `1` so that `0` can be reserved for "no hit".
#[derive(Debug)]
pub struct HitRegionRegistry {
    id_to_region: HashMap<FocusId, u32>,
    region_to_id: HashMap<u32, FocusId>,
    next_region_id: u32,
}

impl Default for HitRegionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HitRegionRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            id_to_region: HashMap::new(),
            region_to_id: HashMap::new(),
            next_region_id: 1,
        }
    }

    /// Register a [`FocusId`] and return its unique region ID.
    ///
    /// If the id has already been registered, the existing region ID is
    /// returned without allocating a new one.
    pub fn register(&mut self, id: FocusId) -> u32 {
        if let Some(&region) = self.id_to_region.get(&id) {
            return region;
        }
        let region = self.next_region_id;
        self.next_region_id += 1;
        self.id_to_region.insert(id, region);
        self.region_to_id.insert(region, id);
        region
    }

    /// Look up the [`FocusId`] associated with a region ID.
    pub fn lookup(&self, region_id: u32) -> Option<FocusId> {
        self.region_to_id.get(&region_id).copied()
    }

    /// Remove all entries and reset the region counter.
    pub fn clear(&mut self) {
        self.id_to_region.clear();
        self.region_to_id.clear();
        self.next_region_id = 1;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_returns_unique_ids() {
        let mut reg = HitRegionRegistry::new();
        let r1 = reg.register(FocusId(1));
        let r2 = reg.register(FocusId(2));
        assert_ne!(r1, r2);
    }

    #[test]
    fn register_same_id_returns_same_region() {
        let mut reg = HitRegionRegistry::new();
        let r1 = reg.register(FocusId(5));
        let r2 = reg.register(FocusId(5));
        assert_eq!(r1, r2);
    }

    #[test]
    fn lookup_returns_correct_id() {
        let mut reg = HitRegionRegistry::new();
        let region = reg.register(FocusId(42));
        assert_eq!(reg.lookup(region), Some(FocusId(42)));
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let reg = HitRegionRegistry::new();
        assert_eq!(reg.lookup(999), None);
    }

    #[test]
    fn clear_resets_registry() {
        let mut reg = HitRegionRegistry::new();
        let r1 = reg.register(FocusId(1));
        reg.clear();

        // Previous region should no longer resolve.
        assert_eq!(reg.lookup(r1), None);

        // New registrations should start from 1 again.
        let r2 = reg.register(FocusId(2));
        assert_eq!(r2, 1);
    }
}
