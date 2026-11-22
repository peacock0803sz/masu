//! Result types returned by a directory read.

/// Everything one directory read produced.
///
/// When `failure` is set, the `entries` and `errors` read before it are kept.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Listing {
    /// Entries whose attributes were read.
    pub(crate) entries: Vec<Entry>,
    /// Entries whose attributes the file system could not return.
    pub(crate) errors: Vec<EntryError>,
    /// Why the read stopped early, if it did.
    pub(crate) failure: Option<Failure>,
}

/// One direct child of the directory.
///
/// `name` is the bytes without the NUL, and masu neither normalizes Unicode nor
/// converts the encoding. Every entry that is not an error has one.
///
/// The other fields are `None` when the file system did not return that
/// attribute, and are never filled with 0. The `link_count` of a directory and
/// the `mount_point` of a non-directory are always `None`. `dev` has the same
/// 32-bit signed type as `dev_t`.
///
/// For a directory, `alloc_size` and `logical_size` come from
/// `ATTR_DIR_ALLOCSIZE` and `ATTR_DIR_DATALENGTH`, and `mount_point` from
/// `ATTR_DIR_MOUNTSTATUS`. The other values of a mount point are those of the
/// file system below it, so compare `mount_point` rather than `dev` with the
/// parent to find where a mount starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) name: Vec<u8>,
    pub(crate) kind: Option<Kind>,
    pub(crate) dev: Option<i32>,
    pub(crate) ino: Option<u64>,
    pub(crate) link_count: Option<u32>,
    pub(crate) alloc_size: Option<u64>,
    pub(crate) logical_size: Option<u64>,
    pub(crate) mount_point: Option<bool>,
}

/// The type of an entry, without following symbolic links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Dir,
    File,
    Symlink,
    Other,
}

/// An entry whose attributes could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryError {
    pub(crate) name: Option<Vec<u8>>,
    /// The value of `ATTR_CMN_ERROR`. It is a macOS errno and is not translated.
    pub(crate) errno: i32,
}

/// Why a directory read stopped before the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The directory could not be opened, including when the path contains NUL.
    // Only the macOS call creates this, so on Linux test builds it would be
    // dead code. The same `cfg` keeps `dead_code` denied instead of allowed.
    #[cfg(target_os = "macos")]
    Open(i32),
    /// A `getattrlistbulk` call failed.
    Read(i32),
    /// The response had a shape that cannot be read. It has no errno, and how
    /// to count it is decided in issue #7.
    Malformed,
}
