//! Minimal recovery loop for the storage MVP.
//!
//! Stage 4 wires together the Stage 2 page store and the Stage 3 WAL stream:
//! open the database files, scan WAL records, redo committed full-page images,
//! ignore uncommitted page images and return a page file that has passed the
//! same page validation used by the VFS layer.

use rdbms_core::{DbResult, Lsn, PageId, TxId};
use rdbms_page::{Page, PAGE_SIZE};
use rdbms_vfs::{PageFile, Vfs, VfsFile};
use rdbms_wal::{redo_committed_page_images, PageImageRedo, WalReader};
use std::path::{Path, PathBuf};

/// Files that make up the current database-open boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabasePaths {
    data_path: PathBuf,
    wal_path: PathBuf,
}

impl DatabasePaths {
    /// Create database paths from explicit data and WAL files.
    pub fn new(data_path: impl Into<PathBuf>, wal_path: impl Into<PathBuf>) -> Self {
        Self {
            data_path: data_path.into(),
            wal_path: wal_path.into(),
        }
    }

    /// Path to the fixed-size page file.
    pub fn data_path(&self) -> &Path {
        &self.data_path
    }

    /// Path to the WAL file.
    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }
}

/// Summary of one recovery pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RecoveryReport {
    /// Number of WAL records scanned and validated.
    pub scanned_wal_records: usize,
    /// Number of committed page images applied to the data file.
    pub redone_page_images: usize,
}

/// Database handle returned after a recovery pass.
pub struct RecoveredDatabase<F: VfsFile> {
    page_file: PageFile<F>,
    report: RecoveryReport,
}

impl<F: VfsFile> RecoveredDatabase<F> {
    /// Borrow the recovered page file.
    pub fn page_file(&self) -> &PageFile<F> {
        &self.page_file
    }

    /// Mutably borrow the recovered page file.
    pub fn page_file_mut(&mut self) -> &mut PageFile<F> {
        &mut self.page_file
    }

    /// Return the recovery report from the open pass.
    pub fn recovery_report(&self) -> RecoveryReport {
        self.report
    }

    /// Consume the handle and return the recovered page file.
    pub fn into_page_file(self) -> PageFile<F> {
        self.page_file
    }
}

/// Open data and WAL files through `vfs`, then run the recovery pass.
pub fn open_database<V>(vfs: &V, paths: &DatabasePaths) -> DbResult<RecoveredDatabase<V::File>>
where
    V: Vfs,
{
    let data_file = vfs.open_database(paths.data_path())?;
    let wal_file = vfs.open_database(paths.wal_path())?;
    recover_page_file(PageFile::new(data_file), wal_file)
}

/// Run recovery over an already opened page file and WAL file.
pub fn recover_page_file<F>(page_file: PageFile<F>, wal_file: F) -> DbResult<RecoveredDatabase<F>>
where
    F: VfsFile,
{
    let records = WalReader::new(wal_file).read_all()?;
    let scanned_wal_records = records.len();
    let mut redo = PageFileRedo::new(page_file);

    redo_committed_page_images(&records, &mut redo)?;
    redo.page_file.sync_data()?;

    Ok(RecoveredDatabase {
        page_file: redo.page_file,
        report: RecoveryReport {
            scanned_wal_records,
            redone_page_images: redo.redone_page_images,
        },
    })
}

struct PageFileRedo<F: VfsFile> {
    page_file: PageFile<F>,
    redone_page_images: usize,
}

impl<F: VfsFile> PageFileRedo<F> {
    fn new(page_file: PageFile<F>) -> Self {
        Self {
            page_file,
            redone_page_images: 0,
        }
    }
}

impl<F: VfsFile> PageImageRedo for PageFileRedo<F> {
    fn redo_page_image(
        &mut self,
        _lsn: Lsn,
        _tx_id: TxId,
        page_id: PageId,
        image: &[u8; PAGE_SIZE],
    ) -> DbResult<()> {
        let page = Page::from_bytes(*image)?;
        self.page_file.write_page_at(page_id, &page)?;
        self.redone_page_images += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbms_core::{DbError, SlotId};
    use rdbms_page::{Page, PageType};
    use rdbms_vfs::{open_page_file, StdVfs, Vfs, VfsFile};
    use rdbms_wal::{WalRecordKind, WalWriter};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn open_scans_wal_redoes_committed_and_ignores_uncommitted() -> DbResult<()> {
        let paths = temp_database_paths("redo_committed");
        let vfs = StdVfs::new();
        let committed_slot: SlotId;

        {
            let mut page_file = open_page_file(&vfs, paths.data_path())?;
            let mut old_page = Page::new(PageId(0), PageType::Heap);
            old_page.insert_record(b"old")?;
            page_file.write_page(&old_page)?;
            page_file.sync_data()?;
        }

        {
            let wal_file = vfs.open_database(paths.wal_path())?;
            let mut writer = WalWriter::new(wal_file)?;

            let mut committed_page = Page::new(PageId(0), PageType::Heap);
            committed_slot = committed_page.insert_record(b"committed")?;
            let mut uncommitted_page = Page::new(PageId(1), PageType::Heap);
            uncommitted_page.insert_record(b"uncommitted")?;

            writer.append(WalRecordKind::BeginTx { tx_id: TxId(10) })?;
            writer.append(WalRecordKind::page_image(TxId(10), &committed_page)?)?;
            writer.append(WalRecordKind::CommitTx { tx_id: TxId(10) })?;
            writer.append(WalRecordKind::BeginTx { tx_id: TxId(20) })?;
            writer.append(WalRecordKind::page_image(TxId(20), &uncommitted_page)?)?;
            writer.sync_data()?;
        }

        let recovered = open_database(&vfs, &paths)?;
        assert_eq!(
            recovered.recovery_report(),
            RecoveryReport {
                scanned_wal_records: 5,
                redone_page_images: 1,
            }
        );
        let page = recovered.page_file().read_page(PageId(0))?;
        assert_eq!(page.read_record(committed_slot)?, Some(&b"committed"[..]));
        drop(recovered);

        cleanup_database_paths(paths);
        Ok(())
    }

    #[test]
    fn recovery_is_idempotent_for_committed_page_images() -> DbResult<()> {
        let paths = temp_database_paths("idempotent");
        let vfs = StdVfs::new();
        let committed_slot: SlotId;

        {
            let wal_file = vfs.open_database(paths.wal_path())?;
            let mut writer = WalWriter::new(wal_file)?;
            let mut page = Page::new(PageId(0), PageType::Heap);
            committed_slot = page.insert_record(b"stable after repeated recovery")?;

            writer.append(WalRecordKind::BeginTx { tx_id: TxId(30) })?;
            writer.append(WalRecordKind::page_image(TxId(30), &page)?)?;
            writer.append(WalRecordKind::CommitTx { tx_id: TxId(30) })?;
            writer.sync_data()?;
        }

        {
            let recovered = open_database(&vfs, &paths)?;
            assert_eq!(recovered.recovery_report().redone_page_images, 1);
        }

        {
            let recovered = open_database(&vfs, &paths)?;
            assert_eq!(recovered.recovery_report().redone_page_images, 1);
            let page = recovered.page_file().read_page(PageId(0))?;
            assert_eq!(
                page.read_record(committed_slot)?,
                Some(&b"stable after repeated recovery"[..])
            );
        }

        cleanup_database_paths(paths);
        Ok(())
    }

    #[test]
    fn recovery_propagates_wal_corruption() -> DbResult<()> {
        let paths = temp_database_paths("wal_corruption");
        let vfs = StdVfs::new();

        {
            let mut wal_file = vfs.open_database(paths.wal_path())?;
            wal_file.write_all_at(0, b"not a wal record")?;
            wal_file.sync_data()?;
        }

        let error = open_database(&vfs, &paths)
            .err()
            .ok_or(DbError::InternalInvariant("corrupt wal was accepted"))?;
        assert!(matches!(error, DbError::Corruption(_)));

        cleanup_database_paths(paths);
        Ok(())
    }

    fn temp_database_paths(test_name: &str) -> DatabasePaths {
        let base = temp_base_path(test_name);
        DatabasePaths::new(base.with_extension("dbonrs"), base.with_extension("wal"))
    }

    fn temp_base_path(test_name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "rdbms_recovery_{test_name}_{}_{}",
            std::process::id(),
            nanos
        ));
        path
    }

    fn cleanup_database_paths(paths: DatabasePaths) {
        let _ = std::fs::remove_file(paths.data_path());
        let _ = std::fs::remove_file(paths.wal_path());
    }
}
