pub mod discovery;
pub mod frontmatter;
pub mod page;

/// Whether an authored source name is private to the build.
pub(crate) fn is_private(name: &std::ffi::OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b"_")
}
