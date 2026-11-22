//! Parses `getattrlistbulk` responses into a listing.

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

#[cfg(test)]
mod tests;
