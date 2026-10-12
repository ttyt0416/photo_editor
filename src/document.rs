use crate::{
    adjustments::Adjustments,
    blend::{self, BlendMode},
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

pub const MAX_PIXELS: u64 = 100_000_000;
const HISTORY_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRegion {
    pub min: [u32; 2],
    pub max: [u32; 2],
}

impl PixelRegion {
    pub fn size(self) -> [u32; 2] {
        std::array::from_fn(|i| self.max[i] - self.min[i])
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i].min(other.min[i])),
            max: std::array::from_fn(|i| self.max[i].max(other.max[i])),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub min: [i32; 2],
    pub max: [i32; 2],
    pub mask: Option<Arc<image::GrayImage>>,
    pub inverted: bool,
}

impl Selection {
    pub fn rectangle(min: [i32; 2], max: [i32; 2]) -> Self {
        Self {
            min,
            max,
            mask: None,
            inverted: false,
        }
    }
    pub fn coverage(&self, x: i32, y: i32) -> f32 {
        let inside = x >= self.min[0] && y >= self.min[1] && x < self.max[0] && y < self.max[1];
        let value = if inside {
            self.mask.as_ref().map_or(1.0, |mask| {
                mask.get_pixel((x - self.min[0]) as u32, (y - self.min[1]) as u32)[0] as f32 / 255.0
            })
        } else {
            0.0
        };
        if self.inverted { 1.0 - value } else { value }
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.coverage(x, y) > 0.0
    }
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub id: u64,
    pub name: String,
    pub pixels: Arc<RgbaImage>,
    pub offset: [i32; 2],
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    pub adjustments: Adjustments,
    pub mask: Option<Arc<image::GrayImage>>,
    pub mask_enabled: bool,
    pub mask_placement: Option<crate::masks::Placement>,
    pub content: Option<crate::vector::Content>,
    pub transform: crate::transform::Transform,
    pub precision: Option<Arc<image::Rgba32FImage>>,
    pub bit_depth: u8,
    pub effects: crate::effects::Effects,
    pub parent: Option<u64>,
    pub group: bool,
    pub adjustment_layer: bool,
    pub pass_through: bool,
    pub clipped: bool,
    pub psd_extra: Arc<Vec<u8>>,
    pub raw_source: Option<Arc<crate::raw_input::Source>>,
}

impl Layer {
    pub fn canvas_space(&self) -> bool {
        self.group || self.adjustment_layer
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub document_id: String,
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub active: u64,
    pub next_id: u64,
    pub psd_resources: Arc<Vec<u8>>,
    pub source_icc: Option<Arc<Vec<u8>>>,
    pub source_metadata: crate::source_metadata::Metadata,
    pub layout: crate::guides::Layout,
}

pub fn validate_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("이미지 크기가 잘못되었거나 현재 메모리 예산을 초과합니다.".into());
    }
    Ok(())
}

impl Document {
    pub fn new(width: u32, height: u32, transparent: bool) -> Result<Self, String> {
        validate_size(width, height)?;
        let color = if transparent { [0, 0, 0, 0] } else { [255; 4] };
        Ok(Self::from_image(
            "배경".into(),
            RgbaImage::from_pixel(width, height, Rgba(color)),
        ))
    }

    pub fn from_image(name: String, pixels: RgbaImage) -> Self {
        Self {
            document_id: crate::document_id::new(),
            width: pixels.width(),
            height: pixels.height(),
            layers: vec![Layer {
                id: 1,
                name,
                pixels: Arc::new(pixels),
                offset: [0, 0],
                visible: true,
                locked: false,
                opacity: 1.0,
                blend: BlendMode::Normal,
                adjustments: Adjustments::default(),
                mask: None,
                mask_enabled: true,
                mask_placement: None,
                content: None,
                transform: Default::default(),
                precision: None,
                bit_depth: 8,
                effects: Default::default(),
                parent: None,
                group: false,
                adjustment_layer: false,
                pass_through: true,
                clipped: false,
                psd_extra: Arc::new(Vec::new()),
                raw_source: None,
            }],
            active: 1,
            next_id: 2,
            psd_resources: Arc::new(Vec::new()),
            source_icc: None,
            source_metadata: Default::default(),
            layout: Default::default(),
        }
    }

    pub fn active_layer(&self) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == self.active)
    }

    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.id == self.active)
    }

    pub fn is_locked(&self, id: u64) -> bool {
        let mut current = Some(id);
        for _ in 0..64 {
            let Some(id) = current else {
                return false;
            };
            let Some(layer) = self.layers.iter().find(|l| l.id == id) else {
                return true;
            };
            if layer.locked {
                return true;
            }
            current = layer.parent;
        }
        true
    }

    pub fn add_blank_layer(&mut self, name: String) -> Result<(), String> {
        let depth = self.layers.iter().map(|l| l.bit_depth).max().unwrap_or(8);
        self.add_layer(name, RgbaImage::new(self.width, self.height))?;
        if depth > 8 {
            let layer = self.active_layer_mut().unwrap();
            layer.precision = Some(Arc::new(image::Rgba32FImage::new(
                layer.pixels.width(),
                layer.pixels.height(),
            )));
            layer.bit_depth = depth;
        }
        Ok(())
    }

    pub fn add_layer(&mut self, name: String, pixels: RgbaImage) -> Result<(), String> {
        let pixels_total =
            self.pixel_count() + u64::from(pixels.width()) * u64::from(pixels.height());
        if pixels_total > MAX_PIXELS {
            return Err("레이어가 현재 문서의 메모리 예산을 초과합니다.".into());
        }
        let id = self.next_id;
        if id == 0 || id == u64::MAX {
            return Err("새 레이어 ID를 만들 수 없습니다.".into());
        }
        self.next_id += 1;
        let parent = self
            .active_layer()
            .and_then(|l| if l.group { Some(l.id) } else { l.parent });
        self.layers.push(Layer {
            id,
            name,
            pixels: Arc::new(pixels),
            offset: [0, 0],
            visible: true,
            locked: false,
            opacity: 1.0,
            blend: BlendMode::Normal,
            adjustments: Adjustments::default(),
            mask: None,
            mask_enabled: true,
            mask_placement: None,
            content: None,
            transform: Default::default(),
            precision: None,
            bit_depth: 8,
            effects: Default::default(),
            parent,
            group: false,
            adjustment_layer: false,
            pass_through: true,
            clipped: false,
            psd_extra: Arc::new(Vec::new()),
            raw_source: None,
        });
        self.active = id;
        Ok(())
    }

    pub fn add_dynamic(&mut self, name: String, image: image::DynamicImage) -> Result<(), String> {
        let source = crate::precision::document(name.clone(), image)?;
        let layer = source.layers.into_iter().next().unwrap();
        self.add_layer(name, (*layer.pixels).clone())?;
        let active = self.active_layer_mut().unwrap();
        active.precision = layer.precision;
        active.bit_depth = layer.bit_depth;
        Ok(())
    }

    pub fn pixel_count(&self) -> u64 {
        self.layers
            .iter()
            .map(|l| u64::from(l.pixels.width()) * u64::from(l.pixels.height()))
            .sum()
    }

    pub fn import_layers(&mut self, source: Document) -> Result<(), String> {
        if !source.hierarchy_valid() || source.active_layer().is_none() {
            return Err("가져올 레이어 구조가 잘못되었습니다.".into());
        }
        if self.pixel_count() + source.pixel_count() > MAX_PIXELS {
            return Err("가져오기가 문서 메모리 예산을 초과합니다.".into());
        }
        let parent = self
            .active_layer()
            .and_then(|l| if l.group { Some(l.id) } else { l.parent });
        if parent.is_some_and(|id| self.is_locked(id)) {
            return Err("잠긴 그룹으로 가져올 수 없습니다.".into());
        }
        self.next_id
            .checked_add(source.layers.len() as u64)
            .ok_or("새 레이어 ID를 만들 수 없습니다.")?;
        let mut candidate = self.clone();
        let mut ids = std::collections::HashMap::new();
        for l in &source.layers {
            ids.insert(l.id, candidate.next_id);
            candidate.next_id += 1;
        }
        candidate.active = ids[&source.active];
        for mut l in source.layers {
            l.id = ids[&l.id];
            l.parent = l.parent.map(|id| ids[&id]).or(parent);
            candidate.layers.push(l);
        }
        if !candidate.hierarchy_valid() {
            return Err("가져오기가 그룹 중첩 깊이 한도를 초과합니다.".into());
        }
        *self = candidate;
        Ok(())
    }

    pub fn composite(&self) -> RgbaImage {
        self.composite_region(PixelRegion {
            min: [0; 2],
            max: [self.width, self.height],
        })
    }

    // Spatial effects need pixels outside the brush bounds and remain on the worker.
    pub fn local_preview(&self) -> bool {
        self.layers.iter().all(|l| {
            !l.effects.enabled() && l.adjustments.local() && (!l.group || !l.adjustments.active())
        })
    }

    pub fn composite_region(&self, region: PixelRegion) -> RgbaImage {
        if self.layers.iter().any(|l| {
            l.mask_placement.is_some()
                || l.precision.is_some()
                || l.effects.enabled()
                || l.canvas_space()
                || l.clipped
                || l.blend == BlendMode::Dissolve
                || l.transform.warp.is_some()
                || l.transform.sampling != crate::sampling::Sampling::Nearest
        }) {
            return crate::precision::preview(&crate::precision::composite_region(self, region));
        }
        let [width, height] = region.size();
        let mut output = RgbaImage::new(width, height);
        for layer in self.layers.iter().filter(|l| l.visible && l.opacity > 0.0) {
            let adjusted = (layer.adjustments.active() && !layer.adjustments.local())
                .then(|| layer.adjustments.apply(&layer.pixels));
            let pixels = adjusted.as_ref().unwrap_or(&layer.pixels);
            let sample_adjustment = adjusted.is_none() && layer.adjustments.active();
            let size = [pixels.width(), pixels.height()];
            let (min, max) = layer.transform.bounds(layer.offset, size);
            let x0 = min[0].max(region.min[0] as i32);
            let y0 = min[1].max(region.min[1] as i32);
            let x1 = max[0].min(region.max[0] as i32);
            let y1 = max[1].min(region.max[1] as i32);
            for y in y0..y1 {
                for x in x0..x1 {
                    let local = layer.transform.to_local(
                        [x as f32 + 0.5, y as f32 + 0.5],
                        layer.offset,
                        size,
                    );
                    if local[0] < 0.0
                        || local[1] < 0.0
                        || local[0] >= size[0] as f32
                        || local[1] >= size[1] as f32
                    {
                        continue;
                    }
                    let (mx, my) = (local[0].floor() as u32, local[1].floor() as u32);
                    let mut source = pixels.get_pixel(mx, my).0;
                    if sample_adjustment {
                        source = layer
                            .adjustments
                            .sample(source.map(|v| v as f32 / 255.0), [mx, my], size)
                            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                    }
                    if layer.mask_enabled
                        && let Some(mask) = &layer.mask
                    {
                        if mx < mask.width() && my < mask.height() {
                            source[3] = ((u16::from(source[3])
                                * u16::from(mask.get_pixel(mx, my)[0])
                                + 127)
                                / 255) as u8;
                        } else {
                            source[3] = 0;
                        }
                    }
                    let target =
                        output.get_pixel_mut(x as u32 - region.min[0], y as u32 - region.min[1]);
                    target.0 = blend::composite(source, target.0, layer.opacity, layer.blend);
                }
            }
        }
        output
    }

    pub fn descendant_of(&self, id: u64, ancestor: u64) -> bool {
        let mut current = Some(id);
        for _ in 0..64 {
            if current == Some(ancestor) {
                return true;
            }
            current = current
                .and_then(|id| self.layers.iter().find(|l| l.id == id))
                .and_then(|l| l.parent);
            if current.is_none() {
                return false;
            }
        }
        false
    }
    pub fn hierarchy_valid(&self) -> bool {
        self.layers.iter().all(|layer| {
            let mut parent = layer.parent;
            for _ in 0..64 {
                let Some(id) = parent else {
                    return true;
                };
                if id == layer.id {
                    return false;
                }
                let Some(group) = self.layers.iter().find(|l| l.id == id && l.group) else {
                    return false;
                };
                parent = group.parent;
            }
            false
        })
    }
    pub fn displayed_layers(&self) -> Vec<(u64, usize)> {
        fn add(doc: &Document, parent: Option<u64>, depth: usize, out: &mut Vec<(u64, usize)>) {
            if depth >= 64 {
                return;
            }
            for l in doc.layers.iter().rev().filter(|l| l.parent == parent) {
                out.push((l.id, depth));
                if l.group {
                    add(doc, Some(l.id), depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        add(self, None, 0, &mut out);
        out
    }
    pub fn selected_roots(&self, ids: &[u64]) -> Vec<u64> {
        self.layers
            .iter()
            .filter(|l| {
                ids.contains(&l.id)
                    && !ids
                        .iter()
                        .any(|id| *id != l.id && self.descendant_of(l.id, *id))
            })
            .map(|l| l.id)
            .collect()
    }
    pub fn movable(&self, ids: &[u64]) -> bool {
        let roots = self.selected_roots(ids);
        !roots.is_empty()
            && !self.layers.iter().any(|l| {
                roots.iter().any(|id| self.descendant_of(l.id, *id)) && self.is_locked(l.id)
            })
    }
    pub fn selected_document(&self, ids: &[u64]) -> Result<Self, String> {
        let roots = self.selected_roots(ids);
        if roots.is_empty() {
            return Err("레이어를 선택하세요.".into());
        }
        let mut doc = self.clone();
        doc.layers
            .retain(|l| roots.iter().any(|id| self.descendant_of(l.id, *id)));
        for l in &mut doc.layers {
            if roots.contains(&l.id) {
                l.parent = None;
            }
        }
        doc.active = roots[0];
        Ok(doc)
    }
    pub fn duplicate_selected(&mut self, ids: &[u64]) -> Result<Vec<u64>, String> {
        let roots = self.selected_roots(ids);
        let selected = self.selected_document(&roots)?;
        if self.pixel_count() + selected.pixel_count() > MAX_PIXELS {
            return Err("복제가 문서 메모리 예산을 초과합니다.".into());
        }
        let mut copies = Vec::new();
        for id in roots {
            self.active = id;
            self.duplicate_active()?;
            copies.push(self.active);
        }
        Ok(copies)
    }
    pub fn delete_selected(&mut self, ids: &[u64]) {
        let depth = self.layers.iter().map(|l| l.bit_depth).max().unwrap_or(8);
        let roots = self.selected_roots(ids);
        if roots.len() == 1 {
            self.active = roots[0];
            self.delete_active();
            return;
        }
        let removed: Vec<u64> = self
            .layers
            .iter()
            .filter(|l| roots.iter().any(|id| self.descendant_of(l.id, *id)))
            .map(|l| l.id)
            .collect();
        self.layers.retain(|l| !removed.contains(&l.id));
        if self.layers.is_empty() {
            self.add_empty(depth);
        } else if removed.contains(&self.active) {
            self.active = self.layers.last().unwrap().id;
        }
    }
    pub fn group_selected(&mut self, ids: &[u64]) -> Result<(), String> {
        let roots = self.selected_roots(ids);
        if roots.is_empty() {
            return Err("레이어를 선택하세요.".into());
        }
        if !self.movable(&roots) {
            return Err("잠긴 레이어는 그룹으로 묶을 수 없습니다.".into());
        }
        let mut candidate = self.clone();
        let new_id = self.next_id;
        for layer in candidate
            .layers
            .iter_mut()
            .filter(|l| roots.contains(&l.id))
        {
            layer.parent = Some(new_id);
        }
        let mut folder = self
            .layers
            .iter()
            .find(|l| l.id == roots[0])
            .unwrap()
            .clone();
        folder.id = new_id;
        folder.group = true;
        candidate.layers.push(folder);
        if !candidate.hierarchy_valid() {
            return Err("그룹 중첩 깊이가 현재 한도를 초과합니다.".into());
        }
        if roots.len() == 1 {
            self.active = roots[0];
            return self.group_active();
        }
        let parent = self
            .layers
            .iter()
            .find(|l| l.id == roots[0])
            .unwrap()
            .parent;
        if roots
            .iter()
            .any(|id| self.layers.iter().find(|l| l.id == *id).unwrap().parent != parent)
        {
            return Err("같은 그룹에 속한 레이어를 선택해 묶으세요.".into());
        }
        let index = self
            .layers
            .iter()
            .position(|l| roots.contains(&l.id))
            .unwrap();
        self.active = roots[0];
        self.add_layer("새 그룹".into(), RgbaImage::new(1, 1))?;
        let group = self.active;
        let folder = self.active_layer_mut().unwrap();
        folder.group = true;
        folder.parent = parent;
        for l in self.layers.iter_mut().filter(|l| roots.contains(&l.id)) {
            l.parent = Some(group);
        }
        let folder = self.layers.pop().unwrap();
        self.layers.insert(index, folder);
        Ok(())
    }
    pub fn reorder_selected(
        &mut self,
        ids: &[u64],
        target: u64,
        into_group: bool,
    ) -> Result<(), String> {
        let roots = self.selected_roots(ids);
        if roots.is_empty() {
            return Err("레이어를 선택하세요.".into());
        }
        let target_layer = self
            .layers
            .iter()
            .find(|l| l.id == target)
            .ok_or("대상 레이어가 없습니다.")?;
        if roots.iter().any(|id| self.descendant_of(target, *id)) {
            return Err("자기 자신이나 자식 그룹으로 이동할 수 없습니다.".into());
        }
        let parent = if into_group && target_layer.group {
            Some(target)
        } else {
            target_layer.parent
        };
        if !self.movable(&roots) || parent.is_some_and(|id| self.is_locked(id)) {
            return Err("잠긴 레이어 또는 그룹은 이동할 수 없습니다.".into());
        }
        let mut candidate = self.clone();
        for layer in candidate
            .layers
            .iter_mut()
            .filter(|l| roots.contains(&l.id))
        {
            layer.parent = parent;
        }
        if !candidate.hierarchy_valid() {
            return Err("그룹 중첩 깊이가 현재 한도를 초과합니다.".into());
        }
        let moved: Vec<u64> = self
            .layers
            .iter()
            .filter(|l| roots.iter().any(|id| self.descendant_of(l.id, *id)))
            .map(|l| l.id)
            .collect();
        let mut remaining = Vec::new();
        let mut moving = Vec::new();
        for mut l in self.layers.drain(..) {
            if roots.contains(&l.id) {
                l.parent = parent;
            }
            if moved.contains(&l.id) {
                moving.push(l);
            } else {
                remaining.push(l);
            }
        }
        let index = remaining.iter().position(|l| l.id == target).unwrap() + 1;
        remaining.splice(index..index, moving);
        self.layers = remaining;
        Ok(())
    }
    pub fn group_active(&mut self) -> Result<(), String> {
        let active = self.active;
        let parent = self.active_layer().and_then(|l| l.parent);
        let index = self.layers.iter().position(|l| l.id == active).unwrap();
        self.add_layer("새 그룹".into(), RgbaImage::new(1, 1))?;
        let group = self.active;
        let layer = self.active_layer_mut().unwrap();
        layer.group = true;
        layer.parent = parent;
        self.layers
            .iter_mut()
            .find(|l| l.id == active)
            .unwrap()
            .parent = Some(group);
        let folder = self.layers.pop().unwrap();
        self.layers.insert(index, folder);
        Ok(())
    }
    pub fn duplicate_active(&mut self) -> Result<(), String> {
        let active = self.active;
        let originals: Vec<Layer> = self
            .layers
            .iter()
            .filter(|l| self.descendant_of(l.id, active))
            .cloned()
            .collect();
        let extra: u64 = originals
            .iter()
            .map(|l| l.pixels.width() as u64 * l.pixels.height() as u64)
            .sum();
        if self.pixel_count() + extra > MAX_PIXELS {
            return Err("복제가 문서 메모리 예산을 초과합니다.".into());
        }
        let mut ids = std::collections::HashMap::new();
        for l in &originals {
            ids.insert(l.id, self.next_id);
            self.next_id += 1;
        }
        for mut layer in originals {
            let old = layer.id;
            layer.id = ids[&old];
            layer.parent = layer.parent.map(|id| ids.get(&id).copied().unwrap_or(id));
            if old == active {
                layer.name += " 복사";
            }
            self.layers.push(layer);
        }
        self.active = ids[&active];
        Ok(())
    }
    pub fn delete_active(&mut self) {
        let depth = self.layers.iter().map(|l| l.bit_depth).max().unwrap_or(8);
        let active = self.active;
        let ids: Vec<u64> = self
            .layers
            .iter()
            .filter(|l| self.descendant_of(l.id, active))
            .map(|l| l.id)
            .collect();
        self.layers.retain(|l| !ids.contains(&l.id));
        if self.layers.is_empty() {
            self.add_empty(depth);
        } else {
            self.active = self.layers.last().unwrap().id;
        }
    }
    fn add_empty(&mut self, depth: u8) {
        self.active = 0;
        let _ = self.add_layer("빈 레이어".into(), RgbaImage::new(1, 1));
        if depth > 8
            && let Some(layer) = self.active_layer_mut()
        {
            layer.bit_depth = depth;
            layer.precision = Some(Arc::new(image::Rgba32FImage::new(1, 1)));
        }
    }

    pub fn move_layer(&mut self, id: u64, offset: [i32; 2]) {
        let Some(layer) = self.layers.iter().find(|l| l.id == id) else {
            return;
        };
        if self.is_locked(id)
            || (layer.group
                && self
                    .layers
                    .iter()
                    .any(|l| l.locked && self.descendant_of(l.id, id)))
        {
            return;
        }
        let delta = [
            offset[0].saturating_sub(layer.offset[0]),
            offset[1].saturating_sub(layer.offset[1]),
        ];
        let ids: Vec<u64> = self
            .layers
            .iter()
            .filter(|l| self.descendant_of(l.id, id))
            .map(|l| l.id)
            .collect();
        for l in self.layers.iter_mut().filter(|l| ids.contains(&l.id)) {
            if l.canvas_space()
                && let Some(scope) = &mut l.adjustments.scope
            {
                scope.rebase(crate::matrix::translate(-delta[0] as f64, -delta[1] as f64));
            }
            l.offset = [
                l.offset[0].saturating_add(delta[0]),
                l.offset[1].saturating_add(delta[1]),
            ];
        }
    }
}

/// Straight-alpha source-over in the current 8-bit sRGB working space.
pub fn over(source: [u8; 4], target: [u8; 4], opacity: f32) -> [u8; 4] {
    let sa = f32::from(source[3]) / 255.0 * opacity;
    let da = f32::from(target[3]) / 255.0;
    let alpha = sa + da * (1.0 - sa);
    if alpha <= 0.0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    for c in 0..3 {
        out[c] = ((f32::from(source[c]) * sa + f32::from(target[c]) * da * (1.0 - sa)) / alpha)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    out[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    out
}

#[derive(Clone)]
struct Edit {
    label: String,
    before: Document,
    after: Document,
    before_revision: u64,
    after_revision: u64,
}

pub struct Editor {
    pub document: Document,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    pending: Option<(String, Document, u64)>,
    pub revision: u64,
    pub saved_revision: Option<u64>,
    next_revision: u64,
}

impl Editor {
    pub fn new(document: Document, saved: bool) -> Self {
        Self {
            document,
            undo: vec![],
            redo: vec![],
            pending: None,
            revision: 0,
            saved_revision: saved.then_some(0),
            next_revision: 1,
        }
    }

    pub fn dirty(&self) -> bool {
        self.pending.is_some() || self.saved_revision != Some(self.revision)
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn is_editing(&self) -> bool {
        self.pending.is_some()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn history(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.undo.iter().map(|edit| edit.label.as_str())
    }

    pub fn begin(&mut self, label: &str) {
        if self.pending.is_none() {
            self.pending = Some((label.into(), self.document.clone(), self.revision));
        }
    }

    pub fn finish(&mut self) {
        if let Some((label, before, before_revision)) = self.pending.take() {
            let after_revision = self.next_revision;
            self.next_revision += 1;
            self.revision = after_revision;
            self.redo.clear();
            self.undo.push(Edit {
                label,
                before,
                after: self.document.clone(),
                before_revision,
                after_revision,
            });
            // Shared pixel assets make metadata history inexpensive. Bound retained unique buffers.
            while self.undo.len() > 1
                && (self.undo.len() > 100 || self.history_bytes() > HISTORY_BYTES)
            {
                self.undo.remove(0);
            }
        }
    }

    pub fn cancel(&mut self) {
        if let Some((_, before, revision)) = self.pending.take() {
            self.document = before;
            self.revision = revision;
        }
    }

    pub fn edit(&mut self, label: &str, apply: impl FnOnce(&mut Document)) {
        self.finish();
        self.begin(label);
        apply(&mut self.document);
        self.finish();
    }

    pub fn undo(&mut self) {
        self.finish();
        if let Some(edit) = self.undo.pop() {
            self.document = edit.before.clone();
            self.revision = edit.before_revision;
            self.redo.push(edit);
        }
    }

    pub fn redo(&mut self) {
        if let Some(edit) = self.redo.pop() {
            self.document = edit.after.clone();
            self.revision = edit.after_revision;
            self.undo.push(edit);
        }
    }

    fn history_bytes(&self) -> u64 {
        let mut buffers = std::collections::HashMap::new();
        for edit in &self.undo {
            for doc in [&edit.before, &edit.after] {
                for (_, asset) in doc.source_metadata.assets() {
                    buffers
                        .entry(Arc::as_ptr(asset) as usize)
                        .or_insert(asset.len() as u64);
                }
                if let Some(profile) = &doc.source_icc {
                    buffers
                        .entry(Arc::as_ptr(profile) as usize)
                        .or_insert(profile.len() as u64);
                }
                for layer in &doc.layers {
                    buffers
                        .entry(Arc::as_ptr(&layer.pixels) as usize)
                        .or_insert(layer.pixels.as_raw().len() as u64);
                    if let Some(mask) = &layer.mask {
                        buffers
                            .entry(Arc::as_ptr(mask) as usize)
                            .or_insert(mask.as_raw().len() as u64);
                    }
                    if let Some(mask) = layer
                        .adjustments
                        .scope
                        .as_ref()
                        .and_then(|scope| scope.mask.as_ref())
                    {
                        buffers
                            .entry(Arc::as_ptr(mask) as usize)
                            .or_insert(mask.as_raw().len() as u64);
                    }
                    if let Some(profile) = &layer.adjustments.lens_profile {
                        buffers
                            .entry(Arc::as_ptr(&profile.xml) as usize)
                            .or_insert(profile.xml.len() as u64);
                        buffers
                            .entry(Arc::as_ptr(&profile.vignetting) as usize)
                            .or_insert(
                                (profile.vignetting.len()
                                    * std::mem::size_of::<crate::lens_profile::Vignetting>())
                                    as u64,
                            );
                    }
                    if let Some(raw) = &layer.raw_source {
                        buffers
                            .entry(Arc::as_ptr(&raw.bytes) as usize)
                            .or_insert(raw.bytes.len() as u64);
                    }
                    if let Some(pixels) = &layer.precision {
                        buffers
                            .entry(Arc::as_ptr(pixels) as usize)
                            .or_insert(pixels.as_raw().len() as u64 * 4);
                    }
                }
            }
        }
        buffers.values().sum()
    }
}

pub struct Brush {
    pub radius: f32,
    pub opacity: f32,
    pub hardness: f32,
    pub color: [u8; 4],
    pub erase: bool,
    pub mask: bool,
}

pub fn paint_line(
    layer: &mut Layer,
    from: [f32; 2],
    to: [f32; 2],
    brush: &Brush,
    selection: Option<&Selection>,
) {
    if layer.locked || (layer.adjustment_layer && !brush.mask) {
        return;
    }
    if brush.mask && layer.mask.is_none() {
        return;
    }
    let radius = brush.radius.max(0.5);
    let (transform, offset, size) = if brush.mask {
        let Some(geometry) = crate::masks::geometry(layer) else {
            return;
        };
        geometry
    } else {
        (
            layer.transform,
            layer.offset,
            [layer.pixels.width(), layer.pixels.height()],
        )
    };
    let local_from = transform.to_local(from, offset, size);
    let local_to = transform.to_local(to, offset, size);
    let bound_radius = radius / transform.scale[0].abs().min(transform.scale[1].abs());
    let min_x = ((local_from[0].min(local_to[0]) - bound_radius).floor() as i32).max(0);
    let min_y = ((local_from[1].min(local_to[1]) - bound_radius).floor() as i32).max(0);
    let max_x = ((local_from[0].max(local_to[0]) + bound_radius).ceil() as i32).min(size[0] as i32);
    let max_y = ((local_from[1].max(local_to[1]) + bound_radius).ceil() as i32).min(size[1] as i32);
    let (min_x, min_y, max_x, max_y) = if transform.warp.is_some() {
        let (min, max) = transform.local_bounds(
            [from[0].min(to[0]) - radius, from[1].min(to[1]) - radius],
            [from[0].max(to[0]) + radius, from[1].max(to[1]) + radius],
            offset,
            size,
        );
        (min[0], min[1], max[0], max[1])
    } else {
        (min_x, min_y, max_x, max_y)
    };
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let length_squared = dx * dx + dy * dy;
    let mut image = (!brush.mask).then(|| Arc::make_mut(&mut layer.pixels));
    let mut precision = if brush.mask {
        None
    } else {
        layer.precision.as_mut().map(Arc::make_mut)
    };
    let mut mask = if brush.mask {
        layer.mask.as_mut().map(Arc::make_mut)
    } else {
        None
    };
    for y in min_y..max_y {
        for x in min_x..max_x {
            let document = transform.to_document([x as f32 + 0.5, y as f32 + 0.5], offset, size);
            if selection.is_some_and(|s| {
                !s.contains(document[0].floor() as i32, document[1].floor() as i32)
            }) {
                continue;
            }
            let px = document[0] - from[0];
            let py = document[1] - from[1];
            let t = if length_squared > 0.0 {
                ((px * dx + py * dy) / length_squared).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let distance = ((px - t * dx).powi(2) + (py - t * dy).powi(2)).sqrt();
            if distance >= radius {
                continue;
            }
            let edge = radius * brush.hardness.clamp(0.0, 0.99);
            let selection_alpha = selection.map_or(1.0, |s| {
                s.coverage(document[0].floor() as i32, document[1].floor() as i32)
            });
            let coverage = ((radius - distance) / (radius - edge)).clamp(0.0, 1.0)
                * brush.opacity
                * selection_alpha;
            if let Some(mask) = &mut mask {
                let value = if brush.erase {
                    255
                } else {
                    (u32::from(brush.color[0]) * 54
                        + u32::from(brush.color[1]) * 183
                        + u32::from(brush.color[2]) * 19)
                        / 256
                } as f32;
                let p = mask.get_pixel_mut(x as u32, y as u32);
                p[0] = (p[0] as f32 * (1.0 - coverage) + value * coverage).round() as u8;
                continue;
            }
            let pixel = image.as_mut().unwrap().get_pixel_mut(x as u32, y as u32);
            if let Some(high) = &mut precision {
                let p = high.get_pixel_mut(x as u32, y as u32);
                if brush.erase {
                    p[3] *= 1.0 - coverage;
                } else {
                    p.0 = crate::precision::over(
                        brush.color.map(|c| c as f32 / 255.0),
                        p.0,
                        coverage,
                        BlendMode::Normal,
                    );
                }
                pixel.0 = p.0.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
                continue;
            }
            if brush.erase {
                pixel.0[3] = (f32::from(pixel.0[3]) * (1.0 - coverage)).round() as u8;
                if pixel.0[3] == 0 {
                    pixel.0 = [0; 4];
                }
            } else {
                pixel.0 = over(brush.color, pixel.0, coverage);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regional_composite_matches_full_pixels_with_transforms_masks_hdr_groups_and_effects() {
        let mut doc = Document::new(48, 40, false).unwrap();
        doc.add_layer(
            "sample".into(),
            RgbaImage::from_fn(32, 24, |x, y| Rgba([(x * 7) as u8, (y * 9) as u8, 90, 160])),
        )
        .unwrap();
        {
            let l = doc.active_layer_mut().unwrap();
            l.offset = [3, -2];
            l.transform.angle = 23.0;
            l.transform.scale = [-1.2, 0.8];
            l.transform.translation = [0.25, 1.7];
            l.mask = Some(Arc::new(image::GrayImage::from_fn(32, 24, |x, y| {
                image::Luma([((x + y) * 4) as u8])
            })));
            l.adjustments.exposure = 0.7;
            l.adjustments.vignette = 0.3;
            l.adjustments.grain = 0.15;
        }
        fn check(doc: &Document) {
            let full = doc.composite();
            for region in [
                PixelRegion {
                    min: [7, 5],
                    max: [29, 26],
                },
                PixelRegion {
                    min: [0, 0],
                    max: [8, 9],
                },
                PixelRegion {
                    min: [38, 30],
                    max: [48, 40],
                },
            ] {
                let patch = doc.composite_region(region);
                assert_eq!(
                    patch,
                    image::imageops::crop_imm(
                        &full,
                        region.min[0],
                        region.min[1],
                        region.size()[0],
                        region.size()[1]
                    )
                    .to_image()
                );
            }
        }
        for mode in BlendMode::ALL {
            doc.active_layer_mut().unwrap().blend = mode;
            check(&doc);
        }
        doc.active_layer_mut().unwrap().precision = Some(Arc::new(
            image::Rgba32FImage::from_pixel(32, 24, Rgba([1.5, 0.123456, -0.2, 0.8])),
        ));
        doc.active_layer_mut().unwrap().bit_depth = 32;
        check(&doc);
        doc.group_active().unwrap();
        let group = doc.active;
        doc.active_layer_mut().unwrap().mask =
            Some(Arc::new(image::GrayImage::from_fn(48, 40, |x, _| {
                image::Luma([(x * 5) as u8])
            })));
        for pass in [true, false] {
            doc.active_layer_mut().unwrap().pass_through = pass;
            check(&doc);
        }
        doc.add_layer(
            "clip".into(),
            RgbaImage::from_pixel(48, 40, Rgba([10, 140, 200, 100])),
        )
        .unwrap();
        doc.active_layer_mut().unwrap().parent = Some(group);
        doc.active_layer_mut().unwrap().clipped = true;
        check(&doc);
        doc.active_layer_mut().unwrap().effects.shadow.enabled = true;
        assert!(!doc.local_preview());
        check(&doc);
        doc.active_layer_mut().unwrap().effects.shadow.enabled = false;
        doc.active_layer_mut().unwrap().adjustments.blur = 1.0;
        assert!(!doc.local_preview());
        check(&doc);
    }

    #[test]
    fn transparent_composite_and_negative_layer_offsets() {
        let mut doc = Document::new(2, 1, true).unwrap();
        doc.add_layer(
            "red".into(),
            RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 128])),
        )
        .unwrap();
        doc.active_layer_mut().unwrap().offset = [-1, 0];
        let result = doc.composite();
        assert_eq!(result.get_pixel(0, 0).0, [255, 0, 0, 128]);
        assert_eq!(result.get_pixel(1, 0).0, [0, 0, 0, 0]);
        assert_eq!(
            over([255, 0, 0, 128], [0, 0, 255, 255], 1.0),
            [128, 0, 127, 255]
        );
    }

    #[test]
    fn undo_redo_saved_revision_and_branching() {
        let mut editor = Editor::new(Document::new(2, 2, true).unwrap(), true);
        editor.edit("move", |doc| {
            doc.active_layer_mut().unwrap().offset = [3, 4]
        });
        assert!(editor.dirty());
        editor.undo();
        assert!(!editor.dirty());
        editor.redo();
        assert_eq!(editor.document.active_layer().unwrap().offset, [3, 4]);
        editor.undo();
        editor.edit("rename", |doc| {
            doc.active_layer_mut().unwrap().name = "new".into()
        });
        assert!(!editor.can_redo());
        assert!(editor.dirty());
    }

    #[test]
    fn stroke_is_one_undo_and_selection_protects_pixels() {
        let mut editor = Editor::new(Document::new(10, 3, true).unwrap(), false);
        editor.begin("stroke");
        let brush = Brush {
            radius: 1.0,
            hardness: 1.0,
            opacity: 1.0,
            color: [20, 80, 200, 255],
            erase: false,
            mask: false,
        };
        paint_line(
            editor.document.active_layer_mut().unwrap(),
            [0.5, 1.5],
            [9.5, 1.5],
            &brush,
            Some(&Selection::rectangle([2, 0], [8, 3])),
        );
        editor.finish();
        assert_eq!(editor.document.composite().get_pixel(5, 1).0, brush.color);
        assert_eq!(editor.document.composite().get_pixel(1, 1).0[3], 0);
        editor.undo();
        assert!(editor.document.composite().pixels().all(|p| p.0[3] == 0));
        editor.redo();
        assert_eq!(editor.document.composite().get_pixel(5, 1).0, brush.color);
    }
}
