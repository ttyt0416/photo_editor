use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Resize([u8; 2]),
    Rotate,
    Move,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Transform {
    pub scale: [f32; 2],
    pub angle: f32,
    pub translation: [f32; 2],
    pub warp: Option<crate::matrix::Matrix>,
    pub sampling: crate::sampling::Sampling,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            scale: [1.0, 1.0],
            angle: 0.0,
            translation: [0.0; 2],
            warp: None,
            sampling: Default::default(),
        }
    }
}
impl Transform {
    pub fn valid(self) -> bool {
        self.warp.is_none_or(|m| {
            m.iter().all(|v| v.is_finite() && v.abs() <= 1e12)
                && crate::matrix::inverse(m).is_some()
        }) && self.angle.is_finite()
            && self
                .translation
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1e9)
            && self
                .scale
                .iter()
                .all(|s| s.is_finite() && s.abs() >= 0.01 && s.abs() <= 100.0)
    }
    pub fn to_local(self, point: [f32; 2], offset: [i32; 2], size: [u32; 2]) -> [f32; 2] {
        let center = [size[0] as f32 / 2.0, size[1] as f32 / 2.0];
        let x = point[0] - offset[0] as f32 - self.translation[0] - center[0];
        let y = point[1] - offset[1] as f32 - self.translation[1] - center[1];
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let p = [
            (x * cos + y * sin) / self.scale[0] + center[0],
            (-x * sin + y * cos) / self.scale[1] + center[1],
        ];
        self.warp.map_or(p, |m| {
            crate::matrix::inverse(m).map_or([f32::NAN; 2], |m| crate::matrix::point(m, p))
        })
    }
    pub fn to_document(self, point: [f32; 2], offset: [i32; 2], size: [u32; 2]) -> [f32; 2] {
        let point = self.warp.map_or(point, |m| crate::matrix::point(m, point));
        let center = [size[0] as f32 / 2.0, size[1] as f32 / 2.0];
        let x = (point[0] - center[0]) * self.scale[0];
        let y = (point[1] - center[1]) * self.scale[1];
        let (sin, cos) = self.angle.to_radians().sin_cos();
        [
            x * cos - y * sin + center[0] + offset[0] as f32 + self.translation[0],
            x * sin + y * cos + center[1] + offset[1] as f32 + self.translation[1],
        ]
    }

    pub fn base_matrix(self, offset: [i32; 2], size: [u32; 2]) -> crate::matrix::Matrix {
        let (sin, cos) = (self.angle as f64).to_radians().sin_cos();
        let sx = self.scale[0] as f64;
        let sy = self.scale[1] as f64;
        let c = size.map(|v| v as f64 / 2.);
        [
            cos * sx,
            -sin * sy,
            offset[0] as f64 + self.translation[0] as f64 + c[0] - cos * sx * c[0]
                + sin * sy * c[1],
            sin * sx,
            cos * sy,
            offset[1] as f64 + self.translation[1] as f64 + c[1]
                - sin * sx * c[0]
                - cos * sy * c[1],
            0.,
            0.,
            1.,
        ]
    }
    pub fn mapping(self, offset: [i32; 2], size: [u32; 2]) -> crate::matrix::Matrix {
        crate::matrix::multiply(
            self.base_matrix(offset, size),
            self.warp.unwrap_or(crate::matrix::IDENTITY),
        )
    }
    pub fn valid_for(self, size: [u32; 2]) -> bool {
        if !self.valid() {
            return false;
        }
        self.warp.is_none_or(|m| {
            let w = [
                m[8],
                m[6] * size[0] as f64 + m[8],
                m[6] * size[0] as f64 + m[7] * size[1] as f64 + m[8],
                m[7] * size[1] as f64 + m[8],
            ];
            w.iter().all(|v| *v > 1e-8) || w.iter().all(|v| *v < -1e-8)
        })
    }
    pub fn world(
        mut self,
        world: crate::matrix::Matrix,
        offset: [i32; 2],
        size: [u32; 2],
    ) -> Option<Self> {
        use crate::matrix as m;
        let mapping = m::normalized(m::multiply(world, self.mapping(offset, size)));
        if m::affine(mapping) {
            let mut sx = mapping[0].hypot(mapping[3]);
            if self.scale[0] < 0. {
                sx = -sx;
            }
            let angle =
                mapping[3].atan2(mapping[0]) - if sx < 0. { std::f64::consts::PI } else { 0. };
            let sy = -angle.sin() * mapping[1] + angle.cos() * mapping[4];
            if (mapping[0] * mapping[1] + mapping[3] * mapping[4]).abs()
                < 1e-7 * (sx.abs() * sy.abs()).max(1.)
                && (0.01..=100.).contains(&sx.abs())
                && (0.01..=100.).contains(&sy.abs())
            {
                self.scale = [sx as f32, sy as f32];
                self.angle = angle.to_degrees() as f32;
                self.warp = None;
                let c = size.map(|v| v as f64 / 2.);
                self.translation = [
                    (mapping[2] - offset[0] as f64 - c[0] + mapping[0] * c[0] + mapping[1] * c[1])
                        as f32,
                    (mapping[5] - offset[1] as f64 - c[1] + mapping[3] * c[0] + mapping[4] * c[1])
                        as f32,
                ];
                return self.valid_for(size).then_some(self);
            }
        }
        self.warp = Some(m::normalized(m::multiply(
            m::inverse(self.base_matrix(offset, size))?,
            mapping,
        )));
        self.valid_for(size).then_some(self)
    }
    pub fn rebased(
        self,
        offset: [i32; 2],
        old: [u32; 2],
        new: [u32; 2],
        origin: [f32; 2],
    ) -> Option<Self> {
        let full = crate::matrix::multiply(
            self.mapping(offset, old),
            crate::matrix::translate(origin[0] as f64, origin[1] as f64),
        );
        let mut transform = self;
        transform.warp = Some(crate::matrix::normalized(crate::matrix::multiply(
            crate::matrix::inverse(self.base_matrix(offset, new))?,
            full,
        )));
        transform.world(crate::matrix::IDENTITY, offset, new)
    }
    pub fn local_bounds(
        self,
        min: [f32; 2],
        max: [f32; 2],
        offset: [i32; 2],
        size: [u32; 2],
    ) -> ([i32; 2], [i32; 2]) {
        let Some(inverse) = crate::matrix::inverse(self.mapping(offset, size)) else {
            return ([0, 0], size.map(|v| v as i32));
        };
        let corners = [min, [max[0], min[1]], max, [min[0], max[1]]];
        let w = corners.map(|p| inverse[6] * p[0] as f64 + inverse[7] * p[1] as f64 + inverse[8]);
        if !(w.iter().all(|v| *v > 1e-8) || w.iter().all(|v| *v < -1e-8)) {
            return ([0, 0], size.map(|v| v as i32));
        }
        let p = corners.map(|p| crate::matrix::point(inverse, p));
        (
            std::array::from_fn(|i| {
                p.iter()
                    .map(|p| p[i])
                    .fold(f32::INFINITY, f32::min)
                    .floor()
                    .max(0.)
                    .min(size[i] as f32) as i32
            }),
            std::array::from_fn(|i| {
                p.iter()
                    .map(|p| p[i])
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil()
                    .max(0.)
                    .min(size[i] as f32) as i32
            }),
        )
    }
    pub fn padded(mut self, pad: i32) -> Self {
        if let Some(w) = self.warp {
            self.warp = Some(crate::matrix::multiply(
                crate::matrix::translate(pad as f64, pad as f64),
                crate::matrix::multiply(w, crate::matrix::translate(-pad as f64, -pad as f64)),
            ));
        }
        self
    }
    pub fn bounds(self, offset: [i32; 2], size: [u32; 2]) -> ([i32; 2], [i32; 2]) {
        let corners = [
            [0.0, 0.0],
            [size[0] as f32, 0.0],
            [0.0, size[1] as f32],
            [size[0] as f32, size[1] as f32],
        ]
        .map(|p| self.to_document(p, offset, size));
        let min = [
            corners
                .iter()
                .map(|p| p[0])
                .fold(f32::INFINITY, f32::min)
                .floor() as i32,
            corners
                .iter()
                .map(|p| p[1])
                .fold(f32::INFINITY, f32::min)
                .floor() as i32,
        ];
        let max = [
            corners
                .iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil() as i32,
            corners
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil() as i32,
        ];
        (min, max)
    }
}

pub fn drag(
    initial: Transform,
    offset: [i32; 2],
    size: [u32; 2],
    handle: Handle,
    points: [[f32; 2]; 2],
    constrain: bool,
    centered: bool,
) -> (Transform, [i32; 2]) {
    let [start, point] = points;
    let mut delta = [point[0] - start[0], point[1] - start[1]];
    let mut result = initial;
    if handle == Handle::Move {
        if constrain {
            delta[usize::from(delta[0].abs() >= delta[1].abs())] = 0.0;
        }
        for (axis, value) in delta.iter().enumerate() {
            result.translation[axis] += value;
        }
        return (result, offset);
    }
    let center = size.map(|s| s as f32 / 2.0);
    let world_center = initial.to_document(center, offset, size);
    if handle == Handle::Rotate {
        let start_angle = (start[1] - world_center[1]).atan2(start[0] - world_center[0]);
        let end_angle = (point[1] - world_center[1]).atan2(point[0] - world_center[0]);
        result.angle = (initial.angle + (end_angle - start_angle).to_degrees()).rem_euclid(360.0);
        if constrain {
            result.angle = (result.angle / 15.0).round() * 15.0;
        }
        return (result, offset);
    }
    let Handle::Resize(handle) = handle else {
        unreachable!()
    };
    let normalized = handle.map(|v| v as f32 / 2.0);
    let anchor = normalized.map(|v| if centered { 0.5 } else { 1.0 - v });
    let anchor_local = std::array::from_fn(|axis| anchor[axis] * size[axis] as f32);
    let anchor_world = initial.to_document(anchor_local, offset, size);
    let handle_world = initial.to_document(
        std::array::from_fn(|axis| normalized[axis] * size[axis] as f32),
        offset,
        size,
    );
    let vector = [
        handle_world[0] + delta[0] - anchor_world[0],
        handle_world[1] + delta[1] - anchor_world[1],
    ];
    let (sin, cos) = initial.angle.to_radians().sin_cos();
    let rotated = [
        vector[0] * cos + vector[1] * sin,
        -vector[0] * sin + vector[1] * cos,
    ];
    for axis in 0..2 {
        if handle[axis] != 1 {
            result.scale[axis] =
                rotated[axis] / ((normalized[axis] - anchor[axis]) * size[axis] as f32);
        }
    }
    if constrain {
        let factors =
            std::array::from_fn::<_, 2, _>(|axis| result.scale[axis] / initial.scale[axis]);
        let axis = if handle[0] == 1 {
            1
        } else if handle[1] == 1 || (factors[0] - 1.0).abs() >= (factors[1] - 1.0).abs() {
            0
        } else {
            1
        };
        result.scale = initial.scale.map(|scale| scale * factors[axis]);
    }
    result.scale = result.scale.map(|scale| {
        let sign = if scale < 0.0 { -1.0 } else { 1.0 };
        sign * scale.abs().clamp(0.01, 100.0)
    });
    let relative = [
        (anchor_local[0] - center[0]) * result.scale[0],
        (anchor_local[1] - center[1]) * result.scale[1],
    ];
    let position = [
        anchor_world[0] - relative[0] * cos + relative[1] * sin - center[0],
        anchor_world[1] - relative[0] * sin - relative[1] * cos - center[1],
    ];
    let offset = position.map(|p| p.round() as i32);
    result.translation = std::array::from_fn(|axis| position[axis] - offset[axis] as f32);
    (result, offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resize_handles_anchor_rotated_flipped_layers_and_preserve_center_and_aspect() {
        let initial = Transform {
            scale: [-1.5, 0.8],
            angle: 37.0,
            ..Default::default()
        };
        let size = [80, 60];
        let offset = [11, -5];
        let start = initial.to_document([80.0, 60.0], offset, size);
        let anchor = initial.to_document([0.0, 0.0], offset, size);
        let point = [start[0] + 23.0, start[1] + 18.0];
        let (changed, moved) = drag(
            initial,
            offset,
            size,
            Handle::Resize([2, 2]),
            [start, point],
            false,
            false,
        );
        let fixed = changed.to_document([0.0, 0.0], moved, size);
        assert!((fixed[0] - anchor[0]).abs() < 1e-5);
        assert!((fixed[1] - anchor[1]).abs() < 1e-5);
        assert!(changed.valid());
        let (changed, moved) = drag(
            initial,
            offset,
            size,
            Handle::Resize([2, 2]),
            [start, point],
            true,
            true,
        );
        assert_eq!(moved, offset);
        assert!(
            (changed.scale[0] / initial.scale[0] - changed.scale[1] / initial.scale[1]).abs()
                < 1e-5
        );
        let (rotated, moved) = drag(
            Transform::default(),
            [0, 0],
            [100, 100],
            Handle::Rotate,
            [[50.0, 0.0], [100.0, 50.0]],
            true,
            false,
        );
        assert_eq!(rotated.angle, 90.0);
        assert_eq!(moved, [0, 0]);
    }
    #[test]
    fn tiny_layer_resize_keeps_subpixel_anchor_and_old_transform_json_loads() {
        let old: Transform = serde_json::from_str(r#"{"scale":[1.0,1.0],"angle":0.0}"#).unwrap();
        assert_eq!(old, Transform::default());
        let offset = [-1, 1];
        let size = [4, 2];
        let start = old.to_document([4.0, 2.0], offset, size);
        let anchor = old.to_document([0.0, 0.0], offset, size);
        let (changed, moved) = drag(
            old,
            offset,
            size,
            Handle::Resize([2, 2]),
            [start, [start[0] + 1.0, start[1] + 0.5]],
            false,
            false,
        );
        assert_eq!(changed.to_document([0.0, 0.0], moved, size), anchor);
        assert_eq!(changed.to_local(anchor, moved, size), [0.0; 2]);
        assert_eq!(changed.scale, [1.25; 2]);
        assert_eq!(changed.translation, [0.5, 0.25]);
        let json = serde_json::to_string(&changed).unwrap();
        assert_eq!(serde_json::from_str::<Transform>(&json).unwrap(), changed);
    }
}
