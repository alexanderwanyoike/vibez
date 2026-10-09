// ── Stub IParameterChanges ──
// DPF-based plugins assert on null input/outputParameterChanges and
// JUCE tolerates but prefers them. This is a stateless, static COM
// object: no parameters in, additions rejected.

#[repr(C)]
struct ParamChangesVtbl {
    query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    get_parameter_count: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32,
    get_parameter_data:
        unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> *mut std::ffi::c_void,
    add_parameter_data: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u32,
        *mut i32,
    ) -> *mut std::ffi::c_void,
}

unsafe extern "system" fn pc_query_interface(
    this: *mut std::ffi::c_void,
    _iid: *const u8,
    obj: *mut *mut std::ffi::c_void,
) -> i32 {
    // Static object: answer everything with ourselves; refcounting is
    // a no-op so over-answering is harmless.
    unsafe { *obj = this };
    0
}
unsafe extern "system" fn pc_add_ref(_this: *mut std::ffi::c_void) -> u32 {
    1
}
unsafe extern "system" fn pc_release(_this: *mut std::ffi::c_void) -> u32 {
    1
}
unsafe extern "system" fn pc_count(_this: *mut std::ffi::c_void) -> i32 {
    0
}
unsafe extern "system" fn pc_get_data(
    _this: *mut std::ffi::c_void,
    _index: i32,
) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}
unsafe extern "system" fn pc_add_data(
    _this: *mut std::ffi::c_void,
    _id: *const u32,
    index: *mut i32,
) -> *mut std::ffi::c_void {
    // Hand back a discard-queue: DPF asserts on a null return and
    // then writes its output points into whatever it gets.
    if !index.is_null() {
        unsafe { *index = 0 };
    }
    &PARAM_QUEUE_STUB as *const ParamQueueStub as *mut std::ffi::c_void
}

// IParamValueQueue stub: identifies as parameter 0, holds no points,
// accepts (and discards) added points.
#[repr(C)]
struct ParamQueueVtbl {
    query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    get_parameter_id: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    get_point_count: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32,
    get_point: unsafe extern "system" fn(*mut std::ffi::c_void, i32, *mut i32, *mut f64) -> i32,
    add_point: unsafe extern "system" fn(*mut std::ffi::c_void, i32, f64, *mut i32) -> i32,
}

unsafe extern "system" fn pq_parameter_id(_this: *mut std::ffi::c_void) -> u32 {
    0
}
unsafe extern "system" fn pq_point_count(_this: *mut std::ffi::c_void) -> i32 {
    0
}
unsafe extern "system" fn pq_get_point(
    _this: *mut std::ffi::c_void,
    _index: i32,
    _offset: *mut i32,
    _value: *mut f64,
) -> i32 {
    1 // kResultFalse
}
unsafe extern "system" fn pq_add_point(
    _this: *mut std::ffi::c_void,
    _offset: i32,
    _value: f64,
    index: *mut i32,
) -> i32 {
    if !index.is_null() {
        unsafe { *index = 0 };
    }
    0
}

static PARAM_QUEUE_VTBL: ParamQueueVtbl = ParamQueueVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_id: pq_parameter_id,
    get_point_count: pq_point_count,
    get_point: pq_get_point,
    add_point: pq_add_point,
};

#[repr(C)]
struct ParamQueueStub {
    vtbl: *const ParamQueueVtbl,
}
unsafe impl Sync for ParamQueueStub {}
static PARAM_QUEUE_STUB: ParamQueueStub = ParamQueueStub {
    vtbl: &PARAM_QUEUE_VTBL,
};

static PARAM_CHANGES_VTBL: ParamChangesVtbl = ParamChangesVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_count: pc_count,
    get_parameter_data: pc_get_data,
    add_parameter_data: pc_add_data,
};

#[repr(C)]
struct ParamChangesStub {
    vtbl: *const ParamChangesVtbl,
}
unsafe impl Sync for ParamChangesStub {}
static PARAM_CHANGES_STUB: ParamChangesStub = ParamChangesStub {
    vtbl: &PARAM_CHANGES_VTBL,
};

fn param_changes_stub() -> *mut std::ffi::c_void {
    &PARAM_CHANGES_STUB as *const ParamChangesStub as *mut std::ffi::c_void
}

// ── Live IParameterChanges (input) ──
// Stack-allocated per process() call; spec-compliant plugins do not
// retain the pointer past the call.

#[repr(C)]
struct LiveParamQueue {
    vtbl: *const ParamQueueVtbl,
    id: u32,
    value: f64,
}

unsafe extern "system" fn lpq_parameter_id(this: *mut std::ffi::c_void) -> u32 {
    unsafe { (*(this as *const LiveParamQueue)).id }
}
unsafe extern "system" fn lpq_point_count(_this: *mut std::ffi::c_void) -> i32 {
    1
}
unsafe extern "system" fn lpq_get_point(
    this: *mut std::ffi::c_void,
    index: i32,
    offset: *mut i32,
    value: *mut f64,
) -> i32 {
    if index != 0 {
        return 1; // kResultFalse
    }
    unsafe {
        if !offset.is_null() {
            *offset = 0;
        }
        if !value.is_null() {
            *value = (*(this as *const LiveParamQueue)).value;
        }
    }
    0
}

static LIVE_PARAM_QUEUE_VTBL: ParamQueueVtbl = ParamQueueVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_id: lpq_parameter_id,
    get_point_count: lpq_point_count,
    get_point: lpq_get_point,
    add_point: pq_add_point,
};

#[repr(C)]
struct LiveParamChanges {
    vtbl: *const ParamChangesVtbl,
    queues: *const LiveParamQueue,
    len: usize,
}

unsafe extern "system" fn lpc_count(this: *mut std::ffi::c_void) -> i32 {
    unsafe { (*(this as *const LiveParamChanges)).len as i32 }
}
unsafe extern "system" fn lpc_get_data(
    this: *mut std::ffi::c_void,
    index: i32,
) -> *mut std::ffi::c_void {
    let changes = unsafe { &*(this as *const LiveParamChanges) };
    if index < 0 || index as usize >= changes.len {
        return std::ptr::null_mut();
    }
    unsafe { changes.queues.add(index as usize) as *mut std::ffi::c_void }
}

static LIVE_PARAM_CHANGES_VTBL: ParamChangesVtbl = ParamChangesVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_count: lpc_count,
    get_parameter_data: lpc_get_data,
    add_parameter_data: pc_add_data,
};

/// Enumerate parameters via IEditController.
/// Vtable slots: FUnknown 0-2, IPluginBase 3-4, then
/// setComponentState(5) setState(6) getState(7) getParameterCount(8)
/// getParameterInfo(9) ... getParamNormalized(14).
fn query_vst3_params(
    controller: *mut std::ffi::c_void,
) -> (Vec<ParamDescriptor>, Vec<f32>, Vec<u32>) {
    type GetCountFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
    type GetInfoFn = unsafe extern "system" fn(
        *mut std::ffi::c_void,
        i32,
        *mut vst3::Steinberg::Vst::ParameterInfo,
    ) -> i32;
    type GetNormalizedFn = unsafe extern "system" fn(*mut std::ffi::c_void, u32) -> f64;

    let mut descriptors = Vec::new();
    let mut values = Vec::new();
    let mut ids = Vec::new();

    unsafe {
        let vtbl = *(controller as *const *const *const std::ffi::c_void);
        let get_count: GetCountFn = std::mem::transmute(*vtbl.add(8));
        let get_info: GetInfoFn = std::mem::transmute(*vtbl.add(9));
        let get_normalized: GetNormalizedFn = std::mem::transmute(*vtbl.add(14));

        let count = get_count(controller).max(0);
        for i in 0..count {
            let mut info: vst3::Steinberg::Vst::ParameterInfo = std::mem::zeroed();
            if get_info(controller, i, &mut info) != 0 {
                continue;
            }
            // Skip read-only / hidden params.
            const CAN_AUTOMATE: i32 = 1 << 0;
            const IS_READ_ONLY: i32 = 1 << 1;
            if info.flags & IS_READ_ONLY != 0 {
                continue;
            }
            if info.flags & CAN_AUTOMATE == 0 {
                continue;
            }
            let title: String = char16_to_string(&info.title);
            let name_static: &'static str = Box::leak(title.into_boxed_str());
            descriptors.push(ParamDescriptor {
                name: name_static,
                min: 0.0,
                max: 1.0,
                default: info.defaultNormalizedValue as f32,
                unit: "",
            });
            values.push(get_normalized(controller, info.id) as f32);
            ids.push(info.id);
        }
    }
    (descriptors, values, ids)
}

fn char16_to_string(chars: &[u16]) -> String {
    let units: Vec<u16> = chars.iter().take_while(|&&c| c != 0).copied().collect();
    String::from_utf16_lossy(&units)
}

/// Output of [`Vst3PluginInstance::load_partial`]: a dlopen'd module
/// with no plugin code executed yet.
pub struct PartialVst3Plugin {
    path: std::path::PathBuf,
    lib: libloading::Library,
    class_uid: String,
    is_instrument: bool,
}

#[allow(dead_code)]
struct NoteEvent {
    is_on: bool,
    pitch: u8,
    velocity: u8,
    frame_offset: u32,
}

// ── Live IEventList (input) ──
// Stack-allocated for each process() call. The VST3 processor reads this
// COM object synchronously and must not retain it after process() returns.

#[repr(C)]
struct EventListVtbl {
    query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    get_event_count: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32,
    get_event: unsafe extern "system" fn(*mut std::ffi::c_void, i32, *mut VstEvent) -> i32,
    add_event: unsafe extern "system" fn(*mut std::ffi::c_void, *mut VstEvent) -> i32,
}

#[repr(C)]
struct LiveEventList<'a> {
    vtbl: *const EventListVtbl,
    events: &'a [NoteEvent],
}

impl<'a> LiveEventList<'a> {
    fn new(events: &'a [NoteEvent]) -> Self {
        Self {
            vtbl: &LIVE_EVENT_LIST_VTBL,
            events,
        }
    }

    fn event_count(&self) -> usize {
        self.events.len()
    }

    fn event(&self, index: usize) -> Option<VstEvent> {
        self.events.get(index).map(vst_event)
    }

    fn as_raw_mut(&mut self) -> *mut std::ffi::c_void {
        self as *mut Self as *mut std::ffi::c_void
    }
}

fn vst_event(event: &NoteEvent) -> VstEvent {
    let sample_offset = event.frame_offset.min(i32::MAX as u32) as i32;
    if event.is_on {
        VstEvent {
            busIndex: 0,
            sampleOffset: sample_offset,
            ppqPosition: 0.0,
            flags: 0,
            r#type: vst3::Steinberg::Vst::Event_::EventTypes_::kNoteOnEvent as u16,
            __field0: VstEventData {
                noteOn: NoteOnEvent {
                    channel: 0,
                    pitch: event.pitch as i16,
                    tuning: 0.0,
                    velocity: event.velocity as f32 / 127.0,
                    length: 0,
                    noteId: -1,
                },
            },
        }
    } else {
        VstEvent {
            busIndex: 0,
            sampleOffset: sample_offset,
            ppqPosition: 0.0,
            flags: 0,
            r#type: vst3::Steinberg::Vst::Event_::EventTypes_::kNoteOffEvent as u16,
            __field0: VstEventData {
                noteOff: NoteOffEvent {
                    channel: 0,
                    pitch: event.pitch as i16,
                    velocity: event.velocity as f32 / 127.0,
                    noteId: -1,
                    tuning: 0.0,
                },
            },
        }
    }
}

unsafe extern "system" fn event_list_count(this: *mut std::ffi::c_void) -> i32 {
    unsafe { (*(this as *const LiveEventList<'_>)).event_count() as i32 }
}

unsafe extern "system" fn event_list_get(
    this: *mut std::ffi::c_void,
    index: i32,
    event: *mut VstEvent,
) -> i32 {
    if index < 0 || event.is_null() {
        return 1;
    }
    let Some(value) = (unsafe { &*(this as *const LiveEventList<'_>) }).event(index as usize)
    else {
        return 1;
    };
    unsafe { *event = value };
    0
}

unsafe extern "system" fn event_list_add(
    _this: *mut std::ffi::c_void,
    _event: *mut VstEvent,
) -> i32 {
    1
}

static LIVE_EVENT_LIST_VTBL: EventListVtbl = EventListVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_event_count: event_list_count,
    get_event: event_list_get,
    add_event: event_list_add,
};

// VST3 IComponent IID: {E831FF31-F2D5-4301-928E-BBEE25697802}
const ICOMPONENT_IID: [u8; 16] = crate::vst3_tuid([
    0xE8, 0x31, 0xFF, 0x31, 0xF2, 0xD5, 0x43, 0x01, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78, 0x02,
]);

// VST3 IAudioProcessor IID: {42043F99-B7DA-453C-A569-E79D9AAEC33D}
const IAUDIOPROCESSOR_IID: [u8; 16] = crate::vst3_tuid([
    0x42, 0x04, 0x3F, 0x99, 0xB7, 0xDA, 0x45, 0x3C, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3, 0x3D,
]);

// VST3 IConnectionPoint IID: {70A4156F-6E6E-4026-9891-48BFAA60D8D1}
const ICONNECTIONPOINT_IID: [u8; 16] = crate::vst3_tuid([
    0x70, 0xA4, 0x15, 0x6F, 0x6E, 0x6E, 0x40, 0x26, 0x98, 0x91, 0x48, 0xBF, 0xAA, 0x60, 0xD8, 0xD1,
]);

/// Raw ProcessSetup matching VST3 C layout.
#[repr(C)]
struct ProcessSetupRaw {
    process_mode: i32,
    symbolic_sample_size: i32,
    max_samples_per_block: i32,
    sample_rate: f64,
}

/// Raw AudioBusBuffers matching VST3 C layout.
#[repr(C)]
struct AudioBusBuffersRaw {
    num_channels: i32,
    silence_flags: u64,
    channel_buffers32: *mut *mut f32,
}

/// Raw ProcessData matching VST3 C layout.
#[repr(C)]
struct ProcessDataRaw {
    process_mode: i32,
    symbolic_sample_size: i32,
    num_samples: i32,
    num_inputs: i32,
    num_outputs: i32,
    inputs: *mut AudioBusBuffersRaw,
    outputs: *mut AudioBusBuffersRaw,
    input_parameter_changes: *mut std::ffi::c_void,
    output_parameter_changes: *mut std::ffi::c_void,
    input_events: *mut std::ffi::c_void,
    output_events: *mut std::ffi::c_void,
    process_context: *mut std::ffi::c_void,
}

/// Helper: get vtable pointer from COM object.
pub(super) unsafe fn vtbl(obj: *mut std::ffi::c_void) -> *const *const std::ffi::c_void {
    *(obj as *const *const *const std::ffi::c_void)
}

