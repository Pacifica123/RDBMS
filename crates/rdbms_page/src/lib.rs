//! Physical page primitives.

use rdbms_core::{DbError, DbResult, Lsn, PageId};

/// Initial page size for the MVP.
pub const PAGE_SIZE: usize = 4096;

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

/// Parsed page header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageHeader {
    /// Page identifier.
    pub page_id: PageId,
    /// Last WAL record applied to the page.
    pub page_lsn: Lsn,
    /// Page kind.
    pub page_type: PageType,
    /// Stored checksum.
    pub checksum: u32,
}

/// In-memory fixed-size page buffer.
#[derive(Clone)]
pub struct Page {
    bytes: Box<[u8; PAGE_SIZE]>,
}

impl Page {
    /// Create a zeroed page for tests and early storage work.
    pub fn zeroed() -> Self {
        Self { bytes: Box::new([0; PAGE_SIZE]) }
    }

    /// Borrow raw page bytes.
    pub fn as_bytes(&self) -> &[u8; PAGE_SIZE] {
        &self.bytes
    }

    /// Mutably borrow raw page bytes.
    pub fn as_mut_bytes(&mut self) -> &mut [u8; PAGE_SIZE] {
        &mut self.bytes
    }
}

/// Placeholder checksum. It is intentionally simple until the format spec is locked.
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0_u32, |acc, byte| acc.wrapping_add(u32::from(*byte)))
}

/// Validate a stored checksum against page bytes.
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
