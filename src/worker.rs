use crate::{
    api::{Api, Metadata, Query},
    assets::{self, Loaded, Source},
    settings::Settings,
    system,
};
use eframe::egui::{ColorImage, Context};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc,
    },
};

pub enum NetJob {
    Tags,
    Image(u64, Query),
    Direct(u64, Metadata),
}
pub enum DiskJob {
    Open(u64, Source, Metadata),
    Thumb(PathBuf),
    Scan(u64, Vec<PathBuf>),
    Save(String, Source, PathBuf),
    Settings(Settings, PathBuf),
    Appearance,
    Flush(Sender<()>),
}
pub enum Event {
    RemoteThumb(String, Result<ColorImage, String>),
    Tags(Result<Vec<String>, String>),
    Image(u64, Result<Box<Loaded>, String>),
    Open(u64, Result<Box<Loaded>, String>),
    Thumb(PathBuf, Result<ColorImage, String>),
    Scan(u64, Result<Vec<PathBuf>, String>),
    Saved(String, Result<PathBuf, String>),
    Settings(Result<(), String>),
    Appearance(system::Appearance),
}
pub struct Workers {
    pub remote_thumbs: SyncSender<Metadata>,
    pub net: SyncSender<NetJob>,
    pub disk: SyncSender<DiskJob>,
    pub events: Receiver<Event>,
}
fn emit(tx: &Sender<Event>, ctx: &Context, event: Event) {
    if tx.send(event).is_ok() {
        ctx.request_repaint();
    }
}
impl Workers {
    pub fn start(ctx: Context) -> Self {
        let (tx, events) = mpsc::channel();
        let (net_tx, net_rx) = mpsc::sync_channel(2);
        let (disk_tx, disk_rx) = mpsc::sync_channel(24);
        let (thumb_tx, thumb_rx) = mpsc::sync_channel::<Metadata>(1);
        let thumb_events = tx.clone();
        let thumb_ctx = ctx.clone();
        std::thread::spawn(move || {
            let api = Api::new();
            while let Ok(meta) = thumb_rx.recv() {
                let key = meta.key.clone();
                let result = api.as_ref().map_err(Clone::clone).and_then(|a| {
                    let d = a.direct(meta)?;
                    assets::load(Source::Memory(Arc::new(d.bytes)), d.meta)
                        .map(|loaded| loaded.thumbnail)
                });
                emit(&thumb_events, &thumb_ctx, Event::RemoteThumb(key, result));
            }
        });
        let nt = tx.clone();
        let nc = ctx.clone();
        std::thread::spawn(move || {
            let api = Api::new();
            while let Ok(job) = net_rx.recv() {
                let event = match job {
                    NetJob::Direct(epoch, meta) => Event::Open(
                        epoch,
                        api.as_ref().map_err(Clone::clone).and_then(|api| {
                            let d = api.direct(meta)?;
                            assets::load(Source::Memory(Arc::new(d.bytes)), d.meta).map(Box::new)
                        }),
                    ),
                    NetJob::Tags => {
                        Event::Tags(api.as_ref().map_err(Clone::clone).and_then(Api::tags))
                    }
                    NetJob::Image(epoch, q) => Event::Image(
                        epoch,
                        api.as_ref().map_err(Clone::clone).and_then(|api| {
                            let d = api.image(&q)?;
                            assets::load(Source::Memory(Arc::new(d.bytes)), d.meta).map(Box::new)
                        }),
                    ),
                };
                emit(&nt, &nc, event);
            }
        });
        std::thread::spawn(move || {
            while let Ok(job) = disk_rx.recv() {
                let event = match job {
                    DiskJob::Open(epoch, source, meta) => {
                        Event::Open(epoch, assets::load(source, meta).map(Box::new))
                    }
                    DiskJob::Thumb(path) => Event::Thumb(path.clone(), assets::thumbnail(&path)),
                    DiskJob::Scan(epoch, roots) => Event::Scan(epoch, assets::scan(roots)),
                    DiskJob::Save(key, source, folder) => {
                        Event::Saved(key, assets::save_png(&source, &folder))
                    }
                    DiskJob::Settings(settings, base) => Event::Settings(settings.save(&base)),
                    DiskJob::Appearance => Event::Appearance(system::read()),
                    DiskJob::Flush(done) => {
                        let _ = done.send(());
                        continue;
                    }
                };
                emit(&tx, &ctx, event);
            }
        });
        Self {
            remote_thumbs: thumb_tx,
            net: net_tx,
            disk: disk_tx,
            events,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_worker_finishes_save_before_flush() {
        use std::io::Cursor;
        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        let folder = std::env::temp_dir().join(format!(
            "waifu-worker-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut encoded = Cursor::new(vec![]);
        image::DynamicImage::new_rgb8(240, 160)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let workers = Workers::start(Context::default());
        workers
            .disk
            .send(DiskJob::Save(
                "key".into(),
                Source::Memory(Arc::new(encoded.into_inner())),
                folder.clone(),
            ))
            .unwrap();
        let (tx, rx) = mpsc::channel();
        workers.disk.send(DiskJob::Flush(tx)).unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let paths = assets::scan(vec![folder.clone()]).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(image::image_dimensions(&paths[0]).unwrap(), (240, 160));
        assert!(matches!(workers.events.try_recv().unwrap(),Event::Saved(key,Ok(_)) if key=="key"));
        std::fs::remove_file(&paths[0]).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
}
