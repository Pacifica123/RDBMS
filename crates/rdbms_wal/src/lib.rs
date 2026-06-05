//! Write-ahead log skeleton.

use rdbms_core::{Lsn, PageId, TxId};

/// Minimal WAL record kinds for the first recovery milestones.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WalRecordKind {
    /// Transaction started.
    BeginTx { tx_id: TxId },
    /// A full page image was written for simple redo.
    PageImage { tx_id: TxId, page_id: PageId },
    /// Transaction committed.
    CommitTx { tx_id: TxId },
    /// Transaction aborted.
    AbortTx { tx_id: TxId },
    /// Recovery checkpoint marker.
    Checkpoint,
}

/// WAL record envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalRecord {
    /// Record LSN.
    pub lsn: Lsn,
    /// Record kind.
    pub kind: WalRecordKind,
}
