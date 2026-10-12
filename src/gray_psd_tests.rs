use super::*;
use serde_json::Value;
use std::path::Path;

fn samples(values: &Value, depth: u16) -> Vec<f32> {
    let maximum = match depth {
        8 => 255.0,
        16 => 65535.0,
        _ => 1.0,
    };
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32 / maximum)
        .collect()
}
fn assert_layer(layer: &Layer, gray: &[f32], alpha: &[f32], depth: u16) {
    assert_eq!(layer.bit_depth, depth as u8);
    assert_eq!(layer.pixels.dimensions(), (4, 3));
    for (i, p) in layer.pixels.pixels().enumerate() {
        assert_eq!(p[0], (gray[i].clamp(0.0, 1.0) * 255.0).round() as u8);
        assert_eq!(p[1], p[0]);
        assert_eq!(p[2], p[0]);
        assert_eq!(p[3], (alpha[i].clamp(0.0, 1.0) * 255.0).round() as u8);
        if depth > 8 {
            let high = layer
                .precision
                .as_ref()
                .unwrap()
                .get_pixel(i as u32 % 4, i as u32 / 4);
            for c in 0..3 {
                assert_eq!(high[c].to_bits(), gray[i].to_bits());
            }
            assert_eq!(high[3].to_bits(), alpha[i].to_bits());
        }
    }
}

#[test]
fn grayscale_external_order_layers_masks_flat_alpha_all_depths_compressions_and_project_preserve_samples()
 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gray-psd");
    let entries: Vec<Value> =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(entries.len(), 72);
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/gray-psd");
    std::fs::create_dir_all(&output).unwrap();
    for entry in entries {
        let name = entry["file"].as_str().unwrap();
        let path = root.join(name);
        let bytes = std::fs::read(&path).unwrap();
        let depth = entry["depth"].as_u64().unwrap() as u16;
        let opened = crate::files::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            opened
                .notes
                .iter()
                .any(|n| n.contains("회색조") && n.contains("RGB 작업 버퍼"))
        );
        let doc = opened.document;
        assert_eq!((doc.width, doc.height), (4, 3));
        assert!(has_non_rgb_profile(&doc));
        assert_eq!(doc.layout.guides[0].position, 1.25);
        let kind = entry["kind"].as_str().unwrap();
        if kind == "layers" {
            assert_eq!(doc.layers.len(), 3);
            let group = doc.layers.iter().find(|l| l.id == 10).unwrap();
            assert!(group.group && group.pass_through && group.parent.is_none());
            assert_eq!(group.name, "회색 그룹");
            assert_eq!(group.mask.as_ref().unwrap().get_pixel(0, 0)[0], 192);
            let foreground = doc.layers.iter().find(|l| l.id == 11).unwrap();
            assert_eq!(foreground.parent, Some(10));
            assert!(foreground.clipped);
            assert_layer(
                foreground,
                &samples(&entry["foreground"], depth),
                &samples(&entry["alpha"], depth),
                depth,
            );
            assert_eq!(
                foreground.mask.as_ref().unwrap().as_raw(),
                &entry["mask8"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u8)
                    .collect::<Vec<_>>()
            );
            let background = doc.layers.iter().find(|l| l.id == 12).unwrap();
            assert_eq!(background.parent, Some(10));
            assert_layer(
                background,
                &samples(&entry["gray"], depth),
                &[1.; 12],
                depth,
            );
            assert_eq!(
                doc.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
                vec![12, 11, 10]
            );
        } else {
            assert_layer(
                &doc.layers[0],
                &samples(&entry["merged"], depth),
                &samples(&entry["merged_alpha"], depth),
                depth,
            );
        }
        for layer in doc.layers.iter().filter(|l| !l.group) {
            let image = layer.precision.as_ref().map_or_else(
                || image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
                |p| (**p).clone(),
            );
            std::fs::write(
                output.join(format!("{name}-{}.rgba32.le", layer.id)),
                image
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        let project = output.join(format!("{name}.rphoto"));
        crate::files::save_project(&doc, &project).unwrap();
        let restored = crate::files::load_project(&project).unwrap();
        assert_eq!(restored.psd_resources, doc.psd_resources);
        assert_eq!(restored.layout, doc.layout);
        for (a, b) in restored.layers.iter().zip(&doc.layers) {
            assert_eq!(a.pixels, b.pixels);
            assert_eq!(a.precision, b.precision);
            if let (Some(a), Some(b)) = (&a.precision, &b.precision) {
                assert_eq!(
                    a.as_raw().iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    b.as_raw().iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                );
            }
            assert_eq!(a.mask, b.mask);
            assert_eq!(a.parent, b.parent);
            assert_eq!(a.clipped, b.clipped);
        }
        // RGB output of neutral pixels retains samples while excluding incompatible GRAY ICC.
        // Group order migration is a separate next feature, so independently test flat output here.
        if kind == "flat-alpha" && entry["compression"] == 0 && entry["large"] == false {
            let target = output.join(format!("neutral-{depth}.psd"));
            crate::files::save_photoshop(&doc, &target).unwrap();
            let rgb = read(&std::fs::read(&target).unwrap()).unwrap();
            assert!(!has_non_rgb_profile(&rgb));
            assert_eq!(rgb.layout.guides, doc.layout.guides);
            assert_layer(
                &rgb.layers[0],
                &samples(&entry["merged"], depth),
                &samples(&entry["merged_alpha"], depth),
                depth,
            );
            assert!(has_non_rgb_profile(&doc));
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn grayscale_invalid_channels_nonfinite_truncated_and_other_modes_are_rejected_without_source_changes()
 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gray-psd");
    let bytes = std::fs::read(root.join("d32-c0-b0-flat-alpha.psd")).unwrap();
    for prefix in [0, 4, 25, bytes.len() - 1] {
        assert!(read(&bytes[..prefix]).is_err());
    }
    for channels in [0u16, 3, 57] {
        let mut bad = bytes.clone();
        bad[12..14].copy_from_slice(&channels.to_be_bytes());
        assert!(read(&bad).is_err());
    }
    for mode in [0u16, 2, 4, 7, 8, 9] {
        let mut bad = bytes.clone();
        bad[24..26].copy_from_slice(&mode.to_be_bytes());
        assert!(read(&bad).is_err());
    }
    let mut bad = bytes.clone();
    let mut r = Reader::new(&bad);
    r.take(26).unwrap();
    r.section(false).unwrap();
    r.section(false).unwrap();
    r.section(false).unwrap();
    r.u16().unwrap();
    let pixel_at = r.at;
    bad[pixel_at..pixel_at + 4].copy_from_slice(&f32::NAN.to_be_bytes());
    assert!(read(&bad).is_err());
    let bytes = std::fs::read(root.join("d8-c0-b0-layers.psd")).unwrap();
    // The first record is the closing divider, the second is the background's one gray plane.
    let mut r = Reader::new(&bytes);
    r.take(26).unwrap();
    r.section(false).unwrap();
    r.section(false).unwrap();
    let outer_at = r.at + 4;
    let mut outer = r.section(false).unwrap();
    let info_at = outer_at + 4;
    let mut info = outer.section(false).unwrap();
    info.i16().unwrap();
    info.take(16).unwrap();
    assert_eq!(info.u16().unwrap(), 0);
    info.take(12).unwrap();
    info.section(false).unwrap();
    info.take(16).unwrap();
    info.u16().unwrap();
    let id_at = info_at + info.at;
    let mut bad = bytes.clone();
    bad[id_at..id_at + 2].copy_from_slice(&1i16.to_be_bytes());
    assert!(read(&bad).is_err());
    assert_eq!(
        std::fs::read(root.join("d8-c0-b0-layers.psd")).unwrap(),
        bytes
    );
}
