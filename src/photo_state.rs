impl GalleryApp {
    fn toggle_photo_favorite(&mut self, meta: Metadata, path: Option<PathBuf>) {
        let path=path.or_else(||self.saved_sources.get(&meta.key).and_then(|p|p.first()).cloned());
        let favorite=self.photo_favorite(&meta.key)||path.as_ref().is_some_and(|p|self.settings.local_favorites.contains(p));
        if favorite {
            let paths=self.saved_sources.get(&meta.key).cloned().unwrap_or_default();
            self.settings.local_favorites.retain(|p|!paths.contains(p)&&Some(p)!=path.as_ref());
            self.changed();
            if self.cloud_data.favorites.iter().any(|p|p.key==meta.key) {self.cloud_send(CloudJob::Op(serde_json::json!({"op":"favorite_remove","key":meta.key})));}
        }else if self.cloud_user.is_some() && meta.key.starts_with("http") {
            self.cloud_send(CloudJob::Op(serde_json::json!({"op":"favorite_put","picture":meta})));
        }else if let Some(path)=path {self.toggle_local_favorite(path);}
        else {self.account_open=true;}
    }
    fn photo_favorite(&self, key: &str) -> bool {
        self.cloud_data.favorites.iter().any(|p|p.key==key)
            || self.saved_sources.get(key).is_some_and(|paths| paths.iter().any(|p|self.settings.local_favorites.contains(p)))
    }
    fn photo_badges(&self, ui: &mut egui::Ui, entry: &GalleryEntry) {
        let (saved,favorite)=match entry {
            GalleryEntry::Local(path)=>(true,self.settings.local_favorites.contains(path)||self.cloud_data.favorites.iter().any(|m|self.saved_sources.get(&m.key).is_some_and(|paths|paths.contains(path)))),
            GalleryEntry::Remote(meta)=>(self.saved_sources.contains_key(&meta.key),self.photo_favorite(&meta.key)),
        };
        ui.horizontal(|ui|{
            if saved {ui.label(RichText::new("Сохранено").size(11.0).color(self.ac()));}
            if favorite {ui.label(RichText::new("В избранном").size(11.0).color(self.ac()));}
        });
    }
}
