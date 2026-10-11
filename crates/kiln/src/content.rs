pub mod discovery;
pub mod frontmatter;
pub(crate) mod index;
pub mod page;

use std::ffi::OsStr;
use std::path::Path;

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
