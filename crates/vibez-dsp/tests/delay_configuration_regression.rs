//! Rejects safely constructible finite activation rates outside Delay's processing contract.

use serde_json::json;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

#[test]
fn finite_invalid_activation_rates_do_not_claim_valid_processing_configuration() {
    let rates = [0.0_f32, -1000.0, 0.5, 999.0, 1000.5, 48_000.5];
    let mut observations = Vec::new();
    let mut accepted = Vec::new();
    for requested_rate in rates {
        // The original constructor is unguarded, so this regression excludes dangerous allocations.
        assert!(requested_rate.is_finite() && requested_rate <= 48_000.5);
        assert!(requested_rate < 1000.0 || requested_rate.fract() != 0.0);
        let effect = DelayEffect::new(requested_rate);
        let configuration_valid = effect.processing_configuration_valid();
        if configuration_valid {
            accepted.push(requested_rate);
        }
        observations.push(json!({
            "requested_rate": requested_rate,
            "requested_rate_bits": requested_rate.to_bits(),
            "reported_activation_rate": effect.activation_sample_rate(),
            "processing_configuration_valid": configuration_valid,
        }));
    }
    println!(
        "DELAY_CONFIGURATION_RECORD {}",
        json!({
            "case": "safe_finite_invalid_activation_rates",
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "safe_fixture_ceiling_hz": 48_000.5,
            "processing_called": false,
            "observed": observations,
        })
    );
    assert!(
        accepted.is_empty(),
        "invalid rates reported valid: {accepted:?}"
    );
}
