use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::scan::listing::{Entry, Kind};

/// A directory under the system temp dir that is removed on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("masu-{label}-{}-{n}", std::process::id()));
        fs::create_dir(&path).expect("create the temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

/// Gives the permissions back to every directory in the tree.
fn restore_permissions(dir: &Path) {
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    let Ok(children) = fs::read_dir(dir) else {
        return;
    };
    for child in children.flatten() {
        if child.file_type().is_ok_and(|t| t.is_dir()) {
            restore_permissions(&child.path());
        } else {
            let _ = fs::set_permissions(child.path(), fs::Permissions::from_mode(0o600));
        }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Tests that take permissions away would stop the removal otherwise.
        restore_permissions(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Fails unless every field the entry has equals what `lstat` reports.
fn assert_matches_lstat(dir: &Path, entry: &Entry) {
    let path = dir.join(OsStr::from_bytes(&entry.name));
    let meta = fs::symlink_metadata(&path).expect("lstat the entry");
    let name = String::from_utf8_lossy(&entry.name);
    if let Some(kind) = entry.kind {
        let file_type = meta.file_type();
        let ok = match kind {
            Kind::Dir => file_type.is_dir(),
            Kind::File => file_type.is_file(),
            Kind::Symlink => file_type.is_symlink(),
            Kind::Other => !file_type.is_dir() && !file_type.is_file() && !file_type.is_symlink(),
        };
        assert!(ok, "{name}: kind {kind:?} differs from {file_type:?}");
    }
    if let Some(dev) = entry.dev {
        assert_eq!(dev, meta.dev() as i32, "{name}: dev");
    }
    if let Some(ino) = entry.ino {
        assert_eq!(ino, meta.ino(), "{name}: ino");
    }
    if let Some(link_count) = entry.link_count {
        assert_eq!(link_count, meta.nlink() as u32, "{name}: link_count");
    }
    if let Some(alloc_size) = entry.alloc_size {
        assert_eq!(alloc_size, meta.blocks() * 512, "{name}: alloc_size");
    }
    if let Some(logical_size) = entry.logical_size {
        assert_eq!(logical_size, meta.size(), "{name}: logical_size");
    }
}
