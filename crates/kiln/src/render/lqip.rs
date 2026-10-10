use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Result, ensure};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

/// Image-pipeline configuration loaded from the `[image]` section of `config.toml`.
/// Unknown keys are rejected so removed fields and typos surface as build errors.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageConfig {
    /// Maximum placeholder width and height, in pixels.
    #[serde(default = "default_lqip_size")]
    pub lqip_size: u32,

    /// WebP encoder quality (1–100, lower = smaller / blurrier).
    #[serde(default = "default_lqip_quality")]
    pub lqip_quality: u8,
}

const fn default_lqip_size() -> u32 {
    16
}

const fn default_lqip_quality() -> u8 {
    25
}

impl ImageConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.lqip_size > 0,
            "image.lqip_size must be greater than zero"
        );
        ensure!(
            (1..=100).contains(&self.lqip_quality),
            "image.lqip_quality must be between 1 and 100"
        );
        Ok(())
    }
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            lqip_size: default_lqip_size(),
            lqip_quality: default_lqip_quality(),
        }
    }
}

/// Per-image metadata: dimensions and optional LQIP data URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lqip_uri: Option<String>,
}

/// Resolves `<img src>` strings to on-disk paths, reads dimensions, and (per [`ImageConfig`])
/// encodes a small WebP LQIP. Memoised per canonical path for the build's lifetime.
pub struct ImageResolver {
    output_root: PathBuf,
    config: ImageConfig,
    cache: Mutex<HashMap<PathBuf, Option<Arc<ImageMeta>>>>,
}

impl ImageResolver {
    /// Constructs a resolver. `output_root` anchors `src` strings that begin with `/` (i.e.,
    /// site-absolute references). Page-bundle-relative paths resolve through the `base_dir`
    /// argument to [`Self::resolve`].
    #[must_use]
    pub fn new(output_root: &Path, config: ImageConfig) -> Self {
        Self {
            output_root: output_root.to_path_buf(),
            config,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Resolves a `src` string to image metadata, or returns `None` when the path can't be located
    /// or the format isn't recognised.
    ///
    /// `base_dir` is the page-bundle anchor for relative paths. Pass `None` for contexts without a
    /// bundle (e.g., feed generation).
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

    /// Maps a `src` reference to a filesystem path under one of the known roots. Returns `None`
    /// for remote URLs and `data:` schemes.
    fn resolve_path(&self, src: &str, base_dir: Option<&Path>) -> Option<PathBuf> {
        let path = src.split(['?', '#']).next()?;
        if path.is_empty() || path.starts_with("//") || path.split('/').next()?.contains(':') {
            return None;
        }
        let path = percent_decode_str(path).decode_utf8().ok()?;
        if let Some(rest) = path.strip_prefix('/') {
            Some(self.output_root.join(rest))
        } else {
            base_dir.map(|dir| dir.join(path.as_ref()))
        }
    }

    /// Reads dimensions and optionally encodes the LQIP.
    fn compute(&self, path: &Path) -> Option<ImageMeta> {
        let dims = imagesize::size(path).ok()?;
        let width = u32::try_from(dims.width).ok()?;
        let height = u32::try_from(dims.height).ok()?;

        if width == 0 || height == 0 {
            return None;
        }

        let lqip_uri = encode_lqip(path, self.config.lqip_size, self.config.lqip_quality);

        Some(ImageMeta {
            width,
            height,
            lqip_uri,
        })
    }
}

/// Encodes a LQIP data URI, or `None` for undecodable formats.
fn encode_lqip(path: &Path, size: u32, quality: u8) -> Option<String> {
    let img = ImageReader::open(path).ok()?.decode().ok()?;

    // `thumbnail` sums 8-bit pixels in u32, so large sampling blocks can overflow.
    let (long, short) = if img.width() >= img.height() {
        (img.width(), img.height())
    } else {
        (img.height(), img.width())
    };
    let max_samples = (u64::from(long.div_ceil(size.max(1))) + 1).saturating_mul(u64::from(short));
    let resized = if matches!(
        &img,
        DynamicImage::ImageLuma8(_)
            | DynamicImage::ImageLumaA8(_)
            | DynamicImage::ImageRgb8(_)
            | DynamicImage::ImageRgba8(_)
    ) && max_samples <= u64::from(u32::MAX / (u32::from(u8::MAX) + 1))
    {
        img.thumbnail(size, size)
    } else {
        img.resize(size, size, FilterType::Triangle)
    };
    let rgba = resized.into_rgba8();

    let webp_bytes = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
        .encode_simple(false, f32::from(quality))
        .ok()?
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
        assert_eq!(config.lqip_size, 16);
        assert_eq!(config.lqip_quality, 25);
    }

    #[test]
    fn config_deserialises_partial_toml() {
        let parsed: ImageConfig = toml::from_str(indoc! {"
            lqip_size = 24
        "})
        .unwrap();
        assert_eq!(parsed.lqip_size, 24);
        assert_eq!(parsed.lqip_quality, 25);
    }

    #[test]
    fn config_rejects_unknown_fields() {
        // Removed pre-0.2 toggle should error rather than silently no-op.
        let err = toml::from_str::<ImageConfig>(indoc! {"
            lqip = false
            lqip_size = 16
        "})
        .unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "expected an unknown-field error, got: {err}",
        );
    }

    // ── ImageConfig::validate ──

    #[test]
    fn validate_rejects_invalid_encoder_settings() {
        for quality in [0, 101, 255] {
            assert!(
                ImageConfig {
                    lqip_quality: quality,
                    ..ImageConfig::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            ImageConfig {
                lqip_size: 0,
                ..ImageConfig::default()
            }
            .validate()
            .is_err()
        );
        for quality in [1, 100] {
            ImageConfig {
                lqip_quality: quality,
                ..ImageConfig::default()
            }
            .validate()
            .unwrap();
        }
    }

    // ── ImageResolver::resolve ──

    #[test]
    fn resolve_reads_dimensions_and_caches_placeholder() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        write_tiny_png(&bundle.join("img.png"));

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let meta = r.resolve("img.png", Some(&bundle)).unwrap();
        assert_eq!((meta.width, meta.height), (2, 2));
        let cached = r.resolve("img.png", Some(&bundle)).unwrap();
        assert!(Arc::ptr_eq(&meta, &cached));
        let uri = meta.lqip_uri.as_deref().expect("lqip should be encoded");
        assert!(uri.starts_with("data:image/webp;base64,"), "uri: {uri}");
        assert!(
            uri.len() > "data:image/webp;base64,".len(),
            "expected non-empty payload, got: {uri}"
        );
    }

    #[test]
    fn resolve_lqip_preserves_aspect_ratio_and_averages_pixels() {
        for (width, height, expected) in [(64, 32, (16, 8)), (32, 64, (8, 16))] {
            let dir = tempdir().unwrap();
            let path = dir.path().join("checker.png");
            let img = image::RgbaImage::from_fn(width, height, |x, _| {
                let value = if x % 2 == 0 { 0 } else { 255 };
                image::Rgba([value, value, value, 255])
            });
            img.save_with_format(&path, image::ImageFormat::Png)
                .unwrap();

            let resolver = ImageResolver::new(dir.path(), ImageConfig::default());
            let meta = resolver.resolve("/checker.png", None).unwrap();
            let encoded = meta
                .lqip_uri
                .as_deref()
                .unwrap()
                .strip_prefix("data:image/webp;base64,")
                .unwrap();
            let bytes = BASE64_STANDARD.decode(encoded).unwrap();
            let placeholder = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
                .unwrap()
                .to_rgb8();

            assert_eq!(placeholder.dimensions(), expected);
            let value = placeholder.get_pixel(expected.0 / 2, expected.1 / 2)[0];
            assert!(
                (96..=160).contains(&value),
                "expected an averaged pixel, got {value}"
            );
        }
    }

    #[test]
    fn resolve_lqip_preserves_large_16_bit_values() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bright.png");
        let img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_pixel(
            257,
            257,
            image::Luma([u16::MAX]),
        );
        img.save_with_format(&path, image::ImageFormat::Png)
            .unwrap();

        let resolver = ImageResolver::new(
            dir.path(),
            ImageConfig {
                lqip_size: 1,
                ..ImageConfig::default()
            },
        );
        let meta = resolver.resolve("/bright.png", None).unwrap();
        let encoded = meta
            .lqip_uri
            .as_deref()
            .unwrap()
            .strip_prefix("data:image/webp;base64,")
            .unwrap();
        let bytes = BASE64_STANDARD.decode(encoded).unwrap();
        let placeholder = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .unwrap()
            .to_rgb8();

        assert_eq!(placeholder.dimensions(), (1, 1));
        assert!(placeholder.get_pixel(0, 0)[0] > 240);
    }

    #[test]
    fn resolve_decodes_local_urls_and_retains_metadata_on_encoding_failure() {
        let dir = tempdir().unwrap();
        write_tiny_png(&dir.path().join("photo one.png"));
        let resolver = ImageResolver::new(
            dir.path(),
            ImageConfig {
                lqip_quality: 101,
                ..ImageConfig::default()
            },
        );
        for src in ["photo%20one.png?v=1#image", "/photo%20one.png#image"] {
            let meta = resolver.resolve(src, Some(dir.path())).unwrap();
            assert_eq!((meta.width, meta.height), (2, 2));
            assert!(meta.lqip_uri.is_none());
        }
    }

    #[test]
    fn resolve_yields_dimensions_but_no_lqip_for_undecodable_png() {
        // The header exposes dimensions to `imagesize` while full image decoding fails.
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
        // Placeholder zeros stand in for the CRC over chunk type + data, which `imagesize` ignores.
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        fs::write(bundle.join("partial.png"), &bytes).unwrap();

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let meta = r.resolve("partial.png", Some(&bundle)).unwrap();
        assert_eq!(meta.width, 4);
        assert_eq!(meta.height, 2);
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

    fn write_tiny_png(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 100, 50, 255]));
        img.save_with_format(path, image::ImageFormat::Png).unwrap();
    }

    // ── ImageResolver::resolve_path ──

    #[test]
    fn resolve_path_absolute_uses_output_root() {
        let dir = tempdir().unwrap();
        let output_root = dir.path().join("public");
        let r = ImageResolver::new(&output_root, ImageConfig::default());
        let resolved = r.resolve_path("/images/cover.webp", None).unwrap();
        assert_eq!(resolved, output_root.join("images/cover.webp"));
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
    fn resolve_path_remote_returns_none() {
        let dir = tempdir().unwrap();
        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        assert!(r.resolve_path("https://example.com/x.png", None).is_none());
        assert!(r.resolve_path("//example.com/x.png", None).is_none());
        assert!(r.resolve_path("data:image/png;base64,xx", None).is_none());
        assert!(r.resolve_path("", None).is_none());
    }

    #[test]
    fn resolve_path_relative_without_base_returns_none() {
        let dir = tempdir().unwrap();
        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        assert!(r.resolve_path("foo.png", None).is_none());
    }
}
