//! An audition waiting for the bar it starts on, counted down block by block.

pub(crate) struct QueuedStart<T> {
    pub(crate) audition: T,
    frames_until_start: u64,
}

impl<T> QueuedStart<T> {
    pub(crate) fn new(audition: T, frames_until_start: u64) -> Self {
        Self {
            audition,
            frames_until_start,
        }
    }
}

/// Takes the queued audition when it starts inside this block of `frames`,
/// with the frame it starts on; a stopped transport starts it at once.
/// Otherwise the block is counted off its wait.
pub(crate) fn take_due<T>(
    queued: &mut Option<QueuedStart<T>>,
    frames: usize,
    playing: bool,
) -> Option<(T, usize)> {
    let waiting = queued.as_mut()?;
    if !playing {
        return queued.take().map(|queued| (queued.audition, 0));
    }
    if waiting.frames_until_start < frames as u64 {
        let lead = waiting.frames_until_start as usize;
        return queued.take().map(|queued| (queued.audition, lead));
    }
    waiting.frames_until_start -= frames as u64;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queued_audition_starts_on_its_frame_after_counting_down() {
        let mut queued = Some(QueuedStart::new('a', 300));

        assert_eq!(take_due(&mut queued, 256, true), None);
        assert_eq!(take_due(&mut queued, 256, true), Some(('a', 44)));
        assert!(queued.is_none());
    }

    #[test]
    fn a_stopped_transport_starts_a_queued_audition_at_once() {
        let mut queued = Some(QueuedStart::new('a', 300));

        assert_eq!(take_due(&mut queued, 256, false), Some(('a', 0)));
    }
}
