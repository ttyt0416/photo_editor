use image::Rgba32FImage;
use moxcms::{ColorProfile, DataColorSpace, ToneReprCurve};

pub(crate) fn convert(
    pixels: &Rgba32FImage,
    bytes: &[u8],
    gray: bool,
) -> Result<Rgba32FImage, String> {
    crate::document::validate_size(pixels.width(), pixels.height())?;
    if pixels.as_raw().iter().any(|v| !v.is_finite()) {
        return Err("색상 또는 알파 값이 유한하지 않습니다".into());
    }
    if bytes.len() > crate::source_metadata::MAX_BYTES {
        return Err("ICC 보관 예산 초과".into());
    }
    let profile = ColorProfile::new_from_slice(bytes).map_err(|e| e.to_string())?;
    if profile.color_space
        != if gray {
            DataColorSpace::Gray
        } else {
            DataColorSpace::Rgb
        }
    {
        return Err("복원 색상 채널과 ICC가 일치하지 않습니다".into());
    }
    if profile.pcs != DataColorSpace::Xyz
        || profile.lut_a_to_b_perceptual.is_some()
        || profile.lut_a_to_b_colorimetric.is_some()
        || profile.lut_a_to_b_saturation.is_some()
    {
        return Err("행렬·명암 곡선 ICC만 작업 변환을 지원합니다".into());
    }
    let destination = ColorProfile::new_from_slice(crate::color_management::srgb_profile())
        .map_err(|e| e.to_string())?;
    let matrix = profile.transform_matrix(&destination).v;
    let curves = if gray {
        [
            profile.gray_trc.as_ref(),
            profile.gray_trc.as_ref(),
            profile.gray_trc.as_ref(),
        ]
    } else {
        [
            profile.red_trc.as_ref(),
            profile.green_trc.as_ref(),
            profile.blue_trc.as_ref(),
        ]
    };
    let [red, green, blue] = curves.map(|c| c.ok_or("ICC 명암 곡선 누락"));
    let curves = [red?, green?, blue?];
    let ToneReprCurve::Parametric(target_curve) =
        destination.red_trc.as_ref().ok_or("작업 곡선 누락")?
    else {
        return Err("작업 곡선 오류".into());
    };
    let [g, a, b, c, d, ..] = target_curve.as_slice() else {
        return Err("작업 곡선 오류".into());
    };
    let (g, a, b, c, d) = (
        f64::from(*g),
        f64::from(*a),
        f64::from(*b),
        f64::from(*c),
        f64::from(*d),
    );
    let mut result = pixels.clone();
    for (source, target) in pixels.pixels().zip(result.pixels_mut()) {
        let linear = [
            tone(curves[0], f64::from(source[0]))?,
            tone(curves[1], f64::from(source[if gray { 0 } else { 1 }]))?,
            tone(curves[2], f64::from(source[if gray { 0 } else { 2 }]))?,
        ];
        for channel in 0..3 {
            let value = if gray {
                linear[channel]
            } else {
                (0..3).map(|i| matrix[channel][i] * linear[i]).sum()
            };
            let encoded = if value < c * d {
                value / c
            } else {
                (value.powf(1. / g) - b) / a
            };
            if !encoded.is_finite() || !(encoded as f32).is_finite() {
                return Err("ICC 변환 결과가 유한하지 않습니다".into());
            }
            target[channel] = encoded as f32;
        }
    }
    Ok(result)
}

// Sampled ICC curves define [0,1]. Outside it, extend each end segment by its
// slope; parametric curves use their stated formula (signed pure gamma).
fn tone(curve: &ToneReprCurve, value: f64) -> Result<f64, String> {
    let result = match curve {
        ToneReprCurve::Lut(table) if table.is_empty() => value,
        ToneReprCurve::Lut(table) if table.len() == 1 => {
            value.signum() * value.abs().powf(f64::from(table[0]) / 256.)
        }
        ToneReprCurve::Lut(table) => {
            let position = value * (table.len() - 1) as f64;
            let index = (position.floor().max(0.) as usize).min(table.len() - 2);
            let fraction = position - index as f64;
            (f64::from(table[index]) * (1. - fraction) + f64::from(table[index + 1]) * fraction)
                / 65535.
        }
        ToneReprCurve::Parametric(p) => {
            let g = f64::from(*p.first().ok_or("빈 ICC 곡선")?);
            match p.as_slice() {
                [_] => value.signum() * value.abs().powf(g),
                [_, a, b] => {
                    if value >= -f64::from(*b) / f64::from(*a) {
                        (f64::from(*a) * value + f64::from(*b)).max(0.).powf(g)
                    } else {
                        0.
                    }
                }
                [_, a, b, c] => {
                    if value >= -f64::from(*b) / f64::from(*a) {
                        (f64::from(*a) * value + f64::from(*b)).max(0.).powf(g) + f64::from(*c)
                    } else {
                        f64::from(*c)
                    }
                }
                [_, a, b, c, d] => {
                    if value >= f64::from(*d) {
                        (f64::from(*a) * value + f64::from(*b)).max(0.).powf(g)
                    } else {
                        f64::from(*c) * value
                    }
                }
                [_, a, b, c, d, e, f] => {
                    if value >= f64::from(*d) {
                        (f64::from(*a) * value + f64::from(*b)).max(0.).powf(g) + f64::from(*e)
                    } else {
                        f64::from(*c) * value + f64::from(*f)
                    }
                }
                _ => return Err("미지원 ICC 곡선".into()),
            }
        }
    };
    if result.is_finite() {
        Ok(result)
    } else {
        Err("ICC 곡선 값이 유한하지 않습니다".into())
    }
}
