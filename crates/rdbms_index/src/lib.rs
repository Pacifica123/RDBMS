//! Persistent B+Tree index v0.
//!
//! Stage 8 keeps the index deliberately small. Nodes are stored as ordinary
//! `PageType::Index` slotted pages with one encoded node record in slot 0. The
//! tree supports equality lookup and append-time insert of `(key, RowId)` pairs.
//! There is no delete, no uniqueness enforcement and no MVCC visibility logic.

use rdbms_core::{DbError, DbResult, PageId, RowId, SlotId};
use rdbms_page::{Page, PageType};
use std::cmp::Ordering;

/// Maximum separator keys per B+Tree node in Stage 8.
///
/// The value is intentionally small so unit tests exercise splits quickly.
pub const MAX_KEYS: usize = 4;

const INDEX_NODE_MAGIC: &[u8; 4] = b"RDBI";
const INDEX_NODE_VERSION: u16 = 1;
const NODE_LEAF: u8 = 1;
const NODE_INTERNAL: u8 = 2;
const KEY_INTEGER: u8 = 1;
const KEY_TEXT: u8 = 2;

/// Ordered key supported by the Stage 8 index.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum IndexKey {
    /// Signed integer key.
    Integer(i64),
    /// UTF-8 text key.
    Text(String),
}

/// One leaf entry: key plus physical row address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexEntry {
    /// Indexed key.
    pub key: IndexKey,
    /// Heap row referenced by this key.
    pub row_id: RowId,
}

/// Storage boundary used by the B+Tree algorithm.
///
/// `rdbms_index` does not own a file handle. The transaction layer supplies an
/// implementation backed by dirty-page staging. Read-only lookups use another
/// implementation backed by committed pages.
pub trait BPlusTreePageStore {
    /// Load a page by id.
    fn load_page(&mut self, page_id: PageId) -> DbResult<Page>;

    /// Store a complete page image.
    fn store_page(&mut self, page: Page) -> DbResult<()>;

    /// Allocate a fresh page id for a split.
    fn allocate_page(&mut self) -> DbResult<PageId>;
}

/// Create an empty root leaf page.
pub fn initialize_root<S: BPlusTreePageStore>(
    store: &mut S,
    root_page_id: PageId,
) -> DbResult<()> {
    store.store_page(node_to_page(root_page_id, &BTreeNode::Leaf {
        next_leaf: None,
        entries: Vec::new(),
    })?)
}

/// Insert one `(key, RowId)` pair and return the current root page id.
///
/// The root page id changes only when the previous root splits.
pub fn insert<S: BPlusTreePageStore>(
    store: &mut S,
    root_page_id: PageId,
    key: IndexKey,
    row_id: RowId,
) -> DbResult<PageId> {
    match insert_recursive(store, root_page_id, key, row_id)? {
        Some(split) => {
            let new_root_page_id = store.allocate_page()?;
            let new_root = BTreeNode::Internal {
                keys: vec![split.separator],
                children: vec![root_page_id, split.right_page_id],
            };
            store.store_page(node_to_page(new_root_page_id, &new_root)?)?;
            Ok(new_root_page_id)
        }
        None => Ok(root_page_id),
    }
}

/// Look up all row ids equal to `key`.
pub fn lookup<S: BPlusTreePageStore>(
    store: &mut S,
    root_page_id: PageId,
    key: &IndexKey,
) -> DbResult<Vec<RowId>> {
    let mut leaf_page_id = find_leaf(store, root_page_id, key)?;
    let mut row_ids = Vec::new();

    loop {
        let page = store.load_page(leaf_page_id)?;
        let node = node_from_page(&page)?;
        let BTreeNode::Leaf { next_leaf, entries } = node else {
            return Err(DbError::Corruption(
                "B+Tree lookup reached a non-leaf page".to_string(),
            ));
        };

        for entry in &entries {
            match entry.key.cmp(key) {
                Ordering::Less => {}
                Ordering::Equal => row_ids.push(entry.row_id),
                Ordering::Greater => return Ok(row_ids),
            }
        }

        match next_leaf {
            Some(next_page_id) => leaf_page_id = next_page_id,
            None => return Ok(row_ids),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BTreeNode {
    Leaf {
        next_leaf: Option<PageId>,
        entries: Vec<IndexEntry>,
    },
    Internal {
        keys: Vec<IndexKey>,
        children: Vec<PageId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SplitResult {
    separator: IndexKey,
    right_page_id: PageId,
}

fn insert_recursive<S: BPlusTreePageStore>(
    store: &mut S,
    page_id: PageId,
    key: IndexKey,
    row_id: RowId,
) -> DbResult<Option<SplitResult>> {
    let page = store.load_page(page_id)?;
    match node_from_page(&page)? {
        BTreeNode::Leaf {
            next_leaf,
            mut entries,
        } => {
            insert_leaf_entry(&mut entries, key, row_id);
            if entries.len() <= MAX_KEYS {
                store.store_page(node_to_page(page_id, &BTreeNode::Leaf { next_leaf, entries })?)?;
                return Ok(None);
            }

            let right_page_id = store.allocate_page()?;
            let split_at = entries.len() / 2;
            let right_entries = entries.split_off(split_at);
            let separator = right_entries[0].key.clone();
            let left = BTreeNode::Leaf {
                next_leaf: Some(right_page_id),
                entries,
            };
            let right = BTreeNode::Leaf {
                next_leaf,
                entries: right_entries,
            };
            store.store_page(node_to_page(page_id, &left)?)?;
            store.store_page(node_to_page(right_page_id, &right)?)?;
            Ok(Some(SplitResult {
                separator,
                right_page_id,
            }))
        }
        BTreeNode::Internal {
            mut keys,
            mut children,
        } => {
            validate_internal_shape(&keys, &children)?;
            let child_index = child_index_for_key(&keys, &key);
            let child_page_id = children[child_index];

            if let Some(split) = insert_recursive(store, child_page_id, key, row_id)? {
                keys.insert(child_index, split.separator);
                children.insert(child_index + 1, split.right_page_id);

                if keys.len() <= MAX_KEYS {
                    store.store_page(node_to_page(page_id, &BTreeNode::Internal { keys, children })?)?;
                    return Ok(None);
                }

                let promoted_index = keys.len() / 2;
                let separator = keys[promoted_index].clone();
                let right_keys = keys.split_off(promoted_index + 1);
                let _promoted = keys.pop();
                let right_children = children.split_off(promoted_index + 1);
                let right_page_id = store.allocate_page()?;

                store.store_page(node_to_page(
                    page_id,
                    &BTreeNode::Internal { keys, children },
                )?)?;
                store.store_page(node_to_page(
                    right_page_id,
                    &BTreeNode::Internal {
                        keys: right_keys,
                        children: right_children,
                    },
                )?)?;

                Ok(Some(SplitResult {
                    separator,
                    right_page_id,
                }))
            } else {
                Ok(None)
            }
        }
    }
}

fn find_leaf<S: BPlusTreePageStore>(
    store: &mut S,
    root_page_id: PageId,
    key: &IndexKey,
) -> DbResult<PageId> {
    let mut page_id = root_page_id;
    loop {
        let page = store.load_page(page_id)?;
        match node_from_page(&page)? {
            BTreeNode::Leaf { .. } => return Ok(page_id),
            BTreeNode::Internal { keys, children } => {
                validate_internal_shape(&keys, &children)?;
                let child_index = child_index_for_key(&keys, key);
                page_id = children[child_index];
            }
        }
    }
}

fn insert_leaf_entry(entries: &mut Vec<IndexEntry>, key: IndexKey, row_id: RowId) {
    let entry = IndexEntry { key, row_id };
    let position = entries
        .binary_search_by(|candidate| compare_entry(candidate, &entry))
        .unwrap_or_else(|position| position);
    entries.insert(position, entry);
}

fn compare_entry(left: &IndexEntry, right: &IndexEntry) -> Ordering {
    left.key.cmp(&right.key).then(left.row_id.cmp(&right.row_id))
}

fn child_index_for_key(keys: &[IndexKey], key: &IndexKey) -> usize {
    keys.partition_point(|separator| key > separator)
}

fn validate_internal_shape(keys: &[IndexKey], children: &[PageId]) -> DbResult<()> {
    if children.len() != keys.len() + 1 {
        return Err(DbError::Corruption(format!(
            "invalid B+Tree internal node shape: keys={}, children={}",
            keys.len(),
            children.len()
        )));
    }
    Ok(())
}

fn node_to_page(page_id: PageId, node: &BTreeNode) -> DbResult<Page> {
    let mut page = Page::new(page_id, PageType::Index);
    let bytes = encode_node(node)?;
    page.insert_record(&bytes)?;
    Ok(page)
}

fn node_from_page(page: &Page) -> DbResult<BTreeNode> {
    let header = page.header()?;
    if header.page_type != PageType::Index {
        return Err(DbError::Corruption(format!(
            "page {} is not an index page",
            header.page_id.0
        )));
    }
    let bytes = page
        .read_record(SlotId(0))?
        .ok_or(DbError::Corruption("index node record is missing".to_string()))?;
    decode_node(bytes)
}

fn encode_node(node: &BTreeNode) -> DbResult<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(INDEX_NODE_MAGIC);
    write_u16(&mut bytes, INDEX_NODE_VERSION);

    match node {
        BTreeNode::Leaf { next_leaf, entries } => {
            bytes.push(NODE_LEAF);
            match next_leaf {
                Some(page_id) => {
                    bytes.push(1);
                    write_u64(&mut bytes, page_id.0);
                }
                None => {
                    bytes.push(0);
                    write_u64(&mut bytes, 0);
                }
            }
            write_u16_len(&mut bytes, entries.len(), "leaf entry count")?;
            for entry in entries {
                encode_key(&mut bytes, &entry.key)?;
                write_u64(&mut bytes, entry.row_id.page_id.0);
                write_u16(&mut bytes, entry.row_id.slot_id.0);
            }
        }
        BTreeNode::Internal { keys, children } => {
            validate_internal_shape(keys, children)?;
            bytes.push(NODE_INTERNAL);
            write_u16_len(&mut bytes, keys.len(), "internal key count")?;
            for key in keys {
                encode_key(&mut bytes, key)?;
            }
            write_u16_len(&mut bytes, children.len(), "internal child count")?;
            for child in children {
                write_u64(&mut bytes, child.0);
            }
        }
    }

    Ok(bytes)
}

fn decode_node(bytes: &[u8]) -> DbResult<BTreeNode> {
    let mut cursor = DecodeCursor::new(bytes);
    let magic = cursor.read_bytes(4)?;
    if magic != &INDEX_NODE_MAGIC[..] {
        return Err(DbError::Corruption("invalid index node magic".to_string()));
    }
    let version = cursor.read_u16()?;
    if version != INDEX_NODE_VERSION {
        return Err(DbError::Corruption(format!(
            "unsupported index node version: {version}"
        )));
    }

    let kind = cursor.read_u8()?;
    let node = match kind {
        NODE_LEAF => {
            let has_next = cursor.read_u8()?;
            let next_page_id = cursor.read_u64()?;
            let next_leaf = match has_next {
                0 => None,
                1 => Some(PageId(next_page_id)),
                _ => {
                    return Err(DbError::Corruption(format!(
                        "invalid leaf next flag: {has_next}"
                    )));
                }
            };
            let entry_count = usize::from(cursor.read_u16()?);
            let mut entries = Vec::with_capacity(entry_count);
            for _ in 0..entry_count {
                let key = decode_key(&mut cursor)?;
                let page_id = PageId(cursor.read_u64()?);
                let slot_id = SlotId(cursor.read_u16()?);
                entries.push(IndexEntry {
                    key,
                    row_id: RowId { page_id, slot_id },
                });
            }
            BTreeNode::Leaf { next_leaf, entries }
        }
        NODE_INTERNAL => {
            let key_count = usize::from(cursor.read_u16()?);
            let mut keys = Vec::with_capacity(key_count);
            for _ in 0..key_count {
                keys.push(decode_key(&mut cursor)?);
            }
            let child_count = usize::from(cursor.read_u16()?);
            let mut children = Vec::with_capacity(child_count);
            for _ in 0..child_count {
                children.push(PageId(cursor.read_u64()?));
            }
            validate_internal_shape(&keys, &children)?;
            BTreeNode::Internal { keys, children }
        }
        _ => {
            return Err(DbError::Corruption(format!(
                "unknown index node kind: {kind}"
            )));
        }
    };

    cursor.finish()?;
    Ok(node)
}

fn encode_key(bytes: &mut Vec<u8>, key: &IndexKey) -> DbResult<()> {
    match key {
        IndexKey::Integer(value) => {
            bytes.push(KEY_INTEGER);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        IndexKey::Text(value) => {
            bytes.push(KEY_TEXT);
            write_u16_len(bytes, value.len(), "index text key length")?;
            bytes.extend_from_slice(value.as_bytes());
        }
    }
    Ok(())
}

fn decode_key(cursor: &mut DecodeCursor<'_>) -> DbResult<IndexKey> {
    match cursor.read_u8()? {
        KEY_INTEGER => Ok(IndexKey::Integer(cursor.read_i64()?)),
        KEY_TEXT => {
            let len = usize::from(cursor.read_u16()?);
            let bytes = cursor.read_bytes(len)?;
            let text = String::from_utf8(bytes.to_vec()).map_err(|_| {
                DbError::Corruption("index text key is not valid UTF-8".to_string())
            })?;
            Ok(IndexKey::Text(text))
        }
        tag => Err(DbError::Corruption(format!(
            "unknown index key tag: {tag}"
        ))),
    }
}

fn write_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u16_len(bytes: &mut Vec<u8>, value: usize, field: &str) -> DbResult<()> {
    let value = u16::try_from(value)
        .map_err(|_| DbError::User(format!("{field} does not fit into u16")))?;
    write_u16(bytes, value);
    Ok(())
}

fn write_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
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
            .ok_or(DbError::Corruption("index node offset overflow".to_string()))?;
        if end > self.bytes.len() {
            return Err(DbError::Corruption("index node is truncated".to_string()));
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

    fn read_u64(&mut self) -> DbResult<u64> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_i64(&mut self) -> DbResult<i64> {
        let bytes = self.read_bytes(8)?;
        Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn finish(&self) -> DbResult<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(DbError::Corruption(
                "index node has trailing bytes".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct MemoryStore {
        pages: BTreeMap<PageId, Page>,
        next_page_id: u64,
    }

    impl MemoryStore {
        fn new() -> Self {
            Self {
                pages: BTreeMap::new(),
                next_page_id: 2,
            }
        }
    }

    impl BPlusTreePageStore for MemoryStore {
        fn load_page(&mut self, page_id: PageId) -> DbResult<Page> {
            self.pages
                .get(&page_id)
                .cloned()
                .ok_or(DbError::InternalInvariant("missing memory page"))
        }

        fn store_page(&mut self, page: Page) -> DbResult<()> {
            let page_id = page.header()?.page_id;
            self.pages.insert(page_id, page);
            Ok(())
        }

        fn allocate_page(&mut self) -> DbResult<PageId> {
            let page_id = PageId(self.next_page_id);
            self.next_page_id += 1;
            Ok(page_id)
        }
    }

    #[test]
    fn inserts_splits_and_finds_integer_keys() -> DbResult<()> {
        let mut store = MemoryStore::new();
        let mut root = PageId(1);
        initialize_root(&mut store, root)?;

        for value in 0..24_i64 {
            root = insert(
                &mut store,
                root,
                IndexKey::Integer(value % 6),
                RowId {
                    page_id: PageId(100 + value as u64),
                    slot_id: SlotId(value as u16),
                },
            )?;
        }

        let matches = lookup(&mut store, root, &IndexKey::Integer(3))?;
        assert_eq!(matches.len(), 4);
        assert!(matches.iter().all(|row_id| row_id.slot_id.0 % 6 == 3));
        Ok(())
    }

    #[test]
    fn supports_text_keys() -> DbResult<()> {
        let mut store = MemoryStore::new();
        let mut root = PageId(1);
        initialize_root(&mut store, root)?;

        for (index, name) in ["ada", "linus", "ada", "grace", "ada"].iter().enumerate() {
            root = insert(
                &mut store,
                root,
                IndexKey::Text((*name).to_string()),
                RowId {
                    page_id: PageId(10),
                    slot_id: SlotId(index as u16),
                },
            )?;
        }

        let matches = lookup(&mut store, root, &IndexKey::Text("ada".to_string()))?;
        assert_eq!(matches.len(), 3);
        Ok(())
    }
}
