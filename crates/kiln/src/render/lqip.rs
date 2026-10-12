use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};
use serde::Serialize;

use crate::config::ImageConfig;

/// Per-image metadata: dimensions and optional LQIP data URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lqip_uri: Option<String>,
}

/// Reads dimensions of published images and (per [`ImageConfig`]) encodes a small WebP LQIP.
/// Memoized per canonical path for the build's lifetime.
pub struct ImageResolver {
    output_root: PathBuf,
    config: ImageConfig,
    cache: Mutex<HashMap<PathBuf, Option<Arc<ImageMeta>>>>,
}

impl ImageResolver {
    /// Constructs a resolver for images published under `output_root`.
    #[must_use]
    pub fn new(output_root: &Path, config: ImageConfig) -> Self {
        Self {
            output_root: output_root.to_path_buf(),
            config,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Returns metadata for a file published at an output-relative path, or `None` when it is
    /// missing or its format isn't recognized.
    ///
    /// # Panics
    ///
    /// Panics if the cache mutex is poisoned.
    #[must_use]
    pub fn resolve_published(&self, path: &Path) -> Option<Arc<ImageMeta>> {
        let canonical = self.output_root.join(path).canonicalize().ok()?;

        // Single-lock entry pattern so concurrent callers don't double-decode.
        self.cache
            .lock()
            .unwrap()
            .entry(canonical.clone())
            .or_insert_with(|| self.compute(&canonical).map(Arc::new))
            .clone()
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

    use tempfile::tempdir;

    use super::*;

    // ── ImageResolver::resolve_published ──

    #[test]
    fn reads_dimensions_and_caches_placeholder() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        write_tiny_png(&bundle.join("img.png"));

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        let meta = r.resolve_published(Path::new("bundle/img.png")).unwrap();
        assert_eq!((meta.width, meta.height), (2, 2));
        let cached = r.resolve_published(Path::new("bundle/img.png")).unwrap();
        assert!(Arc::ptr_eq(&meta, &cached));
        assert_eq!(decode_placeholder(&meta).dimensions(), (16, 16));
    }

    #[test]
    fn placeholder_preserves_aspect_ratio_and_averages_pixels() {
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
            let meta = resolver
                .resolve_published(Path::new("checker.png"))
                .unwrap();
            let placeholder = decode_placeholder(&meta);

            assert_eq!(placeholder.dimensions(), expected);
            let value = placeholder.get_pixel(expected.0 / 2, expected.1 / 2)[0];
            assert!(
                (96..=160).contains(&value),
                "expected an averaged pixel, got {value}"
            );
        }
    }

    #[test]
    fn placeholder_preserves_16_bit_brightness() {
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
        let meta = resolver.resolve_published(Path::new("bright.png")).unwrap();
        let placeholder = decode_placeholder(&meta);

        assert_eq!(placeholder.dimensions(), (1, 1));
        assert!(placeholder.get_pixel(0, 0)[0] > 240);
    }

    #[test]
    fn avif_feature_controls_placeholder() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let resolver = ImageResolver::new(
            &fixtures,
            ImageConfig {
                lqip_size: 4,
                ..ImageConfig::default()
            },
        );
        let meta = resolver
            .resolve_published(Path::new("example.avif"))
            .unwrap();
        assert_eq!((meta.width, meta.height), (8, 6));
        let placeholder = meta
            .lqip_uri
            .is_some()
            .then(|| decode_placeholder(&meta).dimensions());
        assert_eq!(placeholder, cfg!(feature = "avif").then_some((4, 3)));
    }

    #[test]
    fn retains_dimensions_on_encoding_failure() {
        let dir = tempdir().unwrap();
        write_tiny_png(&dir.path().join("photo one.png"));
        let resolver = ImageResolver::new(
            dir.path(),
            ImageConfig {
                lqip_quality: 101,
                ..ImageConfig::default()
            },
        );
        let meta = resolver
            .resolve_published(Path::new("photo one.png"))
            .unwrap();
        assert_eq!((meta.width, meta.height), (2, 2));
        assert!(meta.lqip_uri.is_none());
    }

    #[test]
    fn keeps_dimensions_of_undecodable_png() {
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
        let meta = r
            .resolve_published(Path::new("bundle/partial.png"))
            .unwrap();
        assert_eq!(meta.width, 4);
        assert_eq!(meta.height, 2);
        assert!(meta.lqip_uri.is_none());
    }

    #[test]
    fn none_for_missing_unrecognized_or_empty_images() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("garbage.png"), b"not actually an image").unwrap();
        // A GIF header declaring a 0 × 1 canvas.
        fs::write(bundle.join("empty.gif"), b"GIF89a\0\0\x01\0\0\0\0").unwrap();

        let r = ImageResolver::new(dir.path(), ImageConfig::default());
        for path in [
            "bundle/missing.png",
            "bundle/garbage.png",
            "bundle/empty.gif",
        ] {
            assert!(r.resolve_published(Path::new(path)).is_none(), "{path}");
        }
    }

    fn decode_placeholder(meta: &ImageMeta) -> image::RgbImage {
        let encoded = meta
            .lqip_uri
            .as_deref()
            .unwrap()
            .strip_prefix("data:image/webp;base64,")
            .unwrap();
        let bytes = BASE64_STANDARD.decode(encoded).unwrap();
        image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP)
            .unwrap()
            .to_rgb8()
    }

    fn write_tiny_png(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 100, 50, 255]));
        img.save_with_format(path, image::ImageFormat::Png).unwrap();
    }
}
