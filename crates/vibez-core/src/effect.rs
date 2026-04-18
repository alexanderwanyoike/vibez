use serde::{Deserialize, Serialize};

use crate::id::EffectId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectType {
    Gain,
    Filter,
    Delay,
    Reverb,
    Drive,
    Bitcrush,
    Compressor,
    AutoPan,
    Gate,
    Phaser,
    Eq,
}

impl EffectType {
    pub fn name(self) -> &'static str {
        match self {
            EffectType::Gain => "Gain",
            EffectType::Filter => "Filter",
            EffectType::Delay => "Delay",
            EffectType::Reverb => "Reverb",
            EffectType::Drive => "Drive",
            EffectType::Bitcrush => "Bitcrush",
            EffectType::Compressor => "Compressor",
            EffectType::AutoPan => "Auto-Pan",
            EffectType::Gate => "Gate",
            EffectType::Phaser => "Phaser",
            EffectType::Eq => "EQ",
        }
    }

    pub fn all() -> &'static [EffectType] {
        &[
            EffectType::Gain,
            EffectType::Filter,
            EffectType::Eq,
            EffectType::Compressor,
            EffectType::Gate,
            EffectType::Drive,
            EffectType::Bitcrush,
            EffectType::Delay,
            EffectType::Reverb,
            EffectType::Phaser,
            EffectType::AutoPan,
        ]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamDescriptor {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectInfo {
    pub id: EffectId,
    pub effect_type: EffectType,
    pub bypass: bool,
    pub params: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_type_name() {
        assert_eq!(EffectType::Gain.name(), "Gain");
        assert_eq!(EffectType::Filter.name(), "Filter");
        assert_eq!(EffectType::Delay.name(), "Delay");
        assert_eq!(EffectType::Reverb.name(), "Reverb");
    }

    #[test]
    fn effect_type_all() {
        let all = EffectType::all();
        assert_eq!(all.len(), 11);
    }

    #[test]
    fn effect_info_serde_roundtrip() {
        let info = EffectInfo {
            id: EffectId::new(),
            effect_type: EffectType::Delay,
            bypass: false,
            params: vec![500.0, 0.5, 0.3],
        };
        let json = serde_json::to_string(&info).unwrap();
        let loaded: EffectInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.effect_type, EffectType::Delay);
        assert_eq!(loaded.params.len(), 3);
        assert!(!loaded.bypass);
    }
}
