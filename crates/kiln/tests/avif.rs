use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use kiln::render::lqip::{ImageConfig, ImageResolver};

// ── ImageResolver::resolve ──

#[test]
fn resolve_avif_feature_controls_placeholder() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let resolver = ImageResolver::new(
        &fixtures,
        ImageConfig {
            lqip_size: 4,
            ..ImageConfig::default()
        },
    );
    let metadata = resolver.resolve("/example.avif", None).unwrap();
    assert_eq!((metadata.width, metadata.height), (8, 6));

    if cfg!(feature = "avif") {
        let uri = metadata.lqip_uri.as_deref().unwrap();
        let encoded = uri.strip_prefix("data:image/webp;base64,").unwrap();
        let bytes = BASE64_STANDARD.decode(encoded).unwrap();
        let placeholder =
            image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP).unwrap();
        assert_eq!((placeholder.width(), placeholder.height()), (4, 3));
    } else {
        assert_eq!(metadata.lqip_uri, None);
    }
}
