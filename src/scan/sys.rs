//! The macOS calls that fill the buffers parsed by `bulk`.

use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::bulk::{self, REQUESTED_COMMON, REQUESTED_DIR, REQUESTED_FILE};
use super::listing::{Failure, Listing};

const ATTR_BIT_MAP_COUNT: u16 = 5;

/// The size of the buffer that `getattrlistbulk` fills per call.
pub(crate) const BUF_SIZE: usize = 128 * 1024;

/// The `struct attrlist` of `sys/attr.h`.
///
/// libc's version has a private `reserved` field, so it cannot be built with a
/// struct literal.
#[repr(C)]
struct AttrList {
    bitmapcount: u16,
    reserved: u16,
    commonattr: u32,
    volattr: u32,
    dirattr: u32,
    fileattr: u32,
    forkattr: u32,
}

impl AttrList {
    fn requested() -> Self {
        Self {
            bitmapcount: ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: REQUESTED_COMMON,
            volattr: 0,
            dirattr: REQUESTED_DIR,
            fileattr: REQUESTED_FILE,
            forkattr: 0,
        }
    }
}

fn last_errno() -> i32 {
    std::io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EIO)
}

/// Opens `path` as a directory without following a symbolic link at its end.
fn open_dir(path: &Path) -> Result<OwnedFd, i32> {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return Err(libc::EINVAL);
    };
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
    let fd = unsafe { libc::open(path.as_ptr(), flags) };
    if fd < 0 {
        return Err(last_errno());
    }
    // SAFETY: `open` returned `fd` and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Calls `getattrlistbulk` once and returns how many entries it wrote to `buf`.
fn bulk_call(dir: &OwnedFd, attrs: &mut AttrList, buf: &mut [u8]) -> Result<u32, i32> {
    // SAFETY: `attrs` and `buf` are valid for the call, and `buf.len()` is the
    // size of the buffer that the kernel may write to.
    let count = unsafe {
        libc::getattrlistbulk(
            dir.as_raw_fd(),
            std::ptr::from_mut(attrs).cast(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            0,
        )
    };
    u32::try_from(count).map_err(|_| last_errno())
}

/// Reads every direct child of the directory at `path`.
///
/// The directory is closed before returning, whatever the outcome.
pub(crate) fn read_dir(path: &Path) -> Listing {
    let dir = match open_dir(path) {
        Ok(dir) => dir,
        Err(errno) => {
            return Listing {
                failure: Some(Failure::Open(errno)),
                ..Listing::default()
            };
        }
    };
    let mut attrs = AttrList::requested();
    let mut buf = vec![0u8; BUF_SIZE];
    bulk::collect(&mut buf, |buf| bulk_call(&dir, &mut attrs, buf))
}

#[cfg(test)]
mod tests;
