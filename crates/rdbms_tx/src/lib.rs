//! Minimal transaction boundary for catalog and heap-table v0.
//!
//! Stage 6 implements a deliberately small single-writer transaction layer.
//! Dirty catalog and heap pages are staged in memory, written to WAL as full
//! page images on commit, and only then copied to the database file. Rollback
//! drops staged pages, so uncommitted inserts and table creates do not reach the
//! data file through this API.

use rdbms_catalog::{open_catalog_store, Catalog, CatalogStore, ColumnDef, HeapRow, CATALOG_PAGE_ID};
use rdbms_core::{DbError, DbResult, PageId, RelationId, RowId, SlotId, TxId};
use rdbms_page::{Page, PageType};
use rdbms_vfs::{Vfs, VfsFile};
use rdbms_wal::{WalRecordKind, WalWriter};
use std::collections::BTreeMap;
use std::path::Path;

/// First transaction id produced by a newly opened transaction store.
pub const FIRST_TX_ID: TxId = TxId(1);

/// Runtime state for a transaction handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionState {
    /// The transaction may still accept writes.
    Active,
    /// The transaction committed successfully.
    Committed,
    /// The transaction was explicitly rolled back.
    RolledBack,
}

/// Catalog/heap handle with a WAL-backed transaction boundary.
pub struct TransactionalStore<F: VfsFile> {
    store: CatalogStore<F>,
    wal: WalWriter<F>,
    next_tx_id: u64,
    active_writer: bool,
}

impl<F: VfsFile> TransactionalStore<F> {
    /// Create a transactional store from an opened catalog store and WAL writer.
    pub fn new(store: CatalogStore<F>, wal: WalWriter<F>) -> Self {
        Self {
            store,
            wal,
            next_tx_id: FIRST_TX_ID.0,
            active_writer: false,
        }
    }

    /// Borrow the committed catalog snapshot.
    pub fn catalog(&self) -> &Catalog {
        self.store.catalog()
    }

    /// Scan committed heap rows for a relation.
    pub fn full_scan(&self, relation_id: RelationId) -> DbResult<Vec<HeapRow>> {
        self.store.full_scan(relation_id)
    }

    /// Start one write transaction.
    pub fn begin(&mut self) -> DbResult<Transaction<'_, F>> {
        if self.active_writer {
            return Err(DbError::Retryable(
                "a write transaction is already active".to_string(),
            ));
        }

        let tx_id = self.allocate_tx_id()?;
        self.active_writer = true;
        if let Err(error) = self.wal.append(WalRecordKind::BeginTx { tx_id }) {
            self.active_writer = false;
            return Err(error);
        }

        let working_catalog = self.store.catalog().clone();
        Ok(Transaction {
            manager: self,
            tx_id,
            working_catalog,
            dirty_pages: BTreeMap::new(),
            state: TransactionState::Active,
        })
    }

    /// Create a table in its own transaction.
    pub fn create_table_autocommit(
        &mut self,
        name: impl Into<String>,
        columns: Vec<ColumnDef>,
    ) -> DbResult<RelationId> {
        let mut transaction = self.begin()?;
        let relation_id = transaction.create_table(name, columns)?;
        transaction.commit()?;
        Ok(relation_id)
    }

    /// Insert one raw row in its own transaction.
    pub fn insert_row_autocommit(
        &mut self,
        relation_id: RelationId,
        bytes: &[u8],
    ) -> DbResult<RowId> {
        let mut transaction = self.begin()?;
        let row_id = transaction.insert_row(relation_id, bytes)?;
        transaction.commit()?;
        Ok(row_id)
    }

    /// Force both database and WAL files through their VFS sync boundaries.
    pub fn sync_data(&mut self) -> DbResult<()> {
        self.store.sync_data()?;
        self.wal.sync_data()
    }

    /// Consume the handle and return the catalog store plus WAL writer.
    pub fn into_parts(self) -> (CatalogStore<F>, WalWriter<F>) {
        (self.store, self.wal)
    }

    fn allocate_tx_id(&mut self) -> DbResult<TxId> {
        let tx_id = self.next_tx_id;
        self.next_tx_id = self
            .next_tx_id
            .checked_add(1)
            .ok_or(DbError::User("transaction id overflow".to_string()))?;
        Ok(TxId(tx_id))
    }

    fn finish_writer(&mut self) {
        self.active_writer = false;
    }
}

/// Open catalog data and WAL files through one VFS implementation.
pub fn open_transactional_store<V>(
    vfs: &V,
    data_path: impl AsRef<Path>,
    wal_path: impl AsRef<Path>,
) -> DbResult<TransactionalStore<V::File>>
where
    V: Vfs,
{
    let store = open_catalog_store(vfs, data_path)?;
    let wal_file = vfs.open_database(wal_path.as_ref())?;
    let wal = WalWriter::new(wal_file)?;
    Ok(TransactionalStore::new(store, wal))
}

/// Active write transaction.
pub struct Transaction<'a, F: VfsFile> {
    manager: &'a mut TransactionalStore<F>,
    tx_id: TxId,
    working_catalog: Catalog,
    dirty_pages: BTreeMap<PageId, Page>,
    state: TransactionState,
}

impl<'a, F: VfsFile> Transaction<'a, F> {
    /// Transaction id assigned by the manager.
    pub fn tx_id(&self) -> TxId {
        self.tx_id
    }

    /// Current transaction state.
    pub fn state(&self) -> TransactionState {
        self.state
    }

    /// Borrow the transaction-local catalog snapshot.
    pub fn catalog(&self) -> &Catalog {
        &self.working_catalog
    }

    /// Create a heap table inside this transaction.
    pub fn create_table(
        &mut self,
        name: impl Into<String>,
        columns: Vec<ColumnDef>,
    ) -> DbResult<RelationId> {
        self.ensure_active()?;
        let (relation_id, first_page_id) = self.working_catalog.create_table_metadata(name, columns)?;
        self.dirty_pages
            .insert(first_page_id, Page::new(first_page_id, PageType::Heap));
        self.mark_catalog_dirty()?;
        Ok(relation_id)
    }

    /// Insert raw row bytes into a heap table inside this transaction.
    pub fn insert_row(&mut self, relation_id: RelationId, bytes: &[u8]) -> DbResult<RowId> {
        self.ensure_active()?;
        self.ensure_heap_relation(relation_id)?;
        let page_ids = self.heap_pages(relation_id)?.to_vec();

        for page_id in page_ids {
            let mut page = self.load_page(page_id)?;
            match page.insert_record(bytes) {
                Ok(slot_id) => {
                    self.dirty_pages.insert(page_id, page);
                    return Ok(RowId { page_id, slot_id });
                }
                Err(DbError::User(_)) => {}
                Err(error) => return Err(error),
            }
        }

        let page_id = self.working_catalog.allocate_page_id()?;
        let mut page = Page::new(page_id, PageType::Heap);
        let slot_id = page.insert_record(bytes)?;
        self.working_catalog.append_heap_page(relation_id, page_id)?;
        self.dirty_pages.insert(page_id, page);
        self.mark_catalog_dirty()?;
        Ok(RowId { page_id, slot_id })
    }

    /// Scan heap rows visible inside this transaction.
    pub fn full_scan(&self, relation_id: RelationId) -> DbResult<Vec<HeapRow>> {
        self.ensure_active()?;
        self.ensure_heap_relation(relation_id)?;
        let mut rows = Vec::new();

        for page_id in self.heap_pages(relation_id)? {
            let page = self.load_page(*page_id)?;
            let header = page.header()?;
            if header.page_type != PageType::Heap {
                return Err(DbError::Corruption(format!(
                    "relation page is not a heap page: {}",
                    page_id.0
                )));
            }

            for slot_index in 0..header.slot_count {
                let slot_id = SlotId(slot_index);
                if let Some(bytes) = page.read_record(slot_id)? {
                    rows.push(HeapRow {
                        row_id: RowId {
                            page_id: *page_id,
                            slot_id,
                        },
                        bytes: bytes.to_vec(),
                    });
                }
            }
        }

        Ok(rows)
    }

    /// Commit staged pages: WAL full-page images, WAL sync, data write, data sync.
    pub fn commit(mut self) -> DbResult<()> {
        self.ensure_active()?;
        for page in self.dirty_pages.values() {
            self.manager
                .wal
                .append(WalRecordKind::page_image(self.tx_id, page)?)?;
        }
        self.manager
            .wal
            .append(WalRecordKind::CommitTx { tx_id: self.tx_id })?;
        self.manager.wal.sync_data()?;

        for page in self.dirty_pages.values() {
            self.manager.store.page_file_mut().write_page(page)?;
        }
        self.manager.store.sync_data()?;
        self.manager.store.replace_catalog(self.working_catalog.clone());
        self.state = TransactionState::Committed;
        self.manager.finish_writer();
        Ok(())
    }

    /// Roll back this transaction by discarding all staged pages.
    pub fn rollback(mut self) -> DbResult<()> {
        self.ensure_active()?;
        let result = self
            .manager
            .wal
            .append(WalRecordKind::AbortTx { tx_id: self.tx_id })
            .and_then(|_| self.manager.wal.sync_data());
        self.dirty_pages.clear();
        self.state = TransactionState::RolledBack;
        self.manager.finish_writer();
        result
    }

    fn ensure_active(&self) -> DbResult<()> {
        if self.state == TransactionState::Active {
            Ok(())
        } else {
            Err(DbError::User("transaction is no longer active".to_string()))
        }
    }

    fn mark_catalog_dirty(&mut self) -> DbResult<()> {
        let page = self.working_catalog.to_page()?;
        self.dirty_pages.insert(CATALOG_PAGE_ID, page);
        Ok(())
    }

    fn ensure_heap_relation(&self, relation_id: RelationId) -> DbResult<()> {
        let relation = self.working_catalog.relation_by_id(relation_id).ok_or(DbError::User(
            format!("unknown relation id: {}", relation_id.0),
        ))?;
        if !relation.is_heap_table() {
            return Err(DbError::User(format!(
                "relation is not a heap table: {}",
                relation_id.0
            )));
        }
        Ok(())
    }

    fn heap_pages(&self, relation_id: RelationId) -> DbResult<&[PageId]> {
        let relation = self.working_catalog.relation_by_id(relation_id).ok_or(DbError::User(
            format!("unknown relation id: {}", relation_id.0),
        ))?;
        Ok(relation.heap_pages())
    }

    fn load_page(&self, page_id: PageId) -> DbResult<Page> {
        if let Some(page) = self.dirty_pages.get(&page_id) {
            return Ok(page.clone());
        }
        self.manager.store.page_file().read_page(page_id)
    }
}

impl<'a, F: VfsFile> Drop for Transaction<'a, F> {
    fn drop(&mut self) {
        if self.state == TransactionState::Active {
            self.manager.finish_writer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbms_recovery::{open_database, DatabasePaths};
    use rdbms_vfs::StdVfs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn autocommit_create_insert_and_full_scan() -> DbResult<()> {
        let paths = temp_database_paths("autocommit_scan");
        let vfs = StdVfs::new();
        let mut store = open_transactional_store(&vfs, paths.data_path(), paths.wal_path())?;

        let relation_id = store.create_table_autocommit(
            "events",
            vec![ColumnDef::new("payload", "bytes")],
        )?;
        let row_id = store.insert_row_autocommit(relation_id, b"first event")?;
        let rows = store.full_scan(relation_id)?;

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row_id, row_id);
        assert_eq!(rows[0].bytes, b"first event".to_vec());

        cleanup_database_paths(paths);
        Ok(())
    }

    #[test]
    fn rollback_drops_uncommitted_insert() -> DbResult<()> {
        let paths = temp_database_paths("rollback_insert");
        let vfs = StdVfs::new();
        let mut store = open_transactional_store(&vfs, paths.data_path(), paths.wal_path())?;
        let relation_id = store.create_table_autocommit(
            "items",
            vec![ColumnDef::new("payload", "bytes")],
        )?;

        {
            let mut transaction = store.begin()?;
            transaction.insert_row(relation_id, b"uncommitted")?;
            assert_eq!(transaction.full_scan(relation_id)?.len(), 1);
            transaction.rollback()?;
        }

        assert!(store.full_scan(relation_id)?.is_empty());

        cleanup_database_paths(paths);
        Ok(())
    }

    #[test]
    fn rollback_drops_uncommitted_table_create() -> DbResult<()> {
        let paths = temp_database_paths("rollback_create");
        let vfs = StdVfs::new();
        let mut store = open_transactional_store(&vfs, paths.data_path(), paths.wal_path())?;

        {
            let mut transaction = store.begin()?;
            let relation_id = transaction.create_table(
                "scratch",
                vec![ColumnDef::new("payload", "bytes")],
            )?;
            assert!(transaction.catalog().relation_by_id(relation_id).is_some());
            transaction.rollback()?;
        }

        assert!(store.catalog().relation_by_name("scratch").is_none());

        cleanup_database_paths(paths);
        Ok(())
    }

    #[test]
    fn committed_pages_can_be_recovered_from_wal() -> DbResult<()> {
        let paths = temp_database_paths("recovery_from_wal");
        let vfs = StdVfs::new();
        let relation_id;

        {
            let mut store = open_transactional_store(&vfs, paths.data_path(), paths.wal_path())?;
            let mut transaction = store.begin()?;
            relation_id = transaction.create_table(
                "audit",
                vec![ColumnDef::new("payload", "bytes")],
            )?;
            transaction.insert_row(relation_id, b"committed through wal")?;
            transaction.commit()?;
        }

        std::fs::remove_file(paths.data_path())?;

        let recovered = open_database(&vfs, &paths)?;
        let store = CatalogStore::open(recovered.into_page_file())?;
        let rows = store.full_scan(relation_id)?;

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].bytes, b"committed through wal".to_vec());

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
            "rdbms_tx_{test_name}_{}_{}",
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
