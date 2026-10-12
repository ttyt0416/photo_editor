use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlendMode {
    #[default]
    Normal,
    Dissolve,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Add,
    Subtract,
    LinearBurn,
    DarkerColor,
    LighterColor,
    LinearLight,
    VividLight,
    PinLight,
    HardMix,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [Self; 27] = [
        Self::Normal,
        Self::Dissolve,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::HardLight,
        Self::SoftLight,
        Self::Difference,
        Self::Exclusion,
        Self::Add,
        Self::Subtract,
        Self::LinearBurn,
        Self::DarkerColor,
        Self::LighterColor,
        Self::LinearLight,
        Self::VividLight,
        Self::PinLight,
        Self::HardMix,
        Self::Divide,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Dissolve => "Dissolve",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::ColorDodge => "Color Dodge",
            Self::ColorBurn => "Color Burn",
            Self::HardLight => "Hard Light",
            Self::SoftLight => "Soft Light",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::LinearBurn => "Linear Burn",
            Self::DarkerColor => "Darker Color",
            Self::LighterColor => "Lighter Color",
            Self::LinearLight => "Linear Light",
            Self::VividLight => "Vivid Light",
            Self::PinLight => "Pin Light",
            Self::HardMix => "Hard Mix",
            Self::Divide => "Divide",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
        }
    }
    #[cfg(test)]
    pub fn channel(self, s: f32, d: f32) -> f32 {
        crate::precision::float_channel(self.channel_precise(f64::from(s), f64::from(d)))
    }
    pub(crate) fn channel_precise(self, s: f64, d: f64) -> f64 {
        match self {
            Self::Normal | Self::Dissolve => s,
            Self::Multiply => s * d,
            Self::Screen => {
                if s == 1. || d == 1. {
                    1.
                } else if s.abs() >= d.abs() {
                    s * (1. - d) + d
                } else {
                    d * (1. - s) + s
                }
            }
            Self::Overlay => Self::HardLight.channel_precise(d, s),
            Self::Darken => s.min(d),
            Self::Lighten => s.max(d),
            Self::ColorDodge => {
                if d <= 0.0 {
                    0.0
                } else if s >= 1.0 {
                    1.0
                } else {
                    (d / (1.0 - s)).min(1.0)
                }
            }
            Self::ColorBurn => {
                if d >= 1.0 {
                    1.0
                } else if s <= 0.0 {
                    0.0
                } else if s == 1. {
                    d.max(0.)
                } else {
                    1.0 - ((1.0 - d) / s).min(1.0)
                }
            }
            Self::HardLight => {
                if s <= 0.5 {
                    2.0 * s * d
                } else {
                    1.0 - 2.0 * (1.0 - s) * (1.0 - d)
                }
            }
            Self::SoftLight => {
                if d == 0. || d == 1. {
                    return d;
                }
                if s <= 0.5 {
                    d * (2. * s + (1. - 2. * s) * d)
                } else {
                    let g = if d <= 0.25 {
                        ((16. * d - 12.) * d + 4.) * d
                    } else {
                        d.sqrt()
                    };
                    let factor = 2. * s - 1.;
                    (1. - factor) * d + factor * g
                }
            }
            Self::Difference => (d - s).abs(),
            Self::Exclusion => {
                if s.abs() >= d.abs() {
                    s * (1. - 2. * d) + d
                } else {
                    d * (1. - 2. * s) + s
                }
            }
            Self::Add => (d + s).min(1.0),
            Self::Subtract => (d - s).max(0.0),
            Self::LinearBurn => (if s.abs() >= d.abs() {
                (s - 1.) + d
            } else {
                (d - 1.) + s
            })
            .max(0.),
            Self::LinearLight => {
                let s = 2. * s;
                (if s.abs() >= d.abs() {
                    (s - 1.) + d
                } else {
                    (d - 1.) + s
                })
                .clamp(0., 1.)
            }
            Self::VividLight => {
                if s < 0.5 {
                    Self::ColorBurn.channel_precise(s * 2.0, d)
                } else {
                    Self::ColorDodge.channel_precise(s * 2.0 - 1.0, d)
                }
            }
            Self::PinLight => {
                if s < 0.5 {
                    d.min(s * 2.0)
                } else {
                    d.max(s * 2.0 - 1.0)
                }
            }
            Self::HardMix => {
                if Self::VividLight.channel_precise(s, d) < 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
            Self::Divide => {
                if s <= 0.0 {
                    1.0
                } else {
                    (d / s).min(1.0)
                }
            }
            Self::DarkerColor
            | Self::LighterColor
            | Self::Hue
            | Self::Saturation
            | Self::Color
            | Self::Luminosity => s,
        }
    }

    // W3C Compositing and Blending Level 1, section 10.2.
    pub fn color(self, s: [f32; 3], d: [f32; 3]) -> [f32; 3] {
        self.color_precise(s.map(f64::from), d.map(f64::from))
            .map(crate::precision::float_channel)
    }
    pub(crate) fn color_precise(self, s: [f64; 3], d: [f64; 3]) -> [f64; 3] {
        match self {
            Self::DarkerColor => {
                if s.into_iter().sum::<f64>() < d.into_iter().sum::<f64>() {
                    s
                } else {
                    d
                }
            }
            Self::LighterColor => {
                if s.into_iter().sum::<f64>() > d.into_iter().sum::<f64>() {
                    s
                } else {
                    d
                }
            }
            Self::Hue => set_luminance(set_saturation(s, saturation(d)), luminance_precise(d)),
            Self::Saturation => {
                set_luminance(set_saturation(d, saturation(s)), luminance_precise(d))
            }
            Self::Color => set_luminance(s, luminance_precise(d)),
            Self::Luminosity => set_luminance(d, luminance_precise(s)),
            _ => std::array::from_fn(|c| self.channel_precise(s[c], d[c])),
        }
    }
}

#[cfg(test)]
fn luminance(c: [f32; 3]) -> f64 {
    luminance_precise(c.map(f64::from))
}
fn luminance_precise(c: [f64; 3]) -> f64 {
    // Algebraically 0.3R + 0.59G + 0.11B, preserving equal large channels exactly.
    c[1] + (c[0] - c[1]) * 0.3 + (c[2] - c[1]) * 0.11
}
fn saturation(c: [f64; 3]) -> f64 {
    c.into_iter().fold(f64::NEG_INFINITY, f64::max) - c.into_iter().fold(f64::INFINITY, f64::min)
}
fn set_luminance(mut c: [f64; 3], value: f64) -> [f64; 3] {
    let value = value.clamp(0., 1.);
    let lum = luminance_precise(c);
    let differences = c.map(|v| v - lum);
    let n = differences.into_iter().fold(f64::INFINITY, f64::min);
    let x = differences.into_iter().fold(f64::NEG_INFINITY, f64::max);
    // Use the known target luminance; re-measuring after adding it to HDR differences
    // would lose a small luminance to cancellation. These are the same ClipColor ratios.
    c = differences.map(|v| value + v);
    if n < -value {
        c = differences.map(|v| value + v * (value / -n));
    }
    if x > 1. - value {
        c = c.map(|v| value + (v - value) * ((1. - value) / x));
    }
    c
}
fn set_saturation(mut c: [f64; 3], value: f64) -> [f64; 3] {
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| c[*a].total_cmp(&c[*b]));
    let [min, mid, max] = order;
    if c[max] > c[min] {
        c[mid] = (c[mid] - c[min]) * value / (c[max] - c[min]);
        c[max] = value;
    } else {
        c[mid] = 0.0;
        c[max] = 0.0;
    }
    c[min] = 0.0;
    c
}

pub fn composite(source: [u8; 4], target: [u8; 4], opacity: f32, mode: BlendMode) -> [u8; 4] {
    let sa = source[3] as f32 / 255.0 * opacity;
    let da = target[3] as f32 / 255.0;
    let alpha = sa + da * (1.0 - sa);
    if alpha <= 0.0 {
        return [0; 4];
    }
    let mut result = [0; 4];
    let blended = mode.color(
        std::array::from_fn(|c| source[c] as f32 / 255.0),
        std::array::from_fn(|c| target[c] as f32 / 255.0),
    );
    for c in 0..3 {
        let s = source[c] as f32 / 255.0;
        let d = target[c] as f32 / 255.0;
        let color = (sa * (1.0 - da) * s + sa * da * blended[c] + (1.0 - sa) * da * d) / alpha;
        result[c] = (color * 255.0).clamp(0.0, 255.0).round() as u8;
    }
    result[3] = (alpha * 255.0).round() as u8;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_color_mode_matches_reference_pixels() {
        let references = [
            (BlendMode::Normal, 0.2),
            (BlendMode::Multiply, 0.12),
            (BlendMode::Screen, 0.68),
            (BlendMode::Overlay, 0.36),
            (BlendMode::Darken, 0.2),
            (BlendMode::Lighten, 0.6),
            (BlendMode::ColorDodge, 0.75),
            (BlendMode::ColorBurn, 0.0),
            (BlendMode::HardLight, 0.24),
            (BlendMode::SoftLight, 0.456),
            (BlendMode::Difference, 0.4),
            (BlendMode::Exclusion, 0.56),
            (BlendMode::Add, 0.8),
            (BlendMode::Subtract, 0.4),
            (BlendMode::LinearBurn, 0.0),
            (BlendMode::DarkerColor, 0.2),
            (BlendMode::LighterColor, 0.6),
            (BlendMode::LinearLight, 0.0),
            (BlendMode::VividLight, 0.0),
            (BlendMode::PinLight, 0.4),
            (BlendMode::HardMix, 0.0),
            (BlendMode::Divide, 1.0),
            (BlendMode::Hue, 0.6),
            (BlendMode::Saturation, 0.6),
            (BlendMode::Color, 0.6),
            (BlendMode::Luminosity, 0.2),
        ];
        for (mode, expected) in references {
            let color = mode.color([0.2; 3], [0.6; 3]);
            assert!(
                color.into_iter().all(|v| (v - expected).abs() < 0.00001),
                "{}",
                mode.name()
            );
            let combined =
                crate::precision::over([0.2, 0.2, 0.2, 1.0], [0.6, 0.6, 0.6, 1.0], 0.5, mode);
            assert!(
                (combined[0] - (expected * 0.5 + 0.3)).abs() < 0.00001,
                "{} opacity",
                mode.name()
            );
            assert_eq!(combined[3], 1.0);
        }
    }
    #[test]
    fn nonseparable_modes_keep_expected_luminance_and_transparent_compositing() {
        assert_eq!(BlendMode::ColorDodge.channel(1.0, 0.0), 0.0);
        assert_eq!(BlendMode::ColorBurn.channel(0.0, 1.0), 1.0);
        for mode in BlendMode::ALL {
            assert!(
                mode.color([2.0, 2.0, 2.0], [3.0, 3.0, 3.0])
                    .into_iter()
                    .all(f32::is_finite)
            );
        }
        assert_eq!(
            BlendMode::DarkerColor.color([1.0, 0.0, 0.0], [0.4, 0.4, 0.4]),
            [1.0, 0.0, 0.0]
        );
        let source = [0.9, 0.1, 0.4];
        let target = [0.2, 0.7, 0.3];
        for mode in [BlendMode::Hue, BlendMode::Saturation, BlendMode::Color] {
            let out = mode.color(source, target);
            assert!((luminance(out) - luminance(target)).abs() < 0.00001);
            assert!(out.into_iter().all(|v| (-0.00001..=1.00001).contains(&v)));
        }
        assert!(
            (luminance(BlendMode::Luminosity.color(source, target)) - luminance(source)).abs()
                < 0.00001
        );
        assert_eq!(
            composite([255, 0, 0, 128], [0, 0, 0, 0], 1.0, BlendMode::Multiply),
            [255, 0, 0, 128]
        );
        assert_eq!(
            composite([255, 0, 0, 128], [0, 0, 255, 128], 1.0, BlendMode::Normal),
            [170, 0, 85, 192]
        );
    }
}

#[cfg(test)]
#[path = "blend_hdr_tests.rs"]
mod hdr_tests;
