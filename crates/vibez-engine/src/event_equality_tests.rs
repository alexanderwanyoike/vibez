//! Characterizes value comparisons and identity-only ownership returns in engine events.

use super::*;

#[test]
fn meter_equality_compares_every_channel_and_level() {
    let meter = |peak_l, peak_r, rms_l, rms_r| EngineEvent::Metering {
        peak_l,
        peak_r,
        rms_l,
        rms_r,
    };
    let expected = meter(0.5, 0.4, 0.3, 0.2);
    assert_eq!(expected, meter(0.5, 0.4, 0.3, 0.2));
    assert_ne!(expected, meter(0.6, 0.4, 0.3, 0.2));
    assert_ne!(expected, meter(0.5, 0.6, 0.3, 0.2));
    assert_ne!(expected, meter(0.5, 0.4, 0.6, 0.2));
    assert_ne!(expected, meter(0.5, 0.4, 0.3, 0.6));
    assert_ne!(meter(f32::NAN, 0.4, 0.3, 0.2), meter(f32::NAN, 0.4, 0.3, 0.2));
}

#[test]
fn section_record_lifecycle_variants_remain_distinct() {
    let section_id = SectionId::new();
    let track_id = TrackId::new();
    let armed = EngineEvent::SectionRecordArmed {
        section_id,
        track_id,
        effective_at_samples: 100,
        section_position_samples: 20,
    };
    assert_eq!(armed, EngineEvent::SectionRecordArmed {
        section_id,
        track_id,
        effective_at_samples: 100,
        section_position_samples: 20,
    });
    assert_ne!(armed, EngineEvent::SectionRecordStarted {
        section_id,
        track_id,
        effective_at_samples: 100,
        section_position_samples: 20,
    });
}

#[test]
fn retired_section_equality_uses_owner_identity() {
    let section_id = SectionId::new();
    let transition = || EngineEvent::SectionTransitioned {
        section_id,
        effective_at_samples: 100,
        retired: Box::new(PreparedSectionPlaybackSource::new(section_id, 4.0, true, vec![])),
    };
    let first = transition();
    let second = transition();
    assert_eq!(first, first);
    assert_ne!(first, second);
    let queued = EngineEvent::SectionQueued {
        section_id,
        effective_at_samples: 100,
        retired: None,
    };
    assert_eq!(queued, EngineEvent::SectionQueued {
        section_id,
        effective_at_samples: 100,
        retired: None,
    });
}

#[test]
fn unsupported_ownership_and_failure_variants_remain_uncomparable() {
    let failure = EngineEvent::CompensationFailed { reason: "failure".into() };
    assert_ne!(failure, failure);
    let owner = EngineEvent::ClipRequestRetired(PreparedClipPlayback::stop(TrackId::new(), 1));
    assert_ne!(owner, owner);
}
