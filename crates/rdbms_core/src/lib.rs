//! Shared core types for the architecture-first RDBMS reboot.

use std::fmt;

/// Physical page identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PageId(pub u64);

/// Relation identifier from the system catalog.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RelationId(pub u64);

/// Transaction identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TxId(pub u64);

/// Log sequence number.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Lsn(pub u64);

/// Slot inside a slotted page.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SlotId(pub u16);

/// Physical row address for the heap-table MVP.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RowId {
    /// Page containing the row payload.
    pub page_id: PageId,
    /// Slot inside the page.
    pub slot_id: SlotId,
}

/// Project-wide result type.
pub type DbResult<T> = Result<T, DbError>;

/// Typed error boundary for the DBMS core.
#[derive(Debug)]
pub enum DbError {
    /// User-visible mistake: bad SQL, missing object, constraint violation.
    User(String),
    /// Retryable state: busy writer, lock timeout, conflict.
    Retryable(String),
    /// I/O error converted at subsystem boundary.
    Io(std::io::Error),
    /// Physical corruption or invalid on-disk bytes.
    Corruption(String),
    /// Internal invariant violation.
    InternalInvariant(&'static str),
    /// Extension boundary failure.
    Extension(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(message) => write!(f, "user error: {message}"),
            Self::Retryable(message) => write!(f, "retryable error: {message}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Corruption(message) => write!(f, "corruption: {message}"),
            Self::InternalInvariant(message) => write!(f, "internal invariant violation: {message}"),
            Self::Extension(message) => write!(f, "extension error: {message}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<std::io::Error> for DbError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Minimal value model retained from the legacy intuition.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// SQL NULL.
    Null,
    /// UTF-8 text.
    Text(String),
    /// Signed integer.
    Integer(i64),
    /// IEEE-754 double.
    Double(f64),
}

/// Result of SQL-facing execution.
#[derive(Clone, Debug, PartialEq)]
pub enum ExecResult {
    /// Statement completed without returning rows.
    StatementComplete { rows_affected: u64 },
    /// Query produced a materialized placeholder result.
    Query { columns: Vec<ColumnInfo>, rows: Vec<Vec<Value>> },
    /// Explain output for future planner work.
    Explain { plan: String },
}

/// Public column description.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Type name as exposed to SQL-facing layer.
    pub type_name: String,
}
