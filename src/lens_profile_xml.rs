use super::{Model, Profile};
use std::{path::Path, sync::Arc};
#[derive(Clone, Debug)]
pub struct Body {
    pub maker: String,
    pub name: String,
    pub crop: f64,
    pub mount: String,
}
#[derive(Clone, Debug)]
pub struct Database {
    pub profiles: Vec<Profile>,
    pub bodies: Vec<Body>,
    pub notes: Vec<String>,
}
fn text(node: roxmltree::Node<'_, '_>, tag: &str) -> String {
    let mut values = node.children().filter(|n| n.has_tag_name(tag));
    values
        .clone()
        .find(|n| n.attribute("lang").is_none())
        .or_else(|| values.next())
        .and_then(|n| n.text())
        .unwrap_or("")
        .trim()
        .to_owned()
}
pub(super) fn number(
    node: roxmltree::Node<'_, '_>,
    key: &str,
    default: f64,
) -> Result<f64, String> {
    node.attribute(key)
        .map(|v| {
            v.parse::<f64>()
                .map_err(|_| format!("프로파일의 {key} 값이 잘못되었습니다."))
        })
        .unwrap_or(Ok(default))
}
fn child_number(node: roxmltree::Node<'_, '_>, tag: &str, default: f64) -> Result<f64, String> {
    let value = text(node, tag);
    if value.is_empty() {
        Ok(default)
    } else {
        value
            .parse()
            .map_err(|_| format!("프로파일의 {tag} 값이 잘못되었습니다."))
    }
}
#[cfg(test)]
pub fn load(path: &Path) -> Result<Database, String> {
    load_budget(path, &mut (16 * 1024 * 1024))
}
pub(super) fn load_budget(path: &Path, remaining: &mut usize) -> Result<Database, String> {
    use std::io::Read;
    let limit = (*remaining).min(16 * 1024 * 1024);
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit as u64 {
        return Err("렌즈 XML이 현재 읽기 예산을 초과합니다.".into());
    }
    let mut xml = String::new();
    file.take(limit as u64 + 1)
        .read_to_string(&mut xml)
        .map_err(|e| e.to_string())?;
    if xml.len() > limit {
        return Err("렌즈 XML이 현재 읽기 예산을 초과합니다.".into());
    }
    *remaining -= xml.len();
    parse(&xml)
}
pub fn parse(xml: &str) -> Result<Database, String> {
    let document = roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: 1_000_000,
            ..Default::default()
        },
    )
    .map_err(|e| format!("렌즈 XML을 읽을 수 없습니다: {e}"))?;
    let root = document.root_element();
    if !root.has_tag_name("lensdatabase") {
        return Err("Lensfun lensdatabase XML 파일을 선택하세요.".into());
    }
    let mut db = Database {
        profiles: Vec::new(),
        bodies: Vec::new(),
        notes: Vec::new(),
    };
    for camera in root.children().filter(|n| n.has_tag_name("camera")) {
        let maker = text(camera, "maker");
        let name = text(camera, "model");
        let crop = child_number(camera, "cropfactor", 1.)?;
        if maker.len() > 4096
            || name.len() > 4096
            || !crop.is_finite()
            || !(0.1..=30.).contains(&crop)
        {
            return Err("카메라 프로파일 정보가 잘못되었습니다.".into());
        }
        db.bodies.push(Body {
            maker,
            name,
            crop,
            mount: text(camera, "mount"),
        });
        if db.bodies.len() > 16384 {
            return Err("현재 카메라 데이터 개수 예산을 초과합니다.".into());
        }
    }
    for lens in root.children().filter(|n| n.has_tag_name("lens")) {
        let maker = text(lens, "maker");
        let name = text(lens, "model");
        let crop = child_number(lens, "cropfactor", 1.)?;
        let aspect = child_number(lens, "aspect-ratio", 1.5)?;
        let center = if let Some(n) = lens.children().find(|n| n.has_tag_name("center")) {
            [number(n, "x", 0.)?, number(n, "y", 0.)?]
        } else {
            [0.; 2]
        };
        let original = &xml[lens.range()];
        if original.len() > 1024 * 1024 {
            return Err("개별 렌즈 정보가 현재1MiB 보존 예산을 초과합니다.".into());
        }
        let original = Arc::new(original.to_owned());
        for (calibration_index, calibration) in lens
            .children()
            .filter(|n| n.has_tag_name("calibration"))
            .enumerate()
        {
            let crop = number(calibration, "cropfactor", crop)?;
            let aspect = number(calibration, "aspect-ratio", aspect)?;
            let base = Profile {
                maker: maker.clone(),
                name: name.clone(),
                mounts: lens
                    .children()
                    .filter(|n| n.has_tag_name("mount"))
                    .filter_map(|n| n.text())
                    .map(str::to_owned)
                    .collect(),
                projection: {
                    let kind = text(lens, "type");
                    if kind.is_empty() {
                        "rectilinear".into()
                    } else {
                        kind
                    }
                },
                focal: 1.,
                real_focal: 1.,
                crop,
                input_crop: crop,
                aspect,
                center,
                orientation: 1,
                model: Model::Poly5,
                coefficients: [0.; 5],
                enabled: false,
                tca: None,
                vignetting: Arc::new(Vec::new()),
                vignetting_selected: None,
                vignetting_enabled: false,
                vignetting_query: None,
                calibration: calibration_index,
                interpolated: None,
                xml: original.clone(),
            };
            let mut tcas = Vec::new();
            for node in calibration.children().filter(|n| n.has_tag_name("tca")) {
                match super::tca::parse(node) {
                    Ok(Some(tca)) => {
                        let focal = number(node, "focal", 0.)?;
                        if focal.is_finite() && (0.1..=5000.).contains(&focal) {
                            tcas.push((focal, tca));
                            if tcas.len() > 16384 {
                                return Err("현재 색수차 교정값 개수 예산을 초과합니다.".into());
                            }
                        } else {
                            db.notes
                                .push(format!("{name}: 잘못된 색수차 초점거리를 제외했습니다."));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => db.notes.push(format!("{name}: {error}")),
                }
            }
            let mut vignettes = Vec::new();
            for node in calibration
                .children()
                .filter(|n| n.has_tag_name("vignetting"))
            {
                match super::vignetting::parse(node) {
                    Ok(Some(v)) => {
                        let focal = number(node, "focal", 0.)?;
                        if focal.is_finite() && (0.1..=5000.).contains(&focal) {
                            vignettes.push((focal, v));
                            if vignettes.len() > 16384 {
                                return Err(
                                    "현재 주변부 밝기 교정값 개수 예산을 초과합니다.".into()
                                );
                            }
                        } else {
                            db.notes.push(format!(
                                "{name}: 잘못된 주변부 밝기 초점거리를 제외했습니다."
                            ));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => db.notes.push(format!("{name}: {error}")),
                }
            }
            let begin = db.profiles.len();
            let mut extra = 0;
            for distortion in calibration.children().filter(|n| n.is_element()) {
                if !distortion.has_tag_name("distortion") {
                    if !distortion.has_tag_name("tca") && !distortion.has_tag_name("vignetting") {
                        extra += 1;
                    }
                    continue;
                }
                let (model, names) = match distortion.attribute("model") {
                    Some("poly3") => (Model::Poly3, vec!["k1"]),
                    Some("poly5") => (Model::Poly5, vec!["k1", "k2"]),
                    Some("ptlens") => (Model::Ptlens, vec!["a", "b", "c"]),
                    Some("acm") => (Model::Acm, vec!["k1", "k2", "k3", "k4", "k5"]),
                    Some("none") => continue,
                    other => {
                        db.notes.push(format!(
                            "{name}: 알려지지 않은 왜곡 모델 {other:?}. 원래 XML은 유지합니다."
                        ));
                        continue;
                    }
                };
                let focal = number(distortion, "focal", 0.)?;
                let mut coefficients = [0.; 5];
                for (i, key) in names.iter().enumerate() {
                    coefficients[i] = number(distortion, key, 0.)?;
                }
                let mut profile = base.clone();
                profile.model = model;
                profile.focal = focal;
                profile.real_focal = number(distortion, "real-focal", focal)?;
                profile.coefficients = coefficients;
                profile.enabled = true;
                attach_tca(&mut profile, &tcas, &mut db.notes);
                attach_vignetting(&mut profile, &vignettes, &mut db.notes);
                if !profile.valid() {
                    db.notes.push(format!(
                        "{name} {focal}mm: 잘못되거나 접히는 왜곡 교정값을 제외했습니다."
                    ));
                    continue;
                }
                db.profiles.push(profile);
                if db.profiles.len() > 16384 {
                    return Err("현재 교정값 개수 예산을 초과합니다.".into());
                }
            }
            for focal in tcas
                .iter()
                .map(|(focal, _)| focal)
                .chain(vignettes.iter().map(|(focal, _)| focal))
            {
                if db.profiles[begin..]
                    .iter()
                    .any(|p| (p.focal - focal).abs() < 1e-6)
                {
                    continue;
                }
                let mut profile = base.clone();
                profile.focal = *focal;
                profile.real_focal = *focal;
                attach_tca(&mut profile, &tcas, &mut db.notes);
                attach_vignetting(&mut profile, &vignettes, &mut db.notes);
                if (profile.tca.is_some() || !profile.vignetting.is_empty()) && profile.valid() {
                    db.profiles.push(profile);
                }
                if db.profiles.len() > 16384 {
                    return Err("현재 교정값 개수 예산을 초과합니다.".into());
                }
            }
            if extra > 0 {
                db.notes.push(format!("{name}: 왜곡 이외 교정 정보{extra}개는 원래 XML에 보존하며 추가 연결이 필요합니다."));
            }
        }
    }
    if db.profiles.is_empty() {
        return Err(
            "사용 가능한 렌즈 교정값이 없습니다. 원본 파일과 현재 설정은 유지합니다.".into(),
        );
    }
    Ok(db)
}
fn attach_tca(profile: &mut Profile, tcas: &[(f64, super::Tca)], notes: &mut Vec<String>) {
    let found = tcas
        .iter()
        .filter(|(focal, _)| (*focal - profile.focal).abs() < 1e-6)
        .collect::<Vec<_>>();
    if found.len() == 1 {
        let tca = &found[0].1;
        if tca.valid_for(profile) {
            profile.tca = Some(tca.clone());
        } else {
            notes.push(format!(
                "{} {}mm: 잘못되거나 접히는 색수차 교정값을 제외했습니다.",
                profile.name, profile.focal
            ));
        }
    } else if found.len() > 1 {
        notes.push(format!(
            "{} {}mm: 중복 색수차 교정값을 자동 연결하지 않았습니다.",
            profile.name, profile.focal
        ));
    }
}
fn attach_vignetting(
    profile: &mut Profile,
    entries: &[(f64, super::Vignetting)],
    notes: &mut Vec<String>,
) {
    let mut selected = Vec::new();
    for (_, v) in entries
        .iter()
        .filter(|(focal, _)| (*focal - profile.focal).abs() < 1e-6)
    {
        if !v.valid_for(profile) {
            notes.push(format!(
                "{} {}mm: 잘못되거나 밝기 분모가0에 가까운 교정값을 제외했습니다.",
                profile.name, profile.focal
            ));
            continue;
        }
        if entries
            .iter()
            .filter(|(focal, old)| {
                (*focal - profile.focal).abs() < 1e-6
                    && old.aperture == v.aperture
                    && old.distance == v.distance
                    && old.model == v.model
            })
            .count()
            > 1
        {
            // Conflicting rows remain in the XML and are not silently selected.
            notes.push(format!(
                "{} {}mm: 조리개/거리의 중복 주변부 밝기 교정값을 제외했습니다.",
                profile.name, profile.focal
            ));
            continue;
        }
        selected.push(v.clone());
        if selected.len() > 4096 {
            notes.push(format!(
                "{}: 주변부 밝기 개수 예산을 넘었습니다.",
                profile.name
            ));
            selected.clear();
            break;
        }
    }
    profile.vignetting = Arc::new(selected);
}
fn normalized(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
impl Database {
    pub fn body_crop(&self, info: &crate::raw_input::Info, profile: &Profile) -> Option<f64> {
        let bodies = self
            .bodies
            .iter()
            .filter(|b| {
                normalized(&b.maker) == normalized(&info.make)
                    && normalized(&b.name) == normalized(&info.model)
                    && (profile.mounts.is_empty()
                        || profile
                            .mounts
                            .iter()
                            .any(|m| normalized(m) == normalized(&b.mount)))
            })
            .collect::<Vec<_>>();
        (bodies.len() == 1).then(|| bodies[0].crop)
    }

    pub fn suggest(&self, info: &crate::raw_input::Info) -> Option<Profile> {
        let shooting = &info.shooting;
        let model = shooting.lens_model.as_ref()?;
        let [n, d] = shooting.focal_length?;
        let focal = f64::from(n) / f64::from(d);
        let lenses = self
            .profiles
            .iter()
            .filter(|p| {
                normalized(&p.name) == normalized(model)
                    && shooting
                        .lens_make
                        .as_ref()
                        .is_none_or(|maker| normalized(maker) == normalized(&p.maker))
            })
            .collect::<Vec<_>>();
        // Match the closest compatible calibration before selecting a focal row.
        let camera_maker = normalized(&info.make);
        let camera_name = normalized(&info.model);
        let camera_bodies = self
            .bodies
            .iter()
            .filter(|b| normalized(&b.maker) == camera_maker && normalized(&b.name) == camera_name)
            .map(|b| (b.crop, normalized(&b.mount)))
            .collect::<Vec<_>>();
        let compatible = lenses
            .iter()
            .copied()
            .filter_map(|p| {
                let mut bodies = camera_bodies.iter().filter(|(_, mount)| {
                    p.mounts.is_empty() || p.mounts.iter().any(|m| normalized(m) == *mount)
                });
                let crop = bodies.next()?.0;
                if bodies.next().is_some() {
                    return None;
                }
                let ratio = crop / p.crop;
                (ratio >= 0.96).then_some((p, crop, ratio))
            })
            .collect::<Vec<_>>();
        let best = compatible
            .iter()
            .map(|(_, _, ratio)| *ratio)
            .min_by(f64::total_cmp)?;
        let mut candidates = compatible
            .iter()
            .filter(|(_, _, ratio)| (*ratio - best).abs() < 1e-9)
            .map(|(p, _, _)| *p)
            .collect::<Vec<_>>();
        if candidates.iter().any(|p| p.enabled) {
            candidates.retain(|p| p.enabled);
        }
        let representative = *candidates
            .iter()
            .min_by(|a, b| (a.focal - focal).abs().total_cmp(&(b.focal - focal).abs()))?;
        if candidates.iter().any(|p| {
            p.xml != representative.xml
                || p.calibration != representative.calibration
                || p.crop != representative.crop
                || p.aspect != representative.aspect
                || p.model != representative.model
        }) {
            return None;
        }
        let exact = candidates
            .iter()
            .copied()
            .filter(|p| (p.focal - focal).abs() < 1e-6)
            .collect::<Vec<_>>();
        let mut profile = if exact.len() == 1 {
            exact[0].clone()
        } else if exact.len() > 1 {
            return None;
        } else {
            let mut base = representative.clone();
            base.input_crop = self.body_crop(info, &base)?;
            base.at_focal(focal).ok()?
        };
        profile.input_crop = self.body_crop(info, &profile)?;
        profile.orientation = info.orientation.clamp(1, 8);
        profile.choose_vignetting(info);
        profile.valid().then_some(profile)
    }
}
