use rawler::imgop::xyz::Illuminant;
pub(super) fn inverse(matrix: [[f64; 3]; 3]) -> Result<[[f64; 3]; 3], String> {
    let mut work: [[f64; 6]; 3] = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            if j < 3 {
                matrix[i][j]
            } else {
                f64::from(i == j - 3)
            }
        })
    });
    let scale = matrix
        .as_flattened()
        .iter()
        .map(|v| v.abs())
        .fold(0f64, f64::max);
    if !scale.is_finite() || scale == 0. {
        return Err("RAW 색 행렬을 역변환할 수 없습니다.".into());
    }
    for i in 0..3 {
        let pivot = (i..3)
            .max_by(|a, b| work[*a][i].abs().total_cmp(&work[*b][i].abs()))
            .unwrap();
        if work[pivot][i].abs() <= scale * 1e-12 {
            return Err("RAW 색 행렬이 특이하거나 불안정하여 역변환할 수 없습니다.".into());
        }
        work.swap(i, pivot);
        let divisor = work[i][i];
        for v in &mut work[i] {
            *v /= divisor;
        }
        let row = work[i];
        for (j, target) in work.iter_mut().enumerate() {
            if j == i {
                continue;
            }
            let amount = target[i];
            for c in 0..6 {
                target[c] -= amount * row[c];
            }
        }
    }
    Ok(work.map(|row| [row[3], row[4], row[5]]))
}
pub(super) fn adapt_input(
    native: [[f32; 3]; 4],
    illuminant: Illuminant,
) -> Result<[[f32; 3]; 4], String> {
    if illuminant == Illuminant::D65 {
        return Ok(native);
    }
    adapt_white(native, white_point(illuminant)?)
}
pub(super) fn white_point(illuminant: Illuminant) -> Result<[f64; 3], String> {
    Ok(match illuminant {
        Illuminant::A => [1.09850, 1., 0.35585],
        Illuminant::B => [0.99072, 1., 0.85223],
        Illuminant::C => [0.98074, 1., 1.18232],
        Illuminant::D50 => [0.96422, 1., 0.82521],
        Illuminant::D55 | Illuminant::Flash => [0.95682, 1., 0.92149],
        Illuminant::D75 => [0.94972, 1., 1.22638],
        Illuminant::D65 | Illuminant::Daylight => [0.95047, 1., 1.08883],
        _ => return Err("이 RAW 색 행렬 광원은 아직 처리할 수 없습니다.".into()),
    })
}
pub(super) fn adapt_white(native: [[f32; 3]; 4], white: [f64; 3]) -> Result<[[f32; 3]; 4], String> {
    if white.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("RAW 광원 색상이 잘못되었습니다.".into());
    }
    let d65 = [0.95047, 1., 1.08883];
    const B: [[f64; 3]; 3] = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let dot = |row: [f64; 3], column: [f64; 3]| {
        row.into_iter().zip(column).map(|(a, b)| a * b).sum::<f64>()
    };
    let ratio = std::array::from_fn::<_, 3, _>(|i| dot(B[i], white) / dot(B[i], d65));
    let inv = inverse(B)?;
    let cat = std::array::from_fn::<_, 3, _>(|i| {
        std::array::from_fn::<_, 3, _>(|j| {
            (0..3).map(|k| inv[i][k] * ratio[k] * B[k][j]).sum::<f64>()
        })
    });
    let adapted = native.map(|row| {
        std::array::from_fn::<_, 3, _>(|j| {
            (0..3).map(|k| f64::from(row[k]) * cat[k][j]).sum::<f64>() as f32
        })
    });
    if adapted.as_flattened().iter().any(|v| !v.is_finite()) {
        return Err("RAW 광원 색 행렬이 부동소수점 범위를 넘습니다.".into());
    }
    Ok(adapted)
}

pub(super) fn pseudo_inverse(matrix: [[f64; 3]; 4]) -> Result<[[f64; 4]; 3], String> {
    if matrix.as_flattened().iter().any(|v| !v.is_finite()) {
        return Err("RAW 색 행렬이 잘못되었습니다.".into());
    }
    let gram = std::array::from_fn::<_, 3, _>(|i| {
        std::array::from_fn::<_, 3, _>(|j| matrix.iter().map(|row| row[i] * row[j]).sum::<f64>())
    });
    let inv = inverse(gram)?;
    let result = std::array::from_fn::<_, 3, _>(|i| {
        std::array::from_fn::<_, 4, _>(|j| (0..3).map(|k| inv[i][k] * matrix[j][k]).sum::<f64>())
    });
    if result.as_flattened().iter().any(|v| !v.is_finite()) {
        return Err("RAW 색 행렬을 역변환할 수 없습니다.".into());
    }
    Ok(result)
}
