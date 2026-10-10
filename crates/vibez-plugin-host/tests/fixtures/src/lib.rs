pub const CLAP_ID: &str = "vibez.fixture.routing";
mod clap;
mod timing;
mod vst3;

// Rust's thread handle registers a TLS destructor in the dynamic library.
// Native IDs let the fixture unload before the test thread exits safely.
fn thread_id() -> usize {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn pthread_self() -> usize;
        }
        unsafe { pthread_self() }
    }
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn GetCurrentThreadId() -> u32;
        }
        unsafe { GetCurrentThreadId() as usize }
    }
}

static LIFECYCLE_ERRORS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
fn lifecycle_error() {
    LIFECYCLE_ERRORS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
#[no_mangle]
pub extern "C" fn fixture_lifecycle_errors() -> u32 {
    LIFECYCLE_ERRORS.load(std::sync::atomic::Ordering::Relaxed)
}

static ACTIVATIONS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static MAIN_CALLBACKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static EVENT_ACTIVATIONS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[no_mangle]
pub extern "C" fn fixture_activations() -> u32 {
    ACTIVATIONS.load(std::sync::atomic::Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn fixture_main_callbacks() -> u32 {
    MAIN_CALLBACKS.load(std::sync::atomic::Ordering::Relaxed)
}
#[no_mangle]
pub extern "C" fn fixture_event_activations() -> u32 {
    EVENT_ACTIVATIONS.load(std::sync::atomic::Ordering::Relaxed)
}

struct ProcessingCall<'a>(&'a std::sync::atomic::AtomicBool);
impl<'a> ProcessingCall<'a> {
    fn enter(active: &'a std::sync::atomic::AtomicBool) -> Self {
        if active.swap(true, std::sync::atomic::Ordering::AcqRel) {
            lifecycle_error();
        }
        Self(active)
    }
}
impl Drop for ProcessingCall<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}
