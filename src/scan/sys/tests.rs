use std::ffi::OsStr;
use std::fs;
use std::os::macos::fs::MetadataExt as _;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use std::io::Write as _;

use super::{BUF_SIZE, read_dir, read_dir_with};
use crate::scan::listing::{Entry, Kind, Listing};

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

/// Writes the fixture used by `entries_match_lstat` into `root`.
fn make_fixture(root: &Path) {
    fs::write(root.join("regular"), vec![b'r'; 10000]).unwrap();

    let sparse = fs::File::create(root.join("sparse")).unwrap();
    sparse.set_len(16 * 1024 * 1024).unwrap();
    std::os::unix::fs::FileExt::write_all_at(&sparse, &[b's'; 4096], 0).unwrap();
    std::os::unix::fs::symlink("sparse", root.join("link")).unwrap();

    fs::write(root.join("empty"), b"").unwrap();
    fs::write(root.join("hard-a"), b"shared").unwrap();
    fs::hard_link(root.join("hard-a"), root.join("hard-b")).unwrap();
    // "か" followed by a combining voiced sound mark, as the NFD form of "が".
    let nfd = OsStr::from_bytes(&[0xE3, 0x81, 0x8B, 0xE3, 0x82, 0x99]);
    fs::write(root.join(nfd), b"nfd").unwrap();
    fs::create_dir(root.join("subdir")).unwrap();

    let fifo = std::ffi::CString::new(root.join("fifo").as_os_str().as_bytes()).unwrap();
    // SAFETY: `fifo` is a valid NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o644) }, 0);

    fs::write(root.join("rsrc"), vec![b'f'; 100]).unwrap();
    fs::write(root.join("rsrc/..namedfork/rsrc"), vec![b'x'; 5000]).unwrap();

    // Transparent compression only happens when the file is copied with it.
    let source = TempDir::new("compress-source");
    let original = source.path().join("original");
    fs::write(&original, "masu ".repeat(64 * 1024 / 5)).unwrap();
    let status = std::process::Command::new("/usr/bin/ditto")
        .arg("--hfsCompression")
        .arg(&original)
        .arg(root.join("compressed"))
        .status()
        .expect("run ditto");
    assert!(status.success(), "ditto failed");
    const UF_COMPRESSED: u32 = 0x20;
    let flags = fs::symlink_metadata(root.join("compressed"))
        .unwrap()
        .st_flags();
    assert!(
        flags & UF_COMPRESSED != 0,
        "the fixture was not compressed, so it cannot test compressed files"
    );
}

#[test]
fn entries_match_lstat() {
    let dir = TempDir::new("lstat");
    make_fixture(dir.path());

    let listing = read_dir(dir.path());
    assert_eq!(listing.failure, None);
    assert_eq!(listing.errors, vec![]);

    for entry in &listing.entries {
        assert_matches_lstat(dir.path(), entry);
        if entry.kind == Some(Kind::Dir) {
            assert!(entry.dev.is_some() && entry.ino.is_some(), "{entry:?}");
        } else {
            assert!(
                entry.kind.is_some()
                    && entry.dev.is_some()
                    && entry.ino.is_some()
                    && entry.link_count.is_some()
                    && entry.alloc_size.is_some()
                    && entry.logical_size.is_some(),
                "{entry:?}"
            );
        }
    }

    let find = |name: &[u8]| {
        listing
            .entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("no entry named {}", String::from_utf8_lossy(name)))
    };
    let nfd: &[u8] = &[0xE3, 0x81, 0x8B, 0xE3, 0x82, 0x99];
    for name in [
        &b"regular"[..],
        b"sparse",
        b"empty",
        b"hard-a",
        b"hard-b",
        nfd,
        b"rsrc",
        b"compressed",
    ] {
        assert_eq!(find(name).kind, Some(Kind::File));
    }
    assert_eq!(find(b"link").kind, Some(Kind::Symlink));
    assert_eq!(find(b"subdir").kind, Some(Kind::Dir));
    assert_eq!(find(b"fifo").kind, Some(Kind::Other));

    let sparse = find(b"sparse");
    assert!(sparse.alloc_size < sparse.logical_size);
    let (a, b) = (find(b"hard-a"), find(b"hard-b"));
    assert_eq!(a.link_count, Some(2));
    assert_eq!(b.link_count, Some(2));
    assert_eq!((a.dev, a.ino), (b.dev, b.ino));

    let mut names: Vec<Vec<u8>> = listing.entries.iter().map(|e| e.name.clone()).collect();
    let mut expected: Vec<Vec<u8>> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().as_bytes().to_vec())
        .collect();
    names.sort();
    expected.sort();
    assert_eq!(names, expected);
}

/// Writes a line straight to stderr so that it shows up in CI logs.
///
/// libtest captures `eprintln!` of passing tests and drops it, and
/// `writeln!(stderr(), ..).unwrap()` trips `clippy::explicit_write`.
fn log_line(line: &str) {
    writeln!(std::io::stderr().lock(), "{line}").expect("write to stderr");
}

#[test]
fn reads_ten_thousand_entries() {
    let dir = TempDir::new("many");
    let mut expected = std::collections::BTreeSet::new();
    for i in 0..10_000 {
        let name = format!("{i:05}-{}", "x".repeat(245));
        fs::write(dir.path().join(&name), b"").unwrap();
        expected.insert(name.into_bytes());
    }

    let mut calls = 0u32;
    let mut buf = vec![0u8; BUF_SIZE];
    let listing = read_dir_with(dir.path(), &mut buf, || calls += 1);

    assert_eq!(listing.entries.len(), 10_000);
    let names: std::collections::BTreeSet<_> =
        listing.entries.iter().map(|e| e.name.clone()).collect();
    assert_eq!(names, expected);
    assert_eq!(listing.errors, vec![]);
    assert_eq!(listing.failure, None);
    assert!(calls <= 100, "{calls} getattrlistbulk calls");
    log_line(&format!("getattrlistbulk calls for 10000 entries: {calls}"));
}

#[test]
fn reads_empty_directory() {
    let dir = TempDir::new("empty");
    let listing = read_dir(dir.path());
    assert_eq!(listing, Listing::default());
}
