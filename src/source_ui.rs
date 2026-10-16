impl GalleryApp {
    fn set_source_tags(&mut self, tags: Vec<String>) {
        if tags.is_empty() {return;}
        if !tags.contains(&self.settings.category) {self.settings.category=tags[0].clone();self.invalidate();}
        self.tags=tags;
    }
    fn refresh_source_tags(&mut self) {
        if !self.settings.use_custom_url {
            self.source_busy=false;self.source_status.clear();
            self.tags_loading=self.workers.net.try_send(NetJob::Tags).is_ok();return;
        }
        let profile=self.settings.sources.iter().find(|p|p.url==self.settings.custom_url).cloned()
            .unwrap_or(crate::sources::SourceProfile {url:self.settings.custom_url.clone(),..Default::default()});
        // Clear categories belonging to a different source/mode before discovery completes.
        self.tags=if profile.categories.is_empty() || profile.categories_mode!=self.settings.mode{vec!["waifu".into()]}else{profile.categories.clone()};
        self.source_busy=self.workers.net.try_send(NetJob::SourceTags(profile,self.settings.mode.clone())).is_ok();
        self.tags_loading=self.source_busy;
        self.source_status=if self.source_busy {"Ищем категории…".into()}else{"Очередь занята. Нажмите обновление категорий.".into()};
    }
    fn sources_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);ui.separator();ui.strong("Источники фотографий");
        let mut choose=None;let mut remove=None;
        if ui.selectable_label(!self.settings.use_custom_url,"Waifu.im (встроенный)").clicked(){choose=Some(None);}
        for (i,p) in self.settings.sources.iter().enumerate() {
            ui.push_id(i,|ui|{ui.horizontal(|ui|{
                if ui.selectable_label(self.settings.use_custom_url&&self.settings.custom_url==p.url,&p.name).clicked(){choose=Some(Some(i));}
                if ui.small_button("Удалить").clicked(){remove=Some(i);}
            });});
        }
        if let Some(i)=remove {let p=self.settings.sources.remove(i);if self.settings.custom_url==p.url{choose=Some(None);}}
        if let Some(selection)=choose {
            self.settings.use_custom_url=selection.is_some();
            if let Some(i)=selection {self.settings.custom_url=self.settings.sources[i].url.clone();}
            self.tags=vec!["waifu".into()];self.settings.category="waifu".into();self.invalidate();self.refresh_source_tags();
        }
        ui.collapsing("Добавить API по ссылке",|ui|{
            ui.add(egui::TextEdit::singleline(&mut self.source_draft.name).hint_text("Название (необязательно)").desired_width(f32::INFINITY));
            ui.add(egui::TextEdit::singleline(&mut self.source_draft.url).hint_text("URL картинки / JSON API").desired_width(f32::INFINITY));
            ui.add(egui::TextEdit::singleline(&mut self.source_draft.categories_url).hint_text("URL категорий — необязательно").desired_width(f32::INFINITY));
            ui.small("В URL можно использовать {category}, {tag} и {mode} (sfw/nsfw). JSON: url, image_url, file_url, вложенные data/items/images; также прямые картинки.");
            if ui.add_enabled(!self.source_busy,egui::Button::new("Добавить и найти категории")).clicked(){
                let mut p=self.source_draft.clone();p.url=p.url.trim().to_owned();p.categories_url=p.categories_url.trim().to_owned();
                let parsed=reqwest::Url::parse(&crate::sources::expand(&p.url,"waifu","SFW"));
                if let Some(url)=parsed.ok().filter(|u|["http","https"].contains(&u.scheme())&&u.username().is_empty()&&u.password().is_none()) {
                    if p.name.trim().is_empty(){p.name=url.host_str().unwrap_or("API").into();}
                    if let Some(existing)=self.settings.sources.iter_mut().find(|s|s.url==p.url){*existing=p.clone();}else{self.settings.sources.push(p.clone());}
                    self.settings.use_custom_url=true;self.settings.custom_url=p.url;self.settings.category="waifu".into();self.source_draft=Default::default();self.invalidate();self.refresh_source_tags();
                }else{self.source_status="Укажите полный HTTP(S) URL без логина и пароля".into();}
            }
        });
        if self.settings.use_custom_url {
            ui.label(format!("Активный API: {}",self.settings.custom_url));
            if ui.add_enabled(!self.source_busy,egui::Button::new("Обновить категории")).clicked(){self.refresh_source_tags();}
            ui.collapsing("Категории вручную",|ui|{
                ui.small("Если API не публикует список, добавьте названия по одному.");
                ui.horizontal(|ui|{ui.text_edit_singleline(&mut self.source_draft.name);if ui.button("Добавить категорию").clicked(){let tag=self.source_draft.name.trim().to_string();if crate::sources::category_ok(&tag){if !self.tags.contains(&tag){self.tags.push(tag.clone());}if let Some(p)=self.settings.sources.iter_mut().find(|p|p.url==self.settings.custom_url){p.categories=self.tags.clone();p.categories_mode=self.settings.mode.clone();}self.settings.category=tag;self.source_draft.name.clear();self.invalidate();}}});
            });
            ui.small("Режим и категории применяются через шаблон URL. Фильтрацию содержимого выполняет выбранный API. Ключи авторизации пока не поддерживаются.");
        }
        if self.source_busy {ui.spinner();}
        if !self.source_status.is_empty(){ui.label(&self.source_status);}
    }
}
