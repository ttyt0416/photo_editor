use super::*;

fn native([x, y]: [u32; 2], [w, h]: [u32; 2], orientation: u8) -> [u32; 2] {
    match orientation {
        2 => [w - 1 - x, y],
        3 => [w - 1 - x, h - 1 - y],
        4 => [x, h - 1 - y],
        5 => [y, x],
        6 => [y, h - 1 - x],
        7 => [w - 1 - y, h - 1 - x],
        8 => [w - 1 - y, x],
        _ => [x, y],
    }
}
fn gains(
    average: [f64; 4],
    matrix: [[f32; 4]; 3],
    fourth_gain: f32,
    monochrome: bool,
) -> Result<[f32; 3], String> {
    if !average.iter().all(|v| v.is_finite()) || average[0] <= 0. {
        return Err("선택한 부분이 너무 어둡거나 유효한 색 정보를 갖고 있지 않습니다.".into());
    }
    if monochrome {
        return Ok([1.; 3]);
    }
    if average[1] <= 0. || average[2] <= 0. {
        return Err("선택한 부분에 화이트 밸런스를 계산할 색 정보가 부족합니다.".into());
    }
    // Keep G=1 and the existing fourth-channel gain. Solve camera R/B energy and
    // equal linear working RGB rather than dividing gamma-corrected preview pixels.
    let mut rows: [[f64; 4]; 3] = std::array::from_fn(|i| {
        [
            f64::from(matrix[i][0]),
            f64::from(matrix[i][2]),
            -1.,
            -(f64::from(matrix[i][1]) * average[1]
                + f64::from(matrix[i][3]) * average[3] * f64::from(fourth_gain)),
        ]
    });
    let scale = rows
        .iter()
        .flat_map(|r| r[..3].iter())
        .map(|v| v.abs())
        .fold(0., f64::max);
    for i in 0..3 {
        let pivot = (i..3)
            .max_by(|a, b| rows[*a][i].abs().total_cmp(&rows[*b][i].abs()))
            .unwrap();
        if rows[pivot][i].abs() <= scale * 1e-12 {
            return Err("카메라 색 행렬에서 안정적인 화이트 밸런스를 계산할 수 없습니다.".into());
        }
        rows.swap(i, pivot);
        let divisor = rows[i][i];
        for v in &mut rows[i][i..] {
            *v /= divisor;
        }
        let normalized = rows[i];
        for (j, row) in rows.iter_mut().enumerate() {
            if j != i {
                let factor = row[i];
                for c in i..4 {
                    row[c] -= factor * normalized[c];
                }
            }
        }
    }
    if rows[2][3] <= 0. {
        return Err(
            "선택한 색상을 중립색으로 보정할 수 없습니다. 다른 흰색·회색 부분을 선택하세요.".into(),
        );
    }
    let result = [
        (rows[0][3] / average[0]) as f32,
        1.,
        (rows[1][3] / average[2]) as f32,
    ];
    if !(Settings {
        white_balance: WhiteBalance::Custom,
        gains: result,
        ..Default::default()
    })
    .valid()
    {
        return Err(
            "계산한 화이트 밸런스가 현재 조절 범위를 넘습니다. 다른 흰색·회색 부분을 선택하세요."
                .into(),
        );
    }
    Ok(result)
}
pub(super) fn sample(
    source: &Source,
    focus: [f32; 2],
    radius: u32,
) -> Result<BalanceSample, String> {
    if !source.valid() {
        return Err("RAW 원본 또는 설정이 잘못되었습니다.".into());
    }
    if !focus
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        || radius > 64
    {
        return Err("화이트 밸런스 샘플 위치 또는 반경이 잘못되었습니다.".into());
    }
    let (info, raw, developed) = develop_sensor(&source.bytes, &source.record.filename, false)?;
    let dim = developed.dim();
    let size = [dim.w as u32, dim.h as u32];
    crate::document::validate_size(size[0], size[1])?;
    let oriented = if info.orientation >= 5 {
        [size[1], size[0]]
    } else {
        size
    };
    let center: [u32; 2] =
        std::array::from_fn(|i| (focus[i] * oriented[i].saturating_sub(1) as f32).round() as u32);
    let min = center.map(|v| v.saturating_sub(radius));
    let max =
        std::array::from_fn::<_, 2, _>(|i| center[i].saturating_add(radius).min(oriented[i] - 1));
    let mut average = [0.; 4];
    let mut count = 0.;
    for y in min[1]..=max[1] {
        for x in min[0]..=max[0] {
            let [nx, ny] = native([x, y], size, info.orientation);
            let index = ny as usize * dim.w + nx as usize;
            let p: &[f32] = match &developed {
                Intermediate::Monochrome(p) => std::slice::from_ref(&p.data[index]),
                Intermediate::ThreeColor(p) => &p.data[index],
                Intermediate::FourColor(p) => &p.data[index],
            };
            for (i, v) in p.iter().enumerate() {
                average[i] += f64::from(*v);
            }
            count += 1.;
        }
    }
    average = average.map(|v| v / count);
    let fourth_gain = if raw.wb_coeffs[3].is_finite() && raw.wb_coeffs[3] > 0. {
        raw.wb_coeffs[3]
    } else {
        1.
    };
    let channels = match &developed {
        Intermediate::FourColor(_) => 4,
        _ => 3,
    };
    let cal = dng_calibration::Calibration::read(&source.bytes, channels)?;
    if channels == 3 && (info.monochrome || raw.color_matrix.len() < 2) {
        return Ok(BalanceSample {
            gains: gains(
                average,
                calibration_calibrated(&raw, &cal)?,
                fourth_gain,
                info.monochrome,
            )?,
            gains_fourth: None,
        });
    }
    if average[..channels]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
    {
        return Err("선택한 부분에 화이트 밸런스를 계산할 색 정보가 부족합니다.".into());
    }
    // Every camera sensor channel must balance together. Fixing the fourth
    // camera gain can make the neutral constraints impossible for valid samples.
    let balanced = std::array::from_fn::<_, 4, _>(|i| {
        if i < channels {
            (average[1] / average[i]) as f32
        } else {
            1.
        }
    });
    let sample = BalanceSample {
        gains: balanced[..3].try_into().unwrap(),
        gains_fourth: (channels == 4).then_some(balanced[3]),
    };
    if !(Settings {
        white_balance: WhiteBalance::Custom,
        gains: sample.gains,
        gains_fourth: sample.gains_fourth,
        ..Default::default()
    })
    .valid()
    {
        return Err(
            "계산한 화이트 밸런스가 현재 조절 범위를 넘습니다. 다른 흰색·회색 부분을 선택하세요."
                .into(),
        );
    }
    balance_calibration(
        &raw,
        channels,
        if channels == 4 {
            balanced
        } else {
            [balanced[0], balanced[1], balanced[2], fourth_gain]
        },
        &cal,
    )?;
    Ok(sample)
}
#[cfg(test)]
#[path = "raw_white_balance_tests.rs"]
mod tests;
