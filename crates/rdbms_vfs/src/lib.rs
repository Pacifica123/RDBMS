//! VFS boundary for portability and fault-injection tests.

use rdbms_core::DbResult;
use std::path::Path;

/// Minimal random-access file contract for the first storage milestones.
pub trait VfsFile {
    /// Read exactly into `buf` starting at `offset`.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> DbResult<()>;

    /// Write all bytes from `buf` starting at `offset`.
    fn write_all_at(&mut self, offset: u64, buf: &[u8]) -> DbResult<()>;

    /// Force durable file contents according to the platform implementation.
    fn sync_data(&mut self) -> DbResult<()>;
}

/// Filesystem abstraction used by storage code.
pub trait Vfs {
    /// Concrete file type returned by this VFS.
    type File: VfsFile;

    /// Open or create a database file.
    fn open_database(&self, path: &Path) -> DbResult<Self::File>;
}

/// Standard-library VFS placeholder.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdVfs;

impl StdVfs {
    /// Create a standard VFS value.
    pub fn new() -> Self {
        Self
    }
}
