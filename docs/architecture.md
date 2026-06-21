# Architecture

## 1. Назначение

RDBMS — учебно-инженерный проект собственной СУБД на Rust. Главная цель — не нарисовать SQL shell, а построить ядро с понятными гарантиями хранения, восстановления, каталога, транзакций и расширяемости.

Рабочая формула:

```text
ядро СУБД = storage + WAL + recovery + catalog + transaction boundary + executor API + extension boundary + platform boundary + diagnostics
```

## 2. Почему проект перезапущен

Ранний черновик был построен вокруг `sql_execute(&mut self, query: &str)` и строкового диспетчера. Это давало быстрый видимый прогресс, но скрывало отсутствие физического формата, WAL, recovery, каталога, транзакций и переносимого IO-слоя.

Новая архитектура строится снизу вверх:

```text
байты → страницы → VFS/page store → WAL → recovery → каталог → таблицы → транзакции → SQL subset → индексы → расширения → platform ports
```

SQL остаётся пользовательским интерфейсом, но не является фундаментом ядра.

## 3. Подсистемы

### rdbms_core

Общие типы, ошибки, идентификаторы и публичные контракты. Здесь живут `PageId`, `RelationId`, `TxId`, `Lsn`, `DbError`, `DbResult`, `Value`, `ColumnInfo` и `ExecResult`.

### rdbms_vfs

Абстракция файловой системы, синхронизации данных и первого page store. Нужна для Linux, Windows, Android, тестов отказов и будущей fault injection. Ядро не должно напрямую зависеть от произвольных вызовов `std::fs` в бизнес-логике. Текущий слой реализует `StdVfs`, random-access `read_at/write_at`, `len`, `sync_data` и `PageFile`, где `PageId` отображается в `page_id * PAGE_SIZE`.

### rdbms_page

Физическая страница: размер, заголовок, checksum, layout, границы slotted page. Это первая настоящая единица storage-мышления.

### rdbms_wal

Журнал предзаписи. Текущий слой реализует WAL record binary envelope v0, LSN allocator, append-only writer, sequential reader, commit marker, truncated suffix detection и page-image redo hook. WAL не открывает базу сам и не пишет страницы данных напрямую.

### rdbms_recovery

Минимальный recovery loop. Слой открывает data file и WAL file через VFS, сканирует WAL, применяет только committed full-page images к `PageFile`, игнорирует uncommitted page images и возвращает recovered page-file handle. На текущем этапе это redo-only skeleton без undo и checkpoint state; commit ordering для catalog/heap операций живёт выше, в `rdbms_tx`.

### rdbms_catalog

Persistent catalog и heap table v0. Текущий слой резервирует page 0 под catalog record, хранит `RelationId -> StorageObject::Heap { pages }`, умеет bootstrap catalog, internal `create_table`, raw `insert_row` и `full_scan`. Для Stage 6 каталог также отдаёт transaction-staging helpers: построить catalog page image, выделить page id и обновить heap storage metadata без немедленной записи в data file.

### rdbms_tx

Transactions v0. Слой владеет `CatalogStore` и `WalWriter`, даёт `begin/commit/rollback`, autocommit helpers и один active writer на handle. Dirty catalog/heap pages сначала живут в memory staging map. Commit пишет `PageImage` records в WAL, затем `CommitTx`, затем sync WAL, и только после этого пишет dirty pages в data file. Rollback отбрасывает staged pages без physical undo.

### rdbms_sql

SQL subset v0. Слой содержит маленький lexer/parser, `Statement` AST, SQL row encoding v0 и прямой executor поверх `rdbms_tx::TransactionalStore`. Сейчас поддержаны `CREATE TABLE`, `INSERT INTO ... VALUES ...`, `CREATE INDEX`, `LOAD EXTENSION`, scalar `SELECT function(literal, ...)`, `SELECT *`, `SELECT column list` и `WHERE column = literal`. Binder, optimizer, prepared statements и полноценный operator tree ещё не реализованы.

### rdbms_ext_abi

Стабильная внешняя граница расширений. Наружу нельзя отдавать сырой Rust trait как долгоживущий plugin contract. Для native plugins нужна C-compatible ABI или другой стабильный слой, например WASM.

### rdbms_extension

Static extension registry v0. Текущий слой регистрирует built-in extension descriptors, проверяет ABI version и вызывает scalar functions через безопасные Rust function pointers. Dynamic native plugins пока не загружаются.


### rdbms_android

Android native-library smoke crate. Stage 10 builds it as `rlib` and `cdylib`, exports JNI-shaped smoke symbols and links it against the SQL/core stack. It is not an Android app and it does not expose a full SQL JNI API yet.

### rdbms_cli

Тонкая оболочка. CLI не владеет архитектурой, а только вызывает публичный API.

## 4. Инварианты уровня ядра

1. Страница либо проходит проверку формата и checksum, либо считается повреждённой.
2. WAL record имеет LSN и достаточную информацию для recovery-сценария своего milestone.
3. Commit не считается durable, пока нужные данные не прошли через требуемую sync-границу.
4. Catalog changes восстанавливаются тем же механизмом, что и пользовательские данные.
5. Transaction v0 не пишет dirty pages в data file до durable WAL commit marker.
6. SQL-ошибка не должна превращаться в corruption или internal invariant violation.
7. Расширение не может нарушить память ядра через нестабильный Rust ABI.
8. Android/Linux/Windows различия изолируются за VFS и feature-флагами.

## 5. Public API sketch

```rust
pub struct Database;
pub struct Connection;
pub struct Transaction;

impl Database {
    pub fn open(path: impl AsRef<std::path::Path>, options: DbOptions) -> DbResult<Self>;
    pub fn open_in_memory(options: DbOptions) -> DbResult<Self>;
    pub fn connect(&self) -> DbResult<Connection>;
}

impl Connection {
    pub fn execute(&mut self, sql: &str, params: &[Value]) -> DbResult<ExecResult>;
    pub fn transaction(&mut self) -> DbResult<Transaction>;
}
```

Публичный API не раскрывает page/index internals.

## 6. Минимальная конкурентная модель

MVP: many readers + single writer. Это ограничение принимается явно, чтобы не смешивать первую версию storage/recovery с полноценным MVCC и конкурентными writer-ами.

## 7. Граница с legacy

Старый код сохранён только как исторический материал. Он не импортируется, не компилируется и не является upstream-модулем новой архитектуры.

## Stage 8 — index layer

Stage 8 inserts `rdbms_index` between SQL execution and physical pages.

```text
rdbms_sql
  |
  +-- sequential heap scan through rdbms_tx
  |
  +-- equality lookup through rdbms_index when an index exists

rdbms_tx
  |
  +-- stages catalog pages
  +-- stages heap pages
  +-- stages index pages
```

The index layer does not own files and does not bypass transactions. It receives a small page-store interface from `rdbms_tx`, so index changes participate in the same dirty-page staging and WAL full-page-image commit protocol as catalog and heap changes.

## Stage 9 — extension v0

Stage 9 inserts a safe static extension path above SQL and catalog metadata.

```text
rdbms_sql
  |
  +-- LOAD EXTENSION stdlib
  +-- SELECT scalar_function(literal, ...)

rdbms_extension
  |
  +-- static registry
  +-- ABI version check through rdbms_ext_abi

rdbms_catalog
  |
  +-- persisted extension metadata in catalog page 0
```

Native plugins are still not loaded at runtime. The project now has an ABI descriptor sketch plus a safe built-in registry path. This keeps extension behavior testable without crossing unsafe FFI boundaries.


## Stage 10 — platform ports

Stage 10 adds portability smoke coverage around the current stack.

```text
rdbms_vfs
  |
  +-- cross-platform path/sync smoke
  +-- Windows-only path/fsync smoke in CI

rdbms_android
  |
  +-- cdylib/rlib
  +-- JNI-shaped smoke symbols
  +-- host JNI smoke tests

.github/workflows/ci.yml
  |
  +-- Linux/Windows/macOS Rust matrix
  +-- Android aarch64 native library build
```

The platform layer does not change page, WAL, recovery, catalog, transaction, SQL, index or extension formats. Platform-specific behavior should remain behind narrow boundaries such as `rdbms_vfs` or `rdbms_android`.
