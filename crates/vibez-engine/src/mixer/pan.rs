/// Equal-power pan law.
/// `pan` ranges from 0.0 (hard left) to 1.0 (hard right).
/// Returns `(left_gain, right_gain)`.
/// At center (0.5): both channels get ~0.707 (-3dB).
pub fn equal_power_pan(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(0.0, 1.0);
    let angle = pan * std::f32::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// Stereo balance law for channels that carry already-panned
/// material (buses): center passes both channels at unity, off-
/// center attenuates the far side. Equal-power panning here would
/// tax every centered return 3 dB.
pub fn balance_pan(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(0.0, 1.0);
    (((1.0 - pan) * 2.0).min(1.0), (pan * 2.0).min(1.0))
}
