use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use image::ImageReader;
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};

/// Image-pipeline configuration loaded from the `[image]` section of
/// `config.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageConfig {
    /// Whether to bake LQIP placeholders into the rendered HTML.
    /// Dimensions (`width` / `height`) are always populated regardless.
    #[serde(default = "default_lqip_enabled")]
    pub lqip: bool,

    /// Pixel dimension of the square LQIP source raster before WebP encoding.
    #[serde(default = "default_lqip_size")]
    pub lqip_size: u32,

    /// WebP encoder quality for the LQIP, on the conventional 1–100 scale.
    /// Lower values produce smaller, blurrier placeholders.
    #[serde(default = "default_lqip_quality")]
    pub lqip_quality: u8,
}

const fn default_lqip_enabled() -> bool {
    true
}

const fn default_lqip_size() -> u32 {
    16
}

const fn default_lqip_quality() -> u8 {
    25
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            lqip: default_lqip_enabled(),
            lqip_size: default_lqip_size(),
            lqip_quality: default_lqip_quality(),
        }
    }
}

/// Per-image metadata produced by [`ImageResolver::resolve`].
///
/// `lqip_uri` is `None` when LQIP is disabled in config or when the source
/// format cannot be decoded by the `image` crate (e.g., SVG). The
/// `width` / `height` fields are populated whenever any decoder — including
/// the header-only `imagesize` fallback — recognises the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lqip_uri: Option<String>,
}

/// Resolves `<img src>` strings to on-disk paths, reads dimensions, and
/// optionally encodes a 16×16 WebP LQIP per the active [`ImageConfig`].
///
/// Construction is cheap; the resolver memoises results per canonical path
/// for the lifetime of a single build.
pub struct ImageResolver {
    static_root: PathBuf,
    config: ImageConfig,
    cache: Mutex<HashMap<PathBuf, Option<Arc<ImageMeta>>>>,
}

impl ImageResolver {
    /// Constructs a resolver rooted at `site_root`. Static-prefixed sources
    /// (`src` starting with `/`) resolve under `site_root/static/`.
    #[must_use]
    pub fn new(site_root: &Path, config: ImageConfig) -> Self {
        Self {
            static_root: site_root.join("static"),
            config,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Resolves a `src` string to image metadata, or returns `None` when the
    /// path cannot be located or the format is unrecognisable.
    ///
    /// `base_dir` is the page-bundle directory used for relative paths. Pass
    /// `None` for contexts without a bundle anchor (e.g., feed generation).
    ///
    /// # Panics
    ///
    /// Panics if the internal cache mutex is poisoned. Poisoning would mean
    /// a previous `resolve` call panicked while holding the lock, which the
    /// pure cache logic here cannot trigger; treat any panic as a bug.
    #[must_use]
    pub fn resolve(&self, src: &str, base_dir: Option<&Path>) -> Option<Arc<ImageMeta>> {
        let path = self.resolve_path(src, base_dir)?;
        let canonical = path.canonicalize().ok()?;

        if let Some(cached) = self.cache.lock().unwrap().get(&canonical) {
            return cached.clone();
        }

        let meta = self.compute(&canonical).map(Arc::new);
        self.cache.lock().unwrap().insert(canonical, meta.clone());
        meta
    }

    /// Maps a `src` reference to a filesystem path under one of the known
    /// roots. Returns `None` for remote URLs and `data:` schemes.
    fn resolve_path(&self, src: &str, base_dir: Option<&Path>) -> Option<PathBuf> {
        if src.is_empty()
            || src.starts_with("http://")
            || src.starts_with("https://")
            || src.starts_with("//")
            || src.starts_with("data:")
        {
            return None;
        }

        if let Some(rest) = src.strip_prefix('/') {
            Some(self.static_root.join(rest))
        } else {
            base_dir.map(|d| d.join(src))
        }
    }

    /// Reads dimensions and (optionally) encodes the LQIP for one path.
    ///
    /// Always tries `imagesize` first — it parses headers without decoding
    /// pixels and supports formats the `image` crate cannot (e.g., raw HEIC,
    /// JPEG XL). If LQIP is enabled, the second pass decodes pixels for the
    /// downsample.
    fn compute(&self, path: &Path) -> Option<ImageMeta> {
        let dims = imagesize::size(path).ok()?;
        let width = u32::try_from(dims.width).ok()?;
        let height = u32::try_from(dims.height).ok()?;

        if width == 0 || height == 0 {
            return None;
        }

        let lqip_uri = self
            .config
            .lqip
            .then(|| encode_lqip(path, self.config.lqip_size, self.config.lqip_quality))
            .flatten();

        Some(ImageMeta {
            width,
            height,
            lqip_uri,
        })
    }
}

/// Decodes the source raster, downsamples to a square `size`-pixel preview,
/// and returns a `data:image/webp;base64,...` URI. Returns `None` when the
/// format isn't decodable (e.g., SVG vector, animated AVIF first-frame
/// failure) — the caller falls back to dimension-only output.
fn encode_lqip(path: &Path, size: u32, quality: u8) -> Option<String> {
    let img = ImageReader::open(path).ok()?.decode().ok()?;

    // Resize preserving aspect ratio so the LQIP echoes the source's shape.
    // `Triangle` is the cheapest filter that doesn't visibly alias at small
    // sizes; `Lanczos3` over-sharpens the blur we actually want.
    let resized = img.resize(size, size, FilterType::Triangle);
    let rgba = resized.to_rgba8();

    let webp_bytes = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
        .encode(f32::from(quality))
        .to_vec();

    let mut uri = String::with_capacity(webp_bytes.len() * 4 / 3 + 32);
    uri.push_str("data:image/webp;base64,");
    BASE64_STANDARD.encode_string(&webp_bytes, &mut uri);
    Some(uri)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;
    use tempfile::tempdir;

    use super::*;

    // ── ImageConfig ──

    #[test]
    fn config_defaults_match_constants() {
        let config = ImageConfig::default();
        assert!(config.lqip);
        assert_eq!(config.lqip_size, 16);
        assert_eq!(config.lqip_quality, 25);
    }

    #[test]
    fn config_deserialises_partial_toml() {
        let parsed: ImageConfig = toml::from_str(indoc! {"
            lqip = false
        "})
        .unwrap();
        assert!(!parsed.lqip);
        assert_eq!(parsed.lqip_size, 16);
        assert_eq!(parsed.lqip_quality, 25);
    }

    // ── ImageResolver::resolve_path ──

    #[test]
    fn resolve_path_remote_returns_none() {
        let dir = tempdir().unwrap();
        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        assert!(r.resolve_path("https://example.com/x.png", None).is_none());
        assert!(r.resolve_path("//example.com/x.png", None).is_none());
        assert!(r.resolve_path("data:image/png;base64,xx", None).is_none());
        assert!(r.resolve_path("", None).is_none());
    }

    #[test]
    fn resolve_path_absolute_uses_static_root() {
        let dir = tempdir().unwrap();
        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let resolved = r.resolve_path("/images/cover.webp", None).unwrap();
        assert_eq!(resolved, dir.path().join("static/images/cover.webp"));
    }

    #[test]
    fn resolve_path_relative_uses_base_dir() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let resolved = r.resolve_path("assets/foo.png", Some(&bundle)).unwrap();
        assert_eq!(resolved, bundle.join("assets/foo.png"));
    }

    #[test]
    fn resolve_path_relative_without_base_returns_none() {
        let dir = tempdir().unwrap();
        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        assert!(r.resolve_path("foo.png", None).is_none());
    }

    // ── ImageResolver::resolve ──

    fn write_tiny_png(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 100, 50, 255]));
        img.save_with_format(path, image::ImageFormat::Png).unwrap();
    }

    #[test]
    fn resolve_reads_dimensions() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        write_tiny_png(&bundle.join("img.png"));

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let meta = r.resolve("img.png", Some(&bundle)).unwrap();
        assert_eq!(meta.width, 2);
        assert_eq!(meta.height, 2);
    }

    #[test]
    fn resolve_emits_lqip_data_uri_when_enabled() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        write_tiny_png(&bundle.join("img.png"));

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let meta = r.resolve("img.png", Some(&bundle)).unwrap();
        let uri = meta.lqip_uri.as_deref().expect("lqip should be encoded");
        assert!(uri.starts_with("data:image/webp;base64,"), "uri: {uri}");
        assert!(
            uri.len() > "data:image/webp;base64,".len(),
            "expected non-empty payload, got: {uri}"
        );
    }

    #[test]
    fn resolve_skips_lqip_when_disabled() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        write_tiny_png(&bundle.join("img.png"));

        let r = ImageResolver::new(
            dir.path(),
            ImageConfig {
                lqip: false,
                ..ImageConfig::default()
            },
        );
        let meta = r.resolve("img.png", Some(&bundle)).unwrap();
        assert_eq!(meta.width, 2);
        assert!(meta.lqip_uri.is_none());
    }

    #[test]
    fn resolve_missing_file_returns_none() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        fs::create_dir_all(&bundle).unwrap();

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        assert!(r.resolve("missing.png", Some(&bundle)).is_none());
    }

    #[test]
    fn resolve_remote_returns_none() {
        let dir = tempdir().unwrap();
        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        assert!(r.resolve("https://example.com/x.png", None).is_none());
    }

    #[test]
    fn resolve_caches_repeated_lookups() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        write_tiny_png(&bundle.join("img.png"));

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let first = r.resolve("img.png", Some(&bundle)).unwrap();
        let second = r.resolve("img.png", Some(&bundle)).unwrap();
        assert!(
            Arc::ptr_eq(&first, &second),
            "second lookup should hit the cache"
        );
    }
}
