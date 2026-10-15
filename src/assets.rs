use crate::api::{Metadata, MAX_BYTES};
use eframe::egui::ColorImage;
use image::{DynamicImage, ImageFormat, ImageReader, Limits};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{BufWriter, Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub enum Source {
    Memory(Arc<Vec<u8>>),
    Disk(PathBuf),
}
impl Source {
    pub fn bytes(&self) -> Result<Arc<Vec<u8>>, String> {
        match self {
            Self::Memory(data) => Ok(data.clone()),
            Self::Disk(path) => {
                let file = fs::File::open(path).map_err(|e| e.to_string())?;
                if file.metadata().map_err(|e| e.to_string())?.len() > MAX_BYTES {
                    return Err("Изображение больше 32 МБ".into());
                }
                let mut bytes = vec![];
                file.take(MAX_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() as u64 > MAX_BYTES {
                    return Err("Изображение больше 32 МБ".into());
                }
                Ok(Arc::new(bytes))
            }
        }
    }
    pub fn memory_bytes(&self) -> usize {
        match self {
            Self::Memory(b) => b.len(),
            _ => 0,
        }
    }
}
pub struct Loaded {
    pub source: Source,
    pub meta: Metadata,
    pub dimensions: [u32; 2],
    pub preview: ColorImage,
    pub thumbnail: ColorImage,
}
fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|e| format!("Не удалось прочитать изображение: {e}"))
}
fn color(image: DynamicImage) -> ColorImage {
    let rgba = image.into_rgba8();
    ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    )
}
pub fn load(source: Source, meta: Metadata) -> Result<Loaded, String> {
    let bytes = source.bytes()?;
    let image = decode(&bytes)?;
    let dimensions = [image.width(), image.height()];
    let thumbnail = color(image.thumbnail(220, 170));
    let preview = color(image.thumbnail(2048, 2048));
    Ok(Loaded {
        source,
        meta,
        dimensions,
        preview,
        thumbnail,
    })
}
pub fn thumbnail(path: &Path) -> Result<ColorImage, String> {
    let bytes = Source::Disk(path.into()).bytes()?;
    Ok(color(decode(&bytes)?.thumbnail(220, 170)))
}
pub fn save_png(source: &Source, folder: &Path) -> Result<PathBuf, String> {
    let bytes = source.bytes()?;
    let image = decode(&bytes)?;
    fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    for i in 0..100 {
        let path = folder.join(format!("waifu_{stamp}_{i}.png"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                let mut writer = BufWriter::new(file);
                let result = image
                    .write_to(&mut writer, ImageFormat::Png)
                    .map_err(|e| e.to_string())
                    .and_then(|_| {
                        use std::io::Write;
                        writer.flush().map_err(|e| e.to_string())
                    });
                drop(writer);
                if let Err(error) = result {
                    let _ = fs::remove_file(&path);
                    return Err(error);
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Не удалось выбрать имя файла".into())
}
pub fn scan(roots: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let mut found = vec![];
    let mut seen = HashSet::new();
    let mut stack: Vec<_> = roots.into_iter().map(|p| (p, 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        if !dir.exists() {
            continue;
        }
        let canonical = dir.canonicalize().map_err(|e| e.to_string())?;
        if !seen.insert(canonical) {
            continue;
        }
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let ty = entry.file_type().map_err(|e| e.to_string())?;
            let path = entry.path();
            if ty.is_dir() && depth < 6 {
                stack.push((path, depth + 1));
            } else if ty.is_file()
                && path.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                    ["png", "jpg", "jpeg", "webp", "gif"].contains(&x.to_lowercase().as_str())
                })
            {
                let time = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(UNIX_EPOCH);
                found.push((time, path));
            }
        }
        if found.len() > 10000 {
            break;
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    found.truncate(2000);
    Ok(found.into_iter().map(|(_, p)| p).collect())
}
pub fn disk_metadata(path: &Path) -> Metadata {
    let mode = if path.components().any(|c| c.as_os_str() == "nsfw") {
        "NSFW"
    } else {
        "SFW"
    };
    Metadata {
        key: path.to_string_lossy().into(),
        source: None,
        artist: String::new(),
        mode: mode.into(),
        tag: "Локальная коллекция".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut out = Cursor::new(vec![]);
        DynamicImage::new_rgb8(2400, 1400)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }
    #[test]
    fn preview_is_separate_from_original_and_png_save() {
        let source = Source::Memory(Arc::new(png()));
        let loaded = load(source.clone(), disk_metadata(Path::new("test.png"))).unwrap();
        assert_eq!(loaded.dimensions, [2400, 1400]);
        assert!(loaded.preview.width() <= 2048);
        assert!(loaded.thumbnail.width() <= 220);
        let folder = std::env::temp_dir().join(format!(
            "waifu-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let a = save_png(&source, &folder).unwrap();
        let b = save_png(&source, &folder).unwrap();
        assert_ne!(a, b);
        assert_eq!(image::image_dimensions(&a).unwrap(), (2400, 1400));
        assert_eq!(scan(vec![folder.clone(), folder.clone()]).unwrap().len(), 2);
        fs::remove_file(a).unwrap();
        fs::remove_file(b).unwrap();
        fs::remove_dir(folder).unwrap();
    }
    #[test]
    fn corrupt_data_is_reported() {
        assert!(decode(b"broken").is_err());
    }
}
