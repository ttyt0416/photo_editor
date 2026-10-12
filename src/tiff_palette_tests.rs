use super::*;
use crate::files::{ExportOptions, export, load_project, open, save_project};
use image::ImageFormat;
use std::path::Path;

#[test]
fn tiff_palette_alpha_packed_planar_associated_hdr_icc_projects() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-palette-alpha");
    let root = base.join("qa-output/tiff-palette-alpha");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 320);
    for (index, case) in cases.into_iter().enumerate() {
        let name = case["file"].as_str().unwrap();
        let path = fixtures.join(name);
        let before = std::fs::read(&path).unwrap();
        if index == 0 {
            assert!(image::open(&path).is_err());
        }
        let imported = open(&path).unwrap();
        assert!(
            imported
                .notes
                .iter()
                .any(|n| n.contains("색 번호/알파의 TIFF 팔레트"))
        );
        let document = imported.document;
        assert_eq!(document.layers[0].bit_depth, 32);
        let pixels = document.layers[0].precision.as_ref().unwrap();
        if case["associated"].as_bool().unwrap()
            && case["profile"] == "none"
            && case["bits"].as_u64().unwrap() > 1
        {
            assert!(pixels.pixels().any(|p| p[0] > 1. || p[1] > 1.));
        }
        std::fs::write(
            root.join(format!("{name}.f32")),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let composite = crate::precision::composite(&document);
        std::fs::write(
            root.join(format!("{name}.comp.f32")),
            composite
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        save_project(&document, &root.join(format!("{name}.rphoto"))).unwrap();
        let restored = load_project(&root.join(format!("{name}.rphoto"))).unwrap();
        assert_eq!(
            restored.layers[0]
                .precision
                .as_ref()
                .unwrap()
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            pixels
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(restored.source_icc, document.source_icc);
        assert_eq!(restored.source_metadata.xmp, document.source_metadata.xmp);
        for (ext, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
            let output = root.join(format!("{name}-working.{ext}"));
            export(&restored, &output, format, &Default::default()).unwrap();
            assert_eq!(
                crate::precision::composite(&open(&output).unwrap().document),
                composite
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn tiff_palette_alpha_requires_definition_and_complete_planes() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = base.join("qa-output/tiff-palette-alpha-invalid");
    std::fs::create_dir_all(&root).unwrap();
    let source = base.join("tests/fixtures/tiff-palette-alpha/d1-none-a0-p0-b0-e0-t0-z0.tiff");
    let original = std::fs::read(&source).unwrap();
    let ifd = u32::from_le_bytes(original[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(original[ifd..ifd + 2].try_into().unwrap()) as usize;
    let entry = (0..count)
        .map(|i| ifd + 2 + i * 12)
        .find(|e| u16::from_le_bytes(original[*e..*e + 2].try_into().unwrap()) == 338)
        .unwrap();
    for missing in [false, true] {
        let mut bytes = original.clone();
        if missing {
            bytes[entry..entry + 2].copy_from_slice(&65001u16.to_le_bytes());
        } else {
            bytes[entry + 8..entry + 10].copy_from_slice(&0u16.to_le_bytes());
        }
        let path = root.join(format!("undefined-alpha-{missing}.tiff"));
        std::fs::write(&path, bytes).unwrap();
        assert!(open(&path).err().unwrap().contains("알파로 정의"));
    }
    let path = root.join("truncated-alpha-plane.tiff");
    let source = base.join("tests/fixtures/tiff-palette-alpha/d1-working-a0-p1-b0-e0-t0-z0.tiff");
    let bytes = std::fs::read(&source).unwrap();
    let mut decoder = Decoder::new(BufReader::new(File::open(&source).unwrap())).unwrap();
    let offsets = decoder
        .find_tag_unsigned_vec::<u64>(Tag::StripOffsets)
        .unwrap()
        .unwrap();
    std::fs::write(&path, &bytes[..*offsets.last().unwrap() as usize + 1]).unwrap();
    assert!(open(&path).is_err());
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}

#[test]
fn tiff_palette_packed_integer_colormap_icc_tiles_projects() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-palette");
    let root = base.join("qa-output/tiff-palette");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 240);
    for (index, case) in cases.into_iter().enumerate() {
        let name = case["file"].as_str().unwrap();
        let path = fixtures.join(name);
        let before = std::fs::read(&path).unwrap();
        if index == 0 {
            assert!(image::open(&path).is_err());
        }
        let decoded = crate::tiff_float::read(&path).unwrap().unwrap();
        let native = decoded.pixels.into_rgba16();
        std::fs::write(
            root.join(format!("{name}.rgba16")),
            native
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let imported = open(&path).unwrap();
        assert!(
            imported
                .notes
                .iter()
                .any(|n| n.contains("TIFF 팔레트를 16비트"))
        );
        let document = imported.document;
        assert_eq!(document.layers[0].bit_depth, 16);
        let composite = crate::precision::composite(&document);
        save_project(&document, &root.join(format!("{name}.rphoto"))).unwrap();
        let restored = load_project(&root.join(format!("{name}.rphoto"))).unwrap();
        assert_eq!(crate::precision::composite(&restored), composite);
        assert_eq!(restored.source_icc, document.source_icc);
        assert_eq!(restored.source_metadata.xmp, document.source_metadata.xmp);
        let output = root.join(format!("{name}-working.tiff"));
        export(
            &restored,
            &output,
            ImageFormat::Tiff,
            &ExportOptions::default(),
        )
        .unwrap();
        assert_eq!(image::open(output).unwrap().into_rgba16(), native);
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn tiff_palette_bad_colormap_truncation_and_native_state_restoration() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = base.join("qa-output/tiff-palette-invalid");
    std::fs::create_dir_all(&root).unwrap();
    let source = base.join("tests/fixtures/tiff-palette/d1-none-b0-e0-t0-z0.tiff");
    let original = std::fs::read(&source).unwrap();
    let ifd = u32::from_le_bytes(original[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(original[ifd..ifd + 2].try_into().unwrap()) as usize;
    let entry = |tag| {
        (0..count)
            .map(|i| ifd + 2 + i * 12)
            .find(|e| u16::from_le_bytes(original[*e..*e + 2].try_into().unwrap()) == tag)
            .unwrap()
    };
    for missing in [false, true] {
        let mut bytes = original.clone();
        let e = entry(320);
        if missing {
            bytes[e..e + 2].copy_from_slice(&65001u16.to_le_bytes());
        } else {
            bytes[e + 4..e + 8].copy_from_slice(&5u32.to_le_bytes());
        }
        let path = root.join(format!("colormap-{missing}.tiff"));
        std::fs::write(&path, bytes).unwrap();
        assert!(open(&path).err().unwrap().contains("ColorMap"));
    }
    let mut decoder = Decoder::new(BufReader::new(File::open(&source).unwrap())).unwrap();
    let offsets = decoder
        .find_tag_unsigned_vec::<u64>(Tag::StripOffsets)
        .unwrap()
        .unwrap();
    let lengths = decoder
        .find_tag_unsigned_vec::<u64>(Tag::StripByteCounts)
        .unwrap()
        .unwrap();
    let path = root.join("truncated.tiff");
    std::fs::write(&path, &original[..offsets[0] as usize + 1]).unwrap();
    assert!(open(&path).is_err());
    let mut decoded = DecodingResult::U8(vec![]);
    decoder
        .read_palette_indices_to_buffer(&mut decoded)
        .unwrap();
    assert_eq!(
        decoder
            .get_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
            .unwrap(),
        3
    );
    assert!(decoder.colortype().is_err());
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = 1;
    decoder = decoder.with_limits(limits);
    assert!(
        decoder
            .read_palette_indices_to_buffer(&mut DecodingResult::U8(vec![]))
            .is_err()
    );
    assert_eq!(
        decoder
            .get_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
            .unwrap(),
        3
    );
    assert!(decoder.colortype().is_err());
    let mut reversed = original.clone();
    let e = entry(266);
    reversed[e + 8..e + 10].copy_from_slice(&2u16.to_le_bytes());
    for (&offset, &length) in offsets.iter().zip(&lengths) {
        for byte in &mut reversed[offset as usize..(offset + length) as usize] {
            *byte = byte.reverse_bits();
        }
    }
    let path = root.join("fill-order-2.tiff");
    std::fs::write(&path, reversed).unwrap();
    assert_eq!(
        open(&path).unwrap().document.composite(),
        open(&source).unwrap().document.composite()
    );
    assert_eq!(std::fs::read(source).unwrap(), original);
}
