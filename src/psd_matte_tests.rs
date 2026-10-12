use super::*;
use serde_json::Value;
use std::path::Path;
fn float_layer(layer: &Layer) -> image::Rgba32FImage {
    layer.precision.as_ref().map_or_else(
        || image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
        |p| (**p).clone(),
    )
}
#[test]
fn native_white_matte_flat_opaque_tiny_alpha_hdr_legacy_and_source_layers_all_compressions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd-matte");
    let entries: Vec<Value> =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(entries.len(), 48);
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/psd-matte");
    std::fs::create_dir_all(&output).unwrap();
    for e in entries {
        let name = e["file"].as_str().unwrap();
        let path = root.join(name);
        let bytes = std::fs::read(&path).unwrap();
        let depth = e["depth"].as_u64().unwrap() as u16;
        let values = e["source"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|v| {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap() as f32)
            })
            .collect::<Vec<_>>();
        let expected = image::Rgba32FImage::from_raw(6, 2, values).unwrap();
        let imported = read(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let decoded = float_layer(&imported.layers[0]);
        let quantum = match depth {
            8 => 1.0 / 255.,
            16 => 1.0 / 65535.,
            _ => 0.,
        };
        for (a, b) in decoded.pixels().zip(expected.pixels()) {
            assert!((a[3] - b[3]).abs() <= quantum + 1e-6);
            for c in 0..3 {
                assert!(
                    (a[c] * a[3] - b[c] * b[3]).abs() <= 3. * quantum + 4e-6,
                    "{name}: {a:?} vs {b:?}"
                );
            }
            if b[3] == 0.0 {
                assert_eq!(&a.0[..3], &[0.; 3]);
            }
        }
        if let Some(legacy) = e["legacy_file"].as_str() {
            let old = std::fs::read(root.join(legacy)).unwrap();
            let restored = read_legacy(&old).unwrap();
            let actual = float_layer(&restored.layers[0]);
            assert_eq!(
                actual
                    .as_raw()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                expected
                    .as_raw()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>()
            );
            assert_eq!(std::fs::read(root.join(legacy)).unwrap(), old);
        }
        std::fs::write(
            output.join(format!("{name}.input.rgba32.le")),
            decoded
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let source = match depth {
            8 => image::DynamicImage::ImageRgba8(crate::precision::preview(&expected)),
            16 => image::DynamicImage::ImageRgba16(crate::precision::image16(&expected)),
            _ => image::DynamicImage::ImageRgba32F(expected),
        };
        let doc = crate::precision::document("matte original".into(), source).unwrap();
        let original = float_layer(&doc.layers[0]);
        let target = output.join(name);
        let compression =
            crate::psd_compression::Compression::ALL[e["compression"].as_u64().unwrap() as usize];
        crate::files::save_photoshop_compressed(&doc, &target, compression).unwrap();
        let layered = read(&std::fs::read(&target).unwrap()).unwrap();
        assert_eq!(
            float_layer(&layered.layers[0])
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            original
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
        let composite = crate::precision::composite(&doc);
        std::fs::write(
            output.join(format!("{name}.source.rgba32.le")),
            composite
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn invalid_native_merged_alpha_nonfinite_or_unrepresentable_unmatting_is_rejected() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd-matte/d32-c0-b0-a1.psd");
    let bytes = std::fs::read(&path).unwrap();
    let mut reader = Reader::new(&bytes);
    reader.take(26).unwrap();
    reader.section(false).unwrap();
    reader.section(false).unwrap();
    reader.section(false).unwrap();
    reader.u16().unwrap();
    let sample = reader.at;
    let alpha = sample + 6 * 2 * 3 * 4;
    for value in [-0.25, 1.25, f32::NAN, f32::INFINITY] {
        let mut invalid = bytes.clone();
        invalid[alpha..alpha + 4].copy_from_slice(&value.to_be_bytes());
        assert!(read(&invalid).is_err());
    }
    let mut invalid = bytes.clone();
    invalid[alpha..alpha + 4].copy_from_slice(&f32::from_bits(1).to_be_bytes());
    invalid[sample..sample + 4].copy_from_slice(&f32::MAX.to_be_bytes());
    assert!(read(&invalid).is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
