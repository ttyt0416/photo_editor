//! Structurally valid synthetic ICC inputs for color-management tests.
//! MoxCMS 0.8.1 serialization omits D50 and four-byte tag padding; leave the
//! production import and original-file preservation paths unchanged.
use moxcms::ColorProfile;

pub(crate) fn encode(profile: &ColorProfile) -> Vec<u8> {
    let raw = profile.encode().unwrap();
    let count = u32::from_be_bytes(raw[128..132].try_into().unwrap()) as usize;
    let mut bytes = raw[..132 + count * 12].to_vec();
    bytes[68..80].copy_from_slice(&[
        0x00, 0x00, 0xf6, 0xd6, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xd3, 0x2d,
    ]);
    bytes[84..100].fill(0); // An unset optional profile ID is valid.
    for index in 0..count {
        let entry = 132 + index * 12;
        let offset = u32::from_be_bytes(raw[entry + 4..entry + 8].try_into().unwrap()) as usize;
        let length = u32::from_be_bytes(raw[entry + 8..entry + 12].try_into().unwrap()) as usize;
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        let new_offset = bytes.len();
        bytes.extend_from_slice(&raw[offset..offset + length]);
        bytes[entry + 4..entry + 8].copy_from_slice(&(new_offset as u32).to_be_bytes());
    }
    bytes.resize(bytes.len().next_multiple_of(4), 0);
    let length = bytes.len() as u32;
    bytes[..4].copy_from_slice(&length.to_be_bytes());
    ColorProfile::new_from_slice(&bytes).unwrap();
    bytes
}

#[test]
fn profile_fixture_repacking_retains_every_tag_and_repairs_structure() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/icc-structure");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for (name, profile) in [
        ("srgb", ColorProfile::new_srgb()),
        ("p3", ColorProfile::new_display_p3()),
        ("adobe", ColorProfile::new_adobe_rgb()),
        ("gray1", ColorProfile::new_gray_with_gamma(1.)),
        ("gray18", ColorProfile::new_gray_with_gamma(1.8)),
        ("gray22", ColorProfile::new_gray_with_gamma(2.2)),
    ] {
        let raw = profile.encode().unwrap();
        let packed = encode(&profile);
        let count = u32::from_be_bytes(raw[128..132].try_into().unwrap()) as usize;
        for i in 0..count {
            let entry = 132 + i * 12;
            let old = u32::from_be_bytes(raw[entry + 4..entry + 8].try_into().unwrap()) as usize;
            let new = u32::from_be_bytes(packed[entry + 4..entry + 8].try_into().unwrap()) as usize;
            let length =
                u32::from_be_bytes(raw[entry + 8..entry + 12].try_into().unwrap()) as usize;
            assert_eq!(new % 4, 0);
            assert_eq!(&raw[old..old + length], &packed[new..new + length]);
        }
        std::fs::write(root.join(format!("{name}-raw.icc")), raw).unwrap();
        std::fs::write(root.join(format!("{name}.icc")), packed).unwrap();
        cases.push(name);
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
