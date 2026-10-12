use image::{Rgba, Rgba32FImage};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    Poly3,
    Poly5,
    Ptlens,
    Acm,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub maker: String,
    pub name: String,
    pub mounts: Vec<String>,
    pub projection: String,
    pub focal: f64,
    pub real_focal: f64,
    pub crop: f64,
    pub input_crop: f64,
    pub aspect: f64,
    pub center: [f64; 2],
    #[serde(
        default = "default_orientation",
        skip_serializing_if = "is_default_orientation"
    )]
    pub orientation: u8,
    pub model: Model,
    pub coefficients: [f64; 5],
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tca: Option<Tca>,
    #[serde(default, skip_serializing_if = "vignetting_empty")]
    pub vignetting: std::sync::Arc<Vec<Vignetting>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignetting_selected: Option<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vignetting_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignetting_query: Option<[f64; 3]>,
    #[serde(default)]
    pub calibration: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpolated: Option<[f64; 2]>,
    pub xml: std::sync::Arc<String>,
}
fn vignetting_empty(v: &std::sync::Arc<Vec<Vignetting>>) -> bool {
    v.is_empty()
}
fn extrema(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() < 1e-15 {
        return if b.abs() > 1e-15 {
            vec![-c / b]
        } else {
            Vec::new()
        };
    }
    let d = b * b - 4. * a * c;
    if d < 0. {
        Vec::new()
    } else {
        let d = d.sqrt();
        vec![(-b + d) / (2. * a), (-b - d) / (2. * a)]
    }
}
fn cubic_minimum(c: [f64; 4], end: f64) -> f64 {
    let evaluate = |r: f64| c[0] + r * (c[1] + r * (c[2] + r * c[3]));
    let mut points = vec![0., end];
    points.extend(
        extrema(3. * c[3], 2. * c[2], c[1])
            .into_iter()
            .filter(|r| *r > 0. && *r < end),
    );
    points
        .into_iter()
        .map(evaluate)
        .fold(f64::INFINITY, f64::min)
}
fn default_orientation() -> u8 {
    1
}
fn is_default_orientation(value: &u8) -> bool {
    *value == 1
}
#[path = "lens_orientation.rs"]
mod orientation;
impl Profile {
    pub fn effect_enabled(&self) -> bool {
        self.distortion_enabled()
            || self.tca.as_ref().is_some_and(Tca::effect_enabled)
            || self.vignetting_effect().is_some()
    }
    fn distortion_enabled(&self) -> bool {
        self.enabled && self.coefficients != [0.; 5]
    }
    pub fn valid(&self) -> bool {
        self.vignetting_query.is_none_or(|q| {
            q.iter().all(|v| v.is_finite())
                && (0.1..=5000.).contains(&q[0])
                && (0.1..=1024.).contains(&q[1])
                && (0.001..=1e6).contains(&q[2])
                && (q[0] - self.focal).abs() < 1e-6
                && self.vignetting_selected.is_some()
        }) && (1..=8).contains(&self.orientation)
            && self.calibration < 16384
            && self.interpolated.is_none_or(|r| {
                r.iter().all(|v| v.is_finite() && (0.1..=5000.).contains(v))
                    && r[0] < r[1]
                    && self.focal >= r[0]
                    && self.focal <= r[1]
            })
            && self.distortion_valid()
            && self.tca.as_ref().is_none_or(|t| t.valid_for(self))
            && self.vignetting.len() <= 4096
            && self.vignetting.iter().all(Vignetting::valid)
            && self
                .vignetting_selected
                .is_none_or(|i| self.vignetting.get(i).is_some_and(|v| v.valid_for(self)))
            && (!self.vignetting_enabled || self.vignetting_selected.is_some())
    }
    fn vignetting_effect(&self) -> Option<&Vignetting> {
        if !self.vignetting_enabled {
            return None;
        }
        self.vignetting_selected
            .and_then(|i| self.vignetting.get(i))
            .filter(|v| v.coefficients != [0.; 3])
    }
    pub fn choose_vignetting(&mut self, info: &crate::raw_input::Info) {
        let Some([n, d]) = info.shooting.aperture else {
            return;
        };
        let aperture = f64::from(n) / f64::from(d);
        let matching = self
            .vignetting
            .iter()
            .enumerate()
            .filter(|(_, v)| (v.aperture - aperture).abs() < 1e-6 && v.valid_for(self))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        self.vignetting_selected = (matching.len() == 1).then(|| matching[0]);
    }
    // Conservative radius bound after distortion, used to reject folding TCA mappings.
    fn radius_bound(&self, acm: bool) -> f64 {
        let center = 1. + self.center[0].abs().max(self.center[1].abs());
        let hugin = center * self.crop / self.input_crop * self.aspect.hypot(1.);
        let adobe = center * 36f64.hypot(24.) / (2. * self.input_crop * self.real_focal);
        let r = if self.model == Model::Acm {
            adobe
        } else {
            hugin
        };
        let [a, b, c, d, e] = self.coefficients;
        let gain = if !self.distortion_enabled() {
            1.
        } else {
            match self.model {
                Model::Poly3 => 1. + (a / (1. - a).powi(3)).abs() * r * r,
                Model::Poly5 => 1. + a.abs() * r * r + b.abs() * r.powi(4),
                Model::Ptlens => {
                    let v = 1. - a - b - c;
                    1. + (a / v.powi(4)).abs() * r.powi(3)
                        + (b / v.powi(3)).abs() * r * r
                        + (c / v.powi(2)).abs() * r
                }
                Model::Acm => {
                    1. + a.abs() * r * r
                        + b.abs() * r.powi(4)
                        + c.abs() * r.powi(6)
                        + 3. * (d.abs() + e.abs()) * r
                }
            }
        };
        (if acm { adobe } else { hugin }) * gain
    }
    fn distortion_valid(&self) -> bool {
        if self.mounts.len() > 256
            || self.mounts.iter().any(|m| m.len() > 4096)
            || self.projection.len() > 4096
            || self.name.trim().is_empty()
            || self.name.len() > 4096
            || self.maker.len() > 4096
            || self.xml.len() > 1024 * 1024
            || !self.focal.is_finite()
            || !(0.1..=5000.).contains(&self.focal)
            || !self.real_focal.is_finite()
            || !(0.1..=5000.).contains(&self.real_focal)
            || [self.crop, self.input_crop]
                .iter()
                .any(|v| !v.is_finite() || !(0.1..=30.).contains(v))
            || !self.aspect.is_finite()
            || !(0.1..=10.).contains(&self.aspect)
            || self.center.iter().any(|v| !v.is_finite() || v.abs() > 2.)
            || self
                .coefficients
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 100.)
        {
            return false;
        }
        if match self.model {
            Model::Poly3 => self.coefficients[1..].iter().any(|v| *v != 0.),
            Model::Poly5 => self.coefficients[2..].iter().any(|v| *v != 0.),
            Model::Ptlens => self.coefficients[3..].iter().any(|v| *v != 0.),
            Model::Acm => false,
        } {
            return false;
        }
        let [a, b, c, d, e] = self.coefficients;
        let r = (1. + self.center[0].abs().max(self.center[1].abs())) * self.crop / self.input_crop
            * self.aspect.hypot(1.);
        match self.model {
            Model::Poly3 => {
                let constant = 1. - a;
                if constant <= 0.01 {
                    return false;
                }
                cubic_minimum([1., 0., 3. * a / constant.powi(3), 0.], r) > 0.01
            }
            Model::Poly5 => {
                cubic_minimum([1., 3. * a, 5. * b, 0.], r * r) > 0.01
                    && cubic_minimum([1., a, b, 0.], r * r) > 0.01
            }
            Model::Ptlens => {
                let constant = 1. - a - b - c;
                if constant <= 0.01 {
                    return false;
                }
                cubic_minimum(
                    [
                        1.,
                        2. * c / constant.powi(2),
                        3. * b / constant.powi(3),
                        4. * a / constant.powi(4),
                    ],
                    r,
                ) > 0.01
            }
            Model::Acm => {
                let r = (1. + self.center[0].abs().max(self.center[1].abs())) * 36f64.hypot(24.)
                    / (2. * self.input_crop * self.real_focal);
                let tangent = 6. * (d.abs() + e.abs()) * r;
                let radial = cubic_minimum([1., 3. * a, 5. * b, 7. * c], r * r);
                let tangential = cubic_minimum([1., a, b, c], r * r);
                radial.min(tangential) > 0.01 + tangent
            }
        }
    }
    #[cfg(test)]
    pub fn point(&self, p: [f64; 2], size: [u32; 2]) -> [f64; 2] {
        let native_size = orientation::size(size, self.orientation);
        let native = orientation::inverse(p, size, self.orientation);
        orientation::forward(
            self.point_native(native, native_size),
            native_size,
            self.orientation,
        )
    }
    fn point_native(&self, p: [f64; 2], size: [u32; 2]) -> [f64; 2] {
        if !self.distortion_enabled() {
            return p;
        }
        let diagonal = f64::from(size[0]).hypot(f64::from(size[1]));
        let short = f64::from(size[0].saturating_sub(1).min(size[1].saturating_sub(1)));
        let center = [
            f64::from(size[0]) / 2. + short / 2. * self.center[0],
            f64::from(size[1]) / 2. + short / 2. * self.center[1],
        ];
        let scale = if self.model == Model::Acm {
            36f64.hypot(24.) / (self.input_crop * diagonal * self.real_focal)
        } else {
            2. * self.crop * self.aspect.hypot(1.) / (self.input_crop * diagonal)
        };
        let [x, y] = [(p[0] - center[0]) * scale, (p[1] - center[1]) * scale];
        let r2 = x * x + y * y;
        let r = r2.sqrt();
        let [a, b, c, d, e] = self.coefficients;
        let (dx, dy) = match self.model {
            Model::Poly3 => {
                let factor = 1. + a / (1. - a).powi(3) * r2;
                (x * factor, y * factor)
            }
            Model::Poly5 => {
                let factor = 1. + a * r2 + b * r2 * r2;
                (x * factor, y * factor)
            }
            Model::Ptlens => {
                let constant = 1. - a - b - c;
                let factor = 1.
                    + a / constant.powi(4) * r * r2
                    + b / constant.powi(3) * r2
                    + c / constant.powi(2) * r;
                (x * factor, y * factor)
            }
            Model::Acm => {
                let factor = 1. + a * r2 + b * r2 * r2 + c * r2 * r2 * r2;
                (
                    x * factor + 2. * x * (d * y + e * x) + e * r2,
                    y * factor + 2. * y * (d * y + e * x) + d * r2,
                )
            }
        };
        [center[0] + dx / scale, center[1] + dy / scale]
    }
    pub fn apply_frame(
        &self,
        source: &Rgba32FImage,
        frame: [u32; 2],
        pad: u32,
        sampling: crate::sampling::Sampling,
    ) -> Rgba32FImage {
        if !self.effect_enabled() || !self.valid() {
            return source.clone();
        }
        let corrected = self
            .vignetting_effect()
            .map(|v| v.apply(source, frame, pad, self));
        if !self.distortion_enabled() && !self.tca.as_ref().is_some_and(Tca::effect_enabled) {
            return corrected.unwrap_or_else(|| source.clone());
        }
        let source = corrected.as_ref().unwrap_or(source);
        let native_frame = orientation::size(frame, self.orientation);
        let native_source = orientation::size([source.width(), source.height()], self.orientation);
        Rgba32FImage::from_fn(source.width(), source.height(), |x, y| {
            let output_point = [
                f64::from(x) + 0.5 - f64::from(pad),
                f64::from(y) + 0.5 - f64::from(pad),
            ];
            let point = self.point_native(
                orientation::inverse(output_point, frame, self.orientation),
                native_frame,
            );
            let sample = |point: [f64; 2]| {
                crate::sampling::rgba_precise(
                    point.map(|v| (v + f64::from(pad)) as f32),
                    sampling,
                    |x, y| {
                        if x < 0
                            || y < 0
                            || x >= native_source[0] as i32
                            || y >= native_source[1] as i32
                        {
                            [0.; 4]
                        } else {
                            let [x, y] = orientation::pixel(
                                [x as u32, y as u32],
                                native_source,
                                self.orientation,
                            );
                            source.get_pixel(x, y).0
                        }
                    },
                )
            };
            let mut color = if self.distortion_enabled() {
                sample(point)
            } else {
                source.get_pixel(x, y).0
            };
            if let Some(tca) = self.tca.as_ref().filter(|t| t.effect_enabled()) {
                // Green and alpha remain the reference geometry; each RGB channel is unassociated.
                color[0] = sample(tca.point(point, native_frame, self, false))[0];
                color[2] = sample(tca.point(point, native_frame, self, true))[2];
            }
            Rgba(color)
        })
    }
}
#[path = "lens_interpolation.rs"]
mod interpolation;
#[path = "lens_vignette_interpolation.rs"]
mod vignette_interpolation;
#[path = "lens_vignetting.rs"]
mod vignetting;
pub use vignetting::{VignetteModel, Vignetting};
#[path = "lens_tca.rs"]
mod tca;
pub use tca::{Tca, TcaModel};
#[path = "lens_profile_xml.rs"]
mod xml;
pub use xml::Database;
#[cfg(test)]
pub use xml::load;
#[path = "lens_catalog.rs"]
mod catalog;
#[path = "lens_library.rs"]
mod library;
pub use library::{initialize as initialize_library, poll as poll_library};

pub fn ui(
    profile: &mut Option<Profile>,
    info: Option<&crate::raw_input::Info>,
    ui: &mut eframe::egui::Ui,
) {
    use eframe::egui;
    ui.collapsing("렌즈 프로파일",|ui| {
        let id=egui::Id::new("lens-profile-library");let error_id=ui.id().with("lens-error");

        library::poll(ui.ctx());
        let database = ui.data(|d| d.get_temp::<std::sync::Arc<Database>>(id));
        let (loading, has_paths, library_error) = library::status(ui.ctx());
        if has_paths {library::watch_ui(ui);}
        if ui.add_enabled(!loading, egui::Button::new("프로파일 XML 불러오기…")).clicked()
            && let Some(paths) = rfd::FileDialog::new().add_filter("Lensfun XML", &["xml"]).pick_files()
        {
            library::import(ui.ctx(), database.clone(), paths);
        }
        if loading {
            ui.horizontal(|ui| { ui.spinner(); ui.label("렌즈 목록을 불러오는 중…"); });
            if ui.button("렌즈 목록 불러오기 취소").clicked() { library::cancel(ui.ctx()); }
        }
        if has_paths && ui.add_enabled(!loading, egui::Button::new("렌즈 목록 다시 불러오기")).clicked() { library::retry(ui.ctx()); }
        if let Some(error) = library_error { ui.colored_label(egui::Color32::LIGHT_RED, error); }
        let mut clear_catalog = false;
        if let Some(db) = &database {
            ui.horizontal(|ui| {
                ui.label(format!("렌즈 교정값 {}개", db.profiles.len()));
                clear_catalog = ui.button("불러온 프로파일 비우기").on_hover_text("현재 사진에 선택한 교정값은 유지합니다.").clicked();
            });
            if ui.add_enabled(info.is_some(), egui::Button::new("촬영 정보로 프로파일 찾기")).clicked() {
                if let Some(found) = info.and_then(|info| db.suggest(info)) {
                    *profile = Some(found); ui.data_mut(|d| d.remove::<String>(error_id));
                } else {
                    ui.data_mut(|d| d.insert_temp(error_id, "카메라·렌즈·초점거리가 하나의 호환 프로파일과 측정 범위에 맞지 않습니다. 직접 선택하고 크롭 배율을 확인하세요.".to_owned()));
                }
            }
            let search_id = ui.id().with("lens-search");
            let mut query = ui.data(|d| d.get_temp::<String>(search_id)).unwrap_or_default();
            ui.add(egui::TextEdit::singleline(&mut query).hint_text("렌즈 이름·제조사·초점거리 검색"));
            ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
            type Cache = (std::sync::Arc<Database>, std::sync::Arc<catalog::View>, String, std::sync::Arc<Vec<usize>>);
            let cache_id = ui.id().with("lens-catalog-view");
            let cached = ui.data(|d| d.get_temp::<Cache>(cache_id));
            let view = cached.as_ref().filter(|(old, _, _, _)| std::sync::Arc::ptr_eq(old, db))
                .map_or_else(|| std::sync::Arc::new(catalog::View::new(db)), |(_, view, _, _)| view.clone());
            let indices = cached.as_ref().filter(|(old, _, old_query, _)| std::sync::Arc::ptr_eq(old, db) && *old_query == query)
                .map_or_else(|| std::sync::Arc::new(view.find(&query)), |(_, _, _, rows)| rows.clone());
            ui.data_mut(|d| d.insert_temp(cache_id, (db.clone(), view.clone(), query, indices.clone())));
            let selected = profile.as_ref().map_or("교정값 선택".to_owned(), |p| format!("{} · {} mm", p.name, p.focal));
            egui::ComboBox::from_id_salt("lens-profile-selection").selected_text(selected).show_ui(ui, |ui| {
                if indices.is_empty() { ui.label("검색 결과가 없습니다."); }
                egui::ScrollArea::vertical().max_height(240.).show_rows(ui, ui.text_style_height(&egui::TextStyle::Button), indices.len(), |ui, range| {
                    for row in range {
                        let i = indices[row]; let p = &db.profiles[i];
                        if ui.selectable_label(profile.as_ref().is_some_and(|s| catalog::same_selection(s, p)), view.label(i)).clicked() {
                            let mut chosen = p.clone();
                            if let Some(info) = info {
                                chosen.orientation = info.orientation.clamp(1, 8);
                                if let Some(crop) = db.body_crop(info, &chosen) { chosen.input_crop = crop; }
                                chosen.choose_vignetting(info);
                            }
                            *profile = Some(chosen);
                        }
                    }
                });
            });
            if !db.notes.is_empty(){ui.collapsing("프로파일 읽기 안내",|ui| {for note in &db.notes {ui.label(note);}});}
        }
        if clear_catalog {
            library::clear(ui.ctx());
            ui.data_mut(|d| { d.remove::<std::sync::Arc<Database>>(id); d.remove::<String>(error_id); d.remove::<(std::sync::Arc<Database>,std::sync::Arc<catalog::View>,String,std::sync::Arc<Vec<usize>>)>(ui.id().with("lens-catalog-view")); });
        }
        if let Some(error)=ui.data(|d|d.get_temp::<String>(error_id)){ui.colored_label(egui::Color32::LIGHT_RED,error);}
        let remove=profile.is_some() && ui.button("렌즈 프로파일 제거").clicked();
        if let Some(p)=profile {
            if p.coefficients != [0.;5] {ui.checkbox(&mut p.enabled,"프로파일 왜곡 보정");} else {ui.label("이 초점거리의 왜곡 교정값이 없습니다.");}
            if let Some(tca)=&mut p.tca {ui.checkbox(&mut tca.enabled,"프로파일 색수차 보정").on_hover_text(match tca.model {TcaModel::Linear=>"선형 채널 배율",TcaModel::Poly3=>"3차 채널 반경",TcaModel::Acm=>"Adobe 채널 반경·접선 모델"});} else {ui.label("이 초점거리의 색수차 교정값이 없습니다.");}
            if !p.vignetting.is_empty() {
                ui.add_enabled(p.vignetting_selected.is_some(),egui::Checkbox::new(&mut p.vignetting_enabled,"프로파일 주변부 밝기 보정"));
                let selected=p.vignetting_selected.and_then(|i|p.vignetting.get(i)).map_or("밝기 교정값 선택".to_owned(),|v|format!("f/{} · {} m · {}",v.aperture,v.distance,match v.model {VignetteModel::Pa=>"PA",VignetteModel::Acm=>"ACM"}));
                egui::ComboBox::from_id_salt("lens-vignetting-selection").selected_text(selected).show_ui(ui,|ui| {
                    for (i,v) in p.vignetting.iter().enumerate() {if ui.selectable_label(p.vignetting_selected==Some(i),format!("f/{} · {} m · {}",v.aperture,v.distance,match v.model {VignetteModel::Pa=>"PA",VignetteModel::Acm=>"ACM"})).clicked(){p.vignetting_selected=Some(i);p.vignetting_query=None;}}
                });
            } else {ui.label("이 초점거리의 주변부 밝기 교정값이 없습니다.");}
            let focal_id=ui.id().with("lens-target-focal");
            let identity=(std::sync::Arc::as_ptr(&p.xml) as usize,p.calibration,p.focal.to_bits());
            let mut target=ui.data(|d|d.get_temp::<((usize,usize,u64),f64)>(focal_id)).filter(|(key,_)|*key==identity).map_or(p.focal,|(_,value)|value);
            ui.horizontal(|ui| {
                crate::numeric_slider::labeled_drag(ui,"보정 초점거리 mm",&mut target,0.1..=5000.,0.1,"");
                if ui.button("초점거리 보정값 계산").clicked() {
                    match p.at_focal(target) {
                        Ok(changed)=>{*p=changed;ui.data_mut(|d|d.remove::<String>(error_id));},
                        Err(error)=>{ui.data_mut(|d|d.insert_temp(error_id,error));}
                    }
                }
            });
            ui.data_mut(|d|d.insert_temp(focal_id,((std::sync::Arc::as_ptr(&p.xml) as usize,p.calibration,p.focal.to_bits()),target)));
            if !p.vignetting.is_empty() || p.vignetting_query.is_some() {
                let id=ui.id().with("vignetting-query");
                let current=(std::sync::Arc::as_ptr(&p.xml) as usize,p.calibration,p.vignetting_selected);
                let fallback=p.vignetting_query.map(|q|[q[1],q[2]]).or_else(||p.vignetting_selected.and_then(|i|p.vignetting.get(i)).map(|v|[v.aperture,v.distance])).unwrap_or([2.8,10.]);
                let mut query=ui.data(|d|d.get_temp::<((usize,usize,Option<usize>),[f64;2])>(id)).filter(|(key,_)|*key==current).map_or(fallback,|(_,v)|v);
                ui.horizontal_wrapped(|ui| {crate::numeric_slider::labeled_drag(ui,"교정 조리개 f/",&mut query[0],0.1..=1024.,0.1,"");crate::numeric_slider::labeled_drag(ui,"교정 거리 m",&mut query[1],0.001..=1e6,0.1,"");});
                if ui.button("주변부 밝기 보정값 계산").clicked(){match p.at_optics(target,query[0],query[1]){Ok(changed)=>{*p=changed;ui.data_mut(|d|d.remove::<String>(error_id));},Err(error)=>{ui.data_mut(|d|d.insert_temp(error_id,error));}}}
                ui.data_mut(|d|d.insert_temp(id,((std::sync::Arc::as_ptr(&p.xml) as usize,p.calibration,p.vignetting_selected),query)));
                if let Some(q)=p.vignetting_query {ui.label(format!("주변부 밝기 계산 입력: {} mm · f/{} · {} m",q[0],q[1],q[2]));}
            }

            if let Some(range)=p.interpolated {ui.label(format!("{}–{} mm 측정값 사이에서 보간",range[0],range[1]));}
            ui.label(format!("{} {} · {} mm · {:?}",p.maker,p.name,p.focal,p.model));
            ui.add(crate::numeric_slider::Slider::new(&mut p.input_crop,0.1..=30.).logarithmic(true).text("카메라 크롭 배율"));
            egui::ComboBox::from_id_salt("lens-correction-orientation").selected_text(orientation::LABELS[usize::from(p.orientation.clamp(1,8)-1)]).show_ui(ui, |ui| {
                for (i,label) in orientation::LABELS.iter().enumerate() { ui.selectable_value(&mut p.orientation, i as u8 + 1, *label); }
            }).response.on_hover_text("사진은 그대로 두고 교정 좌표를 촬영 당시 방향에 맞춥니다.");
            ui.label(format!("교정 크롭 {:.3} · 종횡비 {:.3} · {}",p.crop,p.aspect,p.projection));
            ui.label("왜곡·색수차는 측정값과 범위 안 초점거리 보간을 사용합니다. 촬영 초점거리와 크롭 배율을 확인하세요. 주변부 밝기는 선택한 조리개·교정 거리의 값을 사용합니다. 주변부 밝기 계산은 입력한 초점거리·조리개·교정 거리 주변의 측정값을 보간합니다.");
            if !p.valid(){ui.colored_label(egui::Color32::LIGHT_RED,"교정값이 접히거나 허용 범위를 넘습니다. 크롭 배율을 확인하세요.");}
        }
        if remove {*profile=None;}
    });
}

#[cfg(test)]
#[path = "lens_profile_tests.rs"]
mod tests;
