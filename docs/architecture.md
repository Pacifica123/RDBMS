# Architecture

## 1. Назначение

RDBMS — учебно-инженерный проект собственной СУБД на Rust. Главная цель — не нарисовать SQL shell, а построить ядро с понятными гарантиями хранения, восстановления, каталога, транзакций и расширяемости.

Рабочая формула:

```text
ядро СУБД = storage + WAL + recovery + catalog + transaction boundary + executor API + extension boundary + diagnostics
```

## 2. Почему проект перезапущен

Ранний черновик был построен вокруг `sql_execute(&mut self, query: &str)` и строкового диспетчера. Это давало быстрый видимый прогресс, но скрывало отсутствие физического формата, WAL, recovery, каталога, транзакций и переносимого IO-слоя.

Новая архитектура строится снизу вверх:

```text
байты → страницы → VFS/page store → WAL → recovery → каталог → таблицы → транзакции → индексы → SQL → расширения
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

Журнал предзаписи. Текущий слой реализует WAL record binary envelope v0, LSN allocator, append-only writer, sequential reader, commit marker, truncated suffix detection и page-image redo hook. Полный recovery loop остаётся отдельным слоем.

### rdbms_catalog

Системные таблицы и метаданные: relations, columns, indexes, versions. Каталог не должен быть обычным Rust-полем в структуре `Database`; он должен жить в storage и участвовать в recovery.

### rdbms_sql

Поздний слой. Parser, binder, logical plan, optimizer и executor появляются после минимального storage/catalog/transaction основания.

### rdbms_ext_abi

Стабильная внешняя граница расширений. Наружу нельзя отдавать сырой Rust trait как долгоживущий plugin contract. Для native plugins нужна C-compatible ABI или другой стабильный слой, например WASM.

### rdbms_cli

Тонкая оболочка. CLI не владеет архитектурой, а только вызывает публичный API.

## 4. Инварианты уровня ядра

1. Страница либо проходит проверку формата и checksum, либо считается повреждённой.
2. WAL record имеет LSN и достаточную информацию для recovery-сценария своего milestone.
3. Commit не считается durable, пока нужные данные не прошли через требуемую sync-границу.
4. Catalog changes восстанавливаются тем же механизмом, что и пользовательские данные.
5. SQL-ошибка не должна превращаться в corruption или internal invariant violation.
6. Расширение не может нарушить память ядра через нестабильный Rust ABI.
7. Android/Linux/Windows различия изолируются за VFS и feature-флагами.

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
