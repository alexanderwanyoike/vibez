use vibez_core::audio_context::DeviceAudioContext;

pub(crate) fn clap(context: DeviceAudioContext) -> clap_sys::events::clap_event_transport {
    use clap_sys::events::*;
    let mut transport: clap_event_transport = unsafe { std::mem::zeroed() };
    transport.header.size = std::mem::size_of::<clap_event_transport>() as u32;
    transport.header.space_id = CLAP_CORE_EVENT_SPACE_ID;
    transport.header.type_ = CLAP_EVENT_TRANSPORT;
    transport.flags = CLAP_TRANSPORT_HAS_TEMPO
        | CLAP_TRANSPORT_HAS_BEATS_TIMELINE
        | CLAP_TRANSPORT_HAS_SECONDS_TIMELINE;
    if context.playing {
        transport.flags |= CLAP_TRANSPORT_IS_PLAYING;
    }
    transport.tempo = context.bpm;
    transport.song_pos_beats =
        (context.beats() * clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR as f64).round() as i64;
    transport.song_pos_seconds =
        (context.seconds() * clap_sys::fixedpoint::CLAP_SECTIME_FACTOR as f64).round() as i64;
    transport
}

pub(crate) fn vst3(context: DeviceAudioContext) -> vst3::Steinberg::Vst::ProcessContext {
    use vst3::Steinberg::Vst::ProcessContext_::StatesAndFlags_::*;
    let mut transport: vst3::Steinberg::Vst::ProcessContext = unsafe { std::mem::zeroed() };
    transport.state = kTempoValid | kProjectTimeMusicValid | kContTimeValid;
    if context.playing {
        transport.state |= kPlaying;
    }
    transport.sampleRate = context.sample_rate as f64;
    transport.projectTimeSamples = context.musical_sample as i64;
    transport.continousTimeSamples = context.continuous_sample as i64;
    transport.projectTimeMusic = context.beats();
    transport.tempo = context.bpm;
    transport
}
