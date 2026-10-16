impl GalleryApp {
    fn check_updates(&mut self) {
        if !self.update_busy && self.updates.tx.try_send(self.base.clone()).is_ok() {
            self.update_busy = true;
            self.update_status = "Проверяем и загружаем обновление…".into();
            self.settings.update_last_check = crate::updater::now();
            self.changed();
        }
    }
    fn update_events(&mut self) {
        while let Ok(event) = self.updates.rx.try_recv() {
            self.update_busy = false;
            match event {
                crate::updater::Event::Current => {
                    self.update_status = "Новых опубликованных обновлений нет".into()
                }
                crate::updater::Event::Error(e) => self.update_status = e,
                crate::updater::Event::Ready(ready) => {
                    self.update_status = format!("Версия {} загружена и проверена", ready.version);
                    self.update_ready = Some(ready);
                }
            }
        }
    }
    fn update_banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if let Some(ready) = &self.update_ready {
            let version = ready.version.clone();
            egui::Frame::new()
                .fill(self.ac().gamma_multiply(0.15))
                .corner_radius(10)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Обновление {version} готово"));
                        if ui
                            .add_enabled(
                                self.saving.is_empty(),
                                egui::Button::new("Обновить и перезапустить"),
                            )
                            .clicked()
                        {
                            self.update_restart = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if self.settings.auto_update {
                            ui.small("Установится при закрытии приложения");
                        }
                    });
                });
            ui.add_space(8.0);
        }
    }
    fn update_settings_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.strong(format!("Waifu Gallery {}", env!("CARGO_PKG_VERSION")));
        if ui
            .checkbox(
                &mut self.settings.auto_update,
                "Автоматически обновлять приложение",
            )
            .changed()
        {
            self.changed();
        }
        ui.small("Проверка раз в 12 часов. Проверенное обновление устанавливается после закрытия.");
        if ui
            .add_enabled(!self.update_busy, egui::Button::new("Проверить обновления"))
            .clicked()
        {
            self.check_updates();
        }
        ui.label(&self.update_status);
        let installed = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.join(".installed").is_file()))
            .unwrap_or(false);
        if !installed {
            ui.add_space(8.0);
            if ui
                .button("Установить в Windows и добавить в «Пуск»")
                .clicked()
            {
                self.install_after_exit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
