//! Nondestructive Audio Clip fade lengths, curves and crossfade links.

use serde::{Deserialize, Serialize};

use crate::id::ClipId;

/// Persisted fade-curve shape in producer-facing percentage points.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct FadeCurve(i8);

impl<'de> Deserialize<'de> for FadeCurve {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::new(i16::deserialize(deserializer)?))
    }
}

impl FadeCurve {
    pub const MIN: i8 = -100;
    pub const MAX: i8 = 100;

    pub const fn new(percent: i16) -> Self {
        let percent = if percent < Self::MIN as i16 {
            Self::MIN
        } else if percent > Self::MAX as i16 {
            Self::MAX
        } else {
            percent as i8
        };
        Self(percent)
    }

    pub const fn percent(self) -> i8 {
        self.0
    }

    pub const fn is_linear(value: &Self) -> bool {
        value.0 == 0
    }

    /// Map normalized time to gain. Positive values rise early, negative
    /// values rise late, and zero is exactly linear.
    #[inline]
    pub fn gain(self, progress: f32) -> f32 {
        let progress = progress.clamp(0.0, 1.0);
        if self.0 == 0 {
            return progress;
        }
        let exponent = 2.0_f32.powf(-2.0 * f32::from(self.0) / 100.0);
        progress.powf(exponent)
    }

    /// One gain law shared by realtime rendering and the drawn envelope.
    #[inline]
    pub fn gain_for(self, progress: f32, equal_power: bool) -> f32 {
        if equal_power {
            self.crossfade_gains(progress).1
        } else {
            self.gain(progress)
        }
    }

    /// Gain while moving left-to-right through a fade-out region.
    #[inline]
    pub fn fade_out_gain(self, progress: f32, equal_power: bool) -> f32 {
        if equal_power {
            self.crossfade_gains(progress).0
        } else {
            self.gain(1.0 - progress)
        }
    }

    /// Complementary gains for a linked crossfade. The curve moves the
    /// crossover while the sine/cosine pair keeps total power steady.
    #[inline]
    pub fn crossfade_gains(self, progress: f32) -> (f32, f32) {
        let phase = self.gain(progress) * std::f32::consts::FRAC_PI_2;
        (phase.cos(), phase.sin())
    }
}

/// Nondestructive fade lengths at the visible edges of an Audio Clip.
///
/// Values are expressed in timeline frames. They are deliberately independent
/// of source and loop positions, so a looped Clip fades only where the Clip
/// begins and ends instead of on every loop pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClipFades {
    fade_in_frames: u64,
    fade_out_frames: u64,
    fade_in_curve: FadeCurve,
    fade_out_curve: FadeCurve,
    crossfade_in_peer: Option<ClipId>,
    crossfade_out_peer: Option<ClipId>,
    crossfade_in_active: bool,
    crossfade_out_active: bool,
    previous_crossfade_in_curve: FadeCurve,
    previous_crossfade_out_curve: FadeCurve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct CrossfadeMemory {
    peer: ClipId,
    curve: FadeCurve,
}

impl Serialize for ClipFades {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct PersistedFades {
            fade_in_frames: u64,
            fade_out_frames: u64,
            #[serde(skip_serializing_if = "FadeCurve::is_linear")]
            fade_in_curve: FadeCurve,
            #[serde(skip_serializing_if = "FadeCurve::is_linear")]
            fade_out_curve: FadeCurve,
            #[serde(skip_serializing_if = "Option::is_none")]
            crossfade_in_from: Option<ClipId>,
            #[serde(skip_serializing_if = "Option::is_none")]
            crossfade_out_to: Option<ClipId>,
            #[serde(skip_serializing_if = "Option::is_none")]
            previous_crossfade_in: Option<CrossfadeMemory>,
            #[serde(skip_serializing_if = "Option::is_none")]
            previous_crossfade_out: Option<CrossfadeMemory>,
        }

        PersistedFades {
            fade_in_frames: self.fade_in_frames,
            fade_out_frames: self.fade_out_frames,
            fade_in_curve: self.fade_in_curve,
            fade_out_curve: self.fade_out_curve,
            crossfade_in_from: self.crossfade_in_from(),
            crossfade_out_to: self.crossfade_out_to(),
            previous_crossfade_in: (!self.crossfade_in_active)
                .then_some(self.crossfade_in_peer)
                .flatten()
                .map(|peer| CrossfadeMemory {
                    peer,
                    curve: self.previous_crossfade_in_curve,
                }),
            previous_crossfade_out: (!self.crossfade_out_active)
                .then_some(self.crossfade_out_peer)
                .flatten()
                .map(|peer| CrossfadeMemory {
                    peer,
                    curve: self.previous_crossfade_out_curve,
                }),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ClipFades {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PersistedFades {
            #[serde(default)]
            fade_in_frames: u64,
            #[serde(default)]
            fade_out_frames: u64,
            #[serde(default)]
            fade_in_curve: FadeCurve,
            #[serde(default)]
            fade_out_curve: FadeCurve,
            #[serde(default)]
            crossfade_in_from: Option<ClipId>,
            #[serde(default)]
            crossfade_out_to: Option<ClipId>,
            #[serde(default)]
            previous_crossfade_in: Option<CrossfadeMemory>,
            #[serde(default)]
            previous_crossfade_out: Option<CrossfadeMemory>,
        }

        let persisted = PersistedFades::deserialize(deserializer)?;
        let has_fade_in = persisted.fade_in_frames > 0;
        let has_fade_out = persisted.fade_out_frames > 0;
        let crossfade_in_active = has_fade_in && persisted.crossfade_in_from.is_some();
        let crossfade_out_active = has_fade_out && persisted.crossfade_out_to.is_some();
        Ok(Self {
            fade_in_frames: persisted.fade_in_frames,
            fade_out_frames: persisted.fade_out_frames,
            fade_in_curve: if has_fade_in {
                persisted.fade_in_curve
            } else {
                FadeCurve::default()
            },
            fade_out_curve: if has_fade_out {
                persisted.fade_out_curve
            } else {
                FadeCurve::default()
            },
            crossfade_in_peer: if crossfade_in_active {
                persisted.crossfade_in_from
            } else if has_fade_in {
                persisted.previous_crossfade_in.map(|memory| memory.peer)
            } else {
                None
            },
            crossfade_out_peer: if crossfade_out_active {
                persisted.crossfade_out_to
            } else if has_fade_out {
                persisted.previous_crossfade_out.map(|memory| memory.peer)
            } else {
                None
            },
            crossfade_in_active,
            crossfade_out_active,
            previous_crossfade_in_curve: persisted
                .previous_crossfade_in
                .map_or(FadeCurve::default(), |memory| memory.curve),
            previous_crossfade_out_curve: persisted
                .previous_crossfade_out
                .map_or(FadeCurve::default(), |memory| memory.curve),
        })
    }
}

impl ClipFades {
    pub const fn new(fade_in_frames: u64, fade_out_frames: u64, duration: u64) -> Self {
        let fade_in_frames = if fade_in_frames > duration {
            duration
        } else {
            fade_in_frames
        };
        let remaining = duration - fade_in_frames;
        let fade_out_frames = if fade_out_frames > remaining {
            remaining
        } else {
            fade_out_frames
        };
        Self {
            fade_in_frames,
            fade_out_frames,
            fade_in_curve: FadeCurve::new(0),
            fade_out_curve: FadeCurve::new(0),
            crossfade_in_peer: None,
            crossfade_out_peer: None,
            crossfade_in_active: false,
            crossfade_out_active: false,
            previous_crossfade_in_curve: FadeCurve::new(0),
            previous_crossfade_out_curve: FadeCurve::new(0),
        }
    }

    pub const fn fade_in_frames(self) -> u64 {
        self.fade_in_frames
    }

    pub const fn fade_out_frames(self) -> u64 {
        self.fade_out_frames
    }

    pub const fn fade_in_curve(self) -> FadeCurve {
        self.fade_in_curve
    }

    pub const fn fade_out_curve(self) -> FadeCurve {
        self.fade_out_curve
    }

    pub const fn with_fade_in(self, frames: u64, duration: u64) -> Self {
        let mut fades = Self::new(frames, self.fade_out_frames, duration);
        fades.fade_in_curve = if fades.fade_in_frames > 0 {
            if self.crossfade_in_active {
                FadeCurve::new(0)
            } else {
                self.fade_in_curve
            }
        } else {
            FadeCurve::new(0)
        };
        fades.fade_out_curve = if fades.fade_out_frames > 0 {
            self.fade_out_curve
        } else {
            FadeCurve::new(0)
        };
        fades.crossfade_in_peer = if fades.fade_in_frames > 0 {
            self.crossfade_in_peer
        } else {
            None
        };
        fades.crossfade_out_peer = if fades.fade_out_frames > 0 {
            self.crossfade_out_peer
        } else {
            None
        };
        fades.crossfade_out_active = fades.fade_out_frames > 0 && self.crossfade_out_active;
        fades.previous_crossfade_in_curve = if self.crossfade_in_active {
            self.fade_in_curve
        } else {
            self.previous_crossfade_in_curve
        };
        fades.previous_crossfade_out_curve = self.previous_crossfade_out_curve;
        fades
    }

    pub const fn with_fade_out(self, frames: u64, duration: u64) -> Self {
        let fade_out_frames = if frames > duration { duration } else { frames };
        let remaining = duration - fade_out_frames;
        let fade_in_frames = if self.fade_in_frames > remaining {
            remaining
        } else {
            self.fade_in_frames
        };
        Self {
            fade_in_frames,
            fade_out_frames,
            fade_in_curve: if fade_in_frames > 0 {
                self.fade_in_curve
            } else {
                FadeCurve::new(0)
            },
            fade_out_curve: if fade_out_frames > 0 {
                if self.crossfade_out_active {
                    FadeCurve::new(0)
                } else {
                    self.fade_out_curve
                }
            } else {
                FadeCurve::new(0)
            },
            crossfade_in_peer: if fade_in_frames > 0 {
                self.crossfade_in_peer
            } else {
                None
            },
            crossfade_out_peer: if fade_out_frames > 0 {
                self.crossfade_out_peer
            } else {
                None
            },
            crossfade_in_active: fade_in_frames > 0 && self.crossfade_in_active,
            crossfade_out_active: false,
            previous_crossfade_in_curve: self.previous_crossfade_in_curve,
            previous_crossfade_out_curve: if self.crossfade_out_active {
                self.fade_out_curve
            } else {
                self.previous_crossfade_out_curve
            },
        }
    }

    pub const fn clamped_to(self, duration: u64) -> Self {
        let mut fades = Self::new(self.fade_in_frames, self.fade_out_frames, duration);
        fades.fade_in_curve = if fades.fade_in_frames > 0 {
            self.fade_in_curve
        } else {
            FadeCurve::new(0)
        };
        fades.fade_out_curve = if fades.fade_out_frames > 0 {
            self.fade_out_curve
        } else {
            FadeCurve::new(0)
        };
        fades.crossfade_in_peer = if fades.fade_in_frames > 0 {
            self.crossfade_in_peer
        } else {
            None
        };
        fades.crossfade_out_peer = if fades.fade_out_frames > 0 {
            self.crossfade_out_peer
        } else {
            None
        };
        fades.crossfade_in_active = fades.fade_in_frames > 0 && self.crossfade_in_active;
        fades.crossfade_out_active = fades.fade_out_frames > 0 && self.crossfade_out_active;
        fades.previous_crossfade_in_curve = self.previous_crossfade_in_curve;
        fades.previous_crossfade_out_curve = self.previous_crossfade_out_curve;
        fades
    }

    pub fn scaled(self, old_duration: u64, new_duration: u64) -> Self {
        if old_duration == 0 {
            return Self::default();
        }
        let ratio = new_duration as f64 / old_duration as f64;
        let mut fades = Self::new(
            (self.fade_in_frames as f64 * ratio).round() as u64,
            (self.fade_out_frames as f64 * ratio).round() as u64,
            new_duration,
        );
        fades.crossfade_in_peer = if fades.fade_in_frames > 0 {
            self.crossfade_in_peer
        } else {
            None
        };
        fades.crossfade_out_peer = if fades.fade_out_frames > 0 {
            self.crossfade_out_peer
        } else {
            None
        };
        fades.crossfade_in_active = fades.fade_in_frames > 0 && self.crossfade_in_active;
        fades.crossfade_out_active = fades.fade_out_frames > 0 && self.crossfade_out_active;
        fades.fade_in_curve = if fades.fade_in_frames > 0 {
            self.fade_in_curve
        } else {
            FadeCurve::new(0)
        };
        fades.fade_out_curve = if fades.fade_out_frames > 0 {
            self.fade_out_curve
        } else {
            FadeCurve::new(0)
        };
        fades.previous_crossfade_in_curve = self.previous_crossfade_in_curve;
        fades.previous_crossfade_out_curve = self.previous_crossfade_out_curve;
        fades
    }

    /// Preserve only fades belonging to an original edge when a Clip is
    /// split or captured into a smaller fragment.
    pub const fn for_fragment(
        self,
        original_duration: u64,
        local_start: u64,
        fragment_duration: u64,
    ) -> Self {
        let keeps_start = local_start == 0;
        let keeps_end = local_start.saturating_add(fragment_duration) >= original_duration;
        let mut fades = Self::new(
            if keeps_start { self.fade_in_frames } else { 0 },
            if keeps_end { self.fade_out_frames } else { 0 },
            fragment_duration,
        );
        fades.fade_in_curve = if keeps_start && !self.crossfade_in_active {
            self.fade_in_curve
        } else {
            FadeCurve::new(0)
        };
        fades.fade_out_curve = if keeps_end && !self.crossfade_out_active {
            self.fade_out_curve
        } else {
            FadeCurve::new(0)
        };
        fades.crossfade_in_peer = if keeps_start {
            self.crossfade_in_peer
        } else {
            None
        };
        fades.crossfade_out_peer = if keeps_end {
            self.crossfade_out_peer
        } else {
            None
        };
        fades.previous_crossfade_in_curve = if self.crossfade_in_active {
            self.fade_in_curve
        } else {
            self.previous_crossfade_in_curve
        };
        fades.previous_crossfade_out_curve = if self.crossfade_out_active {
            self.fade_out_curve
        } else {
            self.previous_crossfade_out_curve
        };
        fades
    }

    pub const fn is_neutral(value: &Self) -> bool {
        value.fade_in_frames == 0
            && value.fade_out_frames == 0
            && !value.crossfade_in_active
            && !value.crossfade_out_active
    }

    pub const fn crossfade_in_from(self) -> Option<ClipId> {
        if self.crossfade_in_active {
            self.crossfade_in_peer
        } else {
            None
        }
    }

    pub const fn crossfade_out_to(self) -> Option<ClipId> {
        if self.crossfade_out_active {
            self.crossfade_out_peer
        } else {
            None
        }
    }

    pub fn linked_fade_in(self, frames: u64, from: ClipId, duration: u64) -> Self {
        let mut fades = self.with_fade_in(frames, duration);
        fades.fade_in_curve = if self.crossfade_in_peer == Some(from) {
            self.previous_crossfade_in_curve
        } else {
            FadeCurve::new(0)
        };
        fades.crossfade_in_peer = (fades.fade_in_frames > 0).then_some(from);
        fades.crossfade_in_active = fades.fade_in_frames > 0;
        fades.previous_crossfade_in_curve = FadeCurve::new(0);
        fades
    }

    pub fn linked_fade_out(self, frames: u64, to: ClipId, duration: u64) -> Self {
        let mut fades = self.with_fade_out(frames, duration);
        fades.fade_out_curve = if self.crossfade_out_peer == Some(to) {
            self.previous_crossfade_out_curve
        } else {
            FadeCurve::new(0)
        };
        fades.crossfade_out_peer = (fades.fade_out_frames > 0).then_some(to);
        fades.crossfade_out_active = fades.fade_out_frames > 0;
        fades.previous_crossfade_out_curve = FadeCurve::new(0);
        fades
    }

    pub const fn unlinked(self) -> Self {
        self.unlink_fade_in().unlink_fade_out()
    }

    pub const fn unlink_fade_in(self) -> Self {
        if self.crossfade_in_active {
            Self {
                fade_in_curve: FadeCurve::new(0),
                crossfade_in_active: false,
                previous_crossfade_in_curve: self.fade_in_curve,
                ..self
            }
        } else {
            self
        }
    }

    pub const fn unlink_fade_out(self) -> Self {
        if self.crossfade_out_active {
            Self {
                fade_out_curve: FadeCurve::new(0),
                crossfade_out_active: false,
                previous_crossfade_out_curve: self.fade_out_curve,
                ..self
            }
        } else {
            self
        }
    }

    pub const fn with_fade_in_curve(self, curve: FadeCurve) -> Self {
        Self {
            fade_in_curve: if self.fade_in_frames > 0 {
                curve
            } else {
                FadeCurve::new(0)
            },
            ..self
        }
    }

    pub const fn with_fade_out_curve(self, curve: FadeCurve) -> Self {
        Self {
            fade_out_curve: if self.fade_out_frames > 0 {
                curve
            } else {
                FadeCurve::new(0)
            },
            ..self
        }
    }

    pub const fn with_linked_fade_in_curve(self, curve: FadeCurve) -> Self {
        Self {
            fade_in_curve: if self.crossfade_in_active {
                curve
            } else {
                self.fade_in_curve
            },
            ..self
        }
    }

    pub const fn with_linked_fade_out_curve(self, curve: FadeCurve) -> Self {
        Self {
            fade_out_curve: if self.crossfade_out_active {
                curve
            } else {
                self.fade_out_curve
            },
            ..self
        }
    }

    /// Per-frame amplitude. This is safe to call on the audio thread.
    #[inline]
    pub fn gain_at(self, clip_frame: u64, duration: u64) -> f32 {
        if clip_frame >= duration {
            return 0.0;
        }
        let fade_in = if self.fade_in_frames > 0 && clip_frame < self.fade_in_frames {
            let progress = clip_frame as f32 / self.fade_in_frames as f32;
            self.fade_in_curve
                .gain_for(progress, self.crossfade_in_active)
        } else {
            1.0
        };
        let frames_after = duration - 1 - clip_frame;
        let fade_out = if self.fade_out_frames > 0 && frames_after < self.fade_out_frames {
            let progress = if self.crossfade_out_active {
                (frames_after + 1) as f32 / self.fade_out_frames as f32
            } else {
                frames_after as f32 / self.fade_out_frames as f32
            };
            self.fade_out_curve
                .fade_out_gain(1.0 - progress, self.crossfade_out_active)
        } else {
            1.0
        };
        fade_in.min(fade_out)
    }
}
