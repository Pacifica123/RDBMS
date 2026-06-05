//! System catalog model skeleton.

use rdbms_core::RelationId;

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

/// Relation metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationInfo {
    /// Stable relation id.
    pub id: RelationId,
    /// Relation name.
    pub name: String,
    /// Relation kind.
    pub kind: RelationKind,
}
