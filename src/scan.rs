//! Reads the direct children of one directory with `getattrlistbulk`.

mod bulk;
mod listing;
#[cfg(target_os = "macos")]
mod sys;
