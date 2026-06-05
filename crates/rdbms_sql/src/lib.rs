//! SQL-facing placeholder. Real parsing starts after storage/catalog milestones.

use rdbms_core::{DbError, DbResult, ExecResult, Value};

/// Execute SQL against the future engine boundary.
pub fn execute_placeholder(sql: &str, _params: &[Value]) -> DbResult<ExecResult> {
    if sql.trim().is_empty() {
        return Err(DbError::User("empty SQL statement".to_string()));
    }

    Ok(ExecResult::Explain {
        plan: "SQL layer is intentionally deferred until storage/catalog milestones".to_string(),
    })
}
