use crate::{api::Metadata, assets::Loaded};
use eframe::egui::TextureHandle;
use std::{collections::VecDeque, path::PathBuf};
#[derive(Clone)]
pub enum Entry {
    Local(PathBuf),
    Remote(Metadata),
}
impl Entry {
    pub fn key(&self) -> String {
        match self {
            Self::Local(p) => p.to_string_lossy().into(),
            Self::Remote(m) => m.key.clone(),
        }
    }
}
pub struct Viewer {
    pub entries: Vec<Entry>,
    pub index: usize,
    pub requested_index: usize,
    pub epoch: u64,
    pub busy: bool,
    pub texture: Option<TextureHandle>,
    pub error: Option<String>,
    pub zoom: f32,
    pub cache: VecDeque<(String, Loaded)>,
}
impl Viewer {
    pub fn new(entries: Vec<Entry>, index: usize) -> Self {
        Self {
            entries,
            index,
            requested_index: index,
            epoch: 0,
            busy: false,
            texture: None,
            error: None,
            zoom: 1.0,
            cache: VecDeque::new(),
        }
    }
    pub fn step(&mut self, delta: isize) {
        self.index = (self.index as isize + delta).rem_euclid(self.entries.len() as isize) as usize;
        self.texture = None;
        self.error = None;
        self.zoom = 1.0;
    }
    pub fn loaded(&self) -> Option<&Loaded> {
        let key = self.entries[self.index].key();
        self.cache.iter().find(|(k, _)| k == &key).map(|(_, l)| l)
    }
    pub fn insert(&mut self, key: String, loaded: Loaded) {
        self.cache.retain(|(k, _)| k != &key);
        self.cache.push_front((key, loaded));
        while self.cache.len() > 1
            && (self.cache.len() > 3
                || self
                    .cache
                    .iter()
                    .map(|(_, l)| {
                        l.source.memory_bytes()
                            + l.preview.pixels.len() * 4
                            + l.thumbnail.pixels.len() * 4
                    })
                    .sum::<usize>()
                    > 64 * 1024 * 1024)
        {
            self.cache.pop_back();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browsing_wraps_without_changing_the_list() {
        let entries = vec![
            Entry::Local("a.png".into()),
            Entry::Local("b.png".into()),
            Entry::Local("c.png".into()),
        ];
        let mut g = Viewer::new(entries, 0);
        g.step(-1);
        assert_eq!(g.index, 2);
        g.step(1);
        assert_eq!(g.index, 0);
        g.step(1);
        assert_eq!(g.index, 1);
        assert_eq!(g.entries[0].key(), "a.png");
    }
}
