use clap_sys::{
    audio_buffer::clap_audio_buffer, entry::clap_plugin_entry, ext::audio_ports::*,
    ext::latency::*, ext::state::*, factory::plugin_factory::*, host::clap_host, plugin::*,
    process::*, stream::*, version::CLAP_VERSION,
};
use std::ffi::{c_char, c_void, CStr};

struct State {
    active: bool,
    processing: bool,
    processing_thread: usize,
    max_frames: u32,
    sample_rate: f64,
    host: *const clap_host,
    timing: crate::timing::Timing,
    main_thread: usize,
    instrument: bool,
    playing: bool,
}

unsafe extern "C" fn init(_: *const c_char) -> bool {
    true
}
unsafe extern "C" fn deinit() {}
unsafe extern "C" fn factory(id: *const c_char) -> *const c_void {
    if CStr::from_ptr(id) == CLAP_PLUGIN_FACTORY_ID {
        &FACTORY as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}
#[no_mangle]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(init),
    deinit: Some(deinit),
    get_factory: Some(factory),
};

static DESCRIPTOR: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: c"vibez.fixture.routing".as_ptr(),
    name: c"Routing Probe".as_ptr(),
    vendor: c"Vibez tests".as_ptr(),
    url: c"".as_ptr(),
    manual_url: c"".as_ptr(),
    support_url: c"".as_ptr(),
    version: c"1".as_ptr(),
    description: c"Controlled external-input probe".as_ptr(),
    features: std::ptr::null(),
};
static INSTRUMENT_DESCRIPTOR: clap_plugin_descriptor = clap_plugin_descriptor {
    id: c"vibez.fixture.instrument".as_ptr(),
    name: c"Pulse Instrument".as_ptr(),
    ..DESCRIPTOR
};
unsafe extern "C" fn count(_: *const clap_plugin_factory) -> u32 {
    2
}
unsafe extern "C" fn descriptor(
    _: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    if index == 0 {
        &DESCRIPTOR
    } else if index == 1 {
        &INSTRUMENT_DESCRIPTOR
    } else {
        std::ptr::null()
    }
}
unsafe extern "C" fn create(
    _: *const clap_plugin_factory,
    host: *const clap_host,
    id: *const c_char,
) -> *const clap_plugin {
    let instrument = CStr::from_ptr(id) == c"vibez.fixture.instrument";
    if !instrument && CStr::from_ptr(id) != c"vibez.fixture.routing" {
        return std::ptr::null();
    }
    let state = Box::into_raw(Box::new(State {
        active: false,
        processing: false,
        processing_thread: 0,
        max_frames: 0,
        sample_rate: 0.0,
        host,
        timing: Default::default(),
        main_thread: crate::thread_id(),
        instrument,
        playing: false,
    }));
    Box::into_raw(Box::new(clap_plugin {
        desc: if instrument {
            &INSTRUMENT_DESCRIPTOR
        } else {
            &DESCRIPTOR
        },
        plugin_data: state.cast(),
        init: Some(plugin_init),
        destroy: Some(destroy),
        activate: Some(activate),
        deactivate: Some(deactivate),
        start_processing: Some(start),
        stop_processing: Some(stop),
        reset: Some(reset),
        process: Some(process),
        get_extension: Some(extension),
        on_main_thread: Some(main),
    }))
}
static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(count),
    get_plugin_descriptor: Some(descriptor),
    create_plugin: Some(create),
};
unsafe fn state<'a>(plugin: *const clap_plugin) -> &'a mut State {
    &mut *((*plugin).plugin_data as *mut State)
}
unsafe extern "C" fn plugin_init(_: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    if state(plugin).processing || state(plugin).main_thread != crate::thread_id() {
        crate::lifecycle_error();
    }
    drop(Box::from_raw((*plugin).plugin_data as *mut State));
    drop(Box::from_raw(plugin as *mut clap_plugin));
}
unsafe extern "C" fn activate(plugin: *const clap_plugin, rate: f64, min: u32, max: u32) -> bool {
    if min != 1 || max == 0 || !state(plugin).timing.activation_allowed() {
        return false;
    }
    if state(plugin).processing || state(plugin).main_thread != crate::thread_id() {
        return false;
    }
    state(plugin).timing.activate();
    state(plugin).active = true;
    state(plugin).max_frames = max;
    state(plugin).sample_rate = rate;
    true
}
unsafe extern "C" fn deactivate(plugin: *const clap_plugin) {
    state(plugin).active = false;
}
unsafe extern "C" fn start(plugin: *const clap_plugin) -> bool {
    state(plugin).processing_thread = crate::thread_id();
    state(plugin).processing = state(plugin).active;
    state(plugin).processing
}
unsafe extern "C" fn stop(plugin: *const clap_plugin) {
    if state(plugin).processing && state(plugin).processing_thread != crate::thread_id() {
        crate::lifecycle_error();
        return;
    }
    state(plugin).processing = false;
}
unsafe extern "C" fn reset(plugin: *const clap_plugin) {
    state(plugin).timing.reset();
    state(plugin).playing = false;
}
unsafe extern "C" fn main(_: *const clap_plugin) {}
unsafe extern "C" fn extension(_: *const clap_plugin, id: *const c_char) -> *const c_void {
    match CStr::from_ptr(id) {
        id if id == CLAP_EXT_AUDIO_PORTS => &PORTS as *const _ as *const c_void,
        id if id == CLAP_EXT_STATE => &STATE as *const _ as *const c_void,
        id if id == CLAP_EXT_LATENCY => &LATENCY as *const _ as *const c_void,
        _ => std::ptr::null(),
    }
}
unsafe extern "C" fn latency(plugin: *const clap_plugin) -> u32 {
    if state(plugin).main_thread != crate::thread_id()
        || !state(plugin).active
        || state(plugin).processing
    {
        return u32::MAX;
    }
    state(plugin).timing.report()
}
static LATENCY: clap_plugin_latency = clap_plugin_latency { get: Some(latency) };
unsafe extern "C" fn port_count(plugin: *const clap_plugin, input: bool) -> u32 {
    if input {
        if state(plugin).instrument {
            0
        } else {
            4
        }
    } else {
        1
    }
}
unsafe extern "C" fn port_info(
    plugin: *const clap_plugin,
    index: u32,
    input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if state(plugin).active || index >= if input { 4 } else { 1 } {
        return false;
    }
    *info = std::mem::zeroed();
    (*info).id = index + 10;
    (*info).channel_count = match index {
        1 => state(plugin).timing.mono_channels() as u32,
        3 => 6,
        _ => 2,
    };
    (*info).flags = if index == 0 {
        CLAP_AUDIO_PORT_IS_MAIN
    } else {
        0
    };
    let name = match index {
        0 => "Main",
        1 => "Mono detector",
        2 => "Stereo detector",
        _ => "Surround detector",
    };
    for (target, byte) in (*info).name.iter_mut().zip(name.as_bytes()) {
        *target = *byte as c_char;
    }
    (*info).port_type = if (*info).channel_count == 1 {
        CLAP_PORT_MONO.as_ptr()
    } else {
        CLAP_PORT_STEREO.as_ptr()
    };
    (*info).in_place_pair = u32::MAX;
    true
}
static PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(port_count),
    get: Some(port_info),
};
unsafe fn sample(bus: &clap_audio_buffer, channel: usize, frame: usize) -> f32 {
    *(*bus.data32.add(channel)).add(frame)
}
unsafe extern "C" fn process(
    plugin: *const clap_plugin,
    data: *const clap_process,
) -> clap_process_status {
    let data = &*data;
    if !data.transport.is_null() {
        let seconds = (*data.transport).song_pos_seconds as f64
            / clap_sys::fixedpoint::CLAP_SECTIME_FACTOR as f64;
        let sample = (seconds * state(plugin).sample_rate).round() as u64;
        state(plugin).timing.set_context(sample);
    }
    if state(plugin).instrument {
        if !state(plugin).processing
            || data.audio_inputs_count != 0
            || data.audio_outputs_count != 1
            || data.frames_count > state(plugin).max_frames
        {
            return CLAP_PROCESS_ERROR;
        }
        let mut event_index = 0;
        let events = &*data.in_events;
        let output = &*data.audio_outputs;
        for frame in 0..data.frames_count {
            while event_index < (events.size.unwrap())(events) {
                let header = (events.get.unwrap())(events, event_index);
                if (*header).time > frame {
                    break;
                }
                if (*header).space_id == clap_sys::events::CLAP_CORE_EVENT_SPACE_ID {
                    if (*header).type_ == clap_sys::events::CLAP_EVENT_NOTE_ON {
                        state(plugin).playing = true;
                    }
                    if (*header).type_ == clap_sys::events::CLAP_EVENT_NOTE_OFF {
                        state(plugin).playing = false;
                    }
                }
                event_index += 1;
            }
            let level = if state(plugin).playing { 0.75 } else { 0.0 };
            let values = state(plugin).timing.frame([level; 2], 0.0, [0.0; 2]);
            for (channel, value) in values.into_iter().enumerate() {
                *(*output.data32.add(channel)).add(frame as usize) = value;
            }
        }
        return CLAP_PROCESS_CONTINUE;
    }
    if !state(plugin).processing
        || data.frames_count > state(plugin).max_frames
        || data.audio_inputs_count != 4
        || data.audio_outputs_count != 1
    {
        return CLAP_PROCESS_ERROR;
    }
    let inputs = std::slice::from_raw_parts(data.audio_inputs, 4);
    let output = &*data.audio_outputs;
    if inputs[1].channel_count != state(plugin).timing.mono_channels() as u32
        || inputs[2].channel_count != 2
    {
        return CLAP_PROCESS_ERROR;
    }
    for frame in 0..data.frames_count as usize {
        let main = [sample(&inputs[0], 0, frame), sample(&inputs[0], 1, frame)];
        let stereo = [sample(&inputs[2], 0, frame), sample(&inputs[2], 1, frame)];
        let values = state(plugin)
            .timing
            .frame(main, sample(&inputs[1], 0, frame), stereo);
        for (channel, value) in values.into_iter().enumerate() {
            *(*output.data32.add(channel)).add(frame) = value;
        }
    }
    CLAP_PROCESS_CONTINUE
}

unsafe extern "C" fn save(plugin: *const clap_plugin, stream: *const clap_ostream) -> bool {
    let bytes = state(plugin).timing.state();
    ((*stream).write.unwrap())(stream, bytes.as_ptr().cast(), 12) == 12
}
unsafe extern "C" fn load(plugin: *const clap_plugin, stream: *const clap_istream) -> bool {
    let mut bytes = [0u8; 12];
    if ((*stream).read.unwrap())(stream, bytes.as_mut_ptr().cast(), 12) != 12
        || !state(plugin).timing.configure(&bytes)
    {
        return false;
    }
    if state(plugin).active {
        ((*state(plugin).host).request_restart.unwrap())(state(plugin).host);
    }
    true
}
static STATE: clap_plugin_state = clap_plugin_state {
    save: Some(save),
    load: Some(load),
};
