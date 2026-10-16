//! Loadable VST3 input, activation refusal and processing acceptance probe.

#![allow(non_snake_case)]
use std::{
    cell::{Cell, RefCell},
    ffi::{c_char, c_void},
    ptr, slice,
};
use vst3::{uid, Class, ComPtr, ComRef, ComWrapper, Steinberg::Vst::*, Steinberg::*};

struct Probe {
    active: Cell<bool>,
    processing: Cell<bool>,
    in_call: std::sync::atomic::AtomicBool,
    aux_active: Cell<u32>,
    max_frames: Cell<i32>,
    timing: RefCell<crate::timing::Timing>,
    handler: RefCell<Option<ComPtr<IComponentHandler>>>,
    main_thread: usize,
    instrument: bool,
    playing: Cell<bool>,
    scenario: usize,
    main_activations: Cell<u32>,
}
const INSTRUMENT_CID: TUID = uid(0xFEDCBA98, 0x76543210, 0xFEDCBA98, 0x76543210);
const CID: TUID = uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x89ABCDEF);
/// The "Editor edits" class: one parameter whose editor changes reach the
/// processor only through the host, like an NIH-plug plugin.
const EDITOR_SCENARIO: usize = 8;
const EDITOR_PARAM_ID: u32 = 7;
const CIDS: [TUID; 9] = [
    CID,
    INSTRUMENT_CID,
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000001),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000002),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000003),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000004),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000005),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000006),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000007),
];
const NAMES: [&str; 9] = [
    "Routing Probe",
    "Pulse Instrument",
    "Refused aux",
    "Refused surround deactivation",
    "Refused main",
    "Processing error",
    "Surround main",
    "Refused handler",
    "Editor edits",
];

// One editor-edits instance per loaded copy of the fixture library, which
// each test loads separately.
static EDITOR_HANDLER: std::sync::atomic::AtomicPtr<IComponentHandler> =
    std::sync::atomic::AtomicPtr::new(ptr::null_mut());
static CONTROLLER_VALUE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0x3FD9_9999_9999_999A);
static PROCESSOR_VALUE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0x3FD9_9999_9999_999A);

fn load(value: &std::sync::atomic::AtomicU64) -> f64 {
    f64::from_bits(value.load(std::sync::atomic::Ordering::Acquire))
}
fn store(value: &std::sync::atomic::AtomicU64, to: f64) {
    value.store(to.to_bits(), std::sync::atomic::Ordering::Release);
}

/// Acts as the plugin's editor: a knob gesture reported to the host.
#[no_mangle]
pub extern "C" fn fixture_editor_edit(value: f64) -> i32 {
    let handler = EDITOR_HANDLER.load(std::sync::atomic::Ordering::Acquire);
    let Some(handler) = (unsafe { ComRef::from_raw(handler) }) else {
        return kResultFalse;
    };
    store(&CONTROLLER_VALUE, value);
    unsafe {
        handler.beginEdit(EDITOR_PARAM_ID);
        let result = handler.performEdit(EDITOR_PARAM_ID, value);
        handler.endEdit(EDITOR_PARAM_ID);
        result
    }
}

/// Acts as the plugin's preset browser: the controller takes new values and
/// tells the host they all changed.
#[no_mangle]
pub extern "C" fn fixture_editor_preset(value: f64) -> i32 {
    let handler = EDITOR_HANDLER.load(std::sync::atomic::Ordering::Acquire);
    let Some(handler) = (unsafe { ComRef::from_raw(handler) }) else {
        return kResultFalse;
    };
    store(&CONTROLLER_VALUE, value);
    unsafe { handler.restartComponent(RestartFlags_::kParamValuesChanged) }
}

/// The value the processor last received through its parameter changes.
#[no_mangle]
pub extern "C" fn fixture_processor_value() -> f64 {
    load(&PROCESSOR_VALUE)
}
impl Class for Probe {
    type Interfaces = (IComponent, IAudioProcessor, IEditController);
}
impl IPluginBaseTrait for Probe {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}
impl IComponentTrait for Probe {
    unsafe fn getControllerClassId(&self, _id: *mut TUID) -> tresult {
        kNotImplemented
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, direction: BusDirection) -> i32 {
        if media != 0 {
            if self.instrument && direction == 0 {
                1
            } else {
                0
            }
        } else if direction == 0 {
            if self.instrument {
                0
            } else {
                4
            }
        } else {
            1
        }
    }
    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        direction: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if self.active.get()
            || media != 0
            || index < 0
            || index >= self.getBusCount(media, direction)
        {
            return kInvalidArgument;
        }
        *bus = std::mem::zeroed();
        (*bus).mediaType = media;
        (*bus).direction = direction;
        (*bus).channelCount = if self.scenario == 6 && index == 0 {
            6
        } else {
            match index {
                1 => self.timing.borrow().mono_channels() as i32,
                3 => 6,
                _ => 2,
            }
        };
        (*bus).busType = i32::from(index != 0);
        (*bus).flags = 1;
        let name = match index {
            0 => "Main",
            1 => "Mono detector",
            2 => "Stereo detector",
            _ => "Surround detector",
        };
        for (target, character) in (*bus).name.iter_mut().zip(name.encode_utf16()) {
            *target = character;
        }
        kResultOk
    }
    unsafe fn getRoutingInfo(
        &self,
        _input: *mut RoutingInfo,
        _output: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(
        &self,
        media: MediaType,
        direction: BusDirection,
        index: i32,
        enabled: TBool,
    ) -> tresult {
        if media == 0 && direction == 0 {
            if index == 0 && enabled != 0 {
                self.main_activations.set(self.main_activations.get() + 1);
                if self.main_activations.get() > 1 || self.scenario == 4 {
                    return kResultFalse;
                }
            }
            if (self.scenario == 2 && index == 1 && enabled != 0)
                || (self.scenario == 3 && index == 3 && enabled == 0)
            {
                return kNotImplemented;
            }
            let mask = 1u32 << index;
            self.aux_active.set(if enabled != 0 {
                self.aux_active.get() | mask
            } else {
                self.aux_active.get() & !mask
            });
        }
        if media == 1 && enabled != 0 {
            crate::EVENT_ACTIVATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        kResultOk
    }
    unsafe fn setActive(&self, enabled: TBool) -> tresult {
        // Bus preparation ends at an activation attempt, even if activation
        // fails. A later retry must start a fresh preparation epoch.
        self.main_activations.set(0);
        if enabled != 0 {
            if self.processing.get()
                || self.main_thread != crate::thread_id()
                || !self.timing.borrow().activation_allowed()
            {
                return kResultFalse;
            }
            crate::ACTIVATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if self.timing.borrow().flag(19) {
                if let Some(handler) = self.handler.borrow().as_ref() {
                    handler.restartComponent(
                        RestartFlags_::kLatencyChanged | RestartFlags_::kIoChanged,
                    );
                }
            }
            self.timing.borrow_mut().activate();
        }
        if enabled == 0
            && (self.processing.get()
                || self.in_call.load(std::sync::atomic::Ordering::Acquire)
                || self.main_thread != crate::thread_id())
        {
            crate::lifecycle_error();
        }
        self.active.set(enabled != 0);
        kResultOk
    }
    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        let Some(stream) = ComRef::from_raw(stream) else {
            return kInvalidArgument;
        };
        let mut bytes = [0u8; 12];
        let mut read = 0;
        if stream.read(bytes.as_mut_ptr().cast(), 12, &mut read) != kResultOk
            || read != 12
            || !self.timing.borrow_mut().configure(&bytes)
        {
            return kResultFalse;
        }
        if self.scenario == EDITOR_SCENARIO {
            let mut value = [0u8; 8];
            if stream.read(value.as_mut_ptr().cast(), 8, &mut read) == kResultOk && read == 8 {
                store(&PROCESSOR_VALUE, f64::from_le_bytes(value));
                store(&CONTROLLER_VALUE, f64::from_le_bytes(value));
            }
        }
        if self.active.get() {
            if let Some(handler) = self.handler.borrow().as_ref() {
                handler.restartComponent(if self.timing.borrow().flag(21) {
                    RestartFlags_::kIoChanged
                } else {
                    RestartFlags_::kLatencyChanged
                });
            }
        }
        kResultOk
    }
    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        let Some(stream) = ComRef::from_raw(stream) else {
            return kInvalidArgument;
        };
        let bytes = self.timing.borrow().state();
        let mut written = 0;
        let result = stream.write(bytes.as_ptr().cast_mut().cast(), 12, &mut written);
        if self.scenario != EDITOR_SCENARIO || result != kResultOk {
            return result;
        }
        let value = load(&PROCESSOR_VALUE).to_le_bytes();
        stream.write(value.as_ptr().cast_mut().cast(), 8, &mut written)
    }
}
impl IAudioProcessorTrait for Probe {
    unsafe fn setBusArrangements(
        &self,
        _inputs: *mut SpeakerArrangement,
        _num_inputs: i32,
        _outputs: *mut SpeakerArrangement,
        _num_outputs: i32,
    ) -> tresult {
        kResultOk
    }
    unsafe fn getBusArrangement(
        &self,
        _direction: BusDirection,
        index: i32,
        arrangement: *mut SpeakerArrangement,
    ) -> tresult {
        *arrangement = if index == 1 && self.timing.borrow().mono_channels() == 1 {
            SpeakerArr::kMono
        } else if index == 3 || (index == 0 && self.scenario == 6) {
            SpeakerArr::k51
        } else {
            SpeakerArr::kStereo
        };
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == 0 {
            kResultOk
        } else {
            kNotImplemented
        }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        if self.main_thread != crate::thread_id() || self.processing.get() || !self.active.get() {
            return u32::MAX;
        }
        self.timing.borrow().report()
    }
    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        self.max_frames.set((*setup).maxSamplesPerBlock);
        if self.timing.borrow().flag(19) {
            if let Some(handler) = self.handler.borrow().as_ref() {
                handler.restartComponent(RestartFlags_::kLatencyChanged);
            }
        }
        kResultOk
    }
    unsafe fn setProcessing(&self, enabled: TBool) -> tresult {
        if self.in_call.load(std::sync::atomic::Ordering::Acquire) {
            crate::lifecycle_error();
            return kResultFalse;
        }
        if enabled != 0 && self.timing.borrow().flag(18) {
            return kResultFalse;
        }
        if enabled == 0 {
            self.timing.borrow_mut().reset();
            self.playing.set(false);
        }
        self.processing.set(enabled != 0 && self.active.get());
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        if self.timing.borrow().flag(24) {
            self.timing.borrow_mut().clear_flag(24);
            return kResultFalse;
        }
        let _call = crate::ProcessingCall::enter(&self.in_call);
        let data = &*data;
        if self.scenario == EDITOR_SCENARIO {
            if let Some(changes) = ComRef::<IParameterChanges>::from_raw(data.inputParameterChanges)
            {
                for index in 0..changes.getParameterCount() {
                    let Some(queue) = ComRef::from_raw(changes.getParameterData(index)) else {
                        continue;
                    };
                    let points = queue.getPointCount();
                    let (mut offset, mut value) = (0, 0.0);
                    if queue.getParameterId() == EDITOR_PARAM_ID
                        && points > 0
                        && queue.getPoint(points - 1, &mut offset, &mut value) == kResultOk
                    {
                        store(&PROCESSOR_VALUE, value);
                    }
                }
            }
        }
        if !data.processContext.is_null() {
            self.timing
                .borrow_mut()
                .set_context((*data.processContext).projectTimeSamples as u64);
        }
        if self.instrument {
            if !self.processing.get()
                || data.numInputs != 0
                || data.numOutputs != 1
                || data.numSamples > self.max_frames.get()
            {
                return kResultFalse;
            }
            let events = ComRef::from_raw(data.inputEvents);
            let mut event_index = 0;
            let output = &*data.outputs;
            for frame in 0..data.numSamples {
                if let Some(events) = &events {
                    while event_index < events.getEventCount() {
                        let mut event: Event = std::mem::zeroed();
                        events.getEvent(event_index, &mut event);
                        if event.sampleOffset > frame {
                            break;
                        }
                        if event.r#type == 0 {
                            self.playing.set(true);
                        } else if event.r#type == 1 {
                            self.playing.set(false);
                        }
                        event_index += 1;
                    }
                }
                let level = if self.playing.get() { 0.75 } else { 0.0 };
                let values = self.timing.borrow_mut().frame([level; 2], 0.0, [0.0; 2]);
                for (channel, value) in values.into_iter().enumerate() {
                    *(*output.__field0.channelBuffers32.add(channel)).add(frame as usize) = value;
                }
            }
            return kResultOk;
        }
        if !self.processing.get()
            || self.aux_active.get() & (if self.scenario == 2 { 5 } else { 7 })
                != (if self.scenario == 2 { 5 } else { 7 })
            || self.scenario == 5
            || data.numInputs != 4
            || data.numOutputs != 1
            || data.numSamples > self.max_frames.get()
        {
            return kResultFalse;
        }
        let inputs = slice::from_raw_parts(data.inputs, 4);
        let output = &*data.outputs;
        if inputs[1].numChannels != self.timing.borrow().mono_channels() as i32
            || inputs[2].numChannels != 2
            || inputs[3].numChannels != 6
        {
            return kResultFalse;
        }
        if self.scenario == 6 && (inputs[0].numChannels != 6 || output.numChannels != 6) {
            return kResultFalse;
        }
        let mut timing = self.timing.borrow_mut();
        for frame in 0..data.numSamples as usize {
            if self.scenario == 6 {
                for channel in 2..6 {
                    if *(*inputs[0].__field0.channelBuffers32.add(channel)).add(frame) != 0.0 {
                        return kResultFalse;
                    }
                    *(*output.__field0.channelBuffers32.add(channel)).add(frame) = 99.0;
                }
            }
            if matches!(self.scenario, 3 | 6) {
                for auxiliary in 0..6 {
                    if *(*inputs[3].__field0.channelBuffers32.add(auxiliary)).add(frame) != 0.0 {
                        return kResultFalse;
                    }
                }
            }
            let main = std::array::from_fn(|channel| {
                *(*inputs[0].__field0.channelBuffers32.add(channel)).add(frame)
            });
            let mono = *(*inputs[1].__field0.channelBuffers32).add(frame);
            let stereo = std::array::from_fn(|channel| {
                *(*inputs[2].__field0.channelBuffers32.add(channel)).add(frame)
            });
            for (channel, value) in timing.frame(main, mono, stereo).into_iter().enumerate() {
                *(*output.__field0.channelBuffers32.add(channel)).add(frame) = value;
            }
        }
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}
impl IEditControllerTrait for Probe {
    unsafe fn setComponentState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getParameterCount(&self) -> i32 {
        i32::from(self.scenario == EDITOR_SCENARIO)
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        if self.scenario != EDITOR_SCENARIO || index != 0 {
            return kInvalidArgument;
        }
        *info = std::mem::zeroed();
        (*info).id = EDITOR_PARAM_ID;
        for (slot, unit) in (*info).title.iter_mut().zip("Cutoff".encode_utf16()) {
            *slot = unit as TChar;
        }
        (*info).defaultNormalizedValue = 0.4;
        (*info).flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, _: u32, _: f64, _: *mut String128) -> tresult {
        kInvalidArgument
    }
    unsafe fn getParamValueByString(&self, _: u32, _: *mut TChar, _: *mut f64) -> tresult {
        kInvalidArgument
    }
    unsafe fn normalizedParamToPlain(&self, _: u32, value: f64) -> f64 {
        value
    }
    unsafe fn plainParamToNormalized(&self, _: u32, value: f64) -> f64 {
        value
    }
    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        if self.scenario == EDITOR_SCENARIO && id == EDITOR_PARAM_ID {
            load(&CONTROLLER_VALUE)
        } else {
            0.0
        }
    }
    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        if self.scenario != EDITOR_SCENARIO || id != EDITOR_PARAM_ID {
            return kInvalidArgument;
        }
        store(&CONTROLLER_VALUE, value);
        kResultOk
    }
    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        if self.scenario == 7 {
            return kNotImplemented;
        }
        *self.handler.borrow_mut() =
            ComRef::from_raw(handler).map(|reference| reference.to_com_ptr());
        if self.scenario == EDITOR_SCENARIO {
            EDITOR_HANDLER.store(handler, std::sync::atomic::Ordering::Release);
        }
        kResultOk
    }
    unsafe fn createView(&self, _: *const c_char) -> *mut IPlugView {
        ptr::null_mut()
    }
}

struct Factory;
impl Class for Factory {
    type Interfaces = (IPluginFactory,);
}
fn copy(text: &str, destination: &mut [c_char]) {
    for (target, byte) in destination.iter_mut().zip(text.bytes()) {
        *target = byte as c_char;
    }
}
impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        *info = std::mem::zeroed();
        copy("Vibez tests", &mut (*info).vendor);
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        CIDS.len() as i32
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if !(0..CIDS.len() as i32).contains(&index) {
            return kInvalidArgument;
        }
        *info = std::mem::zeroed();
        (*info).cid = CIDS[index as usize];
        (*info).cardinality = i32::MAX;
        copy("Audio Module Class", &mut (*info).category);
        copy(NAMES[index as usize], &mut (*info).name);
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        object: *mut *mut c_void,
    ) -> tresult {
        let Some(scenario) = CIDS
            .iter()
            .position(|class| *class == *(cid as *const TUID))
        else {
            *object = ptr::null_mut();
            return kInvalidArgument;
        };
        let instrument = scenario == 1;
        let instance = ComWrapper::new(Probe {
            active: Cell::new(false),
            processing: Cell::new(false),
            in_call: std::sync::atomic::AtomicBool::new(false),
            aux_active: Cell::new(if scenario == 3 { 15 } else { 0 }),
            max_frames: Cell::new(0),
            timing: RefCell::new(Default::default()),
            handler: RefCell::new(None),
            main_thread: crate::thread_id(),
            instrument,
            playing: Cell::new(false),
            scenario,
            main_activations: Cell::new(0),
        })
        .to_com_ptr::<FUnknown>()
        .unwrap();
        let raw = instance.as_ptr();
        ((*(*raw).vtbl).queryInterface)(raw, iid as *mut TUID, object)
    }
}
#[no_mangle]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .unwrap()
        .into_raw()
}
#[no_mangle]
extern "system" fn ModuleEntry(_module: *mut c_void) -> bool {
    true
}
#[no_mangle]
extern "system" fn ModuleExit() -> bool {
    true
}
#[no_mangle]
extern "system" fn InitDll() -> bool {
    true
}
#[no_mangle]
extern "system" fn ExitDll() -> bool {
    true
}
#[no_mangle]
extern "system" fn bundleEntry(_bundle: *mut c_void) -> bool {
    true
}
#[no_mangle]
extern "system" fn bundleExit() -> bool {
    true
}

impl Drop for Probe {
    fn drop(&mut self) {
        if self.processing.get() || self.main_thread != crate::thread_id() {
            crate::lifecycle_error();
        }
    }
}
