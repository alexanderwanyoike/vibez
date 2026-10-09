//! Loadable VST3 input, activation refusal and processing acceptance probe.

#![allow(non_snake_case)]
use std::{
    cell::Cell,
    ffi::{c_char, c_void},
    ptr, slice,
};
use vst3::{uid, Class, ComRef, ComWrapper, Steinberg::Vst::*, Steinberg::*};

struct Probe {
    active: Cell<bool>,
    processing: Cell<bool>,
    aux_active: Cell<u32>,
    max_frames: Cell<i32>,
    instrument: bool,
    playing: Cell<bool>,
    scenario: usize,
    main_activations: Cell<u32>,
}
const INSTRUMENT_CID: TUID = uid(0xFEDCBA98, 0x76543210, 0xFEDCBA98, 0x76543210);
const CID: TUID = uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x89ABCDEF);
const CIDS: [TUID; 6] = [
    CID,
    INSTRUMENT_CID,
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000001),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000002),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000003),
    uid(0x01234567, 0x89ABCDEF, 0x01234567, 0x00000004),
];
const NAMES: [&str; 6] = [
    "Routing Probe",
    "Pulse Instrument",
    "Refused aux",
    "Refused surround deactivation",
    "Refused main",
    "Processing error",
];
impl Class for Probe {
    type Interfaces = (IComponent, IAudioProcessor);
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
        if media != 0 || index < 0 || index >= self.getBusCount(media, direction) {
            return kInvalidArgument;
        }
        *bus = std::mem::zeroed();
        (*bus).mediaType = media;
        (*bus).direction = direction;
        (*bus).channelCount = match index {
            1 => 1,
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
        kResultOk
    }
    unsafe fn setActive(&self, enabled: TBool) -> tresult {
        self.active.set(enabled != 0);
        kResultOk
    }
    unsafe fn setState(&self, _stream: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _stream: *mut IBStream) -> tresult {
        kResultOk
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
        *arrangement = if index == 1 {
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
        0
    }
    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        self.max_frames.set((*setup).maxSamplesPerBlock);
        kResultOk
    }
    unsafe fn setProcessing(&self, enabled: TBool) -> tresult {
        self.processing.set(enabled != 0 && self.active.get());
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = &*data;
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
                for channel in 0..2 {
                    *(*output.__field0.channelBuffers32.add(channel)).add(frame as usize) =
                        if self.playing.get() { 0.75 } else { 0.0 };
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
        if inputs[1].numChannels != 1 || inputs[2].numChannels != 2 || inputs[3].numChannels != 6 {
            return kResultFalse;
        }
        for frame in 0..data.numSamples as usize {
            for channel in 0..2 {
                if self.scenario == 3 {
                    for auxiliary in 0..6 {
                        if *(*inputs[3].__field0.channelBuffers32.add(auxiliary)).add(frame) != 0.0
                        {
                            return kResultFalse;
                        }
                    }
                }
                let main = *(*inputs[0].__field0.channelBuffers32.add(channel)).add(frame);
                let mono = *(*inputs[1].__field0.channelBuffers32).add(frame);
                let stereo = *(*inputs[2].__field0.channelBuffers32.add(channel)).add(frame);
                *(*output.__field0.channelBuffers32.add(channel)).add(frame) =
                    main + 2.0 * mono + 3.0 * stereo;
            }
        }
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
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
        6
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if !(0..6).contains(&index) {
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
            aux_active: Cell::new(if scenario == 3 { 15 } else { 0 }),
            max_frames: Cell::new(0),
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
