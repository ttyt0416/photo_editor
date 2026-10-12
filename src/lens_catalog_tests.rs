use super::*;
use crate::lens_profile::load;
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lens-profiles")
        .join(name)
}
#[test]
fn catalog_merge_deduplicates_preserves_conflicts_assets_and_atomic_import() {
    let db = load(&fixture("synthetic.xml")).unwrap();
    let duplicate = load(&fixture("synthetic.xml")).unwrap();
    let merged = db.merge(&duplicate).unwrap();
    assert_eq!(merged.profiles, db.profiles);
    assert_eq!(merged.bodies.len(), db.bodies.len());
    assert_eq!(merged.notes, db.notes);
    assert!(Arc::ptr_eq(&merged.profiles[0].xml, &db.profiles[0].xml));
    let combined = db
        .import(&[
            fixture("tca.xml"),
            fixture("interpolation.xml"),
            fixture("synthetic.xml"),
        ])
        .unwrap();
    assert_eq!(combined.profiles.len(), db.profiles.len() + 8 + 24);
    let before = db.profiles.clone();
    assert!(
        db.import(&[fixture("tca.xml"), fixture("missing.xml")])
            .is_err()
    );
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/lens-catalog");
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("invalid.xml");
    std::fs::write(&bad, "<bad/>").unwrap();
    assert!(db.import(&[fixture("tca.xml"), bad]).is_err());
    assert_eq!(db.profiles, before);
    let mut changed = db.clone();
    changed.profiles[0].coefficients[0] += 0.0001;
    let conflict = db.merge(&changed).unwrap();
    assert_eq!(conflict.profiles.len(), db.profiles.len() + 1);
    let view = View::new(&conflict);
    assert_ne!(view.label(0), view.label(conflict.profiles.len() - 1));
    assert!(!same_selection(
        &conflict.profiles[0],
        conflict.profiles.last().unwrap()
    ));
    std::fs::write(dir.join("summary.json"), serde_json::to_vec_pretty(&serde_json::json!({"combined_profiles": combined.profiles.len(), "deduplicated_profiles":merged.profiles.len(), "preserved_conflicts":conflict.profiles.len()-db.profiles.len(),"labels":view.labels})).unwrap()).unwrap();
}
#[test]
fn catalog_search_and_budget_limits_do_not_modify_source() {
    let db = load(&fixture("synthetic.xml")).unwrap();
    let view = View::new(&db);
    assert_eq!(view.find("  TEST   pTlEnS 35 ").len(), 1);
    assert_eq!(view.find("").len(), db.profiles.len());
    assert!(view.find("does-not-exist").is_empty());
    assert!(db.import(&vec![fixture("synthetic.xml"); 257]).is_err());
    let mut invalid = db.clone();
    invalid.bodies[0].crop = f64::NAN;
    assert!(db.merge(&invalid).is_err());
    let base = load(&fixture("tca.xml")).unwrap().profiles[0].clone();
    let mut huge = Database {
        profiles: Vec::new(),
        bodies: Vec::new(),
        notes: Vec::new(),
    };
    for i in 0..=RECORD_LIMIT {
        let mut p = base.clone();
        p.focal = 0.1 + i as f64 * 0.01;
        huge.profiles.push(p);
    }
    assert!(db.merge(&huge).unwrap_err().contains("개수"));
    huge.profiles.clear();
    huge.notes.push("x".repeat(BYTE_LIMIT + 1));
    assert!(db.merge(&huge).unwrap_err().contains("예산"));
    assert_eq!(db.profiles.len(), 5);
}
#[test]
fn catalog_auto_selection_chooses_compatible_crop_and_refuses_equal_conflicts() {
    let doc = crate::files::open(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/raw-metadata/metadata-le.dng"),
    )
    .unwrap()
    .document;
    let info = &doc.layers[0].raw_source.as_ref().unwrap().record.info;
    let mut db = load(&fixture("synthetic.xml")).unwrap();
    let mut other = db.profiles[0].clone();
    other.crop = 2.;
    other.calibration = 1;
    db.profiles.push(other);
    assert_eq!(db.suggest(info).unwrap().crop, 1.5);
    let mut full_frame = db.profiles[0].clone();
    full_frame.crop = 1.;
    full_frame.calibration = 2;
    db.profiles.push(full_frame);
    assert_eq!(db.suggest(info).unwrap().crop, 1.5);
    let mut info = info.clone();
    info.model = "other".into();
    db.bodies.push(super::super::xml::Body {
        maker: info.make.clone(),
        name: info.model.clone(),
        crop: 1.,
        mount: db.bodies[0].mount.clone(),
    });
    assert_eq!(db.suggest(&info).unwrap().crop, 1.);
    let mut conflicting = db.profiles.last().unwrap().clone();
    conflicting.coefficients[0] += 0.0001;
    db.profiles.push(conflicting);
    assert!(db.suggest(&info).is_none());
    db.profiles.pop();
    db.profiles.retain(|p| p.crop > 1.);
    assert!(db.suggest(&info).is_none());
}
