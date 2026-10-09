use super::instance::vtbl;
use crate::audio_ports::AudioPort;
use vibez_core::routing::ExternalInputId;
use vst3::Steinberg::Vst::BusInfo;

pub(super) unsafe fn query(
    component: *mut std::ffi::c_void,
    direction: i32,
    count: i32,
    max_frames: usize,
) -> Result<Vec<(String, AudioPort)>, String> {
    type GetBusInfo =
        unsafe extern "system" fn(*mut std::ffi::c_void, i32, i32, i32, *mut BusInfo) -> i32;
    type ActivateBus = unsafe extern "system" fn(*mut std::ffi::c_void, i32, i32, i32, u8) -> i32;
    let get_info: GetBusInfo = std::mem::transmute(*vtbl(component).add(8));
    let activate: ActivateBus = std::mem::transmute(*vtbl(component).add(10));
    let mut ports = Vec::new();
    for index in 0..count {
        let mut info: BusInfo = std::mem::zeroed();
        if get_info(component, 0, direction, index, &mut info) != 0 || info.channelCount < 0 {
            return Err(format!("Cannot query VST3 audio bus {index}"));
        }
        let name = String::from_utf16_lossy(
            &info.name[..info
                .name
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(info.name.len())],
        );
        let main = info.busType == 0;
        let supported = matches!(info.channelCount, 1 | 2);
        if activate(component, 0, direction, index, u8::from(main || supported)) != 0 {
            return Err(format!("Cannot activate VST3 audio bus {name}"));
        }
        ports.push((
            name,
            AudioPort::new(
                ExternalInputId(index as u32),
                info.channelCount as usize,
                main,
                max_frames,
            ),
        ));
    }
    Ok(ports)
}
