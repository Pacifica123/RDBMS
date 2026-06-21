//! Write-ahead log skeleton.
//!
//! This crate owns the first binary WAL envelope and a minimal append/scan
//! path. It deliberately stops before full recovery: Stage 3 can record page
//! images and expose a redo hook, while Stage 4 will decide when and how to
//! apply committed records during database open.

use rdbms_core::{DbError, DbResult, Lsn, PageId, TxId};
use rdbms_page::{Page, PAGE_SIZE};
use rdbms_vfs::VfsFile;
use std::collections::HashSet;

/// WAL file magic for the v0 record stream: "RDBW".
pub const WAL_MAGIC: u32 = 0x5244_4257;
/// WAL record format version.
pub const WAL_VERSION: u16 = 1;
/// Fixed WAL record header size.
pub const WAL_HEADER_SIZE: usize = 40;

const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_KIND: usize = 6;
const OFF_LSN: usize = 8;
const OFF_TX_ID: usize = 16;
const OFF_PAGE_ID: usize = 24;
const OFF_PAYLOAD_LEN: usize = 32;
const OFF_CHECKSUM: usize = 36;

const KIND_BEGIN_TX: u16 = 1;
const KIND_PAGE_IMAGE: u16 = 2;
const KIND_COMMIT_TX: u16 = 3;
const KIND_ABORT_TX: u16 = 4;
const KIND_CHECKPOINT: u16 = 5;
const ABSENT_ID: u64 = u64::MAX;

/// Minimal WAL record kinds for the first recovery milestones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WalRecordKind {
    /// Transaction started.
    BeginTx {
        /// Transaction id.
        tx_id: TxId,
    },
    /// A full page image was written for simple redo.
    PageImage {
        /// Transaction id that produced the image.
        tx_id: TxId,
        /// Page affected by the image.
        page_id: PageId,
        /// Complete serialized page bytes.
        image: Box<[u8; PAGE_SIZE]>,
    },
    /// Transaction committed.
    CommitTx {
        /// Transaction id.
        tx_id: TxId,
    },
    /// Transaction aborted.
    AbortTx {
        /// Transaction id.
        tx_id: TxId,
    },
    /// Recovery checkpoint marker.
    Checkpoint,
}

impl WalRecordKind {
    /// Create a page-image WAL record kind from a validated page.
    pub fn page_image(tx_id: TxId, page: &Page) -> DbResult<Self> {
        page.validate()?;
        let header = page.header()?;
        Ok(Self::PageImage {
            tx_id,
            page_id: header.page_id,
            image: Box::new(*page.as_bytes()),
        })
    }

    fn metadata(&self) -> (u16, u64, u64) {
        match self {
            Self::BeginTx { tx_id } => (KIND_BEGIN_TX, tx_id.0, ABSENT_ID),
            Self::PageImage {
                tx_id, page_id, ..
            } => (KIND_PAGE_IMAGE, tx_id.0, page_id.0),
            Self::CommitTx { tx_id } => (KIND_COMMIT_TX, tx_id.0, ABSENT_ID),
            Self::AbortTx { tx_id } => (KIND_ABORT_TX, tx_id.0, ABSENT_ID),
            Self::Checkpoint => (KIND_CHECKPOINT, ABSENT_ID, ABSENT_ID),
        }
    }

    fn payload(&self) -> Vec<u8> {
        match self {
            Self::PageImage { image, .. } => image.as_ref().to_vec(),
            Self::BeginTx { .. }
            | Self::CommitTx { .. }
            | Self::AbortTx { .. }
            | Self::Checkpoint => Vec::new(),
        }
    }
}

/// WAL record envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalRecord {
    /// Record LSN. In WAL v0 this is the byte offset of the record header.
    pub lsn: Lsn,
    /// Record kind.
    pub kind: WalRecordKind,
}

impl WalRecord {
    /// Encode this record into the binary WAL v0 envelope.
    pub fn encode(&self) -> DbResult<Vec<u8>> {
        encode_record(self)
    }

    /// Decode one complete WAL v0 record from bytes.
    pub fn decode(bytes: &[u8]) -> DbResult<Self> {
        decode_record(bytes)
    }
}

/// Monotonic LSN allocator for append-only WAL records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LsnAllocator {
    next: Lsn,
}

impl LsnAllocator {
    /// Create an allocator whose next LSN is `start`.
    pub fn new(start: Lsn) -> Self {
        Self { next: start }
    }

    /// Return the next LSN without advancing the allocator.
    pub fn next_lsn(&self) -> Lsn {
        self.next
    }

    /// Allocate `encoded_len` bytes and return the record start LSN.
    pub fn allocate(&mut self, encoded_len: u64) -> DbResult<Lsn> {
        let lsn = self.next;
        self.next = Lsn(
            self.next
                .0
                .checked_add(encoded_len)
                .ok_or(DbError::User("wal lsn overflow".to_string()))?,
        );
        Ok(lsn)
    }
}

/// Append-only WAL writer over a VFS file.
pub struct WalWriter<F: VfsFile> {
    file: F,
    allocator: LsnAllocator,
}

impl<F: VfsFile> WalWriter<F> {
    /// Open a writer at the current end of `file`.
    pub fn new(file: F) -> DbResult<Self> {
        let len = file.len()?;
        Ok(Self {
            file,
            allocator: LsnAllocator::new(Lsn(len)),
        })
    }

    /// Return the next record LSN.
    pub fn next_lsn(&self) -> Lsn {
        self.allocator.next_lsn()
    }

    /// Append one WAL record kind, assigning its LSN from the allocator.
    pub fn append(&mut self, kind: WalRecordKind) -> DbResult<Lsn> {
        let encoded_len = encoded_len_for_kind(&kind)?;
        let lsn = self.allocator.allocate(encoded_len)?;
        let record = WalRecord { lsn, kind };
        let bytes = record.encode()?;
        self.file.write_all_at(lsn.0, &bytes)?;
        Ok(lsn)
    }

    /// Force durable WAL bytes through the VFS boundary.
    pub fn sync_data(&mut self) -> DbResult<()> {
        self.file.sync_data()
    }

    /// Return the wrapped VFS file.
    pub fn into_inner(self) -> F {
        self.file
    }
}

/// Sequential WAL reader over a VFS file.
pub struct WalReader<F: VfsFile> {
    file: F,
}

impl<F: VfsFile> WalReader<F> {
    /// Create a reader for a VFS file.
    pub fn new(file: F) -> Self {
        Self { file }
    }

    /// Read and validate every complete record in the WAL file.
    pub fn read_all(&self) -> DbResult<Vec<WalRecord>> {
        let len = self.file.len()?;
        let mut offset = 0_u64;
        let mut records = Vec::new();

        while offset < len {
            let remaining = len - offset;
            if remaining < WAL_HEADER_SIZE as u64 {
                return Err(truncated_wal(offset, remaining));
            }

            let mut header = [0_u8; WAL_HEADER_SIZE];
            self.file.read_exact_at(offset, &mut header)?;
            let payload_len = u64::from(read_u32(&header, OFF_PAYLOAD_LEN));
            let record_len = (WAL_HEADER_SIZE as u64)
                .checked_add(payload_len)
                .ok_or(DbError::Corruption("wal record length overflow".to_string()))?;

            if remaining < record_len {
                return Err(truncated_wal(offset, remaining));
            }

            let record_len_usize = usize::try_from(record_len).map_err(|_| {
                DbError::Corruption("wal record is too large for this platform".to_string())
            })?;
            let mut bytes = vec![0_u8; record_len_usize];
            bytes[..WAL_HEADER_SIZE].copy_from_slice(&header);
            if payload_len > 0 {
                self.file
                    .read_exact_at(offset + WAL_HEADER_SIZE as u64, &mut bytes[WAL_HEADER_SIZE..])?;
            }

            let record = WalRecord::decode(&bytes)?;
            if record.lsn != Lsn(offset) {
                return Err(DbError::Corruption(format!(
                    "wal lsn mismatch: offset={}, record_lsn={}",
                    offset, record.lsn.0
                )));
            }

            records.push(record);
            offset = offset
                .checked_add(record_len)
                .ok_or(DbError::Corruption("wal offset overflow".to_string()))?;
        }

        Ok(records)
    }

    /// Return the wrapped VFS file.
    pub fn into_inner(self) -> F {
        self.file
    }
}

/// Receiver for committed page-image redo records.
pub trait PageImageRedo {
    /// Redo one complete page image.
    fn redo_page_image(
        &mut self,
        lsn: Lsn,
        tx_id: TxId,
        page_id: PageId,
        image: &[u8; PAGE_SIZE],
    ) -> DbResult<()>;
}

/// Replay page-image records whose transaction has a commit marker.
///
/// This is only a hook for Stage 4. It does not open database files, manage
/// checkpoints or implement undo.
pub fn redo_committed_page_images<R>(records: &[WalRecord], redo: &mut R) -> DbResult<()>
where
    R: PageImageRedo,
{
    let mut committed = HashSet::new();
    let mut aborted = HashSet::new();

    for record in records {
        match &record.kind {
            WalRecordKind::CommitTx { tx_id } => {
                committed.insert(*tx_id);
            }
            WalRecordKind::AbortTx { tx_id } => {
                aborted.insert(*tx_id);
            }
            WalRecordKind::BeginTx { .. }
            | WalRecordKind::PageImage { .. }
            | WalRecordKind::Checkpoint => {}
        }
    }

    for record in records {
        if let WalRecordKind::PageImage {
            tx_id,
            page_id,
            image,
        } = &record.kind
        {
            if committed.contains(tx_id) && !aborted.contains(tx_id) {
                let page = Page::from_bytes(*image.as_ref())?;
                let header = page.header()?;
                if header.page_id != *page_id {
                    return Err(DbError::Corruption(format!(
                        "wal page image id mismatch: record={}, image={}",
                        page_id.0, header.page_id.0
                    )));
                }
                redo.redo_page_image(record.lsn, *tx_id, *page_id, image.as_ref())?;
            }
        }
    }

    Ok(())
}

fn encoded_len_for_kind(kind: &WalRecordKind) -> DbResult<u64> {
    let payload_len = u64::try_from(kind.payload().len())
        .map_err(|_| DbError::User("wal payload length overflow".to_string()))?;
    (WAL_HEADER_SIZE as u64)
        .checked_add(payload_len)
        .ok_or(DbError::User("wal record length overflow".to_string()))
}

fn encode_record(record: &WalRecord) -> DbResult<Vec<u8>> {
    let payload = record.kind.payload();
    let payload_len = u32::try_from(payload.len())
        .map_err(|_| DbError::User("wal payload length overflow".to_string()))?;
    let (kind, tx_id, page_id) = record.kind.metadata();
    let mut bytes = vec![0_u8; WAL_HEADER_SIZE + payload.len()];

    write_u32(&mut bytes, OFF_MAGIC, WAL_MAGIC);
    write_u16(&mut bytes, OFF_VERSION, WAL_VERSION);
    write_u16(&mut bytes, OFF_KIND, kind);
    write_u64(&mut bytes, OFF_LSN, record.lsn.0);
    write_u64(&mut bytes, OFF_TX_ID, tx_id);
    write_u64(&mut bytes, OFF_PAGE_ID, page_id);
    write_u32(&mut bytes, OFF_PAYLOAD_LEN, payload_len);
    write_u32(&mut bytes, OFF_CHECKSUM, 0);
    bytes[WAL_HEADER_SIZE..].copy_from_slice(&payload);

    let checksum = wal_checksum(&bytes);
    write_u32(&mut bytes, OFF_CHECKSUM, checksum);
    Ok(bytes)
}

fn decode_record(bytes: &[u8]) -> DbResult<WalRecord> {
    if bytes.len() < WAL_HEADER_SIZE {
        return Err(DbError::Corruption("truncated wal record header".to_string()));
    }

    let magic = read_u32(bytes, OFF_MAGIC);
    if magic != WAL_MAGIC {
        return Err(DbError::Corruption("invalid wal magic".to_string()));
    }

    let version = read_u16(bytes, OFF_VERSION);
    if version != WAL_VERSION {
        return Err(DbError::Corruption(format!(
            "unsupported wal version: {version}"
        )));
    }

    let payload_len = usize::try_from(read_u32(bytes, OFF_PAYLOAD_LEN))
        .map_err(|_| DbError::Corruption("wal payload length overflow".to_string()))?;
    let expected_len = WAL_HEADER_SIZE
        .checked_add(payload_len)
        .ok_or(DbError::Corruption("wal record length overflow".to_string()))?;
    if bytes.len() != expected_len {
        return Err(DbError::Corruption(format!(
            "wal record length mismatch: expected={}, got={}",
            expected_len,
            bytes.len()
        )));
    }

    let stored_checksum = read_u32(bytes, OFF_CHECKSUM);
    let actual_checksum = wal_checksum(bytes);
    if stored_checksum != actual_checksum {
        return Err(DbError::Corruption(format!(
            "wal checksum mismatch: stored={}, actual={}",
            stored_checksum, actual_checksum
        )));
    }

    let lsn = Lsn(read_u64(bytes, OFF_LSN));
    let tx_id = TxId(read_u64(bytes, OFF_TX_ID));
    let page_id = PageId(read_u64(bytes, OFF_PAGE_ID));
    let payload = &bytes[WAL_HEADER_SIZE..];

    let kind = match read_u16(bytes, OFF_KIND) {
        KIND_BEGIN_TX => {
            require_empty_payload(payload, "BeginTx")?;
            WalRecordKind::BeginTx { tx_id }
        }
        KIND_PAGE_IMAGE => {
            if payload.len() != PAGE_SIZE {
                return Err(DbError::Corruption(format!(
                    "invalid page image payload length: {}",
                    payload.len()
                )));
            }
            let mut image = [0_u8; PAGE_SIZE];
            image.copy_from_slice(payload);
            WalRecordKind::PageImage {
                tx_id,
                page_id,
                image: Box::new(image),
            }
        }
        KIND_COMMIT_TX => {
            require_empty_payload(payload, "CommitTx")?;
            WalRecordKind::CommitTx { tx_id }
        }
        KIND_ABORT_TX => {
            require_empty_payload(payload, "AbortTx")?;
            WalRecordKind::AbortTx { tx_id }
        }
        KIND_CHECKPOINT => {
            require_empty_payload(payload, "Checkpoint")?;
            WalRecordKind::Checkpoint
        }
        other => {
            return Err(DbError::Corruption(format!(
                "unknown wal record kind: {other}"
            )));
        }
    };

    Ok(WalRecord { lsn, kind })
}

fn require_empty_payload(payload: &[u8], record_name: &str) -> DbResult<()> {
    if payload.is_empty() {
        Ok(())
    } else {
        Err(DbError::Corruption(format!(
            "{record_name} wal record has unexpected payload length: {}",
            payload.len()
        )))
    }
}

fn truncated_wal(offset: u64, remaining: u64) -> DbError {
    DbError::Corruption(format!(
        "truncated wal record at offset {offset}: remaining bytes={remaining}"
    ))
}

fn wal_checksum(bytes: &[u8]) -> u32 {
    let mut checksum = 0_u32;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if (OFF_CHECKSUM..OFF_CHECKSUM + 4).contains(&index) {
            continue;
        }
        checksum = checksum.wrapping_add(u32::from(byte));
    }
    checksum
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
    use rdbms_page::{Page, PageType};
    use rdbms_vfs::{StdVfs, Vfs};
    use std::fs::OpenOptions;
    use std::io::{Seek, SeekFrom, Write};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn wal_record_round_trips_through_binary_envelope() -> DbResult<()> {
        let mut page = Page::new(PageId(7), PageType::Heap);
        page.insert_record(b"wal image")?;
        let record = WalRecord {
            lsn: Lsn(128),
            kind: WalRecordKind::page_image(TxId(11), &page)?,
        };

        let bytes = record.encode()?;
        let decoded = WalRecord::decode(&bytes)?;

        assert_eq!(decoded, record);
        Ok(())
    }

    #[test]
    fn lsn_allocator_uses_encoded_record_offsets() -> DbResult<()> {
        let mut allocator = LsnAllocator::new(Lsn(0));
        let first = allocator.allocate(WAL_HEADER_SIZE as u64)?;
        let second = allocator.allocate((WAL_HEADER_SIZE + PAGE_SIZE) as u64)?;

        assert_eq!(first, Lsn(0));
        assert_eq!(second, Lsn(WAL_HEADER_SIZE as u64));
        assert_eq!(
            allocator.next_lsn(),
            Lsn((WAL_HEADER_SIZE + WAL_HEADER_SIZE + PAGE_SIZE) as u64)
        );
        Ok(())
    }

    #[test]
    fn writer_appends_and_reader_scans_commit_marker() -> DbResult<()> {
        let path = temp_wal_path("append_scan");
        let vfs = StdVfs::new();

        {
            let file = vfs.open_database(&path)?;
            let mut writer = WalWriter::new(file)?;
            assert_eq!(writer.append(WalRecordKind::BeginTx { tx_id: TxId(1) })?, Lsn(0));
            assert_eq!(
                writer.append(WalRecordKind::CommitTx { tx_id: TxId(1) })?,
                Lsn(WAL_HEADER_SIZE as u64)
            );
            writer.sync_data()?;
        }

        let file = vfs.open_database(&path)?;
        let records = WalReader::new(file).read_all()?;
        assert_eq!(
            records,
            vec![
                WalRecord {
                    lsn: Lsn(0),
                    kind: WalRecordKind::BeginTx { tx_id: TxId(1) },
                },
                WalRecord {
                    lsn: Lsn(WAL_HEADER_SIZE as u64),
                    kind: WalRecordKind::CommitTx { tx_id: TxId(1) },
                },
            ]
        );

        cleanup_temp_file(path);
        Ok(())
    }

    #[test]
    fn reader_detects_truncated_wal_suffix() -> DbResult<()> {
        let path = temp_wal_path("truncated");
        let vfs = StdVfs::new();

        {
            let file = vfs.open_database(&path)?;
            let mut writer = WalWriter::new(file)?;
            writer.append(WalRecordKind::BeginTx { tx_id: TxId(2) })?;
            writer.sync_data()?;
        }

        {
            let mut file = OpenOptions::new().read(true).write(true).open(&path)?;
            let len = file.metadata()?.len();
            file.seek(SeekFrom::Start(len))?;
            file.write_all(&[0_u8; 8])?;
            file.sync_data()?;
        }

        let file = vfs.open_database(&path)?;
        let error = WalReader::new(file)
            .read_all()
            .err()
            .ok_or(DbError::InternalInvariant("truncated wal was accepted"))?;
        assert!(matches!(error, DbError::Corruption(message) if message.contains("truncated wal")));

        cleanup_temp_file(path);
        Ok(())
    }

    #[test]
    fn redo_hook_replays_only_committed_page_images() -> DbResult<()> {
        let mut committed_page = Page::new(PageId(10), PageType::Heap);
        committed_page.insert_record(b"committed")?;
        let mut aborted_page = Page::new(PageId(11), PageType::Heap);
        aborted_page.insert_record(b"aborted")?;

        let records = vec![
            WalRecord {
                lsn: Lsn(0),
                kind: WalRecordKind::BeginTx { tx_id: TxId(1) },
            },
            WalRecord {
                lsn: Lsn(40),
                kind: WalRecordKind::page_image(TxId(1), &committed_page)?,
            },
            WalRecord {
                lsn: Lsn(4176),
                kind: WalRecordKind::CommitTx { tx_id: TxId(1) },
            },
            WalRecord {
                lsn: Lsn(4216),
                kind: WalRecordKind::BeginTx { tx_id: TxId(2) },
            },
            WalRecord {
                lsn: Lsn(4256),
                kind: WalRecordKind::page_image(TxId(2), &aborted_page)?,
            },
            WalRecord {
                lsn: Lsn(8392),
                kind: WalRecordKind::AbortTx { tx_id: TxId(2) },
            },
        ];
        let mut sink = VecRedo::default();

        redo_committed_page_images(&records, &mut sink)?;

        assert_eq!(sink.redone.len(), 1);
        assert_eq!(sink.redone[0], (Lsn(40), TxId(1), PageId(10)));
        Ok(())
    }

    #[derive(Default)]
    struct VecRedo {
        redone: Vec<(Lsn, TxId, PageId)>,
    }

    impl PageImageRedo for VecRedo {
        fn redo_page_image(
            &mut self,
            lsn: Lsn,
            tx_id: TxId,
            page_id: PageId,
            _image: &[u8; PAGE_SIZE],
        ) -> DbResult<()> {
            self.redone.push((lsn, tx_id, page_id));
            Ok(())
        }
    }

    fn temp_wal_path(test_name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "rdbms_wal_{test_name}_{}_{}.wal",
            std::process::id(),
            nanos
        ));
        path
    }

    fn cleanup_temp_file(path: PathBuf) {
        let _ = std::fs::remove_file(path);
    }
}
