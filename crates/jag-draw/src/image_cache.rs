use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Return embedded bytes for built-in raster images that ship with the
/// Detir binary. Currently unused for application assets; all images are
/// loaded from the filesystem.
fn builtin_image_bytes(_path: &Path) -> Option<&'static [u8]> {
    None
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct CacheKey {
    path: PathBuf,
}

#[derive(Clone)]
#[allow(dead_code)]
enum CacheEntry {
    Loading,
    Ready {
        tex: Arc<wgpu::Texture>,
        width: u32,
        height: u32,
        last_tick: u64,
        bytes: usize,
    },
    Failed,
}

/// Simple raster image cache for PNG/JPEG/GIF/WebP with LRU eviction.
pub struct ImageCache {
    device: Arc<wgpu::Device>,
    // LRU state
    map: HashMap<CacheKey, CacheEntry>,
    lru: VecDeque<CacheKey>,
    current_tick: u64,
    // guardrails
    max_bytes: usize,
    total_bytes: usize,
    max_tex_size: u32,
}

impl ImageCache {
    pub fn new(device: Arc<wgpu::Device>) -> Self {
        // Conservative default budget: 256 MiB for cached images
        let max_bytes = 256 * 1024 * 1024;
        let limits = device.limits();
        let max_tex_size = limits.max_texture_dimension_2d;
        Self {
            device,
            map: HashMap::new(),
            lru: VecDeque::new(),
            current_tick: 0,
            max_bytes,
            total_bytes: 0,
            max_tex_size,
        }
    }

    pub fn set_max_bytes(&mut self, bytes: usize) {
        self.max_bytes = bytes;
        self.evict_if_needed();
    }

    fn touch(&mut self, key: &CacheKey) {
        self.current_tick = self.current_tick.wrapping_add(1);
        if let Some(entry) = self.map.get_mut(key) {
            if let CacheEntry::Ready { last_tick, .. } = entry {
                *last_tick = self.current_tick;
            }
        }
        // update LRU order: move key to back
        if let Some(pos) = self.lru.iter().position(|k| k == key) {
            let k = self.lru.remove(pos).unwrap();
            self.lru.push_back(k);
        }
    }

    fn insert(&mut self, key: CacheKey, entry: CacheEntry) {
        self.current_tick = self.current_tick.wrapping_add(1);
        if let CacheEntry::Ready { bytes, .. } = &entry {
            self.total_bytes += bytes;
        }
        self.map.insert(key.clone(), entry);
        self.lru.push_back(key);
        self.evict_if_needed();
    }

    fn evict_if_needed(&mut self) {
        while self.total_bytes > self.max_bytes {
            if let Some(old_key) = self.lru.pop_front() {
                if let Some(entry) = self.map.remove(&old_key) {
                    if let CacheEntry::Ready { bytes, .. } = entry {
                        self.total_bytes = self.total_bytes.saturating_sub(bytes);
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    /// Check if an image is in the cache and return it if ready.
    /// Returns None if loading or failed, Some if ready.
    pub fn get(&mut self, path: &Path) -> Option<(Arc<wgpu::Texture>, u32, u32)> {
        let key = CacheKey {
            path: path.to_path_buf(),
        };

        // Clone the data we need before touching
        let result = if let Some(entry) = self.map.get(&key) {
            match entry {
                CacheEntry::Ready {
                    tex, width, height, ..
                } => Some((tex.clone(), *width, *height)),
                CacheEntry::Loading | CacheEntry::Failed => None,
            }
        } else {
            None
        };

        if result.is_some() {
            self.touch(&key);
        }

        result
    }

    /// Start loading an image if not already in cache.
    /// Marks it as Loading immediately, actual load happens synchronously.
    pub fn start_load(&mut self, path: &Path) {
        let key = CacheKey {
            path: path.to_path_buf(),
        };

        // If already in cache (any state), don't restart
        if self.map.contains_key(&key) {
            return;
        }

        // Mark as loading
        self.map.insert(key, CacheEntry::Loading);
    }

    /// Load an image from disk and cache it as a GPU texture.
    /// Returns a reference to the texture and its dimensions on success.
    pub fn get_or_load(
        &mut self,
        path: &Path,
        queue: &wgpu::Queue,
    ) -> Option<(Arc<wgpu::Texture>, u32, u32)> {
        let key = CacheKey {
            path: path.to_path_buf(),
        };

        // Check cache first - clone data before touching
        let cached_result = if let Some(entry) = self.map.get(&key) {
            match entry {
                CacheEntry::Ready {
                    tex, width, height, ..
                } => Some((tex.clone(), *width, *height)),
                CacheEntry::Loading => {
                    // Still loading, proceed to load now
                    None
                }
                CacheEntry::Failed => return None,
            }
        } else {
            None
        };

        if let Some(result) = cached_result {
            self.touch(&key);
            return Some(result);
        }

        // Load image from embedded bytes (for built-in assets) or from disk.
        let img = if let Some(bytes) = builtin_image_bytes(path) {
            match image::load_from_memory(bytes) {
                Ok(img) => img,
                Err(_) => return None,
            }
        } else {
            match image::open(path) {
                Ok(img) => img,
                Err(_) => return None,
            }
        };

        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();

        // Clamp to max texture size
        if width > self.max_tex_size || height > self.max_tex_size {
            return None;
        }

        // Create GPU texture
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("image:{}", path.display())),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Upload image data
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let bytes = (width * height * 4) as usize;
        let tex_arc = Arc::new(tex);
        let entry = CacheEntry::Ready {
            tex: tex_arc.clone(),
            width,
            height,
            last_tick: self.current_tick,
            bytes,
        };

        self.insert(key, entry);
        Some((tex_arc, width, height))
    }

    /// Check if an image is currently loading
    pub fn is_loading(&self, path: &Path) -> bool {
        let key = CacheKey {
            path: path.to_path_buf(),
        };
        matches!(self.map.get(&key), Some(CacheEntry::Loading))
    }

    /// Check if an image is ready
    pub fn is_ready(&self, path: &Path) -> bool {
        let key = CacheKey {
            path: path.to_path_buf(),
        };
        matches!(self.map.get(&key), Some(CacheEntry::Ready { .. }))
    }

    /// Store a pre-loaded texture in the cache (used for async loading)
    pub fn store_ready(&mut self, path: &Path, tex: Arc<wgpu::Texture>, width: u32, height: u32) {
        let key = CacheKey {
            path: path.to_path_buf(),
        };
        let bytes = (width * height * 4) as usize;

        let entry = CacheEntry::Ready {
            tex,
            width,
            height,
            last_tick: self.current_tick,
            bytes,
        };

        self.insert(key, entry);
    }
}
