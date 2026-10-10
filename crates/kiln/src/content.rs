pub mod discovery;
pub mod frontmatter;
pub(crate) mod index;
pub mod page;

use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};

use anyhow::{Context, Result};

/// Whether an authored source name is private to the build.
pub(crate) fn is_private(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b"_")
}

/// Whether a source path is public, which holds for paths without a final name such as `/`.
pub(crate) fn is_public(path: &Path) -> bool {
    path.file_name().is_none_or(|name| !is_private(name))
}

/// Whether a source file uses the Markdown extension, regardless of ASCII case.
pub(crate) fn is_markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// Whether an authored name is one normal path component on every platform, such as a slug or
/// theme directory name.
pub(crate) fn is_single_component(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && !name.contains(['/', '\\', ':'])
}

/// Reads an optional source file, returning `None` when it does not exist.
pub(crate) fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}
