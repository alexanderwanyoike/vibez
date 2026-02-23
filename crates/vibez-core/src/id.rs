use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
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
