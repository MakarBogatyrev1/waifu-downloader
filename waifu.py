import os
import json
import queue
import threading
from waifu_client import WaifuClient
from pathlib import Path
from PIL import Image, ImageTk
from customtkinter import CTkImage

import customtkinter as ctk


# ==============================
# SettingsManager
# ==============================

class SettingsManager:
    SETTINGS_FILE = "settings.json"

    def __init__(self):
        self.base_dir = Path.cwd()
        self.images_dir = self.base_dir / "images"

        # ---- defaults ----
        self.appearance_mode = "dark"
        self.color_theme = "dark-blue"

        self.mode = "SFW"  # "SFW" or "NSFW"
        self.category = "waifu"

        # saving: A / B / C (как ты хотел)
        # A — по умолчанию структура (mode/category)
        # B — плоская структура в images/
        # C — пользовательская папка
        self.save_mode = "A"
        self.custom_save_folder = str(self.images_dir)

        # loading mode:
        # 1 — только текущая категория
        # 2 — все категории (по умолчанию)
        # 3 — все папки, где есть изображения
        self.load_saved_mode = 2

        # display settings
        self.thumb_size = 120
        self.max_width = 900
        self.max_height = 700

        # custom URL (если пригодится)
        self.use_custom_url = False
        self.custom_url = ""

        self._ensure_dirs()
        self.load_settings()

    # --------------- filesystem ---------------

    def _ensure_dirs(self):
        self.images_dir.mkdir(exist_ok=True, parents=True)

    # --------------- load / save ---------------

    def load_settings(self):
        if not Path(self.SETTINGS_FILE).exists():
            self.save_settings()
            return
        try:
            with open(self.SETTINGS_FILE, "r", encoding="utf-8") as f:
                data = json.load(f)
        except Exception:
            return

        self.appearance_mode = data.get("appearance_mode", self.appearance_mode)
        self.color_theme = data.get("color_theme", self.color_theme)

        self.mode = data.get("mode", self.mode)
        self.category = data.get("category", self.category)

        self.save_mode = data.get("save_mode", self.save_mode)
        self.custom_save_folder = data.get("custom_save_folder", self.custom_save_folder)

        self.load_saved_mode = data.get("load_saved_mode", self.load_saved_mode)

        self.thumb_size = data.get("thumb_size", self.thumb_size)
        self.max_width = data.get("max_width", self.max_width)
        self.max_height = data.get("max_height", self.max_height)

        self.use_custom_url = data.get("use_custom_url", self.use_custom_url)
        self.custom_url = data.get("custom_url", self.custom_url)

    def save_settings(self):
        data = {
            "appearance_mode": self.appearance_mode,
            "color_theme": self.color_theme,
            "mode": self.mode,
            "category": self.category,
            "save_mode": self.save_mode,
            "custom_save_folder": self.custom_save_folder,
            "load_saved_mode": self.load_saved_mode,
            "thumb_size": self.thumb_size,
            "max_width": self.max_width,
            "max_height": self.max_height,
            "use_custom_url": self.use_custom_url,
            "custom_url": self.custom_url,
        }
        with open(self.SETTINGS_FILE, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)

    # --------------- helpers ---------------

    def get_save_folder_for_current(self):
        """
        Возвращает папку, куда сохранять текущие изображения.
        Зависит от save_mode.
        """
        if self.save_mode == "A":
            folder = self.images_dir / self.mode.lower() / self.category
        elif self.save_mode == "B":
            folder = self.images_dir
        else:
            folder = Path(self.custom_save_folder) if self.custom_save_folder else self.images_dir

        folder.mkdir(parents=True, exist_ok=True)
        return folder

    def list_folders_for_loading(self):
        """
        Возвращает список папок, откуда загружать сохранённые изображения,
        в зависимости от load_saved_mode.
        """
        folders = []
        if self.save_mode == "B":
            folders.append(self.images_dir)
        elif self.save_mode == "C" and self.custom_save_folder:
            folders.append(Path(self.custom_save_folder))

        if self.load_saved_mode == 1:
            # только текущая категория
            folder = self.images_dir / self.mode.lower() / self.category
            if folder.exists():
                folders.append(folder)

        elif self.load_saved_mode == 2:
            # все категории текущего режима
            mode_folder = self.images_dir / self.mode.lower()
            if mode_folder.exists():
                for p in mode_folder.rglob("*"):
                    if p.is_dir():
                        folders.append(p)

        elif self.load_saved_mode == 3:
            # все папки, где есть изображения
            if self.images_dir.exists():
                folders.append(self.images_dir)
                for p in self.images_dir.rglob("*"):
                    if p.is_dir():
                        folders.append(p)

        # убираем дубликаты и только существующие
        unique = []
        seen = set()
        for f in folders:
            f = f.resolve()
            if f not in seen:
                seen.add(f)
                unique.append(f)
        return unique


# ==============================
# GalleryManager
# ==============================

class GalleryManager:
    def __init__(self, parent_frame, settings: SettingsManager):
        self.parent_frame = parent_frame
        self.settings = settings

        # новый и сохранённый списки
        self.new_images = []      # list of (Image, path or None)
        self.saved_images = []    # list of (Image, path)

        # tkinter-объекты
        self.thumbnail_buttons = []
        self.thumbnail_images = []  # чтобы не удалялись GC
        self.current_image = None
        self.current_image_tk = None

        self.current_large_label = None  # сюда App прикрепит label

    # --------------- large image ---------------

    def set_large_label(self, label: ctk.CTkLabel):
        self.current_large_label = label

    def show_image_large(self, pil_image: Image.Image):
        if self.current_large_label is None:
            return

        img = pil_image.copy()
        img.thumbnail((self.settings.max_width, self.settings.max_height), Image.LANCZOS)
        self.current_image = pil_image
        self.current_image_tk = CTkImage(light_image=img, dark_image=img, size=img.size)
        self.current_large_label.configure(image=self.current_image_tk, text="")

    # --------------- thumbnails UI ---------------

    def clear_thumbnails(self):
        for btn in self.thumbnail_buttons:
            btn.destroy()
        self.thumbnail_buttons.clear()
        self.thumbnail_images.clear()

    def rebuild_thumbnails(self):
        """
        Новые — слева, сохранённые — справа.
        """
        self.clear_thumbnails()

        all_items = []

        # сначала новые
        for img, path in self.new_images:
            all_items.append(("new", img, path))

        # потом сохранённые
        for img, path in self.saved_images:
            all_items.append(("saved", img, path))

        thumb_size = self.settings.thumb_size
        col = 0
        for kind, pil_img, path in all_items:
            t = pil_img.copy()
            t.thumbnail((thumb_size, thumb_size), Image.LANCZOS)
            tk_img = CTkImage(light_image=t, dark_image=t, size=(thumb_size, thumb_size))
            self.thumbnail_images.append(tk_img)

            def on_click(img=pil_img):
                self.show_image_large(img)

            btn = ctk.CTkButton(
                self.parent_frame,
                image=tk_img,
                text="",
                width=thumb_size,
                height=thumb_size,
                command=on_click
            )
            btn.grid(row=0, column=col, padx=4, pady=4)
            self.thumbnail_buttons.append(btn)
            col += 1

    # --------------- new image management ---------------

    def add_new_image(self, pil_image: Image.Image):
        self.new_images.insert(0, (pil_image, None))  # новые добавляем в начало
        self.show_image_large(pil_image)
        self.rebuild_thumbnails()

    # --------------- saved images scan ---------------

    def reload_saved_images(self):
        """
        Перечитать сохранённые изображения с диска в зависимости от настроек.
        """
        self.saved_images.clear()

        folders = self.settings.list_folders_for_loading()
        exts = {".png", ".jpg", ".jpeg", ".webp", ".gif"}

        found_paths = []

        for folder in folders:
            if not folder.exists():
                continue
            for p in folder.iterdir():
                if p.is_file() and p.suffix.lower() in exts:
                    found_paths.append(p)

        # чтобы порядок был более-менее стабильным, отсортируем по времени/имени
        found_paths = sorted(found_paths, key=lambda x: x.stat().st_mtime, reverse=True)

        for path in found_paths:
            try:
                img = Image.open(path).convert("RGBA")
                self.saved_images.append((img, path))
            except Exception:
                continue

        self.rebuild_thumbnails()

    # --------------- save current image ---------------

    def save_current_image(self):
        """
        Сохранить текущую картинку на диск в нужную папку.
        """
        if self.current_image is None:
            return False, "Нет изображения для сохранения"

        save_folder = self.settings.get_save_folder_for_current()
        save_folder.mkdir(parents=True, exist_ok=True)

        # простое имя файла
        i = 1
        while True:
            filename = save_folder / f"image_{i:04d}.png"
            if not filename.exists():
                break
            i += 1

        try:
            self.current_image.save(filename)
        except Exception as e:
            return False, f"Ошибка сохранения: {e}"

        # обновим список сохранённых и мини-галерею
        self.reload_saved_images()
        return True, f"Сохранено: {filename}"


# ==============================
# SettingsWindow (модальное)
# ==============================

class SettingsWindow(ctk.CTkToplevel):
    def __init__(self, parent, settings: SettingsManager, on_close_callback):
        super().__init__(parent)
        self.title("Настройки")
        self.geometry("520x480")
        self.resizable(False, False)

        self.settings = settings
        self.on_close_callback = on_close_callback

        # делаем окно модальным
        self.transient(parent)
        self.grab_set()

        # --------------- variables ---------------
        self.var_save_mode = ctk.StringVar(value=self.settings.save_mode)
        self.var_custom_save_folder = ctk.StringVar(value=self.settings.custom_save_folder)

        self.var_use_custom_url = ctk.BooleanVar(value=self.settings.use_custom_url)
        self.var_custom_url = ctk.StringVar(value=self.settings.custom_url)

        self.var_load_saved_mode = ctk.IntVar(value=self.settings.load_saved_mode)

        self.var_thumb_size = ctk.IntVar(value=self.settings.thumb_size)
        self.var_max_width = ctk.IntVar(value=self.settings.max_width)
        self.var_max_height = ctk.IntVar(value=self.settings.max_height)

        # --------------- layout ---------------
        self._build_ui()

        # центрируем относительно родителя
        self._center_over_parent(parent)

        # закрытие
        self.protocol("WM_DELETE_WINDOW", self.on_cancel)

    def _center_over_parent(self, parent):
        self.update_idletasks()
        px = parent.winfo_rootx()
        py = parent.winfo_rooty()
        pw = parent.winfo_width()
        ph = parent.winfo_height()

        w = self.winfo_width()
        h = self.winfo_height()

        x = px + (pw - w) // 2
        y = py + (ph - h) // 2
        self.geometry(f"+{x}+{y}")

    def _build_ui(self):
        main_frame = ctk.CTkFrame(self)
        main_frame.pack(fill="both", expand=True, padx=16, pady=16)

        # ---------- Секция: Сохранение ----------
        label_save = ctk.CTkLabel(main_frame, text="Сохранение", anchor="w", font=ctk.CTkFont(size=14, weight="bold"))
        label_save.pack(fill="x", pady=(0, 4))

        frame_save = ctk.CTkFrame(main_frame)
        frame_save.pack(fill="x", pady=(0, 8))

        # строка выбора режима сохранения
        row1 = ctk.CTkFrame(frame_save)
        row1.pack(fill="x", padx=8, pady=4)

        ctk.CTkLabel(row1, text="Режим сохранения:", width=150, anchor="w").pack(side="left")

        self.save_mode_menu = ctk.CTkOptionMenu(
            row1,
            values=["A", "B", "C"],
            variable=self.var_save_mode,
            command=self._on_save_mode_change
        )
        self.save_mode_menu.pack(side="left", padx=(8, 0))

        # строка выбора папки (для C)
        row2 = ctk.CTkFrame(frame_save)
        row2.pack(fill="x", padx=8, pady=4)

        ctk.CTkLabel(row2, text="Папка для сохранения:", width=150, anchor="w").pack(side="left")

        self.entry_custom_folder = ctk.CTkEntry(row2, textvariable=self.var_custom_save_folder, width=260)
        self.entry_custom_folder.pack(side="left", padx=(8, 8))

        self.btn_browse_folder = ctk.CTkButton(row2, text="...", width=32, command=self._choose_folder)
        self.btn_browse_folder.pack(side="left")

        self._update_save_folder_state()

        # ---------- Секция: Загрузка ----------
        label_load = ctk.CTkLabel(main_frame, text="Загрузка", anchor="w", font=ctk.CTkFont(size=14, weight="bold"))
        label_load.pack(fill="x", pady=(8, 4))

        frame_load = ctk.CTkFrame(main_frame)
        frame_load.pack(fill="x", pady=(0, 8))

        # Использовать пользовательский URL
        row3 = ctk.CTkFrame(frame_load)
        row3.pack(fill="x", padx=8, pady=4)

        self.chk_custom_url = ctk.CTkCheckBox(
            row3,
            text="Использовать пользовательский URL",
            variable=self.var_use_custom_url,
            command=self._on_use_custom_url_change
        )
        self.chk_custom_url.pack(side="left", anchor="w")

        # поле URL
        row4 = ctk.CTkFrame(frame_load)
        row4.pack(fill="x", padx=8, pady=4)

        ctk.CTkLabel(row4, text="URL:", width=40, anchor="w").pack(side="left")
        self.entry_custom_url = ctk.CTkEntry(row4, textvariable=self.var_custom_url)
        self.entry_custom_url.pack(side="left", fill="x", expand=True, padx=(8, 0))

        self._update_custom_url_state()

        # OptionMenu варианта загрузки сохранённых
        row5 = ctk.CTkFrame(frame_load)
        row5.pack(fill="x", padx=8, pady=4)

        ctk.CTkLabel(row5, text="Как загружать сохранённые изображения:", anchor="w").pack(
            side="left", padx=(0, 8)
        )

        self.load_mode_menu = ctk.CTkOptionMenu(
            row5,
            values=[
                "1 — Только текущая категория",
                "2 — Все категории",
                "3 — Все папки с изображениями"
            ],
            command=self._on_load_mode_select
        )
        # установить текущий текст
        self._sync_load_mode_menu_text()
        self.load_mode_menu.pack(side="left", fill="x", expand=True)

        # ---------- Секция: Отображение ----------
        label_display = ctk.CTkLabel(main_frame, text="Отображение", anchor="w",
                                     font=ctk.CTkFont(size=14, weight="bold"))
        label_display.pack(fill="x", pady=(8, 4))

        frame_display = ctk.CTkFrame(main_frame)
        frame_display.pack(fill="x", pady=(0, 8))

        # thumb size
        row6 = ctk.CTkFrame(frame_display)
        row6.pack(fill="x", padx=8, pady=4)
        ctk.CTkLabel(row6, text="Размер миниатюр:", width=150, anchor="w").pack(side="left")
        ctk.CTkEntry(row6, textvariable=self.var_thumb_size, width=80).pack(side="left", padx=(8, 0))

        # max width
        row7 = ctk.CTkFrame(frame_display)
        row7.pack(fill="x", padx=8, pady=4)
        ctk.CTkLabel(row7, text="Макс. ширина:", width=150, anchor="w").pack(side="left")
        ctk.CTkEntry(row7, textvariable=self.var_max_width, width=80).pack(side="left", padx=(8, 0))

        # max height
        row8 = ctk.CTkFrame(frame_display)
        row8.pack(fill="x", padx=8, pady=4)
        ctk.CTkLabel(row8, text="Макс. высота:", width=150, anchor="w").pack(side="left")
        ctk.CTkEntry(row8, textvariable=self.var_max_height, width=80).pack(side="left", padx=(8, 0))

        # ---------- Кнопки ----------
        buttons_frame = ctk.CTkFrame(self)
        buttons_frame.pack(fill="x", padx=16, pady=(0, 16))

        btn_cancel = ctk.CTkButton(buttons_frame, text="Отмена", command=self.on_cancel)
        btn_cancel.pack(side="right", padx=(8, 0))

        btn_ok = ctk.CTkButton(buttons_frame, text="Сохранить", command=self.on_ok)
        btn_ok.pack(side="right")

    # --------------- handlers ---------------

    def _on_save_mode_change(self, *_):
        self._update_save_folder_state()

    def _update_save_folder_state(self):
        if self.var_save_mode.get() == "C":
            state = "normal"
        else:
            state = "disabled"
        self.entry_custom_folder.configure(state=state)
        self.btn_browse_folder.configure(state=state)

    def _choose_folder(self):
        from tkinter import filedialog
        folder = filedialog.askdirectory(initialdir=self.var_custom_save_folder.get() or os.getcwd())
        if folder:
            self.var_custom_save_folder.set(folder)

    def _on_use_custom_url_change(self):
        self._update_custom_url_state()

    def _update_custom_url_state(self):
        state = "normal" if self.var_use_custom_url.get() else "disabled"
        self.entry_custom_url.configure(state=state)

    def _on_load_mode_select(self, text):
        if text.startswith("1"):
            self.var_load_saved_mode.set(1)
        elif text.startswith("2"):
            self.var_load_saved_mode.set(2)
        elif text.startswith("3"):
            self.var_load_saved_mode.set(3)

    def _sync_load_mode_menu_text(self):
        if self.var_load_saved_mode.get() == 1:
            self.load_mode_menu.set("1 — Только текущая категория")
        elif self.var_load_saved_mode.get() == 2:
            self.load_mode_menu.set("2 — Все категории")
        else:
            self.load_mode_menu.set("3 — Все папки с изображениями")

    # --------------- OK / Cancel ---------------

    def on_ok(self):
        # забираем значения
        self.settings.save_mode = self.var_save_mode.get()
        self.settings.custom_save_folder = self.var_custom_save_folder.get()

        self.settings.use_custom_url = self.var_use_custom_url.get()
        self.settings.custom_url = self.var_custom_url.get()

        self.settings.load_saved_mode = int(self.var_load_saved_mode.get())

        self.settings.thumb_size = int(self.var_thumb_size.get())
        self.settings.max_width = int(self.var_max_width.get())
        self.settings.max_height = int(self.var_max_height.get())

        self.settings.save_settings()

        if self.on_close_callback:
            self.on_close_callback(apply_changes=True)

        self.destroy()

    def on_cancel(self):
        if self.on_close_callback:
            self.on_close_callback(apply_changes=False)
        self.destroy()


# ==============================
# App
# ==============================

class App(ctk.CTk):
    DEFAULT_TAGS = ["waifu"]

    def __init__(self):
        super().__init__()

        # settings
        self.settings = SettingsManager()
        self.client = WaifuClient()
        self.tags = list(self.DEFAULT_TAGS)
        # Keep the old category until the API confirms whether it is valid.
        self._saved_category = self.settings.category
        self._events = queue.Queue()
        self._loading = False
        self._closed = False
        self._generation = 0
        ctk.set_appearance_mode(self.settings.appearance_mode)  # "dark"
        ctk.set_default_color_theme(self.settings.color_theme)  # "dark-blue"

        self.title("Waifu Gallery")
        self.geometry("1100x800")
        self.minsize(900, 700)

        # main layout
        self._build_ui()

        # gallery manager
        self.gallery = GalleryManager(self.gallery_frame, self.settings)
        self.gallery.set_large_label(self.image_label)

        # применяем начальные настройки
        self._apply_initial_settings()

        # загружаем сохранённые изображения
        self.gallery.reload_saved_images()
        self.protocol("WM_DELETE_WINDOW", self.on_close)
        self._poll_id = self.after(50, self._poll_events)
        self._start_worker("tags", self.client.tags)

    # --------------- UI ---------------

    def _build_ui(self):
        # Верхняя панель (одна линия)
        top_frame = ctk.CTkFrame(self)
        top_frame.pack(fill="x", padx=8, pady=8)

        # Mode OptionMenu
        self.var_mode = ctk.StringVar(value=self.settings.mode)
        self.mode_menu = ctk.CTkOptionMenu(
            top_frame,
            values=["SFW", "NSFW"],
            variable=self.var_mode,
            command=self.on_mode_change
        )
        self.mode_menu.pack(side="left", padx=(4, 4))

        # Category OptionMenu
        self.var_category = ctk.StringVar(value="waifu")
        self.category_menu = ctk.CTkOptionMenu(
            top_frame,
            values=self._get_categories_for_mode(self.settings.mode),
            variable=self.var_category,
            command=self.on_category_change
        )
        self.category_menu.pack(side="left", padx=(4, 12))

        # Кнопка "Следующая"
        self.btn_next = ctk.CTkButton(top_frame, text="Следующая", command=self.on_next)
        self.btn_next.pack(side="left", padx=(4, 4))

        # Кнопка "Сохранить"
        self.btn_save = ctk.CTkButton(top_frame, text="Сохранить", command=self.on_save)
        self.btn_save.pack(side="left", padx=(4, 4))

        # Иконка ⚙ (кнопка настроек)
        self.btn_settings = ctk.CTkButton(top_frame, text="⚙", width=32, command=self.open_settings)
        self.btn_settings.pack(side="right", padx=(4, 4))

        # Большое изображение
        center_frame = ctk.CTkFrame(self)
        center_frame.pack(fill="both", expand=True, padx=8, pady=(0, 8))

        self.image_label = ctk.CTkLabel(center_frame, text="Нет изображения", anchor="center")
        self.image_label.pack(fill="both", expand=True, padx=8, pady=8)

        # Мини-галерея
        bottom_frame = ctk.CTkFrame(self)
        bottom_frame.pack(fill="x", padx=8, pady=(0, 8))

        self.gallery_frame = ctk.CTkFrame(bottom_frame)
        self.gallery_frame.pack(fill="x", padx=4, pady=4)

        # статус-бар
        self.status_var = ctk.StringVar(value="")
        self.status_label = ctk.CTkLabel(self, textvariable=self.status_var, anchor="w")
        self.status_label.pack(fill="x", padx=8, pady=(0, 4))

    # --------------- helpers ---------------

    def _get_categories_for_mode(self, mode: str):
        return self.tags

    def _apply_initial_settings(self):
        # применяем сохранённые режим и категорию (если валидны)
        mode = self.settings.mode if self.settings.mode in ["SFW", "NSFW"] else "SFW"
        self.settings.mode = mode
        self.var_mode.set(mode)

        categories = self._get_categories_for_mode(mode)
        self.category_menu.configure(values=categories)

        if self.settings.category not in categories:
            self.settings.category = categories[0]
        self.var_category.set(self.settings.category)

        # подстроим размер окна
        self.update_idletasks()

    # --------------- event handlers ---------------

    def on_mode_change(self, value):
        self._generation += 1
        self.settings.mode = value

        # обновляем категории
        categories = self._get_categories_for_mode(value)
        self.category_menu.configure(values=categories)

        if self.settings.category not in categories:
            self.settings.category = categories[0]
        self.var_category.set(self.settings.category)

        self.settings.save_settings()

        # при смене режима перезагружаем сохранённые
        self.gallery.reload_saved_images()
        self.set_status(f"Режим: {self.settings.mode}, категория: {self.settings.category}")

    def on_category_change(self, value):
        self._generation += 1
        self._saved_category = value
        self.settings.category = value
        self.settings.save_settings()
        # при смене категории можно перезагрузить сохранённые (зависит от настроек)
        self.gallery.reload_saved_images()
        self.set_status(f"Режим: {self.settings.mode}, категория: {self.settings.category}")

    def _start_worker(self, kind, operation, generation=None):
        events = self._events

        def work():
            try:
                events.put((kind, generation, operation(), None))
            except Exception as error:
                events.put((kind, generation, None, str(error)))
        threading.Thread(target=work, daemon=True).start()

    def _poll_events(self):
        if self._closed:
            return
        try:
            while True:
                kind, generation, result, error = self._events.get_nowait()
                if kind == "tags":
                    if error:
                        self.set_status(f"Не удалось загрузить теги: {error}. Доступен тег waifu.")
                    else:
                        self.tags = result
                        candidate = self._saved_category
                        self.settings.category = (candidate if candidate in result else
                                                  "waifu" if "waifu" in result else result[0])
                        self.var_category.set(self.settings.category)
                        self.category_menu.configure(values=result)
                        self._generation += 1
                        self.settings.save_settings()
                        self.gallery.reload_saved_images()
                        self.set_status("Теги Waifu.im загружены")
                elif kind == "image":
                    self._loading = False
                    self.btn_next.configure(state="normal")
                    if generation != self._generation:
                        self.set_status("Параметры изменились. Нажмите «Следующая» для новой загрузки.")
                    elif error:
                        self.set_status(f"Ошибка загрузки: {error}")
                    else:
                        self.gallery.add_new_image(result)
                        self.set_status("Изображение загружено")
        except queue.Empty:
            pass
        self._poll_id = self.after(50, self._poll_events)

    def on_next(self):
        if self._loading or self._closed:
            return
        mode, tag = self.settings.mode, self.settings.category
        custom_url = (self.settings.custom_url.strip()
                      if self.settings.use_custom_url else None)
        if self.settings.use_custom_url and not custom_url:
            self.set_status("Укажите пользовательский URL в настройках")
            return
        self._loading = True
        self.btn_next.configure(state="disabled")
        self.set_status("Загрузка изображения...")
        self._start_worker("image", lambda: self.client.image(mode, tag, custom_url),
                           self._generation)

    def on_close(self):
        self._closed = True
        for callback in self.tk.splitlist(self.tk.call("after", "info")):
            self.after_cancel(callback)
        self.destroy()

    def on_save(self):
        ok, msg = self.gallery.save_current_image()
        self.set_status(msg)

    def open_settings(self):
        # модальное окно настроек
        SettingsWindow(self, self.settings, on_close_callback=self.on_settings_closed)

    def on_settings_closed(self, apply_changes: bool):
        if apply_changes:
            self._generation += 1
            # пересобираем галерею, т.к. изменились режимы загрузки и отображения
            self.gallery.reload_saved_images()
            self.set_status("Настройки применены")
        else:
            self.set_status("Настройки отменены")

    def set_status(self, text: str):
        self.status_var.set(text)


if __name__ == "__main__":
    app = App()
    app.mainloop()