use crate::{
    document::{Document, Editor},
    matrix::{self as m, Matrix},
    sampling::Sampling,
    transform::{Handle, Transform},
};
#[derive(Clone)]
struct Original {
    id: u64,
    transform: Transform,
    offset: [i32; 2],
    size: [u32; 2],
    group: bool,
    independent_mask: bool,
    scope: Option<crate::adjustment_scope::Scope>,
}
#[derive(Clone)]
pub struct Frame {
    pub roots: Vec<u64>,
    pub mask: Option<u64>,
    pub quad: [[f32; 2]; 4],
    pub distort: bool,
    pub sampling: Sampling,
    originals: Vec<Original>,
}
impl Frame {
    pub fn new(doc: &Document, roots: Vec<u64>) -> Result<Self, String> {
        if roots.is_empty() || !doc.movable(&roots) {
            return Err("잠기지 않은 레이어·그룹을 선택하세요.".into());
        }
        let (min, max) =
            crate::guides::bounds(doc, &roots).ok_or("빈 그룹에는 변형할 내용이 없습니다.")?;
        let quad = if roots.len() == 1
            && let Some(l) = doc
                .layers
                .iter()
                .find(|l| l.id == roots[0] && !l.canvas_space())
        {
            let size = [l.pixels.width(), l.pixels.height()];
            [
                [0., 0.],
                [size[0] as f32, 0.],
                size.map(|s| s as f32),
                [0., size[1] as f32],
            ]
            .map(|p| l.transform.to_document(p, l.offset, size))
        } else {
            [min, [max[0], min[1]], max, [min[0], max[1]]]
        };
        let mut frame = Self {
            roots,
            mask: None,
            quad,
            distort: false,
            sampling: Sampling::Bilinear,
            originals: vec![],
        };
        frame.capture(doc);
        if frame.originals.len() == 1 && frame.originals[0].transform != Default::default() {
            frame.sampling = frame.originals[0].transform.sampling;
        }
        Ok(frame)
    }
    pub fn new_mask(doc: &Document, id: u64) -> Result<Self, String> {
        if doc.is_locked(id) {
            return Err("잠긴 마스크는 변형할 수 없습니다.".into());
        }
        let layer = doc
            .layers
            .iter()
            .find(|l| l.id == id)
            .ok_or("레이어가 없습니다.")?;
        let (_, _, size) = crate::masks::geometry(layer).ok_or("변형할 마스크가 없습니다.")?;
        let quad = [
            [0., 0.],
            [size[0] as f32, 0.],
            size.map(|v| v as f32),
            [0., size[1] as f32],
        ]
        .map(|p| m::point(crate::masks::world(layer), p));
        let mut frame = Self {
            roots: vec![id],
            mask: Some(id),
            quad,
            distort: false,
            sampling: crate::masks::sampling(layer),
            originals: vec![],
        };
        frame.capture(doc);
        Ok(frame)
    }
    pub fn capture(&mut self, doc: &Document) {
        if let Some(id) = self.mask {
            self.originals = doc
                .layers
                .iter()
                .find(|l| l.id == id)
                .and_then(crate::masks::geometry)
                .map(|(transform, offset, size)| Original {
                    id,
                    transform,
                    offset,
                    size,
                    group: false,
                    independent_mask: false,
                    scope: None,
                })
                .into_iter()
                .collect();
            return;
        }
        self.originals = doc
            .layers
            .iter()
            .filter(|l| self.roots.iter().any(|id| doc.descendant_of(l.id, *id)))
            .map(|l| Original {
                id: l.id,
                transform: l.transform,
                offset: l.offset,
                size: if l.canvas_space() {
                    l.mask.as_ref().map_or([doc.width, doc.height], |mask| {
                        [mask.width(), mask.height()]
                    })
                } else {
                    [l.pixels.width(), l.pixels.height()]
                },
                group: l.canvas_space(),
                independent_mask: l.mask_placement.is_some(),
                scope: l.adjustments.scope.clone(),
            })
            .collect();
    }
    pub fn legacy(&self) -> bool {
        self.mask.is_none()
            && !self.distort
            && self.originals.len() == 1
            && !self.originals[0].group
            && !self.originals[0].independent_mask
            && self.originals[0].transform.warp.is_none()
    }
    pub fn handles(&self) -> Vec<(Handle, [f32; 2])> {
        let Some(map) = m::unit_quad(self.quad) else {
            return vec![];
        };
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
        .map(|h| (Handle::Resize(h), m::point(map, h.map(|v| v as f32 / 2.))))
        .collect()
    }
    pub fn center(&self) -> [f32; 2] {
        m::unit_quad(self.quad).map_or([0.; 2], |m| m::point(m, [0.5; 2]))
    }
    pub fn drag(
        &self,
        handle: Handle,
        start: [f32; 2],
        point: [f32; 2],
        shift: bool,
        alt: bool,
    ) -> Result<[[f32; 2]; 4], String> {
        let mut delta = [point[0] - start[0], point[1] - start[1]];
        if shift && (handle == Handle::Move || self.distort) {
            delta[usize::from(delta[0].abs() >= delta[1].abs())] = 0.;
        }
        if handle == Handle::Move {
            return Ok(self.quad.map(|p| [p[0] + delta[0], p[1] + delta[1]]));
        }
        let center = self.center();
        if handle == Handle::Rotate {
            let mut angle = ((point[1] - center[1]).atan2(point[0] - center[0])
                - (start[1] - center[1]).atan2(start[0] - center[0]))
            .to_degrees();
            if shift {
                let current = (self.quad[1][1] - self.quad[0][1])
                    .atan2(self.quad[1][0] - self.quad[0][0])
                    .to_degrees();
                angle = ((current + angle) / 15.).round() * 15. - current;
            }
            return Ok(self.mapped(rotation(center, angle)));
        }
        let Handle::Resize(h) = handle else {
            unreachable!()
        };
        if self.distort {
            let mut quad = self.quad;
            for (i, corner) in [[0, 0], [2, 0], [2, 2], [0, 2]].iter().enumerate() {
                if (h[0] == 1 || h[0] == corner[0]) && (h[1] == 1 || h[1] == corner[1]) {
                    quad[i] = [quad[i][0] + delta[0], quad[i][1] + delta[1]];
                } else if alt
                    && (h[0] == 1 || 2 - h[0] == corner[0])
                    && (h[1] == 1 || 2 - h[1] == corner[1])
                {
                    quad[i] = [quad[i][0] - delta[0], quad[i][1] - delta[1]];
                }
            }
            return m::unit_quad(quad)
                .map(|_| quad)
                .ok_or("모서리가 교차하거나 면적이 없는 왜곡은 적용할 수 없습니다.".into());
        }
        let map = m::unit_quad(self.quad).ok_or("변형 상자를 확인하세요.")?;
        let normalized = h.map(|v| v as f32 / 2.);
        let anchor = normalized.map(|v| if alt { 0.5 } else { 1. - v });
        let p = m::point(map, normalized);
        let local = m::point(
            m::inverse(map).ok_or("변형 역행렬이 없습니다.")?,
            [p[0] + delta[0], p[1] + delta[1]],
        );
        let mut factors = std::array::from_fn::<_, 2, _>(|i| {
            if h[i] == 1 {
                1.
            } else {
                (local[i] - anchor[i]) / (normalized[i] - anchor[i])
            }
        });
        if shift {
            let axis = if h[0] == 1 {
                1
            } else if h[1] == 1 || (factors[0] - 1.).abs() >= (factors[1] - 1.).abs() {
                0
            } else {
                1
            };
            factors = [factors[axis]; 2];
        }
        factors = factors.map(|v| {
            if v < 0. {
                -v.abs().clamp(0.01, 100.)
            } else {
                v.abs().clamp(0.01, 100.)
            }
        });
        let scale = [
            factors[0] as f64,
            0.,
            anchor[0] as f64 * (1. - factors[0]) as f64,
            0.,
            factors[1] as f64,
            anchor[1] as f64 * (1. - factors[1]) as f64,
            0.,
            0.,
            1.,
        ];
        let world = m::multiply(map, m::multiply(scale, m::inverse(map).unwrap()));
        let quad = self.mapped(world);
        m::unit_quad(quad)
            .map(|_| quad)
            .ok_or("변형 상자가 유효하지 않습니다.".into())
    }
    pub fn mapped(&self, mapping: Matrix) -> [[f32; 2]; 4] {
        self.quad.map(|p| m::point(mapping, p))
    }
    pub fn apply(&self, editor: &mut Editor, quad: [[f32; 2]; 4]) -> Result<bool, String> {
        if self.mask.map_or_else(
            || !editor.document.movable(&self.roots),
            |id| editor.document.is_locked(id),
        ) {
            return Err("잠긴 레이어·그룹은 변형할 수 없습니다.".into());
        }
        let world = m::multiply(
            m::unit_quad(quad).ok_or("왜곡 모서리를 확인하세요.")?,
            m::inverse(m::unit_quad(self.quad).ok_or("변형 상자를 확인하세요.")?)
                .ok_or("변형 역행렬이 없습니다.")?,
        );
        if let Some(id) = self.mask {
            let original = self
                .originals
                .first()
                .ok_or("마스크 변형 원본이 없습니다.")?;
            let matrix = m::multiply(
                world,
                original.transform.mapping(original.offset, original.size),
            );
            let layer = editor
                .document
                .layers
                .iter()
                .find(|l| l.id == id)
                .ok_or("마스크 레이어가 없습니다.")?;
            if !crate::masks::valid_mapping(matrix, original.size) {
                return Err("마스크 투영 한계를 넘습니다.".into());
            }
            let old = crate::masks::world(layer);
            if old.iter().zip(matrix).all(|(a, b)| (*a - b).abs() < 1e-7)
                && crate::masks::sampling(layer) == self.sampling
            {
                return Ok(false);
            }
            editor.begin("마스크 변형");
            crate::masks::set_world(
                editor
                    .document
                    .layers
                    .iter_mut()
                    .find(|l| l.id == id)
                    .unwrap(),
                matrix,
                false,
                self.sampling,
            )?;
            return Ok(true);
        }
        let mut updates = vec![];
        for original in &self.originals {
            let mut transform = original
                .transform
                .world(world, original.offset, original.size)
                .ok_or("변형이 원본의 투영 한계를 넘습니다. 모서리를 조절하세요.")?;
            transform.sampling = self.sampling;
            let layer = editor
                .document
                .layers
                .iter()
                .find(|l| l.id == original.id)
                .ok_or("변형 레이어가 없습니다.")?;
            if !original.group && !transform.valid_for(original.size) {
                return Err("원본 영역을 가로지르는 투영은 적용할 수 없습니다.".into());
            }
            let mut candidate = layer.clone();
            candidate.transform = transform;
            candidate.offset = original.offset;
            if !crate::filter_bounds::valid(&candidate) {
                return Err("필터·효과의 외곽 영역이 크기 또는 원근 투영 한계를 넘습니다.".into());
            }
            if original.group {
                candidate.adjustments.scope = original.scope.clone();
                if let Some(scope) = &mut candidate.adjustments.scope {
                    scope.global(world)?;
                }
            }
            if !crate::masks::valid(&candidate) {
                return Err("연결된 마스크의 투영 한계를 넘습니다.".into());
            }
            if layer.transform != transform || layer.offset != original.offset {
                updates.push((
                    original.id,
                    transform,
                    original.offset,
                    candidate.adjustments.scope,
                ));
            }
        }
        if updates.is_empty() {
            return Ok(false);
        }
        editor.begin("캔버스 변형");
        for (id, transform, offset, scope) in updates {
            let l = editor
                .document
                .layers
                .iter_mut()
                .find(|l| l.id == id)
                .unwrap();
            l.transform = transform;
            l.offset = offset;
            l.adjustments.scope = scope;
        }
        Ok(true)
    }
}
pub fn rotation(center: [f32; 2], angle: f32) -> Matrix {
    let (sin, cos) = (angle as f64).to_radians().sin_cos();
    let x = center[0] as f64;
    let y = center[1] as f64;
    [
        cos,
        -sin,
        x - cos * x + sin * y,
        sin,
        cos,
        y - sin * x - cos * y,
        0.,
        0.,
        1.,
    ]
}
