use iced::Task;

/// One router-owned async request lifecycle.
///
/// Tokens reject stale completions. Replacing/cancelling a request aborts any
/// attached task, and dropping the tracker aborts its active task as well.
#[derive(Default)]
pub(super) struct TrackedRequest {
    generation: u64,
    active: Option<u64>,
    abort: Option<Box<dyn FnOnce()>>,
}

impl TrackedRequest {
    pub fn begin(&mut self) -> u64 {
        self.abort_current();
        self.generation = self.generation.wrapping_add(1);
        self.active = Some(self.generation);
        self.generation
    }

    pub fn attach<Message: 'static>(&mut self, task: Task<Message>) -> Task<Message> {
        if let Some(abort) = self.abort.take() {
            abort();
        }
        let (task, handle) = task.abortable();
        self.abort = Some(Box::new(move || handle.abort()));
        task
    }

    pub fn is_current(&self, token: u64) -> bool {
        self.active == Some(token)
    }

    pub fn current(&self) -> Option<u64> {
        self.active
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn finish(&mut self, token: u64) -> bool {
        if !self.is_current(token) {
            return false;
        }
        self.active = None;
        self.abort = None;
        true
    }

    pub fn cancel(&mut self) {
        self.abort_current();
        self.generation = self.generation.wrapping_add(1);
        self.active = None;
    }

    fn abort_current(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort();
        }
        self.active = None;
    }

    #[cfg(test)]
    fn attach_abort_probe(&mut self, probe: std::sync::Arc<std::sync::atomic::AtomicBool>) {
        self.abort = Some(Box::new(move || {
            probe.store(true, std::sync::atomic::Ordering::SeqCst)
        }));
    }
}

impl Drop for TrackedRequest {
    fn drop(&mut self) {
        self.abort_current();
    }
}

#[cfg(test)]
mod tests {
    use super::TrackedRequest;

    #[test]
    fn replacement_rejects_the_previous_token_and_finish_clears_activity() {
        let mut request = TrackedRequest::default();
        let first = request.begin();
        let probe = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        request.attach_abort_probe(std::sync::Arc::clone(&probe));
        assert!(request.is_current(first));
        assert!(request.is_active());

        let second = request.begin();
        assert!(probe.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!request.is_current(first));
        assert!(request.is_current(second));
        assert!(!request.finish(first));
        assert!(request.finish(second));
        assert!(!request.is_active());
    }

    #[test]
    fn cancellation_invalidates_the_current_token() {
        let mut request = TrackedRequest::default();
        let token = request.begin();

        request.cancel();

        assert!(!request.is_current(token));
        assert!(!request.is_active());
    }

    #[test]
    fn drop_aborts_attached_work() {
        let probe = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let mut request = TrackedRequest::default();
            request.begin();
            request.attach_abort_probe(std::sync::Arc::clone(&probe));
        }

        assert!(probe.load(std::sync::atomic::Ordering::SeqCst));
    }
}
