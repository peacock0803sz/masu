use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use super::*;
use crate::scan::listing::{Entry, EntryError, Failure, Kind, Listing};

/// The buffer size the tests hand to `collect`.
const TEST_BUF: usize = 4096;

/// One entry of a mocked response. A `None` field is left out of the entry.
///
/// `returned`, `name_ref` and `length` override what `encode` would derive, so
/// that tests can build broken responses.
#[derive(Clone, Default)]
struct Spec {
    returned: Option<[u32; 5]>,
    error: Option<u32>,
    name: Option<Vec<u8>>,
    name_ref: Option<(i32, u32)>,
    dev: Option<i32>,
    objtype: Option<u32>,
    fileid: Option<u64>,
    link_count: Option<u32>,
    alloc: Option<i64>,
    size: Option<i64>,
    length: Option<u32>,
    mount_status: Option<u32>,
    dir_alloc: Option<i64>,
    dir_size: Option<i64>,
}

impl Spec {
    /// Lays the entry out in the order `getattrlistbulk(2)` documents.
    fn encode(&self) -> Vec<u8> {
        let mut common = ATTR_CMN_RETURNED_ATTRS;
        let mut file = 0;
        let mut dir = 0;
        let set = |present: bool, bits: &mut u32, bit: u32| {
            if present {
                *bits |= bit;
            }
        };
        set(self.error.is_some(), &mut common, ATTR_CMN_ERROR);
        set(self.name.is_some(), &mut common, ATTR_CMN_NAME);
        set(self.dev.is_some(), &mut common, ATTR_CMN_DEVID);
        set(self.objtype.is_some(), &mut common, ATTR_CMN_OBJTYPE);
        set(self.fileid.is_some(), &mut common, ATTR_CMN_FILEID);
        set(self.mount_status.is_some(), &mut dir, ATTR_DIR_MOUNTSTATUS);
        set(self.dir_alloc.is_some(), &mut dir, ATTR_DIR_ALLOCSIZE);
        set(self.dir_size.is_some(), &mut dir, ATTR_DIR_DATALENGTH);
        set(self.link_count.is_some(), &mut file, ATTR_FILE_LINKCOUNT);
        set(self.alloc.is_some(), &mut file, ATTR_FILE_ALLOCSIZE);
        set(self.size.is_some(), &mut file, ATTR_FILE_DATALENGTH);
        let returned = self.returned.unwrap_or([common, 0, dir, file, 0]);

        let mut out = vec![0u8; 4];
        for word in returned {
            out.extend_from_slice(&word.to_ne_bytes());
        }
        if let Some(error) = self.error {
            out.extend_from_slice(&error.to_ne_bytes());
        }
        let name_ref_at = out.len();
        if self.name.is_some() || self.name_ref.is_some() {
            out.extend_from_slice(&[0; 8]);
        }
        if let Some(dev) = self.dev {
            out.extend_from_slice(&dev.to_ne_bytes());
        }
        if let Some(objtype) = self.objtype {
            out.extend_from_slice(&objtype.to_ne_bytes());
        }
        if let Some(fileid) = self.fileid {
            out.extend_from_slice(&fileid.to_ne_bytes());
        }
        if let Some(mount_status) = self.mount_status {
            out.extend_from_slice(&mount_status.to_ne_bytes());
        }
        if let Some(dir_alloc) = self.dir_alloc {
            out.extend_from_slice(&dir_alloc.to_ne_bytes());
        }
        if let Some(dir_size) = self.dir_size {
            out.extend_from_slice(&dir_size.to_ne_bytes());
        }
        if let Some(link_count) = self.link_count {
            out.extend_from_slice(&link_count.to_ne_bytes());
        }
        if let Some(alloc) = self.alloc {
            out.extend_from_slice(&alloc.to_ne_bytes());
        }
        if let Some(size) = self.size {
            out.extend_from_slice(&size.to_ne_bytes());
        }

        let name_at = out.len();
        if let Some(name) = &self.name {
            out.extend_from_slice(name);
            out.push(0);
            out.resize(out.len().next_multiple_of(4), 0);
        }
        if self.name.is_some() || self.name_ref.is_some() {
            let (offset, len) = self.name_ref.unwrap_or_else(|| {
                let name = self.name.as_ref().map_or(0, |n| n.len() + 1);
                ((name_at - name_ref_at) as i32, name as u32)
            });
            out[name_ref_at..name_ref_at + 4].copy_from_slice(&offset.to_ne_bytes());
            out[name_ref_at + 4..name_ref_at + 8].copy_from_slice(&len.to_ne_bytes());
        }

        out.resize(out.len().next_multiple_of(8), 0);
        let length = self.length.unwrap_or(out.len() as u32);
        out[..4].copy_from_slice(&length.to_ne_bytes());
        out
    }
}

/// Joins the entries and returns them with their count.
fn batch(specs: &[Spec]) -> (Vec<u8>, u32) {
    let bytes = specs.iter().flat_map(Spec::encode).collect();
    (bytes, specs.len() as u32)
}

/// How many times a scripted `fill` was called.
type Calls = Rc<Cell<u32>>;

/// Returns a `fill` that plays back `steps` and a counter of its calls.
///
/// Once the steps run out it reports the end of the directory.
fn scripted(
    steps: Vec<Result<(Vec<u8>, u32), i32>>,
) -> (impl FnMut(&mut [u8]) -> Result<u32, i32>, Calls) {
    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    let mut steps = VecDeque::from(steps);
    let fill = move |buf: &mut [u8]| {
        counter.set(counter.get() + 1);
        match steps.pop_front() {
            Some(Ok((bytes, count))) => {
                buf.fill(0);
                buf[..bytes.len()].copy_from_slice(&bytes);
                Ok(count)
            }
            Some(Err(errno)) => Err(errno),
            None => Ok(0),
        }
    };
    (fill, calls)
}

/// A regular file with every attribute present.
fn plain(name: &[u8]) -> Spec {
    Spec {
        name: Some(name.to_vec()),
        dev: Some(7),
        objtype: Some(1),
        fileid: Some(10),
        link_count: Some(1),
        alloc: Some(4096),
        size: Some(5),
        ..Spec::default()
    }
}

/// What `plain` is expected to read as.
fn plain_entry(name: &[u8]) -> Entry {
    Entry {
        name: name.to_vec(),
        kind: Some(Kind::File),
        dev: Some(7),
        ino: Some(10),
        link_count: Some(1),
        alloc_size: Some(4096),
        logical_size: Some(5),
        mount_point: None,
    }
}

fn read(specs: &[Spec]) -> (Listing, u32) {
    let (fill, calls) = scripted(vec![Ok(batch(specs))]);
    let listing = collect(&mut vec![0u8; TEST_BUF], fill);
    (listing, calls.get())
}

#[test]
fn reads_hand_written_entry() {
    // Written from the layout in getattrlistbulk(2), not from `encode`.
    let words: [&[u8]; 17] = [
        &72u32.to_ne_bytes(),
        &0x8200_000Bu32.to_ne_bytes(),
        &0u32.to_ne_bytes(),
        &0u32.to_ne_bytes(),
        &0x0000_0205u32.to_ne_bytes(),
        &0u32.to_ne_bytes(),
        &44i32.to_ne_bytes(),
        &2u32.to_ne_bytes(),
        &0x0100_0004i32.to_ne_bytes(),
        &1u32.to_ne_bytes(),
        &12345u64.to_ne_bytes(),
        &1u32.to_ne_bytes(),
        &4096i64.to_ne_bytes(),
        &5i64.to_ne_bytes(),
        b"a",
        &[0, 0, 0],
        &[],
    ];
    let bytes = words.concat();
    assert_eq!(bytes.len(), 72);

    let spec = Spec {
        name: Some(b"a".to_vec()),
        dev: Some(0x0100_0004),
        objtype: Some(1),
        fileid: Some(12345),
        link_count: Some(1),
        alloc: Some(4096),
        size: Some(5),
        ..Spec::default()
    };
    assert_eq!(spec.encode(), bytes);

    let (fill, _) = scripted(vec![Ok((bytes, 1))]);
    let listing = collect(&mut vec![0u8; TEST_BUF], fill);
    assert_eq!(
        listing,
        Listing {
            entries: vec![Entry {
                name: b"a".to_vec(),
                kind: Some(Kind::File),
                dev: Some(0x0100_0004),
                ino: Some(12345),
                link_count: Some(1),
                alloc_size: Some(4096),
                logical_size: Some(5),
                mount_point: None,
            }],
            errors: vec![],
            failure: None,
        }
    );
}

#[test]
fn maps_object_types_to_kinds() {
    for (objtype, kind) in [
        (1, Kind::File),
        (5, Kind::Symlink),
        (7, Kind::Other),
        (0, Kind::Other),
    ] {
        let spec = Spec {
            objtype: Some(objtype),
            ..plain(b"x")
        };
        let (listing, _) = read(&[spec]);
        assert_eq!(listing.entries[0].kind, Some(kind), "objtype {objtype}");
    }
}

#[test]
fn directory_without_directory_attributes_has_no_sizes() {
    let spec = Spec {
        name: Some(b"d".to_vec()),
        dev: Some(7),
        objtype: Some(2),
        fileid: Some(11),
        ..Spec::default()
    };
    let (listing, _) = read(&[spec]);
    assert_eq!(
        listing.entries,
        vec![Entry {
            name: b"d".to_vec(),
            kind: Some(Kind::Dir),
            dev: Some(7),
            ino: Some(11),
            link_count: None,
            alloc_size: None,
            logical_size: None,
            mount_point: None,
        }]
    );
    assert_eq!(listing.failure, None);
}

#[test]
fn keeps_the_bytes_of_a_decomposed_name() {
    let name = [0xE3, 0x81, 0x8B, 0xE3, 0x82, 0x99];
    let (listing, _) = read(&[plain(&name)]);
    assert_eq!(listing.entries[0].name, name);
}

#[test]
fn name_ends_at_the_first_nul() {
    let (listing, _) = read(&[plain(b"ab\0cd")]);
    assert_eq!(listing.entries[0].name, b"ab");
}

#[test]
fn missing_attributes_stay_none() {
    let no_alloc = Spec {
        alloc: None,
        ..plain(b"x")
    };
    let no_type = Spec {
        objtype: None,
        ..plain(b"y")
    };
    let (listing, _) = read(&[no_alloc, no_type]);
    assert_eq!(
        listing.entries[0],
        Entry {
            alloc_size: None,
            ..plain_entry(b"x")
        }
    );
    assert_eq!(
        listing.entries[1],
        Entry {
            kind: None,
            ..plain_entry(b"y")
        }
    );
}

#[test]
fn zero_error_reads_as_a_plain_entry() {
    let spec = Spec {
        error: Some(0),
        ..plain(b"x")
    };
    let (listing, _) = read(&[spec]);
    assert_eq!(listing.entries, vec![plain_entry(b"x")]);
    assert_eq!(listing.errors, vec![]);
    assert_eq!(listing.failure, None);
}

#[test]
fn keeps_the_order_of_a_response() {
    let (listing, _) = read(&[plain(b"b"), plain(b"a")]);
    assert_eq!(listing.entries, vec![plain_entry(b"b"), plain_entry(b"a")]);
}

#[test]
fn keeps_entries_read_before_a_failed_call() {
    let (fill, calls) = scripted(vec![Ok(batch(&[plain(b"a"), plain(b"b")])), Err(5)]);
    let listing = collect(&mut vec![0u8; TEST_BUF], fill);
    assert_eq!(listing.entries, vec![plain_entry(b"a"), plain_entry(b"b")]);
    assert_eq!(listing.failure, Some(Failure::Read(5)));
    assert_eq!(calls.get(), 2);
}

#[test]
fn count_larger_than_the_bytes_is_malformed() {
    let (bytes, _) = batch(&[plain(b"a")]);
    let (fill, _) = scripted(vec![Ok((bytes, 2))]);
    let listing = collect(&mut vec![0u8; TEST_BUF], fill);
    assert_eq!(listing.entries, vec![plain_entry(b"a")]);
    assert_eq!(listing.failure, Some(Failure::Malformed));
}

#[test]
fn entry_without_a_name_is_malformed() {
    let nameless = Spec {
        name: None,
        ..plain(b"")
    };
    let (listing, _) = read(&[plain(b"a"), nameless]);
    assert_eq!(listing.entries, vec![plain_entry(b"a")]);
    assert_eq!(listing.failure, Some(Failure::Malformed));
}

#[test]
fn broken_second_entry_stops_the_read() {
    let with_returned = |returned: [u32; 5]| Spec {
        returned: Some(returned),
        ..plain(b"x")
    };
    let common = REQUESTED_COMMON;
    let file = REQUESTED_FILE;
    let broken = [
        (
            "entry shorter than its header",
            Spec {
                length: Some(16),
                ..plain(b"x")
            },
        ),
        (
            "entry longer than the buffer",
            Spec {
                length: Some(TEST_BUF as u32 + 8),
                ..plain(b"x")
            },
        ),
        (
            "values past the end of the entry",
            Spec {
                length: Some(28),
                ..plain(b"x")
            },
        ),
        (
            "common attribute that was not requested",
            with_returned([common | 0x4, 0, 0, file, 0]),
        ),
        ("volume attributes", with_returned([common, 1, 0, file, 0])),
        (
            "name outside of the entry",
            Spec {
                name_ref: Some((1000, 2)),
                ..plain(b"x")
            },
        ),
        (
            "negative allocation size",
            Spec {
                alloc: Some(-1),
                ..plain(b"x")
            },
        ),
    ];
    for (why, second) in broken {
        let (listing, calls) = read(&[plain(b"a"), second]);
        assert_eq!(listing.entries, vec![plain_entry(b"a")], "{why}");
        assert_eq!(listing.failure, Some(Failure::Malformed), "{why}");
        assert_eq!(calls, 1, "{why}");
    }
}

#[test]
fn reads_until_the_end_across_calls() {
    let (fill, calls) = scripted(vec![
        Ok(batch(&[plain(b"a"), plain(b"b")])),
        Ok(batch(&[plain(b"c"), plain(b"d"), plain(b"e")])),
        Ok(batch(&[plain(b"f")])),
    ]);
    let listing = collect(&mut vec![0u8; TEST_BUF], fill);
    let names: Vec<_> = listing.entries.iter().map(|e| e.name.clone()).collect();
    assert_eq!(
        names,
        [b"a", b"b", b"c", b"d", b"e", b"f"].map(|n| n.to_vec())
    );
    assert_eq!(listing.failure, None);
    assert_eq!(calls.get(), 4);
}

#[test]
fn empty_directory_ends_after_one_call() {
    let (fill, calls) = scripted(vec![]);
    let listing = collect(&mut vec![0u8; TEST_BUF], fill);
    assert_eq!(listing, Listing::default());
    assert_eq!(calls.get(), 1);
}

#[test]
fn reads_error_entries_between_plain_ones() {
    let denied = Spec {
        error: Some(13),
        name: Some(b"denied".to_vec()),
        ..Spec::default()
    };
    let (listing, _) = read(&[plain(b"a"), denied, plain(b"b")]);
    assert_eq!(listing.entries, vec![plain_entry(b"a"), plain_entry(b"b")]);
    assert_eq!(
        listing.errors,
        vec![EntryError {
            name: Some(b"denied".to_vec()),
            errno: 13
        }]
    );
    assert_eq!(listing.failure, None);
}

#[test]
fn error_entry_without_a_name_has_none() {
    let spec = Spec {
        error: Some(2),
        ..Spec::default()
    };
    let (listing, _) = read(&[spec]);
    assert_eq!(
        listing.errors,
        vec![EntryError {
            name: None,
            errno: 2
        }]
    );
    assert_eq!(listing.entries, vec![]);
    assert_eq!(listing.failure, None);
}

#[test]
fn reads_hand_written_error_entry() {
    // Written from the layout in getattrlistbulk(2), not from `encode`.
    let parts: [&[u8]; 9] = [
        &48u32.to_ne_bytes(),
        &0xA000_0001u32.to_ne_bytes(),
        &[0; 16],
        &13u32.to_ne_bytes(),
        &8i32.to_ne_bytes(),
        &7u32.to_ne_bytes(),
        b"denied\0",
        &[0; 5],
        &[],
    ];
    let bytes = parts.concat();
    assert_eq!(bytes.len(), 48);

    let spec = Spec {
        error: Some(13),
        name: Some(b"denied".to_vec()),
        ..Spec::default()
    };
    assert_eq!(spec.encode(), bytes);

    let (fill, _) = scripted(vec![Ok((bytes, 1))]);
    let listing = collect(&mut vec![0u8; TEST_BUF], fill);
    assert_eq!(
        listing.errors,
        vec![EntryError {
            name: Some(b"denied".to_vec()),
            errno: 13
        }]
    );
    assert_eq!(listing.entries, vec![]);
    assert_eq!(listing.failure, None);
}

#[test]
fn retries_an_interrupted_call() {
    let (fill, calls) = scripted(vec![Err(4), Ok(batch(&[plain(b"a")]))]);
    let listing = collect(&mut vec![0u8; TEST_BUF], fill);
    assert_eq!(listing.entries, vec![plain_entry(b"a")]);
    assert_eq!(listing.failure, None);
    assert_eq!(calls.get(), 3);
}

#[test]
fn errno_that_does_not_fit_is_malformed() {
    let spec = Spec {
        error: Some(0x8000_0000),
        ..plain(b"x")
    };
    let (listing, _) = read(&[plain(b"a"), spec]);
    assert_eq!(listing.entries, vec![plain_entry(b"a")]);
    assert_eq!(listing.failure, Some(Failure::Malformed));
}

/// A directory with every directory attribute present.
fn directory(name: &[u8], mount_status: u32) -> Spec {
    Spec {
        name: Some(name.to_vec()),
        dev: Some(7),
        objtype: Some(2),
        fileid: Some(11),
        mount_status: Some(mount_status),
        dir_alloc: Some(0),
        dir_size: Some(64),
        ..Spec::default()
    }
}

#[test]
fn directory_attributes_fill_sizes_and_mount_mark() {
    let (listing, _) = read(&[directory(b"d", 1)]);
    assert_eq!(
        listing.entries,
        vec![Entry {
            name: b"d".to_vec(),
            kind: Some(Kind::Dir),
            dev: Some(7),
            ino: Some(11),
            link_count: None,
            alloc_size: Some(0),
            logical_size: Some(64),
            mount_point: Some(true),
        }]
    );
    assert_eq!(listing.failure, None);
}

#[test]
fn only_the_mount_point_bit_marks_a_mount_point() {
    for status in [0, DIR_MNTSTATUS_TRIGGER] {
        let (listing, _) = read(&[directory(b"d", status)]);
        assert_eq!(listing.entries[0].mount_point, Some(false), "{status}");
    }
}

#[test]
fn kindless_entry_reads_whichever_group_came_back() {
    let spec = Spec {
        objtype: None,
        ..directory(b"d", 0)
    };
    let (listing, _) = read(&[spec]);
    let entry = &listing.entries[0];
    assert_eq!(entry.kind, None);
    assert_eq!(entry.mount_point, Some(false));
    assert_eq!(entry.logical_size, Some(64));
}

#[test]
fn mismatched_attribute_groups_are_malformed() {
    let both = Spec {
        link_count: Some(1),
        ..directory(b"x", 0)
    };
    let dir_with_file_attributes = Spec {
        link_count: Some(1),
        alloc: Some(0),
        size: Some(0),
        mount_status: None,
        dir_alloc: None,
        dir_size: None,
        ..directory(b"x", 0)
    };
    let file_with_dir_attributes = Spec {
        mount_status: Some(0),
        link_count: None,
        alloc: None,
        size: None,
        ..plain(b"x")
    };
    for (why, second) in [
        ("both groups", both),
        ("directory with file attributes", dir_with_file_attributes),
        ("file with directory attributes", file_with_dir_attributes),
    ] {
        let (listing, _) = read(&[plain(b"a"), second]);
        assert_eq!(listing.entries, vec![plain_entry(b"a")], "{why}");
        assert_eq!(listing.failure, Some(Failure::Malformed), "{why}");
    }
}
