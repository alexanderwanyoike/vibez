//! Transport commands reach the callback on their own ring and overtake only pending content edits.

use rtrb::{Consumer, Producer, RingBuffer};

use crate::commands::EngineCommand;

const TRANSPORT_RING_CAPACITY: usize = 64;

impl EngineCommand {
    /// Commands the transport owns. They travel on the transport ring so a
    /// paused content edit never delays Play or Stop.
    pub fn is_transport(&self) -> bool {
        matches!(
            self,
            Self::Play
                | Self::Stop
                | Self::Seek(_)
                | Self::SetArrangementLoop(_)
                | Self::SetArrangementLoopRegion { .. }
        )
    }

    /// Arrangement content edits: the only commands a transport command may
    /// overtake. Anything else keeps its order relative to the transport.
    pub fn is_content_edit(&self) -> bool {
        matches!(
            self,
            Self::AddClip { .. }
                | Self::RemoveClip(..)
                | Self::ReplaceClipAudio { .. }
                | Self::ReplaceClipBuffer { .. }
                | Self::MoveClip { .. }
                | Self::SetClipGain { .. }
                | Self::SetClipFades { .. }
                | Self::SetClipPlaybackDirection { .. }
                | Self::SetClipWarpMarkers { .. }
                | Self::SetClipBounds { .. }
                | Self::SetClipLoop { .. }
                | Self::SetClipStartMarker { .. }
                | Self::SetAutomationLane { .. }
                | Self::RemoveAutomationLane { .. }
                | Self::AddNoteClip { .. }
                | Self::RemoveNoteClip(..)
                | Self::MoveNoteClip { .. }
                | Self::SetNoteClipDuration { .. }
                | Self::SetNoteClipGrooveGrid { .. }
                | Self::SetNoteClipLoop { .. }
                | Self::SetNoteClipStartMarker { .. }
                | Self::AddNote { .. }
                | Self::RemoveNote { .. }
                | Self::EditNote { .. }
        )
    }
}

/// A transport command stamped with how many non-content commands were sent
/// on the main command ring before it.
pub struct TransportCommand {
    controls_before: u64,
    command: EngineCommand,
}

impl TransportCommand {
    pub fn command(&self) -> &EngineCommand {
        &self.command
    }
}

/// The UI end of the transport ring.
pub struct TransportSender {
    producer: Producer<TransportCommand>,
    controls_sent: u64,
}

impl TransportSender {
    /// Call for every command sent on the main command ring, in send order,
    /// including commands the sender holds back while that ring is full.
    pub fn record_main_command(&mut self, command: &EngineCommand) {
        if !command.is_content_edit() {
            self.controls_sent += 1;
        }
    }

    /// Stamp at send time, not push time: a command held back while this
    /// ring is full must not wait for controls sent after it.
    pub fn stamp(&self, command: EngineCommand) -> TransportCommand {
        debug_assert!(command.is_transport());
        TransportCommand {
            controls_before: self.controls_sent,
            command,
        }
    }

    #[allow(
        clippy::result_large_err,
        reason = "a full ring hands the stamped command back to wait in order"
    )]
    pub fn push(&mut self, command: TransportCommand) -> Result<(), TransportCommand> {
        self.producer
            .push(command)
            .map_err(|rtrb::PushError::Full(command)| command)
    }
}

/// The callback end of the transport ring.
pub(crate) struct TransportReceiver {
    consumer: Consumer<TransportCommand>,
    controls_applied: u64,
}

impl TransportReceiver {
    pub(crate) fn new(consumer: Consumer<TransportCommand>) -> Self {
        Self {
            consumer,
            controls_applied: 0,
        }
    }

    pub(crate) fn record_main_command(&mut self, command: &EngineCommand) {
        if !command.is_content_edit() {
            self.controls_applied += 1;
        }
    }

    /// The next transport command, once every non-content command sent
    /// before it has been applied.
    pub(crate) fn ready(&self) -> Option<&EngineCommand> {
        self.consumer
            .peek()
            .ok()
            .filter(|next| next.controls_before <= self.controls_applied)
            .map(|next| &next.command)
    }

    pub(crate) fn pop_ready(&mut self) -> Option<EngineCommand> {
        self.ready()?;
        self.consumer.pop().ok().map(|next| next.command)
    }
}

/// A transport ring whose callback end is a plain consumer, for callers that
/// observe transport commands without an engine.
pub fn transport_ring() -> (TransportSender, Consumer<TransportCommand>) {
    let (producer, consumer) = RingBuffer::new(TRANSPORT_RING_CAPACITY);
    (
        TransportSender {
            producer,
            controls_sent: 0,
        },
        consumer,
    )
}
