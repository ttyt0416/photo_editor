use super::*;
use crate::files::{ExportOptions, export, load_project, open, save_project};
use std::sync::Arc;
fn packet(mode: usize) -> Vec<u8> {
    let fields=match mode {
        0=>"<rdf:Description a:Orientation='&#x36;' a:ImageWidth='7' a:ImageLength='5' e:PixelXDimension='7' e:PixelYDimension='5' q:Orientation='leave' />".into(),
        1=>"<rdf:Description><a:Orientation>6</a:Orientation><a:ImageWidth>7</a:ImageWidth><a:ImageLength>5</a:ImageLength><e:PixelXDimension>7</e:PixelXDimension><e:PixelYDimension>5</e:PixelYDimension><q:Orientation>leave</q:Orientation></rdf:Description>".into(),
        2=>"<rdf:Description><a:Orientation><![CDATA[6]]></a:Orientation><a:ImageWidth><![CDATA[7]]></a:ImageWidth><a:ImageLength>5</a:ImageLength><e:PixelXDimension>7</e:PixelXDimension><e:PixelYDimension>5</e:PixelYDimension></rdf:Description>".into(),
        3=>"<rdf:Description><a:Orientation rdf:parseType='Resource'><rdf:value>6</rdf:value><q:source>camera</q:source></a:Orientation><a:ImageWidth>7</a:ImageWidth><a:ImageLength>5</a:ImageLength><e:PixelXDimension>7</e:PixelXDimension><e:PixelYDimension>5</e:PixelYDimension></rdf:Description>".into(),
        4=>"<rdf:Description><a:Orientation /><a:ImageWidth><!--keep--></a:ImageWidth><a:ImageLength></a:ImageLength><e:PixelXDimension/><e:PixelYDimension/></rdf:Description>".into(),
        _=>format!("<rdf:Description a:Orientation{}={} '&#x36;' a:ImageWidth='7' a:ImageLength='5' e:PixelXDimension='7' e:PixelYDimension='5' q:Orientation='leave' />"," ".repeat(320)," ".repeat(260)),
    };
    format!("<?xpacket begin='\u{feff}'?><x:xmpmeta xmlns:x='adobe:ns:meta/' xmlns:rdf='{RDF}' xmlns:a='{TIFF}' xmlns:e='{EXIF}' xmlns:p='{XMP}' xmlns:q='urn:photo-editor:qa' xmlns:dc='http://purl.org/dc/elements/1.1/'><!--keep packet comment--><rdf:RDF>{fields}<rdf:Description q:rating='4'><dc:creator><rdf:Seq><rdf:li>촬영자</rdf:li></rdf:Seq></dc:creator><q:notes><![CDATA[color & <unrelated>]]></q:notes><p:Thumbnails><rdf:Alt><rdf:li>old preview</rdf:li></rdf:Alt></p:Thumbnails></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>").into_bytes()
}
fn exif() -> Vec<u8> {
    let mut b = b"II\x2a\0\x08\0\0\0\x01\0".to_vec();
    b.extend([0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0]);
    b
}
#[test]
fn xmp_resource_and_ambiguous_qualified_scalars_are_rejected() {
    for body in [
        "<a:Orientation rdf:resource='urn:orientation:6'/>",
        "<a:Orientation rdf:nodeID='old'/>",
        "<a:Orientation rdf:parseType='Literal'>6</a:Orientation>",
        "<a:Orientation rdf:parseType='Resource'><rdf:value>6</rdf:value><rdf:value>8</rdf:value></a:Orientation>",
        "<a:Orientation rdf:parseType='Resource'><rdf:value rdf:resource='urn:orientation:6'/></a:Orientation>",
        "<a:Orientation rdf:parseType='Resource'>6<rdf:value>8</rdf:value></a:Orientation>",
    ] {
        let xml = format!("<r xmlns:a='{TIFF}' xmlns:rdf='{RDF}'>{body}</r>");
        assert!(normalize(xml.as_bytes(), [9, 11]).is_err(), "{body}");
    }
}
#[test]
fn xmp_formats_namespace_properties_scalar_forms_metadata_flags_payloads_and_precision() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/xmp-output");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for mode in 0..6 {
        let original = packet(mode);
        std::fs::write(root.join(format!("source-{mode}.xmp")), &original).unwrap();
        for (label, format, depth, lossless) in [
            ("png8", image::ImageFormat::Png, 8, true),
            ("jpg", image::ImageFormat::Jpeg, 8, true),
            ("webp-lossless", image::ImageFormat::WebP, 8, true),
            ("webp-lossy", image::ImageFormat::WebP, 8, false),
            ("tiff8", image::ImageFormat::Tiff, 8, true),
            ("png16", image::ImageFormat::Png, 16, true),
            ("tiff16", image::ImageFormat::Tiff, 16, true),
            ("tiff32", image::ImageFormat::Tiff, 32, true),
        ] {
            let pixels = image::Rgba32FImage::from_fn(9, 11, |x, y| {
                image::Rgba([
                    0.031 + x as f32 / 19.,
                    0.127 + y as f32 / 29.,
                    if depth == 32 {
                        2.5 + x as f32 / 17.
                    } else {
                        0.219 + x as f32 / 31.
                    },
                    if x == 0 { 0.5 } else { 1. },
                ])
            });
            let image = match depth {
                8 => image::DynamicImage::ImageRgba8(crate::precision::preview(&pixels)),
                16 => image::DynamicImage::ImageRgba16(crate::precision::image16(&pixels)),
                _ => image::DynamicImage::ImageRgba32F(pixels),
            };
            let mut doc = crate::precision::document("XMP".into(), image).unwrap();
            let mut notes = vec![];
            doc.source_metadata = crate::source_metadata::Metadata::from_bytes(
                Some(exif()),
                Some(original.clone()),
                &mut notes,
            );
            let source = doc.source_metadata.xmp.clone().unwrap();
            let pixels = doc.layers[0].pixels.clone();
            let high = doc.layers[0].precision.clone();
            for icc in [false, true] {
                for include_exif in [false, true] {
                    let mut baseline = None;
                    for include in [false, true] {
                        let options = ExportOptions {
                            include_icc: icc,
                            include_exif,
                            include_xmp: include,
                            webp_lossless: lossless,
                            tiff_depth: if depth == 32 {
                                3
                            } else if depth == 16 {
                                2
                            } else {
                                1
                            },
                            tiff_big: mode % 2 == 1,
                            ..Default::default()
                        };
                        let extension = if format == image::ImageFormat::Jpeg {
                            "jpg"
                        } else if format == image::ImageFormat::Tiff {
                            "tiff"
                        } else if format == image::ImageFormat::WebP {
                            "webp"
                        } else {
                            "png"
                        };
                        let name = format!(
                            "m{mode}-{label}-i{icc}-e{include_exif}-x{include}.{extension}"
                        );
                        let path = root.join(&name);
                        export(&doc, &path, format, &options).unwrap();
                        let opened = open(&path).unwrap();
                        assert_eq!((opened.document.width, opened.document.height), (9, 11));
                        assert_eq!(opened.document.source_metadata.xmp.is_some(), include);
                        if include {
                            let xmp = std::str::from_utf8(
                                opened.document.source_metadata.xmp.as_ref().unwrap(),
                            )
                            .unwrap();
                            let parsed = roxmltree::Document::parse(xmp).unwrap();
                            assert!(
                                !parsed
                                    .descendants()
                                    .any(|n| n.has_tag_name((XMP, "Thumbnails")))
                            );
                            assert!(xmp.contains("<!--keep packet comment-->"));
                            assert!(xmp.contains("<![CDATA[color & <unrelated>]]>"));
                            assert!(xmp.contains("촬영자"));
                        }
                        let decoded = crate::precision::composite(&opened.document);
                        if let Some(before) = &baseline {
                            assert_eq!(&decoded, before);
                        } else {
                            baseline = Some(decoded.clone());
                        }
                        if format != image::ImageFormat::Jpeg && lossless {
                            assert_eq!(decoded, crate::precision::composite(&doc));
                        }
                        let raw = format!("{name}.rgba32f.le");
                        std::fs::write(
                            root.join(&raw),
                            decoded
                                .as_raw()
                                .iter()
                                .flat_map(|v| v.to_le_bytes())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                        save_project(&opened.document, &root.join(format!("{name}.rphoto")))
                            .unwrap();
                        assert_eq!(
                            crate::precision::composite(
                                &load_project(&root.join(format!("{name}.rphoto"))).unwrap()
                            ),
                            decoded
                        );
                        assert!(Arc::ptr_eq(
                            &source,
                            doc.source_metadata.xmp.as_ref().unwrap()
                        ));
                        assert_eq!(source.as_slice(), original);
                        assert!(Arc::ptr_eq(&pixels, &doc.layers[0].pixels));
                        if let Some(high) = &high {
                            assert!(Arc::ptr_eq(high, doc.layers[0].precision.as_ref().unwrap()));
                        }
                        cases.push(serde_json::json!({"file":name,"raw":raw,"mode":mode,"label":label,"icc":icc,"exif":include_exif && format!=image::ImageFormat::Tiff,"exif_option":include_exif,"include":include,"depth":depth,"lossless":lossless,"big":mode%2==1}));
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 384);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn xmp_invalid_encoding_graph_extended_reference_and_jpeg_size_keep_destination() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/xmp-output/errors");
    std::fs::create_dir_all(&root).unwrap();
    let mut doc = crate::document::Document::new(3, 2, true).unwrap();
    let complex = format!(
        "<a:Orientation xmlns:a='{TIFF}' xmlns:rdf='{RDF}'><rdf:Seq><rdf:li>6</rdf:li></rdf:Seq></a:Orientation>"
    );
    let extended = "<rdf:Description xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#' xmlns:n='http://ns.adobe.com/xmp/note/' n:HasExtendedXMP='0123456789ABCDEF0123456789ABCDEF'/>";
    for (index, data) in [
        vec![255, 0],
        b"<unclosed>".to_vec(),
        complex.into_bytes(),
        extended.as_bytes().to_vec(),
        b"<!DOCTYPE r [<!ENTITY x '6'>]><r>&x;</r>".to_vec(),
    ]
    .into_iter()
    .enumerate()
    {
        doc.source_metadata.xmp = Some(Arc::new(data));
        for (label, format) in [
            ("png", image::ImageFormat::Png),
            ("jpg", image::ImageFormat::Jpeg),
            ("webp", image::ImageFormat::WebP),
            ("tiff", image::ImageFormat::Tiff),
        ] {
            let path = root.join(format!("invalid{index}.{label}"));
            std::fs::write(&path, b"original").unwrap();
            assert!(export(&doc, &path, format, &ExportOptions::default()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"original");
            export(
                &doc,
                &path,
                format,
                &ExportOptions {
                    include_xmp: false,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(open(&path).unwrap().document.source_metadata.xmp.is_none());
        }
    }
    let large = format!("<r><!--{}--></r>", "x".repeat(65510));
    doc.source_metadata.xmp = Some(Arc::new(large.into_bytes()));
    let path = root.join("too-large.jpg");
    std::fs::write(&path, b"original").unwrap();
    assert!(
        export(
            &doc,
            &path,
            image::ImageFormat::Jpeg,
            &ExportOptions::default()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert!(
        std::fs::read_dir(root).unwrap().all(|p| !p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
}
