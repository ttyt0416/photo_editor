use super::*;
use serde_json::Value;
use std::path::Path;
fn float(layer: &Layer) -> image::Rgba32FImage {
    layer.precision.as_ref().map_or_else(
        || image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
        |p| (**p).clone(),
    )
}
fn dump(path: &Path, image: &image::Rgba32FImage) {
    std::fs::write(
        path,
        image
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
#[test]
fn grayscale_native_layers_masks_profiles_depths_compressions_keep_exact_source_and_composite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gray-psd");
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/gray-psd-export");
    std::fs::create_dir_all(&output).unwrap();
    let entries: Vec<Value> =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(entries.len(), 72);
    for e in entries {
        let name = e["file"].as_str().unwrap();
        let source = root.join(name);
        let original = std::fs::read(&source).unwrap();
        let doc = read(&original).unwrap();
        assert!(neutral(&doc));
        let options = OutputOptions {
            compression: crate::psd_compression::Compression::ALL
                [e["compression"].as_u64().unwrap() as usize],
            color: OutputColor::Grayscale,
            grayscale_layers: false,
        };
        let target = output.join(name);
        crate::files::save_photoshop_options(&doc, &target, options).unwrap();
        let bytes = std::fs::read(&target).unwrap();
        assert_eq!(&bytes[12..14], &2u16.to_be_bytes());
        assert_eq!(&bytes[24..26], &1u16.to_be_bytes());
        let restored = read(&bytes).unwrap();
        assert_eq!(restored.layers.len(), doc.layers.len());
        assert_eq!(restored.psd_resources, doc.psd_resources);
        assert_eq!(restored.layout, doc.layout);
        for a in &doc.layers {
            let b = restored.layers.iter().find(|l| l.id == a.id).unwrap();
            assert_eq!(a.parent, b.parent);
            assert_eq!(a.mask, b.mask);
            assert_eq!(a.clipped, b.clipped);
            if !a.group {
                assert_eq!(
                    float(a)
                        .as_raw()
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                    float(b)
                        .as_raw()
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>()
                );
            }
        }
        dump(
            &output.join(format!("{name}.rgba32.le")),
            &crate::precision::composite(&doc),
        );
        assert_eq!(std::fs::read(source).unwrap(), original);
    }
}
#[test]
fn grayscale_colored_composite_or_per_layer_conversion_preserves_original_alpha_and_explicit_structure_choice()
 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rgb-psd-order");
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/gray-psd-color");
    std::fs::create_dir_all(&output).unwrap();
    let entries: Vec<Value> =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(entries.len(), 48);
    for e in entries {
        let name = e["file"].as_str().unwrap();
        let source = root.join(name);
        let original = std::fs::read(&source).unwrap();
        let doc = read(&original).unwrap();
        assert!(!neutral(&doc));
        dump(
            &output.join(format!("{name}.source.rgba32.le")),
            &crate::precision::composite(&doc),
        );
        for keep in [false, true] {
            let options = OutputOptions {
                compression: crate::psd_compression::Compression::ALL
                    [e["compression"].as_u64().unwrap() as usize],
                color: OutputColor::Grayscale,
                grayscale_layers: keep,
            };
            let target = output.join(format!("keep{}-{name}", u8::from(keep)));
            crate::files::save_photoshop_options(&doc, &target, options).unwrap();
            let restored = read(&std::fs::read(target).unwrap()).unwrap();
            assert!(neutral(&restored));
            assert!(!has_incompatible_profile(&restored, OutputColor::Grayscale));
            assert!(restored.psd_resources.len() < doc.psd_resources.len());
            assert_eq!(restored.layout, doc.layout);
            if keep {
                assert_eq!(restored.displayed_layers(), doc.displayed_layers());
                for a in doc.layers.iter().filter(|l| !l.group) {
                    let b = restored.layers.iter().find(|l| l.id == a.id).unwrap();
                    assert_eq!(a.parent, b.parent);
                    assert_eq!(a.mask, b.mask);
                    assert_eq!(a.clipped, b.clipped);
                    for (a, b) in float(a).pixels().zip(float(b).pixels()) {
                        assert_eq!(a[3].to_bits(), b[3].to_bits());
                    }
                }
            } else {
                assert_eq!(restored.layers.len(), 1);
                assert!(!restored.layers[0].group);
            }
        }
        assert_eq!(std::fs::read(source).unwrap(), original);
        assert!(!neutral(&doc));
    }
}
#[test]
fn grayscale_prepared_color_adjustments_and_failed_source_keep_original_and_existing_output() {
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/gray-psd-adjustments");
    std::fs::create_dir_all(&output).unwrap();
    let mut doc = Document::from_image(
        "gray original".into(),
        RgbaImage::from_pixel(4, 3, image::Rgba([100, 100, 100, 200])),
    );
    doc.group_active().unwrap();
    let source = doc.layers.iter().find(|l| !l.group).unwrap().id;
    let layer = doc.layers.iter_mut().find(|l| l.id == source).unwrap();
    layer.adjustments.color_balance.shifts[1] = [0.5, -0.2, 0.1];
    let source_pixels = layer.pixels.clone();
    let before = crate::precision::composite(&doc);
    assert!(neutral(&doc));
    let options = OutputOptions {
        color: OutputColor::Grayscale,
        ..Default::default()
    };
    let target = output.join("adjusted.psd");
    crate::files::save_photoshop_options(&doc, &target, options).unwrap();
    let restored = read(&std::fs::read(&target).unwrap()).unwrap();
    assert_eq!(restored.layers.len(), 1);
    assert!(neutral(&restored));
    dump(&output.join("source.rgba32.le"), &before);
    assert!(Arc::ptr_eq(
        &source_pixels,
        &doc.layers.iter().find(|l| l.id == source).unwrap().pixels
    ));
    assert_eq!(crate::precision::composite(&doc), before);
    let mut invalid = crate::precision::document(
        "invalid float".into(),
        image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
            4,
            3,
            image::Rgba([1., 0.5, 0.25, 1.]),
        )),
    )
    .unwrap();
    Arc::make_mut(invalid.layers[0].precision.as_mut().unwrap()).get_pixel_mut(0, 0)[0] = f32::NAN;
    let failure = output.join("failed.psd");
    std::fs::write(&failure, b"keep previous").unwrap();
    assert!(crate::files::save_photoshop_options(&invalid, &failure, options).is_err());
    assert_eq!(std::fs::read(&failure).unwrap(), b"keep previous");
}
