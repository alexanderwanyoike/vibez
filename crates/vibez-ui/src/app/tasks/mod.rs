//! Off-UI-thread task modules, one per domain, sharing one dispatch idiom.

use super::*;

mod browser;
mod clip_dsp;
mod project;
mod remote;
mod render;

pub(in crate::app) use browser::*;
pub(in crate::app) use clip_dsp::*;
pub(in crate::app) use project::*;
pub(in crate::app) use remote::*;
pub(in crate::app) use render::*;

pub(crate) async fn run_off_ui_thread<T, F>(label: &'static str, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("{label} task failed: {error}"))
}

#[cfg(test)]
mod off_ui_thread_tests {
    use std::time::Duration;

    use super::run_off_ui_thread;

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_work_never_blocks_the_ui_executor() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let work = tokio::spawn(run_off_ui_thread("Test", move || {
            release_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("UI executor should release the blocking worker");
            42
        }));

        tokio::task::yield_now().await;
        assert!(release_tx.send(()).is_ok());
        assert_eq!(work.await.unwrap().unwrap(), 42);
    }
}
