//! Hosted plugins report the notes they output, timed inside the block that played them.

#[allow(dead_code)]
mod support;
#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;

use vibez_core::note_output::{BlockNote, NoteOutput};

fn reports_output_notes_with_their_frames(format: &str) {
    let fixture = support::Fixture::new();
    let mut plugin = fixture.load_instrument(format, 64);
    plugin.note_on_at(60, 100, 5);
    plugin.note_off_at(60, 20);
    let mut buffer = [0.0; 128];

    let activity = support::allocation::count_activity(|| plugin.process_audio(&mut buffer, 2));

    assert_eq!(activity, (0, 0), "{format}");
    assert_eq!(
        plugin.note_output().map(NoteOutput::notes),
        Some(&[BlockNote::on(5, 72, 100), BlockNote::off(20, 72)][..]),
        "{format}"
    );
    plugin.process_audio(&mut buffer, 2);
    assert_eq!(
        plugin.note_output().map(NoteOutput::notes),
        Some(&[][..]),
        "{format} keeps only the last block's notes"
    );
    plugin.stop_processing();
    plugin.deactivate();
}

#[test]
fn a_clap_plugin_reports_the_notes_it_outputs_with_their_frames() {
    reports_output_notes_with_their_frames("clap");
}

#[test]
fn a_vst3_plugin_reports_the_notes_it_outputs_with_their_frames() {
    reports_output_notes_with_their_frames("vst3");
}

#[test]
fn a_hosted_instrument_reports_its_plugins_notes_after_each_render() {
    use vibez_instruments::Instrument;
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instrument =
            vibez_plugin_host::PluginInstrumentWrapper::new(fixture.load_instrument(format, 64));
        instrument.note_on_at(48, 90, 7);
        instrument.render(&mut [0.0; 128], 2);

        assert_eq!(
            instrument.note_output().map(NoteOutput::notes),
            Some(&[BlockNote::on(7, 60, 90)][..]),
            "{format}"
        );
        instrument.stop_processing();
    }
}

#[test]
fn a_failed_process_call_reports_no_notes() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut plugin = fixture.load_instrument(format, 64);
        let fail_next_process: Vec<u8> = [0u32, 0, 1 << 24]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert!(plugin.load_state(&fail_next_process), "{format}");
        plugin.note_on_at(60, 100, 5);
        plugin.process_audio(&mut [0.0; 128], 2);

        assert_eq!(
            plugin.note_output().map(NoteOutput::notes),
            Some(&[][..]),
            "{format}"
        );
        plugin.stop_processing();
        plugin.deactivate();
    }
}
