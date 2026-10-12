use crate::{
    blend::BlendMode,
    canvas::View,
    document::{self, Brush, Document, Editor, PixelRegion, Selection},
    editing, files,
    icons::{self, Icon},
    recovery,
    vector::{Content, ShapeKind},
};
#[cfg(test)]
use eframe::egui::Modifiers;
use eframe::egui::{self, Color32, Key, PointerButton, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(90, 153, 245);
const CANVAS_BACKGROUND: Color32 = Color32::from_rgb(24, 27, 32);

fn photoshop_output_ui(ui: &mut egui::Ui, options: &mut crate::psd::OutputOptions) {
    egui::ComboBox::from_label("색 모드")
        .selected_text(options.color.label())
        .show_ui(ui, |ui| {
            for color in crate::psd::OutputColor::ALL {
                ui.selectable_value(&mut options.color, color, color.label());
            }
        });
    if options.color == crate::psd::OutputColor::Grayscale {
        ui.label("중성 RGB 레이어는 회색조 채널과 그룹·마스크·정밀도를 유지합니다. 컬러 결과는 현재 RGB 값의 밝기로 변환합니다. ICC 색상 변환은 아직 적용되지 않습니다.");
        ui.checkbox(&mut options.grayscale_layers, "컬러 레이어 구조 유지");
        ui.label(if options.grayscale_layers {"각 레이어를 회색조로 바꾸고 구조를 유지합니다. 컬러 블렌드 결과와 보정·효과의 외관이 합성 후 변환과 달라질 수 있습니다."}else{"컬러 결과가 있으면 전체 합성을 회색조 한 레이어로 저장해 변환 외관을 유지합니다. 원래 편집 구조는 .rphoto 프로젝트에 보존됩니다."});
    }
    egui::ComboBox::from_label("압축")
        .selected_text(options.compression.label())
        .show_ui(ui, |ui| {
            for compression in crate::psd_compression::Compression::ALL {
                ui.selectable_value(&mut options.compression, compression, compression.label());
            }
        });
}

pub(crate) fn curve_editor(
    ui: &mut egui::Ui,
    curve: &mut crate::adjustments::Curve,
    drag: &mut Option<usize>,
) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(320.0, 160.0), Sense::click_and_drag());
    let screen = |p: [f32; 2]| {
        Pos2::new(
            rect.left() + p[0] * rect.width(),
            rect.bottom() - p[1] * rect.height(),
        )
    };
    let point = response.interact_pointer_pos().map(|p| {
        [
            ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - p.y) / rect.height()).clamp(0.0, 1.0),
        ]
    });
    let nearest = response.interact_pointer_pos().and_then(|p| {
        curve
            .points
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let distance = screen(*v).distance(p);
                (distance <= 10.0).then_some((i, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|p| p.0)
    });
    if response.drag_started() {
        *drag = nearest;
    }
    if response.dragged()
        && let (Some(i), Some(mut p)) = (*drag, point)
    {
        p[0] = if i == 0 {
            0.0
        } else if i + 1 == curve.points.len() {
            1.0
        } else {
            p[0].clamp(
                curve.points[i - 1][0] + 0.001,
                curve.points[i + 1][0] - 0.001,
            )
        };
        curve.points[i] = p;
    }
    if response.drag_stopped() {
        *drag = None;
    }
    if response.double_clicked()
        && nearest.is_none()
        && curve.points.len() < 64
        && let Some(p) = point
    {
        let i = curve.points.partition_point(|v| v[0] < p[0]);
        if i > 0
            && i < curve.points.len()
            && p[0] - curve.points[i - 1][0] >= 0.001
            && curve.points[i][0] - p[0] >= 0.001
        {
            curve.points.insert(i, p);
        }
    }
    if response.secondary_clicked()
        && let Some(i) = nearest
        && i > 0
        && i + 1 < curve.points.len()
    {
        curve.points.remove(i);
        *drag = None;
    }
    ui.painter().rect_filled(rect, 0.0, Color32::from_gray(20));
    for n in 0..=4 {
        let t = n as f32 / 4.0;
        for line in [
            [screen([t, 0.0]), screen([t, 1.0])],
            [screen([0.0, t]), screen([1.0, t])],
        ] {
            ui.painter()
                .line_segment(line, Stroke::new(1.0, Color32::from_gray(55)));
        }
    }
    ui.painter().line_segment(
        [screen([0.0, 0.0]), screen([1.0, 1.0])],
        Stroke::new(1.0, Color32::GRAY),
    );
    for pair in curve.points.windows(2) {
        ui.painter()
            .line_segment([screen(pair[0]), screen(pair[1])], Stroke::new(2.0, ACCENT));
    }
    for p in &curve.points {
        ui.painter().circle_filled(screen(*p), 4.0, Color32::WHITE);
    }
    ui.label("더블 클릭: 점 추가 · 드래그: 조절 · 오른쪽 클릭: 점 삭제");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Tool {
    Move,
    Brush,
    Eraser,
    Select,
    Eyedropper,
    Hand,
    Zoom,
    Ellipse,
    Lasso,
    Wand,
    Crop,
    Fill,
    Stamp,
    Heal,
    Blur,
    Shape,
    Gradient,
    Text,
    Pen,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveTarget {
    Layers,
    Boundary,
    Pixels,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "manual brush performance diagnosis; run with --ignored --nocapture"]
    fn diagnose_brush_pipeline_costs() {
        use std::hint::black_box;
        fn time(apply: impl FnOnce()) -> f64 {
            let start = Instant::now();
            apply();
            start.elapsed().as_secs_f64() * 1000.0
        }
        fn median(mut apply: impl FnMut()) -> f64 {
            let mut values = [time(&mut apply), time(&mut apply), time(&mut apply)];
            values.sort_by(f64::total_cmp);
            values[1]
        }
        fn properties_frame(app: &mut PhotoEditor, ctx: &egui::Context, input: &egui::RawInput) {
            let mut output = ctx.run_ui(input.clone(), |ui| app.properties_ui(ui));
            output.textures_delta.clear();
            black_box(output);
        }
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let mut rows = Vec::new();
        for (width, height, depth, layers) in [
            (512, 512, 8, 1),
            (1920, 1080, 8, 1),
            (3840, 2160, 8, 1),
            (1920, 1080, 32, 1),
            (1920, 1080, 8, 4),
        ] {
            let mut doc = Document::new(width, height, false).unwrap();
            if depth > 8 {
                doc.layers[0].bit_depth = depth;
                doc.layers[0].precision = Some(Arc::new(image::Rgba32FImage::from_pixel(
                    width,
                    height,
                    image::Rgba([0.4, 0.3, 0.2, 1.0]),
                )));
            }
            for _ in 1..layers {
                doc.add_layer(
                    "layer".into(),
                    image::RgbaImage::from_pixel(width, height, image::Rgba([100, 80, 60, 128])),
                )
                .unwrap();
            }
            let center = [width as f32 / 2.0, height as f32 / 2.0];
            let end = [center[0] + 8.0, center[1] + 4.0];
            let brush = Brush {
                radius: 16.0,
                opacity: 1.0,
                hardness: 0.8,
                color: [20, 70, 200, 255],
                erase: false,
                mask: false,
            };
            let paint_unique_ms = time(|| {
                document::paint_line(doc.active_layer_mut().unwrap(), center, end, &brush, None)
            });
            let snapshot = doc.clone();
            let paint_shared_ms = time(|| {
                document::paint_line(
                    doc.active_layer_mut().unwrap(),
                    end,
                    [end[0] + 8.0, end[1] + 4.0],
                    &brush,
                    None,
                )
            });
            black_box(&snapshot);
            drop(snapshot);
            let mut composite = None;
            let composite_ms = median(|| composite = Some(black_box(doc.composite())));
            let composite = composite.unwrap();
            let color_image_ms = median(|| {
                black_box(egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    composite.as_raw(),
                ));
            });
            let thumbnail_ms = median(|| {
                black_box(image::imageops::thumbnail(
                    &*doc.active_layer().unwrap().pixels,
                    28,
                    28,
                ));
            });
            let histogram_ms = median(|| {
                let mut counts = [0u32; 256];
                for p in composite.pixels() {
                    counts[((u32::from(p[0]) * 54 + u32::from(p[1]) * 183 + u32::from(p[2]) * 19)
                        / 256) as usize] += 1;
                }
                black_box(counts);
            });
            let large_brush = Brush {
                radius: 150.0,
                ..brush
            };
            let paint_300_ms = time(|| {
                document::paint_line(
                    doc.active_layer_mut().unwrap(),
                    center,
                    end,
                    &large_brush,
                    None,
                )
            });
            let ctx = egui::Context::default();
            let mut app = PhotoEditor::from_context(&ctx);
            app.add_tab(files::Opened {
                source_stamp: None,
                document: doc.clone(),
                project_path: None,
                title: "diagnostic".into(),
                notes: vec![],
            });
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320.0, 860.0))),
                max_texture_side: Some(16384),
                ..Default::default()
            };
            properties_frame(&mut app, &ctx, &input);
            let properties_cached_ms = median(|| {
                properties_frame(&mut app, &ctx, &input);
            });
            app.tab_mut().unwrap().editor.begin("diagnostic stroke");
            document::paint_line(
                app.tab_mut()
                    .unwrap()
                    .editor
                    .document
                    .active_layer_mut()
                    .unwrap(),
                center,
                end,
                &brush,
                None,
            );
            let properties_invalidated_ms = time(|| {
                properties_frame(&mut app, &ctx, &input);
            });
            app.show_histogram = true;
            app.tab_mut().unwrap().histogram = Some(Rendered::new(composite.clone()).histogram);
            app.tab_mut().unwrap().composite = Some(composite);
            let properties_histogram_ms = median(|| {
                properties_frame(&mut app, &ctx, &input);
            });
            prepare_preview(&mut app, &ctx);
            app.tab_mut().unwrap().editor.finish();
            app.tab_mut()
                .unwrap()
                .editor
                .begin("timed continuous stroke");
            let mut stroke_times = Vec::new();
            for step in 0..20 {
                let from = [center[0] + step as f32 * 8.0, center[1] + step as f32 * 4.0];
                let to = [from[0] + 8.0, from[1] + 4.0];
                stroke_times.push(time(|| {
                    let tab = app.tab_mut().unwrap();
                    document::paint_line(
                        tab.editor.document.active_layer_mut().unwrap(),
                        from,
                        to,
                        &brush,
                        None,
                    );
                    tab.damage_stroke(from, to, brush.radius);
                    tab.update_painted_region();
                }));
                ctx.tex_manager().write().take_delta().clear();
            }
            let stroke_first_ms = stroke_times.remove(0);
            stroke_times.sort_by(f64::total_cmp);
            let stroke_preview_median_ms = stroke_times[stroke_times.len() / 2];
            let stroke_preview_max_ms = *stroke_times.last().unwrap();
            let row = serde_json::json!({ "profile": profile, "width": width, "height": height, "depth": depth, "layers": layers,
                "paint_unique_ms": paint_unique_ms, "paint_shared_ms": paint_shared_ms, "paint_300_ms": paint_300_ms,
                "composite_ms": composite_ms, "color_image_ms": color_image_ms, "thumbnail_ms": thumbnail_ms, "histogram_ms": histogram_ms,
                "properties_cached_ms": properties_cached_ms, "properties_invalidated_ms": properties_invalidated_ms, "properties_histogram_ms": properties_histogram_ms,
                "stroke_first_ms": stroke_first_ms, "stroke_preview_median_ms": stroke_preview_median_ms, "stroke_preview_max_ms": stroke_preview_max_ms });
            println!("{row}");
            rows.push(row);
        }
        let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/brush-diagnosis");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(format!("{profile}-after.json")),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn continuous_brush_presents_every_frame_while_background_preview_finishes() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        app.brush_size = 32.0;
        app.brush_smoothing = 0.0;
        let original = app.tab().unwrap().editor.document.composite();
        let texture = ctx.load_texture(
            "diagnostic",
            egui::ColorImage::from_rgba_unmultiplied([256, 256], original.as_raw()),
            egui::TextureOptions::LINEAR,
        );
        let tab = app.tab_mut().unwrap();
        tab.texture = Some(texture);
        tab.composite = Some(original.clone());
        tab.render_dirty = false;
        let start = Pos2::new(168.0, 168.0);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(start)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerButton {
                pos: start,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
        let mut displayed = 0;
        for step in 1..=16 {
            // Complete the previous snapshot immediately before the next input frame.
            // This deliberately removes slow rendering as a confounder for the scheduling rule.
            let previous = app.tab().unwrap().editor.document.composite();
            let (sender, receiver) = mpsc::channel();
            sender.send(Rendered::new(previous)).unwrap();
            app.tab_mut().unwrap().render_job = Some(receiver);
            app.tab_mut().unwrap().render_dirty = false;
            input_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![egui::Event::PointerMoved(
                    start + Vec2::new(step as f32 * 8.0, 0.0),
                )],
                Modifiers::NONE,
            );
            if app.tab().unwrap().composite.as_ref().unwrap() != &original {
                displayed += 1;
            }
            assert_eq!(
                app.tab().unwrap().composite.as_ref().unwrap(),
                &app.tab().unwrap().editor.document.composite(),
                "completed snapshot must include the newest stroke segment"
            );
        }
        let painted = app.tab().unwrap().editor.document.composite();
        let holes = (40..168)
            .filter(|x| painted.get_pixel(*x, 40).0 != app.color)
            .count();
        println!(
            "continuous input: presented_frames={displayed}/16, final_centerline_holes={holes}/128"
        );
        assert_eq!(holes, 0, "brush geometry left gaps in the actual pixels");
        assert_eq!(
            displayed, 16,
            "continuous brush input must not starve presentation"
        );
        let (sender, receiver) = mpsc::channel();
        sender.send(Rendered::new(painted)).unwrap();
        app.tab_mut().unwrap().render_job = Some(receiver);
        app.tab_mut().unwrap().render_dirty = false;
        input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        assert_ne!(app.tab().unwrap().composite.as_ref().unwrap(), &original);
        println!("quiet input frame: newest completed snapshot presented");
    }
    fn prepare_preview(app: &mut PhotoEditor, ctx: &egui::Context) {
        let rendered = Rendered::new(app.tab().unwrap().editor.document.composite());
        let tab = app.tab_mut().unwrap();
        tab.texture =
            Some(ctx.load_texture("test-preview", rendered.color, egui::TextureOptions::LINEAR));
        tab.composite = Some(rendered.pixels);
        tab.histogram = Some(rendered.histogram);
        tab.render_dirty = false;
        tab.render_job = None;
    }

    #[test]
    fn imported_svg_canvas_drag_insert_delete_escape_and_modal_protection() {
        let (mut app, ctx, mut clock) = input_app(Tool::Pen);
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><path id="main" fill="red" d="M40 40C60 20 100 60 140 40L140 140L40 140Z"/><path id="other" fill="blue" d="M180 40L220 40L200 80Z"/></svg>"#;
        let content = Content::Svg {
            source: source.into(),
        };
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .add_layer("SVG".into(), content.render().unwrap())
            .unwrap();
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .content = Some(content);
        app.tab_mut().unwrap().pen.drawing = false;
        drag_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(168., 168.),
            Pos2::new(180., 178.),
            Modifiers::NONE,
        );
        let Some(Content::Svg { source: changed }) = &app
            .tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .content
        else {
            panic!()
        };
        let e = crate::svg_paths::elements(changed).unwrap().remove(0);
        assert_eq!(e.paths[0].nodes[0].anchor, [52., 50.]);
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        let stable = changed.clone();
        app.command_panel.open = true;
        drag_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(180., 178.),
            Pos2::new(190., 190.),
            Modifiers::NONE,
        );
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .content,
            Some(Content::Svg {
                source: stable.clone()
            })
        );
        app.command_panel.open = false;
        let middle = crate::paths::point(&e.paths[0].nodes[0], &e.paths[0].nodes[1], 0.5);
        click_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(middle[0] + 128., middle[1] + 128.),
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        );
        let Some(Content::Svg { source: changed }) = &app
            .tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .content
        else {
            panic!()
        };
        assert_eq!(
            crate::svg_paths::elements(changed).unwrap()[0].paths[0]
                .nodes
                .len(),
            5
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::Delete)],
            Modifiers::NONE,
        );
        let Some(Content::Svg { source: changed }) = &app
            .tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .content
        else {
            panic!()
        };
        assert_eq!(
            crate::svg_paths::elements(changed).unwrap()[0].paths[0]
                .nodes
                .len(),
            4
        );
        let stable = changed.clone();
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(Pos2::new(180., 178.))],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerButton {
                    pos: Pos2::new(180., 178.),
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerMoved(Pos2::new(190., 190.)),
            ],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::Escape)],
            Modifiers::NONE,
        );
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .content,
            Some(Content::Svg { source: stable })
        );
        assert_eq!(app.tab().unwrap().editor.history().count(), 3);
        app.tab_mut().unwrap().editor.undo();
        app.tab_mut().unwrap().editor.undo();
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .content,
            Some(Content::Svg {
                source: source.into()
            })
        );
    }
    #[test]
    fn imported_svg_native_object_canvas_move_resize_escape_and_modal_protection() {
        let (mut app, ctx, mut clock) = input_app(Tool::Pen);
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><rect id="box" x="40" y="40" width="40" height="40" fill="red"/><circle id="keep" cx="200" cy="100" r="20" fill="blue"/></svg>"#;
        let content = Content::Svg {
            source: source.into(),
        };
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .add_layer("SVG".into(), content.render().unwrap())
            .unwrap();
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .content = Some(content);
        app.tab_mut().unwrap().pen.drawing = false;
        app.tab_mut().unwrap().pen.svg.object_mode = true;
        drag_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(188., 188.),
            Pos2::new(200., 198.),
            Modifiers::NONE,
        );
        let layer = app.tab().unwrap().editor.document.active_layer().unwrap();
        let Some(Content::Svg { source: changed }) = &layer.content else {
            panic!()
        };
        let xml = roxmltree::Document::parse(changed).unwrap();
        let n = xml
            .descendants()
            .find(|n| n.attribute("id") == Some("box"))
            .unwrap();
        assert_eq!(n.tag_name().name(), "rect");
        assert_eq!(n.attribute("x"), Some("40"));
        let e = crate::svg_objects::elements(changed).unwrap().remove(0);
        assert!((e.bounds[0] - 52.).abs() < 0.001);
        assert!((e.bounds[1] - 50.).abs() < 0.001);
        let stable = changed.clone();
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        app.command_panel.open = true;
        drag_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(200., 198.),
            Pos2::new(210., 208.),
            Modifiers::NONE,
        );
        app.command_panel.open = false;
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .content,
            Some(Content::Svg {
                source: stable.clone()
            })
        );
        drag_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(220., 218.),
            Pos2::new(240., 238.),
            Modifiers::NONE,
        );
        let layer = app.tab().unwrap().editor.document.active_layer().unwrap();
        let Some(Content::Svg { source: changed }) = &layer.content else {
            panic!()
        };
        let e = crate::svg_objects::elements(changed).unwrap().remove(0);
        assert!((e.bounds[2] - 112.).abs() < 0.001);
        assert!((e.bounds[3] - 110.).abs() < 0.001);
        let stable = changed.clone();
        assert_eq!(app.tab().unwrap().editor.history().count(), 2);
        let start = Pos2::new(200., 198.);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(Pos2::new(210., 208.))],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::Escape)],
            Modifiers::NONE,
        );
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .content,
            Some(Content::Svg { source: stable })
        );
        assert_eq!(app.tab().unwrap().editor.history().count(), 2);
        app.tab_mut().unwrap().editor.undo();
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .content,
            Some(Content::Svg {
                source: source.into()
            })
        );
    }
    #[test]
    fn pen_canvas_click_drag_close_edit_and_tool_switch_save_one_undo() {
        let (mut app, ctx, mut clock) = input_app(Tool::Pen);
        let start = Pos2::new(168.0, 168.0);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(start)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerMoved(start + Vec2::new(20.0, -10.0)),
                egui::Event::PointerButton {
                    pos: start + Vec2::new(20.0, -10.0),
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        click_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(250.0, 200.0),
            Modifiers::NONE,
        );
        assert_eq!(app.tab().unwrap().pen.draft.len(), 2);
        assert_eq!(app.tab().unwrap().pen.draft[0].outgoing, [60.0, 30.0]);
        app.set_tool(Tool::Move);
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        assert!(matches!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .content,
            Some(Content::Path { closed: false, .. })
        ));
        app.set_tool(Tool::Pen);
        app.tab_mut().unwrap().pen.drawing = true;
        click_at(&mut app, &ctx, &mut clock, start, Modifiers::NONE);
        click_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(250.0, 168.0),
            Modifiers::NONE,
        );
        click_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(210.0, 240.0),
            Modifiers::NONE,
        );
        click_at(&mut app, &ctx, &mut clock, start, Modifiers::NONE);
        let tab = app.tab().unwrap();
        assert!(tab.pen.draft.is_empty());
        assert_eq!(tab.editor.history().count(), 2);
        assert!(matches!(
            tab.editor.document.active_layer().unwrap().content,
            Some(Content::Path { closed: true, .. })
        ));
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 2);
        app.tab_mut().unwrap().pen.drawing = true;
        app.tab_mut().unwrap().selection = Some(Selection::rectangle([10, 10], [40, 40]));
        click_at(&mut app, &ctx, &mut clock, start, Modifiers::NONE);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::Escape)],
            Modifiers::NONE,
        );
        assert!(app.tab().unwrap().pen.draft.is_empty());
        assert!(app.tab().unwrap().selection.is_some());
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 2);
    }

    #[test]
    fn brush_and_eraser_upload_only_changed_region_with_hdr_mask_selection_and_undo() {
        for (tool, mask) in [
            (Tool::Brush, false),
            (Tool::Eraser, false),
            (Tool::Brush, true),
            (Tool::Eraser, true),
        ] {
            let (mut app, ctx, mut clock) = input_app(tool);
            app.brush_size = 32.0;
            {
                let tab = app.tab_mut().unwrap();
                tab.edit_mask = mask;
                tab.selection = Some(Selection::rectangle([30, 30], [120, 100]));
                let layer = tab.editor.document.active_layer_mut().unwrap();
                layer.precision = Some(Arc::new(image::Rgba32FImage::from_pixel(
                    256,
                    256,
                    image::Rgba([1.5, 0.12, 0.4, 0.8]),
                )));
                layer.bit_depth = 32;
                layer.transform.angle = 19.0;
                layer.transform.scale = [-0.85, 1.2];
                layer.offset = [5, -3];
                layer.mask = Some(Arc::new(image::GrayImage::from_pixel(
                    256,
                    256,
                    image::Luma([128]),
                )));
                layer.adjustments.exposure = 0.3;
                layer.adjustments.vignette = 0.2;
            }
            prepare_preview(&mut app, &ctx);
            let original = app.tab().unwrap().editor.document.composite();
            ctx.tex_manager().write().take_delta().clear();
            let start = Pos2::new(178.0, 178.0);
            input_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![egui::Event::PointerMoved(start)],
                Modifiers::NONE,
            );
            input_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![
                    egui::Event::PointerMoved(start),
                    egui::Event::PointerButton {
                        pos: start,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                ],
                Modifiers::NONE,
            );
            for step in 1..=4 {
                clock += 0.016;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        time: Some(clock),
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(512.0))),
                        events: vec![egui::Event::PointerMoved(
                            start + Vec2::new(step as f32 * 8.0, 6.0),
                        )],
                        ..Default::default()
                    },
                    |ui| app.canvas_ui(ui, &ctx),
                );
                let texture = app.tab().unwrap().texture.as_ref().unwrap().id();
                let updates: Vec<_> = output
                    .textures_delta
                    .set
                    .iter()
                    .filter(|(id, _)| **id == texture)
                    .flat_map(|(_, updates)| updates.iter())
                    .collect();
                let summaries: Vec<_> = updates.iter().map(|u| (u.pos, u.image.size())).collect();
                output.textures_delta.clear();
                assert_eq!(summaries.len(), 1);
                assert!(summaries[0].0.is_some(), "brush uploaded the full texture");
                assert!(
                    summaries[0].1[0] < 80 && summaries[0].1[1] < 80,
                    "{tool:?} mask={mask} {summaries:?}"
                );
                let tab = app.tab().unwrap();
                assert!(
                    tab.render_job.is_none(),
                    "brush launched a full document render"
                );
                assert_eq!(
                    tab.composite.as_ref().unwrap(),
                    &tab.editor.document.composite()
                );
                assert_eq!(
                    tab.histogram.unwrap(),
                    Rendered::new(tab.editor.document.composite()).histogram
                );
            }
            let end = start + Vec2::new(32.0, 6.0);
            input_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
                Modifiers::NONE,
            );
            assert_eq!(app.tab().unwrap().editor.history().count(), 1);
            app.tab_mut().unwrap().editor.undo();
            assert_eq!(app.tab().unwrap().editor.document.composite(), original);
        }
    }

    #[test]
    fn brush_waits_for_pending_full_edit_then_replays_all_stroke_damage() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        prepare_preview(&mut app, &ctx);
        let original = app.tab().unwrap().composite.clone().unwrap();
        app.tab_mut().unwrap().editor.edit("full edit", |doc| {
            editing::fill(
                doc.active_layer_mut().unwrap(),
                None,
                [220, 40, 20, 255],
                false,
            );
        });
        let full_edit = app.tab().unwrap().editor.document.composite();
        let (sender, receiver) = mpsc::channel();
        app.tab_mut().unwrap().render_job = Some(receiver);
        let start = Pos2::new(168.0, 168.0);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(start)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        assert!(app.tab().unwrap().render_job.is_some());
        assert_eq!(app.tab().unwrap().composite.as_ref().unwrap(), &original);
        sender.send(Rendered::new(full_edit)).unwrap();
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(start + Vec2::new(20.0, 0.0))],
            Modifiers::NONE,
        );
        let tab = app.tab().unwrap();
        assert_eq!(
            tab.composite.as_ref().unwrap(),
            &tab.editor.document.composite()
        );
        assert_eq!(
            tab.composite.as_ref().unwrap().get_pixel(200, 200).0,
            [220, 40, 20, 255]
        );
        assert!(tab.render_job.is_none());
    }

    #[test]
    fn effects_preview_progresses_during_stroke_and_converges_after_release() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .effects
            .shadow
            .enabled = true;
        prepare_preview(&mut app, &ctx);
        let start = Pos2::new(168.0, 168.0);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(start)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        for step in 1..=3 {
            let previous = app.tab().unwrap().editor.document.composite();
            let (sender, receiver) = mpsc::channel();
            sender.send(Rendered::new(previous.clone())).unwrap();
            app.tab_mut().unwrap().render_job = Some(receiver);
            app.tab_mut().unwrap().render_dirty = false;
            input_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![egui::Event::PointerMoved(
                    start + Vec2::new(step as f32 * 8.0, 0.0),
                )],
                Modifiers::NONE,
            );
            assert_eq!(app.tab().unwrap().composite.as_ref().unwrap(), &previous);
            assert!(app.tab().unwrap().render_job.is_some());
        }
        let end = start + Vec2::new(24.0, 0.0);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerButton {
                pos: end,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
        let final_image = app.tab().unwrap().editor.document.composite();
        let (sender, receiver) = mpsc::channel();
        sender.send(Rendered::new(final_image.clone())).unwrap();
        app.tab_mut().unwrap().render_job = Some(receiver);
        app.tab_mut().unwrap().render_dirty = false;
        app.tab_mut().unwrap().paint_damage = None;
        input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        assert_eq!(app.tab().unwrap().composite.as_ref().unwrap(), &final_image);
    }

    #[test]
    fn properties_with_mask_controls_stay_outside_canvas_at_small_and_large_window_sizes() {
        for size in [[880.0, 620.0], [1320.0, 860.0]] {
            let (mut app, ctx, _) = input_app(Tool::Move);
            let doc = &mut app.tabs[0].editor.document;
            doc.group_active().unwrap();
            doc.active_layer_mut().unwrap().mask = Some(Arc::new(image::GrayImage::from_pixel(
                256,
                256,
                image::Luma([128]),
            )));
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(size[0], size[1]))),
                ..Default::default()
            };
            for _ in 0..3 {
                let mut output = ctx.run_ui(input.clone(), |ui| {
                    app.top_ui(ui, &ctx);
                    app.tools_ui(ui);
                    app.properties_ui(ui);
                    app.canvas_ui(ui, &ctx);
                });
                let canvas_right = output
                    .shapes
                    .iter()
                    .filter_map(|clipped| match &clipped.shape {
                        egui::Shape::Rect(rect) if rect.fill == CANVAS_BACKGROUND => {
                            Some(rect.rect.right())
                        }
                        _ => None,
                    })
                    .fold(0.0, f32::max);
                let heading_left = output
                    .shapes
                    .iter()
                    .find_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(text) if text.galley.job.text == "문서" => {
                            Some(text.pos.x)
                        }
                        _ => None,
                    })
                    .unwrap();
                assert!(canvas_right > 100.0);
                assert!(
                    heading_left >= canvas_right,
                    "properties heading {heading_left} covered by canvas ending at {canvas_right}, window {size:?}"
                );
                output.textures_delta.clear();
            }
        }
    }
    #[test]
    fn tool_groups_cover_all_tools_and_retain_selection_without_document_edits() {
        let (mut app, _, _) = input_app(Tool::Move);
        assert_eq!(Tool::entries().len(), 19);
        for group in 0..10 {
            let members = Tool::entries()
                .into_iter()
                .filter(|(t, _, _)| t.group() == group)
                .collect::<Vec<_>>();
            assert!(!members.is_empty());
            for (tool, _, _) in members {
                app.set_tool(tool);
                app.set_tool(Tool::Move);
                if group != 0 {
                    assert_eq!(
                        Tool::group_tool(group, app.tool, &app.workspace.layout.remembered),
                        tool
                    );
                }
            }
        }
        assert_eq!(app.workspace.layout.remembered.len(), 10);
        assert!(app.workspace.layout.valid());
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        let source = serde_json::to_vec(&app.workspace.layout).unwrap();
        let loaded: crate::workspace::Layout = serde_json::from_slice(&source).unwrap();
        assert_eq!(loaded.remembered, app.workspace.layout.remembered);
    }
    #[test]
    fn tool_group_arrow_opens_choices_and_click_switches_tool() {
        let (mut app, ctx, mut time) = input_app(Tool::Move);
        fn draw(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            time: &mut f64,
            events: Vec<egui::Event>,
        ) -> Vec<(String, Pos2)> {
            *time += 0.4;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*time),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(512.))),
                    events,
                    ..Default::default()
                },
                |ui| app.tools_ui(ui),
            );
            let text = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((t.galley.job.text.clone(), t.pos)),
                    _ => None,
                })
                .collect();
            out.textures_delta.clear();
            text
        }
        let text = draw(&mut app, &ctx, &mut time, vec![]);
        let at = text.iter().find(|(s, _)| s == "▾").unwrap().1 + Vec2::splat(4.);
        draw(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::PointerMoved(at)],
        );
        let events = vec![
            egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ];
        draw(&mut app, &ctx, &mut time, events);
        let text = draw(&mut app, &ctx, &mut time, vec![]);
        let at = text
            .iter()
            .find(|(s, _)| s.starts_with("타원 선택"))
            .unwrap()
            .1
            + Vec2::splat(4.);
        draw(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::PointerMoved(at)],
        );
        draw(
            &mut app,
            &ctx,
            &mut time,
            vec![
                egui::Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        assert_eq!(app.tool, Tool::Ellipse);
    }
    #[test]
    fn canvas_only_f_hides_panels_preserves_preferences_and_restores_fullscreen() {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            time: &mut f64,
            mut events: Vec<egui::Event>,
            fullscreen: bool,
        ) -> egui::FullOutput {
            let releases: Vec<_> = events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Key {
                        pressed: true,
                        repeat: false,
                        ..
                    } = event
                    {
                        let mut release = event.clone();
                        if let egui::Event::Key { pressed, .. } = &mut release {
                            *pressed = false;
                        }
                        Some(release)
                    } else {
                        None
                    }
                })
                .collect();
            events.extend(releases);
            *time += 0.1;
            let mut raw = egui::RawInput {
                time: Some(*time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                events,
                ..Default::default()
            };
            raw.viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .fullscreen = Some(fullscreen);
            ctx.run_ui(raw, |ui| {
                app.shortcuts(ctx);
                app.workspace_ui(ui, ctx)
            })
        }
        for dpi in [1., 2.] {
            for fullscreen in [false, true] {
                let (mut app, ctx, mut time) = input_app(Tool::Move);
                ctx.set_pixels_per_point(dpi);
                app.workspace.layout.left = true;
                app.workspace.layout.width = 410.;
                app.workspace.layout.colors = true;
                app.workspace.layout.navigator = true;
                app.tab_mut().unwrap().editor.document.layout.rulers = true;
                let layout = app.workspace.layout.clone();
                let pixels = app.tab().unwrap().editor.document.layers[0].pixels.clone();
                let zoom = app.tab().unwrap().view.zoom;
                let pan = app.tab().unwrap().view.pan;
                for _ in 0..3 {
                    let mut out = frame(&mut app, &ctx, &mut time, vec![], fullscreen);
                    out.textures_delta.clear();
                }
                let mut out = frame(
                    &mut app,
                    &ctx,
                    &mut time,
                    vec![key_event(Key::F)],
                    fullscreen,
                );
                let command = out.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .iter()
                    .any(|c| matches!(c, egui::ViewportCommand::Fullscreen(true)));
                let area = out.shapes.iter().find_map(|s| match &s.shape {
                    egui::Shape::Rect(r) if r.fill == Color32::BLACK => Some(r.rect),
                    _ => None,
                });
                let hidden = !out
                    .shapes
                    .iter()
                    .any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="문서"));
                out.textures_delta.clear();
                assert!(app.canvas_only && command && hidden);
                assert!(area.unwrap().width() > 879.);
                assert!(area.unwrap().height() > 619.);
                assert_eq!(app.workspace.layout, layout);
                assert_eq!(app.tab().unwrap().view.zoom, zoom);
                assert_eq!(app.tab().unwrap().view.pan, pan);
                let mut out = frame(&mut app, &ctx, &mut time, vec![key_event(Key::F)], true);
                let command=out.viewport_output[&egui::ViewportId::ROOT].commands.iter().any(|c|matches!(c,egui::ViewportCommand::Fullscreen(value) if *value==fullscreen));
                out.textures_delta.clear();
                assert!(!app.canvas_only && command);
                assert_eq!(app.workspace.layout, layout);
                assert!(Arc::ptr_eq(
                    &pixels,
                    &app.tab().unwrap().editor.document.layers[0].pixels
                ));
                assert_eq!(app.tab().unwrap().editor.history().count(), 0);
            }
        }
    }
    #[test]
    fn canvas_only_custom_key_modal_guard_and_escape_fallback() {
        fn press(key: Key) -> Vec<egui::Event> {
            let down = key_event(key);
            let mut up = down.clone();
            if let egui::Event::Key { pressed, .. } = &mut up {
                *pressed = false;
            }
            vec![down, up]
        }
        let (mut app, ctx, mut time) = input_app(Tool::Crop);
        app.workspace
            .layout
            .shortcuts
            .assign(
                crate::shortcuts::Action::CanvasOnly,
                Some((Key::F9, Modifiers::NONE)),
            )
            .unwrap();
        input_frame(&mut app, &ctx, &mut time, press(Key::F), Modifiers::NONE);
        assert!(!app.canvas_only);
        app.new_dialog = true;
        input_frame(&mut app, &ctx, &mut time, press(Key::F9), Modifiers::NONE);
        assert!(!app.canvas_only);
        app.new_dialog = false;
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![key_event(Key::F9)],
            Modifiers::NONE,
        );
        assert!(app.canvas_only);
        let mut repeat = key_event(Key::F9);
        if let egui::Event::Key { repeat: value, .. } = &mut repeat {
            *value = true;
        }
        input_frame(&mut app, &ctx, &mut time, vec![repeat], Modifiers::NONE);
        assert!(app.canvas_only);
        let mut released = key_event(Key::F9);
        if let egui::Event::Key { pressed, .. } = &mut released {
            *pressed = false;
        }
        input_frame(&mut app, &ctx, &mut time, vec![released], Modifiers::NONE);
        app.colors_panel.open = true;
        input_frame(&mut app, &ctx, &mut time, press(Key::F9), Modifiers::NONE);
        assert!(!app.canvas_only);
        assert!(app.colors_panel.open);
        app.colors_panel.open = false;
        app.toggle_canvas_only(&ctx);

        app.workspace
            .layout
            .shortcuts
            .assign(crate::shortcuts::Action::CanvasOnly, None)
            .unwrap();
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            press(Key::Escape),
            Modifiers::NONE,
        );
        assert!(!app.canvas_only);
    }
    #[test]
    fn docked_hidden_and_wide_panels_preserve_canvas_space_at_multiple_dpi() {
        for dpi in [1., 2.] {
            for left in [false, true] {
                let (mut app, ctx, _) = input_app(Tool::Move);
                ctx.set_pixels_per_point(dpi);
                app.workspace.layout.left = left;
                app.workspace.layout.width = 500.;
                app.workspace.layout.colors = true;
                app.workspace.layout.navigator = true;
                let raw = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                    ..Default::default()
                };
                for _ in 0..3 {
                    let mut output = ctx.run_ui(raw.clone(), |ui| {
                        app.top_ui(ui, &ctx);
                        app.tools_ui(ui);
                        app.properties_ui(ui);
                        app.canvas_ui(ui, &ctx);
                    });
                    let canvas = output
                        .shapes
                        .iter()
                        .find_map(|c| match &c.shape {
                            egui::Shape::Rect(r) if r.fill == CANVAS_BACKGROUND => Some(r.rect),
                            _ => None,
                        })
                        .unwrap();
                    let heading = output
                        .shapes
                        .iter()
                        .find_map(|c| match &c.shape {
                            egui::Shape::Text(t) if t.galley.job.text == "문서" => Some(t.pos),
                            _ => None,
                        })
                        .unwrap();
                    assert!(canvas.width() > 230., "{canvas:?}");
                    if left {
                        assert!(heading.x < canvas.left());
                    } else {
                        assert!(heading.x >= canvas.right());
                    }
                    output.textures_delta.clear();
                }
                app.workspace.layout.colors = false;
                app.workspace.layout.navigator = false;
                app.workspace.layout.layers = false;
                app.workspace.layout.properties = false;
                app.workspace.layout.history = false;
                let mut output = ctx.run_ui(raw, |ui| {
                    app.tools_ui(ui);
                    app.properties_ui(ui);
                    app.canvas_ui(ui, &ctx);
                });
                assert!(
                    !output.shapes.iter().any(
                        |c| matches!(&c.shape,egui::Shape::Text(t)if t.galley.job.text=="문서")
                    )
                );
                output.textures_delta.clear();
            }
        }
    }
    #[test]
    fn document_layer_transfer_rolls_back_failed_import_and_both_documents_can_undo() {
        let (mut app, _ctx, _) = input_app(Tool::Move);
        let mut source = crate::precision::document(
            "HDR".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                3,
                2,
                image::Rgba([2.125, -0.2, 0.3, 1.0]),
            )),
        )
        .unwrap();
        let child = source.active;
        source.group_active().unwrap();
        let group = source.active;
        source.active_layer_mut().unwrap().mask = Some(Arc::new(image::GrayImage::from_pixel(
            3,
            2,
            image::Luma([128]),
        )));
        let before = crate::precision::composite(&source);
        app.tabs[0].editor = Editor::new(source, true);
        let payload = LayerDrag {
            tab: app.tabs[0].id,
            ids: vec![group, child],
        };
        let mut destination = Document::new(3, 2, true).unwrap();
        destination.group_active().unwrap();
        destination.active_layer_mut().unwrap().locked = true;
        app.add_tab(files::Opened {
            source_stamp: None,
            document: destination,
            project_path: None,
            title: "대상".into(),
            notes: vec![],
        });
        let destination_layers = app.tabs[1].editor.document.layers.len();
        app.drop_layers(&payload, 1, None, false);
        assert!(app.message.is_some());
        assert_eq!(app.tabs[0].editor.document.layers.len(), 2);
        assert_eq!(app.tabs[1].editor.document.layers.len(), destination_layers);
        app.tabs[1]
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .locked = false;
        app.drop_layers(&payload, 1, None, true);
        assert_eq!(app.tabs[0].editor.document.layers.len(), 2); // Alt-copy retains the source.
        assert_eq!(
            crate::precision::composite(&app.tabs[1].editor.document),
            before
        );
        app.tabs[1].editor.undo();
        app.drop_layers(&payload, 1, None, false);
        assert_eq!(app.tabs[0].editor.document.layers.len(), 1);
        assert_eq!(app.tabs[0].editor.document.layers[0].bit_depth, 32);
        assert_eq!(
            crate::precision::composite(&app.tabs[1].editor.document),
            before
        );
        app.tabs[0].editor.undo();
        app.tabs[1].editor.undo();
        assert_eq!(
            crate::precision::composite(&app.tabs[0].editor.document),
            before
        );
        assert_eq!(app.tabs[1].editor.document.layers.len(), destination_layers);
        app.tabs[0].editor.redo();
        app.tabs[1].editor.redo();
        assert_eq!(
            crate::precision::composite(&app.tabs[1].editor.document),
            before
        );
    }
    #[test]
    fn program_info_help_menu_shortcuts_modal_guards_and_no_document() {
        use crate::commands::Command;
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let revision = app.tab().unwrap().editor.revision;
        let source = app
            .tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .pixels
            .clone();
        app.workspace
            .layout
            .shortcuts
            .assign(
                Command::About.shortcut_action().unwrap(),
                Some((Key::F8, Modifiers::NONE)),
            )
            .unwrap();
        app.workspace
            .layout
            .shortcuts
            .assign(
                Command::Licenses.shortcut_action().unwrap(),
                Some((Key::F9, Modifiers::NONE)),
            )
            .unwrap();
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::F8)],
            Modifiers::NONE,
        );
        assert!(app.about_panel.open);
        app.execute_command(Command::NewLayer, &ctx);
        click_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(100., 100.),
            Modifiers::NONE,
        );
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 1);
        assert_eq!(app.tab().unwrap().editor.revision, revision);
        assert!(Arc::ptr_eq(
            &source,
            &app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .pixels
        ));
        assert!(!app.tab().unwrap().editor.can_undo());
        app.canvas_only = true;
        assert!(app.command_blocked());
        app.about_panel.open = false;
        app.canvas_only = false;
        app.tabs.clear();
        assert!(app.command_reason(Command::About).is_none());
        assert!(app.command_reason(Command::Licenses).is_none());
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::F9)],
            Modifiers::NONE,
        );
        assert!(app.about_panel.open);
        app.about_panel.open = false;
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
        ) -> Vec<(String, Rect)> {
            *clock += 0.1;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    app.top_ui(ui, ctx);
                    app.about_panel.ui(ctx);
                },
            );
            let labels = out
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some((
                            t.galley.job.text.clone(),
                            Rect::from_min_size(t.pos, t.galley.size()),
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            out.textures_delta.clear();
            labels
        }
        for target in ["도움말", "프로그램 정보…"] {
            frame(&mut app, &ctx, &mut clock, vec![]);
            let labels = frame(&mut app, &ctx, &mut clock, vec![]);
            let at = labels.iter().find(|(s, _)| s == target).unwrap().1.center();
            for pressed in [true, false] {
                frame(
                    &mut app,
                    &ctx,
                    &mut clock,
                    vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            button: PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        assert!(app.about_panel.open);
        let labels = frame(&mut app, &ctx, &mut clock, vec![]);
        assert!(
            labels
                .iter()
                .any(|(s, _)| s.contains(env!("CARGO_PKG_VERSION")))
        );
        frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Escape)]);
        assert!(!app.about_panel.open);
        assert!(app.tabs.is_empty());
    }
    include!("menu_app_tests.rs");
    include!("icc_output_app_tests.rs");
    include!("options_app_tests.rs");
    include!("raw_app_tests.rs");
    include!("raw_advanced_app_tests.rs");
    include!("svg_image_app_tests.rs");
    include!("raw_white_balance_app_tests.rs");
    include!("color_mixer_app_tests.rs");
    include!("color_grading_app_tests.rs");
    include!("denoise_app_tests.rs");
    include!("detail_sharpen_app_tests.rs");
    include!("lens_profile_app_tests.rs");
    fn command_frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        clock: &mut f64,
        events: Vec<egui::Event>,
    ) {
        *clock += 0.4;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(*clock),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(640.0))),
                events,
                ..Default::default()
            },
            |ui| {
                app.shortcuts(ctx);
                app.canvas_ui(ui, ctx);
                app.command_ui(ctx);
            },
        );
        output.textures_delta.clear();
    }
    #[test]
    fn legacy_photoshop_open_command_background_import_and_guards_preserve_original() {
        use crate::commands::Command;
        let (mut app, ctx, _) = input_app(Tool::Move);
        assert!(app.command_reason(Command::LegacyPhotoshop).is_none());
        let command = app
            .command_entries()
            .into_iter()
            .find(|e| e.command == Command::LegacyPhotoshop)
            .unwrap();
        assert_eq!(command.category, "파일");
        app.busy = Some("검사");
        assert!(app.command_reason(Command::LegacyPhotoshop).is_some());
        app.busy = None;
        app.message = Some(("검사".into(), String::new()));
        assert!(app.command_reason(Command::LegacyPhotoshop).is_some());
        app.message = None;
        app.transforming = true;
        assert!(app.command_reason(Command::LegacyPhotoshop).is_some());
        app.transforming = false;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/rgb-psd-order/legacy-d32-c3-b1-g0.psb");
        let original = std::fs::read(&path).unwrap();
        let count = app.tabs.len();
        // Supplying the path exercises the real background import without a native file dialog.
        app.open_file_impl(Some(path.clone()), None, &ctx, true);
        let started = Instant::now();
        while app.busy.is_some() && started.elapsed() < Duration::from_secs(10) {
            app.poll_jobs(&ctx);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(app.busy.is_none());
        assert_eq!(app.tabs.len(), count + 1);
        let tab = app.tab().unwrap();
        assert!(tab.path.is_none());
        assert!(!tab.editor.can_undo());
        assert_eq!(
            tab.editor
                .document
                .layers
                .iter()
                .map(|l| l.id)
                .collect::<Vec<_>>(),
            vec![12, 11]
        );
        assert!(app.message.as_ref().unwrap().1.contains("이전 개발"));
        assert_eq!(std::fs::read(path).unwrap(), original);
        app.message = None;
        app.tabs.clear();
        assert!(app.command_reason(Command::LegacyPhotoshop).is_none());
    }
    #[test]
    fn command_search_keyboard_executes_tools_and_undoable_commands() {
        use crate::{commands::Command, shortcuts::Action};
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.open_commands();
        app.command_panel.query = "도구".into();
        command_frame(&mut app, &ctx, &mut clock, vec![]);
        let tools: Vec<_> = app
            .command_entries()
            .into_iter()
            .filter(|e| e.category == "도구")
            .collect();
        let mut down = key_event(Key::ArrowDown);
        command_frame(&mut app, &ctx, &mut clock, vec![down.clone()]);
        if let egui::Event::Key { pressed, .. } = &mut down {
            *pressed = false;
        }
        command_frame(&mut app, &ctx, &mut clock, vec![down]);
        command_frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Enter)]);
        assert!(!app.command_panel.open);
        let Command::Action(Action::Tool(expected)) = tools[1].command else {
            panic!()
        };
        assert_eq!(app.tool, expected);
        let count = app.tab().unwrap().editor.document.layers.len();
        app.execute_command(Command::NewLayer, &ctx);
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), count + 1);
        app.execute_command(Command::Action(Action::Undo), &ctx);
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), count);
        app.execute_command(Command::Action(Action::Redo), &ctx);
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), count + 1);
    }
    #[test]
    fn command_search_disabled_context_escape_and_canvas_guard() {
        use crate::{commands::Command, shortcuts::Action};
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        assert!(app.command_reason(Command::Crop).is_some());
        assert!(app.command_reason(Command::Action(Action::Undo)).is_some());
        let before = app.tab().unwrap().editor.document.clone();
        app.execute_command(Command::Crop, &ctx);
        assert_eq!(
            crate::precision::composite(&app.tab().unwrap().editor.document),
            crate::precision::composite(&before)
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layers.len(),
            before.layers.len()
        );
        assert!(!app.tab().unwrap().editor.can_undo());
        app.open_commands();
        app.command_panel.query = "PSD".into();
        command_frame(&mut app, &ctx, &mut clock, vec![]);
        click_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(256.0, 256.0),
            Modifiers::NONE,
        );
        assert_eq!(
            crate::precision::composite(&app.tab().unwrap().editor.document),
            crate::precision::composite(&before)
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layers.len(),
            before.layers.len()
        );
        assert!(!app.tab().unwrap().editor.can_undo());
        command_frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Escape)]);
        assert!(!app.command_panel.open);
        assert_eq!(
            crate::precision::composite(&app.tab().unwrap().editor.document),
            crate::precision::composite(&before)
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layers.len(),
            before.layers.len()
        );
        assert!(!app.tab().unwrap().editor.can_undo());
        app.message = Some(("입력 중".into(), String::new()));
        app.open_commands();
        assert!(!app.command_panel.open);
        app.message = None;
        app.transforming = true;
        app.open_commands();
        assert!(!app.command_panel.open);
        app.transforming = false;
        app.busy = Some("검사");
        for entry in app.command_entries() {
            assert!(entry.disabled.is_some());
            app.execute_command(entry.command, &ctx);
        }
        assert_eq!(
            crate::precision::composite(&app.tab().unwrap().editor.document),
            crate::precision::composite(&before)
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layers.len(),
            before.layers.len()
        );
        assert!(!app.tab().unwrap().editor.can_undo());
        app.busy = None;
        app.tabs.clear();
        assert!(app.command_reason(Command::Action(Action::New)).is_none());
        assert!(app.command_reason(Command::Psb).is_some());
    }
    #[test]
    fn command_search_ime_confirmation_does_not_execute_command() {
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.open_commands();
        app.command_panel.query = "새 문서".into();
        command_frame(&mut app, &ctx, &mut clock, vec![]);
        command_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::Ime(egui::ImeEvent::Preedit {
                    text: "새".into(),
                    active_range_chars: None,
                }),
                key_event(Key::Enter),
            ],
        );
        assert!(app.command_panel.open);
        assert!(!app.new_dialog);
        let mut up = key_event(Key::Enter);
        if let egui::Event::Key { pressed, .. } = &mut up {
            *pressed = false;
        }
        command_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                up.clone(),
                egui::Event::Ime(egui::ImeEvent::Commit("새".into())),
            ],
        );
        assert!(app.command_panel.open);
        assert!(!app.new_dialog);
        app.command_panel.query = "새 문서".into();
        command_frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Enter)]);
        assert!(app.new_dialog);
        assert!(!app.command_panel.open);
    }
    #[test]
    fn command_search_pointer_selection_opens_save_options() {
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.open_commands();
        app.command_panel.query = "PSD".into();
        command_frame(&mut app, &ctx, &mut clock, vec![]);
        clock += 0.4;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(clock),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(640.0))),
                ..Default::default()
            },
            |ui| {
                app.canvas_ui(ui, &ctx);
                app.command_ui(&ctx);
            },
        );
        let position = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text.contains("PSD로 저장") => {
                    Some(t.pos + Vec2::new(10.0, 5.0))
                }
                _ => None,
            })
            .unwrap();
        output.textures_delta.clear();
        command_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        command_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerButton {
                pos: position,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert_eq!(app.psd_dialog, Some(false));
        assert!(!app.command_panel.open);
        assert!(!app.tab().unwrap().editor.can_undo());
    }
    #[test]
    fn menu_shortcut_real_keys_history_repeat_context_lock_modal_focus_and_search_hint() {
        use crate::commands::Command;
        fn tap(key: Key) -> Vec<egui::Event> {
            let mut release = key_event(key);
            if let egui::Event::Key { pressed, .. } = &mut release {
                *pressed = false;
            }
            vec![release, key_event(key)]
        }

        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let create = Command::NewLayer.shortcut_action().unwrap();
        app.workspace
            .layout
            .shortcuts
            .assign(create, Some((Key::F8, Modifiers::NONE)))
            .unwrap();
        app.workspace
            .layout
            .shortcuts
            .assign(
                Command::RawDevelop.shortcut_action().unwrap(),
                Some((Key::F9, Modifiers::NONE)),
            )
            .unwrap();
        assert_eq!(
            app.command_entries()
                .iter()
                .find(|e| e.command == Command::NewLayer)
                .unwrap()
                .shortcut,
            "F8"
        );
        let initial = app.tab().unwrap().editor.document.layers.len();
        command_frame(&mut app, &ctx, &mut clock, tap(Key::F8));
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), initial + 1);
        let mut repeat = key_event(Key::F8);
        if let egui::Event::Key { repeat: r, .. } = &mut repeat {
            *r = true;
        }
        command_frame(&mut app, &ctx, &mut clock, vec![repeat]);
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), initial + 1);
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), initial);
        assert!(!app.tab().unwrap().editor.can_undo());
        command_frame(&mut app, &ctx, &mut clock, tap(Key::F9));
        assert!(app.raw_dialog.is_none());
        assert!(app.status.contains("RAW"));
        app.workspace
            .layout
            .shortcuts
            .assign(
                Command::AdjustmentLayer.shortcut_action().unwrap(),
                Some((Key::F11, Modifiers::NONE)),
            )
            .unwrap();
        app.tab_mut().unwrap().editor.document.layers[0].locked = true;
        command_frame(&mut app, &ctx, &mut clock, tap(Key::F11));
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), initial);
        assert!(app.status.contains("잠금"));
        app.tab_mut().unwrap().editor.document.layers[0].locked = false;
        app.new_dialog = true;
        command_frame(&mut app, &ctx, &mut clock, tap(Key::F8));
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), initial);
        app.new_dialog = false;
        app.transforming = true;
        command_frame(&mut app, &ctx, &mut clock, tap(Key::F8));
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), initial);
        app.transforming = false;
        let mut text = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
        });
        output.textures_delta.clear();
        command_frame(&mut app, &ctx, &mut clock, tap(Key::F8));
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), initial);
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        command_frame(&mut app, &ctx, &mut clock, vec![]);
        command_frame(&mut app, &ctx, &mut clock, vec![]);
        assert!(!ctx.egui_wants_keyboard_input());
        app.tabs.clear();
        command_frame(&mut app, &ctx, &mut clock, tap(Key::F8));
        assert!(app.tabs.is_empty());
        assert!(
            app.status.contains("문서"),
            "status={:?}; reason={:?}; blocked={}",
            app.status,
            app.command_reason(Command::NewLayer),
            app.command_blocked()
        );
        app.workspace
            .layout
            .shortcuts
            .assign(
                Command::PixelGrid.shortcut_action().unwrap(),
                Some((Key::F10, Modifiers::NONE)),
            )
            .unwrap();
        let grid = app.show_grid;
        command_frame(&mut app, &ctx, &mut clock, tap(Key::F10));
        assert_ne!(app.show_grid, grid);
    }
    #[test]
    fn command_search_shortcut_remap_and_no_document_new_dialog() {
        use crate::shortcuts::Action;
        let ctx = egui::Context::default();
        let mut app = PhotoEditor::from_context(&ctx);
        let mut clock = 0.0;
        app.workspace
            .layout
            .shortcuts
            .assign(Action::CommandPalette, Some((Key::K, Modifiers::CTRL)))
            .unwrap();
        let mut event = key_event(Key::K);
        if let egui::Event::Key { modifiers, .. } = &mut event {
            *modifiers = Modifiers::CTRL;
        }
        command_frame(&mut app, &ctx, &mut clock, vec![event]);
        assert!(app.command_panel.open);
        app.command_panel.query = "새 문서".into();
        command_frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Enter)]);
        assert!(app.new_dialog);
        assert!(!app.command_panel.open);
    }
    fn name_frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        clock: &mut f64,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        *clock += 0.4;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(*clock),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320.0, 860.0))),
                events,
                ..Default::default()
            },
            |ui| {
                app.shortcuts(ctx);
                app.workspace_ui(ui, ctx);
            },
        );
        output.textures_delta.clear();
        output
    }
    #[test]
    fn inline_layer_name_keyboard_keeps_selection_source_and_one_undo() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let source = app
            .tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .pixels
            .clone();
        let layer = app.tab().unwrap().editor.document.active;
        app.tab_mut().unwrap().selected_layers = vec![layer];
        app.start_rename(layer, &ctx);
        name_frame(&mut app, &ctx, &mut clock, vec![]);
        name_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Text("하늘 레이어 🌤".into())],
        );
        assert_eq!(app.rename.as_ref().unwrap().name, "하늘 레이어 🌤");
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .name,
            "배경"
        );
        name_frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Enter)]);
        assert!(app.rename.is_none());
        let tab = app.tab().unwrap();
        assert_eq!(
            tab.editor.document.active_layer().unwrap().name,
            "하늘 레이어 🌤"
        );
        assert_eq!(tab.selected_layers, vec![layer]);
        assert_eq!(tab.editor.document.active, layer);
        assert_eq!(tab.editor.history().count(), 1);
        assert!(std::sync::Arc::ptr_eq(
            &source,
            &tab.editor.document.active_layer().unwrap().pixels
        ));
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/layer-name");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("unicode-name.rphoto");
        files::save_project(&tab.editor.document, &path).unwrap();
        assert_eq!(
            files::open(&path)
                .unwrap()
                .document
                .active_layer()
                .unwrap()
                .name,
            "하늘 레이어 🌤"
        );
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .name,
            "배경"
        );
        app.tab_mut().unwrap().editor.redo();
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .name,
            "하늘 레이어 🌤"
        );
    }
    #[test]
    fn inline_layer_name_escape_empty_noop_and_ime_preserve_document() {
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        let layer = app.tab().unwrap().editor.document.active;
        app.start_rename(layer, &ctx);
        name_frame(&mut app, &ctx, &mut clock, vec![]);
        name_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Text("   ".into())],
        );
        name_frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Enter)]);
        assert!(app.rename.is_some());
        name_frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Escape)]);
        assert!(app.rename.is_none());
        assert!(!app.tab().unwrap().editor.can_undo());
        app.start_rename(layer, &ctx);
        name_frame(&mut app, &ctx, &mut clock, vec![]);
        let mut up = key_event(Key::Enter);
        if let egui::Event::Key { pressed, .. } = &mut up {
            *pressed = false;
        }
        name_frame(&mut app, &ctx, &mut clock, vec![up.clone()]);
        name_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::Ime(egui::ImeEvent::Preedit {
                    text: "한".into(),
                    active_range_chars: None,
                }),
                key_event(Key::Enter),
            ],
        );
        assert!(app.rename.is_some());
        assert!(!app.tab().unwrap().editor.can_undo());
        name_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![up, egui::Event::Ime(egui::ImeEvent::Commit("한글".into()))],
        );
        assert!(app.rename.is_some());
        assert_eq!(app.rename.as_ref().unwrap().name, "한글");
        name_frame(&mut app, &ctx, &mut clock, vec![key_event(Key::Enter)]);
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .name,
            "한글"
        );
        let history = app.tab().unwrap().editor.history().count();
        app.start_rename(layer, &ctx);
        app.finish_rename(true);
        assert_eq!(app.tab().unwrap().editor.history().count(), history);
    }
    #[test]
    fn inline_layer_name_duplicates_hidden_group_and_stale_target_are_safe() {
        let (mut app, ctx, _) = input_app(Tool::Move);
        app.new_layer();
        let tab = app.tab_mut().unwrap();
        let first = tab.editor.document.layers[0].id;
        let second = tab.editor.document.layers[1].id;
        tab.editor.document.group_selected(&[first]).unwrap();
        let group = tab.editor.document.active;
        tab.editor.document.layers[0].parent = Some(group);
        tab.collapsed.insert(group);
        tab.selected_layers = vec![first, second];
        let selected = tab.selected_layers.clone();
        let active = tab.editor.document.active;
        app.workspace.layout.layers = false;
        app.start_rename(first, &ctx);
        assert!(app.workspace.layout.layers);
        assert!(!app.tab().unwrap().collapsed.contains(&group));
        app.rename.as_mut().unwrap().name = "동일 이름".into();
        app.finish_rename(true);
        app.start_rename(second, &ctx);
        app.rename.as_mut().unwrap().name = "동일 이름".into();
        app.finish_rename(true);
        let tab = app.tab().unwrap();
        assert_eq!(tab.selected_layers, selected);
        assert_eq!(tab.editor.document.active, active);
        assert!(
            tab.editor
                .document
                .layers
                .iter()
                .filter(|l| l.id == first || l.id == second)
                .all(|l| l.name == "동일 이름")
        );
        app.start_rename(first, &ctx);
        app.rename.as_mut().unwrap().name = "삭제된 대상".into();
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .layers
            .retain(|l| l.id != first);
        app.validate_rename();
        assert!(app.rename.is_none());
        app.start_rename(second, &ctx);
        app.workspace.layout.layers = false;
        app.validate_rename();
        assert!(app.rename.is_none());
        app.start_rename(second, &ctx);
        app.active = usize::MAX;
        app.validate_rename();
        assert!(app.rename.is_none());
    }
    #[test]
    fn inline_layer_name_blur_click_does_not_start_brush_stroke() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let layer = app.tab().unwrap().editor.document.active;
        let source = app
            .tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .pixels
            .clone();
        app.start_rename(layer, &ctx);
        name_frame(&mut app, &ctx, &mut clock, vec![]);
        name_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Text("확정할 이름".into())],
        );
        let position = app.tab().unwrap().canvas_area.unwrap().center();
        name_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        name_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(position + Vec2::new(15.0, 5.0))],
        );
        name_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerButton {
                pos: position,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(app.rename.is_none());
        let tab = app.tab().unwrap();
        assert!(tab.gesture.is_none());
        assert_eq!(tab.editor.history().count(), 1);
        assert_eq!(
            tab.editor.document.active_layer().unwrap().name,
            "확정할 이름"
        );
        assert!(std::sync::Arc::ptr_eq(
            &source,
            &tab.editor.document.active_layer().unwrap().pixels
        ));
    }
    #[test]
    fn inline_layer_name_double_click_enters_row_editor() {
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        let output = name_frame(&mut app, &ctx, &mut clock, vec![]);
        let position = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == "배경" => {
                    Some(t.pos + Vec2::new(10.0, 5.0))
                }
                _ => None,
            })
            .unwrap();
        for _ in 0..2 {
            clock += 0.02;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320.0, 860.0))),
                    events: vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: PointerButton::Primary,
                            pressed: true,
                            modifiers: Modifiers::NONE,
                        },
                        egui::Event::PointerButton {
                            pos: position,
                            button: PointerButton::Primary,
                            pressed: false,
                            modifiers: Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| app.workspace_ui(ui, &ctx),
            );
            output.textures_delta.clear();
        }
        assert!(app.rename.is_some());
        let output = name_frame(&mut app, &ctx, &mut clock, vec![]);
        assert!(ctx.memory(|m| m.has_focus(egui::Id::new((
            "layer-name",
            app.tab().unwrap().id,
            app.tab().unwrap().editor.document.active
        )))));
        assert!(!output.shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Text(t) if t.galley.job.text=="레이어 이름")
        ));
        assert!(!app.tab().unwrap().editor.can_undo());
    }
    #[test]
    fn numeric_label_resize_dialog_drag_cancel_apply_original_and_one_undo() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let original = app.tabs[0].editor.document.clone();
        for apply in [false, true] {
            app.execute_command(crate::commands::Command::Resize, &ctx);
            adjustment_frame(&mut app, &ctx, &mut clock, vec![], "폭");
            let at = adjustment_frame(&mut app, &ctx, &mut clock, vec![], "폭").unwrap();
            adjustment_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                ],
                "",
            );
            let end = at + Vec2::new(20., 0.);
            adjustment_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![egui::Event::PointerMoved(end)],
                "",
            );
            adjustment_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
                "",
            );
            assert_eq!(app.resize_width, original.width + 20);
            assert_eq!(app.tabs[0].editor.document.width, original.width);
            adjustment_click(
                &mut app,
                &ctx,
                &mut clock,
                if apply { "적용" } else { "취소" },
            );
            assert!(app.size_dialog.is_none());
            if apply {
                assert_eq!(app.tabs[0].editor.document.width, original.width + 20);
                assert_eq!(app.tabs[0].editor.history().count(), 1);
                app.tabs[0].editor.undo();
            }
            assert_eq!(app.tabs[0].editor.document.width, original.width);
            assert!(Arc::ptr_eq(
                &app.tabs[0].editor.document.layers[0].pixels,
                &original.layers[0].pixels
            ));
            assert!(!app.tabs[0].editor.can_undo());
        }
    }
    fn adjustment_frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        clock: &mut f64,
        events: Vec<egui::Event>,
        target: &str,
    ) -> Option<Pos2> {
        *clock += 0.4;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(*clock),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                events,
                ..Default::default()
            },
            |ui| {
                app.poll_jobs(ctx);
                app.shortcuts(ctx);
                app.workspace_ui(ui, ctx);
                app.dialogs(ctx);
            },
        );
        let point = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.job.text == target => Some(t.pos + Vec2::splat(5.)),
            _ => None,
        });
        output.textures_delta.clear();
        point
    }
    fn adjustment_click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
        adjustment_frame(app, ctx, clock, vec![], target);
        let point = adjustment_frame(app, ctx, clock, vec![], target)
            .unwrap_or_else(|| panic!("missing adjustment control: {target}"));
        adjustment_frame(
            app,
            ctx,
            clock,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            target,
        );
        adjustment_frame(
            app,
            ctx,
            clock,
            vec![egui::Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            target,
        );
    }
    #[test]
    fn tonal_contrast_controls_reset_cancel_apply_bake_and_undo() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let doc = crate::precision::document(
            "tone UI".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_fn(24, 20, |x, _| {
                image::Rgba([x as f32 / 10., 0.2, -0.1, 0.7])
            })),
        )
        .unwrap();
        app.tab_mut().unwrap().editor = Editor::new(doc, false);
        let source = app.tab().unwrap().editor.document.layers[0]
            .precision
            .clone()
            .unwrap();
        app.open_adjustments();
        adjustment_click(&mut app, &ctx, &mut clock, "톤별 대비");
        for label in [
            "전체 강도 %",
            "그림자 대비 %",
            "중간톤 대비 %",
            "하이라이트 대비 %",
            "세부 반경 px",
        ] {
            assert!(
                adjustment_frame(&mut app, &ctx, &mut clock, vec![], label).is_some(),
                "missing {label}"
            );
        }
        app.tab_mut().unwrap().editor.document.layers[0]
            .adjustments
            .tonal_contrast
            .amount = 70.;
        adjustment_click(&mut app, &ctx, &mut clock, "톤 대비 초기화");
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .tonal_contrast
                .amount,
            0.
        );
        app.tab_mut().unwrap().editor.document.layers[0]
            .adjustments
            .tonal_contrast
            .amount = 70.;
        adjustment_click(&mut app, &ctx, &mut clock, "취소");
        assert!(!app.tab().unwrap().editor.can_undo());
        app.open_adjustments();
        app.tab_mut().unwrap().editor.document.layers[0]
            .adjustments
            .tonal_contrast
            .amount = 70.;
        let expected = crate::precision::composite(&app.tab().unwrap().editor.document);
        adjustment_click(&mut app, &ctx, &mut clock, "적용");
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        assert!(Arc::ptr_eq(
            &source,
            app.tab().unwrap().editor.document.layers[0]
                .precision
                .as_ref()
                .unwrap()
        ));
        app.open_adjustments();
        adjustment_click(&mut app, &ctx, &mut clock, "원본 픽셀에 적용");
        assert!(
            !app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .active()
        );
        assert_eq!(
            crate::precision::composite(&app.tab().unwrap().editor.document),
            expected
        );
        app.tab_mut().unwrap().editor.undo();
        assert!(
            app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .tonal_contrast
                .enabled()
        );
    }
    #[test]
    fn expanded_filter_checkbox_apply_cancel_and_undo_preserve_original() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let original = app.tab().unwrap().editor.document.layers[0].pixels.clone();
        app.open_adjustments();
        adjustment_click(&mut app, &ctx, &mut clock, "레이어 경계 밖까지 확산");
        assert!(
            app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .expand_filters
        );
        adjustment_click(&mut app, &ctx, &mut clock, "취소");
        assert!(
            !app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .expand_filters
        );
        assert!(!app.tab().unwrap().editor.can_undo());
        app.open_adjustments();
        adjustment_click(&mut app, &ctx, &mut clock, "레이어 경계 밖까지 확산");
        app.tab_mut().unwrap().editor.document.layers[0]
            .adjustments
            .blur = 1.;
        adjustment_click(&mut app, &ctx, &mut clock, "적용");
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        assert!(Arc::ptr_eq(
            &original,
            &app.tab().unwrap().editor.document.layers[0].pixels
        ));
        app.tab_mut().unwrap().editor.undo();
        assert!(
            !app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .expand_filters
        );
        app.tab_mut().unwrap().editor.redo();
        assert!(
            app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .expand_filters
        );
    }
    #[test]
    fn standalone_adjustment_command_create_cancel_apply_reedit_and_undo_keep_original() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let original = app.tab().unwrap().editor.document.layers[0].pixels.clone();
        app.tab_mut().unwrap().selection = Some(Selection::rectangle([20, 20], [100, 120]));
        app.execute_command(crate::commands::Command::AdjustmentLayer, &ctx);
        assert!(app.adjust_dialog.is_some());
        assert!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .adjustment_layer
        );
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .adjustments
            .brightness = -0.4;
        adjustment_click(&mut app, &ctx, &mut clock, "취소");
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 1);
        assert!(!app.tab().unwrap().editor.can_undo());
        app.execute_command(crate::commands::Command::AdjustmentLayer, &ctx);
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .adjustments
            .brightness = -0.4;
        let selected = app.tab().unwrap().editor.document.composite();
        assert_eq!(selected.get_pixel(150, 150), original.get_pixel(150, 150));
        assert_ne!(selected.get_pixel(50, 50), original.get_pixel(50, 50));
        adjustment_click(&mut app, &ctx, &mut clock, "원본 픽셀에 적용");
        assert!(app.adjust_dialog.is_some());
        adjustment_click(&mut app, &ctx, &mut clock, "적용");
        assert!(app.adjust_dialog.is_none());
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        assert!(Arc::ptr_eq(
            &original,
            &app.tab().unwrap().editor.document.layers[0].pixels
        ));
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 1);
        assert_eq!(app.tab().unwrap().editor.document.composite(), *original);
        app.tab_mut().unwrap().editor.redo();
        assert_eq!(app.tab().unwrap().editor.document.composite(), selected);
        app.tab_mut().unwrap().selection = None;
        app.open_adjustments();
        assert!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .adjustments
                .scope
                .is_some()
        );
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .adjustments
            .brightness = -0.2;
        adjustment_click(&mut app, &ctx, &mut clock, "취소");
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        assert_eq!(app.tab().unwrap().editor.document.composite(), selected);
    }
    #[test]
    fn standalone_adjustment_auto_levels_uses_below_stack_and_mask_brush_guards_pixels() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let size = app.tab().unwrap().editor.document.layers[0]
            .pixels
            .dimensions();
        app.tab_mut().unwrap().editor.document.layers[0].pixels =
            Arc::new(image::RgbaImage::from_fn(size.0, size.1, |x, _| {
                let value = if x < size.0 / 2 { 80 } else { 180 };
                image::Rgba([value, value, value, 255])
            }));
        let original = app.tab().unwrap().editor.document.layers[0].pixels.clone();
        app.execute_command(crate::commands::Command::AdjustmentLayer, &ctx);
        adjustment_click(&mut app, &ctx, &mut clock, "자동 레벨");
        let adjustments = &app
            .tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .adjustments;
        assert!(adjustments.black > 0. || adjustments.white < 1.);
        adjustment_click(&mut app, &ctx, &mut clock, "적용");
        let layer = app
            .tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap();
        layer.mask = Some(Arc::new(image::GrayImage::from_pixel(
            200,
            200,
            image::Luma([255]),
        )));
        app.tab_mut().unwrap().edit_mask = true;
        assert!(
            app.command_reason(crate::commands::Command::Rasterize)
                .is_some()
        );
        let layer = app
            .tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap();
        let brush = document::Brush {
            mask: true,
            color: [0, 0, 0, 255],
            radius: 5.,
            hardness: 1.,
            opacity: 1.,
            erase: false,
        };
        document::paint_line(layer, [50., 50.], [60., 50.], &brush, None);
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(55, 50)[0], 0);
        assert!(Arc::ptr_eq(
            &original,
            &app.tab().unwrap().editor.document.layers[0].pixels
        ));
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .locked = true;
        assert!(
            app.command_reason(crate::commands::Command::AdjustmentLayer)
                .is_some()
        );
    }
    #[test]
    fn selected_adjustment_dialog_toggle_cancel_apply_reopen_keeps_source_and_one_undo() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let original = app.tab().unwrap().editor.document.layers[0].pixels.clone();
        app.tab_mut().unwrap().selection =
            Some(crate::editing::ellipse_smooth([20, 20], [100, 120], true));
        app.open_adjustments();
        assert!(app.adjust_dialog.is_some());
        let scope = app.tab().unwrap().editor.document.layers[0]
            .adjustments
            .scope
            .clone()
            .unwrap();
        app.tab_mut().unwrap().editor.document.layers[0]
            .adjustments
            .brightness = -0.4;
        let selected = app.tab().unwrap().editor.document.composite();
        assert_eq!(selected.get_pixel(150, 150), original.get_pixel(150, 150));
        assert_ne!(selected.get_pixel(50, 50), original.get_pixel(50, 50));
        adjustment_click(&mut app, &ctx, &mut clock, "선택 영역에만 적용");
        assert!(
            app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .scope
                .is_none()
        );
        assert_ne!(
            app.tab()
                .unwrap()
                .editor
                .document
                .composite()
                .get_pixel(150, 150),
            original.get_pixel(150, 150)
        );
        adjustment_click(&mut app, &ctx, &mut clock, "선택 영역에만 적용");
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .scope,
            Some(scope.clone())
        );
        adjustment_click(&mut app, &ctx, &mut clock, "취소");
        assert!(app.adjust_dialog.is_none());
        assert!(
            app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .scope
                .is_none()
        );
        assert!(!app.tab().unwrap().editor.can_undo());
        app.open_adjustments();
        app.tab_mut().unwrap().editor.document.layers[0]
            .adjustments
            .brightness = -0.4;
        adjustment_click(&mut app, &ctx, &mut clock, "적용");
        assert!(app.adjust_dialog.is_none());
        let tab = app.tab().unwrap();
        assert_eq!(tab.editor.history().count(), 1);
        assert!(Arc::ptr_eq(
            &original,
            &tab.editor.document.layers[0].pixels
        ));
        app.tab_mut().unwrap().selection = None;
        app.open_adjustments();
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .adjustments
                .scope,
            Some(scope)
        );
        adjustment_click(&mut app, &ctx, &mut clock, "취소");
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(app.tab().unwrap().editor.document.composite(), *original);
        app.tab_mut().unwrap().editor.redo();
        assert_eq!(app.tab().unwrap().editor.document.composite(), selected);
    }
    #[test]
    fn selected_adjustment_original_apply_bakes_group_and_locked_layer_is_guarded() {
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .group_active()
            .unwrap();
        let group = app.tab().unwrap().editor.document.active;
        app.tab_mut().unwrap().selection = Some(Selection::rectangle([20, 20], [100, 120]));
        app.open_adjustments();
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .adjustments
            .brightness = -0.4;
        let expected = app.tab().unwrap().editor.document.composite();
        adjustment_click(&mut app, &ctx, &mut clock, "원본 픽셀에 적용");
        assert!(app.adjust_dialog.is_none());
        let tab = app.tab().unwrap();
        assert_eq!(tab.editor.document.active, group);
        assert!(!tab.editor.document.active_layer().unwrap().group);
        assert_eq!(tab.editor.document.layers.len(), 1);
        assert_eq!(tab.editor.document.composite(), expected);
        assert_eq!(tab.editor.history().count(), 1);
        app.tab_mut().unwrap().editor.undo();
        assert!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .group
        );
        app.tab_mut()
            .unwrap()
            .editor
            .document
            .active_layer_mut()
            .unwrap()
            .locked = true;
        app.open_adjustments();
        assert!(app.adjust_dialog.is_none());
        assert!(
            app.command_reason(crate::commands::Command::Adjust)
                .is_some()
        );
    }

    #[test]
    fn floating_docked_panels_restore_small_window_dpi_without_changing_document_and_canvas_only_hides_them()
     {
        use crate::panels::{Action, Kind};
        for dpi in [1., 2.] {
            let (mut app, ctx, mut time) = input_app(Tool::Brush);
            ctx.set_pixels_per_point(dpi);
            let before = app.tab().unwrap().editor.document.clone();
            let revision = app.tab().unwrap().editor.revision;
            let state = app
                .workspace
                .layout
                .panels
                .iter_mut()
                .find(|s| s.kind == Kind::Layers)
                .unwrap();
            state.floating = true;
            state.position = Some([5000., 5000.]);
            state.size = [280., 260.];
            app.workspace.layout.properties = false;
            app.workspace.layout.history = false;
            assert!(!app.workspace.layout.sidebar_visible());
            let frame = |app: &mut PhotoEditor, time: f64| {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                        ..Default::default()
                    },
                    |ui| {
                        app.properties_ui(ui);
                        app.floating_panels_ui(&ctx);
                    },
                );
                out.textures_delta.clear();
                out
            };
            for _ in 0..4 {
                time += 0.1;
                frame(&mut app, time);
            }
            let saved = app
                .workspace
                .layout
                .panels
                .iter()
                .find(|s| s.kind == Kind::Layers)
                .unwrap()
                .clone();
            assert!(saved.position.unwrap()[0] < 880. && saved.position.unwrap()[1] < 620.);
            assert!(app.workspace.layout.valid());
            time += 0.1;
            let out = frame(&mut app, time);
            assert!(out.shapes.iter().any(
                |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="+ 새 레이어")
            ));
            app.canvas_only = true;
            time += 0.1;
            let out = frame(&mut app, time);
            assert!(!out.shapes.iter().any(
                |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="+ 새 레이어")
            ));
            assert_eq!(
                app.workspace
                    .layout
                    .panels
                    .iter()
                    .find(|s| s.kind == Kind::Layers)
                    .unwrap(),
                &saved
            );
            app.canvas_only = false;
            app.panel_action(Kind::Layers, Action::Float);
            assert!(app.workspace.layout.sidebar_visible());
            app.panel_action(Kind::Layers, Action::Collapse);
            time += 0.1;
            let out = frame(&mut app, time);
            assert!(!out.shapes.iter().any(
                |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text=="+ 새 레이어")
            ));
            app.panel_action(Kind::Layers, Action::Collapse);
            app.panel_action(Kind::Layers, Action::Close);
            assert!(!app.workspace.layout.layers);
            assert_eq!(app.tab().unwrap().editor.revision, revision);
            assert_eq!(
                app.tab().unwrap().editor.document.composite(),
                before.composite()
            );
            assert!(Arc::ptr_eq(
                &before.active_layer().unwrap().pixels,
                &app.tab()
                    .unwrap()
                    .editor
                    .document
                    .active_layer()
                    .unwrap()
                    .pixels
            ));
        }
    }
    #[test]
    fn floating_layer_controls_edit_once_and_transform_blocks_input() {
        use crate::panels::{Action, Kind};
        let (mut app, ctx, mut time) = input_app(Tool::Move);
        app.panel_action(Kind::Layers, Action::Float);
        let frame = |app: &mut PhotoEditor, time: f64, events| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                    events,
                    ..Default::default()
                },
                |_ui| app.floating_panels_ui(&ctx),
            );
            out.textures_delta.clear();
            out
        };
        let point = |out: &egui::FullOutput| {
            out.shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.job.text == "+ 새 레이어" => {
                        Some(t.pos + Vec2::splat(4.))
                    }
                    _ => None,
                })
                .unwrap()
        };
        let click = |app: &mut PhotoEditor, time: &mut f64, p| {
            *time += 0.1;
            frame(app, *time, vec![egui::Event::PointerMoved(p)]);
            for pressed in [true, false] {
                *time += 0.1;
                frame(
                    app,
                    *time,
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    }],
                );
            }
        };
        for _ in 0..4 {
            time += 0.1;
            frame(&mut app, time, vec![]);
        }
        time += 0.1;
        let out = frame(&mut app, time, vec![]);
        let p = point(&out);
        click(&mut app, &mut time, p);
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 2);
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 1);
        app.transforming = true;
        time += 0.1;
        let out = frame(&mut app, time, vec![]);
        click(&mut app, &mut time, point(&out));
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 1);
    }

    #[test]
    fn floating_panel_pointer_move_resize_and_fresh_context_restore_geometry_without_growth() {
        use crate::panels::{Action, Kind};
        let (mut app, ctx, mut time) = input_app(Tool::Move);
        app.panel_action(Kind::Layers, Action::Float);
        let frame = |app: &mut PhotoEditor, ctx: &egui::Context, time: f64, events| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 800.))),
                    events,
                    ..Default::default()
                },
                |_ui| app.floating_panels_ui(ctx),
            );
            out.textures_delta.clear();
            out
        };
        let drag = |app: &mut PhotoEditor, time: &mut f64, start: Pos2, end: Pos2| {
            *time += 0.1;
            frame(app, &ctx, *time, vec![egui::Event::PointerMoved(start)]);
            *time += 0.1;
            frame(
                app,
                &ctx,
                *time,
                vec![egui::Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                }],
            );
            for p in [start + (end - start) * 0.4, end] {
                *time += 0.1;
                frame(app, &ctx, *time, vec![egui::Event::PointerMoved(p)]);
            }
            *time += 0.1;
            frame(
                app,
                &ctx,
                *time,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
            );
        };
        for _ in 0..4 {
            time += 0.1;
            frame(&mut app, &ctx, time, vec![]);
        }
        let id = crate::panels::window_id(&ctx, Kind::Layers);
        let before = ctx.memory(|m| m.area_rect(id)).unwrap();
        let start = before.min + Vec2::new(100., 12.);
        drag(&mut app, &mut time, start, start + Vec2::new(40., 25.));
        let moved = ctx.memory(|m| m.area_rect(id)).unwrap();
        assert!((moved.min - before.min - Vec2::new(40., 25.)).length() < 2.);
        let start = moved.max - Vec2::splat(3.);
        drag(&mut app, &mut time, start, start + Vec2::new(50., 40.));
        for _ in 0..3 {
            time += 0.1;
            frame(&mut app, &ctx, time, vec![]);
        }
        let resized = ctx.memory(|m| m.area_rect(id)).unwrap();
        assert!(resized.width() > moved.width() + 30. && resized.height() > moved.height() + 20.);
        let saved = app.workspace.layout.clone();
        let restored = egui::Context::default();
        app.workspace.layout =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        for _ in 0..4 {
            time += 0.1;
            frame(&mut app, &restored, time, vec![]);
        }
        let actual = restored
            .memory(|m| m.area_rect(crate::panels::window_id(&restored, Kind::Layers)))
            .unwrap();
        assert!((actual.min - resized.min).length() < 2.);
        assert!(
            (actual.size() - resized.size()).length() < 2.,
            "old={resized:?} restored={actual:?} saved={:?}",
            saved.panels
        );
        let old_id = crate::panels::window_id(&restored, Kind::Layers);
        app.execute_command(crate::commands::Command::ResetWorkspace, &restored);
        assert_ne!(old_id, crate::panels::window_id(&restored, Kind::Layers));
        assert_eq!(app.workspace.layout.panels, crate::panels::defaults());
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
    }
    fn input_app(tool: Tool) -> (PhotoEditor, egui::Context, f64) {
        let ctx = egui::Context::default();
        let mut app = PhotoEditor::from_context(&ctx);
        app.add_tab(files::Opened {
            source_stamp: None,
            document: Document::new(256, 256, false).unwrap(),
            project_path: None,
            title: "입력 검증".into(),
            notes: vec![],
        });
        app.export_preview.enabled = false;
        app.tool = tool;
        app.brush_size = 8.0;
        app.tab_mut().unwrap().view.fit = false;
        (app, ctx, 0.0)
    }
    fn input_frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        clock: &mut f64,
        mut events: Vec<egui::Event>,
        modifiers: Modifiers,
    ) {
        *clock += 0.4;
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(*clock),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(512.0))),
                events,
                ..Default::default()
            },
            |ui| {
                app.shortcuts(ctx);
                app.canvas_ui(ui, ctx);
            },
        );
        output.textures_delta.clear();
    }
    fn click_at(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        clock: &mut f64,
        position: Pos2,
        modifiers: Modifiers,
    ) {
        input_frame(
            app,
            ctx,
            clock,
            vec![egui::Event::PointerMoved(position)],
            modifiers,
        );
        input_frame(
            app,
            ctx,
            clock,
            vec![
                egui::Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
                egui::Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers,
                },
            ],
            modifiers,
        );
    }
    fn key_event(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }
    #[test]
    fn webp_export_mode_pointer_switch_preserves_quality_effort_and_history() {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Pos2> {
            *clock += 0.1;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |_| app.dialogs(ctx),
            );
            let p = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(t.pos + Vec2::splat(3.))
                }
                _ => None,
            });
            out.textures_delta.clear();
            p
        }
        fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
            for _ in 0..3 {
                frame(app, ctx, clock, vec![], target);
            }
            let p = frame(app, ctx, clock, vec![], target).unwrap();
            frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
            frame(
                app,
                ctx,
                clock,
                vec![egui::Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                }],
                target,
            );
            frame(
                app,
                ctx,
                clock,
                vec![egui::Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
                target,
            );
        }
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.export_dialog = true;
        app.export_format = files::EXPORT_FORMATS
            .iter()
            .position(|f| f.0 == image::ImageFormat::WebP)
            .unwrap();
        click(&mut app, &ctx, &mut clock, "손실");
        assert!(!app.export_options.webp_lossless);
        assert!(frame(&mut app, &ctx, &mut clock, vec![], "품질").is_some());
        assert!(frame(&mut app, &ctx, &mut clock, vec![], "압축 노력").is_some());
        assert_eq!(app.export_options.webp_quality, 90);
        assert_eq!(app.export_options.webp_method, 4);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        click(&mut app, &ctx, &mut clock, "무손실");
        assert!(app.export_options.webp_lossless);
        assert!(frame(&mut app, &ctx, &mut clock, vec![], "압축 노력").is_none());
        assert_eq!(app.export_options.webp_quality, 90);
        assert!(app.message.is_none());
    }
    #[test]
    fn photoshop_grayscale_output_pointer_choice_compression_cancel_and_small_window_keep_source_history()
     {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            size: Vec2,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Rect> {
            *clock += 0.1;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |_| app.dialogs(ctx),
            );
            let rect = output.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(Rect::from_min_size(t.pos, t.galley.size()))
                }
                _ => None,
            });
            output.textures_delta.clear();
            rect
        }
        fn click(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            size: Vec2,
            target: &str,
        ) {
            for _ in 0..3 {
                frame(app, ctx, clock, size, vec![], target);
            }
            let pos = frame(app, ctx, clock, size, vec![], target)
                .unwrap_or_else(|| panic!("missing {target}"))
                .min
                + Vec2::splat(3.);
            frame(
                app,
                ctx,
                clock,
                size,
                vec![egui::Event::PointerMoved(pos)],
                target,
            );
            for pressed in [true, false] {
                frame(
                    app,
                    ctx,
                    clock,
                    size,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    }],
                    target,
                );
            }
        }
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        let size = Vec2::new(880., 620.);
        app.psd_dialog = Some(false);
        let pixels = app.tab().unwrap().editor.document.layers[0].pixels.clone();
        click(&mut app, &ctx, &mut clock, size, "RGB");
        click(&mut app, &ctx, &mut clock, size, "회색조");
        assert_eq!(app.psd_options.color, crate::psd::OutputColor::Grayscale);
        click(&mut app, &ctx, &mut clock, size, "컬러 레이어 구조 유지");
        assert!(app.psd_options.grayscale_layers);
        click(&mut app, &ctx, &mut clock, size, "무압축");
        click(&mut app, &ctx, &mut clock, size, "ZIP 예측");
        assert_eq!(
            app.psd_options.compression,
            crate::psd_compression::Compression::Prediction
        );
        click(&mut app, &ctx, &mut clock, size, "회색조");
        click(&mut app, &ctx, &mut clock, size, "RGB");
        assert_eq!(app.psd_options.color, crate::psd::OutputColor::Rgb);
        assert!(app.psd_options.grayscale_layers);
        click(&mut app, &ctx, &mut clock, size, "RGB");
        click(&mut app, &ctx, &mut clock, size, "회색조");
        for dpi in [1., 2.] {
            ctx.set_pixels_per_point(dpi);
            for size in [Vec2::new(880., 620.), Vec2::new(640., 320.)] {
                for _ in 0..3 {
                    frame(&mut app, &ctx, &mut clock, size, vec![], "저장 진행");
                }
                let save = frame(&mut app, &ctx, &mut clock, size, vec![], "저장 진행").unwrap();
                let cancel = frame(&mut app, &ctx, &mut clock, size, vec![], "취소").unwrap();
                let screen = Rect::from_min_size(Pos2::ZERO, size);
                assert!(screen.contains_rect(save));
                assert!(screen.contains_rect(cancel));
            }
        }
        ctx.set_pixels_per_point(1.);
        click(&mut app, &ctx, &mut clock, size, "취소");
        assert!(app.psd_dialog.is_none());
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        assert!(Arc::ptr_eq(
            &pixels,
            &app.tab().unwrap().editor.document.layers[0].pixels
        ));
    }
    #[test]
    fn tiff_export_pointer_depth_switch_disables_integer_predictor_and_selects_bigtiff() {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Pos2> {
            *clock += 0.1;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |_| app.dialogs(ctx),
            );
            let p = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(t.pos + Vec2::splat(3.))
                }
                _ => None,
            });
            out.textures_delta.clear();
            p
        }
        fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
            for _ in 0..3 {
                frame(app, ctx, clock, vec![], target);
            }
            let p = frame(app, ctx, clock, vec![], target)
                .unwrap_or_else(|| panic!("missing {target}"));
            frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
            for pressed in [true, false] {
                frame(
                    app,
                    ctx,
                    clock,
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    }],
                    target,
                );
            }
        }
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.export_dialog = true;
        app.export_format = files::EXPORT_FORMATS
            .iter()
            .position(|f| f.0 == image::ImageFormat::Tiff)
            .unwrap();
        {
            let doc = &mut app.tab_mut().unwrap().editor.document;
            doc.layers[0].precision = Some(Arc::new(image::Rgba32FImage::from_pixel(
                doc.width,
                doc.height,
                image::Rgba([2.25, -0.125, 0.5, 1.]),
            )));
            doc.layers[0].bit_depth = 32;
        }
        assert_eq!(app.export_options.tiff_depth, 0);
        for _ in 0..3 {
            frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![],
                "HDR 밝기와 음수 RGB를 부동소수점으로 보존합니다.",
            );
        }
        assert!(
            frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![],
                "HDR 밝기와 음수 RGB를 부동소수점으로 보존합니다."
            )
            .is_some()
        );
        app.export_options.tiff_predictor = true;
        click(&mut app, &ctx, &mut clock, "자동 (8/16/32비트)");
        click(&mut app, &ctx, &mut clock, "32비트 부동소수점");
        assert_eq!(app.export_options.tiff_depth, 3);
        assert!(!app.export_options.tiff_predictor);
        assert!(
            frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![],
                "HDR 밝기와 음수 RGB를 부동소수점으로 보존합니다."
            )
            .is_some()
        );
        click(&mut app, &ctx, &mut clock, "없음");
        click(&mut app, &ctx, &mut clock, "LZW");
        click(&mut app, &ctx, &mut clock, "부동소수점 예측");
        assert!(app.export_options.tiff_float_predictor);
        assert!(!app.export_options.tiff_predictor);
        click(&mut app, &ctx, &mut clock, "LZW");
        click(&mut app, &ctx, &mut clock, "PackBits");
        assert!(!app.export_options.tiff_float_predictor);
        click(&mut app, &ctx, &mut clock, "BigTIFF");
        assert!(app.export_options.tiff_big);
        let original = app.tab().unwrap().editor.document.clone();
        click(&mut app, &ctx, &mut clock, "회색조");
        assert!(app.export_options.tiff_grayscale);
        assert!(
            frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![],
                "회색조로 변환하며 투명도와 선택한 정밀도를 유지합니다."
            )
            .is_some()
        );
        click(&mut app, &ctx, &mut clock, "회색조");
        assert!(!app.export_options.tiff_grayscale);
        assert_eq!(
            crate::precision::composite(&app.tab().unwrap().editor.document),
            crate::precision::composite(&original)
        );
        assert_eq!(app.export_options.tiff_resolution, 72);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        assert!(app.message.is_none());
    }
    #[test]
    fn jpeg2000_export_depth_lossless_lossy_pointer_controls_cancel_and_source_history() {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Pos2> {
            *clock += 0.1;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |_| app.dialogs(ctx),
            );
            let p = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(t.pos + Vec2::splat(3.))
                }
                _ => None,
            });
            out.textures_delta.clear();
            p
        }
        fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
            for _ in 0..3 {
                frame(app, ctx, clock, vec![], target);
            }
            let p = frame(app, ctx, clock, vec![], target)
                .unwrap_or_else(|| panic!("missing {target}"));
            frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
            for pressed in [true, false] {
                frame(
                    app,
                    ctx,
                    clock,
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    }],
                    target,
                );
            }
        }
        for format in files::EXPORT_FORMATS.len() + 3..files::export_count() {
            let (mut app, ctx, mut clock) = input_app(Tool::Move);
            app.export_dialog = true;
            app.export_format = format;
            let before = app.tab().unwrap().editor.document.layers[0].pixels.clone();
            assert!(frame(&mut app, &ctx, &mut clock, vec![], "PNG 압축 수준").is_none());
            click(&mut app, &ctx, &mut clock, "회색조");
            assert!(app.export_options.jp2_grayscale);
            click(&mut app, &ctx, &mut clock, "RGB");
            assert!(!app.export_options.jp2_grayscale);
            click(&mut app, &ctx, &mut clock, "16비트");
            assert_eq!(app.export_options.jp2_depth, 16);
            click(&mut app, &ctx, &mut clock, "무손실 압축");
            assert!(!app.export_options.jp2_lossless);
            assert!(frame(&mut app, &ctx, &mut clock, vec![], "압축 배율").is_some());
            click(&mut app, &ctx, &mut clock, "무손실 압축");
            assert!(app.export_options.jp2_lossless);
            assert!(frame(&mut app, &ctx, &mut clock, vec![], "압축 배율").is_none());
            assert_eq!(app.export_options.jp2_ratio, 8);
            click(&mut app, &ctx, &mut clock, "8비트");
            assert_eq!(app.export_options.jp2_depth, 8);
            click(&mut app, &ctx, &mut clock, "화질 우선");
            click(&mut app, &ctx, &mut clock, "해상도 우선");
            assert_eq!(app.export_options.jp2_progression, 1);
            assert!(frame(&mut app, &ctx, &mut clock, vec![], "품질 단계").is_some());
            assert_eq!(app.export_options.jp2_layers, 1);
            click(&mut app, &ctx, &mut clock, "분할 없음");
            click(&mut app, &ctx, &mut clock, "128픽셀");
            assert_eq!(app.export_options.jp2_tile, 128);
            click(&mut app, &ctx, &mut clock, "128픽셀");
            click(&mut app, &ctx, &mut clock, "분할 없음");
            assert_eq!(app.export_options.jp2_tile, 0);
            if format == files::EXPORT_FORMATS.len() + 3 {
                click(&mut app, &ctx, &mut clock, "출력 해상도 정보 포함");
                assert!(app.export_options.jp2_resolution_enabled);
                assert_eq!(app.export_options.jp2_resolution, 72);
                assert!(frame(&mut app, &ctx, &mut clock, vec![], "해상도 (ppi)").is_some());
                click(&mut app, &ctx, &mut clock, "출력 해상도 정보 포함");
                assert!(!app.export_options.jp2_resolution_enabled);
            } else {
                assert!(
                    frame(&mut app, &ctx, &mut clock, vec![], "출력 해상도 정보 포함").is_none()
                );
            }
            click(&mut app, &ctx, &mut clock, "취소");
            assert!(!app.export_dialog);
            assert_eq!(app.tab().unwrap().editor.history().count(), 0);
            assert!(Arc::ptr_eq(
                &before,
                &app.tab().unwrap().editor.document.layers[0].pixels
            ));
            assert!(app.message.is_none());
        }
    }
    #[test]
    fn avif_export_depth_and_matrix_pointer_controls_preserve_independent_settings() {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Pos2> {
            *clock += 0.1;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |_| app.dialogs(ctx),
            );
            let p = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(t.pos + Vec2::splat(3.))
                }
                _ => None,
            });
            out.textures_delta.clear();
            p
        }
        fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
            for _ in 0..3 {
                frame(app, ctx, clock, vec![], target);
            }
            let p = frame(app, ctx, clock, vec![], target)
                .unwrap_or_else(|| panic!("missing {target}"));
            frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
            for pressed in [true, false] {
                frame(
                    app,
                    ctx,
                    clock,
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    }],
                    target,
                );
            }
        }
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.export_dialog = true;
        app.export_format = files::EXPORT_FORMATS
            .iter()
            .position(|f| f.0 == image::ImageFormat::Avif)
            .unwrap();
        click(&mut app, &ctx, &mut clock, "10비트");
        assert_eq!(app.export_options.avif_depth, 10);
        click(&mut app, &ctx, &mut clock, "YCbCr (BT.601)");
        click(&mut app, &ctx, &mut clock, "RGB");
        assert_eq!(app.export_options.avif_color, 2);
        assert_eq!(app.export_options.avif_quality, 80);
        assert_eq!(app.export_options.avif_alpha_quality, 80);
        assert_eq!(app.export_options.avif_threads, 1);
        assert!(frame(&mut app, &ctx, &mut clock, vec![], "투명도 품질").is_some());
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        assert!(app.message.is_none());
    }
    #[test]
    fn jpeg_progressive_and_sampling_pointer_options_preserve_quality_background_and_history() {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Pos2> {
            *clock += 0.1;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |_| app.dialogs(ctx),
            );
            let p = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(t.pos + Vec2::splat(3.))
                }
                _ => None,
            });
            out.textures_delta.clear();
            p
        }
        fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
            for _ in 0..3 {
                frame(app, ctx, clock, vec![], target);
            }
            let p = frame(app, ctx, clock, vec![], target)
                .unwrap_or_else(|| panic!("missing {target}"));
            frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
            for pressed in [true, false] {
                frame(
                    app,
                    ctx,
                    clock,
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    }],
                    target,
                );
            }
        }
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.export_dialog = true;
        app.export_format = files::EXPORT_FORMATS
            .iter()
            .position(|f| f.0 == image::ImageFormat::Jpeg)
            .unwrap();
        click(&mut app, &ctx, &mut clock, "프로그레시브 저장");
        assert!(app.export_options.jpeg_progressive);
        assert!(frame(&mut app, &ctx, &mut clock, vec![], "스캔 수").is_some());
        click(&mut app, &ctx, &mut clock, "4:4:4");
        click(&mut app, &ctx, &mut clock, "4:2:0");
        assert_eq!(app.export_options.jpeg_sampling, 3);
        click(&mut app, &ctx, &mut clock, "압축 최적화");
        assert!(app.export_options.jpeg_optimize);
        assert_eq!(app.export_options.quality, 90);
        assert_eq!(app.export_options.background, [255; 3]);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        assert!(app.message.is_none());
    }
    #[test]
    fn gif_dither_alpha_and_interlace_pointer_options_preserve_background_and_history() {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Pos2> {
            *clock += 0.1;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |_| app.dialogs(ctx),
            );
            let p = out.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(t.pos + Vec2::splat(3.))
                }
                _ => None,
            });
            out.textures_delta.clear();
            p
        }
        fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
            for _ in 0..3 {
                frame(app, ctx, clock, vec![], target);
            }
            let p = frame(app, ctx, clock, vec![], target)
                .unwrap_or_else(|| panic!("missing {target}"));
            frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
            for pressed in [true, false] {
                frame(
                    app,
                    ctx,
                    clock,
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    }],
                    target,
                );
            }
        }
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.export_dialog = true;
        app.export_format = files::EXPORT_FORMATS
            .iter()
            .position(|f| f.0 == image::ImageFormat::Gif)
            .unwrap();
        click(&mut app, &ctx, &mut clock, "디더링");
        assert!(app.export_options.gif_dither);
        click(&mut app, &ctx, &mut clock, "인터레이스 저장");
        assert!(app.export_options.gif_interlaced);
        assert!(frame(&mut app, &ctx, &mut clock, vec![], "투명도 경계").is_some());
        click(&mut app, &ctx, &mut clock, "투명 영역 유지");
        assert!(!app.export_options.gif_transparent);
        assert!(frame(&mut app, &ctx, &mut clock, vec![], "투명도 경계").is_none());
        assert_eq!(app.export_options.gif_colors, 256);
        assert_eq!(app.export_options.quality, 90);
        assert_eq!(app.export_options.background, [255; 3]);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        assert!(app.message.is_none());
    }
    #[test]
    fn display_sampling_commands_change_existing_full_and_partial_textures_without_resampling_source_or_history()
     {
        use crate::display_sampling::Mode;
        for depth in [8, 16, 32] {
            let (mut app, ctx, _) = input_app(Tool::Brush);
            let pixels = image::Rgba32FImage::from_fn(256, 256, |x, y| {
                image::Rgba([if (x + y) % 2 == 0 { 1.5 } else { 0. }, 0.25, -0.125, 0.75])
            });
            let document = if depth == 8 {
                Document::from_image(
                    "display".into(),
                    image::DynamicImage::ImageRgba32F(pixels).to_rgba8(),
                )
            } else if depth == 16 {
                crate::precision::document(
                    "display".into(),
                    image::DynamicImage::ImageRgba16(
                        image::DynamicImage::ImageRgba32F(pixels).to_rgba16(),
                    ),
                )
                .unwrap()
            } else {
                crate::precision::document(
                    "display".into(),
                    image::DynamicImage::ImageRgba32F(pixels),
                )
                .unwrap()
            };
            app.tab_mut().unwrap().editor = Editor::new(document, false);
            prepare_preview(&mut app, &ctx);
            ctx.tex_manager().write().take_delta().clear();
            let source = app.tab().unwrap().editor.document.layers[0].pixels.clone();
            let precision = app.tab().unwrap().editor.document.layers[0]
                .precision
                .clone();
            let composite = app.tab().unwrap().composite.clone().unwrap();
            app.tab_mut().unwrap().view.zoom = 0.125;
            app.tab_mut().unwrap().view.pan = Vec2::new(7., 11.);
            app.tab_mut().unwrap().view.fit = false;
            for mode in [
                Mode::Nearest,
                Mode::HighQuality,
                Mode::Smooth,
                Mode::HighQuality,
            ] {
                app.execute_command(crate::commands::Command::DisplaySampling(mode), &ctx);
                let entries = app.command_entries();
                assert_eq!(
                    entries
                        .iter()
                        .filter(|e| matches!(
                            e.command,
                            crate::commands::Command::DisplaySampling(_)
                        ))
                        .count(),
                    3
                );
                let tab = app.tab_mut().unwrap();
                tab.apply_display_sampling(mode);
                let mut delta = ctx.tex_manager().write().take_delta();
                let changes: Vec<_> = delta
                    .set
                    .iter()
                    .filter(|(id, _)| **id == tab.texture.as_ref().unwrap().id())
                    .flat_map(|(_, deltas)| deltas.iter())
                    .collect();
                assert_eq!(changes.len(), 1);
                assert_eq!(changes[0].pos, Some([0, 0]));
                assert_eq!(changes[0].image.size(), [1, 1]);
                assert_eq!(changes[0].options, mode.options());
                delta.clear();
                tab.apply_display_sampling(mode);
                let mut unchanged = ctx.tex_manager().write().take_delta();
                assert!(unchanged.set.is_empty());
                unchanged.clear();
                assert_eq!(tab.composite.as_ref().unwrap(), &composite);
                assert!(std::sync::Arc::ptr_eq(
                    &source,
                    &tab.editor.document.layers[0].pixels
                ));
                if let Some(precision) = &precision {
                    assert!(std::sync::Arc::ptr_eq(
                        precision,
                        tab.editor.document.layers[0].precision.as_ref().unwrap()
                    ));
                }
                assert_eq!(tab.editor.history().count(), 0);
                assert_eq!(tab.view.zoom, 0.125);
                assert_eq!(tab.view.pan, Vec2::new(7., 11.));
            }
            let tab = app.tab_mut().unwrap();
            tab.editor.begin("display brush");
            document::paint_line(
                tab.editor.document.active_layer_mut().unwrap(),
                [40., 40.],
                [80., 60.],
                &document::Brush {
                    radius: 4.,
                    opacity: 1.,
                    hardness: 1.,
                    color: [255, 0, 0, 255],
                    erase: false,
                    mask: false,
                },
                None,
            );
            tab.damage_stroke([40., 40.], [80., 60.], 4.);
            tab.update_painted_region();
            let mut delta = ctx.tex_manager().write().take_delta();
            let patch = delta
                .set
                .iter()
                .filter(|(id, _)| **id == tab.texture.as_ref().unwrap().id())
                .flat_map(|(_, deltas)| deltas.iter())
                .next()
                .unwrap();
            assert!(patch.image.size()[0] < 256 && patch.image.size()[1] < 256);
            assert_eq!(patch.options, Mode::HighQuality.options());
            delta.clear();
            assert_eq!(
                tab.composite.as_ref().unwrap(),
                &tab.editor.document.composite()
            );
            tab.editor.finish();
            assert_eq!(tab.editor.history().count(), 1);
            let rendered = Rendered::new(tab.editor.document.composite());
            let (sender, receiver) = mpsc::channel();
            sender.send(rendered).unwrap();
            tab.render_job = Some(receiver);
            tab.render_dirty = false;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(1.),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    ..Default::default()
                },
                |ui| app.canvas_ui(ui, &ctx),
            );
            let texture = app.tab().unwrap().texture.as_ref().unwrap().id();
            let full = output
                .textures_delta
                .set
                .iter()
                .filter(|(id, _)| **id == texture)
                .flat_map(|(_, deltas)| deltas.iter())
                .find(|d| d.pos.is_none())
                .unwrap();
            assert_eq!(full.options, Mode::HighQuality.options());
            output.textures_delta.clear();
        }
        let ctx = egui::Context::default();
        let mut app = PhotoEditor::from_context(&ctx);
        app.execute_command(
            crate::commands::Command::DisplaySampling(Mode::HighQuality),
            &ctx,
        );
        assert_eq!(app.workspace.layout.display_sampling, Mode::HighQuality);
        app.new_dialog = true;
        app.execute_command(
            crate::commands::Command::DisplaySampling(Mode::Nearest),
            &ctx,
        );
        assert_eq!(app.workspace.layout.display_sampling, Mode::HighQuality);
    }
    #[test]
    fn export_modal_keeps_format_options_preview_and_footer_inside_small_window() {
        for format in 0..files::export_count() {
            let (mut app, ctx, _) = input_app(Tool::Brush);
            app.export_dialog = true;
            app.export_format = format;
            for i in 0..3 {
                let mut warm = ctx.run_ui(
                    egui::RawInput {
                        time: Some(i as f64 * 0.1),
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                        ..Default::default()
                    },
                    |_| app.dialogs(&ctx),
                );
                warm.textures_delta.clear();
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(1.),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                    ..Default::default()
                },
                |_| app.dialogs(&ctx),
            );
            output.textures_delta.clear();
            for text in ["출력 미리보기", "실시간 미리보기", "내보내기", "취소"]
            {
                let positions: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.job.text == text => Some(t.pos),
                        _ => None,
                    })
                    .collect();
                assert!(!positions.is_empty(), "format{format} missing{text}");
                assert!(
                    positions
                        .iter()
                        .all(|p| p.x >= 0. && p.x < 880. && p.y >= 0. && p.y < 620.),
                    "format{format} {text} {positions:?}"
                );
            }
            assert_eq!(app.tab().unwrap().editor.history().count(), 0);
            assert!(app.message.is_none());
        }
    }
    #[test]
    fn pcx_export_options_render_without_indexing_image_format_table() {
        let (mut app, ctx, _) = input_app(Tool::Move);
        app.export_dialog = true;
        app.export_format = files::EXPORT_FORMATS.len() + 1;
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    ..Default::default()
                },
                |_| app.dialogs(&ctx),
            );
            output.textures_delta.clear();
        }
        assert!(app.export_dialog);
        assert!(app.message.is_none());
    }
    #[test]
    fn external_file_dialog_protects_dirty_history_and_save_then_opens_new_version_separately() {
        external_file_dialog_flow(false);
    }
    #[test]
    fn external_file_same_size_time_content_dialog_preserves_edits_conflict_and_explicit_overwrite()
    {
        external_file_dialog_flow(true);
    }
    fn external_file_dialog_flow(same_metadata: bool) {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Pos2> {
            *clock += 0.4;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |_| {
                    app.poll_jobs(ctx);
                    app.poll_files(ctx);
                    app.shortcuts(ctx);
                    app.dialogs(ctx);
                },
            );
            let point = out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(t.pos + Vec2::splat(5.))
                }
                _ => None,
            });
            out.textures_delta.clear();
            point
        }
        fn wait(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            condition: impl Fn(&PhotoEditor) -> bool,
        ) {
            let end = Instant::now() + Duration::from_secs(5);
            while !condition(app) {
                assert!(Instant::now() < end, "background operation did not finish");
                frame(app, ctx, clock, vec![], "");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
            frame(app, ctx, clock, vec![], target);
            frame(app, ctx, clock, vec![], target);
            let p = frame(app, ctx, clock, vec![], target)
                .unwrap_or_else(|| panic!("missing button {target}"));
            frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
            frame(
                app,
                ctx,
                clock,
                vec![
                    egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    },
                ],
                target,
            );
        }
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if same_metadata {
            "qa-output/file-watch-content-ui"
        } else {
            "qa-output/file-watch-ui"
        });
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("document.rphoto");
        files::save_project(&app.tab().unwrap().editor.document, &path).unwrap();
        let tab = app.tab_mut().unwrap();
        tab.path = Some(path.clone());
        tab.file_watch
            .reset(crate::file_watch::snapshot(&path).unwrap());
        tab.editor.saved_revision = Some(tab.editor.revision);
        tab.editor.edit("user edit", |doc| {
            doc.layers[0].name = "unsaved edit".into()
        });
        let id = tab.editor.document.document_id.clone();
        let revision = tab.editor.revision;
        let mut external_bytes = b"external replacement".to_vec();
        if same_metadata {
            let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
            external_bytes = std::fs::read(&path).unwrap();
            let last = external_bytes.len() - 1;
            external_bytes[last] ^= 0x51;
            std::fs::write(&path, &external_bytes).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
            app.tab_mut().unwrap().file_watch.check_content_now();
        } else {
            std::fs::write(&path, &external_bytes).unwrap();
        }
        wait(&mut app, &ctx, &mut clock, |app| {
            app.external_dialog.is_some()
        });
        frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Key {
                key: Key::E,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            "",
        );
        assert_eq!(app.tool, Tool::Brush);
        click(&mut app, &ctx, &mut clock, "현재 문서 계속 편집");
        assert!(app.external_dialog.is_none());
        assert!(app.tab().unwrap().editor.dirty());
        assert_eq!(app.tab().unwrap().editor.revision, revision);
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        app.save(false, &ctx);
        wait(&mut app, &ctx, &mut clock, |app| {
            app.external_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.saving)
        });
        assert_eq!(std::fs::read(&path).unwrap(), external_bytes);
        click(&mut app, &ctx, &mut clock, "현재 편집으로 덮어쓰기");
        wait(&mut app, &ctx, &mut clock, |app| app.busy.is_none());
        assert!(app.message.is_none());
        assert!(!app.tab().unwrap().editor.dirty());
        assert_eq!(files::open(&path).unwrap().document.document_id, id);
        let external = Document::new(4, 3, true).unwrap();
        let external_id = external.document_id.clone();
        files::save_project(&external, &path).unwrap();
        wait(&mut app, &ctx, &mut clock, |app| {
            app.external_dialog.is_some()
        });
        click(&mut app, &ctx, &mut clock, "새 버전을 별도 탭으로 열기");
        wait(&mut app, &ctx, &mut clock, |app| app.busy.is_none());
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[0].editor.document.document_id, id);
        assert_eq!(app.tabs[0].editor.history().count(), 1);
        assert_eq!(app.tabs[1].editor.document.document_id, external_id);
    }
    #[test]
    fn pattern_dialog_cancel_and_apply_keep_source_selection_and_one_undo() {
        fn frame(
            app: &mut PhotoEditor,
            ctx: &egui::Context,
            clock: &mut f64,
            events: Vec<egui::Event>,
            target: &str,
        ) -> Option<Pos2> {
            *clock += 0.4;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(*clock),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                    events,
                    ..Default::default()
                },
                |_| app.dialogs(ctx),
            );
            let position = output.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.job.text == target => {
                    Some(t.pos + Vec2::splat(5.))
                }
                _ => None,
            });
            output.textures_delta.clear();
            position
        }
        fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
            let p = (0..3)
                .find_map(|_| frame(app, ctx, clock, vec![], target))
                .unwrap_or_else(|| panic!("missing button: {target}"));
            frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
            frame(
                app,
                ctx,
                clock,
                vec![
                    egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos: p,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    },
                ],
                target,
            );
        }
        let (mut app, ctx, mut clock) = input_app(Tool::Fill);
        app.tab_mut().unwrap().selection = Some(Selection {
            min: [40, 50],
            max: [80, 90],
            mask: None,
            inverted: false,
        });
        app.open_pattern();
        click(&mut app, &ctx, &mut clock, "취소");
        assert!(app.content_dialog.is_none());
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        app.clipboard = Some(image::DynamicImage::ImageRgba8(crate::pattern::preset(
            1,
            [[255, 0, 0, 255], [0, 0, 255, 128]],
        )));
        app.open_pattern();
        click(&mut app, &ctx, &mut clock, "복사한 이미지");
        let Content::Svg { source } = &app.content_dialog.as_ref().unwrap().3 else {
            unreachable!()
        };
        assert_eq!(
            crate::pattern::Pattern::read(source)
                .unwrap()
                .settings
                .tile_size,
            [16, 16]
        );
        click(&mut app, &ctx, &mut clock, "적용");
        assert!(app.content_dialog.is_none());
        let doc = &app.tab().unwrap().editor.document;
        let layer = doc.active_layer().unwrap();
        assert!(crate::pattern::Pattern::read(&layer.content.as_ref().unwrap().svg()).is_some());
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(0, 0)[0], 0);
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(50, 60)[0], 255);
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 1);
        app.tab_mut().unwrap().editor.redo();
        assert_eq!(app.tab().unwrap().editor.document.layers.len(), 2);
        for gradient in [false, true] {
            let (mut app, ctx, mut clock) = input_app(Tool::Fill);
            app.color = [23, 45, 67, 89];
            app.tab_mut().unwrap().selection = Some(Selection {
                min: [40, 50],
                max: [80, 90],
                mask: None,
                inverted: false,
            });
            app.open_fill(gradient);
            click(&mut app, &ctx, &mut clock, "취소");
            assert_eq!(app.tab().unwrap().editor.history().count(), 0);
            app.open_fill(gradient);
            click(&mut app, &ctx, &mut clock, "적용");
            let layer = app.tab().unwrap().editor.document.active_layer().unwrap();
            let id = layer.id;
            let mask = layer.mask.clone();
            let offset = layer.offset;
            assert_eq!(mask.as_ref().unwrap().get_pixel(50, 60)[0], 255);
            let content = layer.content.clone().unwrap();
            assert!(matches!(
                content,
                Content::Solid {
                    color: [23, 45, 67, 89],
                    ..
                } | Content::Gradient { .. }
            ));
            assert_eq!(app.tab().unwrap().editor.history().count(), 1);
            let mut changed = content.clone();
            match &mut changed {
                Content::Solid { color, .. } => *color = [200, 30, 50, 128],
                Content::Gradient { colors, stops, .. } => {
                    stops[0].color = [200, 30, 50, 128];
                    colors[0] = stops[0].color;
                }
                _ => unreachable!(),
            }
            let tab_id = app.tab().unwrap().id;
            app.content_dialog = Some((tab_id, Some(id), offset, changed.clone()));
            frame(&mut app, &ctx, &mut clock, vec![], "적용");
            frame(&mut app, &ctx, &mut clock, vec![], "적용");
            click(&mut app, &ctx, &mut clock, "적용");
            assert!(app.content_dialog.is_none(), "{:?}", app.content_dialog);
            let layer = app.tab().unwrap().editor.document.active_layer().unwrap();
            assert_eq!(layer.content.as_ref(), Some(&changed));
            assert_eq!(layer.mask, mask);
            assert_eq!(app.tab().unwrap().editor.history().count(), 2);
            app.tab_mut().unwrap().editor.undo();
            assert_eq!(
                app.tab()
                    .unwrap()
                    .editor
                    .document
                    .active_layer()
                    .unwrap()
                    .content
                    .as_ref(),
                Some(&content)
            );
            app.tab_mut().unwrap().editor.undo();
            assert_eq!(app.tab().unwrap().editor.document.layers.len(), 1);
            app.tab_mut().unwrap().editor.redo();
            assert_eq!(
                app.tab()
                    .unwrap()
                    .editor
                    .document
                    .active_layer()
                    .unwrap()
                    .content
                    .as_ref(),
                Some(&content)
            );
        }
    }
    #[test]
    fn status_coordinates_and_tool_cursors_follow_zoom_pan_and_dpi() {
        for dpi in [1., 2.] {
            for (tool, expected) in [
                (Tool::Hand, egui::CursorIcon::Grab),
                (Tool::Text, egui::CursorIcon::Text),
                (Tool::Zoom, egui::CursorIcon::ZoomIn),
                (Tool::Brush, egui::CursorIcon::Crosshair),
            ] {
                let (mut app, ctx, _) = input_app(tool);
                ctx.set_pixels_per_point(dpi);
                app.tab_mut().unwrap().view.zoom = 1.5;
                app.tab_mut().unwrap().view.pan = Vec2::new(31., -22.);
                let raw = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(512.))),
                    ..Default::default()
                };
                let mut first = ctx.run_ui(raw.clone(), |ui| {
                    app.status_ui(ui, &ctx);
                    app.canvas_ui(ui, &ctx);
                });
                first.textures_delta.clear();
                let tab = app.tab().unwrap();
                let rect = tab
                    .view
                    .image_rect(tab.canvas_area.unwrap(), [256, 256], dpi);
                let pointer = rect.min + Vec2::new(80.5, 120.25) * 1.5 / dpi;
                let mut input = raw.clone();
                input.events = vec![egui::Event::PointerMoved(pointer)];
                let mut output = ctx.run_ui(input, |ui| {
                    app.status_ui(ui, &ctx);
                    app.canvas_ui(ui, &ctx);
                });
                assert_eq!(output.platform_output.cursor_icon, expected);
                assert!(output.shapes.iter().any(|c|matches!(&c.shape,egui::Shape::Text(t)if t.galley.job.text.starts_with("X: 80.5  Y: 120."))));
                output.textures_delta.clear();
                let mut input = raw;
                input.events = vec![egui::Event::PointerMoved(Pos2::new(510., 510.))];
                let mut output = ctx.run_ui(input, |ui| {
                    app.status_ui(ui, &ctx);
                    app.canvas_ui(ui, &ctx);
                });
                assert!(!output.shapes.iter().any(|c|matches!(&c.shape,egui::Shape::Text(t)if t.galley.job.text.starts_with("X:"))));
                output.textures_delta.clear();
            }
        }
    }
    #[test]
    fn customized_shortcuts_dispatch_exact_modifiers_and_respect_modal_and_text_focus() {
        use crate::shortcuts::Action;
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.workspace
            .layout
            .shortcuts
            .assign(Action::Tool(Tool::Brush), Some((Key::F8, Modifiers::NONE)))
            .unwrap();
        app.workspace
            .layout
            .shortcuts
            .assign(Action::Undo, Some((Key::F9, Modifiers::NONE)))
            .unwrap();
        app.workspace
            .layout
            .shortcuts
            .assign(Action::Redo, Some((Key::F10, Modifiers::NONE)))
            .unwrap();
        for (key, expected) in [(Key::B, Tool::Move), (Key::F8, Tool::Brush)] {
            input_frame(
                &mut app,
                &ctx,
                &mut clock,
                vec![key_event(key)],
                Modifiers::NONE,
            );
            assert_eq!(app.tool, expected);
        }
        app.set_tool(Tool::Move);
        let mut event = key_event(Key::F8);
        if let egui::Event::Key { modifiers, .. } = &mut event {
            *modifiers = Modifiers::SHIFT;
        }
        input_frame(&mut app, &ctx, &mut clock, vec![event], Modifiers::SHIFT);
        assert_eq!(app.tool, Tool::Move);
        app.edit("test", |doc| doc.width = 128);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::F9)],
            Modifiers::NONE,
        );
        assert_eq!(app.tab().unwrap().editor.document.width, 256);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::F10)],
            Modifiers::NONE,
        );
        assert_eq!(app.tab().unwrap().editor.document.width, 128);
        app.shortcuts_panel.open = true;
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::F8)],
            Modifiers::NONE,
        );
        assert_eq!(app.tool, Tool::Move);
        app.shortcuts_panel.open = false;
        app.text_session = Some(crate::text_canvas::Session::new(
            app.tab().unwrap().id,
            [0., 0.],
            app.color,
        ));
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::F8)],
            Modifiers::NONE,
        );
        assert_eq!(app.tool, Tool::Move);
        app.text_session = None;
        // A numeric/text property with focus must retain even a reassigned plain key.
        let mut value = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.text_edit_singleline(&mut value).request_focus();
        });
        output.textures_delta.clear();
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::F8)],
            Modifiers::NONE,
        );
        assert_eq!(app.tool, Tool::Move);
    }
    #[test]
    fn eyedropper_uses_latest_pixels_press_modifiers_and_color_shortcuts_respect_modal() {
        let (mut app, ctx, mut clock) = input_app(Tool::Eyedropper);
        app.sample_merged = true;
        app.sample_size = 3;
        let tab = app.tab_mut().unwrap();
        tab.editor.document.layers[0].pixels = Arc::new(image::RgbaImage::from_pixel(
            256,
            256,
            image::Rgba([230, 40, 60, 255]),
        ));
        tab.composite = Some(image::RgbaImage::from_pixel(
            256,
            256,
            image::Rgba([0, 0, 255, 255]),
        ));
        let original = app.color;
        let point = Pos2::new(168.0, 168.0);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(point)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerButton {
                    pos: point,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::ALT,
                },
                egui::Event::PointerButton {
                    pos: point,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        assert_eq!(app.color, original);
        assert_eq!(app.background, [230, 40, 60, 255]);
        app.sample_background = true;
        click_at(&mut app, &ctx, &mut clock, point, Modifiers::ALT);
        assert_eq!(app.color, [230, 40, 60, 255]);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        app.background = [20, 30, 40, 50];
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::X)],
            Modifiers::NONE,
        );
        assert_eq!(app.color, [20, 30, 40, 50]);
        assert_eq!(app.background, [230, 40, 60, 255]);
        app.colors_panel.open = true;
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::D)],
            Modifiers::NONE,
        );
        assert_eq!(app.color, [20, 30, 40, 50]);
        app.colors_panel.open = false;
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::D)],
            Modifiers::NONE,
        );
        assert_eq!(app.color, [0, 0, 0, 255]);
        assert_eq!(app.background, [255; 4]);
    }

    #[test]
    fn gradient_drag_captures_reverse_interpolation_and_one_undo() {
        let (mut app, ctx, mut clock) = input_app(Tool::Gradient);
        app.color = [0, 0, 0, 255];
        app.background = [255; 4];
        app.gradient_reversed = true;
        app.gradient_interpolation = crate::vector::GradientInterpolation::LinearRgb;
        let start = Pos2::new(168.0, 168.0);
        let end = Pos2::new(328.0, 208.0);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(start)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerMoved(end),
                egui::Event::PointerButton {
                    pos: end,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        let tab = app.tab_mut().unwrap();
        assert_eq!(tab.editor.history().count(), 1);
        let content = tab
            .editor
            .document
            .active_layer()
            .unwrap()
            .content
            .clone()
            .unwrap();
        if let Content::Gradient {
            colors,
            interpolation,
            start,
            end,
            ..
        } = &content
        {
            assert_eq!(*colors, [[255; 4], [0, 0, 0, 255]]);
            assert_eq!(
                *interpolation,
                crate::vector::GradientInterpolation::LinearRgb
            );
            assert_eq!(*start, [40.0, 40.0]);
            assert_eq!(*end, [200.0, 80.0]);
        } else {
            panic!("Expected an editable gradient");
        }
        tab.editor.undo();
        assert_eq!(tab.editor.document.layers.len(), 1);
        tab.editor.redo();
        assert_eq!(
            tab.editor.document.active_layer().unwrap().content.as_ref(),
            Some(&content)
        );
    }

    #[test]
    fn selection_boundary_and_pixel_drag_copy_cancel_and_undo_use_original_snapshot() {
        for (target, alt) in [
            (MoveTarget::Boundary, false),
            (MoveTarget::Pixels, false),
            (MoveTarget::Pixels, true),
        ] {
            let (mut app, ctx, mut time) = input_app(Tool::Move);
            app.move_target = target;
            let selection = Selection::rectangle([64, 64], [80, 80]);
            app.tab_mut().unwrap().selection = Some(selection.clone());
            app.tab_mut().unwrap().editor.document.layers[0].pixels =
                Arc::new(image::RgbaImage::from_fn(256, 256, |x, y| {
                    image::Rgba(if selection.contains(x as i32, y as i32) {
                        [255, 0, 0, 255]
                    } else {
                        [0; 4]
                    })
                }));
            let before = app.tab().unwrap().editor.document.layers[0].pixels.clone();
            let start = Pos2::new(196.0, 196.0);
            let end = start + Vec2::new(32.0, 0.0);
            let modifiers = if alt { Modifiers::ALT } else { Modifiers::NONE };
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![egui::Event::PointerMoved(start)],
                modifiers,
            );
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![
                    egui::Event::PointerButton {
                        pos: start,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    },
                    egui::Event::PointerMoved(end),
                    egui::Event::PointerButton {
                        pos: end,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers,
                    },
                ],
                modifiers,
            );
            assert_eq!(app.tab().unwrap().selection.as_ref().unwrap().min, [96, 64]);
            let tab = app.tab_mut().unwrap();
            if target == MoveTarget::Boundary {
                assert!(Arc::ptr_eq(&before, &tab.editor.document.layers[0].pixels));
                assert_eq!(tab.editor.history().count(), 0);
            } else {
                let rendered = tab.editor.document.composite();
                assert_eq!(rendered.get_pixel(100, 68).0, [255, 0, 0, 255]);
                assert_eq!(rendered.get_pixel(68, 68)[3], if alt { 255 } else { 0 });
                assert_eq!(tab.editor.history().count(), 1);
                tab.editor.undo();
                assert!(Arc::ptr_eq(&before, &tab.editor.document.layers[0].pixels));
                tab.editor.redo();
                assert_eq!(tab.editor.document.composite(), rendered);
            }
            let previous = app.tab().unwrap().selection.clone();
            let before_pixels = app.tab().unwrap().editor.document.layers[0].pixels.clone();
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![egui::Event::PointerMoved(start)],
                Modifiers::NONE,
            );
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![
                    egui::Event::PointerButton {
                        pos: start,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    egui::Event::PointerMoved(end),
                ],
                Modifiers::NONE,
            );
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![key_event(Key::Escape)],
                Modifiers::NONE,
            );
            assert_eq!(app.tab().unwrap().selection, previous);
            assert!(Arc::ptr_eq(
                &before_pixels,
                &app.tab().unwrap().editor.document.layers[0].pixels
            ));
        }
    }
    #[test]
    fn canvas_transform_preview_releases_without_commit_and_apply_cancel_preserve_assets() {
        let (mut app, ctx, mut time) = input_app(Tool::Brush);
        let original = app.tab().unwrap().editor.document.layers[0].pixels.clone();
        let start = Pos2::new(384.0, 384.0);
        let end = Pos2::new(448.0, 416.0);
        for apply in [false, true] {
            app.start_transform();
            assert!(app.transforming);
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![egui::Event::PointerMoved(start)],
                Modifiers::NONE,
            );
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![
                    egui::Event::PointerButton {
                        pos: start,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    egui::Event::PointerMoved(end),
                    egui::Event::PointerButton {
                        pos: end,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    },
                ],
                Modifiers::NONE,
            );
            let tab = app.tab().unwrap();
            assert!(tab.editor.is_editing());
            assert_eq!(tab.editor.revision, 0);
            assert_eq!(tab.editor.history().count(), 0);
            assert_eq!(tab.editor.document.layers[0].transform.scale, [1.25, 1.125]);
            assert!(Arc::ptr_eq(
                &original,
                &tab.editor.document.layers[0].pixels
            ));
            let key = if apply { Key::Enter } else { Key::Escape };
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![key_event(key)],
                Modifiers::NONE,
            );
            assert!(!app.transforming);
            assert!(!app.tab().unwrap().editor.is_editing());
            if apply {
                assert_eq!(app.tab().unwrap().editor.history().count(), 1);
                app.tab_mut().unwrap().editor.undo();
            }
            assert_eq!(
                app.tab().unwrap().editor.document.layers[0].transform,
                crate::transform::Transform::default()
            );
            assert!(Arc::ptr_eq(
                &original,
                &app.tab().unwrap().editor.document.layers[0].pixels
            ));
        }
    }
    #[test]
    fn selection_tools_use_smoothing_option_in_pointer_input() {
        for tool in [Tool::Ellipse, Tool::Lasso, Tool::Wand] {
            for smooth in [false, true] {
                let (mut app, ctx, mut clock) = input_app(tool);
                app.workspace.layout.selection_smooth = smooth;
                if tool == Tool::Ellipse {
                    drag_at(
                        &mut app,
                        &ctx,
                        &mut clock,
                        Pos2::new(148., 148.),
                        Pos2::new(165., 161.),
                        Modifiers::NONE,
                    );
                } else if tool == Tool::Lasso {
                    app.polygon_lasso = true;
                    for p in [
                        Pos2::new(148.25, 148.25),
                        Pos2::new(168.75, 150.75),
                        Pos2::new(150.25, 167.5),
                    ] {
                        click_at(&mut app, &ctx, &mut clock, p, Modifiers::NONE);
                    }
                    input_frame(
                        &mut app,
                        &ctx,
                        &mut clock,
                        vec![key_event(Key::Enter)],
                        Modifiers::NONE,
                    );
                } else {
                    app.tolerance = 0;
                    app.tab_mut().unwrap().editor.document.layers[0].pixels =
                        Arc::new(image::RgbaImage::from_fn(256, 256, |x, _| {
                            image::Rgba([if x < 128 { 0 } else { 255 }, 0, 0, 255])
                        }));
                    click_at(
                        &mut app,
                        &ctx,
                        &mut clock,
                        Pos2::new(148., 148.),
                        Modifiers::NONE,
                    );
                }
                let tab = app.tab().unwrap();
                let s = tab.selection.as_ref().unwrap();
                let partial = s
                    .mask
                    .as_ref()
                    .unwrap()
                    .pixels()
                    .any(|p| p[0] > 0 && p[0] < 255);
                assert_eq!(partial, smooth, "{tool:?}");
                assert_eq!(tab.editor.history().count(), 0);
                assert!(app.message.is_none());
            }
        }
    }
    #[test]
    fn canvas_brush_expands_mask_outside_small_layer_and_undo_restores_original() {
        let (mut app, ctx, mut clock) = input_app(Tool::Brush);
        app.color = [255; 4];
        app.brush_size = 1.5;
        app.brush_hardness = 1.;
        let tab = app.tab_mut().unwrap();
        tab.edit_mask = true;
        let l = &mut tab.editor.document.layers[0];
        l.pixels = Arc::new(image::RgbaImage::from_pixel(8, 8, image::Rgba([255; 4])));
        l.offset = [128, 128];
        l.mask = Some(Arc::new(image::GrayImage::from_pixel(
            8,
            8,
            image::Luma([255]),
        )));
        let mask = l.mask.clone().unwrap();
        let pixels = l.pixels.clone();
        click_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(218.5, 228.5),
            Modifiers::NONE,
        );
        assert!(app.message.is_none());
        let tab = app.tab().unwrap();
        let l = &tab.editor.document.layers[0];
        assert_eq!(crate::masks::document(l).unwrap().value([90.5, 100.5]), 1.);
        assert!(l.mask.as_ref().unwrap().width() > 8);
        assert!(Arc::ptr_eq(&pixels, &l.pixels));
        assert_eq!(tab.editor.history().count(), 1);
        assert_eq!(tab.editor.document.composite().get_pixel(90, 100)[3], 0);
        app.tab_mut().unwrap().editor.undo();
        let l = &app.tab().unwrap().editor.document.layers[0];
        assert!(Arc::ptr_eq(&mask, l.mask.as_ref().unwrap()));
        assert!(l.mask_placement.is_none());
    }
    #[test]
    fn canvas_mask_transform_drag_release_apply_cancel_keeps_layer_and_raw_mask() {
        let (mut app, ctx, mut time) = input_app(Tool::Brush);
        let tab = app.tab_mut().unwrap();
        tab.edit_mask = true;
        let layer = &mut tab.editor.document.layers[0];
        layer.mask = Some(Arc::new(image::GrayImage::from_fn(256, 256, |x, _| {
            image::Luma([if x < 128 { 255 } else { 0 }])
        })));
        let mask = layer.mask.clone().unwrap();
        let pixels = layer.pixels.clone();
        for apply in [false, true] {
            app.start_transform();
            assert!(
                app.tab()
                    .unwrap()
                    .transform_frame
                    .as_ref()
                    .unwrap()
                    .mask
                    .is_some()
            );
            drag_at(
                &mut app,
                &ctx,
                &mut time,
                Pos2::new(384., 384.),
                Pos2::new(448., 416.),
                Modifiers::NONE,
            );
            let tab = app.tab().unwrap();
            let l = &tab.editor.document.layers[0];
            assert_eq!(l.transform, Default::default());
            assert_eq!(l.offset, [0, 0]);
            assert!(!crate::masks::linked(l));
            assert!(Arc::ptr_eq(&mask, l.mask.as_ref().unwrap()));
            assert!(Arc::ptr_eq(&pixels, &l.pixels));
            assert_eq!(tab.editor.history().count(), 0);
            let world = crate::masks::world(l);
            let corner = crate::matrix::point(world, [256., 256.]);
            assert!(
                (corner[0] - 320.).abs() < 0.001 && (corner[1] - 288.).abs() < 0.001,
                "{corner:?}"
            );
            assert!(tab.transform_frame.as_ref().unwrap().mask.is_some());
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![key_event(if apply { Key::Enter } else { Key::Escape })],
                Modifiers::NONE,
            );
            if apply {
                assert_eq!(app.tab().unwrap().editor.history().count(), 1);
                app.tab_mut().unwrap().editor.undo();
            }
            assert!(
                app.tab().unwrap().editor.document.layers[0]
                    .mask_placement
                    .is_none()
            );
        }
    }
    fn drag_at(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        time: &mut f64,
        start: Pos2,
        end: Pos2,
        modifiers: Modifiers,
    ) {
        input_frame(
            app,
            ctx,
            time,
            vec![egui::Event::PointerMoved(start)],
            modifiers,
        );
        input_frame(
            app,
            ctx,
            time,
            vec![
                egui::Event::PointerButton {
                    pos: start,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
                egui::Event::PointerMoved(end),
                egui::Event::PointerButton {
                    pos: end,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers,
                },
            ],
            modifiers,
        );
    }
    #[test]
    fn transform_handles_snap_actual_geometry_with_grab_offset_and_one_undo() {
        let (mut app, ctx, mut time) = input_app(Tool::Move);
        app.tab_mut().unwrap().editor.document.layout.guides = vec![
            crate::guides::Guide {
                axis: 0,
                position: 315.,
            },
            crate::guides::Guide {
                axis: 1,
                position: 295.,
            },
        ];
        app.tab_mut().unwrap().editor.document.layout.snap = true;
        app.start_transform();
        drag_at(
            &mut app,
            &ctx,
            &mut time,
            Pos2::new(380., 380.),
            Pos2::new(438., 418.),
            Modifiers::NONE,
        );
        let tab = app.tab().unwrap();
        let l = &tab.editor.document.layers[0];
        let corner = l.transform.to_document([256., 256.], l.offset, [256, 256]);
        assert!(
            (corner[0] - 315.).abs() < 0.002 && (corner[1] - 295.).abs() < 0.002,
            "{corner:?}"
        );
        assert_eq!(tab.editor.history().count(), 0);
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![key_event(Key::Enter)],
            Modifiers::NONE,
        );
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0].transform,
            Default::default()
        );
    }
    #[test]
    fn canvas_group_distortion_and_multi_layer_scale_keep_assets_and_cancel() {
        for grouped in [false, true] {
            let (mut app, ctx, mut time) = input_app(Tool::Move);
            let doc = &mut app.tab_mut().unwrap().editor.document;
            doc.add_layer(
                "둘째".into(),
                image::RgbaImage::from_pixel(32, 32, image::Rgba([255, 0, 0, 255])),
            )
            .unwrap();
            let ids = doc.layers.iter().map(|l| l.id).collect::<Vec<_>>();
            if grouped {
                doc.group_selected(&ids).unwrap();
            } else {
                app.tab_mut().unwrap().selected_layers = ids;
            }
            let before = app.tab().unwrap().editor.document.clone();
            app.start_transform();
            assert!(app.transforming);
            if grouped {
                app.tab_mut()
                    .unwrap()
                    .transform_frame
                    .as_mut()
                    .unwrap()
                    .distort = true;
                drag_at(
                    &mut app,
                    &ctx,
                    &mut time,
                    Pos2::new(128., 128.),
                    Pos2::new(145., 139.),
                    Modifiers::NONE,
                );
            } else {
                drag_at(
                    &mut app,
                    &ctx,
                    &mut time,
                    Pos2::new(384., 384.),
                    Pos2::new(416., 408.),
                    Modifiers::NONE,
                );
            }
            assert!(app.tab().unwrap().editor.is_editing());
            assert_eq!(app.tab().unwrap().editor.history().count(), 0);
            for (a, b) in before
                .layers
                .iter()
                .zip(&app.tab().unwrap().editor.document.layers)
            {
                assert!(Arc::ptr_eq(&a.pixels, &b.pixels));
                assert_eq!(a.parent, b.parent);
                assert_ne!(a.transform, b.transform);
            }
            input_frame(
                &mut app,
                &ctx,
                &mut time,
                vec![key_event(Key::Escape)],
                Modifiers::NONE,
            );
            assert!(!app.transforming);
            assert_eq!(
                app.tab().unwrap().editor.document.composite(),
                before.composite()
            );
            assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        }
    }
    #[test]
    fn crop_handles_move_snap_cancel_modal_and_enter_keep_original_and_one_history() {
        let (mut app, ctx, mut time) = input_app(Tool::Move);
        let previous = Some(editing::ellipse([48, 48], [192, 192]));
        app.tab_mut().unwrap().selection = previous.clone();
        app.set_tool(Tool::Crop);
        app.tab_mut().unwrap().editor.document.layout.guides = vec![
            crate::guides::Guide {
                axis: 0,
                position: 224.,
            },
            crate::guides::Guide {
                axis: 1,
                position: 216.,
            },
        ];
        app.tab_mut().unwrap().editor.document.layout.snap = true;
        let original = app.tab().unwrap().editor.document.layers[0].pixels.clone();
        drag_at(
            &mut app,
            &ctx,
            &mut time,
            Pos2::new(316., 316.),
            Pos2::new(347., 339.),
            Modifiers::NONE,
        );
        let selected = app.tab().unwrap().selection.as_ref().unwrap();
        assert_eq!(selected.min, [48, 48]);
        assert_eq!(selected.max, [224, 216]);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        app.message = Some(("모달".into(), "테스트".into()));
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![key_event(Key::Escape)],
            Modifiers::NONE,
        );
        assert_eq!(
            app.tab().unwrap().selection.as_ref().unwrap().max,
            [224, 216]
        );
        app.message = None;
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![key_event(Key::Escape)],
            Modifiers::NONE,
        );
        assert_eq!(app.tab().unwrap().selection, previous);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        drag_at(
            &mut app,
            &ctx,
            &mut time,
            Pos2::new(256., 256.),
            Pos2::new(265., 268.),
            Modifiers::NONE,
        );
        let selected = app.tab().unwrap().selection.as_ref().unwrap();
        assert_eq!(selected.min, [56, 56]);
        assert_eq!(selected.max, [200, 200]);
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![key_event(Key::Enter)],
            Modifiers::NONE,
        );
        let tab = app.tab().unwrap();
        assert_eq!(
            [tab.editor.document.width, tab.editor.document.height],
            [144, 144]
        );
        assert_eq!(tab.editor.history().count(), 1);
        assert!(Arc::ptr_eq(
            &original,
            &tab.editor.document.layers[0].pixels
        ));
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(app.tab().unwrap().editor.document.width, 256);
    }
    #[test]
    fn polygon_clicks_wait_for_enter_and_escape_restores_previous_selection() {
        let (mut app, ctx, mut time) = input_app(Tool::Lasso);
        app.polygon_lasso = true;
        for position in [
            Pos2::new(192.0, 192.0),
            Pos2::new(320.0, 192.0),
            Pos2::new(192.0, 320.0),
        ] {
            click_at(&mut app, &ctx, &mut time, position, Modifiers::NONE);
            assert!(matches!(
                app.tab().unwrap().gesture,
                Some(Gesture::Polygon { .. })
            ));
        }
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![key_event(Key::Enter)],
            Modifiers::NONE,
        );
        let selection = app.tab().unwrap().selection.clone().unwrap();
        assert!(selection.contains(80, 80));
        assert!(!selection.contains(180, 180));
        assert!(app.tab().unwrap().gesture.is_none());
        click_at(
            &mut app,
            &ctx,
            &mut time,
            Pos2::new(208.0, 208.0),
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![key_event(Key::Escape)],
            Modifiers::NONE,
        );
        assert_eq!(app.tab().unwrap().selection.as_ref(), Some(&selection));
        assert!(!app.tab().unwrap().editor.can_undo());
    }
    #[test]
    fn smoothing_flushes_endpoint_and_shift_click_connects_one_undo_stroke() {
        let (mut app, ctx, mut time) = input_app(Tool::Brush);
        app.brush_smoothing = 0.8;
        let start = Pos2::new(192.0, 192.0);
        let end = Pos2::new(256.0, 256.0);
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::PointerMoved(start)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::PointerButton {
                pos: start,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::PointerMoved(end)],
            Modifiers::NONE,
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .pixels
                .get_pixel(128, 128)
                .0,
            [255; 4]
        );
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::PointerButton {
                pos: end,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .pixels
                .get_pixel(128, 128)
                .0,
            app.color
        );
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .pixels
                .get_pixel(96, 96)
                .0,
            [255; 4]
        );
        click_at(&mut app, &ctx, &mut time, start, Modifiers::NONE);
        click_at(
            &mut app,
            &ctx,
            &mut time,
            end,
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .pixels
                .get_pixel(96, 96)
                .0,
            app.color
        );
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .pixels
                .get_pixel(96, 96)
                .0,
            [255; 4]
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layers[0]
                .pixels
                .get_pixel(64, 64)
                .0,
            app.color
        );
    }
    #[test]
    fn ruler_drag_creates_moves_and_deletes_guide_with_undo() {
        let (mut app, ctx, mut time) = input_app(Tool::Move);
        app.tab_mut().unwrap().editor.document.layout.rulers = true;
        let ruler = Pos2::new(300.0, 10.0);
        let destination = Pos2::new(300.0, 238.0);
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::PointerMoved(ruler)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![
                egui::Event::PointerButton {
                    pos: ruler,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerMoved(destination),
                egui::Event::PointerButton {
                    pos: destination,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        let guides = &app.tab().unwrap().editor.document.layout.guides;
        assert_eq!(
            guides,
            &vec![crate::guides::Guide {
                axis: 1,
                position: 100.0
            }]
        );
        let moved = Pos2::new(300.0, 258.0);
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::PointerMoved(destination)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![
                egui::Event::PointerButton {
                    pos: destination,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerMoved(moved),
                egui::Event::PointerButton {
                    pos: moved,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        assert_eq!(
            app.tab().unwrap().editor.document.layout.guides[0].position,
            120.0
        );
        input_frame(
            &mut app,
            &ctx,
            &mut time,
            vec![
                egui::Event::PointerButton {
                    pos: moved,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerMoved(ruler),
                egui::Event::PointerButton {
                    pos: ruler,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        assert!(app.tab().unwrap().editor.document.layout.guides.is_empty());
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(
            app.tab().unwrap().editor.document.layout.guides[0].position,
            120.0
        );
        assert_eq!(app.tab().unwrap().editor.document.layers[0].offset, [0, 0]);
    }
    #[test]
    fn clone_alignment_preserves_source_offset_between_strokes() {
        for aligned in [true, false] {
            let (mut app, ctx, mut time) = input_app(Tool::Stamp);
            app.clone_aligned = aligned;
            app.brush_size = 1.2;
            app.brush_hardness = 1.0;
            app.tab_mut().unwrap().editor.document.layers[0].pixels =
                Arc::new(image::RgbaImage::from_fn(256, 256, |x, _| {
                    image::Rgba([x as u8, 0, 0, 255])
                }));
            app.tab_mut().unwrap().clone_origin = Some([64.5, 64.5]);
            for x in [128.5, 160.5] {
                click_at(
                    &mut app,
                    &ctx,
                    &mut time,
                    Pos2::new(128.0 + x, 192.5),
                    Modifiers::NONE,
                );
            }
            assert_eq!(
                app.tab().unwrap().editor.document.layers[0]
                    .pixels
                    .get_pixel(160, 64)[0],
                if aligned { 96 } else { 64 }
            );
        }
    }

    #[test]
    fn autosave_keeps_revision_dirty_and_clean_close_cancels_pending_backup() {
        let ctx = egui::Context::default();
        let mut app = PhotoEditor::from_context(&ctx);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("qa-output/auto-ui")
            .join(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
                    .to_string(),
            );
        app.recovery = Some(recovery::Store::new(root.clone()).unwrap());
        app.add_tab(files::Opened {
            source_stamp: None,
            document: Document::new(4, 3, false).unwrap(),
            project_path: None,
            title: "자동 저장".into(),
            notes: Vec::new(),
        });
        let first = app.tab().unwrap().editor.revision;
        app.auto_deadline = Instant::now();
        app.auto_save(&ctx);
        app.edit("다음 편집", |doc| doc.layers[0].offset = [2, 1]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.tab().unwrap().auto_work.is_some() && Instant::now() < deadline {
            app.auto_save(&ctx);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(app.tab().unwrap().auto_revision, Some(first));
        assert!(app.tab().unwrap().editor.dirty());
        let key = app.recovery.as_ref().unwrap().key(app.tab().unwrap().id);
        let snapshot = files::load_project(&root.join(&key).join("document.rphoto")).unwrap();
        assert_eq!(snapshot.layers[0].offset, [0, 0]);
        app.auto_deadline = Instant::now();
        app.auto_save(&ctx);
        app.tab_mut().unwrap().editor.saved_revision = Some(app.tab().unwrap().editor.revision);
        app.request_close(CloseTarget::App, &ctx);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !app.allow_exit && Instant::now() < deadline {
            app.auto_save(&ctx);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.allow_exit);
        assert!(!root.join(key).exists());
    }

    fn rapid_drag(tool: Tool) -> PhotoEditor {
        let ctx = egui::Context::default();
        let mut app = PhotoEditor::from_context(&ctx);
        app.add_tab(files::Opened {
            source_stamp: None,
            document: Document::new(256, 256, false).unwrap(),
            project_path: None,
            title: "Input regression".into(),
            notes: vec![],
        });
        app.tool = tool;
        app.brush_size = 8.0;
        app.tab_mut().unwrap().view.fit = false;
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::splat(512.0));
        let start = Pos2::new(192.0, 192.0);
        let corner = Pos2::new(256.0, 192.0);
        let end = Pos2::new(256.0, 256.0);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(start)],
                ..Default::default()
            },
            |ui| app.canvas_ui(ui, &ctx),
        );
        output.textures_delta.clear();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![
                    egui::Event::PointerButton {
                        pos: start,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    egui::Event::PointerMoved(corner),
                    egui::Event::PointerMoved(end),
                    egui::Event::PointerButton {
                        pos: end,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ui| app.canvas_ui(ui, &ctx),
        );
        output.textures_delta.clear();
        app
    }

    #[test]
    fn same_frame_press_move_release_paints_path_and_one_undo() {
        let mut app = rapid_drag(Tool::Brush);
        let color = app.color;
        let tab = app.tab_mut().unwrap();
        let pixels = &tab.editor.document.active_layer().unwrap().pixels;
        assert_eq!(pixels.get_pixel(96, 64).0, color);
        assert_eq!(pixels.get_pixel(128, 96).0, color);
        assert_eq!(pixels.get_pixel(96, 96).0, [255; 4]);
        assert_eq!(tab.editor.history().count(), 1);
        assert!(tab.gesture.is_none());
        tab.editor.undo();
        assert_eq!(
            tab.editor
                .document
                .active_layer()
                .unwrap()
                .pixels
                .get_pixel(96, 64)
                .0,
            [255; 4]
        );
    }

    #[test]
    fn same_frame_drag_keeps_selection_move_and_pan_origins() {
        let selected = rapid_drag(Tool::Select);
        let selection = selected.tab().unwrap().selection.as_ref().unwrap();
        assert_eq!(selection.min, [64, 64]);
        assert_eq!(selection.max, [128, 128]);
        let moved = rapid_drag(Tool::Move);
        assert_eq!(
            moved
                .tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .offset,
            [64, 64]
        );
        assert_eq!(moved.tab().unwrap().editor.history().count(), 1);
        let panned = rapid_drag(Tool::Hand);
        assert_eq!(panned.tab().unwrap().view.pan, Vec2::splat(64.0));
        assert!(!panned.tab().unwrap().editor.can_undo());
    }

    #[test]
    fn stale_background_preview_cannot_overwrite_newer_edit() {
        let ctx = egui::Context::default();
        let mut app = PhotoEditor::from_context(&ctx);
        app.add_tab(files::Opened {
            source_stamp: None,
            document: Document::new(8, 8, false).unwrap(),
            project_path: None,
            title: "미리보기".into(),
            notes: vec![],
        });
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Rendered::new(image::RgbaImage::from_pixel(
                8,
                8,
                image::Rgba([255; 4]),
            )))
            .unwrap();
        let tab = app.tab_mut().unwrap();
        tab.render_job = Some(receiver);
        editing::fill(
            tab.editor.document.active_layer_mut().unwrap(),
            None,
            [0, 0, 0, 255],
            false,
        );
        tab.render_dirty = true;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(300.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input.clone(), |ui| app.canvas_ui(ui, &ctx));
        output.textures_delta.clear();
        assert!(app.tab().unwrap().composite.is_none());
        let image = app
            .tab_mut()
            .unwrap()
            .render_job
            .take()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(image.pixels.get_pixel(0, 0).0, [0, 0, 0, 255]);
        let (sender, receiver) = mpsc::channel();
        sender.send(image).unwrap();
        app.tab_mut().unwrap().render_job = Some(receiver);
        let mut output = ctx.run_ui(input, |ui| app.canvas_ui(ui, &ctx));
        output.textures_delta.clear();
        assert_eq!(
            app.tab()
                .unwrap()
                .composite
                .as_ref()
                .unwrap()
                .get_pixel(0, 0)
                .0,
            [0, 0, 0, 255]
        );
    }
    #[test]
    fn canvas_text_typing_ime_commit_cancel_and_tool_switch_are_single_edits() {
        let (mut app, ctx, mut clock) = input_app(Tool::Text);
        input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        click_at(
            &mut app,
            &ctx,
            &mut clock,
            Pos2::new(168.0, 168.0),
            Modifiers::NONE,
        );
        assert!(app.text_session.is_some());
        input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Text("Hello ".into())],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "ㅎ".into(),
                active_range_chars: Some(0..1),
            })],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::Escape)],
            Modifiers::NONE,
        );
        assert!(
            app.text_session.is_some(),
            "IME 조합 중에는 문단 편집을 취소하지 않습니다"
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Ime(egui::ImeEvent::Commit("한글".into()))],
            Modifiers::NONE,
        );
        assert!(app.tool == Tool::Text);
        assert!(
            matches!(&app.text_session.as_ref().unwrap().content,Content::Text{text,..} if text=="Hello 한글"),
            "{:?}",
            app.text_session.as_ref().map(|s| &s.content)
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::Enter)],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Text("문단".into())],
            Modifiers::NONE,
        );
        let enter = egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        };
        input_frame(&mut app, &ctx, &mut clock, vec![enter], Modifiers::COMMAND);
        assert!(app.text_session.is_none());
        let editor = &app.tab().unwrap().editor;
        assert_eq!(editor.history().count(), 1);
        assert!(
            matches!(&editor.document.active_layer().unwrap().content,Some(Content::Text{text,..}) if text=="Hello 한글\n문단")
        );
        let id = editor.document.active;
        let layer = editor.document.active_layer().unwrap().clone();
        app.text_session = crate::text_canvas::Session::existing(app.tab().unwrap().id, &layer);
        input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::Text(" 취소".into())],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![key_event(Key::Escape)],
            Modifiers::NONE,
        );
        assert!(app.text_session.is_none());
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
        assert_eq!(app.tab().unwrap().editor.document.active, id);
        app.text_session = crate::text_canvas::Session::existing(app.tab().unwrap().id, &layer);
        crate::rich_text::replace_text(
            &mut app.text_session.as_mut().unwrap().content,
            "도구 전환 저장".into(),
        );
        app.set_tool(Tool::Move);
        assert!(app.text_session.is_none());
        assert_eq!(app.tab().unwrap().editor.history().count(), 2);
        app.tab_mut().unwrap().editor.undo();
        assert_eq!(
            app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .content,
            layer.content
        );
    }

    #[test]
    fn canvas_text_move_drag_changes_only_translation_and_commits_once() {
        let (mut app, ctx, mut clock) = input_app(Tool::Text);
        let mut session =
            crate::text_canvas::Session::new(app.tab().unwrap().id, [0.0, 0.0], [0, 0, 0, 255]);
        session.resize([100, 80]);
        crate::rich_text::replace_text(&mut session.content, "이동".into());
        app.text_session = Some(session);
        for _ in 0..5 {
            input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        }
        let header = ctx
            .read_response(egui::Id::new("text-box-move"))
            .unwrap()
            .rect
            .center();
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerMoved(header),
                egui::Event::PointerButton {
                    pos: header,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(header + Vec2::new(25.0, 16.0))],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerButton {
                pos: header + Vec2::new(25.0, 16.0),
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
        let session = app.text_session.as_ref().unwrap();
        assert_eq!(session.size(), [100, 80]);
        assert_eq!(session.transform.translation, [25.0, 16.0]);
        assert!(!app.tab().unwrap().editor.can_undo());
        app.finish_text(true);
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
    }
    #[test]
    fn canvas_text_resize_handle_responds_to_pointer_drag() {
        let (mut app, ctx, mut clock) = input_app(Tool::Text);
        let mut session =
            crate::text_canvas::Session::new(app.tab().unwrap().id, [0.0, 0.0], [255; 4]);
        session.resize([100, 80]);
        crate::rich_text::replace_text(&mut session.content, "상자 크기".into());
        app.text_session = Some(session);
        input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        for _ in 0..3 {
            input_frame(&mut app, &ctx, &mut clock, vec![], Modifiers::NONE);
        }
        let handle = ctx
            .read_response(egui::Id::new("text-box-resize"))
            .unwrap()
            .rect
            .center();
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![
                egui::Event::PointerMoved(handle),
                egui::Event::PointerButton {
                    pos: handle,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerMoved(handle + Vec2::new(35.0, 25.0))],
            Modifiers::NONE,
        );
        input_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![egui::Event::PointerButton {
                pos: handle + Vec2::new(35.0, 25.0),
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
        let size = app.text_session.as_ref().unwrap().size();
        assert!(size[0] > 100 && size[1] > 80, "{size:?}");
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        app.finish_text(true);
        assert_eq!(app.tab().unwrap().editor.history().count(), 1);
    }
}

impl Tool {
    pub(crate) fn group(self) -> usize {
        match self {
            Self::Move => 0,
            Self::Select | Self::Ellipse | Self::Lasso | Self::Wand => 1,
            Self::Crop => 2,
            Self::Brush | Self::Eraser => 3,
            Self::Stamp | Self::Heal | Self::Blur => 4,
            Self::Gradient | Self::Fill => 5,
            Self::Shape | Self::Pen => 6,
            Self::Text => 7,
            Self::Eyedropper => 8,
            Self::Hand | Self::Zoom => 9,
        }
    }
    pub(crate) fn entries() -> [(Self, Icon, &'static str); 19] {
        [
            (Self::Move, Icon::Move, "V"),
            (Self::Select, Icon::Selection, "M"),
            (Self::Ellipse, Icon::Ellipse, ""),
            (Self::Lasso, Icon::Lasso, "L"),
            (Self::Wand, Icon::Wand, "W"),
            (Self::Crop, Icon::Crop, "C"),
            (Self::Brush, Icon::Brush, "B"),
            (Self::Eraser, Icon::Eraser, "E"),
            (Self::Stamp, Icon::Stamp, "S"),
            (Self::Heal, Icon::Heal, "J"),
            (Self::Blur, Icon::Blur, ""),
            (Self::Gradient, Icon::Gradient, "G"),
            (Self::Fill, Icon::Fill, ""),
            (Self::Shape, Icon::Shapes, "U"),
            (Self::Pen, Icon::Pen, "P"),
            (Self::Text, Icon::Text, "T"),
            (Self::Eyedropper, Icon::Eyedropper, "I"),
            (Self::Hand, Icon::Hand, "H"),
            (Self::Zoom, Icon::Zoom, "Z"),
        ]
    }
    fn group_tool(group: usize, active: Self, remembered: &[Self]) -> Self {
        if active.group() == group {
            return active;
        }
        remembered
            .iter()
            .copied()
            .find(|t| t.group() == group)
            .unwrap_or_else(|| {
                Self::entries()
                    .iter()
                    .find(|(t, _, _)| t.group() == group)
                    .unwrap()
                    .0
            })
    }

    fn usage(self) -> &'static str {
        match self {
            Self::Move => "드래그 이동 · Shift 축 고정 · Alt 복제",
            Self::Brush | Self::Eraser => "드래그 그리기 · Shift 이전 획과 직선 연결",
            Self::Select | Self::Ellipse | Self::Lasso | Self::Wand => {
                "Shift 추가 · Alt 빼기 · 선택 메뉴에서 경계/픽셀 조절"
            }
            Self::Crop => "드래그 영역 지정 · 경계 핸들 조절 · Enter 적용 · Esc 취소",
            Self::Stamp => "Alt 클릭으로 복제 원본 지정 · 드래그 복제",
            Self::Heal | Self::Blur => "드래그로 현재 브러시 영역 보정",
            Self::Gradient => "드래그 방향 지정 · 속성에서 색상점·보간·반전",
            Self::Fill => "클릭하여 색상/연결 범위 채우기",
            Self::Shape => "드래그 도형 생성 · 속성에서 채우기·선 편집",
            Self::Pen => "클릭 점 추가 · 드래그 곡선 · Enter 완료 · 점 편집에서 핸들 조절",
            Self::Text => "클릭 문단 편집/생성 · Ctrl+Enter 적용 · Esc 취소",
            Self::Eyedropper => "클릭 색상 추출 · Alt로 반대 색상 대상",
            Self::Hand => "드래그 화면 이동 · Space 임시 손 도구",
            Self::Zoom => "클릭 확대 · Alt 클릭 축소 · 휠 커서 중심 확대",
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Move => "이동",
            Self::Brush => "브러시",
            Self::Eraser => "지우개",
            Self::Select => "사각형 선택",
            Self::Eyedropper => "스포이드",
            Self::Hand => "손",
            Self::Zoom => "확대·축소",
            Self::Ellipse => "타원 선택",
            Self::Lasso => "올가미",
            Self::Wand => "마술봉",
            Self::Crop => "자르기",
            Self::Fill => "채우기",
            Self::Stamp => "복제 도장",
            Self::Heal => "스팟 복구",
            Self::Blur => "블러",
            Self::Shape => "도형",
            Self::Gradient => "그라디언트",
            Self::Text => "텍스트",
            Self::Pen => "펜·경로",
        }
    }
}

fn luminance(pixel: [u8; 4]) -> usize {
    ((u32::from(pixel[0]) * 54 + u32::from(pixel[1]) * 183 + u32::from(pixel[2]) * 19) / 256)
        as usize
}

struct Rendered {
    pixels: image::RgbaImage,
    color: egui::ColorImage,
    histogram: [u32; 256],
}

impl Rendered {
    fn new(pixels: image::RgbaImage) -> Self {
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [pixels.width() as usize, pixels.height() as usize],
            pixels.as_raw(),
        );
        let mut histogram = [0; 256];
        for p in pixels.pixels() {
            histogram[luminance(p.0)] += 1;
        }
        Self {
            pixels,
            color,
            histogram,
        }
    }
}

struct Tab {
    file_watch: crate::file_watch::Watch,
    mask_overlay: crate::mask_overlay::Overlay,
    id: u64,
    title: String,
    path: Option<PathBuf>,
    editor: Editor,
    view: View,
    canvas_area: Option<Rect>,
    texture: Option<egui::TextureHandle>,
    display_sampling: crate::display_sampling::Mode,
    composite: Option<image::RgbaImage>,
    render_dirty: bool,
    render_job: Option<mpsc::Receiver<Rendered>>,
    paint_damage: Option<PixelRegion>,
    histogram: Option<[u32; 256]>,
    selection: Option<Selection>,
    gesture: Option<Gesture>,
    notes: Vec<String>,
    clone_origin: Option<[f32; 2]>,
    clone_delta: Option<[f32; 2]>,
    last_paint: Option<(u64, bool, [f32; 2])>,
    edit_mask: bool,
    auto_revision: Option<u64>,
    auto_work: Option<AutoWork>,
    recovered_from: Option<String>,
    selected_layers: Vec<u64>,
    collapsed: HashSet<u64>,
    thumbnails: HashMap<u64, (u64, usize, egui::TextureHandle)>,
    snap_lines: [Option<f32>; 2],
    pen: crate::paths::Pen,
    transform_frame: Option<crate::transform_batch::Frame>,
    crop_before: Option<Option<Selection>>,
}

impl Tab {
    fn selected_ids(&self) -> Vec<u64> {
        if self.selected_layers.contains(&self.editor.document.active) {
            self.editor.document.selected_roots(&self.selected_layers)
        } else {
            vec![self.editor.document.active]
        }
    }
    fn thumbnail(&mut self, layer: &document::Layer, ctx: &egui::Context) -> egui::TextureId {
        let revision = self.editor.revision;
        let pointer = Arc::as_ptr(&layer.pixels) as usize;
        if self
            .thumbnails
            .get(&layer.id)
            .is_none_or(|(r, p, _)| *r != revision || *p != pointer)
        {
            // Sample at most 28x28 pixels; box downsampling scanned the entire layer.
            let source = &layer.pixels;
            let scale = 28.0 / source.width().max(source.height()) as f32;
            let width = (source.width() as f32 * scale).round().clamp(1.0, 28.0) as u32;
            let height = (source.height() as f32 * scale).round().clamp(1.0, 28.0) as u32;
            let pixels = image::RgbaImage::from_fn(width, height, |x, y| {
                let sx = ((x as f64 + 0.5) * source.width() as f64 / width as f64) as u32;
                let sy = ((y as f64 + 0.5) * source.height() as f64 / height as f64) as u32;
                *source.get_pixel(sx.min(source.width() - 1), sy.min(source.height() - 1))
            });
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [pixels.width() as usize, pixels.height() as usize],
                pixels.as_raw(),
            );
            let texture = ctx.load_texture(
                format!("thumb-{}-{}", self.id, layer.id),
                image,
                egui::TextureOptions::LINEAR,
            );
            self.thumbnails
                .insert(layer.id, (revision, pointer, texture));
        }
        self.thumbnails[&layer.id].2.id()
    }

    fn damage_stroke(&mut self, from: [f32; 2], to: [f32; 2], radius: f32) {
        let doc = &self.editor.document;
        let layer = doc.active_layer().unwrap();
        let color_scale = layer.transform.scale[0]
            .abs()
            .max(layer.transform.scale[1].abs());
        let mask_geometry = self
            .edit_mask
            .then(|| crate::masks::geometry(layer))
            .flatten();
        let mask_scale =
            mask_geometry.map_or(0., |(t, _, _)| t.scale[0].abs().max(t.scale[1].abs()));
        let pad = radius.max(0.5) + 2. * (color_scale + mask_scale);
        let size = [doc.width, doc.height];
        let mut region = PixelRegion {
            min: std::array::from_fn(|i| {
                (from[i].min(to[i]) - pad)
                    .floor()
                    .clamp(0.0, size[i] as f32) as u32
            }),
            max: std::array::from_fn(|i| {
                (from[i].max(to[i]) + pad).ceil().clamp(0.0, size[i] as f32) as u32
            }),
        };
        if self.edit_mask
            && ((layer.mask_placement.is_some()
                && mask_geometry.is_some_and(|(t, _, _)| t.warp.is_some()))
                || (layer.mask_placement.is_none()
                    && layer.transform != Default::default()
                    && (layer.group
                        || layer
                            .mask
                            .as_ref()
                            .is_some_and(|m| m.dimensions() != layer.pixels.dimensions()))))
        {
            region = PixelRegion {
                min: [0; 2],
                max: size,
            };
        }
        if region.size().contains(&0) {
            return;
        }
        self.paint_damage = Some(self.paint_damage.map_or(region, |old| old.union(region)));
    }

    fn apply_display_sampling(&mut self, mode: crate::display_sampling::Mode) {
        if self.display_sampling == mode {
            return;
        }
        self.display_sampling = mode;
        if let Some(texture) = &mut self.texture
            && let Some(composite) = &self.composite
        {
            // A single unchanged texel updates sampler options without re-uploading
            // the full image. glow also rebuilds mip levels for this delta.
            let pixel = composite.get_pixel(0, 0);
            texture.set_partial(
                [0, 0],
                egui::ColorImage::from_rgba_unmultiplied([1, 1], &pixel.0),
                mode.options(),
            );
        }
    }
    fn update_painted_region(&mut self) {
        if self.render_dirty
            || self.render_job.is_some()
            || self.composite.is_none()
            || self.texture.is_none()
            || !self.editor.document.local_preview()
        {
            return;
        }
        let Some(region) = self.paint_damage.take() else {
            return;
        };
        let patch = self.editor.document.composite_region(region);
        let composite = self.composite.as_mut().unwrap();
        for (x, y, p) in patch.enumerate_pixels() {
            let target = composite.get_pixel_mut(x + region.min[0], y + region.min[1]);
            if let Some(counts) = &mut self.histogram {
                counts[luminance(target.0)] -= 1;
                counts[luminance(p.0)] += 1;
            }
            *target = *p;
        }
        self.texture.as_mut().unwrap().set_partial(
            region.min.map(|v| v as usize),
            egui::ColorImage::from_rgba_unmultiplied(
                region.size().map(|v| v as usize),
                patch.as_raw(),
            ),
            self.display_sampling.options(),
        );
    }
}

#[derive(Clone)]
struct LayerDrag {
    tab: u64,
    ids: Vec<u64>,
}

struct AutoWork {
    revision: u64,
    alive: Arc<AtomicBool>,
}
struct AutoResult {
    tab: u64,
    revision: u64,
    result: Result<(), String>,
}

enum Gesture {
    Paint {
        last: [f32; 2],
    },
    Move {
        start: [f32; 2],
        offsets: Vec<(u64, [i32; 2])>,
        bounds: Option<([f32; 2], [f32; 2])>,
    },
    Boundary {
        start: [f32; 2],
        previous: Selection,
    },
    Pixels {
        start: [f32; 2],
        previous: Selection,
        source: Box<editing::PixelMove>,
        last_delta: [i32; 2],
    },
    Transform {
        initial: crate::transform::Transform,
        offset: [i32; 2],
        size: [u32; 2],
        handle: crate::transform::Handle,
        start: [f32; 2],
        batch: Box<crate::transform_batch::Frame>,
    },
    CropBox {
        initial: Selection,
        handle: crate::transform::Handle,
        start: [f32; 2],
    },
    Guide {
        index: usize,
    },
    Select {
        start: [f32; 2],
        previous: Option<Selection>,
        add: bool,
        subtract: bool,
    },
    Lasso {
        points: Vec<[f32; 2]>,
        previous: Option<Selection>,
        add: bool,
        subtract: bool,
    },
    Polygon {
        points: Vec<[f32; 2]>,
        previous: Option<Selection>,
        add: bool,
        subtract: bool,
    },
    Retouch {
        last: [f32; 2],
        source: std::sync::Arc<image::Rgba32FImage>,
        delta: [f32; 2],
        mode: editing::Retouch,
    },
    Vector {
        start: [f32; 2],
        end: [f32; 2],
    },
    Pan {
        last: Pos2,
    },
}

enum SaveOutcome {
    Saved(PathBuf, crate::file_watch::Snapshot),
    Conflict(PathBuf, crate::file_watch::Snapshot),
}
struct ExternalDialog {
    tab: u64,
    path: PathBuf,
    snapshot: crate::file_watch::Snapshot,
    saving: bool,
}
enum JobResult {
    Recover {
        result: Result<Document, String>,
        entry: recovery::Entry,
    },
    Open {
        result: Result<Option<files::Opened>, String>,
        import_into: Option<u64>,
    },
    Save {
        result: Result<Option<SaveOutcome>, String>,
        tab: u64,
        revision: u64,
    },
    Export(Result<Option<PathBuf>, String>),
}

#[derive(Clone, Copy)]
enum CloseTarget {
    Tab(u64),
    App,
}

pub struct PhotoEditor {
    tabs: Vec<Tab>,
    active: usize,
    next_tab: u64,
    tool: Tool,
    move_target: MoveTarget,
    transforming: bool,
    command_panel: crate::commands::Panel,
    command_options: Option<crate::commands::Command>,
    layout_options: Option<(u64, crate::guides::Layout)>,
    canvas_only: bool,
    canvas_was_fullscreen: bool,
    brush_size: f32,
    brush_opacity: f32,
    brush_hardness: f32,
    brush_smoothing: f32,
    clone_aligned: bool,
    polygon_lasso: bool,
    crop_ratio: Option<f32>,
    color: [u8; 4],
    sample_merged: bool,
    sample_size: u32,
    sample_background: bool,
    colors_panel: crate::colors::Panel,
    gradient_interpolation: crate::vector::GradientInterpolation,
    gradient_reversed: bool,
    new_dialog: bool,
    new_width: u32,
    new_height: u32,
    new_transparent: bool,
    new_depth: u8,
    export_dialog: bool,
    export_format: usize,
    export_options: files::ExportOptions,
    export_preview: crate::export_preview::Panel,
    rename: Option<crate::layer_name::Session>,
    rename_pointer_blocked: bool,
    message: Option<(String, String)>,
    external_dialog: Option<ExternalDialog>,
    close_target: Option<CloseTarget>,
    allow_exit: bool,
    jobs: mpsc::Receiver<JobResult>,
    sender: mpsc::Sender<JobResult>,
    busy: Option<&'static str>,
    status: String,
    background: [u8; 4],
    shape_kind: ShapeKind,
    pen_fill: bool,
    shape_stroke: f32,
    tolerance: u8,
    contiguous: bool,
    clipboard: Option<image::DynamicImage>,
    layer_clipboard: Option<Document>,
    size_dialog: Option<bool>,
    resize_width: u32,
    resize_height: u32,
    raw_dialog: Option<crate::raw_panel::Panel>,
    adjust_dialog: Option<(u64, u64)>,
    adjust_selection_snapshot: Option<Selection>,
    adjust_saved_scope: Option<crate::adjustment_scope::Scope>,
    content_dialog: Option<(u64, Option<u64>, [i32; 2], Content)>,
    text_session: Option<crate::text_canvas::Session>,
    text_selection: std::ops::Range<usize>,
    text_style: Option<crate::rich_text::Style>,
    svg_panel: crate::svg_edit::Panel,
    pattern_panel: crate::pattern::Panel,
    fill_panel: crate::fill::Panel,
    mask_view: crate::mask_view::Panel,
    fill_mask: Option<Selection>,
    fill_restrict: bool,
    show_grid: bool,
    show_histogram: bool,
    workspace: crate::workspace::Store,
    shortcuts_panel: crate::shortcuts::Panel,
    about_panel: crate::about::Panel,
    psd_dialog: Option<bool>,
    psd_options: crate::psd::OutputOptions,
    selection_feather: f32,
    selection_expand: i32,
    curve_channel: usize,
    curve_drag: Option<usize>,
    mask_feather: f32,
    recovery: Option<recovery::Store>,
    recovery_entries: Vec<recovery::Entry>,
    recovery_warnings: Vec<String>,
    recovery_dialog: bool,
    auto_sender: mpsc::Sender<AutoResult>,
    auto_jobs: mpsc::Receiver<AutoResult>,
    auto_interval: u64,
    auto_deadline: Instant,
}

fn gradient_interpolation_ui(ui: &mut egui::Ui, mode: &mut crate::vector::GradientInterpolation) {
    use crate::vector::GradientInterpolation;
    egui::ComboBox::from_id_salt("gradient-interpolation")
        .selected_text(match mode {
            GradientInterpolation::Srgb => "기존 색상 보간",
            GradientInterpolation::LinearRgb => "선형 광량 보간",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(mode, GradientInterpolation::Srgb, "기존 색상 보간");
            ui.selectable_value(mode, GradientInterpolation::LinearRgb, "선형 광량 보간");
        });
}

impl PhotoEditor {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::from_context(&cc.egui_ctx);
        if let Some(root) = recovery::data_directory() {
            crate::lens_profile::initialize_library(&cc.egui_ctx, root.join("lens-library.json"));
            match crate::workspace::Store::load(root.join("workspace.json")) {
                Ok(store) => {
                    app.mask_view.open = store.layout.mask_viewer;
                    app.show_grid = store.layout.pixel_grid;
                    app.show_histogram = store.layout.histogram;
                    app.workspace = store;
                }
                Err(error) => app.status = format!("작업 화면 설정 읽기 실패: {error}"),
            }
            match crate::colors::Panel::load(root.join("colors.json")) {
                Ok(panel) => {
                    app.color = panel.library.foreground;
                    app.background = panel.library.background;
                    app.colors_panel = panel;
                }
                Err(error) => app.status = format!("색상 설정 읽기 실패: {error}"),
            }
            match recovery::Store::new(root.join("recovery")) {
                Ok(store) => {
                    match store.list() {
                        Ok(listing) => {
                            app.recovery_dialog =
                                !listing.entries.is_empty() || !listing.warnings.is_empty();
                            app.recovery_entries = listing.entries;
                            app.recovery_warnings = listing.warnings;
                        }
                        Err(error) => app.message = Some(("복구 목록 읽기 실패".into(), error)),
                    }
                    app.recovery = Some(store);
                }
                Err(error) => {
                    app.message = Some(("자동 저장을 준비하지 못했습니다.".into(), error))
                }
            }
        }
        if let Some(path) = std::env::args_os().nth(1) {
            app.open_file(Some(PathBuf::from(path)), None, &cc.egui_ctx);
        }
        app
    }

    fn from_context(ctx: &egui::Context) -> Self {
        ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.visuals.panel_fill = Color32::from_rgb(33, 36, 42);
            style.visuals.window_fill = Color32::from_rgb(37, 41, 48);
            style.visuals.selection.bg_fill = Color32::from_rgb(49, 90, 149);
            style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        });
        let (sender, jobs) = mpsc::channel();
        let (auto_sender, auto_jobs) = mpsc::channel();
        Self {
            tabs: vec![],
            active: 0,
            next_tab: 1,
            tool: Tool::Move,
            move_target: MoveTarget::Layers,
            transforming: false,
            command_panel: Default::default(),
            command_options: None,
            layout_options: None,
            canvas_only: false,
            canvas_was_fullscreen: false,
            brush_size: 32.0,
            brush_opacity: 1.0,
            brush_hardness: 0.8,
            brush_smoothing: 0.0,
            clone_aligned: true,
            polygon_lasso: false,
            crop_ratio: None,
            color: [90, 153, 245, 255],
            sample_merged: true,
            sample_size: 1,
            sample_background: false,
            colors_panel: Default::default(),
            gradient_interpolation: Default::default(),
            gradient_reversed: false,
            new_dialog: false,
            new_width: 1600,
            new_height: 1000,
            new_transparent: false,
            new_depth: 8,
            export_dialog: false,
            export_format: 0,
            export_options: Default::default(),
            export_preview: Default::default(),
            rename: None,
            rename_pointer_blocked: false,
            message: None,
            external_dialog: None,
            close_target: None,
            allow_exit: false,
            jobs,
            sender,
            busy: None,
            status: "사진을 열거나 새 문서를 만드세요.".into(),
            background: [255; 4],
            shape_kind: ShapeKind::Rectangle,
            pen_fill: false,
            shape_stroke: 2.0,
            tolerance: 32,
            contiguous: true,
            clipboard: None,
            layer_clipboard: None,
            size_dialog: None,
            resize_width: 1600,
            resize_height: 1000,
            raw_dialog: None,
            adjust_dialog: None,
            adjust_selection_snapshot: None,
            adjust_saved_scope: None,
            content_dialog: None,
            text_session: None,
            text_selection: 0..0,
            text_style: None,
            svg_panel: Default::default(),
            pattern_panel: Default::default(),
            fill_panel: Default::default(),
            mask_view: Default::default(),
            fill_mask: None,
            fill_restrict: true,
            show_grid: false,
            show_histogram: false,
            workspace: Default::default(),
            shortcuts_panel: Default::default(),
            about_panel: Default::default(),
            psd_dialog: None,
            psd_options: Default::default(),
            selection_feather: 2.0,
            selection_expand: 2,
            curve_channel: 0,
            curve_drag: None,
            mask_feather: 2.0,
            recovery: None,
            recovery_entries: Vec::new(),
            recovery_warnings: Vec::new(),
            recovery_dialog: false,
            auto_sender,
            auto_jobs,
            auto_interval: 30,
            auto_deadline: Instant::now() + Duration::from_secs(30),
        }
    }

    fn tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }
    fn tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    fn finish_text(&mut self, apply: bool) {
        if let Some(mut session) = self.text_session.take()
            && apply
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == session.tab)
        {
            match session.apply(&mut tab.editor) {
                Ok(changed) => tab.render_dirty |= changed,
                Err(error) => {
                    self.status = error.clone();
                    session.last_error = Some(error);
                    self.text_session = Some(session);
                }
            }
        }
    }
    fn finish_gesture(&mut self) {
        if self.layout_options.is_some() {
            self.close_command_options(false);
        }
        self.finish_text(true);
        self.transforming = false;
        if let Some(tab) = self.tab_mut() {
            tab.transform_frame = None;
            tab.snap_lines = [None; 2];
        }
        let fill = if self.pen_fill {
            self.background
        } else {
            [0; 4]
        };
        let stroke = self.color;
        let thickness = self.shape_stroke;
        if let Some(tab) = self.tab_mut() {
            if tab.pen.draft.len() >= 2 {
                match tab.pen.apply(
                    &mut tab.editor,
                    crate::paths::Action::Finish,
                    1.0,
                    fill,
                    stroke,
                    thickness,
                ) {
                    Ok(changed) => tab.render_dirty |= changed,
                    Err(error) => tab.notes.push(error),
                }
            }
            tab.editor.finish();
            tab.pen.reset();
            if let Some(Gesture::Polygon { previous, .. }) = tab.gesture.take() {
                tab.selection = previous;
            }
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        if self.tool == Tool::Crop && tool != Tool::Crop {
            self.cancel_crop();
        }
        if tool == Tool::Crop
            && self.tool != Tool::Crop
            && let Some(tab) = self.tab_mut()
        {
            tab.crop_before = Some(tab.selection.clone());
        }
        self.tool = tool;
        self.workspace
            .layout
            .remembered
            .retain(|t| t.group() != tool.group());
        self.workspace.layout.remembered.push(tool);
    }

    fn start_transform(&mut self) {
        self.finish_gesture();
        if self.busy.is_some() || self.text_session.is_some() {
            return;
        }
        if let Some(tab) = self.tab_mut() {
            match if tab.edit_mask {
                crate::transform_batch::Frame::new_mask(
                    &tab.editor.document,
                    tab.editor.document.active,
                )
            } else {
                crate::transform_batch::Frame::new(&tab.editor.document, tab.selected_ids())
            } {
                Ok(frame) => {
                    tab.transform_frame = Some(frame);
                    self.transforming = true;
                }
                Err(error) => self.message = Some(("레이어 변형".into(), error)),
            }
        }
    }

    fn end_transform(&mut self, apply: bool) {
        self.transforming = false;
        if let Some(tab) = self.tab_mut() {
            tab.gesture = None;
            tab.transform_frame = None;
            tab.snap_lines = [None; 2];
            if apply {
                tab.editor.finish();
            } else {
                tab.editor.cancel();
            }
            tab.render_dirty = true;
        }
    }

    fn edit(&mut self, label: &str, apply: impl FnOnce(&mut Document)) {
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        if let Some(tab) = self.tab_mut() {
            let active = tab.editor.document.active;
            tab.editor.edit(label, apply);
            if tab.editor.document.active != active {
                tab.edit_mask = false;
            }
            tab.render_dirty = true;
        }
    }

    fn apply_result(
        &mut self,
        label: &str,
        apply: impl FnOnce(&mut Document) -> Result<(), String>,
    ) {
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        if let Some(tab) = self.tab_mut() {
            let active = tab.editor.document.active;
            tab.editor.begin(label);
            match apply(&mut tab.editor.document) {
                Ok(()) => {
                    if tab.editor.document.active != active {
                        tab.edit_mask = false;
                    }
                    tab.editor.finish();
                    tab.render_dirty = true;
                }
                Err(error) => {
                    tab.editor.cancel();
                    self.message = Some(("편집 실패".into(), error));
                }
            }
        }
    }
    fn copy(&mut self, cut: bool, merged: bool) {
        if !merged
            && self.tab().is_some_and(|t| {
                t.editor
                    .document
                    .active_layer()
                    .is_some_and(|l| l.adjustment_layer)
            })
        {
            self.status="조정 레이어는 픽셀 원본이 없습니다. 선택 레이어 복사 또는 병합하여 복사를 사용하세요.".into();
            return;
        }
        if let Some(tab) = self.tab() {
            self.clipboard = Some(editing::copy_pixels(
                &tab.editor.document,
                tab.selection.as_ref(),
                merged,
            ));
            if cut {
                if self
                    .tab()
                    .is_some_and(|t| t.editor.document.is_locked(t.editor.document.active))
                {
                    return;
                }
                let selection = self.tab().unwrap().selection.clone();
                self.edit("잘라내기", |doc| {
                    editing::fill(
                        doc.active_layer_mut().unwrap(),
                        selection.as_ref(),
                        [0; 4],
                        true,
                    )
                });
            }
        }
    }
    fn paste(&mut self) {
        if let Some(pixels) = self.clipboard.clone() {
            self.apply_result("붙여넣기", |doc| {
                doc.add_dynamic("붙여넣은 이미지".into(), pixels)
            });
        }
    }
    fn invert_selection(&mut self) {
        if let Some(tab) = self.tab_mut() {
            if let Some(selection) = &mut tab.selection {
                selection.inverted = !selection.inverted;
            } else {
                tab.selection = Some(Selection::rectangle(
                    [0, 0],
                    [
                        tab.editor.document.width as i32,
                        tab.editor.document.height as i32,
                    ],
                ));
            }
        }
    }
    fn crop(&mut self) {
        if let Some(selection) = self.tab().and_then(|t| t.selection.clone()) {
            self.apply_result("자르기", |doc| editing::crop(doc, &selection));
            if let Some(tab) = self.tab_mut() {
                tab.selection = None;
                tab.crop_before = Some(None);
                tab.view.fit = true;
            }
        }
    }
    fn cancel_crop(&mut self) {
        if let Some(tab) = self.tab_mut() {
            tab.gesture = None;
            if let Some(before) = tab.crop_before.take() {
                tab.selection = before;
            }
            tab.snap_lines = [None; 2];
        }
    }
    fn open_adjustments(&mut self) {
        self.edit_adjustments(true);
    }
    fn new_adjustment_layer(&mut self) {
        self.finish_gesture();
        if self.busy.is_some() || self.text_session.is_some() {
            return;
        }
        if let Some(tab) = self.tab_mut() {
            tab.editor.begin("조정 레이어");
            if let Err(error) = crate::adjustment_layer::add(&mut tab.editor.document) {
                tab.editor.cancel();
                self.status = error;
                return;
            }
            tab.edit_mask = false;
            tab.render_dirty = true;
        }
        self.edit_adjustments(false);
        if self.adjust_dialog.is_none()
            && let Some(tab) = self.tab_mut()
        {
            tab.editor.cancel();
            tab.render_dirty = true;
        }
    }
    fn edit_adjustments(&mut self, finish: bool) {
        if finish {
            self.finish_gesture();
        }
        if self.text_session.is_some() || self.busy.is_some() {
            return;
        }
        let Some(tab) = self.tab() else {
            return;
        };
        let Some(layer) = tab.editor.document.active_layer() else {
            return;
        };
        if tab.editor.document.is_locked(layer.id) {
            self.status = "잠금 해제한 레이어에 보정을 적용하세요.".into();
            return;
        }
        let saved_scope = layer.adjustments.scope.clone();
        let selection = tab.selection.clone();
        let initial = if saved_scope.is_none() {
            selection
                .as_ref()
                .map(|s| {
                    crate::adjustment_scope::Scope::capture(
                        layer,
                        s,
                        [tab.editor.document.width, tab.editor.document.height],
                    )
                })
                .transpose()
        } else {
            Ok(saved_scope.clone())
        };
        let scope = match initial {
            Ok(scope) => scope,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.adjust_selection_snapshot = selection;
        self.adjust_saved_scope = saved_scope.or_else(|| scope.clone());
        self.curve_drag = None;
        if let Some(tab) = self.tab_mut() {
            tab.editor.begin("사진 보정");
            tab.editor
                .document
                .active_layer_mut()
                .unwrap()
                .adjustments
                .scope = scope;
            tab.render_dirty = true;
            self.adjust_dialog = Some((tab.id, tab.editor.document.active));
        }
    }
    fn open_raw(&mut self, ctx: &egui::Context) {
        self.finish_gesture();
        if let Some(tab) = self.tab()
            && let Some(layer) = tab.editor.document.active_layer()
            && layer.raw_source.is_some()
        {
            self.raw_dialog =
                crate::raw_panel::Panel::for_layer(tab.id, tab.editor.revision, layer, ctx);
        }
    }
    fn raw_ui(&mut self, ctx: &egui::Context) {
        let Some(mut panel) = self.raw_dialog.take() else {
            return;
        };
        let valid = self.tab().is_some_and(|t| {
            t.id == panel.tab
                && t.editor.revision == panel.revision
                && t.editor.document.active == panel.layer
                && !t.editor.document.is_locked(panel.layer)
                && t.editor
                    .document
                    .active_layer()
                    .and_then(|l| l.raw_source.as_ref())
                    .is_some_and(|s| Arc::ptr_eq(s, &panel.source))
        });
        if !valid {
            self.status = "RAW 현상 대상이 바뀌어 닫았습니다.".into();
            return;
        }
        match panel.ui(ctx) {
            Some(crate::raw_panel::Action::Apply(pixels, settings, adjustments)) => {
                if let Some(tab) = self.tab_mut() {
                    let mut updated = tab.editor.document.active_layer().unwrap().clone();
                    match crate::raw_input::apply(&mut updated, pixels, settings, &panel.source) {
                        Ok(()) => {
                            updated.adjustments = *adjustments;
                            tab.editor
                                .edit("RAW 현상", |d| *d.active_layer_mut().unwrap() = updated);
                            tab.render_dirty = true;
                        }
                        Err(error) => {
                            self.status = error;
                            self.raw_dialog = Some(panel);
                        }
                    }
                }
            }
            Some(crate::raw_panel::Action::Cancel) => {}
            None => self.raw_dialog = Some(panel),
        }
    }
    fn open_resize(&mut self, resample: bool) {
        if let Some(tab) = self.tab() {
            self.resize_width = tab.editor.document.width;
            self.resize_height = self.tab().unwrap().editor.document.height;
            self.size_dialog = Some(resample);
        }
    }
    fn merge_layers(&mut self) {
        self.apply_result("보이는 레이어 병합", editing::merge_visible);
    }
    fn open_fill(&mut self, gradient: bool) {
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        let Some(tab) = self.tab() else {
            return;
        };
        let id = tab.id;
        let width = tab.editor.document.width;
        let height = tab.editor.document.height;
        self.fill_mask = tab.selection.clone();
        self.fill_restrict = true;
        self.fill_panel = Default::default();
        let content = if gradient {
            Content::Gradient {
                width,
                height,
                start: [0., 0.],
                end: [width as f32, 0.],
                colors: [self.color, self.background],
                stops: vec![],
                interpolation: Default::default(),
            }
        } else {
            Content::Solid {
                width,
                height,
                color: self.color,
            }
        };
        self.content_dialog = Some((id, None, [0, 0], content));
    }
    fn open_pattern(&mut self) {
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        let Some(tab) = self.tab() else {
            return;
        };
        let tab_id = tab.id;
        let size = [tab.editor.document.width, tab.editor.document.height];
        self.fill_mask = tab.selection.clone();
        self.fill_restrict = true;
        self.pattern_panel = Default::default();
        match crate::pattern::Pattern::from_tile(
            &crate::pattern::preset(0, [self.color, self.background]),
            size,
        )
        .and_then(|p| p.content())
        {
            Ok(content) => self.content_dialog = Some((tab_id, None, [0, 0], content)),
            Err(error) => self.message = Some(("패턴 만들기 실패".into(), error)),
        }
    }
    fn add_content(&mut self, content: Content, offset: [i32; 2]) {
        match content.render() {
            Ok(pixels) => self.apply_result("벡터·텍스트 레이어", |doc| {
                doc.add_layer("새 요소".into(), pixels)?;
                let layer = doc.active_layer_mut().unwrap();
                layer.offset = offset;
                layer.content = Some(content);
                Ok(())
            }),
            Err(error) => self.message = Some(("요소 만들기 실패".into(), error)),
        }
    }

    fn add_tab(&mut self, opened: files::Opened) {
        self.finish_gesture();
        let saved = opened.project_path.is_some();
        self.tabs.push(Tab {
            file_watch: crate::file_watch::Watch::new(opened.source_stamp),
            mask_overlay: Default::default(),
            id: self.next_tab,
            title: opened.title,
            path: opened.project_path,
            editor: Editor::new(opened.document, saved),
            view: View::default(),
            canvas_area: None,
            texture: None,
            display_sampling: Default::default(),
            composite: None,
            render_dirty: true,
            render_job: None,
            paint_damage: None,
            histogram: None,
            selection: None,
            gesture: None,
            notes: opened.notes.clone(),
            clone_origin: None,
            clone_delta: None,
            last_paint: None,
            edit_mask: false,
            auto_revision: None,
            auto_work: None,
            recovered_from: None,
            selected_layers: Vec::new(),
            collapsed: HashSet::new(),
            thumbnails: HashMap::new(),
            snap_lines: [None; 2],
            pen: crate::paths::Pen::default(),
            transform_frame: None,
            crop_before: None,
        });
        self.next_tab += 1;
        self.active = self.tabs.len() - 1;
        if !opened.notes.is_empty() {
            self.message = Some(("파일 열기 안내".into(), opened.notes.join("\n\n")));
        }
    }

    fn open_file(&mut self, path: Option<PathBuf>, import_into: Option<u64>, ctx: &egui::Context) {
        self.open_file_impl(path, import_into, ctx, false);
    }
    fn open_file_impl(
        &mut self,
        path: Option<PathBuf>,
        import_into: Option<u64>,
        ctx: &egui::Context,
        legacy: bool,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        self.busy = Some("파일 여는 중");
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = {
                let path = path.or_else(|| {
                    if legacy {
                        rfd::FileDialog::new()
                            .set_title("이전 개발 PSD/PSB 열기")
                            .add_filter("이전 개발 Photoshop 파일", &["psd", "psb"])
                            .pick_file()
                    } else {
                        rfd::FileDialog::new()
                            .set_title("이미지 또는 프로젝트 열기")
                            .add_filter("이미지", files::IMAGE_EXTENSIONS)
                            .add_filter("Photo Editor 프로젝트", &[files::PROJECT_EXTENSION])
                            .add_filter("모든 파일", &["*"])
                            .pick_file()
                    }
                });
                path.map(|p| {
                    if legacy {
                        files::open_legacy_photoshop(&p)
                    } else {
                        files::open(&p)
                    }
                })
                .transpose()
            };
            let _ = sender.send(JobResult::Open {
                result,
                import_into,
            });
            ctx.request_repaint();
        });
    }

    fn save(&mut self, save_as: bool, ctx: &egui::Context) {
        if self.busy.is_some() {
            return;
        }
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        let Some(tab) = self.tab() else {
            return;
        };
        let doc = tab.editor.document.clone();
        let id = tab.id;
        let revision = tab.editor.revision;
        let path = if save_as { None } else { tab.path.clone() };
        let expected = if save_as {
            None
        } else {
            tab.file_watch.baseline.clone()
        };
        let suggested = format!("{}.{}", tab.title, files::PROJECT_EXTENSION);
        self.busy = Some("프로젝트 저장 중");
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let path = path.or_else(|| {
                    rfd::FileDialog::new()
                        .set_title("프로젝트 저장")
                        .add_filter("Photo Editor 프로젝트", &[files::PROJECT_EXTENSION])
                        .set_file_name(suggested)
                        .save_file()
                });
                if let Some(path) = path {
                    let path = if path.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("psd") || e.eq_ignore_ascii_case("psb")
                    }) {
                        path
                    } else {
                        path.with_extension(files::PROJECT_EXTENSION)
                    };
                    match files::save_document_checked(&doc, &path, expected.as_ref())? {
                        files::CheckedSave::Saved(stamp) => {
                            Ok(Some(SaveOutcome::Saved(path, stamp)))
                        }
                        files::CheckedSave::Conflict(stamp) => {
                            Ok(Some(SaveOutcome::Conflict(path, stamp)))
                        }
                    }
                } else {
                    Ok(None)
                }
            })();
            let _ = sender.send(JobResult::Save {
                result,
                tab: id,
                revision,
            });
            ctx.request_repaint();
        });
    }

    fn export_psd(&mut self, large: bool, ctx: &egui::Context) {
        if self.busy.is_some() {
            return;
        }
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        let Some(tab) = self.tab() else {
            return;
        };
        let doc = tab.editor.document.clone();
        let extension = if large { "psb" } else { "psd" };
        let suggested = format!("{}.{extension}", tab.title);
        let options = self.psd_options;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.busy = Some("PSD/PSB 저장 중");
        std::thread::spawn(move || {
            let result = (|| {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Photoshop 파일 저장")
                    .add_filter("Photoshop", &[extension])
                    .set_file_name(suggested)
                    .save_file()
                {
                    let path = path.with_extension(extension);
                    files::save_photoshop_options(&doc, &path, options)?;
                    Ok(Some(path))
                } else {
                    Ok(None)
                }
            })();
            let _ = sender.send(JobResult::Export(result));
            ctx.request_repaint();
        });
    }

    fn export(&mut self, ctx: &egui::Context) {
        if self.busy.is_some() {
            return;
        }
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        let Some(tab) = self.tab() else {
            return;
        };
        let doc = tab.editor.document.clone();
        let name = files::export_label(self.export_format).expect("valid export selection");
        let extension =
            files::export_extension(self.export_format).expect("valid export selection");
        let suggested = format!("{}-export.{extension}", tab.title);
        let options = self.export_options.clone();
        let index = self.export_format;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        self.busy = Some("이미지 내보내는 중");
        std::thread::spawn(move || {
            let result = (|| {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("이미지 내보내기")
                    .add_filter(name, &[extension])
                    .set_file_name(suggested)
                    .save_file()
                {
                    let path = path.with_extension(extension);
                    files::export_by_index(&doc, &path, index, &options)?;
                    Ok(Some(path))
                } else {
                    Ok(None)
                }
            })();
            let _ = sender.send(JobResult::Export(result));
            ctx.request_repaint();
        });
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        while let Ok(job) = self.jobs.try_recv() {
            self.busy = None;
            match job {
                JobResult::Recover { result, entry } => match result {
                    Ok(document) => {
                        self.add_tab(files::Opened {
                            source_stamp: None,
                            document,
                            project_path: None,
                            title: format!("{} (복구됨)", entry.title),
                            notes: Vec::new(),
                        });
                        self.tab_mut().unwrap().recovered_from = Some(entry.key.clone());
                        self.recovery_entries.retain(|e| e.key != entry.key);
                        self.status = "복구 사본을 새 문서로 열었습니다.".into();
                    }
                    Err(error) => self.message = Some(("복구 실패".into(), error)),
                },
                JobResult::Open {
                    result,
                    import_into,
                } => match result {
                    Ok(Some(opened)) => {
                        if let Some(id) = import_into {
                            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                                let doc = &mut tab.editor.document;
                                if doc.pixel_count() + opened.document.pixel_count()
                                    > document::MAX_PIXELS
                                {
                                    self.message = Some((
                                        "가져오기 실패".into(),
                                        "레이어가 현재 문서의 메모리 예산을 초과합니다.".into(),
                                    ));
                                    continue;
                                }
                                tab.editor.edit("레이어 가져오기", |doc| {
                                    let _ = doc.import_layers(opened.document);
                                });
                                tab.edit_mask = false;
                                tab.render_dirty = true;
                                tab.notes.extend(opened.notes.clone());
                                if !opened.notes.is_empty() {
                                    self.message =
                                        Some(("가져오기 안내".into(), opened.notes.join("\n\n")));
                                }
                            }
                        } else {
                            self.add_tab(opened);
                        }
                        self.status = "파일을 열었습니다.".into();
                    }
                    Ok(None) => {}
                    Err(error) => self.message = Some(("파일 열기 실패".into(), error)),
                },
                JobResult::Save {
                    result,
                    tab: id,
                    revision,
                } => match result {
                    Ok(Some(SaveOutcome::Saved(path, stamp))) => {
                        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                            tab.title = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            tab.path = Some(path);
                            tab.file_watch.reset(stamp);
                            tab.editor.saved_revision = Some(revision);
                        }
                        self.status = "프로젝트를 저장했습니다.".into();
                        self.complete_close(ctx);
                    }
                    Ok(Some(SaveOutcome::Conflict(path, snapshot))) => {
                        self.close_target = None;
                        self.external_dialog = Some(ExternalDialog {
                            tab: id,
                            path,
                            snapshot,
                            saving: true,
                        });
                    }
                    Ok(None) => {
                        self.close_target = None;
                    }
                    Err(error) => self.message = Some(("프로젝트 저장 실패".into(), error)),
                },
                JobResult::Export(result) => match result {
                    Ok(Some(path)) => {
                        self.status = format!(
                            "내보냄: {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )
                    }
                    Ok(None) => {}
                    Err(error) => self.message = Some(("내보내기 실패".into(), error)),
                },
            }
        }
    }

    fn poll_files(&mut self, ctx: &egui::Context) {
        for tab in &mut self.tabs {
            if let Some(path) = &tab.path {
                tab.file_watch.poll(path, ctx);
            }
        }
        if self.external_dialog.is_some()
            || self.command_panel.open
            || self.command_options.is_some()
            || self.busy.is_some()
            || self.message.is_some()
            || self.text_session.is_some()
            || self.transforming
            || self.new_dialog
            || self.export_dialog
            || self.psd_dialog.is_some()
            || self.rename.is_some()
            || self.close_target.is_some()
            || self.size_dialog.is_some()
            || self.raw_dialog.is_some()
            || self.adjust_dialog.is_some()
            || self.content_dialog.is_some()
            || self.recovery_dialog
            || (self.colors_panel.open && !self.canvas_only)
            || self.about_panel.open
            || (self.shortcuts_panel.open && !self.canvas_only)
            || self.tabs.iter().any(|tab| tab.gesture.is_some())
        {
            return;
        }
        for tab in &mut self.tabs {
            if let (Some(path), Some(snapshot)) = (&tab.path, tab.file_watch.notice.take()) {
                self.external_dialog = Some(ExternalDialog {
                    tab: tab.id,
                    path: path.clone(),
                    snapshot,
                    saving: false,
                });
                break;
            }
        }
    }

    fn request_close(&mut self, target: CloseTarget, ctx: &egui::Context) {
        self.close_command_options(false);
        self.finish_gesture();
        if self.text_session.is_some() {
            return;
        }
        self.close_target = Some(target);
        self.complete_close(ctx);
    }

    fn clear_recovery(&mut self, index: usize) {
        let tab = &mut self.tabs[index];
        if let Some(work) = &tab.auto_work {
            work.alive.store(false, Ordering::SeqCst);
        }
        if let Some(store) = &self.recovery {
            let mut keys = vec![store.key(tab.id)];
            if let Some(key) = tab.recovered_from.take() {
                keys.push(key);
            }
            for key in keys {
                if let Err(error) = store.remove(&key) {
                    self.status = format!("복구 사본 정리 실패: {error}");
                }
            }
        }
        tab.auto_revision = None;
    }

    fn auto_save(&mut self, ctx: &egui::Context) {
        while let Ok(report) = self.auto_jobs.try_recv() {
            if let Some(index) = self.tabs.iter().position(|t| t.id == report.tab) {
                let tab = &mut self.tabs[index];
                if tab
                    .auto_work
                    .as_ref()
                    .is_some_and(|w| w.revision == report.revision)
                {
                    tab.auto_work = None;
                    match report.result {
                        Ok(()) => tab.auto_revision = Some(report.revision),
                        Err(error) => {
                            tab.notes.push(format!("자동 저장 실패: {error}"));
                            self.status = "자동 저장에 실패했습니다. 직접 저장해주세요.".into();
                        }
                    }
                    if !tab.editor.dirty() {
                        self.clear_recovery(index);
                    }
                }
            }
        }
        for index in 0..self.tabs.len() {
            if !self.tabs[index].editor.dirty()
                && (self.tabs[index].auto_revision.is_some()
                    || self.tabs[index].recovered_from.is_some())
            {
                self.clear_recovery(index);
            }
        }
        if matches!(self.close_target, Some(CloseTarget::App))
            && self.tabs.iter().all(|t| !t.editor.dirty())
        {
            self.complete_close(ctx);
        }
        let Some(store) = &self.recovery else {
            return;
        };
        let now = Instant::now();
        if now < self.auto_deadline {
            ctx.request_repaint_after(self.auto_deadline - now);
            return;
        }
        self.auto_deadline = now + Duration::from_secs(self.auto_interval);
        ctx.request_repaint_after(Duration::from_secs(self.auto_interval));
        for tab in &mut self.tabs {
            if !tab.editor.dirty()
                || tab.editor.is_editing()
                || tab.auto_work.is_some()
                || tab.auto_revision == Some(tab.editor.revision)
            {
                continue;
            }
            let alive = Arc::new(AtomicBool::new(true));
            let revision = tab.editor.revision;
            tab.auto_work = Some(AutoWork {
                revision,
                alive: alive.clone(),
            });
            let entry = recovery::Entry {
                document_id: Some(tab.editor.document.document_id.clone()),
                key: store.key(tab.id),
                title: tab.title.clone(),
                original: tab.path.clone(),
                revision,
                saved_at: 0,
            };
            let document = tab.editor.document.clone();
            let store = store.clone();
            let sender = self.auto_sender.clone();
            let ctx = ctx.clone();
            let id = tab.id;
            std::thread::spawn(move || {
                let result = store.write(&entry, &document);
                if !alive.load(Ordering::SeqCst) {
                    let _ = store.remove(&entry.key);
                }
                let _ = sender.send(AutoResult {
                    tab: id,
                    revision,
                    result,
                });
                ctx.request_repaint();
            });
        }
    }

    fn complete_close(&mut self, ctx: &egui::Context) {
        match self.close_target {
            Some(CloseTarget::Tab(id)) => {
                if let Some(index) = self.tabs.iter().position(|t| t.id == id) {
                    if self.tabs[index].editor.dirty() {
                        self.active = index;
                        return;
                    }
                    self.clear_recovery(index);
                    self.tabs.remove(index);
                    self.active = self.active.min(self.tabs.len().saturating_sub(1));
                }
                self.close_target = None;
            }
            Some(CloseTarget::App) => {
                if let Some(index) = self.tabs.iter().position(|t| t.editor.dirty()) {
                    self.active = index;
                } else {
                    for index in 0..self.tabs.len() {
                        self.clear_recovery(index);
                    }
                    if self.tabs.iter().any(|t| t.auto_work.is_some()) {
                        self.status = "자동 저장을 마무리하고 있습니다.".into();
                        ctx.request_repaint_after(Duration::from_millis(100));
                        return;
                    }
                    self.allow_exit = true;
                    self.close_target = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            None => {}
        }
    }

    fn toggle_canvas_only(&mut self, ctx: &egui::Context) {
        if !self.canvas_only && self.tabs.is_empty() {
            return;
        }
        if !self.canvas_only {
            self.canvas_was_fullscreen = ctx.input(|i| i.viewport().fullscreen).unwrap_or(false);
        }
        self.canvas_only = !self.canvas_only;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(
            self.canvas_only || self.canvas_was_fullscreen,
        ));
        ctx.request_repaint();
    }
    fn workspace_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pressed = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::PointerButton { pressed: true, .. }))
        });
        if self.rename.is_some() && pressed {
            self.rename_pointer_blocked = true;
        }
        if !pressed && !ctx.input(|i| i.pointer.any_down()) {
            self.rename_pointer_blocked = false;
        }

        if !self.canvas_only {
            self.top_ui(ui, ctx);
            self.status_ui(ui, ctx);
            if !self.tabs.is_empty() {
                if self.workspace.layout.tools {
                    self.tools_ui(ui);
                }
                self.properties_ui(ui);
            }
        }
        self.canvas_ui(ui, ctx);
        self.floating_panels_ui(ctx);
    }
    fn run_action(&mut self, action: crate::shortcuts::Action, ctx: &egui::Context) {
        use crate::shortcuts::Action;
        match action {
            Action::CommandPalette => self.open_commands(),
            Action::CanvasOnly => self.toggle_canvas_only(ctx),
            Action::New => self.new_dialog = true,
            Action::Open => self.open_file(None, None, ctx),
            Action::Save => self.save(false, ctx),
            Action::SaveAs => self.save(true, ctx),
            Action::Close => {
                if let Some(tab) = self.tab() {
                    self.request_close(CloseTarget::Tab(tab.id), ctx);
                }
            }
            Action::SwapColors => std::mem::swap(&mut self.color, &mut self.background),
            Action::DefaultColors => {
                self.color = [0, 0, 0, 255];
                self.background = [255; 4];
            }
            Action::Transform => self.start_transform(),
            Action::Copy => self.copy(false, false),
            Action::CopyMerged => self.copy(false, true),
            Action::Cut => self.copy(true, false),
            Action::Paste => self.paste(),
            Action::InvertSelection => self.invert_selection(),
            Action::Redo | Action::RedoAlternate | Action::Undo => {
                self.finish_gesture();
                if let Some(tab) = self.tab_mut() {
                    if action == Action::Undo {
                        tab.editor.undo();
                    } else {
                        tab.editor.redo();
                    }
                    tab.render_dirty = true;
                }
            }
            Action::Deselect => {
                if let Some(tab) = self.tab_mut() {
                    tab.selection = None;
                }
            }
            Action::Tool(tool) => self.set_tool(tool),
            Action::Menu(id) => {
                if let Some(command) = crate::commands::Command::from_shortcut_id(id) {
                    self.execute_command(command, ctx);
                }
            }
        }
    }
    fn command_blocked(&self) -> bool {
        self.busy.is_some()
            || self.transforming
            || self.text_session.is_some()
            || self.new_dialog
            || self.export_dialog
            || self.message.is_some()
            || self.rename.is_some()
            || self.close_target.is_some()
            || self.size_dialog.is_some()
            || self.raw_dialog.is_some()
            || self.adjust_dialog.is_some()
            || self.content_dialog.is_some()
            || self.external_dialog.is_some()
            || self.psd_dialog.is_some()
            || self.recovery_dialog
            || self.command_options.is_some()
            || self.about_panel.open
            || (!self.canvas_only && (self.colors_panel.open || self.shortcuts_panel.open))
    }
    fn open_commands(&mut self) {
        if !self.command_blocked() {
            self.finish_gesture();
            self.command_panel.show();
        }
    }
    fn command_reason(&self, command: crate::commands::Command) -> Option<&'static str> {
        use crate::{commands::Command::*, shortcuts::Action as A};
        if self.command_blocked() {
            return Some("진행 중인 작업이나 입력을 먼저 마치세요.");
        }
        let tab = self.tab();
        let doc = tab.map(|t| &t.editor.document);
        let layer = doc.and_then(|d| d.active_layer());
        let has_document = tab.is_some();
        let global = matches!(
            command,
            Action(A::New | A::Open | A::SwapColors | A::DefaultColors | A::CommandPalette)
                | Recovery
                | LegacyPhotoshop
                | PixelGrid
                | Histogram
                | Tools
                | GroupedTools
                | Layers
                | Properties
                | History
                | Navigator
                | Colors
                | MaskView
                | MaskOverlay
                | OverlaySettings
                | LeftPanels
                | ResetWorkspace
                | Shortcuts
                | ColorWindow
                | About
                | Licenses
                | DisplaySampling(_)
        );
        if !global && !has_document {
            return Some("문서를 먼저 열거나 만드세요.");
        }
        match command {
            Action(A::Transform)
                if tab.is_some_and(|t| {
                    if t.edit_mask {
                        t.editor.document.is_locked(t.editor.document.active)
                            || t.editor
                                .document
                                .active_layer()
                                .is_none_or(|l| l.mask.is_none())
                    } else {
                        !t.editor.document.movable(&t.selected_ids())
                    }
                }) =>
            {
                Some("잠금 해제한 레이어·그룹 또는 마스크를 선택하세요.")
            }
            Delete if tab.is_some_and(|t| !t.editor.document.movable(&t.selected_ids())) => {
                Some("잠긴 레이어는 삭제할 수 없습니다.")
            }
            RawDevelop
                if layer.is_none_or(|l| l.raw_source.is_none() || doc.unwrap().is_locked(l.id)) =>
            {
                Some("잠금 해제한 RAW 원본 레이어를 선택하세요.")
            }
            Recovery if self.recovery.is_none() => Some("자동 저장 복구를 사용할 수 없습니다."),
            Action(A::Undo) if tab.is_none_or(|t| !t.editor.can_undo()) => {
                Some("실행 취소할 작업이 없습니다.")
            }
            Action(A::Redo | A::RedoAlternate) if tab.is_none_or(|t| !t.editor.can_redo()) => {
                Some("다시 실행할 작업이 없습니다.")
            }
            Action(A::Paste) if self.clipboard.is_none() => Some("복사한 이미지가 없습니다."),
            Action(A::Copy | A::Cut) if layer.is_some_and(|l| l.adjustment_layer) => {
                Some("조정 레이어는 선택 레이어 복사 또는 병합하여 복사를 사용하세요.")
            }
            AlphaSelection if layer.is_some_and(|l| l.adjustment_layer) => {
                Some("조정 레이어는 마스크에서 선택 영역을 만드세요.")
            }
            PasteLayers if self.layer_clipboard.is_none() => Some("복사한 레이어가 없습니다."),
            Refine | Crop if tab.is_none_or(|t| t.selection.is_none()) => {
                Some("선택 영역을 먼저 만드세요.")
            }
            AdjustmentLayer
                if doc.is_some_and(|d| d.active_layer().is_some_and(|l| d.is_locked(l.id))) =>
            {
                Some("잠금 해제한 레이어·그룹 위에 조정을 추가하세요.")
            }
            Adjust | Filter if doc.is_some_and(|d| d.is_locked(d.active)) => {
                Some("잠금 해제한 레이어에 보정을 적용하세요.")
            }
            MaskSelection if layer.is_none_or(|l| l.mask.is_none()) => {
                Some("레이어에 마스크가 없습니다.")
            }
            Rasterize if layer.is_none_or(|l| l.canvas_space() || doc.unwrap().is_locked(l.id)) => {
                Some("잠금 해제한 픽셀·텍스트·벡터 레이어를 선택하세요.")
            }
            Delete if doc.is_none_or(|d| d.layers.len() <= 1) => {
                Some("마지막 레이어는 삭제할 수 없습니다.")
            }
            MergeDown if tab.is_none_or(|t| t.selected_ids().len() != 1) => {
                Some("레이어 하나를 선택하세요.")
            }
            Ungroup if layer.is_none_or(|l| l.parent.is_none()) => {
                Some("그룹 안의 레이어를 선택하세요.")
            }
            VerticalGuide | HorizontalGuide
                if doc.is_some_and(|d| d.layout.guides.len() >= 4096) =>
            {
                Some("가이드는 최대 4096개입니다.")
            }
            _ => None,
        }
    }
    fn command_entries(&self) -> Vec<crate::commands::Entry> {
        crate::commands::catalog()
            .into_iter()
            .map(|mut entry| {
                entry.disabled = self.command_reason(entry.command);
                if let Some(action) = entry.command.shortcut_action() {
                    entry.shortcut = self.workspace.layout.shortcuts.label(action);
                    if entry.shortcut == "지정 안 함" {
                        entry.shortcut.clear();
                    }
                }
                entry
            })
            .collect()
    }
    fn execute_command(&mut self, command: crate::commands::Command, ctx: &egui::Context) {
        use crate::commands::Command::*;
        // Re-check current document, selection and job state immediately before execution.
        if let Some(reason) = self.command_reason(command) {
            self.status = reason.into();
            return;
        }
        match command {
            Action(action) => self.run_action(action, ctx),
            Recovery => {
                if let Some(store) = &self.recovery {
                    match store.list() {
                        Ok(listing) => {
                            self.recovery_entries = listing.entries;
                            self.recovery_warnings = listing.warnings;
                            self.recovery_dialog = true;
                        }
                        Err(error) => self.message = Some(("복구 목록 읽기 실패".into(), error)),
                    }
                }
            }
            RawDevelop => self.open_raw(ctx),
            LegacyPhotoshop => self.open_file_impl(None, None, ctx, true),
            Psd | Psb => self.psd_dialog = Some(command == Psb),
            Import => self.open_file(None, self.tab().map(|t| t.id), ctx),
            Export => self.export_dialog = true,
            Fill | Gradient => self.open_fill(command == Gradient),
            Pattern => self.open_pattern(),
            CopyLayers => self.copy_layers(),
            PasteLayers => self.paste_layers(),
            MergeSelected => self.merge_selected(),
            MergeDown => self.merge_down(),
            MergeVisible => self.merge_layers(),
            Rasterize => self.apply_result("래스터화", |doc| {
                editing::rasterize(doc.active_layer_mut().unwrap())
            }),
            NewLayer => self.new_layer(),
            AdjustmentLayer => self.new_adjustment_layer(),
            Duplicate => self.duplicate_layer(),
            Delete => self.delete_layer(),
            Group => self.group_selected(),
            Ungroup => self.edit("그룹에서 꺼내기", |doc| {
                let parent = doc.active_layer().unwrap().parent;
                let grandparent = parent
                    .and_then(|id| doc.layers.iter().find(|l| l.id == id))
                    .and_then(|l| l.parent);
                doc.active_layer_mut().unwrap().parent = grandparent;
            }),
            Rename => {
                if let Some(layer) = self.tab().map(|t| t.editor.document.active) {
                    self.start_rename(layer, ctx);
                }
            }
            AlphaSelection | MaskSelection => {
                if let Some(t) = self.tab_mut() {
                    t.selection =
                        editing::layer_selection(&t.editor.document, command == MaskSelection);
                }
            }
            SelectAll => {
                if let Some(t) = self.tab_mut() {
                    t.selection = Some(Selection::rectangle(
                        [0, 0],
                        [
                            t.editor.document.width as i32,
                            t.editor.document.height as i32,
                        ],
                    ));
                }
            }
            LayoutSettings | GuideEdit => {
                self.finish_gesture();
                self.layout_options = self.tab().map(|t| (t.id, t.editor.document.layout.clone()));
                self.command_options = Some(command);
            }
            Refine | OverlaySettings => self.command_options = Some(command),
            PixelGrid => self.show_grid = !self.show_grid,
            Histogram => self.show_histogram = !self.show_histogram,
            Fit => {
                if let Some(t) = self.tab_mut() {
                    t.view.fit = true;
                }
            }
            DisplaySampling(mode) => self.workspace.layout.display_sampling = mode,
            ActualSize => {
                if let Some(t) = self.tab_mut() {
                    t.view.zoom = 1.0;
                    t.view.pan = Vec2::ZERO;
                    t.view.fit = false;
                }
            }
            Rulers | Guides | Grid | Snap | SnapGuides | SnapGrid | SnapLayers | SnapDocument
            | VerticalGuide | HorizontalGuide | ClearGuides => {
                self.edit("가이드·격자 설정", |doc| {
                    let layout = &mut doc.layout;
                    match command {
                        Rulers => layout.rulers = !layout.rulers,
                        Guides => layout.guides_visible = !layout.guides_visible,
                        Grid => layout.grid_visible = !layout.grid_visible,
                        Snap => layout.snap = !layout.snap,
                        SnapGuides => layout.snap_guides = !layout.snap_guides,
                        SnapGrid => layout.snap_grid = !layout.snap_grid,
                        SnapLayers => layout.snap_layers = !layout.snap_layers,
                        SnapDocument => layout.snap_document = !layout.snap_document,
                        VerticalGuide | HorizontalGuide => {
                            let axis = usize::from(command == HorizontalGuide);
                            layout.guides.push(crate::guides::Guide {
                                axis,
                                position: if axis == 0 {
                                    doc.width as f32 / 2.0
                                } else {
                                    doc.height as f32 / 2.0
                                },
                            });
                        }
                        ClearGuides => layout.guides.clear(),
                        _ => unreachable!(),
                    }
                })
            }
            Resize | CanvasSize => self.open_resize(command == Resize),
            Crop => self.crop(),
            Adjust | Filter => self.open_adjustments(),
            Trim => {
                self.apply_result("투명 가장자리 자르기", editing::trim_transparent);
                if let Some(t) = self.tab_mut() {
                    t.selection = None;
                    t.view.fit = true;
                }
            }
            FlipH | FlipV => {
                self.edit(
                    if command == FlipH {
                        "캔버스 좌우 뒤집기"
                    } else {
                        "캔버스 상하 뒤집기"
                    },
                    |doc| editing::flip_canvas(doc, command == FlipH),
                );
                if let Some(t) = self.tab_mut() {
                    t.selection = None;
                }
            }
            Tools => self.workspace.layout.tools = !self.workspace.layout.tools,
            GroupedTools => {
                self.workspace.layout.grouped_tools = !self.workspace.layout.grouped_tools
            }
            Layers => self.workspace.layout.layers = !self.workspace.layout.layers,
            Properties => self.workspace.layout.properties = !self.workspace.layout.properties,
            History => self.workspace.layout.history = !self.workspace.layout.history,
            Navigator => self.workspace.layout.navigator = !self.workspace.layout.navigator,
            Colors => self.workspace.layout.colors = !self.workspace.layout.colors,
            MaskView => self.mask_view.open = !self.mask_view.open,
            MaskOverlay => {
                self.workspace.layout.mask_overlay.enabled =
                    !self.workspace.layout.mask_overlay.enabled
            }
            LeftPanels => self.workspace.layout.left = !self.workspace.layout.left,
            ResetWorkspace => {
                let shortcuts = self.workspace.layout.shortcuts.clone();
                self.workspace.layout = Default::default();
                crate::panels::reset(ctx);
                self.workspace.layout.shortcuts = shortcuts;
                self.mask_view.open = false;
                self.show_histogram = false;
                self.show_grid = false;
                for id in ["properties", "properties-left"] {
                    ctx.data_mut(|d| {
                        d.remove::<egui::containers::panel::PanelState>(egui::Id::new(id))
                    });
                }
            }
            Shortcuts => self.shortcuts_panel.open = true,
            ColorWindow => self.colors_panel.show(self.color, self.background),
            About | Licenses => self.about_panel.show(command == Licenses),
        }
    }
    fn close_command_options(&mut self, apply: bool) {
        if let Some((id, before)) = self.layout_options.take()
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
        {
            if apply && tab.editor.document.layout != before {
                tab.editor.finish();
            } else {
                tab.editor.cancel();
            }
            tab.render_dirty = true;
        }
        self.command_options = None;
    }
    fn command_ui(&mut self, ctx: &egui::Context) {
        let entries = self.command_entries();
        if let Some(command) = self.command_panel.ui(ctx, &entries) {
            self.execute_command(command, ctx);
        }
        if self
            .layout_options
            .as_ref()
            .is_some_and(|(id, _)| self.tab().is_none_or(|t| t.id != *id))
        {
            self.close_command_options(false);
            return;
        }
        let Some(command) = self.command_options else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("command-options")).show(ctx, |ui| {
            use crate::commands::Command;
            let width = (ctx.content_rect().width() - 64.).clamp(96., 460.);
            ui.set_width(width);
            ui.heading(match command {
                Command::Refine => "선택 경계",
                Command::OverlaySettings => "마스크 오버레이 설정",
                Command::GuideEdit => "가이드 편집",
                _ => "격자·스냅 설정",
            });
            egui::ScrollArea::both()
                .id_salt((
                    "command-options-body",
                    command.shortcut_action().map(|a| a.label()),
                ))
                .max_width(width)
                .max_height((ctx.content_rect().height() - 170.).clamp(80., 520.))
                .show(ui, |ui| {
                    ui.set_max_width(width);
                    match command {
                        Command::Refine => {
                            ui.add(
                                crate::numeric_slider::Slider::new(
                                    &mut self.selection_feather,
                                    0.0..=100.0,
                                )
                                .text("페더 px"),
                            );
                            ui.add(
                                crate::numeric_slider::Slider::new(
                                    &mut self.selection_expand,
                                    -100..=100,
                                )
                                .text("확대·축소 px"),
                            );
                        }
                        Command::OverlaySettings => self.workspace.layout.mask_overlay.ui(ui),
                        _ => self.layout_menu(ui, command),
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if command == Command::Refine {
                    if ui.button("선택 경계에 적용").clicked() {
                        let (feather, expand) = (self.selection_feather, self.selection_expand);
                        if let Some(t) = self.tab_mut() {
                            let doc = &t.editor.document;
                            t.selection = t.selection.as_ref().map(|selection| {
                                editing::refine_selection(
                                    selection, doc.width, doc.height, feather, expand,
                                )
                            });
                        }
                        self.close_command_options(true);
                    }
                } else if self.layout_options.is_some() && ui.button("적용").clicked() {
                    self.close_command_options(true);
                }
                let label = if command == Command::OverlaySettings {
                    "닫기"
                } else {
                    "취소"
                };
                if ui.button(label).clicked() {
                    self.close_command_options(false);
                }
            });
        });
        if response.should_close() {
            self.close_command_options(false);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.command_panel.open || self.command_options.is_some() {
            return;
        }
        if self.text_session.is_some() {
            return;
        }
        if self.new_dialog
            || self.export_dialog
            || self.message.is_some()
            || self.rename.is_some()
            || self.close_target.is_some()
            || self.size_dialog.is_some()
            || self.raw_dialog.is_some()
            || self.adjust_dialog.is_some()
            || self.content_dialog.is_some()
            || self.external_dialog.is_some()
            || self.psd_dialog.is_some()
            || self.recovery_dialog
            || (self.colors_panel.open && !self.canvas_only)
            || self.about_panel.open
            || (self.shortcuts_panel.open && !self.canvas_only)
        {
            return;
        }
        if !ctx.egui_wants_keyboard_input()
            && self
                .workspace
                .layout
                .shortcuts
                .consume_action(ctx, crate::shortcuts::Action::CanvasOnly)
        {
            self.toggle_canvas_only(ctx);
            return;
        }
        if self.transforming {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.end_transform(false);
            } else if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(Key::Enter)) {
                self.end_transform(true);
            }
            return;
        }
        if self.canvas_only
            && !ctx.egui_wants_keyboard_input()
            && ctx.input(|i| i.key_pressed(Key::Escape))
            && self.tab().is_none_or(|t| {
                t.gesture.is_none() && !t.editor.is_editing() && t.crop_before.is_none()
            })
        {
            self.toggle_canvas_only(ctx);
            return;
        }
        if self.tool == Tool::Crop && !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.cancel_crop();
                return;
            }
            if ctx.input(|i| i.key_pressed(Key::Enter)) {
                self.crop();
                return;
            }
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if let Some(action) = self.workspace.layout.shortcuts.consume(ctx) {
            self.run_action(action, ctx);
        }
        let pen_tool = self.tool == Tool::Pen;
        if ctx.input(|i| i.key_pressed(Key::Escape))
            && let Some(tab) = self.tab_mut()
        {
            tab.editor.cancel();
            tab.pen.reset();
            tab.selection = match tab.gesture.take() {
                Some(Gesture::Polygon { previous, .. })
                | Some(Gesture::Lasso { previous, .. })
                | Some(Gesture::Select { previous, .. }) => previous,
                Some(Gesture::Boundary { previous, .. })
                | Some(Gesture::Pixels { previous, .. }) => Some(previous),
                _ if pen_tool => tab.selection.clone(),
                _ => None,
            };
            tab.render_dirty = true;
        }
    }

    fn status_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(tab) = self.tab_mut() {
                    ui.add(
                        egui::DragValue::new(&mut tab.view.zoom)
                            .range(0.01..=32.0)
                            .speed(0.01)
                            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                            .custom_parser(|text| {
                                text.trim()
                                    .trim_end_matches('%')
                                    .parse::<f64>()
                                    .ok()
                                    .map(|v| v / 100.0)
                            }),
                    );
                    if ui.small_button("화면 맞춤").clicked() {
                        tab.view.fit = true;
                    }
                    if ui.small_button("100%").clicked() {
                        tab.view.zoom = 1.0;
                        tab.view.pan = Vec2::ZERO;
                        tab.view.fit = false;
                    }
                    if let Some(warning) = &tab.file_watch.warning {
                        ui.label(
                            egui::RichText::new("파일 변경 확인 실패").color(egui::Color32::YELLOW),
                        )
                        .on_hover_text(warning);
                    }
                    ui.separator();
                    ui.label(format!(
                        "{} × {} px",
                        tab.editor.document.width, tab.editor.document.height
                    ));
                    ui.separator();
                    ui.label(format!(
                        "RGB / {}비트",
                        tab.editor
                            .document
                            .layers
                            .iter()
                            .map(|l| l.bit_depth)
                            .max()
                            .unwrap_or(8)
                    ));
                }
                if let Some(tab) = self.tab()
                    && let Some(area) = tab.canvas_area
                    && let Some(pointer) = ctx
                        .input(|i| i.pointer.hover_pos())
                        .filter(|p| area.contains(*p))
                {
                    let point = tab.view.document_pos(
                        area,
                        [tab.editor.document.width, tab.editor.document.height],
                        ctx.pixels_per_point(),
                        pointer,
                    );
                    ui.separator();
                    ui.label(format!("X: {:.1}  Y: {:.1} px", point[0], point[1]));
                }
                if let Some(busy) = self.busy {
                    ui.spinner();
                    ui.label(busy);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(&self.status).small().weak());
                });
            });
        });
    }

    fn menu_entries(&self) -> Vec<crate::commands::Entry> {
        use crate::{
            commands::{Command, Entry},
            shortcuts::Action,
        };
        let mut entries: Vec<_> = self
            .command_entries()
            .into_iter()
            .filter(|e| e.category != "도구")
            .collect();
        entries.insert(
            0,
            Entry {
                command: Command::Action(Action::CommandPalette),
                category: "편집",
                label: Action::CommandPalette.label(),
                keywords: "command search",
                shortcut: self
                    .workspace
                    .layout
                    .shortcuts
                    .label(Action::CommandPalette),
                disabled: self.command_reason(Command::Action(Action::CommandPalette)),
            },
        );
        for entry in &mut entries {
            if entry.shortcut == "지정 안 함" {
                entry.shortcut.clear();
            }
            if entry.command == Command::Action(Action::Redo) {
                let alternate = self.workspace.layout.shortcuts.label(Action::RedoAlternate);
                if alternate != "지정 안 함" && alternate != entry.shortcut {
                    if !entry.shortcut.is_empty() {
                        entry.shortcut.push_str(" / ");
                    }
                    entry.shortcut.push_str(&alternate);
                }
            }
        }
        entries
    }
    fn command_checked(&self, command: crate::commands::Command) -> Option<bool> {
        use crate::{commands::Command::*, shortcuts::Action as A};
        let workspace = &self.workspace.layout;
        let layout = self.tab().map(|t| &t.editor.document.layout);
        Some(match command {
            Action(A::CanvasOnly) => self.canvas_only,
            PixelGrid => self.show_grid,
            Histogram => self.show_histogram,
            Tools => workspace.tools,
            GroupedTools => workspace.grouped_tools,
            Layers => workspace.layers,
            Properties => workspace.properties,
            History => workspace.history,
            Navigator => workspace.navigator,
            Colors => workspace.colors,
            MaskView => self.mask_view.open,
            MaskOverlay => workspace.mask_overlay.enabled,
            LeftPanels => workspace.left,
            DisplaySampling(mode) => workspace.display_sampling == mode,
            Rulers => layout?.rulers,
            Guides => layout?.guides_visible,
            Grid => layout?.grid_visible,
            Snap => layout?.snap,
            SnapGuides => layout?.snap_guides,
            SnapGrid => layout?.snap_grid,
            SnapLayers => layout?.snap_layers,
            SnapDocument => layout?.snap_document,
            _ => return None,
        })
    }
    fn main_menus_ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        entries: &[crate::commands::Entry],
    ) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("PHOTO EDITOR").strong().color(ACCENT));
                ui.separator();
                for category in crate::commands::MENU_CATEGORIES {
                    ui.menu_button(*category, |ui| {
                        ui.set_max_width((ctx.content_rect().width() - 48.).clamp(180., 460.));
                        egui::ScrollArea::vertical()
                            .id_salt(("main-menu", category))
                            .max_height((ctx.content_rect().height() - 100.).max(100.))
                            .show(ui, |ui| {
                                for entry in entries.iter().filter(|e| e.category == *category) {
                                    let mut button = egui::Button::new(entry.label)
                                        .shortcut_text(entry.shortcut.clone()).wrap();
                                    if let Some(checked) = self.command_checked(entry.command) {
                                        button = button.selected(checked);
                                    }
                                    let mut response = ui.add_enabled(entry.disabled.is_none(), button);
                                    if let Some(reason) = entry.disabled {
                                        response = response.on_disabled_hover_text(reason);
                                    }
                                    if let crate::commands::Command::DisplaySampling(mode) = entry.command {
                                        response = response.on_hover_text(format!("{} 화면 표시에 적용하며 원본과 저장 파일은 유지합니다.", mode.label()));
                                    }
                                    if entry.command == crate::commands::Command::LegacyPhotoshop {
                                        response = response.on_hover_text("이전 개발 버전이 저장한 픽셀 레이어의 순서를 유지합니다. 일반 외부 PSD/PSB는 기본 열기를 사용하세요.");
                                    }
                                    if response.clicked() {
                                        self.execute_command(entry.command, ctx);
                                        ui.close();
                                    }
                                }
                            });
                    });
                }
                if let Some(busy) = self.busy {
                    ui.spinner();
                    ui.label(busy);
                }
            });
        });
    }
    fn top_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let entries = self.menu_entries();
        egui::Panel::top("menu").show(ui, |ui| {
            if self.transforming {
                ui.disable();
            }
            self.main_menus_ui(ui, ctx, &entries);
        });
        if self.tabs.is_empty() {
            return;
        }
        egui::Panel::top("options").show(ui, |ui| {
            if self.transforming {
                ui.disable();
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(self.tool.name()).strong());
                ui.separator();
                match self.tool {
                    Tool::Brush | Tool::Eraser | Tool::Stamp | Tool::Heal | Tool::Blur => {
                        crate::numeric_slider::labeled_drag(ui,"크기",&mut self.brush_size,1.0..=500.0,1.," px");
                        ui.add(
                            crate::numeric_slider::Slider::new(&mut self.brush_opacity, 0.0..=1.0).text("불투명도")
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                        );
                        ui.add(
                            crate::numeric_slider::Slider::new(&mut self.brush_hardness, 0.0..=1.0).text("경도")
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                        );
                        if self.tool == Tool::Brush {
                            ui.color_edit_button_srgba_unmultiplied(&mut self.color);
                        }
                        ui.add(
                            crate::numeric_slider::Slider::new(&mut self.brush_smoothing, 0.0..=0.95)
                                .text("흔들림 보정")
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                        );
                        if matches!(self.tool, Tool::Brush | Tool::Eraser) {
                            ui.label("Shift+클릭: 이전 지점에서 직선");
                        }
                        if self.tool == Tool::Stamp {
                            ui.checkbox(&mut self.clone_aligned, "정렬 유지");
                            ui.label("Alt+클릭으로 샘플 지점 지정");
                        }
                        if matches!(self.tool, Tool::Stamp | Tool::Heal | Tool::Blur) {
                            ui.checkbox(&mut self.sample_merged, "전체 합성에서 샘플");
                        }
                    }
                    Tool::Select | Tool::Ellipse | Tool::Lasso | Tool::Crop => {
                        if self.tool == Tool::Crop {
                            egui::ComboBox::from_id_salt("crop-ratio")
                                .selected_text(
                                    self.crop_ratio
                                        .map_or("자유 비율".into(), |r| format!("{r:.3}:1")),
                                )
                                .show_ui(ui, |ui| {
                                    for (ratio, label) in [
                                        (None, "자유 비율"),
                                        (Some(1.0), "1:1"),
                                        (Some(4.0 / 3.0), "4:3"),
                                        (Some(3.0 / 2.0), "3:2"),
                                        (Some(16.0 / 9.0), "16:9"),
                                    ] {
                                        ui.selectable_value(&mut self.crop_ratio, ratio, label);
                                    }
                                    if let Some(tab) = self.tab() {
                                        let ratio = tab.editor.document.width as f32
                                            / tab.editor.document.height as f32;
                                        ui.selectable_value(
                                            &mut self.crop_ratio,
                                            Some(ratio),
                                            "현재 문서 비율",
                                        );
                                    }
                                });
                            if ui.button("가로·세로 교환").clicked() {
                                self.crop_ratio = self.crop_ratio.map(|r| 1.0 / r);
                            }
                            ui.label("Alt: 중심에서 조절 · Shift: 정사각형");
                        }
                        if matches!(self.tool,Tool::Ellipse|Tool::Lasso){ui.checkbox(&mut self.workspace.layout.selection_smooth,"경계 부드럽게");}
                        if self.tool == Tool::Lasso {
                            if ui.checkbox(&mut self.polygon_lasso, "다각형").changed() {
                                self.finish_gesture();
                            }
                            if self.polygon_lasso {
                                ui.label("클릭으로 점 추가 · Enter/더블클릭 완료 · Esc 취소");
                            }
                        }
                        ui.label(format!("드래그로 선택 · {} 해제", self.workspace.layout.shortcuts.label(crate::shortcuts::Action::Deselect)));
                        if self.tool == Tool::Crop {
                            if ui.button("적용 · Enter").clicked() {self.crop();}
                            if ui.button("취소 · Esc").clicked() {self.cancel_crop();}
                            let ratio=self.crop_ratio;
                            if let Some(tab)=self.tab_mut() && let Some(selection)=tab.selection.clone() {
                                let mut position=selection.min;
                                let mut dimensions=[selection.max[0]-selection.min[0],selection.max[1]-selection.min[1]];
                                let size=[tab.editor.document.width,tab.editor.document.height];
                                let mut changed=false;
                                for (axis,label) in [(0,"X "),(1,"Y ")] {changed|=ui.add(egui::DragValue::new(&mut position[axis]).range(0..=size[axis] as i32-1).prefix(label)).changed();}
                                let w=ui.add(egui::DragValue::new(&mut dimensions[0]).range(1..=size[0]).prefix("폭 ")).changed();
                                let h=ui.add(egui::DragValue::new(&mut dimensions[1]).range(1..=size[1]).prefix("높이 ")).changed();
                                if w && let Some(r)=ratio {dimensions[1]=(dimensions[0] as f32/r).round().max(1.) as i32;}
                                if h && let Some(r)=ratio {dimensions[0]=(dimensions[1] as f32*r).round().max(1.) as i32;}
                                if changed||w||h {
                                    tab.crop_before.get_or_insert(Some(selection));
                                    let min=position.map(|v|v as f32);
                                    tab.selection=Some(editing::selection_rectangle(min,[min[0]+dimensions[0] as f32,min[1]+dimensions[1] as f32],size,ratio,false));
                                }
                            }
                        }
                        if self.tool != Tool::Crop {
                            ui.label("Shift 추가 · Alt 빼기");
                        }
                    }
                    Tool::Move => {
                        egui::ComboBox::from_id_salt("move-target")
                            .selected_text(match self.move_target {
                                MoveTarget::Layers => "레이어 이동",
                                MoveTarget::Boundary => "선택 경계 이동",
                                MoveTarget::Pixels => "선택 픽셀 이동",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.move_target,
                                    MoveTarget::Layers,
                                    "레이어 이동",
                                );
                                ui.selectable_value(
                                    &mut self.move_target,
                                    MoveTarget::Boundary,
                                    "선택 경계 이동",
                                );
                                ui.selectable_value(
                                    &mut self.move_target,
                                    MoveTarget::Pixels,
                                    "선택 픽셀 이동",
                                );
                            });
                        if self.move_target == MoveTarget::Pixels {
                            ui.label("Alt+드래그: 선택 픽셀 복제 · 마스크 위치 유지");
                        }
                        ui.label("드래그 이동 · Shift 축 고정 · Esc 취소");
                    }
                    Tool::Hand => {
                        ui.label("드래그로 화면 이동 · Space로 임시 전환");
                    }
                    Tool::Zoom => {
                        ui.label("클릭 확대 · Alt+클릭 축소 · 휠 확대·축소");
                    }
                    Tool::Eyedropper => {
                        ui.checkbox(&mut self.sample_merged, "전체 합성에서 샘플");
                        egui::ComboBox::from_id_salt("sample-size").selected_text(if self.sample_size == 1 { "한 점".into() } else {format!("{}×{} 평균",self.sample_size,self.sample_size)}).show_ui(ui,|ui| {
                            for size in [1,3,5,11,31,101] { ui.selectable_value(&mut self.sample_size,size,if size == 1 {"한 점".into()} else {format!("{size}×{size} 평균")}); }
                        });
                        ui.selectable_value(&mut self.sample_background,false,"전경색");
                        ui.selectable_value(&mut self.sample_background,true,"배경색");
                        ui.label("Alt: 반대 색상에 적용 · 활성 레이어는 원본 픽셀");
                    }
                    Tool::Wand => {
                        ui.add(
                            egui::DragValue::new(&mut self.tolerance)
                                .range(0..=255)
                                .prefix("허용 오차 "),
                        );
                        ui.checkbox(&mut self.contiguous, "인접 영역만");
                        ui.checkbox(&mut self.workspace.layout.selection_smooth,"경계 부드럽게");
                        ui.label("Shift 추가 · Alt 빼기");
                    }
                    Tool::Fill => {
                        ui.color_edit_button_srgba_unmultiplied(&mut self.color);
                        ui.label("클릭하여 레이어·선택 영역 채우기");
                    }
                    Tool::Shape => {
                        egui::ComboBox::from_id_salt("shape_kind")
                            .selected_text(format!("{:?}", self.shape_kind))
                            .show_ui(ui, |ui| {
                                for (kind, label) in [
                                    (ShapeKind::Rectangle, "사각형"),
                                    (ShapeKind::Rounded, "둥근 사각형"),
                                    (ShapeKind::Ellipse, "타원"),
                                    (ShapeKind::Line, "선"),
                                ] {
                                    ui.selectable_value(&mut self.shape_kind, kind, label);
                                }
                            });
                        ui.color_edit_button_srgba_unmultiplied(&mut self.color);
                        ui.color_edit_button_srgba_unmultiplied(&mut self.background);
                        ui.add(
                            egui::DragValue::new(&mut self.shape_stroke)
                                .range(0.0..=100.0)
                                .prefix("선 ")
                                .suffix(" px"),
                        );
                    }
                    Tool::Gradient => {
                        ui.color_edit_button_srgba_unmultiplied(&mut self.color);
                        ui.color_edit_button_srgba_unmultiplied(&mut self.background);
                        gradient_interpolation_ui(ui,&mut self.gradient_interpolation);
                        ui.checkbox(&mut self.gradient_reversed,"반전");
                        ui.label("드래그로 방향과 길이 지정");
                    }
                    Tool::Text => {
                        ui.label("텍스트 클릭: 직접 편집 · 빈 곳 클릭: 새 문단 · Ctrl+Enter 적용 · Esc 취소");
                    }
                    Tool::Pen => {
                        let fill = if self.pen_fill { self.background } else { [0; 4] };
                        let stroke = self.color;
                        let thickness = self.shape_stroke;
                        ui.color_edit_button_srgba_unmultiplied(&mut self.color);
                        ui.checkbox(&mut self.pen_fill, "채우기");
                        if self.pen_fill { ui.color_edit_button_srgba_unmultiplied(&mut self.background); }
                        ui.add(egui::DragValue::new(&mut self.shape_stroke).range(0.1..=100.0).prefix("선 ").suffix(" px"));
                        if let Some(tab) = self.tab_mut() {
                            if ui.selectable_label(tab.pen.drawing, "새 경로").clicked() { tab.editor.finish(); tab.pen.reset(); tab.pen.drawing = true; }
                            if ui.selectable_label(!tab.pen.drawing, "점 편집").clicked() { tab.editor.finish(); tab.pen.reset(); tab.pen.drawing = false; }
                            if !tab.pen.drawing && let Some(layer)=tab.editor.document.active_layer() {tab.pen.svg.ui(ui,layer);}
                            if ui.add_enabled(tab.pen.draft.len() >= 2, egui::Button::new("완료")).clicked() {
                                match tab.pen.apply(&mut tab.editor, crate::paths::Action::Finish, 1.0, fill, stroke, thickness) {
                                    Ok(changed) => tab.render_dirty |= changed,
                                    Err(error) => tab.notes.push(error),
                                }
                            }
                            if ui.button("취소").clicked() {
                                let _ = tab.pen.apply(&mut tab.editor, crate::paths::Action::Cancel, 1.0, fill, stroke, thickness);
                                tab.render_dirty = true;
                            }
                        }
                        ui.label("드래그: 곡선 · 첫 점: 닫기 · Enter 완료 · Ctrl 점 추가 · Alt 핸들 분리");
                    }
                }
            });
        });
        egui::Panel::top("tabs").show(ui, |ui| {
            if self.transforming {
                ui.disable();
            }
            ui.horizontal_wrapped(|ui| {
                let mut select = None;
                let mut close = None;
                let mut drop = None;
                for (index, tab) in self.tabs.iter().enumerate() {
                    let name = format!(
                        "{}{}",
                        tab.title,
                        if tab.editor.dirty() { " •" } else { "" }
                    );
                    let response = ui.selectable_label(index == self.active, name);
                    if response.clicked() {
                        select = Some(index);
                    }
                    if let Some(payload) = response.dnd_release_payload::<LayerDrag>()
                        && payload.tab != tab.id
                    {
                        drop = Some((index, payload));
                    }
                    if ui.small_button("×").on_hover_text("문서 닫기").clicked() {
                        close = Some(tab.id);
                    }
                    ui.separator();
                }
                if ui.small_button("+").on_hover_text("새 문서").clicked() {
                    self.new_dialog = true;
                }
                if let Some(index) = select {
                    self.finish_gesture();
                    self.active = index;
                }
                if let Some((index, payload)) = drop {
                    self.drop_layers(&payload, index, None, ui.input(|i| i.modifiers.alt));
                }
                if let Some(id) = close {
                    self.request_close(CloseTarget::Tab(id), ctx);
                }
            });
        });
    }

    fn tools_ui(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("tools")
            .resizable(false)
            .default_size(if self.workspace.layout.grouped_tools {
                82.0
            } else {
                66.0
            })
            .exact_size(if self.workspace.layout.grouped_tools {
                82.0
            } else {
                66.0
            })
            .show(ui, |ui| {
                if self.transforming {
                    ui.disable();
                }
                egui::ScrollArea::vertical()
                    .id_salt("tool_buttons")
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            if self.workspace.layout.grouped_tools {
                                for group in 0..10 {
                                    let tool = Tool::group_tool(
                                        group,
                                        self.tool,
                                        &self.workspace.layout.remembered,
                                    );
                                    let (_, icon, _) = Tool::entries()
                                        .into_iter()
                                        .find(|(t, _, _)| *t == tool)
                                        .unwrap();
                                    let key = self
                                        .workspace
                                        .layout
                                        .shortcuts
                                        .label(crate::shortcuts::Action::Tool(tool));
                                    ui.horizontal(|ui| {
                                        let response = icons::tool_button(
                                            ui,
                                            icon,
                                            tool.name(),
                                            self.tool == tool,
                                            true,
                                        )
                                        .on_hover_text(format!(
                                            "{} {}\n{}\n오른쪽 클릭/▾: 같은 계열 도구",
                                            tool.name(),
                                            key,
                                            tool.usage()
                                        ));
                                        if response.clicked() {
                                            self.set_tool(tool);
                                        }
                                        let mut selected = None;
                                        response.context_menu(|ui| {
                                            for (member, _, _) in Tool::entries()
                                                .into_iter()
                                                .filter(|(t, _, _)| t.group() == group)
                                            {
                                                if ui
                                                    .selectable_label(
                                                        self.tool == member,
                                                        self.workspace.layout.shortcuts.menu_label(
                                                            crate::shortcuts::Action::Tool(member),
                                                        ),
                                                    )
                                                    .clicked()
                                                {
                                                    selected = Some(member);
                                                    ui.close();
                                                }
                                            }
                                        });
                                        if Tool::entries()
                                            .iter()
                                            .filter(|(t, _, _)| t.group() == group)
                                            .count()
                                            > 1
                                        {
                                            ui.menu_button("▾", |ui| {
                                                for (member, _, _) in Tool::entries()
                                                    .into_iter()
                                                    .filter(|(t, _, _)| t.group() == group)
                                                {
                                                    if ui
                                                        .selectable_label(
                                                            self.tool == member,
                                                            self.workspace
                                                                .layout
                                                                .shortcuts
                                                                .menu_label(
                                                                    crate::shortcuts::Action::Tool(
                                                                        member,
                                                                    ),
                                                                ),
                                                        )
                                                        .clicked()
                                                    {
                                                        selected = Some(member);
                                                        ui.close();
                                                    }
                                                }
                                            });
                                        }
                                        if let Some(tool) = selected {
                                            self.set_tool(tool);
                                        }
                                    });
                                }
                            } else {
                                for (tool, icon, _) in Tool::entries() {
                                    let key = self
                                        .workspace
                                        .layout
                                        .shortcuts
                                        .label(crate::shortcuts::Action::Tool(tool));
                                    let hint = if key == "지정 안 함" {
                                        tool.name().to_string()
                                    } else {
                                        format!("{} ({key})", tool.name())
                                    };
                                    if icons::tool_button(
                                        ui,
                                        icon,
                                        tool.name(),
                                        self.tool == tool,
                                        true,
                                    )
                                    .on_hover_text(format!("{hint}\n{}", tool.usage()))
                                    .clicked()
                                    {
                                        self.set_tool(tool);
                                    }
                                }
                            }
                            ui.separator();
                            ui.color_edit_button_srgba_unmultiplied(&mut self.color);
                            ui.color_edit_button_srgba_unmultiplied(&mut self.background);
                            if ui.small_button("색상…").clicked() {
                                self.colors_panel.show(self.color, self.background);
                            }
                            if ui
                                .small_button("교환")
                                .on_hover_text(
                                    self.workspace
                                        .layout
                                        .shortcuts
                                        .menu_label(crate::shortcuts::Action::SwapColors),
                                )
                                .clicked()
                            {
                                std::mem::swap(&mut self.color, &mut self.background);
                            }
                            if icons::tool_button(ui, Icon::DefaultColors, "기본색", false, true)
                                .on_hover_text(
                                    self.workspace
                                        .layout
                                        .shortcuts
                                        .menu_label(crate::shortcuts::Action::DefaultColors),
                                )
                                .clicked()
                            {
                                self.color = [0, 0, 0, 255];
                                self.background = [255; 4];
                            }
                        });
                    });
            });
    }

    fn new_layer(&mut self) {
        let Some(tab) = self.tab() else {
            return;
        };
        let doc = &tab.editor.document;
        if doc.pixel_count() + u64::from(doc.width) * u64::from(doc.height) > document::MAX_PIXELS {
            self.message = Some((
                "레이어 추가 실패".into(),
                "현재 메모리 예산을 초과합니다.".into(),
            ));
            return;
        }
        self.apply_result("새 레이어", |doc| {
            doc.add_blank_layer(format!("레이어 {}", doc.next_id))
        });
    }

    fn duplicate_layer(&mut self) {
        if let Some(tab) = self.tab() {
            let ids = tab.selected_ids();
            let mut copies = Vec::new();
            self.apply_result("레이어·그룹 복제", |doc| {
                copies = doc.duplicate_selected(&ids)?;
                Ok(())
            });
            if let Some(tab) = self.tab_mut()
                && !copies.is_empty()
            {
                tab.selected_layers = copies;
            }
        }
    }

    fn delete_layer(&mut self) {
        if self
            .tab()
            .is_none_or(|t| t.editor.document.layers.len() <= 1)
        {
            return;
        }
        let ids = self.tab().unwrap().selected_ids();
        if !self.tab().unwrap().editor.document.movable(&ids) {
            self.status = "잠긴 레이어는 삭제할 수 없습니다.".into();
            return;
        }
        self.edit("레이어·그룹 삭제", |doc| doc.delete_selected(&ids));
    }

    fn copy_layers(&mut self) {
        if let Some(tab) = self.tab() {
            match tab.editor.document.selected_document(&tab.selected_ids()) {
                Ok(doc) => {
                    self.layer_clipboard = Some(doc);
                    self.status = "레이어·그룹을 복사했습니다.".into();
                }
                Err(error) => self.message = Some(("레이어 복사 실패".into(), error)),
            }
        }
    }
    fn paste_layers(&mut self) {
        if let Some(source) = self.layer_clipboard.clone() {
            self.apply_result("레이어·그룹 붙여넣기", |doc| {
                doc.import_layers(source)
            });
        }
    }
    fn merge_selected(&mut self) {
        if let Some(tab) = self.tab() {
            let ids = tab.selected_ids();
            self.apply_result("선택 레이어 병합", |doc| {
                editing::merge_selected(doc, &ids)
            });
        }
    }
    fn group_selected(&mut self) {
        if let Some(tab) = self.tab() {
            let ids = tab.selected_ids();
            self.apply_result("레이어 그룹", |doc| doc.group_selected(&ids));
        }
    }
    fn merge_down(&mut self) {
        if let Some(tab) = self.tab() {
            let ids = tab.selected_ids();
            self.apply_result("아래 레이어와 병합", |doc| {
                editing::merge_down(doc, &ids)
            });
        }
    }
    fn drop_layers(
        &mut self,
        payload: &LayerDrag,
        destination: usize,
        target: Option<u64>,
        duplicate: bool,
    ) {
        let Some(source_index) = self.tabs.iter().position(|t| t.id == payload.tab) else {
            return;
        };
        let source = &self.tabs[source_index];
        if source_index != destination
            && !duplicate
            && !source.editor.document.movable(&payload.ids)
        {
            self.message = Some((
                "레이어 이동 실패".into(),
                "잠긴 레이어는 다른 문서로 이동할 수 없습니다. Alt를 눌러 복사할 수 있습니다."
                    .into(),
            ));
            return;
        }
        let source_document = source.editor.document.selected_document(&payload.ids);
        self.finish_gesture();
        self.active = destination;
        if self.tabs[destination].id == payload.tab && !duplicate {
            if let Some(target) = target {
                if payload.ids.iter().any(|id| {
                    self.tabs[destination]
                        .editor
                        .document
                        .descendant_of(target, *id)
                }) {
                    return;
                }
                self.apply_result("레이어 이동", |doc| {
                    doc.reorder_selected(&payload.ids, target, true)
                });
            }
        } else {
            match source_document {
                Ok(source) => {
                    let tab = &mut self.tabs[destination];
                    let start_id = tab.editor.document.next_id;
                    let roots = source
                        .layers
                        .iter()
                        .enumerate()
                        .filter(|(_, l)| l.parent.is_none())
                        .map(|(index, _)| start_id + index as u64)
                        .collect();
                    tab.editor.begin("문서 간 레이어 가져오기");
                    if let Some(target) = target {
                        tab.editor.document.active = target;
                    }
                    match tab.editor.document.import_layers(source) {
                        Ok(()) => {
                            tab.editor.finish();
                            tab.render_dirty = true;
                            tab.selected_layers = roots;
                            tab.edit_mask = false;
                            if source_index != destination && !duplicate {
                                let source = &mut self.tabs[source_index];
                                source.editor.edit("다른 문서로 레이어 이동", |doc| {
                                    doc.delete_selected(&payload.ids)
                                });
                                source.selected_layers = vec![source.editor.document.active];
                                source.edit_mask = false;
                                source.render_dirty = true;
                            }
                        }
                        Err(error) => {
                            tab.editor.cancel();
                            self.message = Some(("레이어 가져오기 실패".into(), error));
                        }
                    }
                }
                Err(error) => self.message = Some(("레이어 가져오기 실패".into(), error)),
            }
        }
    }

    fn navigator_ui(&mut self, ui: &mut egui::Ui) {
        let dpi = ui.ctx().pixels_per_point();
        if let Some(tab) = self.tab_mut() {
            let size = [tab.editor.document.width, tab.editor.document.height];
            crate::navigator::ui(
                ui,
                &mut tab.view,
                tab.canvas_area,
                tab.texture.as_ref(),
                size,
                dpi,
            );
        }
    }

    fn start_rename(&mut self, layer: u64, ctx: &egui::Context) {
        if self.busy.is_some() || self.transforming || self.text_session.is_some() {
            return;
        }
        self.finish_gesture();
        let Some(tab) = self.tab_mut() else {
            return;
        };
        let Some(name) = tab
            .editor
            .document
            .layers
            .iter()
            .find(|l| l.id == layer)
            .map(|l| l.name.clone())
        else {
            return;
        };
        tab.collapsed.retain(|parent| {
            *parent == layer || !tab.editor.document.descendant_of(layer, *parent)
        });
        self.rename = Some(crate::layer_name::Session::new(tab.id, layer, name));
        self.workspace.layout.layers = true;
        if self.canvas_only {
            self.toggle_canvas_only(ctx);
        }
    }
    fn validate_rename(&mut self) {
        if self.rename.as_ref().is_some_and(|session| {
            self.canvas_only
                || !self.workspace.layout.layers
                || self.tab().is_none_or(|tab| {
                    tab.id != session.tab
                        || !tab
                            .editor
                            .document
                            .layers
                            .iter()
                            .any(|l| l.id == session.layer)
                        || tab.collapsed.iter().any(|parent| {
                            *parent != session.layer
                                && tab.editor.document.descendant_of(session.layer, *parent)
                        })
                })
        }) {
            self.rename = None;
        }
    }
    fn finish_rename(&mut self, accept: bool) {
        if let Some(session) = self.rename.take()
            && accept
            && !session.name.trim().is_empty()
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == session.tab)
            && tab
                .editor
                .document
                .layers
                .iter()
                .any(|l| l.id == session.layer && l.name != session.name)
        {
            tab.editor.edit("레이어 이름", |doc| {
                if let Some(layer) = doc.layers.iter_mut().find(|l| l.id == session.layer) {
                    layer.name = session.name;
                }
            });
        }
    }
    fn inline_name_ui(&mut self, ui: &mut egui::Ui, layer: u64) -> bool {
        let tab = self.tab().map(|t| t.id);
        let Some(session) = self
            .rename
            .as_mut()
            .filter(|session| Some(session.tab) == tab && session.layer == layer)
        else {
            return false;
        };
        if let Some(outcome) = session.ui(ui) {
            self.finish_rename(matches!(outcome, crate::layer_name::Outcome::Accept));
        }
        true
    }

    fn panel_action(&mut self, kind: crate::panels::Kind, action: crate::panels::Action) {
        use crate::panels::Action;
        let Some(index) = self
            .workspace
            .layout
            .panels
            .iter()
            .position(|s| s.kind == kind)
        else {
            return;
        };
        match action {
            Action::Collapse => self.workspace.layout.panels[index].collapsed ^= true,
            Action::Float => self.workspace.layout.panels[index].floating ^= true,
            Action::Close => {
                self.workspace.layout.set_panel_visible(kind, false);
                if kind == crate::panels::Kind::Histogram {
                    self.show_histogram = false;
                }
            }
            Action::Up if index > 0 => self.workspace.layout.panels.swap(index, index - 1),
            Action::Down if index + 1 < self.workspace.layout.panels.len() => {
                self.workspace.layout.panels.swap(index, index + 1)
            }
            Action::Before(source) => {
                crate::panels::move_before(&mut self.workspace.layout.panels, source, kind)
            }
            _ => {}
        }
    }
    fn panel_contents(&mut self, ui: &mut egui::Ui, kind: crate::panels::Kind) {
        use crate::panels::Kind;
        let Some(doc) = self.tab().map(|t| t.editor.document.clone()) else {
            return;
        };
        match kind {
            Kind::Layers => self.layers_panel_ui(ui, &doc),
            Kind::Properties => self.layer_properties_ui(ui, &doc),
            Kind::History => self.history_panel_ui(ui),
            Kind::Histogram => self.histogram_panel_ui(ui),
            Kind::Navigator => self.navigator_ui(ui),
            Kind::Colors => {
                self.colors_panel
                    .contents(ui, &mut self.color, &mut self.background);
                if ui.small_button("색상 창 열기…").clicked() {
                    self.colors_panel.show(self.color, self.background);
                }
            }
        }
    }
    fn docked_panels_ui(&mut self, ui: &mut egui::Ui) {
        let states = self.workspace.layout.panels.clone();
        for (index, state) in states.iter().enumerate() {
            if state.floating || !self.workspace.layout.panel_visible(state.kind) {
                continue;
            }
            ui.push_id(state.kind, |ui| {
                ui.separator();
                if let Some(action) =
                    crate::panels::header(ui, state, index == 0, index + 1 == states.len())
                {
                    self.panel_action(state.kind, action);
                }
                let current = self
                    .workspace
                    .layout
                    .panels
                    .iter()
                    .find(|s| s.kind == state.kind)
                    .unwrap();
                if !current.collapsed
                    && !current.floating
                    && self.workspace.layout.panel_visible(state.kind)
                {
                    self.panel_contents(ui, state.kind);
                }
            });
        }
    }
    fn floating_panels_ui(&mut self, ctx: &egui::Context) {
        if self.canvas_only || self.tabs.is_empty() {
            return;
        }
        let states = self.workspace.layout.panels.clone();
        for (index, state) in states.iter().enumerate() {
            if !state.floating || !self.workspace.layout.panel_visible(state.kind) {
                continue;
            }
            let mut open = true;
            let response = egui::Window::new(state.kind.label())
                .id(crate::panels::window_id(ctx, state.kind))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_pos(state.position.map_or(
                    Pos2::new(70. + index as f32 * 24., 90. + index as f32 * 24.),
                    |p| Pos2::new(p[0], p[1]),
                ))
                .default_size(Vec2::new(state.size[0], state.size[1]))
                .min_size(Vec2::new(220., 64.))
                .max_size(Vec2::new(
                    640.,
                    (ctx.content_rect().height() - 80.).max(120.),
                ))
                .show(ctx, |ui| {
                    if self.transforming {
                        ui.disable();
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("floating-body")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(action) = crate::panels::header(
                                ui,
                                state,
                                index == 0,
                                index + 1 == states.len(),
                            ) {
                                self.panel_action(state.kind, action);
                            }
                            if !self
                                .workspace
                                .layout
                                .panels
                                .iter()
                                .find(|s| s.kind == state.kind)
                                .unwrap()
                                .collapsed
                            {
                                self.panel_contents(ui, state.kind);
                            }
                        });
                });
            if !open {
                self.panel_action(state.kind, crate::panels::Action::Close);
            }
            if let Some(response) = response {
                let saved = self
                    .workspace
                    .layout
                    .panels
                    .iter_mut()
                    .find(|s| s.kind == state.kind)
                    .unwrap();
                saved.position = Some([response.response.rect.min.x, response.response.rect.min.y]);
                if !saved.collapsed {
                    let size = response.response.rect.size();
                    saved.size = [size.x.clamp(32., 10000.), size.y.clamp(32., 10000.)];
                }
            }
        }
    }
    fn properties_ui(&mut self, ui: &mut egui::Ui) {
        self.validate_rename();
        self.workspace.layout.mask_viewer = self.mask_view.open;
        self.workspace.layout.histogram = self.show_histogram;
        if !self.workspace.layout.sidebar_visible() {
            return;
        }
        let (min, max) = self.workspace.layout.panel_widths(ui.available_width());
        let panel = if self.workspace.layout.left {
            egui::Panel::left("properties-left")
        } else {
            egui::Panel::right("properties")
        };
        let response = panel
            .resizable(true)
            .default_size(self.workspace.layout.width)
            .size_range(min..=max)
            .show(ui, |ui| {
                if self.transforming {
                    ui.disable();
                }
                egui::ScrollArea::vertical()
                    .id_salt("properties-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let Some(tab) = self.tab() else {
                            ui.heading("속성");
                            ui.label(
                                RichText::new("문서를 열면 속성과 레이어가 표시됩니다.").weak(),
                            );
                            return;
                        };
                        ui.heading("문서");
                        ui.label(format!(
                            "{} × {} px",
                            tab.editor.document.width, tab.editor.document.height
                        ));
                        ui.label(
                            RichText::new(format!(
                                "RGB · {}비트",
                                tab.editor
                                    .document
                                    .layers
                                    .iter()
                                    .map(|l| l.bit_depth)
                                    .max()
                                    .unwrap_or(8)
                            ))
                            .small()
                            .weak(),
                        );
                        self.docked_panels_ui(ui);
                        if let Some(tab) = self.tab()
                            && !tab.notes.is_empty()
                        {
                            ui.separator();
                            for note in &tab.notes {
                                ui.label(
                                    RichText::new(note)
                                        .small()
                                        .color(Color32::from_rgb(231, 190, 120)),
                                );
                            }
                        }
                    });
            });
        let edge = if self.workspace.layout.left {
            response.response.rect.right()
        } else {
            response.response.rect.left()
        };
        if ui.input(|i| {
            i.pointer.any_down()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| (p.x - edge).abs() < 14.)
        }) && (220. ..=640.).contains(&response.response.rect.width())
        {
            self.workspace.layout.width = response.response.rect.width();
        }
    }
    fn layers_panel_ui(&mut self, ui: &mut egui::Ui, doc: &Document) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("+ 새 레이어").clicked() {
                self.new_layer();
            }
            if ui.button("가져오기").clicked() {
                self.open_file(None, self.tab().map(|t| t.id), ui.ctx());
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("layers")
            .max_height(270.0)
            .show(ui, |ui| {
                for (id, depth) in doc.displayed_layers() {
                    let layer = doc.layers.iter().find(|l| l.id == id).unwrap();
                    if self
                        .tab()
                        .unwrap()
                        .collapsed
                        .iter()
                        .any(|parent| *parent != id && doc.descendant_of(id, *parent))
                    {
                        continue;
                    }
                    ui.push_id(layer.id, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(depth.min(8) as f32 * 12.0);
                            if layer.group {
                                let closed = self.tab().unwrap().collapsed.contains(&layer.id);
                                if ui.small_button(if closed { "▸" } else { "▾" }).clicked() {
                                    let tab = self.tab_mut().unwrap();
                                    if closed {
                                        tab.collapsed.remove(&layer.id);
                                    } else {
                                        tab.collapsed.insert(layer.id);
                                    }
                                }
                            } else if layer.adjustment_layer {
                                ui.label(RichText::new("◐").size(26.0))
                                    .on_hover_text("조정 레이어");
                            } else {
                                let texture = self.tab_mut().unwrap().thumbnail(layer, ui.ctx());
                                ui.image((texture, Vec2::splat(28.0)));
                            }
                            let mut visible = layer.visible;
                            if ui
                                .checkbox(&mut visible, "")
                                .on_hover_text("레이어 표시")
                                .changed()
                            {
                                let id = layer.id;
                                self.edit("레이어 표시", |doc| {
                                    if let Some(layer) = doc.layers.iter_mut().find(|l| l.id == id)
                                    {
                                        layer.visible = visible;
                                    }
                                });
                            }
                            if self.inline_name_ui(ui, layer.id) {
                                return;
                            }
                            let label = format!(
                                "{}{}{}{}{}{}{}",
                                if layer.group {
                                    "그룹 "
                                } else if layer.adjustment_layer {
                                    "조정 "
                                } else {
                                    ""
                                },
                                if layer.clipped { "↳ " } else { "" },
                                layer.name,
                                if doc.is_locked(layer.id) {
                                    " · 잠금"
                                } else {
                                    ""
                                },
                                if layer.mask.is_some() {
                                    if layer.mask_enabled {
                                        " · M"
                                    } else {
                                        " · M 끔"
                                    }
                                } else {
                                    ""
                                },
                                if layer.adjustments != Default::default() {
                                    " · 보정"
                                } else {
                                    ""
                                },
                                if layer.effects.enabled() {
                                    " · fx"
                                } else {
                                    ""
                                }
                            );
                            let tab = self.tab().unwrap();
                            let selected = layer.id == doc.active
                                || (tab.selected_layers.contains(&doc.active)
                                    && tab.selected_layers.contains(&layer.id));
                            let response = ui
                                .add(
                                    egui::Button::new(label)
                                        .truncate()
                                        .selected(selected)
                                        .sense(Sense::click_and_drag()),
                                )
                                .on_hover_text(format!(
                                    "불투명도 {:.0}% · {:?}\n{}{}{}",
                                    layer.opacity * 100.0,
                                    layer.blend,
                                    if layer.mask.is_some() {
                                        "마스크 있음 "
                                    } else {
                                        ""
                                    },
                                    if layer.adjustments != Default::default() {
                                        "보정 적용 "
                                    } else {
                                        ""
                                    },
                                    if layer.effects.enabled() {
                                        "효과 적용"
                                    } else {
                                        ""
                                    }
                                ));
                            if (response.clicked() && !response.double_clicked())
                                || (response.secondary_clicked() && !selected)
                            {
                                self.finish_gesture();
                                let modifiers = ui.input(|i| i.modifiers);
                                if let Some(tab) = self.tab_mut() {
                                    if modifiers.shift {
                                        let displayed: Vec<u64> = doc
                                            .displayed_layers()
                                            .into_iter()
                                            .filter(|(id, _)| {
                                                !tab.collapsed.iter().any(|parent| {
                                                    *id != *parent
                                                        && doc.descendant_of(*id, *parent)
                                                })
                                            })
                                            .map(|p| p.0)
                                            .collect();
                                        let a = displayed
                                            .iter()
                                            .position(|id| *id == doc.active)
                                            .unwrap_or(0);
                                        let b = displayed
                                            .iter()
                                            .position(|id| *id == layer.id)
                                            .unwrap();
                                        tab.selected_layers =
                                            displayed[a.min(b)..=a.max(b)].to_vec();
                                    } else if modifiers.command || modifiers.ctrl {
                                        let mut selected = tab.selected_ids();
                                        if selected.contains(&layer.id) && selected.len() > 1 {
                                            selected.retain(|id| *id != layer.id);
                                        } else if !selected.contains(&layer.id) {
                                            selected.push(layer.id);
                                        }
                                        tab.selected_layers = selected;
                                    } else {
                                        tab.selected_layers = vec![layer.id];
                                    }
                                    tab.editor.document.active = layer.id;
                                    if !tab.selected_layers.contains(&layer.id) {
                                        tab.editor.document.active =
                                            *tab.selected_layers.last().unwrap();
                                    }
                                    tab.edit_mask = false;
                                }
                            }
                            if response.drag_started() {
                                let tab = self.tab_mut().unwrap();
                                if !selected {
                                    tab.editor.document.active = layer.id;
                                    tab.selected_layers = vec![layer.id];
                                }
                                response.dnd_set_drag_payload(LayerDrag {
                                    tab: tab.id,
                                    ids: tab.selected_ids(),
                                });
                            }
                            if let Some(payload) = response.dnd_release_payload::<LayerDrag>() {
                                self.drop_layers(
                                    &payload,
                                    self.active,
                                    Some(layer.id),
                                    ui.input(|i| i.modifiers.alt),
                                );
                            }
                            response.context_menu(|ui| {
                                if ui.button("레이어 이름 변경").clicked() {
                                    self.start_rename(layer.id, ui.ctx());
                                    ui.close();
                                }
                                if ui.button("선택 레이어 복제").clicked() {
                                    self.duplicate_layer();
                                    ui.close();
                                }
                                if ui.button("레이어·그룹 복사").clicked() {
                                    self.copy_layers();
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        self.layer_clipboard.is_some(),
                                        egui::Button::new("레이어·그룹 붙여넣기"),
                                    )
                                    .clicked()
                                {
                                    self.paste_layers();
                                    ui.close();
                                }
                                if ui.button("선택 레이어 병합").clicked() {
                                    self.merge_selected();
                                    ui.close();
                                }
                                if ui.button("아래 레이어와 병합").clicked() {
                                    self.merge_down();
                                    ui.close();
                                }
                                if ui.button("그룹으로 묶기").clicked() {
                                    self.group_selected();
                                    ui.close();
                                }
                                if ui.button("선택 레이어 삭제").clicked() {
                                    self.delete_layer();
                                    ui.close();
                                }
                            });
                            if response.double_clicked() {
                                self.start_rename(layer.id, ui.ctx());
                            }
                            if layer.locked {
                                ui.label(RichText::new("잠금").small().weak());
                            }
                        });
                    });
                }
            });
    }
    fn layer_properties_ui(&mut self, ui: &mut egui::Ui, doc: &Document) {
        if let Some(layer) = self
            .tab()
            .and_then(|t| t.editor.document.active_layer())
            .cloned()
        {
            ui.separator();
            ui.label(RichText::new(&layer.name).strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button("그룹으로 묶기").clicked() {
                    self.group_selected();
                }
                if let Some(parent) = layer.parent
                    && ui.button("그룹에서 꺼내기").clicked()
                {
                    let grandparent = doc
                        .layers
                        .iter()
                        .find(|l| l.id == parent)
                        .and_then(|l| l.parent);
                    self.edit("그룹에서 꺼내기", |doc| {
                        doc.active_layer_mut().unwrap().parent = grandparent
                    });
                }
            });
            let mut parent = layer.parent;
            egui::ComboBox::from_id_salt("parent-group")
                .selected_text(
                    parent
                        .and_then(|p| doc.layers.iter().find(|l| l.id == p))
                        .map_or("최상위", |l| l.name.as_str()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut parent, None, "최상위");
                    for group in doc
                        .layers
                        .iter()
                        .filter(|l| l.group && !doc.descendant_of(l.id, layer.id))
                    {
                        ui.selectable_value(&mut parent, Some(group.id), &group.name);
                    }
                });
            if parent != layer.parent {
                self.edit("그룹 이동", |doc| {
                    doc.active_layer_mut().unwrap().parent = parent
                });
            }
            if layer.group {
                let mut pass = layer.pass_through;
                if ui.checkbox(&mut pass, "그룹 통과 합성").changed() {
                    self.edit("그룹 합성", |doc| {
                        doc.active_layer_mut().unwrap().pass_through = pass
                    });
                }
            }
            let mut clipped = layer.clipped;
            if ui.checkbox(&mut clipped, "아래 레이어에 클리핑").changed() {
                self.edit("클리핑 마스크", |doc| {
                    doc.active_layer_mut().unwrap().clipped = clipped
                });
            }
            let mut locked = layer.locked;
            if ui.checkbox(&mut locked, "편집 잠금").changed() {
                self.edit("레이어 잠금", |doc| {
                    doc.active_layer_mut().unwrap().locked = locked
                });
            }
            let mut opacity = layer.opacity;
            let response = ui.add(
                crate::numeric_slider::Slider::new(&mut opacity, 0.0..=1.0)
                    .text("불투명도")
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
            if let Some(tab) = self.tab_mut() {
                if response.changed() {
                    tab.editor.begin("불투명도");
                    tab.editor.document.active_layer_mut().unwrap().opacity = opacity;
                    tab.render_dirty = true;
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    tab.editor.finish();
                }
            }
            let mut blend = layer.blend;
            egui::ComboBox::from_id_salt("blend")
                .selected_text(blend.name())
                .show_ui(ui, |ui| {
                    for mode in BlendMode::ALL {
                        ui.selectable_value(&mut blend, mode, mode.name());
                    }
                });
            if blend != layer.blend {
                self.edit("블렌드 모드", |doc| {
                    doc.active_layer_mut().unwrap().blend = blend
                });
            }
            if layer.raw_source.is_some()
                && ui
                    .add_enabled(
                        self.command_reason(crate::commands::Command::RawDevelop)
                            .is_none(),
                        egui::Button::new("RAW 현상…"),
                    )
                    .clicked()
            {
                self.execute_command(crate::commands::Command::RawDevelop, ui.ctx());
            }
            if ui.button("보정·필터…").clicked() {
                self.open_adjustments();
            }
            ui.collapsing("레이어 효과", |ui| {
                let mut effects = layer.effects.clone();
                let mut changed = false;
                let mut finished = false;
                for (label, effect) in [
                    ("외곽선", &mut effects.stroke),
                    ("드롭 섀도", &mut effects.shadow),
                    ("색상 오버레이", &mut effects.overlay),
                    ("내부 그림자", &mut effects.inner_shadow),
                    ("외부 글로우", &mut effects.outer_glow),
                    ("내부 글로우", &mut effects.inner_glow),
                ] {
                    let r = ui.checkbox(&mut effect.enabled, label);
                    changed |= r.changed();
                    finished |= r.changed();
                    if effect.enabled {
                        ui.indent(label, |ui| {
                            let r = ui.color_edit_button_srgba_unmultiplied(&mut effect.color);
                            changed |= r.changed();
                            finished |= r.changed();
                            let r = ui.add(
                                crate::numeric_slider::Slider::new(&mut effect.opacity, 0.0..=1.0)
                                    .text("불투명도"),
                            );
                            changed |= r.changed();
                            finished |= r.drag_stopped() || (r.changed() && !r.dragged());
                            if label != "색상 오버레이" {
                                let r = ui.add(
                                    crate::numeric_slider::Slider::new(
                                        &mut effect.radius,
                                        0.0..=100.0,
                                    )
                                    .text("반경"),
                                );
                                changed |= r.changed();
                                finished |= r.drag_stopped() || (r.changed() && !r.dragged());
                            }
                            if label.contains("그림자") || label == "드롭 섀도" {
                                for (value, axis) in effect.offset.iter_mut().zip(["X ", "Y "]) {
                                    let r = ui.add(
                                        egui::DragValue::new(value).range(-200..=200).prefix(axis),
                                    );
                                    changed |= r.changed();
                                    finished |= r.drag_stopped() || (r.changed() && !r.dragged());
                                }
                            }
                        });
                    }
                }
                if let Some(tab) = self.tab_mut() {
                    if changed {
                        tab.editor.begin("레이어 효과");
                        tab.editor.document.active_layer_mut().unwrap().effects = effects;
                        tab.render_dirty = true;
                    }
                    if finished {
                        tab.editor.finish();
                    }
                }
            });
            if let Some(content) = &layer.content
                && ui
                    .add_enabled(!doc.is_locked(layer.id), egui::Button::new("요소 편집…"))
                    .clicked()
            {
                if matches!(content, Content::Text { .. }) {
                    self.text_session = self
                        .tab()
                        .and_then(|t| crate::text_canvas::Session::existing(t.id, &layer));
                } else {
                    self.svg_panel = Default::default();
                    self.content_dialog = self
                        .tab()
                        .map(|t| (t.id, Some(layer.id), layer.offset, content.clone()));
                }
            }
            ui.horizontal_wrapped(|ui| {
                if layer.mask.is_none() {
                    if ui.button("마스크 추가").clicked() {
                        let selection = self.tab().unwrap().selection.clone();
                        self.edit("레이어 마스크", |doc| {
                            let document_size = [doc.width, doc.height];
                            let l = doc.active_layer_mut().unwrap();
                            let size = if l.canvas_space() {
                                document_size
                            } else {
                                [l.pixels.width(), l.pixels.height()]
                            };
                            l.mask = Some(std::sync::Arc::new(image::GrayImage::from_fn(
                                size[0],
                                size[1],
                                |x, y| {
                                    image::Luma([(selection.as_ref().map_or(1.0, |s| {
                                        let point = l.transform.to_document(
                                            [x as f32 + 0.5, y as f32 + 0.5],
                                            l.offset,
                                            size,
                                        );
                                        s.coverage(point[0].floor() as i32, point[1].floor() as i32)
                                    }) * 255.0)
                                        .round()
                                        as u8])
                                },
                            )));
                        });
                    }
                } else {
                    let mut linked = crate::masks::linked(&layer);
                    if ui
                        .add_enabled(
                            !doc.is_locked(layer.id),
                            egui::Checkbox::new(&mut linked, "레이어와 연결"),
                        )
                        .changed()
                    {
                        self.apply_result("마스크 연결", |doc| {
                            if doc.is_locked(doc.active) {
                                return Err("잠긴 마스크입니다.".into());
                            }
                            crate::masks::set_linked(doc.active_layer_mut().unwrap(), linked)
                        });
                    }
                    if ui
                        .add_enabled(
                            !doc.is_locked(layer.id),
                            egui::Button::new("마스크만 변형…"),
                        )
                        .clicked()
                    {
                        if let Some(tab) = self.tab_mut() {
                            tab.edit_mask = true;
                        }
                        self.start_transform();
                    }
                    if ui
                        .add_enabled(
                            !doc.is_locked(layer.id),
                            egui::Button::new("마스크를 캔버스까지 확장"),
                        )
                        .clicked()
                    {
                        self.apply_result("마스크 확장", |doc| {
                            if doc.is_locked(doc.active) {
                                return Err("잠긴 마스크입니다.".into());
                            }
                            let size = [doc.width, doc.height];
                            crate::masks::expand_document(
                                doc.active_layer_mut().unwrap(),
                                [0., 0.],
                                size.map(|v| v as f32),
                            )?;
                            Ok(())
                        });
                    }
                    let mut enabled = layer.mask_enabled;
                    if ui.checkbox(&mut enabled, "마스크 적용").changed() {
                        self.edit("마스크 적용", |doc| {
                            doc.active_layer_mut().unwrap().mask_enabled = enabled
                        });
                    }
                    if ui.button("반전").clicked() {
                        self.edit("마스크 반전", |doc| {
                            let mask = doc.active_layer_mut().unwrap().mask.as_mut().unwrap();
                            for p in std::sync::Arc::make_mut(mask).pixels_mut() {
                                p[0] = 255 - p[0];
                            }
                        });
                    }
                    ui.add(
                        crate::numeric_slider::Slider::new(&mut self.mask_feather, 0.0..=100.0)
                            .text("페더 px"),
                    );
                    if ui
                        .add_enabled(
                            !doc.is_locked(layer.id),
                            egui::Button::new("마스크 페더 적용"),
                        )
                        .clicked()
                    {
                        let sigma = self.mask_feather;
                        self.apply_result("마스크 페더", |doc| {
                            if doc.is_locked(doc.active) {
                                return Err("잠긴 마스크입니다.".into());
                            }
                            crate::masks::feather(doc.active_layer_mut().unwrap(), sigma)
                        });
                    }
                }
            });
            if layer.mask.is_some()
                && let Some(tab) = self.tab_mut()
            {
                ui.checkbox(&mut tab.edit_mask, "마스크에 그리기");
                ui.checkbox(&mut self.mask_view.open, "마스크 원본 보기");
                self.workspace.layout.mask_overlay.ui(ui);
            }
            ui.collapsing("변형", |ui| {
                if ui
                    .add_enabled(
                        !doc.is_locked(layer.id) && !layer.canvas_space(),
                        egui::Button::new("캔버스 핸들로 변형…"),
                    )
                    .clicked()
                {
                    if let Some(tab) = self.tab_mut() {
                        tab.edit_mask = false;
                    }
                    self.start_transform();
                }
                let mut transform = layer.transform;
                let mut offset = layer.offset;
                let mut position = std::array::from_fn::<_, 2, _>(|axis| {
                    offset[axis] as f32 + transform.translation[axis]
                });
                let mut changed = false;
                let mut finished = false;
                ui.add_enabled_ui(!doc.is_locked(layer.id) && !layer.canvas_space(), |ui| {
                    for (value, label) in transform.scale.iter_mut().zip(["가로 배율", "세로 배율"])
                    {
                        let r = ui.add(
                            egui::DragValue::new(value)
                                .speed(0.01)
                                .range(-100.0..=100.0)
                                .prefix(label),
                        );
                        changed |= r.changed();
                        finished |= r.drag_stopped() || (r.changed() && !r.dragged());
                        if value.abs() < 0.01 {
                            *value = 0.01;
                        }
                    }
                    let r = ui.add(
                        egui::DragValue::new(&mut transform.angle)
                            .speed(1.0)
                            .range(-360.0..=360.0)
                            .prefix("회전 ")
                            .suffix("°"),
                    );
                    changed |= r.changed();
                    finished |= r.drag_stopped() || (r.changed() && !r.dragged());
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("가로 뒤집기").clicked() {
                            transform.scale[0] *= -1.0;
                            changed = true;
                            finished = true;
                        }
                        if ui.button("세로 뒤집기").clicked() {
                            transform.scale[1] *= -1.0;
                            changed = true;
                            finished = true;
                        }
                    });
                    for (value, label) in position.iter_mut().zip(["X ", "Y "]) {
                        let r = ui.add(
                            egui::DragValue::new(value)
                                .speed(0.1)
                                .range(-1e9..=1e9)
                                .prefix(label),
                        );
                        changed |= r.changed();
                        finished |= r.drag_stopped() || (r.changed() && !r.dragged());
                    }
                });
                if let Some(tab) = self.tab_mut() {
                    if changed {
                        offset = position.map(|p| p.round() as i32);
                        transform.translation =
                            std::array::from_fn(|axis| position[axis] - offset[axis] as f32);
                        tab.editor.begin("레이어 변형");
                        let id = tab.editor.document.active;
                        tab.editor.document.move_layer(id, offset);
                        let l = tab.editor.document.active_layer_mut().unwrap();
                        if !l.canvas_space() {
                            l.transform = transform;
                        }
                        tab.render_dirty = true;
                    }
                    if finished {
                        tab.editor.finish();
                    }
                }
            });
            ui.label(format!(
                "위치: {:.2}, {:.2}",
                layer.offset[0] as f32 + layer.transform.translation[0],
                layer.offset[1] as f32 + layer.transform.translation[1]
            ));
            ui.horizontal_wrapped(|ui| {
                if ui.button("복제").clicked() {
                    self.duplicate_layer();
                }
                if ui.button("이름").clicked() {
                    self.start_rename(layer.id, ui.ctx());
                }
                if ui
                    .add_enabled(doc.layers.len() > 1, egui::Button::new("삭제"))
                    .clicked()
                {
                    self.delete_layer();
                }
            });
            ui.horizontal_wrapped(|ui| {
                let index = doc.layers.iter().position(|l| l.id == layer.id).unwrap();
                if ui
                    .add_enabled(index + 1 < doc.layers.len(), egui::Button::new("위로"))
                    .clicked()
                {
                    self.edit("레이어 순서", |doc| doc.layers.swap(index, index + 1));
                }
                if ui
                    .add_enabled(index > 0, egui::Button::new("아래로"))
                    .clicked()
                {
                    self.edit("레이어 순서", |doc| doc.layers.swap(index, index - 1));
                }
            });
        }
    }
    fn histogram_panel_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(counts) = self.tab().and_then(|t| t.histogram.as_ref()) {
            let maximum = *counts.iter().max().unwrap_or(&1);
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 80.0), Sense::hover());
            for (i, count) in counts.iter().enumerate() {
                let x = rect.left() + rect.width() * i as f32 / 256.0;
                ui.painter().line_segment(
                    [
                        Pos2::new(x, rect.bottom()),
                        Pos2::new(
                            x,
                            rect.bottom() - rect.height() * *count as f32 / maximum.max(1) as f32,
                        ),
                    ],
                    Stroke::new(1.0, ACCENT),
                );
            }
        }
    }
    fn history_panel_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(tab) = self.tab() {
            egui::ScrollArea::vertical()
                .id_salt("history")
                .max_height(130.)
                .show(ui, |ui| {
                    ui.label(RichText::new("문서 시작").weak());
                    for label in tab.editor.history().rev().take(8) {
                        ui.label(label);
                    }
                });
        }
    }

    fn layout_menu(&mut self, ui: &mut egui::Ui, command: crate::commands::Command) {
        let Some(tab) = self.tab() else {
            return;
        };
        let mut layout = tab.editor.document.layout.clone();
        let original = layout.clone();
        let size = [tab.editor.document.width, tab.editor.document.height];
        ui.checkbox(&mut layout.rulers, "눈금자");
        ui.checkbox(&mut layout.guides_visible, "가이드 표시");
        ui.checkbox(&mut layout.grid_visible, "레이아웃 격자");
        ui.separator();
        if command == crate::commands::Command::LayoutSettings {
            for axis in 0..2 {
                ui.add(
                    egui::DragValue::new(&mut layout.spacing[axis])
                        .range(1.0..=1e6)
                        .prefix(if axis == 0 {
                            "가로 간격 "
                        } else {
                            "세로 간격 "
                        })
                        .suffix(" px"),
                );
            }
            ui.add(
                egui::DragValue::new(&mut layout.subdivisions)
                    .range(1..=16)
                    .prefix("세분화 "),
            );
            ui.separator();
            ui.checkbox(&mut layout.snap, "스냅 사용");
            ui.checkbox(&mut layout.snap_guides, "가이드에 스냅");
            ui.checkbox(&mut layout.snap_grid, "격자에 스냅");
            ui.checkbox(&mut layout.snap_layers, "다른 레이어 가장자리·중심");
            ui.checkbox(&mut layout.snap_document, "문서 가장자리·중심");
            ui.label("이동 중 Shift: 가로·세로 축 고정");
        } else {
            ui.label("눈금자에서 끌어 생성 · 이동 도구로 끌어 조정");
            ui.label("캔버스 밖으로 끌면 삭제됩니다.");
            ui.horizontal_wrapped(|ui| {
                for (axis, label) in [(0, "세로 가이드 추가"), (1, "가로 가이드 추가")]
                {
                    if ui
                        .add_enabled(layout.guides.len() < 4096, egui::Button::new(label))
                        .clicked()
                    {
                        layout.guides.push(crate::guides::Guide {
                            axis,
                            position: size[axis] as f32 / 2.0,
                        });
                    }
                }
            });
            if ui.button("가이드 모두 삭제").clicked() {
                layout.guides.clear();
            }
            let mut remove = None;
            let row_height = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
            egui::ScrollArea::vertical()
                .id_salt("guide-option-rows")
                .max_height(220.)
                .show_rows(ui, row_height, layout.guides.len(), |ui, rows| {
                    for index in rows {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                let guide = &mut layout.guides[index];
                                crate::numeric_slider::labeled_drag(
                                    ui,
                                    if guide.axis == 0 { "세로" } else { "가로" },
                                    &mut guide.position,
                                    -1e9..=1e9,
                                    1.,
                                    " px",
                                );
                                if ui.small_button("삭제").clicked() {
                                    remove = Some(index);
                                }
                            });
                        });
                    }
                });
            if let Some(index) = remove {
                layout.guides.remove(index);
            }
        }
        if layout != original
            && let Some(tab) = self.tab_mut()
        {
            tab.editor.begin("가이드·격자 설정");
            tab.editor.document.layout = layout;
            tab.render_dirty = true;
        }
    }

    fn canvas_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(if self.canvas_only {Color32::BLACK}else{CANVAS_BACKGROUND}))
            .show(ui, |ui| {
                if self.tabs.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space((ui.available_height() * 0.29).max(30.0));
                        ui.label(RichText::new("사진으로 시작하세요").size(30.0).strong());
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("이미지를 이곳에 놓거나 파일을 열어 작업을 시작하세요.")
                                .weak(),
                        );
                        ui.add_space(22.0);
                        if ui
                            .add_sized(
                                [190.0, 42.0],
                                egui::Button::new("이미지 열기")
                                    .fill(Color32::from_rgb(48, 94, 161)),
                            )
                            .clicked()
                        {
                            self.open_file(None, None, ctx);
                        }
                        if ui
                            .add_sized([190.0, 38.0], egui::Button::new("새 문서 만들기"))
                            .clicked()
                        {
                            self.new_dialog = true;
                        }
                    });
                    return;
                }
                let (full_area, response) =
                    ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                let rulers = !self.canvas_only&&self.tab().is_some_and(|t| t.editor.document.layout.rulers);
                let area = if rulers {
                    Rect::from_min_max(full_area.min + Vec2::splat(20.0), full_area.max)
                } else {
                    full_area
                };
                let pixels_per_point = ctx.pixels_per_point();
                let tool = self.tool;
                let move_target = self.move_target;
                let transforming = self.transforming;
                let brush = Brush {
                    radius: self.brush_size / 2.0,
                    opacity: self.brush_opacity,
                    hardness: self.brush_hardness,
                    color: self.color,
                    erase: tool == Tool::Eraser,
                    mask: self.tab().is_some_and(|t| t.edit_mask),
                };
                let sample_merged = self.sample_merged;
                let sample_size = self.sample_size;
                let sample_background = self.sample_background;
                let gradient_interpolation = self.gradient_interpolation;
                let gradient_reversed = self.gradient_reversed;
                let show_grid = self.show_grid;
                let mask_overlay_settings=self.workspace.layout.mask_overlay.clone();
                let smoothing = self.brush_smoothing;
                let clone_aligned = self.clone_aligned;
                let polygon_lasso = self.polygon_lasso;
                let selection_smooth=self.workspace.layout.selection_smooth;
                let display_sampling=self.workspace.layout.display_sampling;
                let crop_ratio = self.crop_ratio;
                let tolerance = self.tolerance;
                let contiguous = self.contiguous;
                let shape_kind = self.shape_kind;
                let shape_stroke = self.shape_stroke;
                let pen_fill = self.pen_fill;
                let pen_enabled = !self.rename_pointer_blocked && !self.command_panel.open && self.command_options.is_none() && !self.new_dialog && !self.export_dialog && self.message.is_none() && self.rename.is_none() && self.close_target.is_none() && self.size_dialog.is_none() && self.raw_dialog.is_none() && self.adjust_dialog.is_none() && self.content_dialog.is_none() && self.psd_dialog.is_none() && !self.recovery_dialog && (self.canvas_only || !self.colors_panel.open) && (self.canvas_only || !self.shortcuts_panel.open) && !self.about_panel.open && self.external_dialog.is_none();
                let colors = [self.color, self.background];
                let modifiers = ctx.input(|i| i.modifiers);
                let mut created_content = None;
                let mut pending_text = None;
                let text_editing = self.text_session.is_some();
                let mut sampled = None;
                let mut move_error = None;
                let tab = self.tab_mut().unwrap();
                tab.apply_display_sampling(display_sampling);
                let size = [tab.editor.document.width, tab.editor.document.height];
                tab.canvas_area=Some(area);
                if tab.view.fit {
                    tab.view.fit_to(area, size, pixels_per_point);
                }
                let pointer = ctx.input(|i| i.pointer.interact_pos());
                let space =
                    ctx.input(|i| i.key_down(Key::Space)) && !ctx.egui_wants_keyboard_input();
                if response.hovered()
                    && pen_enabled
                    && let Some(pointer) = pointer
                {
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        tab.view.zoom_at(area, pointer, (scroll * 0.003).exp());
                    }
                }
                let image_rect = tab.view.image_rect(area, size, pixels_per_point);
                let events = ctx.input(|i| i.raw.events.clone());
                let press = events.iter().enumerate().find_map(|(index, event)| {
                    if let egui::Event::PointerButton {
                        pos,
                        button,
                        pressed: true,
                        ..
                    } = event
                        && matches!(button, PointerButton::Primary | PointerButton::Middle)
                        && response.hovered()
                        && !text_editing
                        && pen_enabled
                        && full_area.contains(*pos)
                    {
                        Some((index, *pos, *button))
                    } else {
                        None
                    }
                });
                let down = ctx.input(|i| i.pointer.primary_down() || i.pointer.middle_down());
                let released = ctx.input(|i| {
                    i.pointer.button_released(PointerButton::Primary)
                        || i.pointer.button_released(PointerButton::Middle)
                });
                if let Some((press_index, pointer, button)) = press {
                    let point = tab.view.document_pos(area, size, pixels_per_point, pointer);
                    let new_axis = if rulers && !area.contains(pointer) {
                        if pointer.x < area.left() && pointer.y >= area.top() {
                            Some(0)
                        } else if pointer.y < area.top() && pointer.x >= area.left() {
                            Some(1)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let existing = if tool == Tool::Move
                        && move_target == MoveTarget::Layers
                        && tab.editor.document.layout.guides_visible
                        && !space
                        && !modifiers.alt
                    {
                        tab.editor
                            .document
                            .layout
                            .guides
                            .iter()
                            .enumerate()
                            .filter_map(|(i, g)| {
                                let distance = (point[g.axis] - g.position).abs() * tab.view.zoom
                                    / pixels_per_point;
                                (distance <= 4.0).then_some((i, distance))
                            })
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                            .map(|(i, _)| i)
                    } else {
                        None
                    };
                    if transforming && button == PointerButton::Primary && !space {
                        if let Some(mut batch)=tab.transform_frame.clone() {
                            batch.capture(&tab.editor.document);
                            let scale=tab.view.zoom/pixels_per_point;
                            let handles=batch_handles(&batch,image_rect,scale);
                            let handle=handles.iter().filter_map(|(h,p)|{let d=p.distance(pointer);(d<=9.0).then_some((*h,d))}).min_by(|a,b|a.1.total_cmp(&b.1)).map(|p|p.0);
                            let inside=crate::matrix::unit_quad(batch.quad).and_then(crate::matrix::inverse).map(|m|crate::matrix::point(m,point)).is_some_and(|p|p.iter().all(|v|*v>=0.0&&*v<=1.0));
                            if let Some(handle)=handle.or_else(||inside.then_some(crate::transform::Handle::Move)) && let Some(layer)=tab.editor.document.active_layer() {
                                tab.gesture=Some(Gesture::Transform{initial:layer.transform,offset:layer.offset,size:[layer.pixels.width(),layer.pixels.height()],handle,start:point,batch:Box::new(batch)});
                            }
                        }
                    } else if button == PointerButton::Primary
                        && (new_axis.is_some() || existing.is_some())
                        && !space
                    {
                        if let Some(index) = existing.filter(|_| new_axis.is_none()) {
                            tab.editor.begin("가이드 이동");
                            tab.gesture = Some(Gesture::Guide { index });
                        } else if let Some(axis) = new_axis
                            && tab.editor.document.layout.guides.len() < 4096
                        {
                            tab.editor.begin("가이드 추가");
                            let index = tab.editor.document.layout.guides.len();
                            tab.editor
                                .document
                                .layout
                                .guides
                                .push(crate::guides::Guide {
                                    axis,
                                    position: point[axis],
                                });
                            tab.gesture = Some(Gesture::Guide { index });
                        }
                    } else if area.contains(pointer)
                        && (space || tool == Tool::Hand || button == PointerButton::Middle)
                    {
                        tab.gesture = Some(Gesture::Pan { last: pointer });
                    } else if area.contains(pointer)
                        && (image_rect.contains(pointer) || tool == Tool::Move)
                    {
                        match tool {
                            Tool::Brush | Tool::Eraser
                                if tab.editor.document.active_layer().is_some_and(|l| {
                                    !tab.editor.document.is_locked(l.id)
                                        && ((!l.canvas_space() && l.content.is_none()) || tab.edit_mask)
                                }) =>
                            {
                                tab.editor.begin(if tool == Tool::Brush {
                                    "브러시"
                                } else {
                                    "지우개"
                                });
                                let from = if modifiers.shift {
                                    tab.last_paint
                                        .filter(|(id, mask, _)| {
                                            *id == tab.editor.document.active && *mask == brush.mask
                                        })
                                        .map_or(point, |(_, _, p)| p)
                                } else {
                                    point
                                };
                                if let Err(error) = crate::masks::paint(
                                    tab.editor.document.active_layer_mut().unwrap(),
                                    from,
                                    point,
                                    &brush,
                                    tab.selection.as_ref(),
                                ) {move_error=Some(error);}
                                tab.damage_stroke(from, point, brush.radius);
                                tab.gesture = Some(Gesture::Paint { last: point });
                            }
                            Tool::Move if move_target != MoveTarget::Layers => {
                                if let Some(previous) = tab.selection.clone() {
                                    if move_target == MoveTarget::Boundary {
                                        tab.gesture = Some(Gesture::Boundary { start: point, previous });
                                    } else if tab.edit_mask {
                                        move_error = Some("픽셀 이동 전에 이미지 편집 대상을 선택하세요. 마스크는 브러시·채우기·블러로 편집할 수 있습니다.".into());
                                    } else {
                                        match editing::PixelMove::new(&tab.editor.document, &previous, modifiers.alt) {
                                            Ok(source) => tab.gesture = Some(Gesture::Pixels {
                                                start: point, previous, source: Box::new(source), last_delta: [0, 0],
                                            }),
                                            Err(error) => move_error = Some(error),
                                        }
                                    }
                                }
                            }
                            Tool::Move if tab.editor.document.movable(&tab.selected_ids()) => {
                                let offsets = tab
                                    .selected_ids()
                                    .into_iter()
                                    .map(|id| {
                                        (
                                            id,
                                            tab.editor
                                                .document
                                                .layers
                                                .iter()
                                                .find(|l| l.id == id)
                                                .unwrap()
                                                .offset,
                                        )
                                    })
                                    .collect();
                                tab.gesture = Some(Gesture::Move {
                                    start: point,
                                    offsets,
                                    bounds: crate::guides::bounds(
                                        &tab.editor.document,
                                        &tab.selected_ids(),
                                    ),
                                });
                            }
                            Tool::Select | Tool::Ellipse | Tool::Crop => {
                                if tool==Tool::Crop && let Some(selection)=tab.selection.clone() {
                                    tab.crop_before.get_or_insert_with(||Some(selection.clone()));
                                    let handles=crate::crop_handles::handles(&selection);
                                    let scale=tab.view.zoom/pixels_per_point;
                                    let handle=handles.into_iter().filter_map(|(h,p)|{let distance=(image_rect.min+Vec2::new(p[0],p[1])*scale).distance(pointer);(distance<=9.0).then_some((h,distance))}).min_by(|a,b|a.1.total_cmp(&b.1)).map(|p|p.0).or_else(||selection.contains(point[0].floor() as i32,point[1].floor() as i32).then_some(crate::transform::Handle::Move));
                                    if let Some(handle)=handle {tab.gesture=Some(Gesture::CropBox{initial:selection,handle,start:point});}
                                    else {let previous=tab.selection.take();tab.gesture=Some(Gesture::Select{start:crate::guides::point(&tab.editor.document,point,tab.view.zoom/pixels_per_point),previous,add:modifiers.shift,subtract:modifiers.alt});}
                                } else {
                                    if tool==Tool::Crop {tab.crop_before.get_or_insert_with(||tab.selection.clone());}
                                    let previous=tab.selection.take();
                                    tab.gesture=Some(Gesture::Select{start:crate::guides::point(&tab.editor.document,point,tab.view.zoom/pixels_per_point),previous,add:modifiers.shift,subtract:modifiers.alt});
                                }
                            }
                            Tool::Lasso => {
                                if polygon_lasso {
                                    if let Some(Gesture::Polygon { points, .. }) = &mut tab.gesture
                                    {
                                        if points.len() < 100_000 {
                                            points.push(point);
                                        }
                                    } else {
                                        let previous = tab.selection.take();
                                        tab.gesture = Some(Gesture::Polygon {
                                            points: vec![point],
                                            previous,
                                            add: modifiers.shift,
                                            subtract: modifiers.alt,
                                        });
                                    }
                                } else {
                                    let previous = tab.selection.take();
                                    tab.gesture = Some(Gesture::Lasso {
                                        points: vec![point],
                                        previous,
                                        add: modifiers.shift,
                                        subtract: modifiers.alt,
                                    });
                                }
                            }
                            Tool::Wand => {
                                let image = tab.editor.document.composite();
                                if let Some(selection) = editing::color_selection(
                                    &image,
                                    [point[0] as i32, point[1] as i32],
                                    tolerance,
                                    contiguous,
                                ) {
                                    let selection=if selection_smooth {crate::selection_edges::color(selection)}else{selection};
                                    tab.selection = Some(if modifiers.shift || modifiers.alt {
                                        editing::combine_selection(
                                            tab.selection.as_ref(),
                                            selection,
                                            modifiers.alt,
                                            size[0],
                                            size[1],
                                        )
                                    } else {
                                        selection
                                    });
                                }
                            }
                            Tool::Fill
                                if !tab.editor.document.is_locked(tab.editor.document.active) =>
                            {
                                let selection = tab.selection.clone();
                                let mask = tab.edit_mask;
                                tab.editor.edit("채우기", |doc| {
                                    if mask {
                                        if let Err(error)=crate::masks::fill(doc.active_layer_mut().unwrap(),selection.as_ref(),brush.color,size){move_error=Some(error);}
                                    } else if doc.active_layer().unwrap().content.is_none()
                                        && !doc.active_layer().unwrap().group
                                    {
                                        editing::fill(
                                            doc.active_layer_mut().unwrap(),
                                            selection.as_ref(),
                                            brush.color,
                                            false,
                                        )
                                    }
                                });
                                tab.render_dirty = true;
                            }
                            Tool::Stamp if modifiers.alt => {
                                tab.clone_origin = Some(point);
                                tab.clone_delta = None;
                            }
                            Tool::Stamp | Tool::Heal | Tool::Blur
                                if tab.editor.document.active_layer().is_some_and(|l| {
                                    !tab.editor.document.is_locked(l.id)
                                        && ((!l.canvas_space() && l.content.is_none() && !tab.edit_mask)
                                            || (tool == Tool::Blur
                                                && tab.edit_mask
                                                && l.mask.is_some()))
                                }) =>
                            {
                                if tool != Tool::Stamp || tab.clone_origin.is_some() {
                                    let source = editing::retouch_source(
                                        &tab.editor.document,
                                        sample_merged,
                                        tab.edit_mask,
                                    );
                                    let source = if tool == Tool::Blur {
                                        crate::precision::blur(&source, 3.0)
                                    } else {
                                        source
                                    };
                                    let delta = if tool == Tool::Stamp {
                                        let origin = tab.clone_origin.unwrap();
                                        let delta = [origin[0] - point[0], origin[1] - point[1]];
                                        let delta = if clone_aligned {
                                            tab.clone_delta.unwrap_or(delta)
                                        } else {
                                            delta
                                        };
                                        tab.clone_delta = Some(delta);
                                        delta
                                    } else {
                                        [0.0, 0.0]
                                    };
                                    let mode = match tool {
                                        Tool::Stamp => editing::Retouch::Clone,
                                        Tool::Heal => editing::Retouch::Heal,
                                        _ => editing::Retouch::Blur,
                                    };
                                    tab.editor.begin(tool.name());
                                    if let Err(error) = crate::masks::retouch(
                                        tab.editor.document.active_layer_mut().unwrap(),
                                        &source,
                                        point,
                                        point,
                                        delta,
                                        &brush,
                                        tab.selection.as_ref(),
                                        mode,
                                    ) {move_error=Some(error);}
                                    tab.gesture = Some(Gesture::Retouch {
                                        last: point,
                                        source: std::sync::Arc::new(source),
                                        delta,
                                        mode,
                                    });
                                    tab.damage_stroke(point, point, brush.radius);
                                }
                            }
                            Tool::Shape | Tool::Gradient => {
                                tab.gesture = Some(Gesture::Vector {
                                    start: crate::guides::point(
                                        &tab.editor.document,
                                        point,
                                        tab.view.zoom / pixels_per_point,
                                    ),
                                    end: crate::guides::point(
                                        &tab.editor.document,
                                        point,
                                        tab.view.zoom / pixels_per_point,
                                    ),
                                });
                            }
                            Tool::Text => {
                                pending_text = Some((tab.id, point, crate::text_canvas::hit(&tab.editor.document, point)));
                            }
                            Tool::Zoom => {
                                tab.view.zoom_at(
                                    area,
                                    pointer,
                                    if ctx.input(|i| i.modifiers.alt) {
                                        0.8
                                    } else {
                                        1.25
                                    },
                                );
                            }
                            Tool::Eyedropper => {
                                let alt = match &events[press_index] { egui::Event::PointerButton { modifiers, .. } => modifiers.alt, _ => false };
                                sampled = crate::colors::sample(&tab.editor.document,point,sample_size,sample_merged).map(|color|(color,sample_background ^ alt));
                            }
                            _ => {}
                        }
                    }
                }
                if (down || released)
                    && let Some(mut gesture) = tab.gesture.take()
                {
                    let start_index = press.map_or(0, |(index, _, _)| index + 1);
                    let mut positions: Vec<Pos2> = events[start_index..]
                        .iter()
                        .filter_map(|event| match event {
                            egui::Event::PointerMoved(pos) => Some(*pos),
                            egui::Event::PointerButton {
                                pos,
                                pressed: false,
                                ..
                            } => Some(*pos),
                            _ => None,
                        })
                        .collect();
                    if let Some(pointer) = pointer
                        && positions.last() != Some(&pointer)
                    {
                        positions.push(pointer);
                    }
                    let position_count = positions.len();
                    for (position_index, pointer) in positions.into_iter().enumerate() {
                        let point = tab.view.document_pos(area, size, pixels_per_point, pointer);
                        let flush = released && position_index + 1 == position_count;
                        match &mut gesture {
                            Gesture::Pan { last } => {
                                tab.view.pan += pointer - *last;
                                *last = pointer;
                                tab.view.fit = false;
                            }
                            Gesture::Paint { last } => {
                                let point = if flush {
                                    point
                                } else {
                                    [
                                        last[0] + (point[0] - last[0]) * (1.0 - smoothing),
                                        last[1] + (point[1] - last[1]) * (1.0 - smoothing),
                                    ]
                                };
                                if point != *last {
                                    if let Err(error) = crate::masks::paint(
                                        tab.editor.document.active_layer_mut().unwrap(),
                                        *last,
                                        point,
                                        &brush,
                                        tab.selection.as_ref(),
                                    ) {move_error=Some(error);}
                                    tab.damage_stroke(*last, point, brush.radius);
                                    *last = point;
                                }
                            }
                            Gesture::Guide { index } => {
                                let guide = &mut tab.editor.document.layout.guides[*index];
                                guide.position = point[guide.axis].clamp(-1e9, 1e9);
                            }
                            Gesture::Transform { initial, offset, size, handle, start, batch } => {
                                let scale=tab.view.zoom/pixels_per_point;
                                let mut target=point;
                                tab.snap_lines=[None;2];
                                if matches!(handle,crate::transform::Handle::Resize(_)) {
                                    let actual=batch.handles().into_iter().find(|(h,_)|h==handle).map_or(point,|(_,p)|[p[0]+point[0]-start[0],p[1]+point[1]-start[1]]);
                                    let anchors=[vec![actual[0]],vec![actual[1]]];
                                    let (delta,lines)=crate::guides::snap(&tab.editor.document,&anchors,&batch.roots,scale);
                                    target=[point[0]+delta[0],point[1]+delta[1]];tab.snap_lines=lines;
                                }
                                if batch.legacy() {
                                    let (mut transform,new_offset)=crate::transform::drag(*initial,*offset,*size,*handle,[*start,target],modifiers.shift,modifiers.alt);
                                    if *handle==crate::transform::Handle::Move {
                                        let q=[[0.,0.],[size[0] as f32,0.],size.map(|v|v as f32),[0.,size[1] as f32]].map(|p|transform.to_document(p,new_offset,*size));
                                        let anchors=std::array::from_fn(|axis|vec![q.iter().map(|p|p[axis]).fold(f32::INFINITY,f32::min),q.iter().map(|p|p[axis]).fold(f32::NEG_INFINITY,f32::max),q.iter().map(|p|p[axis]).sum::<f32>()/4.0]);
                                        let (delta,mut lines)=crate::guides::snap(&tab.editor.document,&anchors,&batch.roots,scale);
                                        let locked=modifiers.shift.then(||usize::from((point[0]-start[0]).abs()>=(point[1]-start[1]).abs()));
                                        for axis in 0..2 {if locked!=Some(axis){transform.translation[axis]+=delta[axis];}else{lines[axis]=None;}}
                                        tab.snap_lines=lines;
                                    }
                                    transform.sampling=batch.sampling;
                                    if tab.editor.document.active_layer().is_some_and(|l|l.transform!=transform||l.offset!=new_offset) {
                                        tab.editor.begin("캔버스 변형");let layer=tab.editor.document.active_layer_mut().unwrap();layer.transform=transform;layer.offset=new_offset;
                                        if let Ok(mut frame)=if let Some(id)=batch.mask {crate::transform_batch::Frame::new_mask(&tab.editor.document,id)} else {crate::transform_batch::Frame::new(&tab.editor.document,batch.roots.clone())} {frame.sampling=batch.sampling;tab.transform_frame=Some(frame);}
                                        tab.render_dirty=true;
                                    }
                                } else {
                                    match batch.drag(*handle,*start,target,modifiers.shift,modifiers.alt) {
                                        Ok(mut quad)=>{
                                            if *handle==crate::transform::Handle::Move {
                                                let anchors=std::array::from_fn(|axis|vec![quad.iter().map(|p|p[axis]).fold(f32::INFINITY,f32::min),quad.iter().map(|p|p[axis]).fold(f32::NEG_INFINITY,f32::max),quad.iter().map(|p|p[axis]).sum::<f32>()/4.0]);
                                                let (mut delta,mut lines)=crate::guides::snap(&tab.editor.document,&anchors,&batch.roots,scale);
                                                if modifiers.shift {let axis=usize::from((point[0]-start[0]).abs()>=(point[1]-start[1]).abs());delta[axis]=0.0;lines[axis]=None;}
                                                quad=quad.map(|p|[p[0]+delta[0],p[1]+delta[1]]);tab.snap_lines=lines;
                                            }
                                            match batch.apply(&mut tab.editor,quad) {Ok(changed)=>{if let Some(frame)=&mut tab.transform_frame {frame.quad=quad;}tab.render_dirty|=changed;},Err(e)=>move_error=Some(e)}
                                        }
                                        Err(e)=>move_error=Some(e),
                                    }
                                }
                                if let Some((_,actual))=tab.transform_frame.as_ref().and_then(|frame|frame.handles().into_iter().find(|(h,_)|h==handle)) {for (axis,value) in actual.iter().enumerate(){if tab.snap_lines[axis].is_some_and(|line|(line-value).abs()>0.001){tab.snap_lines[axis]=None;}}}
                            }
                            Gesture::CropBox { initial, handle, start } => {
                                let scale=tab.view.zoom/pixels_per_point;
                                let mut target=point;tab.snap_lines=[None;2];
                                if *handle!=crate::transform::Handle::Move {
                                    let actual=crate::crop_handles::handles(initial).into_iter().find(|(h,_)|h==handle).map_or(point,|(_,p)|[p[0]+point[0]-start[0],p[1]+point[1]-start[1]]);
                                    let (delta,lines)=crate::guides::snap(&tab.editor.document,&[vec![actual[0]],vec![actual[1]]],&[],scale);target=[point[0]+delta[0],point[1]+delta[1]];tab.snap_lines=lines;
                                }
                                let mut selection=crate::crop_handles::drag(initial,*handle,*start,target,size,if modifiers.shift{Some(1.0)}else{crop_ratio},modifiers.alt);
                                if *handle==crate::transform::Handle::Move{
                                    let anchors=std::array::from_fn(|i|vec![selection.min[i] as f32,selection.max[i] as f32,(selection.min[i]+selection.max[i]) as f32/2.]);
                                    let (delta,lines)=crate::guides::snap(&tab.editor.document,&anchors,&[],scale);
                                    selection=crate::crop_handles::drag(&selection,*handle,[0.,0.],delta,size,None,false);tab.snap_lines=lines;
                                }
                                if let Some((_,actual))=crate::crop_handles::handles(&selection).into_iter().find(|(h,_)|h==handle){for (i,value) in actual.iter().enumerate(){if tab.snap_lines[i].is_some_and(|line|(line-value).abs()>0.6){tab.snap_lines[i]=None;}}}
                                tab.selection=Some(selection);
                            }
                            Gesture::Boundary { start, previous }
                            | Gesture::Pixels { start, previous, .. } => {
                                let mut delta = [point[0] - start[0], point[1] - start[1]];
                                if modifiers.shift {
                                    let axis = usize::from(delta[0].abs() >= delta[1].abs());
                                    delta[axis] = 0.0;
                                }
                                let delta = delta.map(|d| d.round() as i32);
                                tab.selection = Some(editing::shifted_selection(previous, delta, size));
                                if let Gesture::Pixels { source, last_delta, .. } = &mut gesture
                                    && delta != *last_delta {
                                    match source.translated(delta) {
                                        Ok(layer) => {
                                            tab.editor.begin("선택 픽셀 이동·복제");
                                            if let Some(target) = tab.editor.document.active_layer_mut() {
                                                *target = layer;
                                            }
                                            *last_delta = delta;
                                            tab.render_dirty = true;
                                        }
                                        Err(error) => move_error = Some(error),
                                    }
                                }
                            }
                            Gesture::Move {
                                start,
                                offsets,
                                bounds,
                            } => {
                                let mut delta = [point[0] - start[0], point[1] - start[1]];
                                let locked_axis = modifiers
                                    .shift
                                    .then(|| usize::from(delta[0].abs() >= delta[1].abs()));
                                if let Some(axis) = locked_axis {
                                    delta[axis] = 0.0;
                                }
                                if let Some((min, max)) = bounds {
                                    let anchors = std::array::from_fn(|axis| {
                                        vec![
                                            min[axis] + delta[axis],
                                            (min[axis] + max[axis]) / 2.0 + delta[axis],
                                            max[axis] + delta[axis],
                                        ]
                                    });
                                    let ids = offsets.iter().map(|(id, _)| *id).collect::<Vec<_>>();
                                    let (adjustment, lines) = crate::guides::snap(
                                        &tab.editor.document,
                                        &anchors,
                                        &ids,
                                        tab.view.zoom / pixels_per_point,
                                    );
                                    tab.snap_lines = lines;
                                    if let Some(axis) = locked_axis {
                                        tab.snap_lines[axis] = None;
                                    }
                                    for axis in 0..2 {
                                        if locked_axis != Some(axis) {
                                            delta[axis] += adjustment[axis];
                                        }
                                    }
                                }
                                let delta = delta.map(|d| d.round() as i32);
                                for (id, offset) in offsets {
                                    let new_offset = [
                                        offset[0].saturating_add(delta[0]),
                                        offset[1].saturating_add(delta[1]),
                                    ];
                                    if tab
                                        .editor
                                        .document
                                        .layers
                                        .iter()
                                        .find(|l| l.id == *id)
                                        .is_some_and(|l| l.offset != new_offset)
                                    {
                                        tab.editor.begin("선택 레이어 이동");
                                        tab.editor.document.move_layer(*id, new_offset);
                                        tab.render_dirty = true;
                                    }
                                }
                            }
                            Gesture::Select {
                                start,
                                previous,
                                add,
                                subtract,
                            } => {
                                let point = crate::guides::point(
                                    &tab.editor.document,
                                    point,
                                    tab.view.zoom / pixels_per_point,
                                );
                                let selection = editing::selection_rectangle(
                                    *start,
                                    point,
                                    size,
                                    if tool == Tool::Crop {
                                        if modifiers.shift {
                                            Some(1.0)
                                        } else {
                                            crop_ratio
                                        }
                                    } else {
                                        None
                                    },
                                    tool == Tool::Crop && *subtract,
                                );
                                tab.selection = (selection.max[0] > selection.min[0]
                                    && selection.max[1] > selection.min[1])
                                    .then(|| {
                                        let selection = if tool == Tool::Ellipse {
                                            editing::ellipse_smooth(selection.min, selection.max,selection_smooth)
                                        } else {
                                            selection
                                        };
                                        if tool != Tool::Crop && (*add || *subtract) {
                                            editing::combine_selection(
                                                previous.as_ref(),
                                                selection,
                                                *subtract,
                                                size[0],
                                                size[1],
                                            )
                                        } else {
                                            selection
                                        }
                                    });
                            }
                            Gesture::Lasso { points, .. } => {
                                if points.last() != Some(&point) && points.len() < 100_000 {
                                    points.push(point);
                                }
                            }
                            Gesture::Polygon { .. } => {}
                            Gesture::Retouch {
                                last,
                                source,
                                delta,
                                mode,
                            } => {
                                let point = if flush {
                                    point
                                } else {
                                    [
                                        last[0] + (point[0] - last[0]) * (1.0 - smoothing),
                                        last[1] + (point[1] - last[1]) * (1.0 - smoothing),
                                    ]
                                };
                                if let Err(error) = crate::masks::retouch(
                                    tab.editor.document.active_layer_mut().unwrap(),
                                    source,
                                    *last,
                                    point,
                                    *delta,
                                    &brush,
                                    tab.selection.as_ref(),
                                    *mode,
                                ) {move_error=Some(error);}
                                tab.damage_stroke(*last, point, brush.radius);
                                *last = point;
                            }
                            Gesture::Vector { end, .. } => {
                                *end = crate::guides::point(
                                    &tab.editor.document,
                                    point,
                                    tab.view.zoom / pixels_per_point,
                                );
                            }
                        }
                    }
                    if move_error.is_some() {
                        if let Gesture::Pixels { previous, .. } = gesture {
                            tab.selection = Some(previous);
                        }
                        tab.editor.cancel();
                        tab.render_dirty = true;
                    } else {
                        tab.gesture = Some(gesture);
                    }
                }
                let finish_polygon =
                    ctx.input(|i| i.key_pressed(Key::Enter)) || response.double_clicked();
                if (!down
                    || (finish_polygon && matches!(tab.gesture, Some(Gesture::Polygon { .. }))))
                    && let Some(gesture) = tab.gesture.take()
                {
                    match gesture {
                        Gesture::Polygon {
                            points,
                            previous,
                            add,
                            subtract,
                        } => {
                            if finish_polygon && points.len() >= 3 {
                                if let Some(selection) = editing::polygon_smooth(&points, size[0], size[1],selection_smooth)
                                {
                                    tab.selection = Some(if add || subtract {
                                        editing::combine_selection(
                                            previous.as_ref(),
                                            selection,
                                            subtract,
                                            size[0],
                                            size[1],
                                        )
                                    } else {
                                        selection
                                    });
                                } else {
                                    tab.selection = previous;
                                }
                            } else {
                                tab.gesture = Some(Gesture::Polygon {
                                    points,
                                    previous,
                                    add,
                                    subtract,
                                });
                            }
                        }
                        Gesture::Paint { last } => {
                            tab.last_paint = Some((tab.editor.document.active, brush.mask, last));
                            tab.editor.finish();
                        }
                        Gesture::Guide { index } => {
                            if pointer.is_some_and(|p| !area.contains(p)) {
                                tab.editor.document.layout.guides.remove(index);
                            }
                            tab.editor.finish();
                        }
                        Gesture::Transform { .. } | Gesture::CropBox { .. } => {}
                        Gesture::Lasso {
                            points,
                            previous,
                            add,
                            subtract,
                        } => {
                            if let Some(selection) = editing::polygon_smooth(&points, size[0], size[1],selection_smooth) {
                                tab.selection = Some(if add || subtract {
                                    editing::combine_selection(
                                        previous.as_ref(),
                                        selection,
                                        subtract,
                                        size[0],
                                        size[1],
                                    )
                                } else {
                                    selection
                                });
                            }
                        }
                        Gesture::Vector { start, end } => {
                            if tool == Tool::Gradient {
                                created_content = Some((
                                    Content::Gradient {
                                        width: size[0],
                                        height: size[1],
                                        start,
                                        end,
                                        colors: if gradient_reversed { [colors[1],colors[0]] } else { colors },
                                        stops: vec![],
                                        interpolation: gradient_interpolation,
                                    },
                                    [0, 0],
                                ));
                            } else {
                                let width = (end[0] - start[0]).abs().round().max(1.0) as u32;
                                let height = (end[1] - start[1]).abs().round().max(1.0) as u32;
                                created_content = Some((
                                    Content::Shape {
                                        kind: shape_kind,
                                        width,
                                        height,
                                        fill: colors[0],
                                        stroke: colors[1],
                                        thickness: shape_stroke,
                                    },
                                    [start[0].min(end[0]) as i32, start[1].min(end[1]) as i32],
                                ));
                            }
                        }
                        _ => {
                            tab.editor.finish();
                        }
                    }
                }
                if tool == Tool::Pen && pen_enabled && !space && !transforming && ui.is_enabled() {
                    let fill = if pen_fill { colors[1] } else { [0; 4] };
                    let tolerance = 7.0 * pixels_per_point / tab.view.zoom;
                    let mut actions = Vec::new();
                    for event in &events {
                        match event {
                            egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers } if response.hovered() && area.contains(*pos) => {
                                actions.push(crate::paths::Action::Press(tab.view.document_pos(area, size, pixels_per_point, *pos), modifiers.alt, modifiers.ctrl || modifiers.command));
                            }
                            egui::Event::PointerMoved(pos) => actions.push(crate::paths::Action::Move(tab.view.document_pos(area, size, pixels_per_point, *pos), modifiers.alt)),
                            egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, .. } => {
                                actions.push(crate::paths::Action::Move(tab.view.document_pos(area, size, pixels_per_point, *pos), modifiers.alt));
                                actions.push(crate::paths::Action::Release);
                            }
                            _ => {}
                        }
                    }
                    if !ctx.egui_wants_keyboard_input() {
                        if ctx.input(|i| i.key_pressed(Key::Enter)) { actions.push(crate::paths::Action::Finish); }
                        if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) { actions.push(crate::paths::Action::Delete); }
                    }
                    for action in actions {
                        match tab.pen.apply(&mut tab.editor, action, tolerance, fill, colors[0], shape_stroke) {
                            Ok(changed) => tab.render_dirty |= changed,
                            Err(error) => tab.notes.push(error),
                        }
                    }
                }
                let ready = match tab.render_job.as_ref().map(|job| job.try_recv()) {
                    Some(Ok(image)) => Some(image),
                    Some(Err(mpsc::TryRecvError::Disconnected)) => {
                        tab.render_job = None;
                        tab.render_dirty = false;
                        tab.notes.push(
                            "미리보기를 만들지 못했습니다. 다음 편집 시 다시 시도합니다.".into(),
                        );
                        None
                    }
                    _ => None,
                };
                if let Some(rendered) = ready {
                    tab.render_job = None;
                    if !tab.render_dirty {
                        if let Some(texture) = &mut tab.texture {
                            texture.set(rendered.color, tab.display_sampling.options());
                        } else {
                            tab.texture = Some(ctx.load_texture(
                                format!("document-{}", tab.id),
                                rendered.color,
                                tab.display_sampling.options(),
                            ));
                        }
                        tab.composite = Some(rendered.pixels);
                        tab.histogram = Some(rendered.histogram);
                    }
                }
                tab.update_painted_region();
                if tab.paint_damage.is_some() && tab.render_job.is_none() {
                    tab.render_dirty = true;
                }
                if tab.render_dirty && tab.render_job.is_none() {
                    let document = tab.editor.document.clone();
                    let (sender, receiver) = mpsc::channel();
                    tab.render_job = Some(receiver);
                    tab.render_dirty = false;
                    tab.paint_damage = None;
                    let context = ctx.clone();
                    std::thread::spawn(move || {
                        let image = Rendered::new(document.composite());
                        let _ = sender.send(image);
                        context.request_repaint();
                    });
                }
                if tab.render_job.is_some() {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
                let image_rect = tab.view.image_rect(area, size, pixels_per_point);
                let painter = ui.painter_at(area);
                let screen_point = |point: [f32; 2]| {
                    image_rect.min
                        + Vec2::new(point[0], point[1]) * tab.view.zoom / pixels_per_point
                };
                let visible = image_rect.intersect(area);
                if visible.is_positive() {
                    let cell = 16.0;
                    let min = ((visible.min - image_rect.min) / cell).floor();
                    let max = ((visible.max - image_rect.min) / cell).ceil();
                    let checker = painter.with_clip_rect(visible);
                    for y in min.y as i32..max.y as i32 {
                        for x in min.x as i32..max.x as i32 {
                            let rect = Rect::from_min_size(
                                image_rect.min + Vec2::new(x as f32, y as f32) * cell,
                                Vec2::splat(cell),
                            );
                            checker.rect_filled(
                                rect,
                                0.0,
                                if (x + y) % 2 == 0 {
                                    Color32::from_gray(185)
                                } else {
                                    Color32::from_gray(225)
                                },
                            );
                        }
                    }
                    if let Some(texture) = &tab.texture {
                        painter.image(
                            texture.id(),
                            image_rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    tab.mask_overlay.draw(&painter,tab.editor.document.active_layer(),&mask_overlay_settings,crate::mask_overlay::Frame{image:image_rect,area,scale:tab.view.zoom/pixels_per_point,revision:tab.editor.revision,editing:tab.gesture.is_some()});
                    painter.rect_stroke(
                        image_rect,
                        0.0,
                        Stroke::new(1.0, Color32::from_gray(70)),
                        egui::StrokeKind::Outside,
                    );
                    if let Some(selection) = &tab.selection {
                        let scale = tab.view.zoom / pixels_per_point;
                        let step = (1.0 / scale).ceil().max(1.0) as usize;
                        let min = ((visible.min - image_rect.min) / scale).floor();
                        let max = ((visible.max - image_rect.min) / scale).ceil();
                        let x0 = (min.x as i32).max(0);
                        let y0 = (min.y as i32).max(0);
                        let x1 = (max.x as i32).min(size[0] as i32);
                        let y1 = (max.y as i32).min(size[1] as i32);
                        for y in (y0..y1).step_by(step) {
                            for x in (x0..x1).step_by(step) {
                                if selection.coverage(x, y) < 0.5 {
                                    continue;
                                }
                                let next_x = (x + step as i32).min(size[0] as i32);
                                let next_y = (y + step as i32).min(size[1] as i32);
                                for (other, edge) in [
                                    ([x - step as i32, y], [[x, y], [x, next_y]]),
                                    ([x, y - step as i32], [[x, y], [next_x, y]]),
                                    ([next_x, y], [[next_x, y], [next_x, next_y]]),
                                    ([x, next_y], [[x, next_y], [next_x, next_y]]),
                                ] {
                                    let inside = other[0] >= 0
                                        && other[1] >= 0
                                        && other[0] < size[0] as i32
                                        && other[1] < size[1] as i32;
                                    if inside && selection.coverage(other[0], other[1]) >= 0.5 {
                                        continue;
                                    }
                                    let points = edge.map(|p| screen_point(p.map(|v| v as f32)));
                                    painter.line_segment(points, Stroke::new(2.0, Color32::BLACK));
                                    painter.line_segment(points, Stroke::new(1.0, ACCENT));
                                }
                            }
                        }
                    }
                }
                if let Some(Gesture::Vector { start, end }) = &tab.gesture {
                    painter.rect_stroke(
                        Rect::from_two_pos(screen_point(*start), screen_point(*end)),
                        0.0,
                        Stroke::new(1.0, ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
                if tool == Tool::Pen { tab.pen.paint(&painter, tab.editor.document.active_layer(), screen_point); }
                if let Some(Gesture::Lasso { points, .. } | Gesture::Polygon { points, .. }) =
                    &tab.gesture
                {
                    let mut preview: Vec<Pos2> = points.iter().map(|p| screen_point(*p)).collect();
                    if matches!(tab.gesture, Some(Gesture::Polygon { .. })) {
                        if let Some(pointer) = pointer {
                            preview.push(pointer);
                        }
                        if let Some(first) = preview.first().copied() {
                            preview.push(first);
                        }
                    }
                    painter.add(egui::Shape::line(preview, Stroke::new(1.0, ACCENT)));
                }
                if show_grid && tab.view.zoom / pixels_per_point >= 6.0 {
                    let scale = tab.view.zoom / pixels_per_point;
                    let visible = image_rect.intersect(area);
                    let min = ((visible.min - image_rect.min) / scale).floor();
                    let max = ((visible.max - image_rect.min) / scale).ceil();
                    for x in min.x as i32..=max.x as i32 {
                        let px = image_rect.left() + x as f32 * scale;
                        painter.line_segment(
                            [
                                Pos2::new(px, visible.top()),
                                Pos2::new(px, visible.bottom()),
                            ],
                            Stroke::new(0.5, Color32::from_black_alpha(80)),
                        );
                    }
                    for y in min.y as i32..=max.y as i32 {
                        let py = image_rect.top() + y as f32 * scale;
                        painter.line_segment(
                            [
                                Pos2::new(visible.left(), py),
                                Pos2::new(visible.right(), py),
                            ],
                            Stroke::new(0.5, Color32::from_black_alpha(80)),
                        );
                    }
                }
                draw_layout(
                    ui,
                    area,
                    full_area,
                    image_rect,
                    &tab.editor.document.layout,
                    tab.view.zoom / pixels_per_point,
                    rulers,
                );
                if transforming && let Some(frame)=&tab.transform_frame {
                    let handles=batch_handles(frame,image_rect,tab.view.zoom/pixels_per_point);
                    if handles.len()==9 {
                        let corners=[0,2,4,6].map(|i|handles[i].1);
                        for i in 0..4 {painter.line_segment([corners[i],corners[(i+1)%4]],Stroke::new(1.0,ACCENT));}
                        painter.line_segment([handles[1].1,handles[8].1],Stroke::new(1.0,ACCENT));
                        for (handle,point) in handles {if handle==crate::transform::Handle::Rotate{painter.circle_filled(point,5.0,ACCENT);}else{painter.rect_filled(Rect::from_center_size(point,Vec2::splat(7.0)),0.0,Color32::WHITE);}}
                    }
                }
                if tool==Tool::Crop && let Some(selection)=&tab.selection {
                    for (_,point) in crate::crop_handles::handles(selection) {let screen=image_rect.min+Vec2::new(point[0],point[1])*(tab.view.zoom/pixels_per_point);painter.rect_filled(Rect::from_center_size(screen,Vec2::splat(8.0)),0.0,Color32::WHITE);}
                }
                if matches!(tab.gesture, Some(Gesture::Move { .. }) | Some(Gesture::Transform { .. }) | Some(Gesture::CropBox { .. })) {
                    for (axis, position) in tab.snap_lines.into_iter().enumerate() {
                        if let Some(position) = position {
                            let mut a = area.min;
                            let mut b = area.max;
                            let screen =
                                image_rect.min[axis] + position * tab.view.zoom / pixels_per_point;
                            a[axis] = screen;
                            b[axis] = screen;
                            painter.line_segment(
                                [a, b],
                                Stroke::new(1.0, Color32::from_rgb(235, 100, 220)),
                            );
                        }
                    }
                }
                if response.hovered() || tab.gesture.is_some() {
                    let cursor = if space || tool == Tool::Hand || matches!(tab.gesture, Some(Gesture::Pan { .. })) {
                        if down { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab }
                    } else {
                        match tool {
                            Tool::Move => egui::CursorIcon::Move,
                            Tool::Text => egui::CursorIcon::Text,
                            Tool::Zoom if modifiers.alt => egui::CursorIcon::ZoomOut,
                            Tool::Zoom => egui::CursorIcon::ZoomIn,
                            _ => egui::CursorIcon::Crosshair,
                        }
                    };
                    let handles = if transforming {
                        tab.transform_frame.as_ref().map(|frame| batch_handles(frame, image_rect, tab.view.zoom / pixels_per_point)).unwrap_or_default()
                    } else if tool == Tool::Crop {
                        tab.selection.as_ref().map(|selection| crate::crop_handles::handles(selection).into_iter().map(|(handle, point)| (handle, image_rect.min + Vec2::new(point[0],point[1]) * (tab.view.zoom / pixels_per_point))).collect()).unwrap_or_default()
                    } else { vec![] };
                    let handle = pointer.and_then(|p| handles.iter().filter_map(|(h, at)| { let distance = at.distance(p); (distance <= 9.).then_some((*h, distance)) }).min_by(|a,b| a.1.total_cmp(&b.1)).map(|(h,_)| h));
                    let corners = if handles.len()>=8 { [handles[0].1,handles[2].1,handles[4].1,handles[6].1] } else { [image_rect.left_top(),image_rect.right_top(),image_rect.right_bottom(),image_rect.left_bottom()] };
                    ctx.set_cursor_icon(handle.map_or(if transforming { egui::CursorIcon::Move } else { cursor }, |h| crate::canvas::handle_cursor(h, corners, down)));
                    if matches!(
                        tool,
                        Tool::Brush | Tool::Eraser | Tool::Stamp | Tool::Heal | Tool::Blur
                    ) && !space && !transforming
                        && let Some(pointer) = pointer
                    {
                        painter.circle_stroke(
                            pointer,
                            (brush.radius * tab.view.zoom / pixels_per_point).max(2.0),
                            Stroke::new(1.0, Color32::WHITE),
                        );
                    }
                }
                if let Some((color, background)) = sampled {
                    if background { self.background = color; } else { self.color = color; }
                }
                if let Some(error) = move_error {
                    if transforming {self.status=error;}else{self.message = Some(("편집 실패".into(), error));}
                }
                if let Some((content, offset)) = created_content {
                    self.add_content(content, offset);
                    ctx.request_repaint();
                }
                if let Some((id, point, layer)) = pending_text {
                    self.text_session = layer.and_then(|layer|self.tab().and_then(|tab|tab.editor.document.layers.iter().find(|l|l.id==layer)).and_then(|layer|crate::text_canvas::Session::existing(id,layer))).or_else(||Some(crate::text_canvas::Session::new(id,point,colors[0])));
                    ctx.request_repaint();
                }
                if let Some(mut session) = self.text_session.take() {
                    if let Some(tab) = self.tab().filter(|tab|tab.id==session.tab) {
                        let decision=session.ui(ctx,area,&tab.view,&tab.editor.document);
                        self.text_session=Some(session);
                        if let Some(apply)=decision {self.finish_text(apply);}
                    } else {self.text_session=Some(session);}
                }
            });
    }

    fn transform_ui(&mut self, ctx: &egui::Context) {
        if !self.transforming {
            return;
        }
        let mut open = true;
        let mut decision = None;
        let mut error = None;
        let mask = self
            .tab()
            .and_then(|t| t.transform_frame.as_ref())
            .is_some_and(|f| f.mask.is_some());
        egui::Window::new(if mask {
            "마스크 변형"
        } else {
            "레이어 변형"
        })
        .open(&mut open)
        .default_pos(Pos2::new(80., 125.))
        .default_width(340.)
        .vscroll(true)
        .max_height(500.)
        .show(ctx, |ui| {
            ui.label("변형 중: 원본 유지 · Enter 적용 · Esc 취소");
            ui.label("Shift: 비율/15°/축 고정 · Alt: 중심 기준");
            if let Some(tab) = self.tab_mut()
                && let Some(mut frame) = tab.transform_frame.take()
            {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut frame.distort, false, "크기·회전");
                    ui.selectable_value(&mut frame.distort, true, "자유 왜곡");
                });
                if frame.distort {
                    ui.label("모서리: 개별 왜곡 · 변: 양 끝 조절 · 안쪽: 이동");
                } else {
                    ui.label("모서리·변: 크기 · 원형 핸들: 회전 · 안쪽: 이동");
                }
                frame.capture(&tab.editor.document);
                let previous = frame.sampling;
                egui::ComboBox::from_id_salt("transform-sampling")
                    .selected_text(frame.sampling.label())
                    .show_ui(ui, |ui| {
                        for mode in [
                            crate::sampling::Sampling::Nearest,
                            crate::sampling::Sampling::Bilinear,
                            crate::sampling::Sampling::Bicubic,
                        ] {
                            ui.selectable_value(&mut frame.sampling, mode, mode.label());
                        }
                    });
                let mut quad = frame.quad;
                let mut changed = previous != frame.sampling;
                if frame.legacy()
                    && let Some(layer) = tab.editor.document.active_layer()
                {
                    let mut transform = layer.transform;
                    let mut position = std::array::from_fn::<_, 2, _>(|axis| {
                        layer.offset[axis] as f32 + transform.translation[axis]
                    });
                    let mut numeric = false;
                    ui.horizontal(|ui| {
                        for (v, label) in transform.scale.iter_mut().zip(["가로 ", "세로 "]) {
                            numeric |= ui
                                .add(
                                    egui::DragValue::new(v)
                                        .speed(0.01)
                                        .range(-100.0..=100.0)
                                        .prefix(label)
                                        .suffix(" 배"),
                                )
                                .changed();
                            if v.abs() < 0.01 {
                                *v = if v.is_sign_negative() { -0.01 } else { 0.01 };
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        numeric |= ui
                            .add(
                                egui::DragValue::new(&mut transform.angle)
                                    .speed(1.)
                                    .range(-360.0..=360.0)
                                    .prefix("회전 ")
                                    .suffix("°"),
                            )
                            .changed();
                        for (v, label) in position.iter_mut().zip(["X ", "Y "]) {
                            numeric |= ui
                                .add(
                                    egui::DragValue::new(v)
                                        .speed(0.1)
                                        .range(-1e9..=1e9)
                                        .prefix(label),
                                )
                                .changed();
                        }
                    });
                    if numeric {
                        let offset = position.map(|v| v.round() as i32);
                        transform.translation =
                            std::array::from_fn(|i| position[i] - offset[i] as f32);
                        let size = [layer.pixels.width(), layer.pixels.height()];
                        quad = [
                            [0., 0.],
                            [size[0] as f32, 0.],
                            size.map(|s| s as f32),
                            [0., size[1] as f32],
                        ]
                        .map(|p| transform.to_document(p, offset, size));
                        changed = true;
                    }
                } else {
                    let old_center = frame.center();
                    let mut center = old_center;
                    let edge = [
                        frame.quad[1][0] - frame.quad[0][0],
                        frame.quad[1][1] - frame.quad[0][1],
                    ];
                    let old_angle = edge[1].atan2(edge[0]).to_degrees();
                    let mut angle = old_angle;
                    let lengths = [
                        edge[0].hypot(edge[1]),
                        (frame.quad[3][0] - frame.quad[0][0])
                            .hypot(frame.quad[3][1] - frame.quad[0][1]),
                    ];
                    let mut new_lengths = lengths;
                    ui.horizontal(|ui| {
                        for (v, label) in new_lengths.iter_mut().zip(["폭 ", "높이 "]) {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(v)
                                        .speed(1.)
                                        .range(1.0..=1e7)
                                        .prefix(label)
                                        .suffix(" px"),
                                )
                                .changed();
                        }
                    });
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut angle)
                                    .speed(1.)
                                    .range(-360.0..=360.0)
                                    .prefix("회전 ")
                                    .suffix("°"),
                            )
                            .changed();
                        for (v, label) in center.iter_mut().zip(["중심 X ", "Y "]) {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(v)
                                        .speed(0.1)
                                        .range(-1e9..=1e9)
                                        .prefix(label),
                                )
                                .changed();
                        }
                    });
                    if changed && let Some(map) = crate::matrix::unit_quad(frame.quad) {
                        let f = [
                            new_lengths[0] as f64 / lengths[0] as f64,
                            new_lengths[1] as f64 / lengths[1] as f64,
                        ];
                        let scale = [
                            f[0],
                            0.,
                            (1. - f[0]) / 2.,
                            0.,
                            f[1],
                            (1. - f[1]) / 2.,
                            0.,
                            0.,
                            1.,
                        ];
                        let local = crate::matrix::multiply(
                            map,
                            crate::matrix::multiply(scale, crate::matrix::inverse(map).unwrap()),
                        );
                        let world = crate::matrix::multiply(
                            crate::matrix::translate(
                                (center[0] - old_center[0]) as f64,
                                (center[1] - old_center[1]) as f64,
                            ),
                            crate::matrix::multiply(
                                crate::transform_batch::rotation(old_center, angle - old_angle),
                                local,
                            ),
                        );
                        quad = frame.mapped(world);
                    }
                }
                ui.horizontal(|ui| {
                    for (axis, label) in [(0, "좌우 뒤집기"), (1, "상하 뒤집기")] {
                        if ui.button(label).clicked()
                            && let Some(map) = crate::matrix::unit_quad(quad)
                        {
                            let mut flip = crate::matrix::IDENTITY;
                            flip[axis * 3 + axis] = -1.;
                            flip[axis * 3 + 2] = 1.;
                            quad = quad.map(|p| {
                                crate::matrix::point(
                                    crate::matrix::multiply(
                                        map,
                                        crate::matrix::multiply(
                                            flip,
                                            crate::matrix::inverse(map).unwrap(),
                                        ),
                                    ),
                                    p,
                                )
                            });
                            changed = true;
                        }
                    }
                });
                if changed {
                    match frame.apply(&mut tab.editor, quad) {
                        Ok(changed) => {
                            frame.quad = quad;
                            tab.render_dirty |= changed;
                        }
                        Err(e) => error = Some(e),
                    }
                }
                tab.transform_frame = Some(frame);
            }
            ui.horizontal(|ui| {
                if ui.button("적용 · Enter").clicked() {
                    decision = Some(true);
                }
                if ui.button("취소 · Esc").clicked() {
                    decision = Some(false);
                }
            });
        });
        if let Some(error) = error {
            self.status = error;
        }
        if !open {
            decision = Some(false);
        }
        if let Some(apply) = decision {
            self.end_transform(apply);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.content_dialog.is_none() {
            self.svg_panel.close();
        }
        self.raw_ui(ctx);
        if let Some(dialog) = self.external_dialog.as_ref() {
            let id = dialog.tab;
            let path = dialog.path.clone();
            let snapshot = dialog.snapshot.clone();
            let saving = dialog.saving;
            let mut action = 0;
            egui::Modal::new(egui::Id::new("external-file")).show(ctx, |ui| {
                ui.set_min_width(380.);
                ui.heading("파일이 외부에서 변경됐습니다");
                ui.label(path.display().to_string());
                ui.label("현재 문서와 실행 취소 기록은 그대로 보존합니다.");
                if snapshot == crate::file_watch::Snapshot::Missing {
                    ui.label("파일이 이동되었거나 삭제됐습니다.");
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            snapshot != crate::file_watch::Snapshot::Missing,
                            egui::Button::new("새 버전을 별도 탭으로 열기"),
                        )
                        .clicked()
                    {
                        action = 1;
                    }
                    if ui.button("다른 이름으로 저장").clicked() {
                        action = 2;
                    }
                    if saving && ui.button("현재 편집으로 덮어쓰기").clicked() {
                        action = 3;
                    }
                    if ui.button("현재 문서 계속 편집").clicked() {
                        action = 4;
                    }
                });
            });
            if action != 0 {
                self.external_dialog = None;
                if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
                    if action != 4 {
                        self.active = index;
                    }
                    match action {
                        1 => self.open_file(Some(path), None, ctx),
                        2 => self.save(true, ctx),
                        3 => {
                            self.tabs[index].file_watch.reset(snapshot);
                            self.save(false, ctx);
                        }
                        _ => {}
                    }
                }
            }
        }

        if self.recovery_dialog {
            let mut restore = None;
            egui::Modal::new(egui::Id::new("recovery")).show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.heading("자동 저장 복구");
                ui.label("복구 사본을 새 문서로 엽니다. 원본 파일은 유지됩니다.");
                if self.recovery_entries.is_empty() {
                    ui.label("복구할 사본이 없습니다.");
                }
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        if !self.recovery_warnings.is_empty() {
                            ui.label(
                                "일부 복구 정보를 읽지 못했습니다. 해당 사본은 그대로 보관됩니다.",
                            );
                            for warning in &self.recovery_warnings {
                                ui.label(RichText::new(warning).small().weak());
                            }
                            ui.separator();
                        }
                        for entry in &self.recovery_entries {
                            ui.horizontal(|ui| {
                                ui.label(&entry.title);
                                if ui.button("복구").clicked() {
                                    restore = Some(entry.clone());
                                }
                            });
                            if let Some(path) = &entry.original {
                                ui.label(
                                    egui::RichText::new(path.display().to_string())
                                        .small()
                                        .weak(),
                                );
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    crate::numeric_slider::labeled_drag(
                        ui,
                        "자동 저장 간격",
                        &mut self.auto_interval,
                        15..=600,
                        1.,
                        " 초",
                    );
                });
                if ui.button("닫기").clicked() {
                    self.recovery_dialog = false;
                }
            });
            if let Some(entry) = restore
                && let Some(store) = &self.recovery
            {
                let store = store.clone();
                let sender = self.sender.clone();
                let ctx = ctx.clone();
                self.recovery_dialog = false;
                self.busy = Some("복구 사본 여는 중");
                std::thread::spawn(move || {
                    let result = store.restore(&entry.key);
                    let _ = sender.send(JobResult::Recover { result, entry });
                    ctx.request_repaint();
                });
            }
        }
        if let Some(large) = self.psd_dialog {
            egui::Modal::new(egui::Id::new("psd-save")).show(ctx,|ui|{
                ui.set_max_width((ctx.content_rect().width()-48.0).clamp(160.0,450.0));ui.heading(if large{"PSB 저장"}else{"PSD 저장"});
                egui::ScrollArea::vertical().id_salt("photoshop-save-options").max_height((ctx.content_rect().height()-140.0).max(96.0)).show(ui,|ui| {
                ui.label("픽셀 레이어·그룹, 순서, 이름, 위치, 표시, 잠금, 불투명도, 블렌드·클리핑과 픽셀 마스크를 저장합니다.");
                if self.tab().is_some_and(|t|crate::psd::has_incompatible_profile(&t.editor.document,self.psd_options.color)) {
                    ui.label("선택한 색 모드와 맞지 않는 원래 ICC 프로파일은 포함하지 않습니다. 원래 리소스는 .rphoto 프로젝트에 보존됩니다. 현재 ICC 변환은 적용되지 않습니다.");
                }
                if self.tab().is_some_and(|t|t.editor.document.layers.iter().any(|l|!l.psd_extra.is_empty())) {
                    ui.label(RichText::new("해석하지 못한 Photoshop 추가 레코드는 원본 값을 유지합니다. 편집 후 해당 레코드의 유효성은 아직 검증되지 않았으므로 .rphoto 프로젝트와 원본 파일을 함께 보관하세요.").color(Color32::from_rgb(231,190,120)));
                }
                if self.tab().is_some_and(|t|t.editor.document.layers.iter().any(|l|l.content.is_some()||l.adjustments!=Default::default()||l.effects.enabled()||l.transform!=Default::default())) {
                    ui.label(RichText::new("현재 텍스트·도형·그라디언트·보정·효과·변형은 외관을 유지한 픽셀 레이어로 변환됩니다. 편집 설정을 보존하려면 .rphoto 프로젝트도 저장하세요.").color(Color32::from_rgb(231,190,120)));
                }
                if self.tab().is_some_and(|t|t.editor.document.layers.iter().any(|l|l.group&&(l.effects.enabled()||l.adjustments!=Default::default()))) {
                    ui.label(RichText::new("보정·효과가 적용된 그룹은 그룹 내부의 합성 결과를 픽셀 레이어로 변환합니다.").color(Color32::from_rgb(231,190,120)));
                }
                if self.tab().is_some_and(|t|t.editor.document.layers.iter().any(|l|l.raw_source.is_some())) {ui.label("RAW 원본과 현상 설정은 .rphoto에 보존합니다. PSD/PSB에는 현재 현상한 픽셀을 저장합니다.");}
                if self.tab().is_some_and(|t|t.editor.document.layers.iter().any(|l|l.adjustment_layer)) {ui.label("별도 조정 레이어가 있는 문서는 현재 외관을 유지한 전체 합성 하나의 픽셀 레이어로 저장합니다. 조정·마스크·원본 편집 구조는 .rphoto에 보존됩니다.");}
                if self.tab().is_some_and(|t|t.editor.document.layers.iter().any(|l|crate::adjustment_scope::needs_backdrop(&t.editor.document,l))) {ui.label("선택 영역 보정이 다른 레이어와 블렌드되는 통과 그룹에 있으면 외관을 유지하도록 전체 합성을 하나의 픽셀 레이어로 저장합니다. 편집 구조는 .rphoto에 보존됩니다.");}
                if self.tab().is_some_and(|t|t.editor.document.layers.iter().any(|l|l.mask_placement.is_some())) {ui.label("독립 마스크 위치·연결·변형은 출력 외관에 적용한 8비트 픽셀 마스크로 변환합니다. 원본 마스크와 독립 편집 설정은 .rphoto에 보존됩니다.");}
                if self.tab().is_some_and(|t|t.editor.document.layers.iter().any(|l|l.bit_depth>8)) {
                    ui.label("문서의 16/32비트 정밀도로 저장합니다. 32비트 출력은 HDR 값을 보존하며, 16비트 출력은 0–1 범위로 양자화됩니다.");
                }
                if self.tab().is_some_and(|t|!t.editor.document.layout.guides.is_empty()) {
                    ui.label("가이드 위치는 Photoshop 가이드 리소스에 1/32px 단위로 저장합니다. 눈금자 표시·스냅·격자 세분화 설정은 .rphoto에 보존됩니다.");
                }
                photoshop_output_ui(ui,&mut self.psd_options);
                ui.label("모든 압축은 무손실입니다. 큰 문서의 RLE 행 제한에 걸리면 ZIP/ZIP 예측 또는 PSB를 선택하세요.");
                });
                ui.horizontal(|ui|{
                    if ui.button("저장 진행").clicked(){self.psd_dialog=None;self.export_psd(large,ctx);}
                    if ui.button("취소").clicked(){self.psd_dialog=None;}
                });
            });
        }
        if let Some(resample) = self.size_dialog {
            egui::Modal::new(egui::Id::new("resize")).show(ctx, |ui| {
                ui.heading(if resample {
                    "이미지 크기"
                } else {
                    "캔버스 크기"
                });
                ui.horizontal(|ui| {
                    crate::numeric_slider::labeled_drag(
                        ui,
                        "폭",
                        &mut self.resize_width,
                        1..=100_000,
                        1.,
                        " px",
                    );
                });
                ui.horizontal(|ui| {
                    crate::numeric_slider::labeled_drag(
                        ui,
                        "높이",
                        &mut self.resize_height,
                        1..=100_000,
                        1.,
                        " px",
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button("적용").clicked() {
                        let (w, h) = (self.resize_width, self.resize_height);
                        self.apply_result("크기 변경", |doc| {
                            editing::resize(doc, w, h, resample)
                        });
                        self.size_dialog = None;
                        if let Some(tab) = self.tab_mut() {
                            tab.view.fit = true;
                            tab.selection = None;
                        }
                    }
                    if ui.button("취소").clicked() {
                        self.size_dialog = None;
                    }
                });
            });
        }
        if let Some((tab_id, layer_id)) = self.adjust_dialog
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
        {
            let mut action = 0;
            let mut auto_group = false;
            let backdrop_dependent = tab
                .editor
                .document
                .layers
                .iter()
                .find(|l| l.id == layer_id)
                .is_some_and(|layer| {
                    crate::adjustment_scope::needs_backdrop(&tab.editor.document, layer)
                });
            if let Some(layer) = tab
                .editor
                .document
                .layers
                .iter_mut()
                .find(|l| l.id == layer_id)
            {
                egui::Modal::new(egui::Id::new("adjustments")).show(ctx, |ui| {
                    ui.set_min_width(360.0);
                    ui.heading("사진 보정·필터");
                    ui.label("원본을 유지하며 미리보기합니다.");
                    let before = layer.adjustments.clone();
                    let mut restricted=layer.adjustments.scope.is_some();
                    if ui.add_enabled(self.adjust_selection_snapshot.is_some() || self.adjust_saved_scope.is_some(),egui::Checkbox::new(&mut restricted,"선택 영역에만 적용")).changed(){
                        if restricted {
                            layer.adjustments.scope=self.adjust_saved_scope.clone();
                            if layer.adjustments.scope.is_none() && let Some(selection)=&self.adjust_selection_snapshot {
                                match crate::adjustment_scope::Scope::capture(layer,selection,[tab.editor.document.width,tab.editor.document.height]) {
                                    Ok(scope)=>layer.adjustments.scope=Some(scope),Err(error)=>self.status=error
                                }
                            }
                        } else {layer.adjustments.scope=None;}
                    }
                    if restricted {
                        ui.label("선택을 해제해도 보정 범위는 레이어와 함께 유지됩니다.");
                        if ui.add_enabled(self.adjust_selection_snapshot.is_some(),egui::Button::new("현재 선택으로 범위 변경")).clicked()
                            && let Some(selection)=&self.adjust_selection_snapshot
                        {
                            match crate::adjustment_scope::Scope::capture(layer,selection,[tab.editor.document.width,tab.editor.document.height]) {
                                Ok(scope)=>{self.adjust_saved_scope=Some(scope.clone());layer.adjustments.scope=Some(scope);},Err(error)=>self.status=error
                            }
                        }
                    }
                    if layer.adjustment_layer {ui.label("같은 그룹에서 아래 레이어의 합성에 적용합니다. 통과 그룹에서는 그룹 아래 합성에도 적용하며, 클리핑을 켜면 바로 아래 기준 레이어에만 적용합니다.");}
                    if layer.group {ui.label("원본 픽셀에 적용하면 그룹 내부 합성을 하나의 픽셀 레이어로 변환합니다.");}
                    let raw_info=layer.raw_source.as_ref().map(|source|source.record.info.clone());
                    let a = &mut layer.adjustments;
                    egui::ScrollArea::vertical()
                        .id_salt("adjustment-controls")
                        .max_height(440.0)
                        .show(ui, |ui| {
                            ui.checkbox(&mut a.expand_filters,"레이어 경계 밖까지 확산").on_hover_text("가우시안·모션 블러와 블룸의 외곽을 유지합니다. 문서 경계와 마스크는 그대로 적용합니다.");
                            if ui.button("자동 레벨").clicked() {
                                if layer.group || layer.adjustment_layer {
                                    auto_group = true;
                                } else {
                                    let source = layer.precision.as_ref().map_or_else(
                                        || {
                                            image::DynamicImage::ImageRgba8((*layer.pixels).clone())
                                                .into_rgba32f()
                                        },
                                        |p| (**p).clone(),
                                    );
                                    a.auto_levels(&source);
                                }
                            }
                            a.tonal_contrast.ui(ui);
                            a.color_mixer.ui(ui);
                            a.color_grading.ui(ui);
                            a.denoise.ui(ui);
                            a.detail_sharpen.ui(ui);
                            crate::lens_profile::ui(&mut a.lens_profile,raw_info.as_ref(),ui);
                            a.geometry.ui(ui);
                            for (value, label, range) in [
                                (&mut a.brightness, "밝기", -1.0..=1.0),
                                (&mut a.contrast, "대비", -1.0..=2.0),
                                (&mut a.exposure, "노출", -5.0..=5.0),
                                (&mut a.saturation, "채도", -1.0..=2.0),
                                (&mut a.hue, "색조", -180.0..=180.0),
                                (&mut a.temperature, "색온도", -1.0..=1.0),
                                (&mut a.tint, "틴트", -1.0..=1.0),
                                (&mut a.gamma, "감마", 0.1..=5.0),
                                (&mut a.black, "블랙 레벨", 0.0..=0.95),
                                (&mut a.white, "화이트 레벨", 0.05..=1.0),
                                (&mut a.blur, "가우시안 블러", 0.0..=30.0),
                                (&mut a.sharpen, "선명화", 0.0..=5.0),
                                (&mut a.grain, "그레인", 0.0..=1.0),
                                (&mut a.noise, "색상 노이즈", 0.0..=1.0),
                                (&mut a.vignette, "비네트", -1.0..=1.0),
                            ] {
                                ui.add(crate::numeric_slider::Slider::new(value, range).text(label));
                            }

                            a.color_balance.ui(ui);
                            a.bloom.ui(ui);
                            a.lens.ui(ui);
                            a.white = a.white.max(a.black + 0.01);
                            ui.add(
                                crate::numeric_slider::Slider::new(&mut a.motion_length, 0..=100)
                                    .text("모션 블러 px"),
                            );
                            ui.add(
                                crate::numeric_slider::Slider::new(&mut a.motion_angle, -180.0..=180.0)
                                    .text("모션 방향 °"),
                            );
                            ui.checkbox(&mut a.gradient_map, "그라디언트 맵");
                            if a.gradient_map {
                                ui.horizontal(|ui| {
                                    ui.label("어두운 색");
                                    ui.color_edit_button_srgba_unmultiplied(
                                        &mut a.gradient_colors[0],
                                    );
                                    ui.label("밝은 색");
                                    ui.color_edit_button_srgba_unmultiplied(
                                        &mut a.gradient_colors[1],
                                    );
                                });
                            }
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label("커브");
                                for (i, name) in ["RGB", "R", "G", "B"].iter().enumerate() {
                                    if ui
                                        .selectable_value(&mut self.curve_channel, i, *name)
                                        .clicked()
                                    {
                                        self.curve_drag = None;
                                    }
                                }
                                if ui.button("커브 초기화").clicked() {
                                    a.curves[self.curve_channel] = Default::default();
                                    self.curve_drag = None;
                                }
                            });
                            curve_editor(
                                ui,
                                &mut a.curves[self.curve_channel],
                                &mut self.curve_drag,
                            );
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut a.monochrome, "흑백");
                                ui.checkbox(&mut a.invert, "색상 반전");
                                if ui.button("초기화").clicked() {
                                    let scope=a.scope.take();
                                    *a = Default::default();
                                    a.scope=scope;
                                    self.curve_drag = None;
                                }
                            });
                        });
                    if *a != before {
                        tab.render_dirty = true;
                        ctx.request_repaint();
                    }
                    let filter_valid=crate::filter_bounds::valid(layer);
                    if !filter_valid { ui.colored_label(egui::Color32::YELLOW,"필터 외곽의 크기 또는 원근 변형이 한계를 넘습니다. 범위를 줄이세요."); }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(filter_valid, egui::Button::new("적용"))
                            .clicked()
                        {
                            action = 1;
                        }
                        if ui.add_enabled(filter_valid && !backdrop_dependent && !layer.adjustment_layer,egui::Button::new("원본 픽셀에 적용")).on_hover_text(if layer.adjustment_layer {"조정 레이어는 아래 레이어와 병합하거나 보정 설정으로 유지하세요."} else if backdrop_dependent {"다른 레이어와 블렌드되는 통과 그룹은 보정 설정으로 유지하거나 그룹 통과를 끈 뒤 원본에 적용하세요."}else{"원본 픽셀에 보정 결과를 적용합니다."}).clicked(){action=3;}
                        if ui.button("취소").clicked() {
                            action = 2;
                        }
                    });
                });
            }
            if auto_group {
                let mut source_doc = tab.editor.document.clone();
                source_doc.active = layer_id;
                if let Some(layer) = source_doc.active_layer_mut() {
                    layer.adjustments = Default::default();
                    layer.effects = Default::default();
                    layer.opacity = 1.0;
                }
                let source = if source_doc
                    .active_layer()
                    .is_some_and(|l| l.adjustment_layer)
                {
                    crate::adjustment_layer::input(&source_doc, layer_id)
                } else {
                    editing::copy_pixels(&source_doc, None, false).into_rgba32f()
                };
                if let Some(layer) = tab
                    .editor
                    .document
                    .layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                {
                    layer.adjustments.auto_levels(&source);
                }
                tab.render_dirty = true;
            }
            if action == 3 {
                match crate::adjustment_scope::bake(&mut tab.editor.document, layer_id) {
                    Ok(()) => action = 1,
                    Err(error) => {
                        self.status = error;
                        action = 0;
                    }
                }
            }
            if action > 0 {
                if action == 1 {
                    tab.editor.finish();
                } else {
                    tab.editor.cancel();
                }
                tab.render_dirty = true;
                self.adjust_dialog = None;
                self.adjust_selection_snapshot = None;
                self.adjust_saved_scope = None;
            }
        }
        if let Some((tab_id, layer_id, offset, mut content)) = self.content_dialog.clone() {
            let mut action = 0;
            let mut reverse_gradient = false;
            let modal = egui::Modal::new(egui::Id::new("vector-content"));
            let modal = if matches!(&content, Content::Svg { .. }) {
                modal.area(
                    egui::Modal::default_area(egui::Id::new("vector-content"))
                        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0., 40.)),
                )
            } else {
                modal
            };
            modal.show(ctx, |ui| {
                ui.set_min_width(440.0);
                ui.heading("요소 편집");
                if matches!(&content, Content::Svg { .. }) {
                    ui.set_width(440.);
                }
                match &mut content {
                    Content::Solid { color, .. } => self.fill_panel.color_ui(ui, color),
                    Content::Text { .. } => {
                        crate::rich_text::edit_ui(
                            ui,
                            &mut content,
                            &mut self.text_selection,
                            &mut self.text_style,
                        );
                    }
                    Content::Shape {
                        width,
                        height,
                        fill,
                        stroke,
                        thickness,
                        ..
                    } => {
                        ui.color_edit_button_srgba_unmultiplied(fill);
                        ui.color_edit_button_srgba_unmultiplied(stroke);
                        ui.add(
                            crate::numeric_slider::Slider::new(thickness, 0.0..=100.0)
                                .text("선 두께"),
                        );
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(width).range(1..=10000).prefix("폭 "));
                            ui.add(
                                egui::DragValue::new(height)
                                    .range(1..=10000)
                                    .prefix("높이 "),
                            );
                        });
                    }
                    Content::Gradient {
                        colors,
                        start,
                        end,
                        stops,
                        interpolation,
                        ..
                    } => {
                        gradient_interpolation_ui(ui, interpolation);
                        if ui.button("방향 반전").clicked() {
                            reverse_gradient = true;
                        }
                        if stops.is_empty() {
                            *stops = vec![
                                crate::vector::GradientStop {
                                    position: 0.0,
                                    color: colors[0],
                                },
                                crate::vector::GradientStop {
                                    position: 1.0,
                                    color: colors[1],
                                },
                            ];
                        }
                        let mut remove = None;
                        let can_remove = stops.len() > 2;
                        egui::ScrollArea::vertical()
                            .max_height(240.0)
                            .show(ui, |ui| {
                                for (index, stop) in stops.iter_mut().enumerate() {
                                    ui.push_id(index, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                crate::numeric_slider::Slider::new(
                                                    &mut stop.position,
                                                    0.0..=1.0,
                                                )
                                                .text("위치"),
                                            );
                                            let mut candidate = stop.color;
                                            if ui
                                                .color_edit_button_srgba_unmultiplied(
                                                    &mut candidate,
                                                )
                                                .changed()
                                            {
                                                stop.color = candidate;
                                            }
                                            if ui
                                                .add_enabled(
                                                    can_remove,
                                                    egui::Button::new("−").small(),
                                                )
                                                .clicked()
                                            {
                                                remove = Some(index);
                                            }
                                        })
                                    });
                                }
                            });
                        if let Some(index) = remove
                            && stops.len() > 2
                        {
                            stops.remove(index);
                        }
                        if ui
                            .add_enabled(stops.len() < 256, egui::Button::new("색상점 추가"))
                            .clicked()
                        {
                            stops.push(crate::vector::GradientStop {
                                position: 0.5,
                                color: self.color,
                            });
                        }
                        for (label, point) in [("시작", start), ("끝", end)] {
                            ui.horizontal(|ui| {
                                ui.label(label);
                                ui.add(egui::DragValue::new(&mut point[0]).prefix("X "));
                                ui.add(egui::DragValue::new(&mut point[1]).prefix("Y "));
                            });
                        }
                    }
                    Content::Svg { source } => {
                        if crate::pattern::Pattern::read(source).is_some() {
                            let selected = self.tab().is_some_and(|t| t.selection.is_some());
                            if let Some(action) = self.pattern_panel.ui(
                                ui,
                                source,
                                [self.color, self.background],
                                self.clipboard.is_some(),
                                selected,
                            ) {
                                let tile = match action {
                                    crate::pattern::Source::Clipboard => {
                                        self.clipboard.as_ref().map(|image| image.to_rgba8())
                                    }
                                    crate::pattern::Source::Selection => self.tab().map(|tab| {
                                        editing::copy_pixels(
                                            &tab.editor.document,
                                            tab.selection.as_ref(),
                                            true,
                                        )
                                        .to_rgba8()
                                    }),
                                };
                                if let Some(tile) = tile
                                    && let Some(mut pattern) = crate::pattern::Pattern::read(source)
                                {
                                    match pattern
                                        .replace_tile(&tile)
                                        .and_then(|()| pattern.source())
                                    {
                                        Ok(next) => *source = next,
                                        Err(error) => {
                                            self.message =
                                                Some(("패턴 원본 변경 실패".into(), error))
                                        }
                                    }
                                }
                            }
                            ui.label("패턴 타일은 8비트 RGBA PNG로 보존합니다.");
                            if layer_id.is_none() && self.fill_mask.is_some() {
                                ui.checkbox(
                                    &mut self.fill_restrict,
                                    "처음 선택한 영역으로 마스크 만들기",
                                );
                            }
                        } else {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, true])
                                .id_salt("svg-properties-scroll")
                                .max_height((ctx.content_rect().height() - 180.).max(120.))
                                .show(ui, |ui| self.svg_panel.ui(ui, source));
                        }
                    }
                    Content::Path {
                        closed,
                        fill,
                        stroke,
                        thickness,
                        ..
                    } => {
                        ui.label("펜 도구의 ‘점 편집’에서 앵커와 핸들을 드래그하세요.");
                        ui.checkbox(closed, "닫힌 경로");
                        ui.label("채우기");
                        ui.color_edit_button_srgba_unmultiplied(fill);
                        ui.label("선");
                        ui.color_edit_button_srgba_unmultiplied(stroke);
                        ui.add(
                            crate::numeric_slider::Slider::new(thickness, 0.1..=100.0)
                                .text("선 두께"),
                        );
                    }
                }
                if matches!(content, Content::Solid { .. } | Content::Gradient { .. }) {
                    self.fill_panel.preview(ui, &content);
                    if layer_id.is_none() && self.fill_mask.is_some() {
                        ui.checkbox(
                            &mut self.fill_restrict,
                            "처음 선택한 영역으로 마스크 만들기",
                        );
                    }
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.svg_panel.busy(), egui::Button::new("적용"))
                        .clicked()
                    {
                        action = 1;
                    }
                    if ui.button("취소").clicked() {
                        action = 2;
                    }
                });
            });
            if reverse_gradient {
                if let Content::Gradient { stops, .. } = &mut content {
                    stops.sort_by(|a, b| a.position.total_cmp(&b.position));
                }
                content.reverse_gradient();
            }
            if action == 1 || action == 2 {
                self.svg_panel.close();
            }
            if action == 1 {
                if let Content::Gradient { colors, stops, .. } = &mut content {
                    stops.sort_by(|a, b| a.position.total_cmp(&b.position));
                    *colors = [stops.first().unwrap().color, stops.last().unwrap().color];
                }
                if let Some(index) = self.tabs.iter().position(|t| t.id == tab_id) {
                    self.active = index;
                }
                if let Some(layer_id) = layer_id {
                    match content.render() {
                        Ok(pixels) => self.apply_result("요소 편집", |doc| {
                            let Some(layer) = doc.layers.iter_mut().find(|l| l.id == layer_id)
                            else {
                                return Err("레이어를 찾을 수 없습니다.".into());
                            };
                            let old_size = [layer.pixels.width(), layer.pixels.height()];
                            let mask_world = layer
                                .mask_placement
                                .as_ref()
                                .map(|p| (crate::masks::world(layer), p.linked, p.sampling));
                            layer.pixels = std::sync::Arc::new(pixels);
                            layer.precision = None;
                            layer.bit_depth = 8;
                            layer.content = Some(content);
                            if layer.mask_placement.is_none()
                                && let Some(mask) = &layer.mask
                            {
                                layer.mask = Some(std::sync::Arc::new(image::imageops::resize(
                                    &**mask,
                                    layer.pixels.width(),
                                    layer.pixels.height(),
                                    image::imageops::FilterType::Triangle,
                                )));
                            }
                            if let Some((matrix, linked, sampling)) = mask_world {
                                let scale = [
                                    layer.pixels.width() as f64 / old_size[0] as f64,
                                    0.,
                                    0.,
                                    0.,
                                    layer.pixels.height() as f64 / old_size[1] as f64,
                                    0.,
                                    0.,
                                    0.,
                                    1.,
                                ];
                                let matrix = if linked {
                                    let old = layer.transform.mapping(layer.offset, old_size);
                                    crate::matrix::multiply(
                                        old,
                                        crate::matrix::multiply(
                                            scale,
                                            crate::matrix::multiply(
                                                crate::matrix::inverse(old)
                                                    .ok_or("레이어 역행렬이 없습니다.")?,
                                                matrix,
                                            ),
                                        ),
                                    )
                                } else {
                                    matrix
                                };
                                crate::masks::set_world(layer, matrix, linked, sampling)?;
                            }
                            if doc.pixel_count() > document::MAX_PIXELS {
                                return Err("요소가 문서 메모리 예산을 초과합니다.".into());
                            }
                            Ok(())
                        }),
                        Err(error) => self.message = Some(("요소 편집 실패".into(), error)),
                    }
                } else if matches!(content, Content::Solid { .. } | Content::Gradient { .. }) {
                    let selection = if self.fill_restrict {
                        self.fill_mask.clone()
                    } else {
                        None
                    };
                    self.apply_result("채우기 레이어", |doc| {
                        crate::fill::add(doc, content, selection.as_ref())
                    });
                } else if matches!(&content,Content::Svg{source} if crate::pattern::Pattern::read(source).is_some())
                {
                    let selection = if self.fill_restrict {
                        self.fill_mask.clone()
                    } else {
                        None
                    };
                    self.apply_result("패턴 채우기", |doc| {
                        crate::pattern::add(doc, content, selection.as_ref())
                    });
                } else {
                    self.add_content(content, offset);
                }
                self.fill_mask = None;
                self.content_dialog = None;
            } else if action == 2 {
                self.fill_mask = None;
                self.content_dialog = None;
            } else {
                self.content_dialog = Some((tab_id, layer_id, offset, content));
            }
        }
        if self.new_dialog {
            egui::Modal::new(egui::Id::new("new-document")).show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.heading("새 문서");
                ui.separator();
                ui.horizontal(|ui| {
                    crate::numeric_slider::labeled_drag(
                        ui,
                        "폭",
                        &mut self.new_width,
                        1..=100_000,
                        1.,
                        " px",
                    );
                });
                ui.horizontal(|ui| {
                    crate::numeric_slider::labeled_drag(
                        ui,
                        "높이",
                        &mut self.new_height,
                        1..=100_000,
                        1.,
                        " px",
                    );
                });
                ui.checkbox(&mut self.new_transparent, "투명 배경");
                egui::ComboBox::from_label("RGB 정밀도")
                    .selected_text(format!("{}비트", self.new_depth))
                    .show_ui(ui, |ui| {
                        for depth in [8, 16, 32] {
                            ui.selectable_value(&mut self.new_depth, depth, format!("{depth}비트"));
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("만들기").clicked() {
                        match Document::new(self.new_width, self.new_height, self.new_transparent) {
                            Ok(mut document) => {
                                if self.new_depth > 8 {
                                    let layer = document.active_layer_mut().unwrap();
                                    layer.precision = Some(std::sync::Arc::new(
                                        image::DynamicImage::ImageRgba8((*layer.pixels).clone())
                                            .into_rgba32f(),
                                    ));
                                    layer.bit_depth = self.new_depth;
                                }
                                self.add_tab(files::Opened {
                                    source_stamp: None,
                                    document,
                                    project_path: None,
                                    title: format!("제목 없음 {}", self.next_tab),
                                    notes: vec![],
                                });
                                self.new_dialog = false;
                            }
                            Err(error) => self.message = Some(("문서 생성 실패".into(), error)),
                        }
                    }
                    if ui.button("취소").clicked() {
                        self.new_dialog = false;
                    }
                });
            });
        }
        if !self.export_dialog {
            self.export_preview.close();
        }
        if self.export_dialog {
            egui::Modal::new(egui::Id::new("export")).show(ctx, |ui| {
                ui.set_min_width(320.0);
                ui.heading("이미지 내보내기");
                ui.separator();
                let height=(ctx.content_rect().height()-160.).max(200.);
                ui.horizontal_top(|ui| {
                ui.vertical(|ui| {ui.set_min_width(340.);ui.set_max_width(360.);
                egui::ScrollArea::vertical().id_salt("export-options-scroll").max_height(height).show(ui,|ui| {
                egui::ComboBox::from_label("형식")
                    .selected_text(files::export_label(self.export_format).unwrap_or("출력 형식"))
                    .show_ui(ui, |ui| {
                        for (i, (_, name, _)) in files::EXPORT_FORMATS.iter().enumerate() {
                            ui.selectable_value(&mut self.export_format, i, *name);
                        }
                        ui.selectable_value(&mut self.export_format, files::EXPORT_FORMATS.len(), "SVG");
                        ui.selectable_value(&mut self.export_format, files::EXPORT_FORMATS.len()+1, "PCX");
                        ui.selectable_value(&mut self.export_format, files::EXPORT_FORMATS.len()+2, "SVGZ");
                        for (i,(label,_)) in crate::jpeg2000_output::FORMATS.iter().enumerate() {
                            ui.selectable_value(&mut self.export_format,files::EXPORT_FORMATS.len()+3+i,*label);
                        }
                    });
                let svgz=self.export_format==files::EXPORT_FORMATS.len()+2;
        let svg = self.export_format == files::EXPORT_FORMATS.len() || svgz;
                let pcx=self.export_format==files::EXPORT_FORMATS.len()+1;
                let jp2 = self.export_format >= files::EXPORT_FORMATS.len()+3;
                if !svg && !pcx && !jp2 && files::EXPORT_FORMATS[self.export_format].0 == image::ImageFormat::Jpeg {
                    ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.quality, 1..=100).text("품질"));
                }
                let format=if svg || pcx || jp2 { image::ImageFormat::Png } else { files::EXPORT_FORMATS[self.export_format].0 };
                if jp2 {
                    ui.horizontal(|ui|{
                        ui.selectable_value(&mut self.export_options.jp2_grayscale,false,"RGB");
                        ui.selectable_value(&mut self.export_options.jp2_grayscale,true,"회색조");
                    });
                    if self.export_options.jp2_grayscale { ui.label("현재 RGB 값의 밝기로 변환합니다. 회색조 색상 프로파일 변환은 적용되지 않습니다."); }
                    ui.checkbox(&mut self.export_options.jp2_lossless,"무손실 압축");
                    ui.horizontal(|ui|{
                        for (value,label) in [(0,"자동"),(8,"8비트"),(16,"16비트")] { ui.selectable_value(&mut self.export_options.jp2_depth,value,label); }
                    });
                    if !self.export_options.jp2_lossless {
                        ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.jp2_ratio,2..=256).text("압축 배율"));
                        ui.label("배율이 높을수록 파일이 작아지고 색상·투명도의 손실이 커질 수 있습니다.");
                    }
                    egui::ComboBox::from_id_salt("jp2-progression")
                        .selected_text(crate::jpeg2000_output::PROGRESSION_LABELS[self.export_options.jp2_progression])
                        .show_ui(ui,|ui|{for (i,label) in crate::jpeg2000_output::PROGRESSION_LABELS.iter().enumerate(){ui.selectable_value(&mut self.export_options.jp2_progression,i,*label);}});
                    ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.jp2_layers,1..=100).text("품질 단계"));
                    egui::ComboBox::from_id_salt("jp2-tile")
                        .selected_text(if self.export_options.jp2_tile == 0 { "분할 없음".into() } else {format!("{}픽셀",self.export_options.jp2_tile)})
                        .show_ui(ui,|ui|{for size in crate::jpeg2000_output::TILE_SIZES {ui.selectable_value(&mut self.export_options.jp2_tile,*size,if *size==0 {"분할 없음".into()}else{format!("{size}픽셀")});}});
                    ui.label("타일로 나누면 다른 뷰어가 부분 영역을 읽기 쉬워집니다. 손실 압축에서는 타일 경계가 보일 수 있습니다.");
                    ui.label("여러 품질 단계는 다른 뷰어가 낮은 화질부터 표시할 수 있게 합니다. 단계가 많으면 파일 용량이 늘어날 수 있습니다.");
                    if self.export_format==files::EXPORT_FORMATS.len()+3 {
                        ui.checkbox(&mut self.export_options.jp2_icc,"작업 색상 프로파일 포함");
                        ui.checkbox(&mut self.export_options.jp2_resolution_enabled,"출력 해상도 정보 포함");
                        if self.export_options.jp2_resolution_enabled {
                            ui.horizontal(|ui|{crate::numeric_slider::labeled_drag(ui,"해상도 (ppi)",&mut self.export_options.jp2_resolution,1..=65535,1.,"");});
                            ui.label("표시·인쇄 크기 정보만 저장합니다. 이미지 픽셀 수는 바뀌지 않습니다.");
                        }
                    }
                    ui.label(if self.export_format==files::EXPORT_FORMATS.len()+4 {"J2K는 색상·알파 픽셀을 저장하는 코드스트림입니다. 색상 프로파일과 메타데이터는 포함하지 않습니다."}else if self.export_options.jp2_grayscale {"JP2는 회색조 색상 정보와 투명도 채널을 함께 저장합니다."}else{"JP2는 RGB 색상 정보와 투명도 채널을 함께 저장합니다."});
                }
                if self.export_format < files::EXPORT_FORMATS.len() && matches!(format,image::ImageFormat::Png|image::ImageFormat::Jpeg|image::ImageFormat::Tiff|image::ImageFormat::WebP) {
                    ui.checkbox(&mut self.export_options.include_icc,"작업 sRGB 프로파일 포함");
                }
                if (self.export_format < files::EXPORT_FORMATS.len() && matches!(format,image::ImageFormat::Png|image::ImageFormat::Jpeg|image::ImageFormat::WebP|image::ImageFormat::Tiff)) || self.export_format == files::EXPORT_FORMATS.len()+3 {
                    ui.checkbox(&mut self.export_options.include_xmp,"촬영 XMP 메타데이터 포함");
                }
                if self.export_format < files::EXPORT_FORMATS.len() && matches!(format,image::ImageFormat::Png|image::ImageFormat::Jpeg|image::ImageFormat::WebP) {
                    ui.checkbox(&mut self.export_options.include_exif,"촬영 EXIF 메타데이터 포함");
                }
                if format==image::ImageFormat::Jpeg {
                    egui::ComboBox::from_id_salt("jpeg-sampling").selected_text(crate::jpeg_output::SAMPLING_LABELS[self.export_options.jpeg_sampling]).show_ui(ui,|ui|{for (i,label) in crate::jpeg_output::SAMPLING_LABELS.iter().enumerate(){ui.selectable_value(&mut self.export_options.jpeg_sampling,i,*label);}});
                    ui.label("4:4:4는 색상 경계를 더 선명하게 보존하고 4:2:0은 보통 파일이 작습니다.");
                    ui.checkbox(&mut self.export_options.jpeg_progressive,"프로그레시브 저장");
                    if self.export_options.jpeg_progressive {ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.jpeg_scans,2..=64).text("스캔 수"));}
                    ui.checkbox(&mut self.export_options.jpeg_optimize,"압축 최적화");
                    ui.horizontal(|ui|{crate::numeric_slider::labeled_drag(ui,"해상도 (ppi)",&mut self.export_options.jpeg_resolution,1..=65535,1.,"");});
                    ui.collapsing("추가 설정",|ui|{ui.checkbox(&mut self.export_options.jpeg_average,"색차 평균 샘플");ui.horizontal(|ui|{crate::numeric_slider::labeled_drag(ui,"재시작 간격",&mut self.export_options.jpeg_restart,0..=65535,1.,"");ui.label("0: 없음");});});
                }
                if format==image::ImageFormat::Gif {
                    ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.gif_colors,2..=256).text("색상 수"));
                    ui.checkbox(&mut self.export_options.gif_dither,"디더링");
                    ui.checkbox(&mut self.export_options.gif_transparent,"투명 영역 유지");
                    if self.export_options.gif_transparent {ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.gif_alpha,1..=255).text("투명도 경계"));}
                    ui.checkbox(&mut self.export_options.gif_interlaced,"인터레이스 저장");
                    ui.label("GIF는 2–256색과 완전 투명/불투명을 사용합니다. 반투명 색은 배경색과 합성합니다.");
                }
                if format==image::ImageFormat::Avif {
                    ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.avif_quality,1..=100).text("색상 품질"));
                    ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.avif_alpha_quality,1..=100).text("투명도 품질"));
                    ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.avif_speed,1..=10).text("속도"));
                    ui.horizontal(|ui|{ui.selectable_value(&mut self.export_options.avif_depth,8,"8비트");ui.selectable_value(&mut self.export_options.avif_depth,10,"10비트");});
                    egui::ComboBox::from_id_salt("avif-color").selected_text(crate::avif_output::COLOR_LABELS[self.export_options.avif_color]).show_ui(ui,|ui|{for (i,label) in crate::avif_output::COLOR_LABELS.iter().enumerate(){ui.selectable_value(&mut self.export_options.avif_color,i,*label);}});
                    ui.horizontal(|ui|{crate::numeric_slider::labeled_drag(ui,"작업 스레드",&mut self.export_options.avif_threads,0..=256,1.,"");ui.label("0: 자동");});
                    ui.label("속도가 낮을수록 저장 시간이 늘어날 수 있습니다. HDR/음수 RGB는 SDR 범위로 변환합니다.");
                }
                if format==image::ImageFormat::Tiff {
                    ui.checkbox(&mut self.export_options.tiff_grayscale,"회색조");
                    if self.export_options.tiff_grayscale {ui.label("회색조로 변환하며 투명도와 선택한 정밀도를 유지합니다.");}
                    egui::ComboBox::from_id_salt("tiff-depth").selected_text(crate::tiff_output::DEPTH_LABELS[self.export_options.tiff_depth]).show_ui(ui,|ui|{for (i,label) in crate::tiff_output::DEPTH_LABELS.iter().enumerate(){ui.selectable_value(&mut self.export_options.tiff_depth,i,*label);}});
                    egui::ComboBox::from_id_salt("tiff-compression").selected_text(crate::tiff_output::COMPRESSION_LABELS[self.export_options.tiff_compression]).show_ui(ui,|ui|{for (i,label) in crate::tiff_output::COMPRESSION_LABELS.iter().enumerate(){ui.selectable_value(&mut self.export_options.tiff_compression,i,*label);}});
                    let float=crate::tiff_output::depth(&self.tab().unwrap().editor.document,&self.export_options)==32;
                    let prediction=(1..=4).contains(&self.export_options.tiff_compression);
                    if !prediction {self.export_options.tiff_predictor=false;self.export_options.tiff_float_predictor=false;ui.label("예측은 LZW·Deflate 압축에서 사용합니다.");}
                    if float {self.export_options.tiff_predictor=false;ui.label("HDR 밝기와 음수 RGB를 부동소수점으로 보존합니다.");}
                    else {self.export_options.tiff_float_predictor=false;}
                    ui.add_enabled(float && prediction,egui::Checkbox::new(&mut self.export_options.tiff_float_predictor,"부동소수점 예측"));
                    ui.add_enabled(!float && prediction,egui::Checkbox::new(&mut self.export_options.tiff_predictor,"수평 예측"));
                    ui.checkbox(&mut self.export_options.tiff_big,"BigTIFF");
                    ui.horizontal(|ui|{crate::numeric_slider::labeled_drag(ui,"해상도 (ppi)",&mut self.export_options.tiff_resolution,1..=9600,1.,"");});
                }
                if format==image::ImageFormat::WebP {
                    ui.horizontal(|ui|{ui.selectable_value(&mut self.export_options.webp_lossless,true,"무손실");ui.selectable_value(&mut self.export_options.webp_lossless,false,"손실");});
                    if self.export_options.webp_lossless {ui.label("픽셀과 투명도를 유지합니다.");}
                    else {
                        ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.webp_quality,0..=100).text("품질"));
                        ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.webp_method,0..=6).text("압축 노력"));
                        ui.label("압축 노력이 높을수록 저장 시간이 늘어날 수 있습니다. 투명도는 유지합니다.");
                        ui.label("가로·세로 16383px 이하로 저장할 수 있습니다.");
                    }
                }
                if matches!(format,image::ImageFormat::Jpeg|image::ImageFormat::Pnm|image::ImageFormat::Hdr|image::ImageFormat::Gif) && !svg && !pcx {
                    ui.horizontal(|ui| { ui.label("배경색"); ui.color_edit_button_srgb(&mut self.export_options.background);
                        if ui.small_button("전경색").clicked(){self.export_options.background.copy_from_slice(&self.color[..3]);}
                        if ui.small_button("흰색").clicked(){self.export_options.background=[255;3];}
                        if ui.small_button("검정").clicked(){self.export_options.background=[0;3];}
                    });
                    ui.label(if format==image::ImageFormat::Gif && self.export_options.gif_transparent {"투명도 경계 이상의 반투명 색은 선택한 배경색으로 합성합니다."}else{"투명 영역은 선택한 배경색으로 합성합니다."});
                }
                if format==image::ImageFormat::Png && !svg && !pcx && !jp2 {
                    ui.add(crate::numeric_slider::Slider::new(&mut self.export_options.png_level,0..=9).text("PNG 압축 수준"));
                    egui::ComboBox::from_id_salt("png-filter").selected_text(files::PNG_FILTER_LABELS[self.export_options.png_filter]).show_ui(ui,|ui|{for (index,label) in files::PNG_FILTER_LABELS.iter().enumerate(){ui.selectable_value(&mut self.export_options.png_filter,index,*label);}});
                    ui.label("0은 압축 없음, 9는 최대 압축입니다. 모든 수준에서 픽셀과 알파를 유지합니다.");
                }
                if let Some(tab)=self.tab() && tab.editor.document.layers.iter().any(|l|l.bit_depth>8) {
                    ui.label(if jp2 {if crate::jpeg2000_output::depth(&self.tab().unwrap().editor.document,&self.export_options)==16 {"16비트 정수로 저장합니다. HDR/음수 값은 변환되고 원본은 프로젝트에 보존됩니다."}else{"8비트 정수로 저장합니다. 원본 정밀도는 프로젝트에 보존됩니다."}}else if pcx {"PCX는 8비트 RGB로 저장합니다. 원본 정밀도는 프로젝트 저장에 보존하세요."}else if svg {"SVG의 사진 요소는 8비트 PNG로 저장합니다. 원본 정밀도는 프로젝트 저장에 보존하세요."}else if matches!(format,image::ImageFormat::OpenExr|image::ImageFormat::Hdr){"32비트 부동소수점·HDR 값으로 저장합니다."}else if format==image::ImageFormat::Avif && self.export_options.avif_depth==10 {"10비트 AVIF로 변환합니다. 원본 정밀도는 프로젝트에 보존하세요."}else if format==image::ImageFormat::Tiff && crate::tiff_output::depth(&tab.editor.document,&self.export_options)==32 {"32비트 부동소수점 TIFF로 저장합니다."}else if format==image::ImageFormat::Tiff && self.export_options.tiff_depth==1 {"8비트 TIFF로 변환합니다. 원본 정밀도는 프로젝트에 보존하세요."}else if matches!(format,image::ImageFormat::Png|image::ImageFormat::Tiff|image::ImageFormat::Farbfeld){"16비트로 저장합니다. 범위를 벗어난 HDR 값은 변환됩니다."}else{"이 형식의 현재 출력은 8비트입니다. 원본 정밀도는 프로젝트 저장에 보존하세요."});
                }
                if pcx { ui.label("PCX는 8비트 RGB/RLE로 출력하며 투명 영역은 흰색으로 합성합니다. 크기 제한: 가로 65534px·세로 65536px."); }
                if format==image::ImageFormat::Hdr {ui.label("HDR 밝기를 RGBE로 저장합니다. 알파는 선택한 배경색으로 합성하고 음수 RGB는 0으로 변환합니다. ICC/감마 변환은 자동 적용하지 않습니다.");}
                if format==image::ImageFormat::Ico{ui.label("아이콘 출력 크기는 가로·세로 256px 이하여야 합니다.");}
                if svg {
                    if let Some(tab) = self.tab() { ui.label(crate::svg_export::report(&tab.editor.document)); }
                } else { ui.label(
                    "레이어를 합친 이미지로 내보냅니다.\n편집 상태는 프로젝트 저장으로 보관하세요.",
                ); }
                });});
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical().id_salt("export-preview-scroll").max_height(height).show(ui,|ui| {
                        let document=self.tabs.get(self.active).map(|t|(&t.editor.document,t.id,t.editor.revision));
                        self.export_preview.ui(ui,ctx,document,self.export_format,&self.export_options);
                    });
                });});
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.busy.is_none(), egui::Button::new("내보내기"))
                        .clicked()
                    {
                        self.export_dialog = false;
                        self.export(ctx);
                    }
                    if ui.button("취소").clicked() {
                        self.export_dialog = false;
                    }
                });
            });
        }
        if let Some(target) = self.close_target
            && self.busy.is_none()
            && let Some(tab) = self.tab()
        {
            let title = tab.title.clone();
            egui::Modal::new(egui::Id::new("close-confirmation")).show(ctx, |ui| {
                ui.set_min_width(340.0);
                ui.heading("변경 내용을 저장할까요?");
                ui.label(title);
                ui.horizontal(|ui| {
                    if ui.button("저장").clicked() {
                        self.save(false, ctx);
                    }
                    if ui.button("저장하지 않음").clicked() {
                        match target {
                            CloseTarget::Tab(id) => {
                                if let Some(index) = self.tabs.iter().position(|t| t.id == id) {
                                    self.clear_recovery(index);
                                    self.tabs.remove(index);
                                    self.active =
                                        self.active.min(self.tabs.len().saturating_sub(1));
                                }
                                self.close_target = None;
                            }
                            CloseTarget::App => {
                                if let Some(tab) = self.tab_mut() {
                                    tab.editor.saved_revision = Some(tab.editor.revision);
                                }
                                self.complete_close(ctx);
                            }
                        }
                    }
                    if ui.button("취소").clicked() {
                        self.close_target = None;
                    }
                });
            });
        }
        if let Some((title, message)) = self.message.clone() {
            egui::Modal::new(egui::Id::new("message")).show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.heading(title);
                ui.separator();
                ui.label(message);
                if ui.button("확인").clicked() {
                    self.message = None;
                }
            });
        }
    }
}

fn batch_handles(
    frame: &crate::transform_batch::Frame,
    image: Rect,
    scale: f32,
) -> Vec<(crate::transform::Handle, Pos2)> {
    let screen = |p: [f32; 2]| image.min + Vec2::new(p[0], p[1]) * scale;
    let mut handles: Vec<_> = frame
        .handles()
        .into_iter()
        .map(|(h, p)| (h, screen(p)))
        .collect();
    if handles.len() == 8 {
        let center = screen(frame.center());
        let top = handles[1].1;
        let direction = (top - center).normalized();
        handles.push((crate::transform::Handle::Rotate, top + direction * 24.0));
    }
    handles
}

fn draw_layout(
    ui: &egui::Ui,
    area: Rect,
    full: Rect,
    image: Rect,
    layout: &crate::guides::Layout,
    scale: f32,
    rulers: bool,
) {
    let painter = ui.painter_at(area);
    if layout.grid_visible {
        let visible = image.intersect(area);
        if visible.is_positive() {
            for axis in 0..2 {
                let step = layout.spacing[axis] / layout.subdivisions as f32;
                let skip = (6.0 / (step * scale)).ceil().max(1.0) as i64;
                let start = ((visible.min[axis] - image.min[axis]) / scale / step).floor() as i64
                    / skip
                    * skip;
                let end = ((visible.max[axis] - image.min[axis]) / scale / step).ceil() as i64;
                for tick in (start..=end).step_by(skip as usize).take(4096) {
                    let position = image.min[axis] + tick as f32 * step * scale;
                    let mut a = visible.min;
                    let mut b = visible.max;
                    a[axis] = position;
                    b[axis] = position;
                    let major = tick % layout.subdivisions as i64 == 0;
                    painter.line_segment(
                        [a, b],
                        Stroke::new(
                            if major { 1.0 } else { 0.5 },
                            Color32::from_rgba_unmultiplied(
                                105,
                                160,
                                230,
                                if major { 100 } else { 45 },
                            ),
                        ),
                    );
                }
            }
        }
    }
    if layout.guides_visible {
        for guide in &layout.guides {
            let position = image.min[guide.axis] + guide.position * scale;
            let mut a = area.min;
            let mut b = area.max;
            a[guide.axis] = position;
            b[guide.axis] = position;
            painter.line_segment([a, b], Stroke::new(1.0, Color32::from_rgb(40, 215, 205)));
        }
    }
    if rulers {
        let ruler = ui.painter_at(full);
        ruler.rect_filled(
            Rect::from_min_max(full.min, Pos2::new(full.right(), area.top())),
            0.0,
            Color32::from_gray(38),
        );
        ruler.rect_filled(
            Rect::from_min_max(full.min, Pos2::new(area.left(), full.bottom())),
            0.0,
            Color32::from_gray(38),
        );
        let power = 10.0_f32.powf((50.0 / scale).log10().floor());
        let step = [1.0, 2.0, 5.0, 10.0]
            .into_iter()
            .map(|factor| power * factor)
            .find(|step| step * scale >= 40.0)
            .unwrap_or(power * 10.0);
        for axis in 0..2 {
            let start = ((area.min[axis] - image.min[axis]) / scale / step).floor() as i64;
            let end = ((area.max[axis] - image.min[axis]) / scale / step).ceil() as i64;
            let rect = if axis == 0 {
                Rect::from_min_max(
                    Pos2::new(area.left(), full.top()),
                    Pos2::new(full.right(), area.top()),
                )
            } else {
                Rect::from_min_max(
                    Pos2::new(full.left(), area.top()),
                    Pos2::new(area.left(), full.bottom()),
                )
            };
            let painter = ruler.with_clip_rect(rect);
            for tick in start..=end {
                let value = tick as f32 * step;
                let position = image.min[axis] + value * scale;
                for minor in 0..5 {
                    let p = position + minor as f32 * step * scale / 5.0;
                    let length = if minor == 0 { 7.0 } else { 3.0 };
                    let points = if axis == 0 {
                        [Pos2::new(p, area.top() - length), Pos2::new(p, area.top())]
                    } else {
                        [
                            Pos2::new(area.left() - length, p),
                            Pos2::new(area.left(), p),
                        ]
                    };
                    painter.line_segment(points, Stroke::new(1.0, Color32::from_gray(140)));
                }
                let at = if axis == 0 {
                    Pos2::new(position + 2.0, full.top() + 1.0)
                } else {
                    Pos2::new(full.left() + 1.0, position + 2.0)
                };
                painter.text(
                    at,
                    egui::Align2::LEFT_TOP,
                    format!("{value:.0}"),
                    egui::FontId::proportional(9.0),
                    Color32::from_gray(200),
                );
            }
        }
    }
}

impl eframe::App for PhotoEditor {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.validate_rename();
        crate::lens_profile::poll_library(&ctx);
        self.poll_jobs(&ctx);
        self.auto_save(&ctx);
        self.poll_files(&ctx);
        if self.external_dialog.is_some()
            || self.command_panel.open
            || self.command_options.is_some()
            || self.about_panel.open
        {
            ui.disable();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.external_dialog.is_none() {
                self.request_close(CloseTarget::App, &ctx);
            }
        }
        if self.busy.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.shortcuts(&ctx);
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| PathBuf::from(f.path()))
                .collect()
        });
        if !self.transforming
            && self.external_dialog.is_none()
            && !self.command_panel.open
            && self.command_options.is_none()
            && !self.about_panel.open
            && let Some(path) = dropped.into_iter().next()
        {
            self.open_file(Some(path), None, &ctx);
        }
        self.workspace_ui(ui, &ctx);
        if !self.canvas_only {
            self.transform_ui(&ctx);
        }
        self.dialogs(&ctx);
        self.command_ui(&ctx);
        self.about_panel.ui(&ctx);
        if !self.canvas_only {
            self.shortcuts_panel
                .ui(&ctx, &mut self.workspace.layout.shortcuts);
            self.colors_panel
                .ui(&ctx, &mut self.color, &mut self.background);
        }
        let tab = self.tabs.get(self.active);
        if !self.canvas_only {
            self.mask_view.ui(
                &ctx,
                tab.and_then(|t| t.editor.document.active_layer()),
                tab.map_or(0, |t| t.editor.revision),
                tab.is_some_and(|t| t.gesture.is_some()),
            );
        }
        self.workspace.layout.histogram = self.show_histogram;
        self.workspace.layout.pixel_grid = self.show_grid;
        if let Err(error) = self.workspace.observe(!ctx.input(|i| i.pointer.any_down())) {
            self.status = format!("작업 화면 설정 저장 실패: {error}");
            self.workspace.stop_persistence();
        }
        if let Err(error) = self.colors_panel.observe(
            self.color,
            self.background,
            !ctx.input(|i| i.pointer.any_down()),
        ) {
            self.status = format!("색상 설정 저장 실패: {error}");
            self.colors_panel.stop_persistence();
        }
    }
}
