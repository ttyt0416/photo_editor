use crate::{document::Selection, transform::Handle};
pub fn handles(rect: &Selection) -> Vec<(Handle, [f32; 2])> {
    [
        [0, 0],
        [1, 0],
        [2, 0],
        [2, 1],
        [2, 2],
        [1, 2],
        [0, 2],
        [0, 1],
    ]
    .into_iter()
    .map(|h| {
        (
            Handle::Resize(h),
            std::array::from_fn(|i| {
                rect.min[i] as f32 + (rect.max[i] - rect.min[i]) as f32 * h[i] as f32 / 2.
            }),
        )
    })
    .collect()
}
pub fn drag(
    initial: &Selection,
    handle: Handle,
    start: [f32; 2],
    point: [f32; 2],
    size: [u32; 2],
    ratio: Option<f32>,
    centered: bool,
) -> Selection {
    let delta = [point[0] - start[0], point[1] - start[1]];
    if handle == Handle::Move {
        let delta = std::array::from_fn::<_, 2, _>(|i| {
            delta[i].round().clamp(
                -initial.min[i] as f32,
                size[i] as f32 - initial.max[i] as f32,
            ) as i32
        });
        return Selection::rectangle(
            [initial.min[0] + delta[0], initial.min[1] + delta[1]],
            [initial.max[0] + delta[0], initial.max[1] + delta[1]],
        );
    }
    let Handle::Resize(h) = handle else {
        return initial.clone();
    };
    let mut min = initial.min.map(|v| v as f32);
    let mut max = initial.max.map(|v| v as f32);
    let center = std::array::from_fn::<_, 2, _>(|i| (min[i] + max[i]) / 2.);
    for i in 0..2 {
        if h[i] == 0 {
            min[i] = (min[i] + delta[i]).clamp(0., max[i] - 1.);
            if centered {
                max[i] = 2. * center[i] - min[i];
            }
        } else if h[i] == 2 {
            max[i] = (max[i] + delta[i]).clamp(min[i] + 1., size[i] as f32);
            if centered {
                min[i] = 2. * center[i] - max[i];
            }
        }
    }
    if centered {
        for i in 0..2 {
            let half = ((max[i] - min[i]) / 2.).min(center[i].min(size[i] as f32 - center[i]));
            min[i] = center[i] - half;
            max[i] = center[i] + half;
        }
    }
    if let Some(r) = ratio.filter(|r| r.is_finite() && *r > 0.) {
        let mut width = max[0] - min[0];
        let mut height = max[1] - min[1];
        if h[0] == 1 {
            width = height * r;
        } else if h[1] == 1 || width / height > r {
            height = width / r;
        } else {
            width = height * r;
        }
        let anchor = std::array::from_fn::<_, 2, _>(|i| {
            if centered || h[i] == 1 {
                center[i]
            } else if h[i] == 0 {
                initial.max[i] as f32
            } else {
                initial.min[i] as f32
            }
        });
        let direction = h.map(|v| if v == 0 { -1. } else { 1. });
        let available = std::array::from_fn::<_, 2, _>(|i| {
            if centered || h[i] == 1 {
                2. * anchor[i].min(size[i] as f32 - anchor[i])
            } else if direction[i] < 0. {
                anchor[i]
            } else {
                size[i] as f32 - anchor[i]
            }
        });
        let factor = (available[0] / width).min(available[1] / height).min(1.);
        width *= factor;
        height *= factor;
        for (i, length) in [width, height].into_iter().enumerate() {
            if centered || h[i] == 1 {
                min[i] = anchor[i] - length / 2.;
                max[i] = anchor[i] + length / 2.;
            } else if h[i] == 0 {
                min[i] = anchor[i] - length;
                max[i] = anchor[i];
            } else {
                min[i] = anchor[i];
                max[i] = anchor[i] + length;
            }
        }
    }
    for i in 0..2 {
        min[i] = min[i].clamp(0., size[i] as f32 - 1.);
        max[i] = max[i].clamp(min[i] + 1., size[i] as f32);
    }
    Selection::rectangle(min.map(|v| v.round() as i32), max.map(|v| v.round() as i32))
}
