pub(crate) fn current() -> usize {
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
