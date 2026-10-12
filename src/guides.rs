use crate::document::Document;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guide {
    pub axis: usize,
    pub position: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    pub rulers: bool,
    pub guides_visible: bool,
    pub guides: Vec<Guide>,
    pub grid_visible: bool,
    pub spacing: [f32; 2],
    pub subdivisions: u32,
    pub snap: bool,
    pub snap_guides: bool,
    pub snap_grid: bool,
    pub snap_layers: bool,
    pub snap_document: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            rulers: false,
            guides_visible: true,
            guides: Vec::new(),
            grid_visible: false,
            spacing: [100.0; 2],
            subdivisions: 4,
            snap: false,
            snap_guides: true,
            snap_grid: false,
            snap_layers: true,
            snap_document: true,
        }
    }
}

impl Layout {
    pub fn valid(&self) -> bool {
        self.guides.len() <= 4096
            && self
                .guides
                .iter()
                .all(|g| g.axis < 2 && g.position.is_finite() && g.position.abs() <= 1e9)
            && self
                .spacing
                .iter()
                .all(|s| s.is_finite() && (1.0..=1e6).contains(s))
            && (1..=16).contains(&self.subdivisions)
    }
    pub fn shift(&mut self, delta: [f32; 2]) {
        for g in &mut self.guides {
            g.position = (g.position + delta[g.axis]).clamp(-1e9, 1e9);
        }
    }
    pub fn scale(&mut self, scale: [f32; 2]) {
        for g in &mut self.guides {
            g.position = (g.position * scale[g.axis]).clamp(-1e9, 1e9);
        }
    }
}

pub fn bounds(document: &Document, ids: &[u64]) -> Option<([f32; 2], [f32; 2])> {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for layer in document
        .layers
        .iter()
        .filter(|l| !l.canvas_space() && ids.iter().any(|id| document.descendant_of(l.id, *id)))
    {
        let (a, b) = layer
            .transform
            .bounds(layer.offset, [layer.pixels.width(), layer.pixels.height()]);
        for axis in 0..2 {
            min[axis] = min[axis].min(a[axis] as f32);
            max[axis] = max[axis].max(b[axis] as f32);
        }
    }
    min[0].is_finite().then_some((min, max))
}

// Tolerance is measured on screen. Exclude the moving trees to avoid snapping to themselves.
pub fn snap(
    document: &Document,
    anchors: &[Vec<f32>; 2],
    excluded: &[u64],
    screen_scale: f32,
) -> ([f32; 2], [Option<f32>; 2]) {
    let layout = &document.layout;
    if !layout.snap || !screen_scale.is_finite() || screen_scale <= 0.0 {
        return ([0.0; 2], [None; 2]);
    }
    let tolerance = 6.0 / screen_scale;
    let mut targets: [Vec<f32>; 2] = Default::default();
    if layout.snap_document {
        for (axis, length) in [document.width, document.height].into_iter().enumerate() {
            targets[axis].extend([0.0, length as f32 / 2.0, length as f32]);
        }
    }
    if layout.snap_guides {
        for guide in &layout.guides {
            targets[guide.axis].push(guide.position);
        }
    }
    if layout.snap_layers {
        for layer in document.layers.iter().filter(|l| {
            !l.canvas_space()
                && l.visible
                && !document.layers.iter().any(|parent| {
                    parent.group && !parent.visible && document.descendant_of(l.id, parent.id)
                })
                && !excluded.iter().any(|id| document.descendant_of(l.id, *id))
        }) {
            let (min, max) = layer
                .transform
                .bounds(layer.offset, [layer.pixels.width(), layer.pixels.height()]);
            for axis in 0..2 {
                targets[axis].extend([
                    min[axis] as f32,
                    (min[axis] as f32 + max[axis] as f32) / 2.0,
                    max[axis] as f32,
                ]);
            }
        }
    }
    let mut delta = [0.0; 2];
    let mut lines = [None; 2];
    for axis in 0..2 {
        let mut distance = tolerance;
        for anchor in &anchors[axis] {
            let grid = layout.snap_grid.then(|| {
                let step = layout.spacing[axis] / layout.subdivisions as f32;
                (anchor / step).round() * step
            });
            for target in targets[axis].iter().copied().chain(grid) {
                let adjustment = target - anchor;
                if adjustment.abs() <= distance {
                    distance = adjustment.abs();
                    delta[axis] = adjustment;
                    lines[axis] = Some(target);
                }
            }
        }
    }
    (delta, lines)
}

pub fn point(document: &Document, point: [f32; 2], scale: f32) -> [f32; 2] {
    let (delta, _) = snap(document, &[vec![point[0]], vec![point[1]]], &[], scale);
    [point[0] + delta[0], point[1] + delta[1]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::Editor, editing, files};
    #[test]
    fn psd_and_psb_guides_use_native_resource_and_updates_replace_old_positions() {
        let mut doc = Document::new(10, 8, true).unwrap();
        doc.layout.guides = vec![
            Guide {
                axis: 0,
                position: 3.125,
            },
            Guide {
                axis: 1,
                position: 6.5,
            },
        ];
        doc.layout.spacing = [64.0, 48.0];
        std::fs::create_dir_all("qa-output").unwrap();
        for (large, name) in [
            (false, "qa-output/guides.psd"),
            (true, "qa-output/guides.psb"),
        ] {
            files::save_document(&doc, std::path::Path::new(name)).unwrap();
            let bytes = std::fs::read(name).unwrap();
            let mut restored = crate::psd::read(&bytes).unwrap();
            assert_eq!(restored.layout.guides, doc.layout.guides);
            assert_eq!(restored.layout.spacing, doc.layout.spacing);
            // Independently inspect the native resource, rather than only exercising our reader.
            let resource = bytes.windows(6).position(|p| p == b"8BIM\x04\x08").unwrap();
            assert_eq!(
                u32::from_be_bytes(bytes[resource + 12..resource + 16].try_into().unwrap()),
                1
            );
            assert_eq!(
                u32::from_be_bytes(bytes[resource + 28..resource + 32].try_into().unwrap()),
                100
            );
            assert_eq!(bytes[resource + 32], 0);
            restored.layout.guides.clear();
            let mut output = std::fs::File::create(format!("{name}.updated")).unwrap();
            crate::psd::write(&restored, &mut output, large).unwrap();
            drop(output);
            let bytes = std::fs::read(format!("{name}.updated")).unwrap();
            assert!(crate::psd::read(&bytes).unwrap().layout.guides.is_empty());
            assert_eq!(
                bytes.windows(6).filter(|p| *p == b"8BIM\x04\x08").count(),
                1
            );
        }
    }

    #[test]
    fn guide_snapping_is_zoom_aware_and_layout_survives_project_history_and_canvas_edits() {
        let mut doc = Document::new(100, 80, true).unwrap();
        doc.layout.snap = true;
        doc.layout.snap_document = false;
        doc.layout.snap_layers = false;
        doc.layout.guides.push(Guide {
            axis: 0,
            position: 40.0,
        });
        assert_eq!(point(&doc, [35.0, 12.0], 1.0), [40.0, 12.0]);
        assert_eq!(point(&doc, [35.0, 12.0], 2.0), [35.0, 12.0]);
        doc.layout.snap_guides = false;
        doc.layout.snap_grid = true;
        assert_eq!(point(&doc, [24.0, 12.0], 1.0), [25.0, 12.0]);
        doc.layout.snap_grid = false;
        doc.layout.snap_layers = true;
        let (delta, lines) = snap(&doc, &[vec![1.0], vec![1.0]], &[doc.active], 1.0);
        assert_eq!((delta, lines), ([0.0; 2], [None; 2]));
        let original = doc.layout.clone();
        let mut editor = Editor::new(doc, true);
        editor.edit("자르기", |d| {
            editing::crop(d, &crate::document::Selection::rectangle([10, 5], [90, 70])).unwrap()
        });
        assert_eq!(editor.document.layout.guides[0].position, 30.0);
        editor.edit("크기", |d| editing::resize(d, 160, 130, true).unwrap());
        assert_eq!(editor.document.layout.guides[0].position, 60.0);
        editor.edit("뒤집기", |d| editing::flip_canvas(d, true));
        assert_eq!(editor.document.layout.guides[0].position, 100.0);
        editor.undo();
        editor.undo();
        editor.undo();
        assert_eq!(editor.document.layout, original);
        let path = std::path::Path::new("qa-output/guides.rphoto");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        files::save_project(&editor.document, path).unwrap();
        assert_eq!(files::load_project(path).unwrap().layout, original);
        let mut invalid = original;
        invalid.guides[0].axis = 2;
        assert!(!invalid.valid());
    }
}
