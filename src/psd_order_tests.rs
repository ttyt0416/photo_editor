use super::*;
use serde_json::Value;
use std::path::Path;
fn pixels(layer: &Layer, channels: &Value, alpha: Option<&Value>, depth: u16) {
    let maximum = match depth {
        8 => 255.,
        16 => 65535.,
        _ => 1.,
    };
    let channels = channels.as_array().unwrap();
    for i in 0..12 {
        let alpha_value = alpha.map_or(maximum, |a| a[i].as_f64().unwrap() as f32);
        for (c, value) in channels
            .iter()
            .map(|channel| channel[i].as_f64().unwrap() as f32)
            .chain(std::iter::once(alpha_value))
            .enumerate()
        {
            let value = value / maximum;
            assert_eq!(
                layer.pixels.get_pixel(i as u32 % 4, i as u32 / 4)[c],
                (value.clamp(0., 1.) * 255.).round() as u8
            );
            if depth > 8 {
                assert_eq!(
                    layer
                        .precision
                        .as_ref()
                        .unwrap()
                        .get_pixel(i as u32 % 4, i as u32 / 4)[c]
                        .to_bits(),
                    value.to_bits()
                );
            }
        }
    }
}
fn matching(a: &Document, b: &Document) {
    assert_eq!(a.layers.len(), b.layers.len());
    assert_eq!(a.displayed_layers(), b.displayed_layers());
    for a in &a.layers {
        let b = b.layers.iter().find(|l| l.id == a.id).unwrap();
        assert_eq!(a.name, b.name);
        assert_eq!(a.parent, b.parent);
        assert_eq!(a.group, b.group);
        if a.group {
            assert_eq!(a.pass_through, b.pass_through);
        }
        assert_eq!(a.clipped, b.clipped);
        if !a.group {
            assert_eq!(a.pixels, b.pixels);
            assert_eq!(a.precision, b.precision);
        }
        assert_eq!(a.mask, b.mask);
    }
    assert_eq!(
        crate::precision::composite(a),
        crate::precision::composite(b)
    );
}

#[test]
fn external_native_rgb_order_and_legacy_groups_flat_preserve_layers_samples_project_and_native_outputs()
 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rgb-psd-order");
    let entries: Vec<Value> =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(entries.len(), 48);
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/psd-order");
    std::fs::create_dir_all(&output).unwrap();
    for e in entries {
        let name = e["file"].as_str().unwrap();
        let path = root.join(name);
        let original = std::fs::read(&path).unwrap();
        let doc = read(&original).unwrap_or_else(|error| panic!("{name}: {error}"));
        let depth = e["depth"].as_u64().unwrap() as u16;
        let grouped = e["groups"].as_bool().unwrap();
        assert_eq!(
            doc.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
            if grouped {
                vec![12, 11, 10]
            } else {
                vec![12, 11]
            }
        );
        assert_eq!(doc.active, if grouped { 10 } else { 11 });
        for id in [12, 11] {
            let layer = doc.layers.iter().find(|l| l.id == id).unwrap();
            assert_eq!(layer.parent, grouped.then_some(10));
            assert_eq!(layer.clipped, id == 11);
            pixels(
                layer,
                &e[if id == 12 { "background" } else { "foreground" }],
                (id == 11).then_some(&e["alpha"]),
                depth,
            );
        }
        if grouped {
            let signature = [
                b"8BIMlsct".as_slice(),
                &12u32.to_be_bytes(),
                &1u32.to_be_bytes(),
            ]
            .concat();
            let at = original
                .windows(signature.len())
                .position(|s| s == signature)
                .unwrap();
            let group = doc.layers.iter().find(|l| l.id == 10).unwrap();
            assert!(group.pass_through);
            let mut changed = original.clone();
            changed[at + 20..at + 24].copy_from_slice(b"mul ");
            let isolated = read(&changed).unwrap();
            let group = isolated.layers.iter().find(|l| l.id == 10).unwrap();
            assert!(!group.pass_through);
            assert_eq!(group.blend, BlendMode::Multiply);
            changed[at + 20..at + 24].copy_from_slice(b"xxxx");
            assert!(read(&changed).is_err());
            changed[at + 16..at + 20].copy_from_slice(b"BAD!");
            assert!(read(&changed).is_err());
        }
        let legacy = root.join(e["legacy_file"].as_str().unwrap());
        let old = std::fs::read(&legacy).unwrap();
        matching(&doc, &read_legacy(&old).unwrap());
        if grouped {
            matching(&doc, &read(&old).unwrap());
        } else {
            assert_ne!(read(&old).unwrap().layers[0].id, doc.layers[0].id);
        }
        let opened = crate::files::open_legacy_photoshop(&legacy).unwrap();
        matching(&doc, &opened.document);
        assert!(opened.project_path.is_none());
        assert!(opened.notes.iter().any(|n| n.contains("이전 개발")));
        let project = output.join(format!("{name}.rphoto"));
        crate::files::save_project(&doc, &project).unwrap();
        matching(&doc, &crate::files::load_project(&project).unwrap());
        let compression =
            crate::psd_compression::Compression::ALL[e["compression"].as_u64().unwrap() as usize];
        let target = output.join(name);
        crate::files::save_photoshop_compressed(&doc, &target, compression).unwrap();
        matching(&doc, &read(&std::fs::read(&target).unwrap()).unwrap());
        let composite = crate::precision::composite(&doc);
        std::fs::write(
            output.join(format!("{name}.rgba32.le")),
            composite
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read(&legacy).unwrap(), old);
    }
    assert!(crate::files::open_legacy_photoshop(Path::new("sample.png")).is_err());
}
#[test]
fn native_nested_group_records_remain_bottom_to_top_in_each_parent() {
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/psd-order");
    std::fs::create_dir_all(&output).unwrap();
    let mut doc = Document::from_image(
        "bottom".into(),
        RgbaImage::from_pixel(4, 3, image::Rgba([255, 0, 0, 255])),
    );
    let bottom = doc.active;
    doc.add_layer(
        "middle".into(),
        RgbaImage::from_pixel(4, 3, image::Rgba([0, 255, 0, 128])),
    )
    .unwrap();
    let middle = doc.active;
    doc.group_active().unwrap();
    let inner = doc.active;
    doc.group_active().unwrap();
    let outer = doc.active;
    doc.add_layer(
        "top".into(),
        RgbaImage::from_pixel(4, 3, image::Rgba([0, 0, 255, 64])),
    )
    .unwrap();
    let top = doc.active;
    let top_layer = doc.active_layer_mut().unwrap();
    top_layer.parent = None;
    for large in [false, true] {
        let target = output.join(if large { "nested.psb" } else { "nested.psd" });
        crate::files::save_photoshop_compressed(
            &doc,
            &target,
            crate::psd_compression::Compression::Prediction,
        )
        .unwrap();
        let restored = read(&std::fs::read(target).unwrap()).unwrap();
        matching(&doc, &restored);
        assert_eq!(
            restored
                .layers
                .iter()
                .find(|l| l.id == middle)
                .unwrap()
                .parent,
            Some(inner)
        );
        assert_eq!(
            restored
                .layers
                .iter()
                .find(|l| l.id == inner)
                .unwrap()
                .parent,
            Some(outer)
        );
    }
    std::fs::write(
        output.join("nested-ids.json"),
        serde_json::to_vec(&[bottom, middle, inner, outer, top]).unwrap(),
    )
    .unwrap();
}
