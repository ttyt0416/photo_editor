use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    version: u8,
    pub width: f32,
    pub panels: Vec<crate::panels::State>,
    pub left: bool,
    pub tools: bool,
    pub layers: bool,
    pub properties: bool,
    pub history: bool,
    pub navigator: bool,
    pub colors: bool,
    pub mask_viewer: bool,
    pub mask_overlay: crate::mask_overlay::Settings,
    pub grouped_tools: bool,
    pub remembered: Vec<crate::app::Tool>,
    pub histogram: bool,
    pub pixel_grid: bool,
    pub selection_smooth: bool,
    pub display_sampling: crate::display_sampling::Mode,
    pub shortcuts: crate::shortcuts::Settings,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            version: 1,
            width: 280.,
            panels: crate::panels::defaults(),
            left: false,
            tools: true,
            layers: true,
            properties: true,
            history: true,
            navigator: false,
            colors: false,
            mask_viewer: false,
            mask_overlay: Default::default(),
            grouped_tools: true,
            remembered: vec![],
            histogram: false,
            pixel_grid: false,
            selection_smooth: true,
            display_sampling: Default::default(),
            shortcuts: Default::default(),
        }
    }
}
impl Layout {
    pub fn valid(&self) -> bool {
        crate::panels::valid(&self.panels)
            && self.shortcuts.validate().is_ok()
            && self.mask_overlay.valid()
            && self.version == 1
            && self.width.is_finite()
            && (220.0..=640.).contains(&self.width)
            && self.remembered.len() <= 10
            && self.remembered.iter().enumerate().all(|(i, t)| {
                !self.remembered[..i]
                    .iter()
                    .any(|old| old.group() == t.group())
            })
    }
    pub fn panel_widths(&self, available: f32) -> (f32, f32) {
        let max = (available - 240.).clamp(180., 640.);
        (180_f32.min(max), max)
    }
    pub fn panel_visible(&self, kind: crate::panels::Kind) -> bool {
        use crate::panels::Kind::*;
        match kind {
            Colors => self.colors,
            Navigator => self.navigator,
            Layers => self.layers,
            Properties => self.properties,
            Histogram => self.histogram,
            History => self.history,
        }
    }
    pub fn set_panel_visible(&mut self, kind: crate::panels::Kind, visible: bool) {
        use crate::panels::Kind::*;
        match kind {
            Colors => self.colors = visible,
            Navigator => self.navigator = visible,
            Layers => self.layers = visible,
            Properties => self.properties = visible,
            Histogram => self.histogram = visible,
            History => self.history = visible,
        }
    }
    pub fn sidebar_visible(&self) -> bool {
        self.panels
            .iter()
            .any(|s| !s.floating && self.panel_visible(s.kind))
    }
}
#[derive(Default)]
pub struct Store {
    pub layout: Layout,
    path: Option<PathBuf>,
    saved: Layout,
}
impl Store {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let layout = if path.exists() {
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            if file.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 {
                return Err("작업 화면 설정이 너무 큽니다.".into());
            }
            let mut bytes = Vec::new();
            file.take(64 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let layout: Layout = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if !layout.valid() {
                return Err("지원하지 않는 작업 화면 설정입니다. 기존 파일을 유지합니다.".into());
            }
            layout
        } else {
            Layout::default()
        };
        Ok(Self {
            saved: layout.clone(),
            layout,
            path: Some(path),
        })
    }
    pub fn observe(&mut self, finished: bool) -> Result<(), String> {
        if !finished || self.layout == self.saved {
            return Ok(());
        }
        if !self.layout.valid() {
            return Err("작업 화면 설정이 유효하지 않습니다.".into());
        }
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let bytes = serde_json::to_vec_pretty(&self.layout).map_err(|e| e.to_string())?;
            crate::files::atomic_write(path, |file| {
                file.write_all(&bytes).map_err(|e| e.to_string())
            })?;
        }
        self.saved = self.layout.clone();
        Ok(())
    }
    pub fn stop_persistence(&mut self) {
        self.path = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn widths_dock_visibility_restore_and_corrupt_file_is_preserved() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/workspace");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("layout.json");
        std::fs::write(&path, serde_json::to_vec(&Layout::default()).unwrap()).unwrap();
        let mut store = Store::load(path.clone()).unwrap();
        store.layout.width = 410.;
        store.layout.left = true;
        store.layout.layers = false;
        store.layout.navigator = true;
        store.layout.mask_overlay.enabled = true;
        store.layout.selection_smooth = false;
        let old: Layout = serde_json::from_str("{}").unwrap();
        assert!(old.selection_smooth);
        store.layout.mask_overlay.color = [20, 160, 230];
        store.layout.mask_overlay.opacity = 0.65;
        store
            .layout
            .shortcuts
            .assign(
                crate::shortcuts::Action::Tool(crate::app::Tool::Blur),
                Some((eframe::egui::Key::F7, eframe::egui::Modifiers::NONE)),
            )
            .unwrap();
        store.observe(false).unwrap();
        assert_eq!(Store::load(path.clone()).unwrap().layout.width, 280.);
        store.observe(true).unwrap();
        assert_eq!(Store::load(path.clone()).unwrap().layout, store.layout);
        let (min, max) = store.layout.panel_widths(560.);
        assert_eq!((min, max), (180., 320.));
        assert!(store.layout.sidebar_visible());
        let bad = b"{\"version\":99,\"width\":500}";
        std::fs::write(&path, bad).unwrap();
        assert!(Store::load(path.clone()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bad);
    }
}
