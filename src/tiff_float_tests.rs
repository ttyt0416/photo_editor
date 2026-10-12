use super::*;
use crate::files::{ExportOptions, export, load_project, open, save_project};
use image::{ImageBuffer, ImageFormat};

#[test]
fn tiff_gray_float_independent_endian_bigtiff_compressed_hdr_icc_orientation_projects() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-gray-float");
    let root = base.join("qa-output/tiff-gray-float");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 80);
    for case in cases {
        let name = case["file"].as_str().unwrap();
        let input = fixtures.join(name);
        let original = std::fs::read(&input).unwrap();
        let opened = open(&input).unwrap();
        assert!(
            opened
                .notes
                .iter()
                .any(|n| n.contains("32비트 회색조 TIFF")),
            "{name}: {:?}",
            opened.notes
        );
        assert_eq!(
            opened.notes.iter().any(|n| n.contains("여러 페이지")),
            case["multipage"].as_bool().unwrap()
        );
        let profile = case["profile"].as_str().unwrap();
        if profile == "invalid" {
            assert!(opened.notes.iter().any(|n| n.contains("적용하지 못")));
        }
        if profile == "linear" || profile == "gamma18" {
            assert!(opened.notes.iter().any(|n| n.contains("작업 sRGB로 변환")));
        }
        if profile == "working" {
            assert!(opened.notes.iter().any(|n| n.contains("원래 float 비트")));
        }
        assert_eq!(opened.document.layers[0].bit_depth, 32);
        let source = opened.document.source_icc.clone();
        if profile == "none" {
            assert!(source.is_none());
        } else {
            assert_eq!(
                source.as_deref().unwrap().as_slice(),
                std::fs::read(fixtures.join(format!("{profile}.icc"))).unwrap()
            );
        }
        let pixels = crate::precision::composite(&opened.document);
        assert!(pixels.pixels().any(|p| p[0] < 0.));
        assert!(pixels.pixels().any(|p| p[0] > 1.));
        let mut editor = crate::document::Editor::new(opened.document, false);
        editor.edit("name", |d| d.layers[0].name = "gray HDR".into());
        editor.undo();
        editor.redo();
        assert_eq!(editor.document.source_icc, source);
        let project = root.join(format!("{name}.rphoto"));
        save_project(&editor.document, &project).unwrap();
        let restored = load_project(&project).unwrap();
        assert_eq!(crate::precision::composite(&restored), pixels);
        assert_eq!(
            restored.source_metadata.xmp,
            editor.document.source_metadata.xmp
        );
        assert_eq!(restored.source_icc, source);
        std::fs::write(
            root.join(format!("{name}.f32")),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        for (extension, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
            let output = root.join(format!("{name}-working.{extension}"));
            export(
                &restored,
                &output,
                format,
                &ExportOptions {
                    tiff_depth: 3,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                crate::precision::composite(&open(&output).unwrap().document),
                pixels
            );
        }
        assert_eq!(std::fs::read(input).unwrap(), original);
    }
}

#[test]
fn tiff_gray_float_nonfinite_truncated_and_regular_tiff_fallback() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-gray-float/errors");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("bad.tiff");
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut encoder = tiff::encoder::TiffEncoder::new(File::create(&path).unwrap()).unwrap();
        encoder
            .new_image::<tiff::encoder::colortype::Gray32Float>(2, 1)
            .unwrap()
            .write_data(&[0.125, value])
            .unwrap();
        assert!(open(&path).err().unwrap().contains("부동소수점"));
    }
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
    assert!(open(&path).is_err());
    let input = DynamicImage::ImageRgba16(ImageBuffer::from_pixel(
        2,
        1,
        Rgba([1, 12345, 65535, 32768]),
    ));
    input.save_with_format(&path, ImageFormat::Tiff).unwrap();
    assert!(read(&path).unwrap().is_none());
    assert_eq!(open(&path).unwrap().document.layers[0].bit_depth, 16);
    input.save_with_format(&path, ImageFormat::Png).unwrap();
    assert!(read(&path).unwrap().is_none());
    assert_eq!(open(&path).unwrap().document.layers[0].bit_depth, 16);
}
