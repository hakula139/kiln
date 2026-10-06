use std::ffi::OsStr;

pub mod discovery;
pub mod frontmatter;
pub mod page;

/// Whether an authored source name is private to the build.
fn is_private(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b"_")
}
