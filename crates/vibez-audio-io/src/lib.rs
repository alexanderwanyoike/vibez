macro_rules! dispatch_sample_format {
    ($format:expr, $build:ident, $unsupported:expr) => {{
        let format = $format;
        match format {
            cpal::SampleFormat::F32 => $build!(f32),
            cpal::SampleFormat::F64 => $build!(f64),
            cpal::SampleFormat::I8 => $build!(i8),
            cpal::SampleFormat::I16 => $build!(i16),
            cpal::SampleFormat::I24 => $build!(cpal::I24),
            cpal::SampleFormat::I32 => $build!(i32),
            cpal::SampleFormat::I64 => $build!(i64),
            cpal::SampleFormat::U8 => $build!(u8),
            cpal::SampleFormat::U16 => $build!(u16),
            cpal::SampleFormat::U32 => $build!(u32),
            cpal::SampleFormat::U64 => $build!(u64),
            unsupported => return Err($unsupported(unsupported)),
        }
    }};
}

pub mod audio_host;
pub mod audio_input;
pub mod audio_stream;
pub mod file_io;
pub mod midi_input;
mod stream_config;
pub use stream_config::StreamOpenError;
