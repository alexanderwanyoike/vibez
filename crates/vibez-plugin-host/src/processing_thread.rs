use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);
thread_local! {static TOKEN:Cell<u64>=const {Cell::new(0)};}

pub(crate) fn current() -> u64 {
    // Native thread identifiers can be reused after a worker exits. This
    // primitive TLS value needs neither allocation nor a TLS destructor.
    TOKEN.with(|token| {
        let current = token.get();
        if current != 0 {
            current
        } else {
            let current = NEXT.fetch_add(1, Ordering::Relaxed);
            token.set(current);
            current
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn terminated_workers_do_not_share_processing_identity() {
        let first = std::thread::spawn(super::current).join().unwrap();
        let second = std::thread::spawn(super::current).join().unwrap();
        assert_ne!(first, second);
    }
}
