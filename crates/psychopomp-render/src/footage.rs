//! The footage store: every source a prepared plan shows, opened once and
//! shared. A still decodes once and stays in memory; a video or image
//! sequence decodes once into a seekable cache on disk (shared by every actor
//! and plan that shows it at the same size), and its frames are read on
//! demand through one bounded, least-recently-used set, so a wall of clips
//! holds only the frames on screen.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};

use crate::{
    render::decode_image,
    video::{Decode, VideoFrameCache},
};

/// One decoded RGBA frame, shared by everything that shows it.
pub(crate) type Frame = Arc<[u8]>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SourceId(usize);

/// The frame a Stage footage element shows in one sample.
pub(crate) struct StageFootageFrame {
    pub element: String,
    /// Equal identities are equal pixels.
    pub identity: u64,
    pub pixels: Frame,
    pub size: [u32; 2],
}

enum Source {
    Still {
        pixels: Frame,
        size: [u32; 2],
    },
    /// Decoded frames, the first at `start` source nanoseconds.
    Video {
        cache: VideoFrameCache,
        start: u64,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum Key {
    Still(PathBuf, u32),
    Video(Decode),
}

/// Reads and hits since the store opened, for measuring a wall.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Stats {
    pub reads: u64,
    pub hits: u64,
    pub evictions: u64,
    pub peak_bytes: usize,
}

pub(crate) struct FootageStore {
    keys: Vec<Key>,
    sources: Vec<Source>,
    frames: HashMap<(usize, u64), (Frame, u64)>,
    bytes: usize,
    budget: usize,
    clock: u64,
    stats: Stats,
    directory: PathBuf,
}

/// Decoded frames kept in memory, by default: enough for a few hundred
/// small tiles or a few dozen 1080p frames. `PSYCHOPOMP_FOOTAGE_CACHE_MB`
/// overrides it.
const DEFAULT_BUDGET_MB: usize = 384;

impl FootageStore {
    pub(crate) fn new() -> Self {
        let megabytes = std::env::var("PSYCHOPOMP_FOOTAGE_CACHE_MB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_BUDGET_MB);
        Self::with_budget(megabytes << 20, default_directory())
    }

    pub(crate) fn with_budget(budget: usize, directory: PathBuf) -> Self {
        Self {
            keys: Vec::new(),
            sources: Vec::new(),
            frames: HashMap::new(),
            bytes: 0,
            budget,
            clock: 0,
            stats: Stats::default(),
            directory,
        }
    }

    fn find(&self, key: &Key) -> Option<SourceId> {
        self.keys.iter().position(|k| k == key).map(SourceId)
    }

    fn insert(&mut self, key: Key, source: Source) -> SourceId {
        self.keys.push(key);
        self.sources.push(source);
        SourceId(self.sources.len() - 1)
    }

    /// A PNG, JPEG, or WebP, decoded once and halved until it is at most
    /// twice `width` wide.
    pub(crate) fn still(&mut self, path: &Path, width: f32) -> Result<SourceId> {
        let key = Key::Still(path.to_owned(), width.to_bits());
        if let Some(id) = self.find(&key) {
            return Ok(id);
        }
        let bytes =
            std::fs::read(path).with_context(|| format!("read image {}", path.display()))?;
        let image = decode_image(&bytes)
            .with_context(|| format!("decode image {}", path.display()))?
            .reduced_for(width);
        Ok(self.insert(
            key,
            Source::Still {
                pixels: image.pixels.into(),
                size: image.size,
            },
        ))
    }

    /// A video or image sequence, decoded once into the shared disk cache.
    pub(crate) fn video(&mut self, decode: Decode) -> Result<SourceId> {
        let key = Key::Video(decode);
        if let Some(id) = self.find(&key) {
            return Ok(id);
        }
        let Key::Video(decode) = &key else {
            unreachable!()
        };
        let cache = VideoFrameCache::open(decode, &self.directory)?;
        let start = decode.range.map_or(0, |(from, _)| from);
        Ok(self.insert(key, Source::Video { cache, start }))
    }

    pub(crate) fn size(&self, id: SourceId) -> [u32; 2] {
        match &self.sources[id.0] {
            Source::Still { size, .. } => *size,
            Source::Video { cache, .. } => cache.size(),
        }
    }

    /// Where the decoded source ends, in source nanoseconds: its decoded
    /// frames at its rate after its start, or zero for a still.
    pub(crate) fn end_nanos(&self, id: SourceId) -> u64 {
        match &self.sources[id.0] {
            Source::Still { .. } => 0,
            Source::Video { cache, start } => {
                start + cache.frame_count() * 1_000_000_000 / u64::from(cache.fps())
            }
        }
    }

    /// The frame showing `source_nanos` into the source.
    pub(crate) fn frame_index(&self, id: SourceId, source_nanos: u64) -> u64 {
        match &self.sources[id.0] {
            Source::Still { .. } => 0,
            Source::Video { cache, start } => {
                cache.frame_index_at(source_nanos.saturating_sub(*start) as f32 / 1_000_000_000.0)
            }
        }
    }

    /// A frame's identity across the store, for visual sample keys and GPU
    /// uploads: equal identities are equal pixels.
    pub(crate) fn identity(&self, id: SourceId, index: u64) -> u64 {
        ((id.0 as u64) << 40) | index
    }

    pub(crate) fn frame(&mut self, id: SourceId, index: u64) -> Result<Frame> {
        let source = &mut self.sources[id.0];
        let cache = match source {
            Source::Still { pixels, .. } => return Ok(pixels.clone()),
            Source::Video { cache, .. } => cache,
        };
        self.clock += 1;
        if let Some((frame, used)) = self.frames.get_mut(&(id.0, index)) {
            *used = self.clock;
            self.stats.hits += 1;
            return Ok(frame.clone());
        }
        let frame: Frame = cache.frame_at_index(index)?.into();
        self.stats.reads += 1;
        self.bytes += frame.len();
        self.frames
            .insert((id.0, index), (frame.clone(), self.clock));
        self.stats.peak_bytes = self.stats.peak_bytes.max(self.bytes);
        while self.bytes > self.budget && self.frames.len() > 1 {
            let Some((&oldest, _)) = self
                .frames
                .iter()
                .filter(|(key, _)| **key != (id.0, index))
                .min_by_key(|(_, (_, used))| *used)
            else {
                break;
            };
            if let Some((evicted, _)) = self.frames.remove(&oldest) {
                self.bytes -= evicted.len();
                self.stats.evictions += 1;
            }
        }
        Ok(frame)
    }

    pub(crate) fn stats(&self) -> Stats {
        self.stats
    }

    #[cfg(test)]
    fn resident_bytes(&self) -> usize {
        self.bytes
    }
}

/// Decoded caches named by the source's content and decode contract. Safe to
/// delete; it refills on demand. `PSYCHOPOMP_CACHE_DIR` chooses the parent.
/// A renderer built in its workspace (`cargo run`) uses that workspace's
/// `target/psychopomp-cache/footage`; an installed one (`cargo install`) uses
/// the user cache, not Cargo's hidden checkout of its source.
fn default_directory() -> PathBuf {
    let workspace_target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let installed = std::env::current_exe().ok().is_some_and(|exe| {
        let target = workspace_target.canonicalize().ok();
        target.is_none_or(|target| !exe.starts_with(target))
    });
    cache_directory(
        std::env::var_os("PSYCHOPOMP_CACHE_DIR").map(PathBuf::from),
        installed,
        workspace_target,
        user_cache(),
    )
    .join("footage")
}

fn cache_directory(
    explicit: Option<PathBuf>,
    installed: bool,
    workspace_target: PathBuf,
    user_cache: PathBuf,
) -> PathBuf {
    match explicit {
        Some(directory) => directory,
        None if installed => user_cache,
        None => workspace_target.join("psychopomp-cache"),
    }
}

/// `$XDG_CACHE_HOME/psychopomp`, else `~/.cache/psychopomp`, else the
/// temporary directory.
fn user_cache() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("psychopomp")
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use super::*;

    #[test]
    fn the_cache_is_the_workspace_target_unless_installed_or_chosen() {
        let target = PathBuf::from("/repo/target");
        let user = PathBuf::from("/home/u/.cache/psychopomp");
        let chosen = Some(PathBuf::from("/fast/cache"));
        assert_eq!(
            cache_directory(None, false, target.clone(), user.clone()),
            PathBuf::from("/repo/target/psychopomp-cache")
        );
        assert_eq!(
            cache_directory(None, true, target.clone(), user.clone()),
            user
        );
        for installed in [false, true] {
            assert_eq!(
                cache_directory(chosen.clone(), installed, target.clone(), user.clone()),
                PathBuf::from("/fast/cache")
            );
        }
        // A test binary runs from this workspace's target, so it is not "installed".
        assert!(default_directory().ends_with("target/psychopomp-cache/footage"));
    }

    /// A decoded cache of `frames` 2×1 frames, each filled with its index.
    fn cached_video(directory: &Path, frames: u8) -> (FootageStore, SourceId) {
        let source = directory.join("source.bin");
        fs::write(&source, b"pretend video").unwrap();
        let decode = Decode {
            source,
            size: [2, 1],
            fps: 10,
            sequence: None,
            decoder: None,
            range: None,
        };
        // Write the cache file the decoder would have.
        let mut store = FootageStore::with_budget(3 * 8, directory.to_owned());
        let key = crate::video::cache_path(&decode, directory).unwrap();
        let mut file = fs::File::create(key).unwrap();
        for index in 0..frames {
            file.write_all(&[index; 8]).unwrap();
        }
        let id = store.video(decode).unwrap();
        (store, id)
    }

    #[test]
    fn frames_are_shared_and_the_least_recently_used_leave_first() {
        let directory =
            std::env::temp_dir().join(format!("psychopomp-store-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let (mut store, id) = cached_video(&directory, 6);
        assert_eq!(store.size(id), [2, 1]);
        assert_eq!(store.end_nanos(id), 600_000_000);
        assert_eq!(store.frame_index(id, 250_000_000), 2);
        assert_eq!(
            store.frame_index(id, 9_000_000_000),
            5,
            "held at the last frame"
        );
        for index in [0, 1, 2] {
            assert_eq!(&*store.frame(id, index).unwrap(), &[index as u8; 8]);
        }
        // Touch 0, so 1 is the least recently used when 3 arrives.
        store.frame(id, 0).unwrap();
        store.frame(id, 3).unwrap();
        assert_eq!(store.resident_bytes(), 24, "bounded at three frames");
        let stats = store.stats();
        assert_eq!((stats.reads, stats.hits, stats.evictions), (4, 1, 1));
        store.frame(id, 0).unwrap();
        assert_eq!(store.stats().hits, 2, "0 stayed");
        store.frame(id, 1).unwrap();
        assert_eq!(store.stats().reads, 5, "1 was evicted and read again");
        // The same decode is the same source.
        let again = store
            .video(Decode {
                source: directory.join("source.bin"),
                size: [2, 1],
                fps: 10,
                sequence: None,
                decoder: None,
                range: None,
            })
            .unwrap();
        assert_eq!(again, id);
        assert_ne!(store.identity(id, 1), store.identity(SourceId(1), 1));
        let _ = fs::remove_dir_all(&directory);
    }
}
