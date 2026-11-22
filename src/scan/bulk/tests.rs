use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use super::*;

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
}

impl Spec {
    /// Lays the entry out in the order `getattrlistbulk(2)` documents.
    fn encode(&self) -> Vec<u8> {
        let mut common = ATTR_CMN_RETURNED_ATTRS;
        let mut file = 0;
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
        set(self.link_count.is_some(), &mut file, ATTR_FILE_LINKCOUNT);
        set(self.alloc.is_some(), &mut file, ATTR_FILE_ALLOCSIZE);
        set(self.size.is_some(), &mut file, ATTR_FILE_DATALENGTH);
        let returned = self.returned.unwrap_or([common, 0, 0, file, 0]);

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

/// Returns a `fill` that plays back `steps` and a counter of its calls.
///
/// Once the steps run out it reports the end of the directory.
fn scripted(
    steps: Vec<Result<(Vec<u8>, u32), i32>>,
) -> (impl FnMut(&mut [u8]) -> Result<u32, i32>, Rc<Cell<u32>>) {
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
