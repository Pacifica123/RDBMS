//! Minimal SQL subset for the Stage 8 storage stack.
//!
//! This crate intentionally implements a very small SQL-facing layer over
//! `rdbms_tx::TransactionalStore`. It is not a full SQL parser, binder or
//! planner. The supported statements are:
//!
//! ```text
//! CREATE TABLE name (column TYPE, ...)
//! INSERT INTO name VALUES (literal, ...)
//! CREATE INDEX name ON table(column)
//! SELECT * FROM name [WHERE column = literal]
//! SELECT column, ... FROM name [WHERE column = literal]
//! ```

use rdbms_catalog::{ColumnDef, RelationInfo};
use rdbms_core::{ColumnInfo, DbError, DbResult, ExecResult, Value};
use rdbms_index::IndexKey;
use rdbms_tx::TransactionalStore;
use rdbms_vfs::VfsFile;

const SQL_ROW_MAGIC: &[u8; 4] = b"RDBR";
const SQL_ROW_VERSION: u16 = 1;
const TAG_NULL: u8 = 0;
const TAG_INTEGER: u8 = 1;
const TAG_TEXT: u8 = 2;
const TAG_DOUBLE: u8 = 3;

/// Parsed statement for the Stage 8 SQL subset.
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    /// `CREATE TABLE name (column TYPE, ...)`.
    CreateTable {
        /// Table name.
        name: String,
        /// Column definitions.
        columns: Vec<ColumnDef>,
    },
    /// `INSERT INTO name VALUES (...)`.
    Insert {
        /// Table name.
        table: String,
        /// Literal values in table-column order.
        values: Vec<Value>,
    },
    /// `CREATE INDEX name ON table(column)`.
    CreateIndex {
        /// Index relation name.
        name: String,
        /// Indexed table name.
        table: String,
        /// Indexed column name.
        column: String,
    },
    /// `SELECT ... FROM name [WHERE column = literal]`.
    Select {
        /// Selected columns, or all columns.
        projection: Projection,
        /// Table name.
        table: String,
        /// Optional equality predicate.
        selection: Option<Selection>,
    },
}

/// Projection used by a parsed SELECT statement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Projection {
    /// `*` projection.
    All,
    /// Explicit column list.
    Columns(Vec<String>),
}

/// Equality predicate supported by Stage 8.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    /// Predicate column.
    pub column: String,
    /// Literal compared with `=`.
    pub value: Value,
}

/// Parse one SQL statement from the supported subset.
pub fn parse_statement(sql: &str) -> DbResult<Statement> {
    let mut parser = Parser::new(sql)?;
    let statement = parser.parse_statement()?;
    parser.finish()?;
    Ok(statement)
}

/// Execute one SQL statement against a transactional store.
///
/// Positional parameters are deliberately rejected in Stage 7. They become part
/// of a later binder/executor milestone.
pub fn execute<F: VfsFile>(
    store: &mut TransactionalStore<F>,
    sql: &str,
    params: &[Value],
) -> DbResult<ExecResult> {
    if !params.is_empty() {
        return Err(DbError::User(
            "SQL parameters are not supported in Stage 8".to_string(),
        ));
    }

    match parse_statement(sql)? {
        Statement::CreateTable { name, columns } => {
            store.create_table_autocommit(name, columns)?;
            Ok(ExecResult::StatementComplete { rows_affected: 0 })
        }
        Statement::Insert { table, values } => execute_insert(store, &table, values),
        Statement::CreateIndex { name, table, column } => {
            execute_create_index(store, &name, &table, &column)
        }
        Statement::Select {
            projection,
            table,
            selection,
        } => execute_select(store, &projection, &table, selection.as_ref()),
    }
}

/// Thin owned SQL session wrapper over `TransactionalStore`.
pub struct SqlSession<F: VfsFile> {
    store: TransactionalStore<F>,
}

impl<F: VfsFile> SqlSession<F> {
    /// Create a SQL session from an opened transaction store.
    pub fn new(store: TransactionalStore<F>) -> Self {
        Self { store }
    }

    /// Execute one SQL statement.
    pub fn execute(&mut self, sql: &str, params: &[Value]) -> DbResult<ExecResult> {
        execute(&mut self.store, sql, params)
    }

    /// Borrow the underlying transaction store.
    pub fn store(&self) -> &TransactionalStore<F> {
        &self.store
    }

    /// Mutably borrow the underlying transaction store.
    pub fn store_mut(&mut self) -> &mut TransactionalStore<F> {
        &mut self.store
    }

    /// Consume the session and return the underlying transaction store.
    pub fn into_store(self) -> TransactionalStore<F> {
        self.store
    }
}

/// Encode SQL values into heap row bytes used by SQL tables.
pub fn encode_row(values: &[Value]) -> DbResult<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(SQL_ROW_MAGIC);
    write_u16(&mut bytes, SQL_ROW_VERSION);
    write_u16_len(&mut bytes, values.len(), "SQL row value count")?;

    for value in values {
        match value {
            Value::Null => bytes.push(TAG_NULL),
            Value::Integer(value) => {
                bytes.push(TAG_INTEGER);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            Value::Text(value) => {
                bytes.push(TAG_TEXT);
                write_u32_len(&mut bytes, value.len(), "SQL text literal length")?;
                bytes.extend_from_slice(value.as_bytes());
            }
            Value::Double(value) => {
                bytes.push(TAG_DOUBLE);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }

    Ok(bytes)
}

/// Decode SQL row bytes from a heap table.
pub fn decode_row(bytes: &[u8]) -> DbResult<Vec<Value>> {
    let mut cursor = DecodeCursor::new(bytes);
    let magic = cursor.read_bytes(4)?;
    if magic != &SQL_ROW_MAGIC[..] {
        return Err(DbError::Corruption("invalid SQL row magic".to_string()));
    }

    let version = cursor.read_u16()?;
    if version != SQL_ROW_VERSION {
        return Err(DbError::Corruption(format!(
            "unsupported SQL row version: {version}"
        )));
    }

    let value_count = usize::from(cursor.read_u16()?);
    let mut values = Vec::with_capacity(value_count);
    for _ in 0..value_count {
        let tag = cursor.read_u8()?;
        let value = match tag {
            TAG_NULL => Value::Null,
            TAG_INTEGER => Value::Integer(cursor.read_i64()?),
            TAG_TEXT => {
                let len = usize::try_from(cursor.read_u32()?).map_err(|_| {
                    DbError::Corruption("SQL text length is too large".to_string())
                })?;
                let bytes = cursor.read_bytes(len)?;
                let text = String::from_utf8(bytes.to_vec()).map_err(|_| {
                    DbError::Corruption("SQL text value is not valid UTF-8".to_string())
                })?;
                Value::Text(text)
            }
            TAG_DOUBLE => Value::Double(cursor.read_f64()?),
            _ => {
                return Err(DbError::Corruption(format!(
                    "unknown SQL row value tag: {tag}"
                )));
            }
        };
        values.push(value);
    }

    cursor.finish()?;
    Ok(values)
}

fn execute_insert<F: VfsFile>(
    store: &mut TransactionalStore<F>,
    table: &str,
    values: Vec<Value>,
) -> DbResult<ExecResult> {
    let relation = lookup_relation(store, table)?.clone();
    let values = coerce_values(&relation.columns, values)?;
    let bytes = encode_row(&values)?;
    let index_relations: Vec<RelationInfo> = store
        .catalog()
        .indexes_on_table(relation.id)
        .into_iter()
        .cloned()
        .collect();

    let mut transaction = store.begin()?;
    let row_id = transaction.insert_row(relation.id, &bytes)?;
    for index_relation in index_relations {
        let index_storage = index_relation.index_storage().ok_or(
            DbError::InternalInvariant("index relation without index storage"),
        )?;
        let column_index = resolve_column_index(&relation, index_storage.column_name())?;
        if let Some(key) = index_key_from_value(&values[column_index])? {
            transaction.insert_index_entry(index_relation.id, key, row_id)?;
        }
    }
    transaction.commit()?;
    Ok(ExecResult::StatementComplete { rows_affected: 1 })
}

fn execute_create_index<F: VfsFile>(
    store: &mut TransactionalStore<F>,
    name: &str,
    table: &str,
    column: &str,
) -> DbResult<ExecResult> {
    let relation = lookup_relation(store, table)?.clone();
    let column_index = resolve_column_index(&relation, column)?;
    ensure_indexable_column(&relation.columns[column_index])?;

    let mut transaction = store.begin()?;
    let (index_relation_id, _root_page_id) =
        transaction.create_index(name.to_string(), relation.id, column.to_string())?;

    for heap_row in transaction.full_scan(relation.id)? {
        let values = decode_row(&heap_row.bytes)?;
        validate_row_width(&relation, &values)?;
        if let Some(key) = index_key_from_value(&values[column_index])? {
            transaction.insert_index_entry(index_relation_id, key, heap_row.row_id)?;
        }
    }

    transaction.commit()?;
    Ok(ExecResult::StatementComplete { rows_affected: 0 })
}

fn execute_select<F: VfsFile>(
    store: &mut TransactionalStore<F>,
    projection: &Projection,
    table: &str,
    selection: Option<&Selection>,
) -> DbResult<ExecResult> {
    let relation = lookup_relation(store, table)?.clone();
    let projected_indexes = resolve_projection(&relation, projection)?;
    let selection_index = match selection {
        Some(selection) => Some(resolve_column_index(&relation, &selection.column)?),
        None => None,
    };

    let candidate_rows = candidate_rows(store, &relation, selection)?;
    let mut rows = Vec::new();
    for heap_row in candidate_rows {
        let values = decode_row(&heap_row.bytes)?;
        validate_row_width(&relation, &values)?;

        if let (Some(index), Some(selection)) = (selection_index, selection) {
            if !sql_values_equal(&values[index], &selection.value) {
                continue;
            }
        }

        let mut output_row = Vec::with_capacity(projected_indexes.len());
        for index in &projected_indexes {
            output_row.push(values[*index].clone());
        }
        rows.push(output_row);
    }

    let columns = projected_indexes
        .iter()
        .map(|index| ColumnInfo {
            name: relation.columns[*index].name.clone(),
            type_name: relation.columns[*index].type_name.clone(),
        })
        .collect();

    Ok(ExecResult::Query { columns, rows })
}

fn candidate_rows<F: VfsFile>(
    store: &mut TransactionalStore<F>,
    relation: &RelationInfo,
    selection: Option<&Selection>,
) -> DbResult<Vec<rdbms_catalog::HeapRow>> {
    let Some(selection) = selection else {
        return store.full_scan(relation.id);
    };
    let Some(index_relation) = find_index_on_column(store, relation.id, &selection.column) else {
        return store.full_scan(relation.id);
    };
    let Some(key) = index_key_from_value(&selection.value)? else {
        return store.full_scan(relation.id);
    };

    let row_ids = store.lookup_index(index_relation.id, &key)?;
    let mut rows = Vec::with_capacity(row_ids.len());
    for row_id in row_ids {
        if let Some(row) = store.read_row(relation.id, row_id)? {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn lookup_relation<'a, F: VfsFile>(
    store: &'a TransactionalStore<F>,
    table: &str,
) -> DbResult<&'a RelationInfo> {
    store
        .catalog()
        .relation_by_name(table)
        .ok_or(DbError::User(format!("unknown relation: {table}")))
}

fn resolve_projection(relation: &RelationInfo, projection: &Projection) -> DbResult<Vec<usize>> {
    match projection {
        Projection::All => Ok((0..relation.columns.len()).collect()),
        Projection::Columns(columns) => columns
            .iter()
            .map(|column| resolve_column_index(relation, column))
            .collect(),
    }
}

fn resolve_column_index(relation: &RelationInfo, column_name: &str) -> DbResult<usize> {
    relation
        .columns
        .iter()
        .position(|column| column.name == column_name)
        .ok_or(DbError::User(format!(
            "unknown column '{}' on relation '{}'",
            column_name, relation.name
        )))
}

fn find_index_on_column<F: VfsFile>(
    store: &TransactionalStore<F>,
    table_id: rdbms_core::RelationId,
    column_name: &str,
) -> Option<RelationInfo> {
    store
        .catalog()
        .indexes_on_table(table_id)
        .into_iter()
        .find(|relation| {
            relation
                .index_storage()
                .is_some_and(|storage| storage.column_name() == column_name)
        })
        .cloned()
}

fn validate_row_width(relation: &RelationInfo, values: &[Value]) -> DbResult<()> {
    if values.len() != relation.columns.len() {
        return Err(DbError::Corruption(format!(
            "SQL row value count {} does not match catalog column count {}",
            values.len(),
            relation.columns.len()
        )));
    }
    Ok(())
}

fn ensure_indexable_column(column: &ColumnDef) -> DbResult<()> {
    match column.type_name.as_str() {
        "INT" | "INTEGER" | "TEXT" => Ok(()),
        other => Err(DbError::User(format!(
            "column '{}' of type '{}' is not indexable in Stage 8",
            column.name, other
        ))),
    }
}

fn index_key_from_value(value: &Value) -> DbResult<Option<IndexKey>> {
    match value {
        Value::Null => Ok(None),
        Value::Integer(value) => Ok(Some(IndexKey::Integer(*value))),
        Value::Text(value) => Ok(Some(IndexKey::Text(value.clone()))),
        Value::Double(_) => Ok(None),
    }
}

fn coerce_values(columns: &[ColumnDef], values: Vec<Value>) -> DbResult<Vec<Value>> {
    if columns.len() != values.len() {
        return Err(DbError::User(format!(
            "INSERT value count {} does not match column count {}",
            values.len(),
            columns.len()
        )));
    }

    columns
        .iter()
        .zip(values)
        .map(|(column, value)| coerce_value(column, value))
        .collect()
}

fn coerce_value(column: &ColumnDef, value: Value) -> DbResult<Value> {
    if value == Value::Null {
        return Ok(Value::Null);
    }

    match column.type_name.as_str() {
        "INT" | "INTEGER" => match value {
            Value::Integer(_) => Ok(value),
            _ => Err(type_error(column, "integer")),
        },
        "TEXT" => match value {
            Value::Text(_) => Ok(value),
            _ => Err(type_error(column, "text")),
        },
        "DOUBLE" | "REAL" | "FLOAT" => match value {
            Value::Double(_) => Ok(value),
            Value::Integer(value) => Ok(Value::Double(value as f64)),
            _ => Err(type_error(column, "double")),
        },
        other => Err(DbError::User(format!(
            "unsupported SQL column type '{}' on column '{}'",
            other, column.name
        ))),
    }
}

fn type_error(column: &ColumnDef, expected: &str) -> DbError {
    DbError::User(format!(
        "column '{}' expects {expected}, type is {}",
        column.name, column.type_name
    ))
}

fn sql_values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Integer(left), Value::Integer(right)) => left == right,
        (Value::Text(left), Value::Text(right)) => left == right,
        (Value::Double(left), Value::Double(right)) => left == right,
        (Value::Double(left), Value::Integer(right)) => *left == *right as f64,
        (Value::Integer(left), Value::Double(right)) => *left as f64 == *right,
        _ => false,
    }
}

fn normalize_identifier(identifier: String) -> String {
    identifier.to_ascii_lowercase()
}

fn normalize_type_name(type_name: String) -> DbResult<String> {
    let normalized = type_name.to_ascii_uppercase();
    match normalized.as_str() {
        "INT" | "INTEGER" | "TEXT" | "DOUBLE" | "REAL" | "FLOAT" => Ok(normalized),
        _ => Err(DbError::User(format!(
            "unsupported SQL type: {type_name}"
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

fn write_u32_len(bytes: &mut Vec<u8>, value: usize, field: &str) -> DbResult<()> {
    let value = u32::try_from(value)
        .map_err(|_| DbError::User(format!("{field} does not fit into u32")))?;
    bytes.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Ident(String),
    Number(String),
    String(String),
    Comma,
    LParen,
    RParen,
    Semicolon,
    Star,
    Eq,
    Eof,
}

struct Lexer<'a> {
    chars: std::str::Chars<'a>,
    lookahead: Option<char>,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        let mut chars = input.chars();
        let lookahead = chars.next();
        Self { chars, lookahead }
    }

    fn tokenize(mut self) -> DbResult<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let token = match self.lookahead {
                Some(',') => {
                    self.bump();
                    Token::Comma
                }
                Some('(') => {
                    self.bump();
                    Token::LParen
                }
                Some(')') => {
                    self.bump();
                    Token::RParen
                }
                Some(';') => {
                    self.bump();
                    Token::Semicolon
                }
                Some('*') => {
                    self.bump();
                    Token::Star
                }
                Some('=') => {
                    self.bump();
                    Token::Eq
                }
                Some('\'') => Token::String(self.read_string()?),
                Some(character) if is_identifier_start(character) => {
                    Token::Ident(self.read_identifier())
                }
                Some(character) if character.is_ascii_digit() || character == '-' => {
                    Token::Number(self.read_number()?)
                }
                Some(character) => {
                    return Err(DbError::User(format!(
                        "unexpected character in SQL: {character}"
                    )));
                }
                None => Token::Eof,
            };
            let is_eof = token == Token::Eof;
            tokens.push(token);
            if is_eof {
                return Ok(tokens);
            }
        }
    }

    fn bump(&mut self) -> Option<char> {
        let current = self.lookahead;
        self.lookahead = self.chars.next();
        current
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.lookahead, Some(character) if character.is_whitespace()) {
            self.bump();
        }
    }

    fn read_identifier(&mut self) -> String {
        let mut identifier = String::new();
        while let Some(character) = self.lookahead {
            if is_identifier_continue(character) {
                identifier.push(character);
                self.bump();
            } else {
                break;
            }
        }
        identifier
    }

    fn read_number(&mut self) -> DbResult<String> {
        let mut number = String::new();
        if self.lookahead == Some('-') {
            number.push('-');
            self.bump();
            if !matches!(self.lookahead, Some(character) if character.is_ascii_digit()) {
                return Err(DbError::User("expected digit after '-'".to_string()));
            }
        }

        while let Some(character) = self.lookahead {
            if character.is_ascii_digit() || character == '.' {
                number.push(character);
                self.bump();
            } else {
                break;
            }
        }
        Ok(number)
    }

    fn read_string(&mut self) -> DbResult<String> {
        self.bump();
        let mut value = String::new();
        loop {
            match self.lookahead {
                Some('\'') => {
                    self.bump();
                    if self.lookahead == Some('\'') {
                        value.push('\'');
                        self.bump();
                    } else {
                        return Ok(value);
                    }
                }
                Some(character) => {
                    value.push(character);
                    self.bump();
                }
                None => return Err(DbError::User("unterminated string literal".to_string())),
            }
        }
    }
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

struct Parser {
    tokens: Vec<Token>,
    offset: usize,
}

impl Parser {
    fn new(sql: &str) -> DbResult<Self> {
        let tokens = Lexer::new(sql).tokenize()?;
        Ok(Self { tokens, offset: 0 })
    }

    fn parse_statement(&mut self) -> DbResult<Statement> {
        if self.consume_keyword("CREATE") {
            if self.consume_keyword("TABLE") {
                self.parse_create_table()
            } else if self.consume_keyword("INDEX") {
                self.parse_create_index()
            } else {
                Err(DbError::User("expected TABLE or INDEX after CREATE".to_string()))
            }
        } else if self.consume_keyword("INSERT") {
            self.parse_insert()
        } else if self.consume_keyword("SELECT") {
            self.parse_select()
        } else {
            Err(DbError::User("expected CREATE, INSERT or SELECT".to_string()))
        }
    }

    fn parse_create_table(&mut self) -> DbResult<Statement> {
        let name = normalize_identifier(self.expect_identifier("table name")?);
        self.expect_token(Token::LParen, "expected '(' after table name")?;

        let mut columns = Vec::new();
        loop {
            let column_name = normalize_identifier(self.expect_identifier("column name")?);
            let type_name = normalize_type_name(self.expect_identifier("column type")?)?;
            columns.push(ColumnDef::new(column_name, type_name));

            if self.consume_token(&Token::Comma) {
                continue;
            }
            break;
        }

        self.expect_token(Token::RParen, "expected ')' after column list")?;
        Ok(Statement::CreateTable { name, columns })
    }

    fn parse_create_index(&mut self) -> DbResult<Statement> {
        let name = normalize_identifier(self.expect_identifier("index name")?);
        self.expect_keyword("ON")?;
        let table = normalize_identifier(self.expect_identifier("table name")?);
        self.expect_token(Token::LParen, "expected '(' before indexed column")?;
        let column = normalize_identifier(self.expect_identifier("indexed column")?);
        self.expect_token(Token::RParen, "expected ')' after indexed column")?;
        Ok(Statement::CreateIndex { name, table, column })
    }

    fn parse_insert(&mut self) -> DbResult<Statement> {
        self.expect_keyword("INTO")?;
        let table = normalize_identifier(self.expect_identifier("table name")?);
        self.expect_keyword("VALUES")?;
        self.expect_token(Token::LParen, "expected '(' before VALUES list")?;

        let mut values = Vec::new();
        if !self.consume_token(&Token::RParen) {
            loop {
                values.push(self.parse_literal()?);
                if self.consume_token(&Token::Comma) {
                    continue;
                }
                self.expect_token(Token::RParen, "expected ')' after VALUES list")?;
                break;
            }
        }

        Ok(Statement::Insert { table, values })
    }

    fn parse_select(&mut self) -> DbResult<Statement> {
        let projection = if self.consume_token(&Token::Star) {
            Projection::All
        } else {
            let mut columns = Vec::new();
            loop {
                columns.push(normalize_identifier(self.expect_identifier("selected column")?));
                if self.consume_token(&Token::Comma) {
                    continue;
                }
                break;
            }
            Projection::Columns(columns)
        };

        self.expect_keyword("FROM")?;
        let table = normalize_identifier(self.expect_identifier("table name")?);
        let selection = if self.consume_keyword("WHERE") {
            let column = normalize_identifier(self.expect_identifier("predicate column")?);
            self.expect_token(Token::Eq, "expected '=' in WHERE predicate")?;
            Some(Selection {
                column,
                value: self.parse_literal()?,
            })
        } else {
            None
        };

        Ok(Statement::Select {
            projection,
            table,
            selection,
        })
    }

    fn parse_literal(&mut self) -> DbResult<Value> {
        match self.peek() {
            Token::Ident(value) if value.eq_ignore_ascii_case("NULL") => {
                self.offset += 1;
                Ok(Value::Null)
            }
            Token::String(value) => {
                let value = value.clone();
                self.offset += 1;
                Ok(Value::Text(value))
            }
            Token::Number(value) => {
                let value = value.clone();
                self.offset += 1;
                if value.contains('.') {
                    let parsed = value.parse::<f64>().map_err(|_| {
                        DbError::User(format!("invalid double literal: {value}"))
                    })?;
                    Ok(Value::Double(parsed))
                } else {
                    let parsed = value.parse::<i64>().map_err(|_| {
                        DbError::User(format!("invalid integer literal: {value}"))
                    })?;
                    Ok(Value::Integer(parsed))
                }
            }
            _ => Err(DbError::User("expected literal value".to_string())),
        }
    }

    fn finish(&mut self) -> DbResult<()> {
        if self.consume_token(&Token::Semicolon) {
            while self.consume_token(&Token::Semicolon) {}
        }
        match self.peek() {
            Token::Eof => Ok(()),
            token => Err(DbError::User(format!(
                "unexpected token after SQL statement: {token:?}"
            ))),
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> DbResult<()> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(DbError::User(format!("expected keyword {keyword}")))
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        match self.peek() {
            Token::Ident(value) if value.eq_ignore_ascii_case(keyword) => {
                self.offset += 1;
                true
            }
            _ => false,
        }
    }

    fn expect_identifier(&mut self, description: &str) -> DbResult<String> {
        match self.peek() {
            Token::Ident(value) => {
                let value = value.clone();
                self.offset += 1;
                Ok(value)
            }
            _ => Err(DbError::User(format!("expected {description}"))),
        }
    }

    fn expect_token(&mut self, token: Token, message: &str) -> DbResult<()> {
        if self.consume_token(&token) {
            Ok(())
        } else {
            Err(DbError::User(message.to_string()))
        }
    }

    fn consume_token(&mut self, token: &Token) -> bool {
        if self.peek() == token {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> &Token {
        if self.offset < self.tokens.len() {
            &self.tokens[self.offset]
        } else {
            &self.tokens[self.tokens.len() - 1]
        }
    }
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
            .ok_or(DbError::Corruption("SQL row offset overflow".to_string()))?;
        if end > self.bytes.len() {
            return Err(DbError::Corruption("SQL row is truncated".to_string()));
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

    fn read_i64(&mut self) -> DbResult<i64> {
        let bytes = self.read_bytes(8)?;
        Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_f64(&mut self) -> DbResult<f64> {
        let bytes = self.read_bytes(8)?;
        Ok(f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn finish(&self) -> DbResult<()> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(DbError::Corruption(
                "SQL row has trailing bytes".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbms_tx::open_transactional_store;
    use rdbms_vfs::StdVfs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_create_insert_and_select() -> DbResult<()> {
        let create = parse_statement("CREATE TABLE users (id INT, name TEXT);")?;
        assert_eq!(
            create,
            Statement::CreateTable {
                name: "users".to_string(),
                columns: vec![ColumnDef::new("id", "INT"), ColumnDef::new("name", "TEXT")],
            }
        );

        let insert = parse_statement("insert into users values (1, 'Ada');")?;
        assert_eq!(
            insert,
            Statement::Insert {
                table: "users".to_string(),
                values: vec![Value::Integer(1), Value::Text("Ada".to_string())],
            }
        );

        let select = parse_statement("select name from users where id = 1")?;
        assert_eq!(
            select,
            Statement::Select {
                projection: Projection::Columns(vec!["name".to_string()]),
                table: "users".to_string(),
                selection: Some(Selection {
                    column: "id".to_string(),
                    value: Value::Integer(1),
                }),
            }
        );

        let create_index = parse_statement("CREATE INDEX users_id_idx ON users(id)")?;
        assert_eq!(
            create_index,
            Statement::CreateIndex {
                name: "users_id_idx".to_string(),
                table: "users".to_string(),
                column: "id".to_string(),
            }
        );
        Ok(())
    }

    #[test]
    fn encodes_and_decodes_sql_rows() -> DbResult<()> {
        let values = vec![
            Value::Integer(42),
            Value::Text("hello".to_string()),
            Value::Null,
            Value::Double(1.5),
        ];
        let bytes = encode_row(&values)?;
        assert_eq!(decode_row(&bytes)?, values);
        Ok(())
    }

    #[test]
    fn executes_create_insert_select_and_where() -> DbResult<()> {
        let paths = temp_paths("select_where");
        let vfs = StdVfs::new();
        let mut store = open_transactional_store(&vfs, &paths.0, &paths.1)?;

        execute(
            &mut store,
            "CREATE TABLE users (id INT, name TEXT, score DOUBLE)",
            &[],
        )?;
        execute(&mut store, "INSERT INTO users VALUES (1, 'Ada', 9.5)", &[])?;
        execute(&mut store, "INSERT INTO users VALUES (2, 'Linus', 7)", &[])?;
        execute(&mut store, "CREATE INDEX users_id_idx ON users(id)", &[])?;
        execute(&mut store, "INSERT INTO users VALUES (3, 'Grace', 8.25)", &[])?;

        let result = execute(&mut store, "SELECT name, score FROM users WHERE id = 2", &[])?;
        assert_eq!(
            result,
            ExecResult::Query {
                columns: vec![
                    ColumnInfo {
                        name: "name".to_string(),
                        type_name: "TEXT".to_string(),
                    },
                    ColumnInfo {
                        name: "score".to_string(),
                        type_name: "DOUBLE".to_string(),
                    },
                ],
                rows: vec![vec![Value::Text("Linus".to_string()), Value::Double(7.0)]],
            }
        );

        let result = execute(&mut store, "SELECT name FROM users WHERE id = 3", &[])?;
        assert_eq!(
            result,
            ExecResult::Query {
                columns: vec![ColumnInfo {
                    name: "name".to_string(),
                    type_name: "TEXT".to_string(),
                }],
                rows: vec![vec![Value::Text("Grace".to_string())]],
            }
        );

        cleanup(paths);
        Ok(())
    }

    #[test]
    fn rejects_insert_with_wrong_value_count() -> DbResult<()> {
        let paths = temp_paths("wrong_count");
        let vfs = StdVfs::new();
        let mut store = open_transactional_store(&vfs, &paths.0, &paths.1)?;

        execute(&mut store, "CREATE TABLE users (id INT, name TEXT)", &[])?;
        let error = execute(&mut store, "INSERT INTO users VALUES (1)", &[])
            .err()
            .ok_or(DbError::InternalInvariant("expected INSERT error"))?;
        assert!(matches!(error, DbError::User(_)));

        cleanup(paths);
        Ok(())
    }

    fn temp_paths(test_name: &str) -> (PathBuf, PathBuf) {
        let mut base = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        base.push(format!(
            "rdbms_sql_{test_name}_{}_{}",
            std::process::id(),
            nanos
        ));
        (base.with_extension("dbonrs"), base.with_extension("wal"))
    }

    fn cleanup(paths: (PathBuf, PathBuf)) {
        let _ = std::fs::remove_file(paths.0);
        let _ = std::fs::remove_file(paths.1);
    }
}
