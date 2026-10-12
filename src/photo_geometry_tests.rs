use super::*;
#[test]
fn geometry_identity_quarter_rotation_translation_validity_and_legacy() {
    let source = Rgba32FImage::from_fn(5, 5, |x, y| {
        Rgba([x as f32 - 2., y as f32 + 2., 0.25, 0.75])
    });
    assert_eq!(Geometry::default().apply_frame(&source, [5, 5], 0), source);
    let mut g = Geometry {
        angle: 90.,
        sampling: crate::sampling::Sampling::Nearest,
        ..Default::default()
    };
    let rotated = g.apply_frame(&source, [5, 5], 0);
    for y in 0..5 {
        for x in 0..5 {
            assert_eq!(rotated.get_pixel(x, y), source.get_pixel(y, 4 - x));
        }
    }
    g.angle = 0.;
    g.shift[0] = 0.2;
    let moved = g.apply_frame(&source, [5, 5], 0);
    for y in 0..5 {
        assert_eq!(moved.get_pixel(0, y).0, [0.; 4]);
        for x in 1..5 {
            assert_eq!(moved.get_pixel(x, y), source.get_pixel(x - 1, y));
        }
    }
    for g in [
        Geometry {
            angle: f32::NAN,
            ..Default::default()
        },
        Geometry {
            perspective: [0.5, 0.],
            ..Default::default()
        },
        Geometry {
            zoom: 0.,
            ..Default::default()
        },
        Geometry {
            aspect: f32::INFINITY,
            ..Default::default()
        },
        Geometry {
            shift: [2., 0.],
            ..Default::default()
        },
    ] {
        assert!(!g.valid());
        assert_eq!(g.apply_frame(&source, [5, 5], 0), source);
        let a = crate::adjustments::Adjustments {
            geometry: g,
            ..Default::default()
        };
        assert!(!a.valid());
        assert!(a.apply_expanded(&source).is_err());
    }
    let old: crate::adjustments::Adjustments = serde_json::from_str("{}").unwrap();
    assert!(old.geometry.is_default());
    assert!(
        serde_json::to_value(&old)
            .unwrap()
            .get("geometry")
            .is_none()
    );
}

#[test]
fn geometry_perspective_modes_tiny_edges_hdr_alpha_outputs_and_padded_scope() {
    use crate::sampling::Sampling;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/photo-geometry");
    std::fs::create_dir_all(&root).unwrap();
    fn bytes(i: &Rgba32FImage) -> Vec<u8> {
        i.as_raw().iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    let mut cases = vec![];
    let variants = [
        Geometry::default(),
        Geometry {
            angle: 90.,
            ..Default::default()
        },
        Geometry {
            angle: -17.5,
            ..Default::default()
        },
        Geometry {
            perspective: [0.25, 0.],
            ..Default::default()
        },
        Geometry {
            perspective: [0., -0.32],
            ..Default::default()
        },
        Geometry {
            perspective: [0.4, 0.4],
            ..Default::default()
        },
        Geometry {
            shift: [0.12, -0.2],
            ..Default::default()
        },
        Geometry {
            zoom: 0.25,
            aspect: 4.,
            ..Default::default()
        },
        Geometry {
            zoom: 4.,
            aspect: 0.25,
            ..Default::default()
        },
        Geometry {
            angle: 27.,
            perspective: [-0.2, 0.3],
            zoom: 1.3,
            aspect: 0.85,
            shift: [-0.04, 0.08],
            ..Default::default()
        },
    ];
    for [w, h] in [[17, 13], [1, 1], [1, 9], [9, 1], [8, 6]] {
        let source = Rgba32FImage::from_fn(w, h, |x, y| {
            Rgba([
                x as f32 * 0.7 - 0.4,
                y as f32 * 0.6 + 0.2,
                (x + y) as f32 * 0.3 - 0.1,
                match (x + y) % 4 {
                    0 => 0.,
                    1 => 0.25,
                    2 => 0.75,
                    _ => 1.,
                },
            ])
        });
        let input = format!("{w}x{h}.f32");
        std::fs::write(root.join(&input), bytes(&source)).unwrap();
        for base in &variants {
            for sampling in [Sampling::Nearest, Sampling::Bilinear, Sampling::Bicubic] {
                let g = Geometry {
                    sampling,
                    ..base.clone()
                };
                assert!(g.valid());
                let output = g.apply_frame(&source, [w, h], 0);
                assert!(output.pixels().all(|p| p.0.iter().all(|v| v.is_finite())));
                let file = format!("{}.f32", cases.len());
                std::fs::write(root.join(&file), bytes(&output)).unwrap();
                cases.push(
                    serde_json::json!({"input":input,"output":file,"size":[w,h],"geometry":g}),
                );
            }
        }
        let shifted = Geometry {
            shift: [1. / w as f32, 0.],
            sampling: Sampling::Nearest,
            ..Default::default()
        };
        let mut padded = Rgba32FImage::new(w + 4, h + 4);
        image::imageops::replace(&mut padded, &source, 2, 2);
        let padded = shifted.apply_frame(&padded, [w, h], 2);
        assert_eq!(
            image::imageops::crop_imm(&padded, 2, 2, w, h).to_image(),
            shifted.apply_frame(&source, [w, h], 0)
        );
    }
    std::fs::write(
        root.join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    let source = Rgba32FImage::from_pixel(5, 5, Rgba([f32::MAX, -f32::MAX, 2., 1e-20]));
    for mode in [Sampling::Bilinear, Sampling::Bicubic] {
        let g = Geometry {
            angle: 17.,
            sampling: mode,
            ..Default::default()
        };
        let out = g.apply_frame(&source, [5, 5], 0);
        assert!(out.pixels().all(|p| p.0.iter().all(|v| v.is_finite())));
        assert!(out.pixels().any(|p| p[0] > 1. && p[1] < 0. && p[3] > 0.));
    }
    let source = Rgba32FImage::from_fn(7, 5, |x, y| Rgba([x as f32, y as f32, 0.4, 1.]));
    let mut doc = crate::precision::document(
        "geometry".into(),
        image::DynamicImage::ImageRgba32F(source.clone()),
    )
    .unwrap();
    let selection = crate::document::Selection {
        min: [2, 1],
        max: [5, 4],
        mask: None,
        inverted: false,
    };
    let a = crate::adjustments::Adjustments {
        geometry: Geometry {
            shift: [1. / 7., 0.],
            sampling: Sampling::Nearest,
            ..Default::default()
        },
        scope: Some(
            crate::adjustment_scope::Scope::capture(&doc.layers[0], &selection, [7, 5]).unwrap(),
        ),
        ..Default::default()
    };
    let out = a.apply_float(&source);
    for y in 0..5 {
        for x in 0..7 {
            assert_eq!(
                out.get_pixel(x, y),
                source.get_pixel(
                    if (2..5).contains(&x) && (1..4).contains(&y) {
                        x - 1
                    } else {
                        x
                    },
                    y
                )
            );
        }
    }
    doc.layers[0].adjustments = a;
    assert!(!doc.local_preview());
    let full = crate::precision::composite(&doc);
    let region = crate::document::PixelRegion {
        min: [1, 1],
        max: [6, 4],
    };
    assert_eq!(
        crate::precision::composite_region(&doc, region),
        image::imageops::crop_imm(&full, 1, 1, 5, 3).to_image()
    );
}
