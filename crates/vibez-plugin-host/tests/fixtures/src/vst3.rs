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
    processing_thread: Cell<usize>,
    aux_active: Cell<u32>,
    max_frames: Cell<i32>,
    timing: RefCell<crate::timing::Timing>,
    handler: RefCell<Option<ComPtr<IComponentHandler>>>,
    main_thread: usize,
    instrument: bool,
    playing: Cell<bool>,
}
const INSTRUMENT_CID: TUID = uid(0xFEDCBA98, 0x76543210, 0xFEDCBA98, 0x76543210);
const CID: TUID = uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x89ABCDEF);
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
        (*bus).channelCount = match index {
            1 => self.timing.borrow().mono_channels() as i32,
            3 => 6,
            _ => 2,
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
            let mask = 1u32 << index;
            self.aux_active.set(if enabled != 0 {
                self.aux_active.get() | mask
            } else {
                self.aux_active.get() & !mask
            });
        }
        kResultOk
    }
    unsafe fn setActive(&self, enabled: TBool) -> tresult {
        if enabled != 0 {
            if self.processing.get()
                || self.main_thread != crate::thread_id()
                || !self.timing.borrow().activation_allowed()
            {
                return kResultFalse;
            }
            self.timing.borrow_mut().activate();
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
        if self.active.get() {
            if let Some(handler) = self.handler.borrow().as_ref() {
                handler.restartComponent(RestartFlags_::kLatencyChanged);
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
        stream.write(bytes.as_ptr().cast_mut().cast(), 12, &mut written)
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
        kResultOk
    }
    unsafe fn setProcessing(&self, enabled: TBool) -> tresult {
        if enabled == 0
            && self.processing.get()
            && self.processing_thread.get() != crate::thread_id()
        {
            crate::lifecycle_error();
            return kResultFalse;
        }
        if enabled != 0 {
            self.processing_thread.set(crate::thread_id());
        }
        if enabled == 0 {
            self.timing.borrow_mut().reset();
            self.playing.set(false);
        }
        self.processing.set(enabled != 0 && self.active.get());
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = &*data;
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
            || self.aux_active.get() & 7 != 7
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
        {
            return kResultFalse;
        }
        let mut timing = self.timing.borrow_mut();
        for frame in 0..data.numSamples as usize {
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
        0
    }
    unsafe fn getParameterInfo(&self, _: i32, _: *mut ParameterInfo) -> tresult {
        kInvalidArgument
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
    unsafe fn getParamNormalized(&self, _: u32) -> f64 {
        0.0
    }
    unsafe fn setParamNormalized(&self, _: u32, _: f64) -> tresult {
        kInvalidArgument
    }
    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        *self.handler.borrow_mut() =
            ComRef::from_raw(handler).map(|reference| reference.to_com_ptr());
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
        2
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if !(0..=1).contains(&index) {
            return kInvalidArgument;
        }
        *info = std::mem::zeroed();
        (*info).cid = if index == 0 { CID } else { INSTRUMENT_CID };
        (*info).cardinality = i32::MAX;
        copy("Audio Module Class", &mut (*info).category);
        copy(
            if index == 0 {
                "Routing Probe"
            } else {
                "Pulse Instrument"
            },
            &mut (*info).name,
        );
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        object: *mut *mut c_void,
    ) -> tresult {
        let instrument = *(cid as *const TUID) == INSTRUMENT_CID;
        if !instrument && *(cid as *const TUID) != CID {
            *object = ptr::null_mut();
            return kInvalidArgument;
        }
        let instance = ComWrapper::new(Probe {
            active: Cell::new(false),
            processing: Cell::new(false),
            processing_thread: Cell::new(0),
            aux_active: Cell::new(0),
            max_frames: Cell::new(0),
            timing: RefCell::new(Default::default()),
            handler: RefCell::new(None),
            main_thread: crate::thread_id(),
            instrument,
            playing: Cell::new(false),
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
