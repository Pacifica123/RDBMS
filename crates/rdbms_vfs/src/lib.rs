//! VFS boundary and page-file storage for portability and fault-injection tests.
//!
//! This crate is the first disk-backed layer above `rdbms_page`: it maps a
//! physical `PageId` to a fixed offset in a database file and validates page
//! bytes after every read.

use rdbms_core::{DbError, DbResult, PageId};
use rdbms_page::{Page, PAGE_SIZE};
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
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

    /// Open an existing database file or create it when it does not exist.
    fn open_database(&self, path: &Path) -> DbResult<Self::File>;
}

/// Standard-library VFS implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdVfs;

impl StdVfs {
    /// Create a standard VFS value.
    pub fn new() -> Self {
        Self
    }
}

impl Vfs for StdVfs {
    type File = StdVfsFile;

    fn open_database(&self, path: &Path) -> DbResult<Self::File> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        Ok(StdVfsFile::new(file))
    }
}

/// Random-access database file backed by `std::fs::File`.
pub struct StdVfsFile {
    file: File,
}

impl StdVfsFile {
    /// Wrap an already opened standard-library file.
    pub fn new(file: File) -> Self {
        Self { file }
    }

    /// Return the wrapped standard-library file.
    pub fn into_inner(self) -> File {
        self.file
    }
}

impl VfsFile for StdVfsFile {
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> DbResult<()> {
        read_exact_at(&self.file, offset, buf)?;
        Ok(())
    }

    fn write_all_at(&mut self, offset: u64, buf: &[u8]) -> DbResult<()> {
        write_all_at(&self.file, offset, buf)?;
        Ok(())
    }

    fn sync_data(&mut self) -> DbResult<()> {
        self.file.sync_data()?;
        Ok(())
    }
}

/// Disk-backed fixed-size page file.
///
/// Layout v0 is intentionally simple: page `N` starts at
/// `N * rdbms_page::PAGE_SIZE`. There is no allocator or file-header bootstrap
/// yet; higher layers will decide which pages are meaningful.
pub struct PageFile<F: VfsFile> {
    file: F,
}

impl<F: VfsFile> PageFile<F> {
    /// Create a page-file abstraction over a VFS file.
    pub fn new(file: F) -> Self {
        Self { file }
    }

    /// Return the wrapped VFS file.
    pub fn into_inner(self) -> F {
        self.file
    }

    /// Write a validated page at the offset implied by its own page id.
    pub fn write_page(&mut self, page: &Page) -> DbResult<()> {
        let header = page.header()?;
        self.write_page_at(header.page_id, page)
    }

    /// Write a validated page at the given page id.
    ///
    /// The supplied `page_id` must match the id stored in the page header.
    pub fn write_page_at(&mut self, page_id: PageId, page: &Page) -> DbResult<()> {
        page.validate()?;
        let header = page.header()?;
        if header.page_id != page_id {
            return Err(DbError::User(format!(
                "page id mismatch on write: target={}, page_header={}",
                page_id.0, header.page_id.0
            )));
        }

        let offset = page_offset(page_id)?;
        self.file.write_all_at(offset, page.as_bytes())
    }

    /// Read and validate a page by physical page id.
    pub fn read_page(&self, page_id: PageId) -> DbResult<Page> {
        let mut bytes = [0_u8; PAGE_SIZE];
        let offset = page_offset(page_id)?;
        self.file.read_exact_at(offset, &mut bytes)?;

        let page = Page::from_bytes(bytes)?;
        let header = page.header()?;
        if header.page_id != page_id {
            return Err(DbError::Corruption(format!(
                "page id mismatch on read: requested={}, page_header={}",
                page_id.0, header.page_id.0
            )));
        }
        Ok(page)
    }

    /// Force durable file contents through the VFS boundary.
    pub fn sync_data(&mut self) -> DbResult<()> {
        self.file.sync_data()
    }
}

/// Open a page file through an arbitrary VFS implementation.
pub fn open_page_file<V>(vfs: &V, path: impl AsRef<Path>) -> DbResult<PageFile<V::File>>
where
    V: Vfs,
{
    let file = vfs.open_database(path.as_ref())?;
    Ok(PageFile::new(file))
}

fn page_offset(page_id: PageId) -> DbResult<u64> {
    page_id
        .0
        .checked_mul(PAGE_SIZE as u64)
        .ok_or(DbError::User("page offset overflow".to_string()))
}

fn read_exact_at(file: &File, mut offset: u64, mut buf: &mut [u8]) -> io::Result<()> {
    while !buf.is_empty() {
        match read_at(file, buf, offset) {
            Ok(0) => return Err(io::Error::from(ErrorKind::UnexpectedEof)),
            Ok(read) => {
                offset = offset
                    .checked_add(read as u64)
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "read offset overflow"))?;
                let (_, rest) = buf.split_at_mut(read);
                buf = rest;
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn write_all_at(file: &File, mut offset: u64, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        match write_at(file, buf, offset) {
            Ok(0) => return Err(io::Error::from(ErrorKind::WriteZero)),
            Ok(written) => {
                offset = offset.checked_add(written as u64).ok_or_else(|| {
                    io::Error::new(ErrorKind::InvalidInput, "write offset overflow")
                })?;
                buf = &buf[written..];
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.read_at(buf, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;

    file.seek_read(buf, offset)
}

#[cfg(not(any(unix, windows)))]
fn read_at(_file: &File, _buf: &mut [u8], _offset: u64) -> io::Result<usize> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "random-access reads are not implemented on this platform",
    ))
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.write_at(buf, offset)
}

#[cfg(windows)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;

    file.seek_write(buf, offset)
}

#[cfg(not(any(unix, windows)))]
fn write_at(_file: &File, _buf: &[u8], _offset: u64) -> io::Result<usize> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "random-access writes are not implemented on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbms_core::{DbError, SlotId};
    use rdbms_page::PageType;
    use std::io::{Seek, SeekFrom, Write};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn std_vfs_writes_reads_and_reopens_page() -> DbResult<()> {
        let path = temp_db_path("reopen");
        let slot_id: SlotId;

        {
            let vfs = StdVfs::new();
            let mut page_file = open_page_file(&vfs, &path)?;
            let mut page = Page::new(PageId(0), PageType::Heap);
            slot_id = page.insert_record(b"record survives reopen")?;

            page_file.write_page(&page)?;
            page_file.sync_data()?;
        }

        {
            let vfs = StdVfs::new();
            let page_file = open_page_file(&vfs, &path)?;
            let page = page_file.read_page(PageId(0))?;

            assert_eq!(
                page.read_record(slot_id)?,
                Some(&b"record survives reopen"[..])
            );
        }

        cleanup_temp_file(path);
        Ok(())
    }

    #[test]
    fn corrupt_page_is_detected_after_reopen() -> DbResult<()> {
        let path = temp_db_path("corrupt");

        {
            let vfs = StdVfs::new();
            let mut page_file = open_page_file(&vfs, &path)?;
            let mut page = Page::new(PageId(1), PageType::Heap);
            page.insert_record(b"checksum protected payload")?;

            page_file.write_page(&page)?;
            page_file.sync_data()?;
        }

        {
            let mut file = OpenOptions::new().read(true).write(true).open(&path)?;
            file.seek(SeekFrom::Start(PAGE_SIZE as u64 + 128))?;
            file.write_all(&[0xA5])?;
            file.sync_data()?;
        }

        let vfs = StdVfs::new();
        let page_file = open_page_file(&vfs, &path)?;
        let error = page_file
            .read_page(PageId(1))
            .err()
            .ok_or(DbError::InternalInvariant("corrupt page was accepted"))?;

        assert!(matches!(error, DbError::Corruption(_)));

        cleanup_temp_file(path);
        Ok(())
    }

    fn temp_db_path(test_name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "rdbms_vfs_{test_name}_{}_{}.dbonrs",
            std::process::id(),
            nanos
        ));
        path
    }

    fn cleanup_temp_file(path: PathBuf) {
        let _ = std::fs::remove_file(path);
    }
}
