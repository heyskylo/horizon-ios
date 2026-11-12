//! Resource-pack drop confirmation and selection-preserving list refresh.
use super::*;

impl OptionsUi {
    /// Whether the resource pack screen is open (not Create World's data packs on the same
    /// page).
    pub fn resource_packs_open(&self) -> bool {
        self.page() == Page::Packs && self.data_pack_dir.is_none()
    }
    /// Returns whether the Resource Packs screen owns this drop; data packs keep their route.
    pub fn drop_resource_pack(
        &mut self,
        path: std::path::PathBuf,
        atlas: &HudAtlas,
        size: (f32, f32),
    ) -> bool {
        if !self.resource_packs_open() {
            return false;
        }
        if self.resource_error.is_none() {
            self.packs(atlas, size).drop_file(path);
        }
        true
    }
    /// The packs in `resourcepacks/` changed while the pack screen is open
    /// (`PackSelectionScreen`'s directory watcher): the screen is made again over
    /// `infos`, keeping the selection made on it so far.
    pub fn update_pack_infos(&mut self, infos: Vec<crate::pack_screen::PackInfo>) {
        if let Some(p) = self.pack_screen.take()
            && p.changed
        {
            self.pack_selection = p.commit();
            self.pack_reselected = true;
        }
        self.pack_infos = infos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_resource_packs_receive_drops_and_yes_emits_the_import_request() {
        let atlas =
            HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap();
        let size = (480.0, 270.0);
        let mut ui = OptionsUi::default();
        let mut settings = Settings::default();
        assert!(!ui.drop_resource_pack("Ignored.zip".into(), &atlas, size));
        ui.open_at(false, Page::Packs);
        assert!(ui.drop_resource_pack("Pack.zip".into(), &atlas, size));
        // The ConfirmScreen's Yes button, centred at y = height/2 + 15.
        let events = ui.click(
            &mut settings,
            &atlas,
            90.0,
            156.0,
            size,
            [480, 270],
            false,
            false,
        );
        assert!(events.iter().any(|event| matches!(event, Event::ImportPacks(paths) if paths == &[std::path::PathBuf::from("Pack.zip")])));
        ui.data_pack_dir = Some("datapacks".into());
        assert!(!ui.drop_resource_pack("Data.zip".into(), &atlas, size));
    }
}
