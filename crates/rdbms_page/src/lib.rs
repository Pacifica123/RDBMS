//! Physical page primitives.
//!
//! This crate owns the first concrete storage invariant of the project:
//! fixed-size pages with a small header, a slot directory and variable-size
//! record payloads packed from the end of the page.

use rdbms_core::{DbError, DbResult, Lsn, PageId, SlotId};

/// Initial page size for the MVP.
pub const PAGE_SIZE: usize = 4096;

const PAGE_MAGIC: u32 = 0x5244_4250; // "RDBP"
const PAGE_VERSION: u16 = 1;
const HEADER_SIZE: usize = 34;
const SLOT_SIZE: usize = 6;

const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_PAGE_TYPE: usize = 6;
const OFF_PAGE_ID: usize = 8;
const OFF_PAGE_LSN: usize = 16;
const OFF_FREE_START: usize = 24;
const OFF_FREE_END: usize = 26;
const OFF_SLOT_COUNT: usize = 28;
const OFF_CHECKSUM: usize = 30;

const SLOT_UNUSED: u16 = 0;
const SLOT_LIVE: u16 = 1;
const SLOT_DEAD: u16 = 2;

/// Known page kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageType {
    /// File header page.
    FileHeader,
    /// Heap/table data page.
    Heap,
    /// Catalog page.
    Catalog,
    /// Free-space or allocator page.
    FreeMap,
}

impl PageType {
    fn to_u16(self) -> u16 {
        match self {
            Self::FileHeader => 1,
            Self::Heap => 2,
            Self::Catalog => 3,
            Self::FreeMap => 4,
        }
    }

    fn from_u16(value: u16) -> DbResult<Self> {
        match value {
            1 => Ok(Self::FileHeader),
            2 => Ok(Self::Heap),
            3 => Ok(Self::Catalog),
            4 => Ok(Self::FreeMap),
            _ => Err(DbError::Corruption(format!("unknown page type: {value}"))),
        }
    }
}

/// Parsed page header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageHeader {
    /// Page identifier.
    pub page_id: PageId,
    /// Last WAL record applied to the page.
    pub page_lsn: Lsn,
    /// Page kind.
    pub page_type: PageType,
    /// Start of free space after the slot directory.
    pub free_start: u16,
    /// End of free space before packed record bytes.
    pub free_end: u16,
    /// Number of slots in the page slot directory.
    pub slot_count: u16,
    /// Stored checksum.
    pub checksum: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SlotEntry {
    offset: u16,
    len: u16,
    flags: u16,
}

impl SlotEntry {
    fn unused() -> Self {
        Self {
            offset: 0,
            len: 0,
            flags: SLOT_UNUSED,
        }
    }

    fn is_live(self) -> bool {
        self.flags == SLOT_LIVE
    }

    fn is_dead(self) -> bool {
        self.flags == SLOT_DEAD
    }
}

/// In-memory fixed-size page buffer.
#[derive(Clone, Debug)]
pub struct Page {
    bytes: [u8; PAGE_SIZE],
}

impl Page {
    /// Create a zeroed page for tests and low-level byte experiments.
    pub fn zeroed() -> Self {
        Self {
            bytes: [0; PAGE_SIZE],
        }
    }

    /// Create a formatted slotted page.
    pub fn new(page_id: PageId, page_type: PageType) -> Self {
        let mut page = Self::zeroed();
        write_u32(&mut page.bytes, OFF_MAGIC, PAGE_MAGIC);
        write_u16(&mut page.bytes, OFF_VERSION, PAGE_VERSION);
        write_u16(&mut page.bytes, OFF_PAGE_TYPE, page_type.to_u16());
        write_u64(&mut page.bytes, OFF_PAGE_ID, page_id.0);
        write_u64(&mut page.bytes, OFF_PAGE_LSN, 0);
        write_u16(&mut page.bytes, OFF_FREE_START, HEADER_SIZE as u16);
        write_u16(&mut page.bytes, OFF_FREE_END, PAGE_SIZE as u16);
        write_u16(&mut page.bytes, OFF_SLOT_COUNT, 0);
        page.refresh_checksum();
        page
    }

    /// Parse and validate a page copied from storage.
    pub fn from_bytes(bytes: [u8; PAGE_SIZE]) -> DbResult<Self> {
        let page = Self {
            bytes,
        };
        page.validate()?;
        Ok(page)
    }

    /// Borrow raw page bytes.
    pub fn as_bytes(&self) -> &[u8; PAGE_SIZE] {
        &self.bytes
    }

    /// Mutably borrow raw page bytes.
    ///
    /// Callers that mutate raw bytes are responsible for restoring invariants.
    pub fn as_mut_bytes(&mut self) -> &mut [u8; PAGE_SIZE] {
        &mut self.bytes
    }

    /// Return parsed header fields after validating basic header shape.
    pub fn header(&self) -> DbResult<PageHeader> {
        if read_u32(&self.bytes, OFF_MAGIC) != PAGE_MAGIC {
            return Err(DbError::Corruption("invalid page magic".to_string()));
        }
        let version = read_u16(&self.bytes, OFF_VERSION);
        if version != PAGE_VERSION {
            return Err(DbError::Corruption(format!(
                "unsupported page version: {version}"
            )));
        }

        let free_start = read_u16(&self.bytes, OFF_FREE_START);
        let free_end = read_u16(&self.bytes, OFF_FREE_END);
        let slot_count = read_u16(&self.bytes, OFF_SLOT_COUNT);
        let min_free_start = HEADER_SIZE + usize::from(slot_count) * SLOT_SIZE;

        if usize::from(free_start) != min_free_start {
            return Err(DbError::Corruption(format!(
                "invalid free_start: expected {min_free_start}, got {free_start}"
            )));
        }
        if free_start > free_end || usize::from(free_end) > PAGE_SIZE {
            return Err(DbError::Corruption(format!(
                "invalid free space boundaries: free_start={free_start}, free_end={free_end}"
            )));
        }

        Ok(PageHeader {
            page_id: PageId(read_u64(&self.bytes, OFF_PAGE_ID)),
            page_lsn: Lsn(read_u64(&self.bytes, OFF_PAGE_LSN)),
            page_type: PageType::from_u16(read_u16(&self.bytes, OFF_PAGE_TYPE))?,
            free_start,
            free_end,
            slot_count,
            checksum: read_u32(&self.bytes, OFF_CHECKSUM),
        })
    }

    /// Validate header, checksum and every live slot boundary.
    pub fn validate(&self) -> DbResult<()> {
        let header = self.header()?;
        validate_page_checksum(&self.bytes)?;

        for slot_index in 0..header.slot_count {
            let slot = self.slot_entry(slot_index)?;
            if slot.flags != SLOT_UNUSED && slot.flags != SLOT_LIVE && slot.flags != SLOT_DEAD {
                return Err(DbError::Corruption(format!(
                    "invalid slot flags at slot {slot_index}: {}",
                    slot.flags
                )));
            }
            if slot.is_live() {
                let start = usize::from(slot.offset);
                let end = start
                    .checked_add(usize::from(slot.len))
                    .ok_or(DbError::Corruption("slot length overflows page".to_string()))?;
                if start < usize::from(header.free_end) || end > PAGE_SIZE {
                    return Err(DbError::Corruption(format!(
                        "slot {slot_index} points outside record area"
                    )));
                }
            }
        }

        Ok(())
    }

    /// Insert variable-size record bytes and return a stable slot id.
    pub fn insert_record(&mut self, record: &[u8]) -> DbResult<SlotId> {
        if record.len() > u16::MAX as usize {
            return Err(DbError::User("record is larger than u16 length limit".to_string()));
        }

        let header = self.header()?;
        let reusable_slot = self.find_dead_slot(header.slot_count)?;
        let needs_slot_bytes = reusable_slot.is_none();
        let slot_bytes = if needs_slot_bytes { SLOT_SIZE } else { 0 };
        let needed = record
            .len()
            .checked_add(slot_bytes)
            .ok_or(DbError::User("record is too large".to_string()))?;
        let free_bytes = usize::from(header.free_end - header.free_start);

        if needed > free_bytes {
            return Err(DbError::User(format!(
                "not enough free page space: need {needed}, have {free_bytes}"
            )));
        }

        let new_offset = usize::from(header.free_end) - record.len();
        self.bytes[new_offset..usize::from(header.free_end)].copy_from_slice(record);
        write_u16(&mut self.bytes, OFF_FREE_END, new_offset as u16);

        let slot_index = if let Some(index) = reusable_slot {
            index
        } else {
            let index = header.slot_count;
            let new_slot_count = index
                .checked_add(1)
                .ok_or(DbError::User("page slot count overflow".to_string()))?;
            write_u16(&mut self.bytes, OFF_SLOT_COUNT, new_slot_count);
            write_u16(
                &mut self.bytes,
                OFF_FREE_START,
                HEADER_SIZE as u16 + new_slot_count * SLOT_SIZE as u16,
            );
            index
        };

        self.write_slot_entry(
            slot_index,
            SlotEntry {
                offset: new_offset as u16,
                len: record.len() as u16,
                flags: SLOT_LIVE,
            },
        )?;
        self.refresh_checksum();
        Ok(SlotId(slot_index))
    }

    /// Read a record by slot id. Dead or unused slots return `Ok(None)`.
    pub fn read_record(&self, slot_id: SlotId) -> DbResult<Option<&[u8]>> {
        let header = self.header()?;
        if slot_id.0 >= header.slot_count {
            return Err(DbError::User(format!("slot {} is out of range", slot_id.0)));
        }

        let slot = self.slot_entry(slot_id.0)?;
        if !slot.is_live() {
            return Ok(None);
        }

        let start = usize::from(slot.offset);
        let end = start
            .checked_add(usize::from(slot.len))
            .ok_or(DbError::Corruption("slot length overflows page".to_string()))?;
        if end > PAGE_SIZE {
            return Err(DbError::Corruption("slot points past page end".to_string()));
        }
        Ok(Some(&self.bytes[start..end]))
    }

    /// Mark a slot as dead. Returns `false` if the slot was already dead/unused.
    pub fn delete_record(&mut self, slot_id: SlotId) -> DbResult<bool> {
        let header = self.header()?;
        if slot_id.0 >= header.slot_count {
            return Err(DbError::User(format!("slot {} is out of range", slot_id.0)));
        }

        let mut slot = self.slot_entry(slot_id.0)?;
        if !slot.is_live() {
            return Ok(false);
        }

        slot.flags = SLOT_DEAD;
        self.write_slot_entry(slot_id.0, slot)?;
        self.refresh_checksum();
        Ok(true)
    }

    /// Repack live record bytes while keeping live slot ids unchanged.
    pub fn compact(&mut self) -> DbResult<()> {
        let header = self.header()?;
        let mut live_records: Vec<(u16, Vec<u8>)> = Vec::new();

        for slot_index in 0..header.slot_count {
            let slot = self.slot_entry(slot_index)?;
            if slot.is_live() {
                let data = self
                    .read_record(SlotId(slot_index))?
                    .ok_or(DbError::Corruption("live slot without data".to_string()))?
                    .to_vec();
                live_records.push((slot_index, data));
            }
        }

        let free_start = HEADER_SIZE + usize::from(header.slot_count) * SLOT_SIZE;
        self.bytes[free_start..PAGE_SIZE].fill(0);

        let mut free_end = PAGE_SIZE;
        for (slot_index, data) in live_records {
            free_end -= data.len();
            self.bytes[free_end..free_end + data.len()].copy_from_slice(&data);
            self.write_slot_entry(
                slot_index,
                SlotEntry {
                    offset: free_end as u16,
                    len: data.len() as u16,
                    flags: SLOT_LIVE,
                },
            )?;
        }

        write_u16(&mut self.bytes, OFF_FREE_END, free_end as u16);
        self.refresh_checksum();
        Ok(())
    }

    /// Number of live records currently stored in the page.
    pub fn live_record_count(&self) -> DbResult<u16> {
        let header = self.header()?;
        let mut count = 0_u16;
        for slot_index in 0..header.slot_count {
            if self.slot_entry(slot_index)?.is_live() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Free bytes available without compaction.
    pub fn free_space(&self) -> DbResult<usize> {
        let header = self.header()?;
        Ok(usize::from(header.free_end - header.free_start))
    }

    /// Recompute and store the page checksum.
    pub fn refresh_checksum(&mut self) {
        write_u32(&mut self.bytes, OFF_CHECKSUM, 0);
        let value = checksum(&self.bytes);
        write_u32(&mut self.bytes, OFF_CHECKSUM, value);
    }

    fn find_dead_slot(&self, slot_count: u16) -> DbResult<Option<u16>> {
        for slot_index in 0..slot_count {
            if self.slot_entry(slot_index)?.is_dead() {
                return Ok(Some(slot_index));
            }
        }
        Ok(None)
    }

    fn slot_entry(&self, slot_index: u16) -> DbResult<SlotEntry> {
        let offset = slot_offset(slot_index)?;
        Ok(SlotEntry {
            offset: read_u16(&self.bytes, offset),
            len: read_u16(&self.bytes, offset + 2),
            flags: read_u16(&self.bytes, offset + 4),
        })
    }

    fn write_slot_entry(&mut self, slot_index: u16, slot: SlotEntry) -> DbResult<()> {
        let offset = slot_offset(slot_index)?;
        write_u16(&mut self.bytes, offset, slot.offset);
        write_u16(&mut self.bytes, offset + 2, slot.len);
        write_u16(&mut self.bytes, offset + 4, slot.flags);
        Ok(())
    }
}

fn slot_offset(slot_index: u16) -> DbResult<usize> {
    let offset = HEADER_SIZE
        .checked_add(usize::from(slot_index) * SLOT_SIZE)
        .ok_or(DbError::Corruption("slot offset overflow".to_string()))?;
    if offset + SLOT_SIZE > PAGE_SIZE {
        return Err(DbError::Corruption(format!(
            "slot {slot_index} is outside page"
        )));
    }
    Ok(offset)
}

/// Placeholder checksum. It is intentionally simple until the format spec is locked.
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0_u32, |acc, byte| acc.wrapping_add(u32::from(*byte)))
}

/// Validate a stored checksum against arbitrary bytes.
pub fn validate_checksum(bytes: &[u8], expected: u32) -> DbResult<()> {
    let actual = checksum(bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(DbError::Corruption(format!(
            "page checksum mismatch: expected {expected}, got {actual}"
        )))
    }
}

/// Validate the checksum embedded in a formatted page.
pub fn validate_page_checksum(bytes: &[u8; PAGE_SIZE]) -> DbResult<()> {
    let expected = read_u32(bytes, OFF_CHECKSUM);
    let mut copy = *bytes;
    write_u32(&mut copy, OFF_CHECKSUM, 0);
    validate_checksum(&copy, expected)
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn insert_and_read_records_by_slot_id() {
        let mut page = Page::new(PageId(7), PageType::Heap);

        let first = page.insert_record(b"alpha").expect("insert first record");
        let second = page.insert_record(b"beta").expect("insert second record");

        assert_eq!(page.read_record(first).expect("read first"), Some(&b"alpha"[..]));
        assert_eq!(page.read_record(second).expect("read second"), Some(&b"beta"[..]));
        assert_eq!(page.live_record_count().expect("count live"), 2);
        page.validate().expect("page remains valid");
    }

    #[test]
    fn delete_hides_record_without_moving_other_slots() {
        let mut page = Page::new(PageId(8), PageType::Heap);

        let first = page.insert_record(b"alpha").expect("insert first");
        let second = page.insert_record(b"beta").expect("insert second");
        let third = page.insert_record(b"gamma").expect("insert third");

        assert!(page.delete_record(second).expect("delete second"));
        assert!(!page.delete_record(second).expect("delete second again"));

        assert_eq!(page.read_record(first).expect("read first"), Some(&b"alpha"[..]));
        assert_eq!(page.read_record(second).expect("read second"), None);
        assert_eq!(page.read_record(third).expect("read third"), Some(&b"gamma"[..]));
        assert_eq!(page.live_record_count().expect("count live"), 2);
        page.validate().expect("page remains valid");
    }

    #[test]
    fn compact_preserves_live_slot_ids_and_payloads() {
        let mut page = Page::new(PageId(9), PageType::Heap);
        let mut live = BTreeMap::new();

        for index in 0..40_u8 {
            let payload = vec![index; usize::from(index % 11 + 1)];
            let slot = page.insert_record(&payload).expect("insert record");
            live.insert(slot, payload);
        }

        let slots: Vec<SlotId> = live.keys().copied().collect();
        for (position, slot) in slots.iter().enumerate() {
            if position % 3 == 0 {
                page.delete_record(*slot).expect("delete record");
                live.remove(slot);
            }
        }

        let free_before = page.free_space().expect("free space before compact");
        page.compact().expect("compact page");
        let free_after = page.free_space().expect("free space after compact");

        assert!(free_after >= free_before);
        for (slot, payload) in live {
            assert_eq!(page.read_record(slot).expect("read after compact"), Some(&payload[..]));
        }
        page.validate().expect("page remains valid");
    }

    #[test]
    fn checksum_detects_corruption() {
        let mut page = Page::new(PageId(10), PageType::Heap);
        page.insert_record(b"stable bytes").expect("insert record");
        page.validate().expect("valid before corruption");

        let mut bytes = *page.as_bytes();
        bytes[PAGE_SIZE - 1] ^= 0x55;

        let err = Page::from_bytes(bytes).expect_err("corruption must be detected");
        assert!(matches!(err, DbError::Corruption(_)));
    }

    #[test]
    fn oversized_record_is_rejected() {
        let mut page = Page::new(PageId(11), PageType::Heap);
        let payload = vec![1_u8; PAGE_SIZE];

        let err = page
            .insert_record(&payload)
            .expect_err("oversized insert must fail");
        assert!(matches!(err, DbError::User(_)));
    }
}
