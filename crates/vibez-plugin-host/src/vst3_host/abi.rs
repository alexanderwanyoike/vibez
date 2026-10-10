//! VST3 process layouts, event adapters and parameter ABI objects.

use vibez_core::effect::ParamDescriptor;
use vst3::Steinberg::Vst::{
    Event as VstEvent, Event__type0 as VstEventData, NoteOffEvent, NoteOnEvent,
};

// ── Stub IParameterChanges ──
// DPF-based plugins assert on null input/outputParameterChanges and
// JUCE tolerates but prefers them. This is a stateless, static COM
// object: no parameters in, additions rejected.

#[repr(C)]
pub(super) struct ParamChangesVtbl {
    pub(super) query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    pub(super) add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    pub(super) release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    pub(super) get_parameter_count: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32,
    pub(super) get_parameter_data:
        unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> *mut std::ffi::c_void,
    pub(super) add_parameter_data: unsafe extern "system" fn(
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
pub(super) struct ParamQueueVtbl {
    pub(super) query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    pub(super) add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    pub(super) release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    pub(super) get_parameter_id: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    pub(super) get_point_count: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32,
    pub(super) get_point:
        unsafe extern "system" fn(*mut std::ffi::c_void, i32, *mut i32, *mut f64) -> i32,
    pub(super) add_point:
        unsafe extern "system" fn(*mut std::ffi::c_void, i32, f64, *mut i32) -> i32,
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

pub(super) static PARAM_QUEUE_VTBL: ParamQueueVtbl = ParamQueueVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_id: pq_parameter_id,
    get_point_count: pq_point_count,
    get_point: pq_get_point,
    add_point: pq_add_point,
};

#[repr(C)]
pub(super) struct ParamQueueStub {
    pub(super) vtbl: *const ParamQueueVtbl,
}
unsafe impl Sync for ParamQueueStub {}
pub(super) static PARAM_QUEUE_STUB: ParamQueueStub = ParamQueueStub {
    vtbl: &PARAM_QUEUE_VTBL,
};

pub(super) static PARAM_CHANGES_VTBL: ParamChangesVtbl = ParamChangesVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_count: pc_count,
    get_parameter_data: pc_get_data,
    add_parameter_data: pc_add_data,
};

#[repr(C)]
pub(super) struct ParamChangesStub {
    pub(super) vtbl: *const ParamChangesVtbl,
}
unsafe impl Sync for ParamChangesStub {}
pub(super) static PARAM_CHANGES_STUB: ParamChangesStub = ParamChangesStub {
    vtbl: &PARAM_CHANGES_VTBL,
};

pub(super) fn param_changes_stub() -> *mut std::ffi::c_void {
    &PARAM_CHANGES_STUB as *const ParamChangesStub as *mut std::ffi::c_void
}

// ── Live IParameterChanges (input) ──
// Stack-allocated per process() call; spec-compliant plugins do not
// retain the pointer past the call.

#[repr(C)]
pub(super) struct LiveParamQueue {
    pub(super) vtbl: *const ParamQueueVtbl,
    pub(super) id: u32,
    pub(super) value: f64,
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

pub(super) static LIVE_PARAM_QUEUE_VTBL: ParamQueueVtbl = ParamQueueVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_id: lpq_parameter_id,
    get_point_count: lpq_point_count,
    get_point: lpq_get_point,
    add_point: pq_add_point,
};

#[repr(C)]
pub(super) struct LiveParamChanges {
    pub(super) vtbl: *const ParamChangesVtbl,
    pub(super) queues: *const LiveParamQueue,
    pub(super) len: usize,
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

pub(super) static LIVE_PARAM_CHANGES_VTBL: ParamChangesVtbl = ParamChangesVtbl {
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
pub(super) fn query_vst3_params(
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

pub(super) fn char16_to_string(chars: &[u16]) -> String {
    let units: Vec<u16> = chars.iter().take_while(|&&c| c != 0).copied().collect();
    String::from_utf16_lossy(&units)
}

#[allow(dead_code)]
pub(super) struct NoteEvent {
    pub(super) is_on: bool,
    pub(super) pitch: u8,
    pub(super) velocity: u8,
    pub(super) frame_offset: u32,
}

// ── Live IEventList (input) ──
// Stack-allocated for each process() call. The VST3 processor reads this
// COM object synchronously and must not retain it after process() returns.

#[repr(C)]
pub(super) struct EventListVtbl {
    pub(super) query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    pub(super) add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    pub(super) release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    pub(super) get_event_count: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32,
    pub(super) get_event:
        unsafe extern "system" fn(*mut std::ffi::c_void, i32, *mut VstEvent) -> i32,
    pub(super) add_event: unsafe extern "system" fn(*mut std::ffi::c_void, *mut VstEvent) -> i32,
}

#[repr(C)]
pub(super) struct LiveEventList<'a> {
    pub(super) vtbl: *const EventListVtbl,
    pub(super) events: &'a [NoteEvent],
}

impl<'a> LiveEventList<'a> {
    pub(super) fn new(events: &'a [NoteEvent]) -> Self {
        Self {
            vtbl: &LIVE_EVENT_LIST_VTBL,
            events,
        }
    }

    pub(super) fn event_count(&self) -> usize {
        self.events.len()
    }

    pub(super) fn event(&self, index: usize) -> Option<VstEvent> {
        self.events.get(index).map(vst_event)
    }

    pub(super) fn as_raw_mut(&mut self) -> *mut std::ffi::c_void {
        self as *mut Self as *mut std::ffi::c_void
    }
}

pub(super) fn vst_event(event: &NoteEvent) -> VstEvent {
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

pub(super) static LIVE_EVENT_LIST_VTBL: EventListVtbl = EventListVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_event_count: event_list_count,
    get_event: event_list_get,
    add_event: event_list_add,
};

// VST3 IComponent IID: {E831FF31-F2D5-4301-928E-BBEE25697802}
pub(super) const ICOMPONENT_IID: [u8; 16] = crate::vst3_tuid([
    0xE8, 0x31, 0xFF, 0x31, 0xF2, 0xD5, 0x43, 0x01, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78, 0x02,
]);

// VST3 IAudioProcessor IID: {42043F99-B7DA-453C-A569-E79D9AAEC33D}
pub(super) const IAUDIOPROCESSOR_IID: [u8; 16] = crate::vst3_tuid([
    0x42, 0x04, 0x3F, 0x99, 0xB7, 0xDA, 0x45, 0x3C, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3, 0x3D,
]);

// VST3 IConnectionPoint IID: {70A4156F-6E6E-4026-9891-48BFAA60D8D1}
pub(super) const ICONNECTIONPOINT_IID: [u8; 16] = crate::vst3_tuid([
    0x70, 0xA4, 0x15, 0x6F, 0x6E, 0x6E, 0x40, 0x26, 0x98, 0x91, 0x48, 0xBF, 0xAA, 0x60, 0xD8, 0xD1,
]);

/// Raw ProcessSetup matching VST3 C layout.
#[repr(C)]
pub(super) struct ProcessSetupRaw {
    pub(super) process_mode: i32,
    pub(super) symbolic_sample_size: i32,
    pub(super) max_samples_per_block: i32,
    pub(super) sample_rate: f64,
}

/// Raw AudioBusBuffers matching VST3 C layout.
#[repr(C)]
pub(super) struct AudioBusBuffersRaw {
    pub(super) num_channels: i32,
    pub(super) silence_flags: u64,
    pub(super) channel_buffers32: *mut *mut f32,
}

/// Raw ProcessData matching VST3 C layout.
#[repr(C)]
pub(super) struct ProcessDataRaw {
    pub(super) process_mode: i32,
    pub(super) symbolic_sample_size: i32,
    pub(super) num_samples: i32,
    pub(super) num_inputs: i32,
    pub(super) num_outputs: i32,
    pub(super) inputs: *mut AudioBusBuffersRaw,
    pub(super) outputs: *mut AudioBusBuffersRaw,
    pub(super) input_parameter_changes: *mut std::ffi::c_void,
    pub(super) output_parameter_changes: *mut std::ffi::c_void,
    pub(super) input_events: *mut std::ffi::c_void,
    pub(super) output_events: *mut std::ffi::c_void,
    pub(super) process_context: *mut std::ffi::c_void,
}

/// Helper: get vtable pointer from COM object.
pub(super) unsafe fn vtbl(obj: *mut std::ffi::c_void) -> *const *const std::ffi::c_void {
    *(obj as *const *const *const std::ffi::c_void)
}

#[cfg(test)]
mod tests {
    use vst3::Steinberg::Vst::Event_::EventTypes_::{kNoteOffEvent, kNoteOnEvent};

    /// Hand-written IIDs must match the SDK-generated constants in the
    /// vst3 crate. A single wrong byte makes every plugin reject the
    /// queryInterface call (a 0x3F-for-0x3D typo in IAudioProcessor
    /// once broke loading of ALL VST3 plugins).
    fn assert_iid(ours: [u8; 16], sdk: [::std::os::raw::c_char; 16]) {
        let sdk_bytes: Vec<u8> = sdk.iter().map(|b| *b as u8).collect();
        assert_eq!(ours.as_slice(), sdk_bytes.as_slice());
    }

    #[test]
    fn icomponent_iid_matches_sdk() {
        assert_iid(super::ICOMPONENT_IID, vst3::Steinberg::Vst::IComponent_iid);
    }

    #[test]
    fn iconnectionpoint_iid_matches_sdk() {
        assert_iid(
            super::ICONNECTIONPOINT_IID,
            vst3::Steinberg::Vst::IConnectionPoint_iid,
        );
    }

    #[test]
    fn iaudioprocessor_iid_matches_sdk() {
        assert_iid(
            super::IAUDIOPROCESSOR_IID,
            vst3::Steinberg::Vst::IAudioProcessor_iid,
        );
    }

    #[test]
    fn live_event_list_exposes_timed_clip_notes_to_vst3() {
        let mut events = vec![
            super::NoteEvent {
                is_on: false,
                pitch: 64,
                velocity: 0,
                frame_offset: 91,
            },
            super::NoteEvent {
                is_on: true,
                pitch: 64,
                velocity: 96,
                frame_offset: 17,
            },
        ];
        events.sort_unstable_by_key(|event| (event.frame_offset, event.is_on));
        let mut list = super::LiveEventList::new(&events);

        assert_eq!(list.event_count(), 2);

        let raw = list.as_raw_mut();
        let vtbl = unsafe { (*raw.cast::<super::LiveEventList<'_>>()).vtbl };
        assert_eq!(unsafe { ((*vtbl).get_event_count)(raw) }, 2);
        let mut via_vtable = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { ((*vtbl).get_event)(raw, 0, &mut via_vtable) }, 0);
        assert_eq!(via_vtable.sampleOffset, 17);

        let note_on = list.event(0).expect("note-on event");
        assert_eq!(note_on.r#type, kNoteOnEvent as u16);
        assert_eq!(note_on.sampleOffset, 17);
        let note_on = unsafe { note_on.__field0.noteOn };
        assert_eq!(note_on.pitch, 64);
        assert!((note_on.velocity - 96.0 / 127.0).abs() < f32::EPSILON);

        let note_off = list.event(1).expect("note-off event");
        assert_eq!(note_off.r#type, kNoteOffEvent as u16);
        assert_eq!(note_off.sampleOffset, 91);
        let note_off = unsafe { note_off.__field0.noteOff };
        assert_eq!(note_off.pitch, 64);
    }
}
