//! How much of one core a Compressor takes on 20 s of pulsing stereo audio.
//! Run with `cargo run --release -p vibez-dsp --example compressor_cost`.

use std::time::Instant;

use vibez_core::effect::EffectType;

fn main() {
    let rate = 48_000.0;
    let seconds = 20.0;
    for (name, edits) in [
        ("defaults", &[][..]),
        (
            "RMS, unlinked, auto release",
            &[(11, 1.0), (12, 1.0), (13, 0.0)][..],
        ),
    ] {
        let mut compressor = vibez_dsp::factory::create_effect(EffectType::Compressor, rate);
        for &(index, value) in edits {
            compressor.set_automated_param(index, value);
        }
        let mut buffer = vec![0.0f32; 512 * 2];
        let mut phase = 0.0f32;
        let start = Instant::now();
        for _ in 0..(seconds * rate / 512.0) as usize {
            for frame in buffer.chunks_mut(2) {
                phase += 0.01;
                let pulse = if (phase * 0.01).sin() > 0.0 { 1.0 } else { 0.2 };
                frame.fill(phase.sin() * 0.5 * pulse);
            }
            compressor.process(&mut buffer, 2);
        }
        let share = start.elapsed().as_secs_f64() / seconds as f64 * 100.0;
        println!("{name}: {share:.2}% of one core");
    }
}
