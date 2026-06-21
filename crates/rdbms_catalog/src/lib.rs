//! Persistent catalog and heap-table v0.
//!
//! Stage 5 keeps the API deliberately internal and byte-oriented. The catalog
//! is stored in page 0 as one encoded record. Heap tables are ordinary slotted
//! pages whose page ids are listed by the catalog storage object. Stage 6 adds
//! transaction-staging helpers used by `rdbms_tx`; those helpers still expose no
//! SQL-facing schema model.

use rdbms_core::{DbError, DbResult, PageId, RelationId, RowId, SlotId};
use rdbms_page::{Page, PageType};
use rdbms_vfs::{open_page_file, PageFile, Vfs, VfsFile};
use std::path::Path;

/// Physical page reserved for catalog metadata.
pub const CATALOG_PAGE_ID: PageId = PageId(0);

const CATALOG_MAGIC: &[u8; 4] = b"RDBC";
const CATALOG_VERSION: u16 = 1;
const STORAGE_HEAP: u8 = 1;
const STORAGE_INDEX: u8 = 2;

/// Relation kind stored in the catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelationKind {
    /// Ordinary heap table.
    Table,
    /// Index relation.
    Index,
    /// System catalog relation.
    System,
}

impl RelationKind {
    fn to_u8(&self) -> u8 {
        match self {
            Self::Table => 1,
            Self::Index => 2,
            Self::System => 3,
        }
    }

    fn from_u8(value: u8) -> DbResult<Self> {
        match value {
            1 => Ok(Self::Table),
            2 => Ok(Self::Index),
            3 => Ok(Self::System),
            _ => Err(DbError::Corruption(format!(
                "unknown catalog relation kind: {value}"
            ))),
        }
    }
}

/// Column metadata stored by the catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnDef {
    /// Column name.
    pub name: String,
    /// Storage-independent type name.
    pub type_name: String,
}

impl ColumnDef {
    /// Create a column definition.
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
        }
    }
}

/// Persisted extension metadata stored in the catalog page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionInfo {
    /// Extension name.
    pub name: String,
    /// ABI version accepted when the extension was installed.
    pub abi_version: u32,
    /// Loading kind, for example `static`.
    pub kind: String,
}

impl ExtensionInfo {
    /// Create extension metadata.
    pub fn new(name: impl Into<String>, abi_version: u32, kind: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            abi_version,
            kind: kind.into(),
        }
    }
}

/// Heap storage metadata for one relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeapStorage {
    pages: Vec<PageId>,
}

impl HeapStorage {
    /// Create heap storage metadata from page ids.
    pub fn new(pages: Vec<PageId>) -> Self {
        Self { pages }
    }

    /// Pages that currently belong to this heap relation.
    pub fn pages(&self) -> &[PageId] {
        &self.pages
    }

    fn push_page(&mut self, page_id: PageId) {
        self.pages.push(page_id);
    }
}

/// B+Tree index storage metadata for one index relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexStorage {
    table_id: RelationId,
    column_name: String,
    root_page_id: PageId,
}

impl IndexStorage {
    /// Create index storage metadata.
    pub fn new(
        table_id: RelationId,
        column_name: impl Into<String>,
        root_page_id: PageId,
    ) -> Self {
        Self {
            table_id,
            column_name: column_name.into(),
            root_page_id,
        }
    }

    /// Heap table indexed by this index relation.
    pub fn table_id(&self) -> RelationId {
        self.table_id
    }

    /// Indexed column name.
    pub fn column_name(&self) -> &str {
        &self.column_name
    }

    /// Root page of the B+Tree.
    pub fn root_page_id(&self) -> PageId {
        self.root_page_id
    }

    /// Update root page after a root split.
    pub fn set_root_page_id(&mut self, root_page_id: PageId) {
        self.root_page_id = root_page_id;
    }
}

/// Physical storage object referenced by a catalog relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageObject {
    /// Heap table storage.
    Heap(HeapStorage),
    /// B+Tree index storage.
    BPlusTree(IndexStorage),
}

impl StorageObject {
    /// Return heap pages when this storage object is a heap.
    pub fn heap_pages(&self) -> &[PageId] {
        match self {
            Self::Heap(storage) => storage.pages(),
            Self::BPlusTree(_) => &[],
        }
    }

    /// Return index storage when this storage object is a B+Tree.
    pub fn index_storage(&self) -> Option<&IndexStorage> {
        match self {
            Self::Heap(_) => None,
            Self::BPlusTree(storage) => Some(storage),
        }
    }

    /// Mutably return index storage when this storage object is a B+Tree.
    pub fn index_storage_mut(&mut self) -> Option<&mut IndexStorage> {
        match self {
            Self::Heap(_) => None,
            Self::BPlusTree(storage) => Some(storage),
        }
    }

    fn heap_storage_mut(&mut self) -> Option<&mut HeapStorage> {
        match self {
            Self::Heap(storage) => Some(storage),
            Self::BPlusTree(_) => None,
        }
    }
}

/// Relation metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationInfo {
    /// Stable relation id.
    pub id: RelationId,
    /// Relation name.
    pub name: String,
    /// Relation kind.
    pub kind: RelationKind,
    /// Physical storage object for this relation.
    pub storage: StorageObject,
    /// Column metadata for the relation.
    pub columns: Vec<ColumnDef>,
}

impl RelationInfo {
    /// Return heap pages for table scans and inserts.
    pub fn heap_pages(&self) -> &[PageId] {
        self.storage.heap_pages()
    }

    /// Return true when this relation is an ordinary heap table.
    pub fn is_heap_table(&self) -> bool {
        self.kind == RelationKind::Table
    }

    /// Return B+Tree index metadata when this relation is an index.
    pub fn index_storage(&self) -> Option<&IndexStorage> {
        self.storage.index_storage()
    }
}

/// In-memory representation of the persistent catalog page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Catalog {
    next_relation_id: u64,
    next_page_id: u64,
    relations: Vec<RelationInfo>,
    extensions: Vec<ExtensionInfo>,
}

impl Catalog {
    /// Create an empty catalog with page 0 reserved for the catalog itself.
    pub fn empty() -> Self {
        Self {
            next_relation_id: 1,
            next_page_id: 1,
            relations: Vec::new(),
            extensions: Vec::new(),
        }
    }

    /// Load the catalog from page 0, or bootstrap it when the file is empty.
    pub fn load_or_bootstrap<F: VfsFile>(page_file: &mut PageFile<F>) -> DbResult<Self> {
        let len = page_file.len()?;
        if len == 0 {
            let catalog = Self::empty();
            catalog.save(page_file)?;
            return Ok(catalog);
        }
        if len < rdbms_page::PAGE_SIZE as u64 {
            return Err(DbError::Corruption(format!(
                "database file is too short for catalog page: {len} bytes"
            )));
        }

        let page = page_file.read_page(CATALOG_PAGE_ID)?;
        let header = page.header()?;
        if header.page_type != PageType::Catalog {
            return Err(DbError::Corruption(
                "page 0 is not a catalog page".to_string(),
            ));
        }

        let bytes = page
            .read_record(SlotId(0))?
            .ok_or(DbError::Corruption("catalog record is missing".to_string()))?;
        decode_catalog(bytes)
    }

    /// Build the catalog page image for transaction staging and direct writes.
    pub fn to_page(&self) -> DbResult<Page> {
        let bytes = encode_catalog(self)?;
        let mut page = Page::new(CATALOG_PAGE_ID, PageType::Catalog);
        page.insert_record(&bytes)?;
        Ok(page)
    }

    /// Persist this catalog as a single record in page 0.
    pub fn save<F: VfsFile>(&self, page_file: &mut PageFile<F>) -> DbResult<()> {
        let page = self.to_page()?;
        page_file.write_page(&page)
    }

    /// Return all relation records in catalog order.
    pub fn relations(&self) -> &[RelationInfo] {
        &self.relations
    }


    /// Return installed extension metadata records in catalog order.
    pub fn extensions(&self) -> &[ExtensionInfo] {
        &self.extensions
    }

    /// Find installed extension metadata by name.
    pub fn extension_by_name(&self, name: &str) -> Option<&ExtensionInfo> {
        self.extensions
            .iter()
            .find(|extension| extension.name == name)
    }

    /// Register extension metadata. Re-registering the same extension is idempotent.
    pub fn register_extension_metadata(
        &mut self,
        name: impl Into<String>,
        abi_version: u32,
        kind: impl Into<String>,
    ) -> DbResult<bool> {
        let name = name.into();
        let kind = kind.into();
        validate_name("extension", &name)?;
        validate_name("extension kind", &kind)?;

        if let Some(existing) = self.extension_by_name(&name) {
            if existing.abi_version == abi_version && existing.kind == kind {
                return Ok(false);
            }
            return Err(DbError::User(format!(
                "extension already registered with different metadata: {name}"
            )));
        }

        self.extensions
            .push(ExtensionInfo::new(name, abi_version, kind));
        Ok(true)
    }

    /// Find a relation by id.
    pub fn relation_by_id(&self, relation_id: RelationId) -> Option<&RelationInfo> {
        self.relations
            .iter()
            .find(|relation| relation.id == relation_id)
    }

    /// Find a relation by name.
    pub fn relation_by_name(&self, name: &str) -> Option<&RelationInfo> {
        self.relations.iter().find(|relation| relation.name == name)
    }

    /// Add heap-table metadata and allocate its first heap page.
    pub fn create_table_metadata(
        &mut self,
        name: impl Into<String>,
        columns: Vec<ColumnDef>,
    ) -> DbResult<(RelationId, PageId)> {
        let name = name.into();
        validate_name("relation", &name)?;
        validate_columns(&columns)?;
        if self.relation_by_name(&name).is_some() {
            return Err(DbError::User(format!("relation already exists: {name}")));
        }

        let relation_id = self.allocate_relation_id()?;
        let page_id = self.allocate_page_id()?;
        self.relations.push(RelationInfo {
            id: relation_id,
            name,
            kind: RelationKind::Table,
            storage: StorageObject::Heap(HeapStorage::new(vec![page_id])),
            columns,
        });
        Ok((relation_id, page_id))
    }

    fn allocate_relation_id(&mut self) -> DbResult<RelationId> {
        let relation_id = self.next_relation_id;
        self.next_relation_id = self
            .next_relation_id
            .checked_add(1)
            .ok_or(DbError::User("relation id overflow".to_string()))?;
        Ok(RelationId(relation_id))
    }

    /// Allocate a fresh page id from catalog metadata.
    pub fn allocate_page_id(&mut self) -> DbResult<PageId> {
        let page_id = self.next_page_id;
        self.next_page_id = self
            .next_page_id
            .checked_add(1)
            .ok_or(DbError::User("page id overflow".to_string()))?;
        Ok(PageId(page_id))
    }

    /// Append a heap page to an existing heap-table storage object.
    pub fn append_heap_page(&mut self, relation_id: RelationId, page_id: PageId) -> DbResult<()> {
        let relation = self
            .relations
            .iter_mut()
            .find(|relation| relation.id == relation_id)
            .ok_or(DbError::User(format!(
                "unknown relation id: {}",
                relation_id.0
            )))?;
        if relation.kind != RelationKind::Table {
            return Err(DbError::User(format!(
                "relation is not a heap table: {}",
                relation_id.0
            )));
        }
        relation
            .storage
            .heap_storage_mut()
            .ok_or(DbError::InternalInvariant("table relation without heap storage"))?
            .push_page(page_id);
        Ok(())
    }

    /// Add B+Tree index metadata and allocate its initial root page.
    pub fn create_index_metadata(
        &mut self,
        name: impl Into<String>,
        table_id: RelationId,
        column_name: impl Into<String>,
    ) -> DbResult<(RelationId, PageId)> {
        let name = name.into();
        let column_name = column_name.into();
        validate_name("index", &name)?;
        validate_name("column", &column_name)?;

        if self.relation_by_name(&name).is_some() {
            return Err(DbError::User(format!("relation already exists: {name}")));
        }

        let table = self.relation_by_id(table_id).ok_or(DbError::User(format!(
            "unknown table relation id: {}",
            table_id.0
        )))?;
        if !table.is_heap_table() {
            return Err(DbError::User(format!(
                "indexed relation is not a heap table: {}",
                table_id.0
            )));
        }
        if table.columns.iter().all(|column| column.name != column_name) {
            return Err(DbError::User(format!(
                "unknown indexed column '{}' on relation '{}'",
                column_name, table.name
            )));
        }
        if self
            .indexes_on_table(table_id)
            .iter()
            .any(|relation| relation.index_storage().is_some_and(|storage| storage.column_name() == column_name.as_str()))
        {
            return Err(DbError::User(format!(
                "index already exists on relation {} column {}",
                table_id.0, column_name
            )));
        }

        let relation_id = self.allocate_relation_id()?;
        let root_page_id = self.allocate_page_id()?;
        self.relations.push(RelationInfo {
            id: relation_id,
            name,
            kind: RelationKind::Index,
            storage: StorageObject::BPlusTree(IndexStorage::new(
                table_id,
                column_name.clone(),
                root_page_id,
            )),
            columns: vec![ColumnDef::new(column_name, "INDEX_KEY")],
        });
        Ok((relation_id, root_page_id))
    }

    /// Return indexes that belong to a heap table.
    pub fn indexes_on_table(&self, table_id: RelationId) -> Vec<&RelationInfo> {
        self.relations
            .iter()
            .filter(|relation| {
                relation.kind == RelationKind::Index
                    && relation
                        .index_storage()
                        .is_some_and(|storage| storage.table_id() == table_id)
            })
            .collect()
    }

    /// Update an index root page after the B+Tree root splits.
    pub fn set_index_root_page(
        &mut self,
        index_relation_id: RelationId,
        root_page_id: PageId,
    ) -> DbResult<()> {
        let relation = self
            .relations
            .iter_mut()
            .find(|relation| relation.id == index_relation_id)
            .ok_or(DbError::User(format!(
                "unknown index relation id: {}",
                index_relation_id.0
            )))?;
        if relation.kind != RelationKind::Index {
            return Err(DbError::User(format!(
                "relation is not an index: {}",
                index_relation_id.0
            )));
        }
        relation
            .storage
            .index_storage_mut()
            .ok_or(DbError::InternalInvariant("index relation without index storage"))?
            .set_root_page_id(root_page_id);
        Ok(())
    }
}

/// Materialized heap row returned by a full scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeapRow {
    /// Physical row address.
    pub row_id: RowId,
    /// Raw row bytes stored in the heap table.
    pub bytes: Vec<u8>,
}

/// Internal database handle for the catalog and heap-table milestone.
pub struct CatalogStore<F: VfsFile> {
    page_file: PageFile<F>,
    catalog: Catalog,
}

impl<F: VfsFile> CatalogStore<F> {
    /// Open a catalog store from an already opened page file.
    pub fn open(mut page_file: PageFile<F>) -> DbResult<Self> {
        let catalog = Catalog::load_or_bootstrap(&mut page_file)?;
        Ok(Self { page_file, catalog })
    }

    /// Borrow the current in-memory catalog snapshot.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Borrow the page file.
    pub fn page_file(&self) -> &PageFile<F> {
        &self.page_file
    }

    /// Mutably borrow the page file.
    pub fn page_file_mut(&mut self) -> &mut PageFile<F> {
        &mut self.page_file
    }

    /// Replace the committed in-memory catalog snapshot after an external
    /// transaction layer has durably installed its catalog page.
    pub fn replace_catalog(&mut self, catalog: Catalog) {
        self.catalog = catalog;
    }

    /// Create a heap table and persist its catalog entry.
    pub fn create_table(
        &mut self,
        name: impl Into<String>,
        columns: Vec<ColumnDef>,
    ) -> DbResult<RelationId> {
        let (relation_id, first_page_id) = self.catalog.create_table_metadata(name, columns)?;
        let page = Page::new(first_page_id, PageType::Heap);
        self.page_file.write_page(&page)?;
        self.catalog.save(&mut self.page_file)?;
        Ok(relation_id)
    }

    /// Insert raw row bytes into a heap table.
    pub fn insert_row(&mut self, relation_id: RelationId, bytes: &[u8]) -> DbResult<RowId> {
        self.ensure_heap_relation(relation_id)?;
        let page_ids = self.heap_pages(relation_id)?.to_vec();

        for page_id in page_ids {
            let mut page = self.page_file.read_page(page_id)?;
            match page.insert_record(bytes) {
                Ok(slot_id) => {
                    self.page_file.write_page(&page)?;
                    return Ok(RowId { page_id, slot_id });
                }
                Err(DbError::User(_)) => {}
                Err(error) => return Err(error),
            }
        }

        let page_id = self.catalog.allocate_page_id()?;
        let mut page = Page::new(page_id, PageType::Heap);
        let slot_id = page.insert_record(bytes)?;
        self.page_file.write_page(&page)?;
        self.catalog.append_heap_page(relation_id, page_id)?;
        self.catalog.save(&mut self.page_file)?;
        Ok(RowId { page_id, slot_id })
    }

    /// Scan every live row in heap-page order.
    pub fn full_scan(&self, relation_id: RelationId) -> DbResult<Vec<HeapRow>> {
        self.ensure_heap_relation(relation_id)?;
        let mut rows = Vec::new();

        for page_id in self.heap_pages(relation_id)? {
            let page = self.page_file.read_page(*page_id)?;
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

    /// Read one heap row by physical row id after checking relation ownership.
    pub fn read_row(&self, relation_id: RelationId, row_id: RowId) -> DbResult<Option<HeapRow>> {
        self.ensure_heap_relation(relation_id)?;
        if !self.heap_pages(relation_id)?.contains(&row_id.page_id) {
            return Err(DbError::User(format!(
                "row page {} does not belong to relation {}",
                row_id.page_id.0, relation_id.0
            )));
        }

        let page = self.page_file.read_page(row_id.page_id)?;
        let header = page.header()?;
        if header.page_type != PageType::Heap {
            return Err(DbError::Corruption(format!(
                "relation page is not a heap page: {}",
                row_id.page_id.0
            )));
        }

        Ok(page.read_record(row_id.slot_id)?.map(|bytes| HeapRow {
            row_id,
            bytes: bytes.to_vec(),
        }))
    }

    /// Persist current page-file contents through the VFS boundary.
    pub fn sync_data(&mut self) -> DbResult<()> {
        self.page_file.sync_data()
    }

    /// Consume the store and return its page file and catalog snapshot.
    pub fn into_parts(self) -> (PageFile<F>, Catalog) {
        (self.page_file, self.catalog)
    }

    fn ensure_heap_relation(&self, relation_id: RelationId) -> DbResult<()> {
        let relation = self.catalog.relation_by_id(relation_id).ok_or(DbError::User(
            format!("unknown relation id: {}", relation_id.0),
        ))?;
        if relation.kind != RelationKind::Table {
            return Err(DbError::User(format!(
                "relation is not a heap table: {}",
                relation_id.0
            )));
        }
        Ok(())
    }

    fn heap_pages(&self, relation_id: RelationId) -> DbResult<&[PageId]> {
        let relation = self.catalog.relation_by_id(relation_id).ok_or(DbError::User(
            format!("unknown relation id: {}", relation_id.0),
        ))?;
        Ok(relation.heap_pages())
    }
}

/// Open a catalog store through an arbitrary VFS implementation.
pub fn open_catalog_store<V>(vfs: &V, path: impl AsRef<Path>) -> DbResult<CatalogStore<V::File>>
where
    V: Vfs,
{
    let page_file = open_page_file(vfs, path)?;
    CatalogStore::open(page_file)
}

fn validate_name(kind: &str, name: &str) -> DbResult<()> {
    if name.is_empty() {
        return Err(DbError::User(format!("{kind} name must not be empty")));
    }
    if name.len() > u16::MAX as usize {
        return Err(DbError::User(format!("{kind} name is too long")));
    }
    Ok(())
}

fn validate_columns(columns: &[ColumnDef]) -> DbResult<()> {
    if columns.len() > u16::MAX as usize {
        return Err(DbError::User("too many columns".to_string()));
    }

    for (index, column) in columns.iter().enumerate() {
        validate_name("column", &column.name)?;
        validate_name("type", &column.type_name)?;
        if columns[..index]
            .iter()
            .any(|previous| previous.name == column.name)
        {
            return Err(DbError::User(format!(
                "duplicate column name: {}",
                column.name
            )));
        }
    }

    Ok(())
}

fn encode_catalog(catalog: &Catalog) -> DbResult<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(CATALOG_MAGIC);
    write_u16(&mut bytes, CATALOG_VERSION);
    write_u64(&mut bytes, catalog.next_relation_id);
    write_u64(&mut bytes, catalog.next_page_id);
    write_u32_len(&mut bytes, catalog.relations.len(), "relation count")?;

    for relation in &catalog.relations {
        write_u64(&mut bytes, relation.id.0);
        bytes.push(relation.kind.to_u8());
        encode_storage_object(&mut bytes, &relation.storage)?;
        write_string(&mut bytes, &relation.name)?;
        write_u16_len(&mut bytes, relation.columns.len(), "column count")?;
        for column in &relation.columns {
            write_string(&mut bytes, &column.name)?;
            write_string(&mut bytes, &column.type_name)?;
        }
    }

    write_u32_len(&mut bytes, catalog.extensions.len(), "extension count")?;
    for extension in &catalog.extensions {
        write_string(&mut bytes, &extension.name)?;
        write_u32(&mut bytes, extension.abi_version);
        write_string(&mut bytes, &extension.kind)?;
    }

    Ok(bytes)
}

fn encode_storage_object(bytes: &mut Vec<u8>, storage: &StorageObject) -> DbResult<()> {
    match storage {
        StorageObject::Heap(heap) => {
            bytes.push(STORAGE_HEAP);
            write_u32_len(bytes, heap.pages.len(), "heap page count")?;
            for page_id in &heap.pages {
                write_u64(bytes, page_id.0);
            }
        }
        StorageObject::BPlusTree(index) => {
            bytes.push(STORAGE_INDEX);
            write_u64(bytes, index.table_id().0);
            write_u64(bytes, index.root_page_id().0);
            write_string(bytes, index.column_name())?;
        }
    }
    Ok(())
}

fn decode_catalog(bytes: &[u8]) -> DbResult<Catalog> {
    let mut cursor = DecodeCursor::new(bytes);
    let magic = cursor.read_bytes(4)?;
    if magic != &CATALOG_MAGIC[..] {
        return Err(DbError::Corruption("invalid catalog magic".to_string()));
    }

    let version = cursor.read_u16()?;
    if version != CATALOG_VERSION {
        return Err(DbError::Corruption(format!(
            "unsupported catalog version: {version}"
        )));
    }

    let next_relation_id = cursor.read_u64()?;
    let next_page_id = cursor.read_u64()?;
    let relation_count = cursor.read_u32()?;
    let relation_count = usize::try_from(relation_count).map_err(|_| {
        DbError::Corruption("catalog relation count is too large".to_string())
    })?;
    let mut relations = Vec::with_capacity(relation_count);

    for _ in 0..relation_count {
        let id = RelationId(cursor.read_u64()?);
        let kind = RelationKind::from_u8(cursor.read_u8()?)?;
        let storage = decode_storage_object(&mut cursor)?;
        let name = cursor.read_string()?;
        let column_count = usize::from(cursor.read_u16()?);
        let mut columns = Vec::with_capacity(column_count);
        for _ in 0..column_count {
            columns.push(ColumnDef {
                name: cursor.read_string()?,
                type_name: cursor.read_string()?,
            });
        }
        relations.push(RelationInfo {
            id,
            name,
            kind,
            storage,
            columns,
        });
    }

    let extensions = if cursor.is_finished() {
        Vec::new()
    } else {
        let extension_count = cursor.read_u32()?;
        let extension_count = usize::try_from(extension_count).map_err(|_| {
            DbError::Corruption("catalog extension count is too large".to_string())
        })?;
        let mut extensions = Vec::with_capacity(extension_count);
        for _ in 0..extension_count {
            let name = cursor.read_string()?;
            let abi_version = cursor.read_u32()?;
            let kind = cursor.read_string()?;
            extensions.push(ExtensionInfo::new(name, abi_version, kind));
        }
        extensions
    };

    cursor.finish()?;
    Ok(Catalog {
        next_relation_id,
        next_page_id,
        relations,
        extensions,
    })
}

fn decode_storage_object(cursor: &mut DecodeCursor<'_>) -> DbResult<StorageObject> {
    let storage_kind = cursor.read_u8()?;
    match storage_kind {
        STORAGE_HEAP => {
            let page_count = cursor.read_u32()?;
            let page_count = usize::try_from(page_count).map_err(|_| {
                DbError::Corruption("heap page count is too large".to_string())
            })?;
            let mut pages = Vec::with_capacity(page_count);
            for _ in 0..page_count {
                pages.push(PageId(cursor.read_u64()?));
            }
            if pages.is_empty() {
                return Err(DbError::Corruption(
                    "heap storage has no pages".to_string(),
                ));
            }
            Ok(StorageObject::Heap(HeapStorage::new(pages)))
        }
        STORAGE_INDEX => {
            let table_id = RelationId(cursor.read_u64()?);
            let root_page_id = PageId(cursor.read_u64()?);
            let column_name = cursor.read_string()?;
            Ok(StorageObject::BPlusTree(IndexStorage::new(
                table_id,
                column_name,
                root_page_id,
            )))
        }
        _ => Err(DbError::Corruption(format!(
            "unknown catalog storage object kind: {storage_kind}"
        ))),
    }
}

fn write_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u16_len(bytes: &mut Vec<u8>, value: usize, field: &str) -> DbResult<()> {
    let value = u16::try_from(value)
        .map_err(|_| DbError::User(format!("{field} does not fit into u16")))?;
    write_u16(bytes, value);
    Ok(())
}


fn write_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u32_len(bytes: &mut Vec<u8>, value: usize, field: &str) -> DbResult<()> {
    let value = u32::try_from(value)
        .map_err(|_| DbError::User(format!("{field} does not fit into u32")))?;
    bytes.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_string(bytes: &mut Vec<u8>, value: &str) -> DbResult<()> {
    write_u16_len(bytes, value.len(), "string length")?;
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

struct DecodeCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> DecodeCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_bytes(&mut self, len: usize) -> DbResult<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(DbError::Corruption("catalog offset overflow".to_string()))?;
        if end > self.bytes.len() {
            return Err(DbError::Corruption(
                "catalog record is truncated".to_string(),
            ));
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> DbResult<u8> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u16(&mut self) -> DbResult<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> DbResult<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_u64(&mut self) -> DbResult<u64> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_string(&mut self) -> DbResult<String> {
        let len = usize::from(self.read_u16()?);
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| {
            DbError::Corruption("catalog string is not valid UTF-8".to_string())
        })
    }

    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn finish(&self) -> DbResult<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(DbError::Corruption(
                "catalog record has trailing bytes".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbms_vfs::StdVfs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn bootstraps_catalog_and_reopens_schema() -> DbResult<()> {
        let path = temp_db_path("reopen_schema");
        let relation_id;

        {
            let vfs = StdVfs::new();
            let mut store = open_catalog_store(&vfs, &path)?;
            relation_id = store.create_table(
                "users",
                vec![
                    ColumnDef::new("id", "int"),
                    ColumnDef::new("name", "text"),
                ],
            )?;
            store.sync_data()?;
        }

        {
            let vfs = StdVfs::new();
            let store = open_catalog_store(&vfs, &path)?;
            let relation = store
                .catalog()
                .relation_by_name("users")
                .ok_or(DbError::InternalInvariant("users relation was not reopened"))?;

            assert_eq!(relation.id, relation_id);
            assert_eq!(relation.kind, RelationKind::Table);
            assert_eq!(relation.heap_pages(), &[PageId(1)]);
            assert_eq!(relation.columns[0], ColumnDef::new("id", "int"));
            assert_eq!(relation.columns[1], ColumnDef::new("name", "text"));
        }

        cleanup_temp_file(path);
        Ok(())
    }

    #[test]
    fn inserts_row_bytes_and_full_scans_heap_table() -> DbResult<()> {
        let path = temp_db_path("heap_scan");
        let vfs = StdVfs::new();
        let mut store = open_catalog_store(&vfs, &path)?;
        let relation_id = store.create_table("events", vec![ColumnDef::new("payload", "bytes")])?;

        let first = store.insert_row(relation_id, b"first row")?;
        let second = store.insert_row(relation_id, b"second row")?;
        let rows = store.full_scan(relation_id)?;

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].row_id, first);
        assert_eq!(rows[0].bytes, b"first row".to_vec());
        assert_eq!(rows[1].row_id, second);
        assert_eq!(rows[1].bytes, b"second row".to_vec());

        cleanup_temp_file(path);
        Ok(())
    }

    #[test]
    fn heap_insert_allocates_additional_pages() -> DbResult<()> {
        let path = temp_db_path("heap_extend");
        let vfs = StdVfs::new();
        let mut store = open_catalog_store(&vfs, &path)?;
        let relation_id = store.create_table("large_rows", vec![ColumnDef::new("payload", "bytes")])?;
        let payload = vec![0x7B; 900];

        for _ in 0..8 {
            store.insert_row(relation_id, &payload)?;
        }

        let rows = store.full_scan(relation_id)?;
        let relation = store
            .catalog()
            .relation_by_id(relation_id)
            .ok_or(DbError::InternalInvariant("relation disappeared"))?;
        assert_eq!(rows.len(), 8);
        assert!(relation.heap_pages().len() > 1);

        cleanup_temp_file(path);
        Ok(())
    }

    #[test]
    fn creates_index_metadata() -> DbResult<()> {
        let path = temp_db_path("index_metadata");
        let vfs = StdVfs::new();
        let mut store = open_catalog_store(&vfs, &path)?;
        let table_id = store.create_table("users", vec![ColumnDef::new("id", "INT")])?;
        let (index_id, root_page_id) = store
            .catalog
            .create_index_metadata("users_id_idx", table_id, "id")?;

        assert!(root_page_id.0 > 0);
        let index = store
            .catalog
            .relation_by_id(index_id)
            .ok_or(DbError::InternalInvariant("index metadata disappeared"))?;
        let storage = index
            .index_storage()
            .ok_or(DbError::InternalInvariant("missing index storage"))?;
        assert_eq!(storage.table_id(), table_id);
        assert_eq!(storage.column_name(), "id");
        assert_eq!(storage.root_page_id(), root_page_id);

        cleanup_temp_file(path);
        Ok(())
    }

    #[test]
    fn persists_extension_metadata() -> DbResult<()> {
        let path = temp_db_path("extension_metadata");
        let vfs = StdVfs::new();

        {
            let mut store = open_catalog_store(&vfs, &path)?;
            assert!(store
                .catalog
                .register_extension_metadata("stdlib", 1, "static")?);
            store.catalog.save(&mut store.page_file)?;
            store.sync_data()?;
        }

        {
            let store = open_catalog_store(&vfs, &path)?;
            let extension = store
                .catalog()
                .extension_by_name("stdlib")
                .ok_or(DbError::InternalInvariant("extension metadata was not reopened"))?;
            assert_eq!(extension.abi_version, 1);
            assert_eq!(extension.kind, "static");
        }

        cleanup_temp_file(path);
        Ok(())
    }

    #[test]
    fn rejects_duplicate_relation_names() -> DbResult<()> {
        let path = temp_db_path("duplicate_relation");
        let vfs = StdVfs::new();
        let mut store = open_catalog_store(&vfs, &path)?;
        store.create_table("items", vec![ColumnDef::new("payload", "bytes")])?;
        let error = store
            .create_table("items", vec![ColumnDef::new("payload", "bytes")])
            .err()
            .ok_or(DbError::InternalInvariant("duplicate relation was accepted"))?;
        assert!(matches!(error, DbError::User(_)));

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
            "rdbms_catalog_{test_name}_{}_{}.dbonrs",
            std::process::id(),
            nanos
        ));
        path
    }

    fn cleanup_temp_file(path: PathBuf) {
        let _ = std::fs::remove_file(path);
    }
}
