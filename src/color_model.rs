use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    #[default]
    Rgb,
    Hsv,
    Hsl,
}
impl Model {
    pub const ALL: [Self; 3] = [Self::Rgb, Self::Hsv, Self::Hsl];
    pub fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Hsv => "HSV",
            Self::Hsl => "HSL",
        }
    }
    pub fn components(self, color: [u8; 4]) -> [f64; 3] {
        if self == Self::Rgb {
            return [color[0], color[1], color[2]].map(f64::from);
        }
        let [r, g, b] = [color[0], color[1], color[2]].map(|v| f64::from(v) / 255.);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let hue = if delta == 0. {
            0.
        } else if max == r {
            60. * ((g - b) / delta).rem_euclid(6.)
        } else if max == g {
            60. * ((b - r) / delta + 2.)
        } else {
            60. * ((r - g) / delta + 4.)
        };
        match self {
            Self::Hsv => [
                hue,
                if max == 0. { 0. } else { delta / max * 100. },
                max * 100.,
            ],
            Self::Hsl => {
                let light = (max + min) / 2.;
                [
                    hue,
                    if delta == 0. {
                        0.
                    } else {
                        (delta / (1. - (2. * light - 1.).abs()) * 100.).clamp(0., 100.)
                    },
                    light * 100.,
                ]
            }
            Self::Rgb => unreachable!(),
        }
    }
    pub fn rgb(self, values: [f64; 3]) -> Option<[u8; 3]> {
        if !values.iter().all(|v| v.is_finite()) {
            return None;
        }
        if self == Self::Rgb {
            return values
                .iter()
                .all(|v| (0. ..=255.).contains(v))
                .then(|| values.map(|v| v.round() as u8));
        }
        let [h, s, v] = values;
        if !(0. ..=360.).contains(&h) || !(0. ..=100.).contains(&s) || !(0. ..=100.).contains(&v) {
            return None;
        }
        let h = h.rem_euclid(360.) / 60.;
        let s = s / 100.;
        let v = v / 100.;
        let chroma = if self == Self::Hsv {
            v * s
        } else {
            (1. - (2. * v - 1.).abs()) * s
        };
        let x = chroma * (1. - (h.rem_euclid(2.) - 1.).abs());
        let offset = if self == Self::Hsv {
            v - chroma
        } else {
            v - chroma / 2.
        };
        let components = match h.floor() as u8 {
            0 => [chroma, x, 0.],
            1 => [x, chroma, 0.],
            2 => [0., chroma, x],
            3 => [0., x, chroma],
            4 => [x, 0., chroma],
            _ => [chroma, 0., x],
        };
        Some(components.map(|value| ((value + offset) * 255.).round().clamp(0., 255.) as u8))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_model_hsv_hsl_boundaries_exact_rgb_roundtrips_and_independent_fixtures() {
        let mut rows = Vec::new();
        for r in [0, 1, 32, 64, 128, 192, 255] {
            for g in [0, 1, 32, 64, 128, 192, 255] {
                for b in [0, 1, 32, 64, 128, 192, 255] {
                    for alpha in [0, 83, 255] {
                        let color = [r, g, b, alpha];
                        for model in Model::ALL {
                            let values = model.components(color);
                            assert_eq!(model.rgb(values), Some([r, g, b]));
                            rows.push(serde_json::json!({"model":model,"rgba":color,"values":values,"rgb":model.rgb(values)}));
                        }
                    }
                }
            }
        }
        let mut inputs = Vec::new();
        for model in [Model::Hsv, Model::Hsl] {
            for h in [0., 0.5, 60., 120., 179.3, 240., 300., 359.9, 360.] {
                for s in [0., 10.1, 50., 100.] {
                    for v in [0., 25.6, 50., 100.] {
                        let values = [h, s, v];
                        inputs.push(serde_json::json!({"model":model,"values":values,"rgb":model.rgb(values)}));
                    }
                }
            }
        }
        for model in Model::ALL {
            for value in [
                [f64::NAN, 0., 0.],
                [0., f64::INFINITY, 0.],
                [-1., 10., 10.],
                [1., -1., 0.],
                [1., 0., -1.],
            ] {
                assert!(model.rgb(value).is_none());
            }
        }
        assert_eq!(Model::Hsv.rgb([360., 100., 100.]), Some([255, 0, 0]));
        assert_eq!(Model::Hsl.rgb([120., 100., 50.]), Some([0, 255, 0]));
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/color-models");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("conversions.json"),
            serde_json::to_vec_pretty(&serde_json::json!({"roundtrips":rows,"inputs":inputs}))
                .unwrap(),
        )
        .unwrap();
    }
}
