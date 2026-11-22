//! Parses `getattrlistbulk` responses into a listing.

use super::listing::{Entry, EntryError, Failure, Kind, Listing};

// The bit values come from `sys/attr.h`. libc defines them only for Apple
// targets, and this module is also built for Linux tests, so they are defined
// here.
const ATTR_CMN_NAME: u32 = 0x0000_0001;
const ATTR_CMN_DEVID: u32 = 0x0000_0002;
const ATTR_CMN_OBJTYPE: u32 = 0x0000_0008;
const ATTR_CMN_FILEID: u32 = 0x0200_0000;
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;
const ATTR_FILE_LINKCOUNT: u32 = 0x0000_0001;
const ATTR_FILE_ALLOCSIZE: u32 = 0x0000_0004;
const ATTR_FILE_DATALENGTH: u32 = 0x0000_0200;

// The values of `enum vtype` in `sys/vnode.h`.
const VREG: u32 = 1;
const VDIR: u32 = 2;
const VLNK: u32 = 5;

pub(super) const REQUESTED_COMMON: u32 = ATTR_CMN_RETURNED_ATTRS
    | ATTR_CMN_NAME
    | ATTR_CMN_DEVID
    | ATTR_CMN_OBJTYPE
    | ATTR_CMN_FILEID
    | ATTR_CMN_ERROR;
pub(super) const REQUESTED_FILE: u32 =
    ATTR_FILE_LINKCOUNT | ATTR_FILE_ALLOCSIZE | ATTR_FILE_DATALENGTH;
// Directory attributes are added when the size and the mount point mark of
// subdirectories are read.
pub(super) const REQUESTED_DIR: u32 = 0;

/// The length field plus the five words of returned attribute sets.
const ENTRY_HEADER: usize = 24;

/// A response whose shape cannot be read.
struct Malformed;

/// Reads values in native byte order from one entry, never past its end.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Malformed> {
        let end = self.pos.checked_add(N).ok_or(Malformed)?;
        let chunk = self.bytes.get(self.pos..end).ok_or(Malformed)?;
        self.pos = end;
        chunk.try_into().map_err(|_| Malformed)
    }

    fn u32(&mut self) -> Result<u32, Malformed> {
        self.take().map(u32::from_ne_bytes)
    }

    fn i32(&mut self) -> Result<i32, Malformed> {
        self.take().map(i32::from_ne_bytes)
    }

    fn u64(&mut self) -> Result<u64, Malformed> {
        self.take().map(u64::from_ne_bytes)
    }

    /// Sizes arrive signed, and a negative one cannot be a size.
    fn size(&mut self) -> Result<u64, Malformed> {
        let size = self.take().map(i64::from_ne_bytes)?;
        u64::try_from(size).map_err(|_| Malformed)
    }
}

/// Reads the `count` entries that one call left at the start of `buf`.
///
/// Entries before a malformed one stay in `listing`.
fn parse_batch(buf: &[u8], count: u32, listing: &mut Listing) -> Result<(), Malformed> {
    let mut offset = 0usize;
    for _ in 0..count {
        let head = buf.get(offset..).and_then(|rest| rest.first_chunk::<4>());
        let length = u32::from_ne_bytes(*head.ok_or(Malformed)?) as usize;
        let end = offset.checked_add(length).ok_or(Malformed)?;
        if length < ENTRY_HEADER || end > buf.len() {
            return Err(Malformed);
        }
        parse_entry(&buf[offset..end], listing)?;
        offset = end;
    }
    Ok(())
}

fn parse_entry(entry: &[u8], listing: &mut Listing) -> Result<(), Malformed> {
    let mut r = Reader {
        bytes: entry,
        pos: 4,
    };
    let [common, vol, dir, file, fork] = [(); 5].map(|()| r.u32());
    let (common, vol, dir, file, fork) = (common?, vol?, dir?, file?, fork?);
    // Anything that was not requested means the layout below does not apply.
    if common & !REQUESTED_COMMON != 0
        || dir & !REQUESTED_DIR != 0
        || file & !REQUESTED_FILE != 0
        || vol != 0
        || fork != 0
    {
        return Err(Malformed);
    }

    // The error comes before the name, so reading them the other way round
    // would take the name's offset for an errno.
    let error = if common & ATTR_CMN_ERROR != 0 {
        r.u32()?
    } else {
        0
    };

    let mut name = None;
    if common & ATTR_CMN_NAME != 0 {
        // The offset counts from the `attrreference_t` itself.
        let reference_at = r.pos;
        let offset = r.i32()?;
        let length = r.u32()? as usize;
        let start = reference_at
            .checked_add_signed(offset as isize)
            .ok_or(Malformed)?;
        let end = start.checked_add(length).ok_or(Malformed)?;
        let bytes = entry.get(start..end).ok_or(Malformed)?;
        let until_nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        name = Some(bytes[..until_nul].to_vec());
    }

    if error != 0 {
        // The attributes after the name are not packed for an error entry.
        let errno = i32::try_from(error).map_err(|_| Malformed)?;
        listing.errors.push(EntryError {
            name: name.filter(|n| !n.is_empty()),
            errno,
        });
        return Ok(());
    }

    let dev = if common & ATTR_CMN_DEVID != 0 {
        Some(r.i32()?)
    } else {
        None
    };
    let kind = if common & ATTR_CMN_OBJTYPE != 0 {
        Some(match r.u32()? {
            VDIR => Kind::Dir,
            VREG => Kind::File,
            VLNK => Kind::Symlink,
            _ => Kind::Other,
        })
    } else {
        None
    };
    let ino = if common & ATTR_CMN_FILEID != 0 {
        Some(r.u64()?)
    } else {
        None
    };
    let link_count = if file & ATTR_FILE_LINKCOUNT != 0 {
        Some(r.u32()?)
    } else {
        None
    };
    let alloc_size = if file & ATTR_FILE_ALLOCSIZE != 0 {
        Some(r.size()?)
    } else {
        None
    };
    let logical_size = if file & ATTR_FILE_DATALENGTH != 0 {
        Some(r.size()?)
    } else {
        None
    };

    // `ATTR_CMN_NAME` is mandatory for the call, so a missing name means the
    // response is read wrongly.
    let name = name.filter(|n| !n.is_empty()).ok_or(Malformed)?;
    listing.entries.push(Entry {
        name,
        kind,
        dev,
        ino,
        link_count,
        alloc_size,
        logical_size,
        mount_point: None,
    });
    Ok(())
}

/// Calls `fill` until it reports the end, and reads what it left in `buf`.
///
/// `fill` writes a response into the buffer and returns how many entries it
/// holds, 0 at the end, or the errno of a failed call. Nothing calls it again
/// after the end, a failure or a malformed response.
pub(crate) fn collect(
    buf: &mut [u8],
    mut fill: impl FnMut(&mut [u8]) -> Result<u32, i32>,
) -> Listing {
    let mut listing = Listing::default();
    loop {
        match fill(buf) {
            Ok(0) => break,
            Ok(count) => {
                if parse_batch(buf, count, &mut listing).is_err() {
                    listing.failure = Some(Failure::Malformed);
                    break;
                }
            }
            Err(errno) => {
                listing.failure = Some(Failure::Read(errno));
                break;
            }
        }
    }
    listing
}

#[cfg(test)]
mod tests;
