use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use image::ImageReader;
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};

/// Image-pipeline configuration loaded from the `[image]` section of
/// `config.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImageConfig {
    /// Toggle LQIP encoding. Dimensions are always emitted regardless.
    #[serde(default = "default_lqip_enabled")]
    pub lqip: bool,

    /// Source raster size (square pixels) before WebP encoding.
    #[serde(default = "default_lqip_size")]
    pub lqip_size: u32,

    /// WebP encoder quality (1–100; lower = smaller / blurrier).
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

/// Per-image metadata produced by [`ImageResolver::resolve`]. `lqip_uri`
/// is `None` for SVG / disabled LQIP; dimensions come from `imagesize`
/// (header-only, covers more formats than the `image` crate's decoder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lqip_uri: Option<String>,
}

/// Resolves `<img src>` strings to on-disk paths, reads dimensions, and
/// (per [`ImageConfig`]) encodes a small WebP LQIP. Memoised per canonical
/// path for the build's lifetime.
pub struct ImageResolver {
    static_root: PathBuf,
    config: ImageConfig,
    cache: Mutex<HashMap<PathBuf, Option<Arc<ImageMeta>>>>,
}

impl ImageResolver {
    /// Constructs a resolver. `static_root` anchors `src` strings that begin
    /// with `/` (i.e., site-absolute references); page-bundle-relative paths
    /// resolve through the `base_dir` argument to [`Self::resolve`].
    #[must_use]
    pub fn new(static_root: &Path, config: ImageConfig) -> Self {
        Self {
            static_root: static_root.to_path_buf(),
            config,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Resolves a `src` string to image metadata, or returns `None` when the
    /// path can't be located or the format isn't recognised.
    ///
    /// `base_dir` is the page-bundle anchor for relative paths; pass `None`
    /// for contexts without a bundle (e.g., feed generation).
    ///
    /// # Panics
    ///
    /// Panics if the cache mutex is poisoned.
    #[must_use]
    pub fn resolve(&self, src: &str, base_dir: Option<&Path>) -> Option<Arc<ImageMeta>> {
        let path = self.resolve_path(src, base_dir)?;
        let canonical = path.canonicalize().ok()?;

        // Single-lock entry pattern so concurrent callers don't double-decode.
        self.cache
            .lock()
            .unwrap()
            .entry(canonical.clone())
            .or_insert_with(|| self.compute(&canonical).map(Arc::new))
            .clone()
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

    /// Reads dimensions (header-only via `imagesize`) and optionally encodes
    /// the LQIP. `imagesize` covers formats the `image` decoder skips
    /// (HEIC, JPEG XL); LQIP encoding still requires a full decode.
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

/// Decodes the raster, downsamples to a `size`-pixel preview, and returns
/// a `data:image/webp;base64,...` URI. Returns `None` for undecodable
/// formats (SVG, animated AVIF first-frame failure).
fn encode_lqip(path: &Path, size: u32, quality: u8) -> Option<String> {
    let img = ImageReader::open(path).ok()?.decode().ok()?;

    // `Triangle` is the cheapest filter that doesn't alias at this size;
    // `Lanczos3` over-sharpens the blur we want.
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
        let static_root = dir.path().join("static");
        let r = ImageResolver::new(&static_root, ImageConfig::default());
        let resolved = r.resolve_path("/images/cover.webp", None).unwrap();
        assert_eq!(resolved, static_root.join("images/cover.webp"));
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
    fn resolve_unrecognized_format_returns_none() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("garbage.png"), b"not actually an image").unwrap();

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        assert!(r.resolve("garbage.png", Some(&bundle)).is_none());
    }

    #[test]
    fn resolve_yields_dimensions_but_no_lqip_for_undecodable_png() {
        // A minimal PNG header (signature + IHDR for a 4×2 RGBA image, with a
        // valid CRC) gives `imagesize` enough to report dimensions, but the
        // `image` crate's full decoder bails when it hits EOF without IDAT.
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        let mut bytes: Vec<u8> = vec![
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, // signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length = 13
            b'I', b'H', b'D', b'R', // chunk type
            0x00, 0x00, 0x00, 0x04, // width = 4
            0x00, 0x00, 0x00, 0x02, // height = 2
            0x08, 0x06, 0x00, 0x00, 0x00, // bit depth, color type, etc.
        ];
        // CRC over chunk type + data; placeholder zeros — `imagesize` ignores it.
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        fs::write(bundle.join("partial.png"), &bytes).unwrap();

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let meta = r.resolve("partial.png", Some(&bundle)).unwrap();
        assert_eq!(meta.width, 4);
        assert_eq!(meta.height, 2);
        // `image` crate refuses the file because IDAT is missing.
        assert!(meta.lqip_uri.is_none());
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
