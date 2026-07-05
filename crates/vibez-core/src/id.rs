use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Advance the global id counter past `raw`. MUST be called with the
/// highest raw id found in any loaded project before minting new ids:
/// persisted ids come from a previous session's counter, while this
/// session's counter restarts at 1, so a new track/clip could
/// otherwise receive the same id as a loaded one. Two objects sharing
/// an id makes selection highlight both and engine commands hit both.
pub fn ensure_ids_above(raw: u64) {
    NEXT_ID.fetch_max(raw.saturating_add(1), Ordering::Relaxed);
}

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(u64);

        impl $name {
            pub fn new() -> Self {
                Self(next_id())
            }

            pub fn raw(self) -> u64 {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

typed_id!(TrackId);
typed_id!(ClipId);
typed_id!(EffectId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_ids_above_prevents_collisions() {
        ensure_ids_above(1_000_000);
        let id = TrackId::new();
        assert!(id.raw() > 1_000_000);
        // Lower values must not move the counter backwards.
        ensure_ids_above(5);
        let id2 = TrackId::new();
        assert!(id2.raw() > id.raw());
    }

    #[test]
    fn ids_are_unique() {
        let a = TrackId::new();
        let b = TrackId::new();
        let c = ClipId::new();
        assert_ne!(a, b);
        // Cross-type IDs also get unique raw values
        assert_ne!(b.raw(), c.raw());
    }

    #[test]
    fn id_display() {
        let id = TrackId::new();
        let s = format!("{id}");
        assert!(s.starts_with("TrackId("));
    }
}
