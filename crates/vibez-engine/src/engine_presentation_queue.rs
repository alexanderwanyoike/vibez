//! Bounded source event ownership and delivery survive UI backpressure.

use super::*;
use crate::events::{PerformanceCaptureStartedEvent, SectionTransitionedEvent};
use super::terminal_notifications::TerminalNotifications;

pub(super) enum CancelableStop {
    Section { effective_at_samples: u64 },
    Performance { effective_at_samples: u64 },
}

pub(super) struct ScheduledPresentation {
    pub(super) due: u64,
    pub(super) event: EngineEvent,
}

pub(super) const PRESENTATION_EVENT_CAPACITY: usize = 2048;
// Track-sized cleanup pauses before consuming the slots needed by the remaining
// fixed command body (recording, Section, Capture and transport acknowledgements).
pub(super) const COMMAND_PRESENTATION_RESERVE: usize = 16;

fn cancelled_event(event: EngineEvent) -> EngineEvent {
    match event {
        EngineEvent::SectionTransitioned(SectionTransitionedEvent { retired, .. }) => {
            EngineEvent::SectionQueueCancelled {
                retired: retired.take(),
            }
        }
        EngineEvent::ClipTransitioned {
            retired: Some(retired),
            ..
        } => EngineEvent::ClipRequestRetired(retired),
        EngineEvent::SectionCaptureSource { offsets, .. }
        | EngineEvent::PerformanceCaptureStarted(PerformanceCaptureStartedEvent {
            offsets, ..
        }) => EngineEvent::CaptureTimingRetired(offsets),
        EngineEvent::PlaybackStarted
        | EngineEvent::ClipTransitioned { .. }
        | EngineEvent::ClipSourceRefreshed { .. }
        | EngineEvent::ClipCaptureSource { .. }
        | EngineEvent::SectionCaptureStopped { .. }
        | EngineEvent::PerformanceCaptureStopped { .. }
        | EngineEvent::InstrumentNoteInput(_)
        | EngineEvent::NoteRepeated(_)
        | EngineEvent::AutomationGestureChanged(_) => EngineEvent::PresentationCancelled,
        event => event,
    }
}

fn source_feed(event: &EngineEvent) -> bool {
    matches!(
        event,
        EngineEvent::SourceNoteInput(_) | EngineEvent::SourceNoteRepeated(_)
    )
}

fn capture_feed(event: &EngineEvent) -> bool {
    matches!(
        event,
        EngineEvent::PerformanceCaptureStarted(_)
            | EngineEvent::PerformanceCaptureStopped { .. }
            | EngineEvent::InstrumentNoteInput(_)
            | EngineEvent::NoteRepeated(_)
            | EngineEvent::AutomationGestureChanged(_)
            | EngineEvent::TrackMuteChanged(_)
            | EngineEvent::SectionCaptureSource { .. }
            | EngineEvent::SectionCaptureStopped { .. }
            | EngineEvent::ClipCaptureSource { .. }
    )
}

fn ready(pending: &ScheduledPresentation, source_now: u64, heard_now: u64) -> bool {
    pending.due
        <= if source_feed(&pending.event) {
            source_now
        } else {
            heard_now
        }
}

fn flush_ready(
    events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>,
    eligible: impl Fn(&ScheduledPresentation) -> bool,
) {
    let mut slots = events.slots();
    // Stable compaction visits each retained owner once, even when immediate
    // recording feeds sit beyond future heard events in physical-time order.
    scheduled.retain_mut(|pending| {
        if slots == 0 || !eligible(pending) {
            return true;
        }
        let event = std::mem::replace(&mut pending.event, EngineEvent::PresentationCancelled);
        let result = events.push(event);
        debug_assert!(result.is_ok());
        slots -= 1;
        false
    });
}

fn queue_event(
    event: EngineEvent,
    events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>,
    now: u64,
    due: u64,
    heard_now: u64,
) -> Option<EngineEvent> {
    let cutoff = if source_feed(&event) { now } else { heard_now };
    let event = if due <= cutoff
        && !scheduled
            .iter()
            .any(|pending| ready(pending, now, heard_now))
    {
        match events.push(event) {
            Ok(()) => return None,
            Err(rtrb::PushError::Full(event)) => event,
        }
    } else {
        event
    };
    if scheduled.len() == scheduled.capacity() {
        return Some(event);
    }
    let index = scheduled.partition_point(|pending| pending.due <= due);
    scheduled.insert(index, ScheduledPresentation { due, event });
    None
}

pub(super) fn emit_repeated(
    source: EngineEvent,
    heard: EngineEvent,
    events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>,
    now: u64,
    due: u64,
    heard_now: u64,
) -> bool {
    debug_assert_eq!(source.time_domain(), crate::events::TimeDomain::Source);
    debug_assert_eq!(heard.time_domain(), crate::events::TimeDomain::Heard);
    flush_ready(events, scheduled, |pending| ready(pending, now, heard_now));
    if scheduled.capacity() - scheduled.len() < 2 {
        return false;
    }
    queue_event(source, events, scheduled, now, now, heard_now).is_none()
        && queue_event(heard, events, scheduled, now, due, heard_now).is_none()
}

pub(super) fn flush_presentation_parts(
    events: &mut rtrb::Producer<EngineEvent>, scheduled: &mut Vec<ScheduledPresentation>,
    terminals: &mut super::terminal_notifications::TerminalNotifications,
    source_now: u64, heard_now: u64,
) {
        if terminals.capture_stop().is_some() {
            flush_ready(
                events,
                scheduled,
                |pending| pending.due <= heard_now && capture_feed(&pending.event),
            );
            if scheduled
                .iter()
                .any(|pending| pending.due <= heard_now && capture_feed(&pending.event))
            {
                return;
            }
        }
        if !terminals
            .publish(events)
        {
            return;
        }
        flush_ready(
            events,
            scheduled,
            |pending| ready(pending, source_now, heard_now),
        );
    }

fn queue_admitted_event(
    event: EngineEvent, events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>, now: u64, due: u64, heard_now: u64,
) {
    let cutoff = if source_feed(&event) { now } else { heard_now };
    let event = if due <= cutoff && !scheduled.iter().any(|pending| ready(pending, now, heard_now)) {
        match events.push(event) { Ok(()) => return, Err(rtrb::PushError::Full(event)) => event }
    } else { event };
    let index = scheduled.partition_point(|pending| pending.due <= due);
    scheduled.insert(index, ScheduledPresentation { due, event });
}

#[must_use = "publish the admitted fixed prefix before entering nested engine work"]
pub(super) struct PresentationAdmission<'a, const N: usize> {
    events: &'a mut rtrb::Producer<EngineEvent>,
    scheduled: &'a mut Vec<ScheduledPresentation>,
    terminals: &'a mut TerminalNotifications,
}

impl<'a, const N: usize> PresentationAdmission<'a, N> {
    pub(super) fn new(
        events: &'a mut rtrb::Producer<EngineEvent>,
        scheduled: &'a mut Vec<ScheduledPresentation>,
        terminals: &'a mut TerminalNotifications,
    ) -> Option<Self> {
        // Admission observes the original guard's queue state. Publication,
        // not admission, performs the existing terminal-priority flush.
        if scheduled.capacity().saturating_sub(scheduled.len()) < N { return None; }
        Some(Self { events, scheduled, terminals })
    }

    pub(super) fn new_clip_batch(
        events: &'a mut rtrb::Producer<EngineEvent>,
        scheduled: &'a mut Vec<ScheduledPresentation>,
        terminals: &'a mut TerminalNotifications, future: bool,
    ) -> Option<Self> {
        // Current callback acknowledgements may spend ring slots only at frame
        // zero. Heard transitions use their original frozen deadline instead.
        let room = scheduled.capacity() - scheduled.len()
            + if future { 0 } else { events.slots().saturating_sub(terminals.occupied_slots()) };
        if room < N { return None; }
        Some(Self { events, scheduled, terminals })
    }

    pub(super) fn publish(self, prefix: [Option<ScheduledPresentation>; N], source_now: u64, heard_now: u64) {
        for pending in prefix.into_iter().flatten() {
            debug_assert!(pending.due <= source_now || pending.event.time_domain() == crate::events::TimeDomain::Heard);
            flush_presentation_parts(self.events, self.scheduled, self.terminals, source_now, heard_now);
            // Only this exclusive borrow can write the Vec. The fixed array cannot
            // spend more than the prefix admitted before its source was detached.
            queue_admitted_event(pending.event, self.events, self.scheduled, source_now, pending.due, heard_now);
        }
    }
    pub(super) fn publish_clip(
        self, prefix: [Option<EngineEvent>; N],
        mut context: super::clip_launcher::ClipEventContext<'_>,
        source_now: u64, heard_now: u64,
    ) {
        for event in prefix.into_iter().flatten() {
            // Clip drop telemetry observes each prior publication, including
            // terminal-priority flushing, before preparing the next fact.
            let pending = context.prepare(event, self.events.is_full());
            debug_assert!(pending.due <= source_now || pending.event.time_domain() == crate::events::TimeDomain::Heard);
            flush_presentation_parts(self.events, self.scheduled, self.terminals, source_now, heard_now);
            queue_admitted_event(pending.event, self.events, self.scheduled, source_now, pending.due, heard_now);
        }
    }

}

impl<'a> PresentationAdmission<'a, 3> {
    pub(super) fn advance(self, pending: Option<ScheduledPresentation>, source_now: u64, heard_now: u64) -> PresentationAdmission<'a, 2> {
        if let Some(pending) = pending {
            debug_assert!(pending.due <= source_now || pending.event.time_domain() == crate::events::TimeDomain::Heard);
            flush_presentation_parts(self.events, self.scheduled, self.terminals, source_now, heard_now);
            queue_admitted_event(pending.event, self.events, self.scheduled, source_now, pending.due, heard_now);
        }
        PresentationAdmission { events: self.events, scheduled: self.scheduled, terminals: self.terminals }
    }
}

impl<'a> PresentationAdmission<'a, 2> {
    pub(super) fn advance(self, pending: Option<ScheduledPresentation>, source_now: u64, heard_now: u64) -> PresentationAdmission<'a, 1> {
        if let Some(pending) = pending {
            debug_assert!(pending.due <= source_now || pending.event.time_domain() == crate::events::TimeDomain::Heard);
            flush_presentation_parts(self.events, self.scheduled, self.terminals, source_now, heard_now);
            queue_admitted_event(pending.event, self.events, self.scheduled, source_now, pending.due, heard_now);
        }
        PresentationAdmission { events: self.events, scheduled: self.scheduled, terminals: self.terminals }
    }
}

impl AudioEngine {
    pub(super) fn presentation_return_has_room(&self) -> bool {
        self.event_tx.slots() > 0
            || self.scheduled_presentation.len() < self.scheduled_presentation.capacity()
            || self.presentation_state.terminals.can_retain_overflow_owner()
            || self.pending_retirements.available() > 0
    }

    pub(super) fn present_natural_playback_stop_after(&mut self, delay: u32) {
        let now = self.output_position + self.rendered_callback_frames as u64;
        // Only the original literal can come back. Natural completion and the
        // fault it discovers share the existing reliable stopped-state terminal.
        if let Err(EngineEvent::PlaybackStopped) = self.try_present_event_at(
            EngineEvent::PlaybackStopped,
            now.saturating_add(delay as u64),
        ) {
            self.presentation_state.terminals.request_playback_stop();
            self.flush_presentation();
        }
    }

    pub(super) fn present_cancelable_stop_after(&mut self, stopped: CancelableStop, delay: u32) {
        let event = match stopped {
            CancelableStop::Section { effective_at_samples } => EngineEvent::SectionCaptureStopped { effective_at_samples },
            CancelableStop::Performance { effective_at_samples } => EngineEvent::PerformanceCaptureStopped { effective_at_samples },
        };
        let now = self.output_position + self.rendered_callback_frames as u64;
        // Both literal variants cancel to a tombstone; Capture also has its
        // existing reliable fault-entry terminal route. Neither can return an owner.
        let result = self.try_present_event_at(event, now.saturating_add(delay as u64));
        debug_assert!(result.is_ok(), "closed cancelable stop cannot return an owner");
    }

    pub(super) fn present_event(&mut self, event: EngineEvent, allowance: &mut RetirementAllowance) {
        let now = self.output_position + self.rendered_callback_frames as u64;
        self.present_event_at(event, now, allowance);
    }

    pub(super) fn present_event_after(&mut self, event: EngineEvent, delay: u32, allowance: &mut RetirementAllowance) {
        let now = self.output_position + self.rendered_callback_frames as u64;
        self.present_event_at(event, now.saturating_add(delay as u64), allowance);
    }

    pub(super) fn present_event_at(&mut self, event: EngineEvent, due: u64, allowance: &mut RetirementAllowance) {
        if let Err(event) = self.try_present_event_at(event, due) {
            self.retire_event(event, allowance);
        }
    }

    #[allow(
        clippy::result_large_err,
        reason = "Boxing a rejected owner would allocate in the audio callback"
    )]
    pub(super) fn try_present_event_at(&mut self, event: EngineEvent, due: u64) -> Result<(), EngineEvent> {
        debug_assert!(
            due <= self.output_position + self.rendered_callback_frames as u64
                || event.time_domain() == crate::events::TimeDomain::Heard,
            "only heard-time events wait for output latency: {event:?}"
        );
        if self.presentation_state.is_faulted() {
            match event {
                EngineEvent::PlaybackStopped => {
                    self.presentation_state.terminals.request_playback_stop();
                    self.flush_presentation();
                    return Ok(());
                }
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples,
                } => {
                    self.presentation_state
                        .terminals
                        .request_capture_stop(effective_at_samples);
                    self.flush_presentation();
                    return Ok(());
                }
                _ => {}
            }
        }
        self.flush_presentation();
        let now = self.output_position + self.rendered_callback_frames as u64;
        if let Some(event) = queue_event(
            event,
            &mut self.event_tx,
            &mut self.scheduled_presentation,
            now,
            due,
            self.output_position,
        ) {
            self.fail_presentation();
            // Ownership producers reserve before handoff. Retain the discovering
            // event separately while the full UI ring holds the scheduled owners.
            let event = cancelled_event(event);
            if let Some(event) = self
                .presentation_state
                .terminals
                .retain_overflow_owner(event)
            {
                // Cancelled feedback owns nothing, so it must not consume the
                // retirement space reserved for owners awaiting the UI.
                if !matches!(event, EngineEvent::PresentationCancelled) {
                    return Err(event);
                }
            }
        }
        Ok(())
    }

    pub(super) fn flush_presentation(&mut self) {
        flush_presentation_parts(
            &mut self.event_tx, &mut self.scheduled_presentation,
            &mut self.presentation_state.terminals,
            self.output_position + self.rendered_callback_frames as u64, self.output_position,
        );
    }

    pub(super) fn fail_presentation(&mut self) {
        if !self.presentation_state.enter_fault() {
            return;
        }
        self.capture_lifecycle.end();
        self.transport.stop();
        for track in &mut self.tracks {
            track.active_clip = None;
            track.flush_notes();
        }
        self.clip_resync_track = Some(0);
        self.cancel_presentation();
        self.presentation_state
            .terminals
            .request_capture_stop(self.compensation_callback_heard_start);
        self.presentation_state.terminals.request_playback_stop();
        self.presentation_state.terminals.request_failure(
            TrackId::MASTER,
            None,
            "Audio presentation event history is full",
        );
        self.flush_presentation();
    }

    pub(super) fn presentation_room(&mut self, events: usize) -> bool {
        if self.presentation_state.is_faulted() {
            return false;
        }
        if self.scheduled_presentation.capacity() - self.scheduled_presentation.len() < events {
            self.fail_presentation();
            false
        } else {
            true
        }
    }

    pub(super) fn cancel_presentation(&mut self) {
        self.cancel_capture_timing();
        for pending in &mut self.scheduled_presentation {
            let event = std::mem::replace(&mut pending.event, EngineEvent::PresentationCancelled);
            pending.event = if pending.due <= self.output_position && capture_feed(&event) {
                event
            } else {
                cancelled_event(event)
            };
            pending.due = self.output_position;
        }
        // Tombstones hold no owner or state and need no UI slot. Reclaiming
        // them leaves the reserved overflow holder for actual ownership.
        self.scheduled_presentation
            .retain(|pending| !matches!(pending.event, EngineEvent::PresentationCancelled));
    }

    pub(super) fn cancel_capture_presentation(&mut self) {
        self.cancel_capture_timing();
        // Preserve physical deadlines so Capture cancellation cannot advance
        // unrelated public state or invalidate the queue's sorted prefix.
        for pending in &mut self.scheduled_presentation {
            let event = std::mem::replace(&mut pending.event, EngineEvent::PresentationCancelled);
            pending.event = match event {
                event if pending.due <= self.output_position && capture_feed(&event) => event,
                EngineEvent::SectionCaptureSource { offsets, .. }
                | EngineEvent::PerformanceCaptureStarted(PerformanceCaptureStartedEvent {
                    offsets,
                    ..
                }) => EngineEvent::CaptureTimingRetired(offsets),
                EngineEvent::ClipCaptureSource { .. }
                | EngineEvent::SectionCaptureStopped { .. }
                | EngineEvent::PerformanceCaptureStopped { .. }
                | EngineEvent::AutomationGestureChanged(_) => EngineEvent::PresentationCancelled,
                event => event,
            };
        }
        self.scheduled_presentation
            .retain(|pending| !matches!(pending.event, EngineEvent::PresentationCancelled));
    }
}
