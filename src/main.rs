#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod about;
mod color_management;
mod commands;
mod crop_handles;
mod layer_name;
mod lens;
mod lens_profile;
mod mask_overlay;
mod mask_view;
mod masks;
mod matrix;
mod navigator;
mod numeric_slider;
mod photo_geometry;
mod sampling;
mod selection_edges;
mod shortcuts;
mod source_metadata;
mod transform_batch;

mod adjustment_layer;
mod adjustment_scope;
mod adjustments;
mod app;
mod avif_output;
mod blend;
mod bloom;
mod canvas;
mod codecs;
mod color_balance;
mod color_grading;
mod color_mixer;
mod color_model;
mod colors;
mod denoise;
mod detail_sharpen;
mod display_sampling;
mod document;
mod document_id;
mod editing;
mod effects;
mod exif_output;
mod export_preview;
mod file_watch;
mod files;
mod fill;
mod filter_bounds;
mod font_picker;
mod gif_output;
mod guides;
#[cfg(test)]
mod icc_fixture;
mod icc_float;
mod icons;
mod jpeg2000_icc;
mod jpeg2000_input;
mod jpeg2000_output;
mod jpeg2000_xmp;
mod jpeg_output;
mod jxl_color;
mod jxl_input;
mod jxl_metadata;
mod panels;
mod paths;
mod pattern;
mod pcx;
mod precision;
mod psd;
mod psd_compression;
mod raw_input;
mod raw_metadata;
mod raw_panel;
mod raw_preview;
mod recovery;
mod rich_text;
mod svg_assets;
mod svg_edit;
mod svg_export;
mod svg_namespace;
mod svg_objects;
mod svg_paths;
mod text_canvas;
mod tiff_exif;
mod tiff_float;
mod tiff_output;
mod tiff_palette;
mod tonal_contrast;
mod transform;
mod vector;
mod webp_output;
mod workspace;
mod xmp_output;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Photo Editor")
            .with_inner_size([1320.0, 860.0])
            .with_min_inner_size([880.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "photo_editor",
        options,
        Box::new(|cc| {
            install_korean_font(&cc.egui_ctx);
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(app::PhotoEditor::new(cc)))
        }),
    )
}

fn install_korean_font(ctx: &egui::Context) {
    let windows_dir = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
    let font_path = windows_dir.join("Fonts").join("malgun.ttf");
    if let Ok(bytes) = std::fs::read(font_path) {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "malgun".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "malgun".to_owned());
        ctx.set_fonts(fonts);
    }
}

#[cfg(test)]
mod transform_tests;
