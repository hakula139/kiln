pub mod discovery;
pub mod frontmatter;
pub mod page;

use std::ffi::OsStr;

/// Whether an authored source name is private to the build.
pub(crate) fn is_private(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b"_")
}
