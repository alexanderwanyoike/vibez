use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::ext::audio_ports::{
    clap_audio_port_info, clap_plugin_audio_ports, CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS,
};
use clap_sys::plugin::clap_plugin;
use vibez_core::routing::ExternalInputId;

use crate::audio_ports::AudioPort;

pub(super) unsafe fn query(
    plugin: *const clap_plugin,
    input: bool,
    frames: usize,
) -> Result<Vec<(String, AudioPort)>, String> {
    let extension = ((*plugin).get_extension.unwrap())(plugin, CLAP_EXT_AUDIO_PORTS.as_ptr())
        as *const clap_plugin_audio_ports;
    if extension.is_null() {
        return Ok(Vec::new());
    }
    let mut ports = Vec::new();
    for index in 0..((*extension).count.unwrap())(plugin, input) {
        let mut info: clap_audio_port_info = std::mem::zeroed();
        if !((*extension).get.unwrap())(plugin, index, input, &mut info) {
            return Err(format!("Cannot query CLAP audio port {index}"));
        }
        {
            let name = std::ffi::CStr::from_ptr(info.name.as_ptr())
                .to_string_lossy()
                .into_owned();
            if ports
                .iter()
                .any(|(_, port): &(String, AudioPort)| port.id == ExternalInputId(info.id))
            {
                return Err("CLAP declared duplicate audio port identities".into());
            }
            ports.push((
                name,
                AudioPort::new(
                    ExternalInputId(info.id),
                    info.channel_count as usize,
                    info.flags & CLAP_AUDIO_PORT_IS_MAIN != 0,
                    frames,
                ),
            ));
        }
    }
    Ok(ports)
}

pub(super) fn buffers(ports: &mut [(String, AudioPort)]) -> Vec<clap_audio_buffer> {
    ports
        .iter_mut()
        .map(|(_, port)| clap_audio_buffer {
            data32: port.pointers.as_mut_ptr(),
            data64: std::ptr::null_mut(),
            channel_count: port.channels as u32,
            latency: 0,
            constant_mask: 0,
        })
        .collect()
}
