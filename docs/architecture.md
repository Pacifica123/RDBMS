# Архитектура RDBMS

## 1. Назначение проекта

RDBMS — учебно-инженерная СУБД на Rust. Проект нужен не для того, чтобы быстро показать SQL-консоль, а для того, чтобы собрать ядро с понятными границами: хранение, WAL, восстановление, каталог, транзакции, SQL layer, индексы, расширения и переносимость.

Рабочая формула:

```text
ядро СУБД = страницы + VFS + WAL + recovery + catalog + transactions + SQL executor + indexes + extensions + platform boundary
```

Каждый слой должен иметь маленький публичный контракт и тесты. Если слой нельзя проверить отдельно, значит его граница выбрана плохо.

## 2. Почему проект начат заново

Ранний прототип `RDBMS-master` начинался со строкового SQL-диспетчера и `Vec<Table>` в памяти. Такой подход быстро даёт видимый результат, но не отвечает на главные вопросы СУБД:

- где физически лежит строка;
- как найти страницу после reopen;
- как понять, что страница повреждена;
- как восстановить committed данные после сбоя;
- где хранится catalog;
- как отличить committed и uncommitted изменения;
- как перенести IO на Windows/Android без переписывания ядра.

Поэтому новый проект идёт снизу вверх:

```text
байты → страницы → VFS/page store → WAL → recovery → catalog → heap table → transactions → SQL subset → B+Tree index → extensions → platform ports
```

SQL остаётся пользовательским интерфейсом, но не является фундаментом.

## 3. Слои проекта

### `rdbms_core`

Общие типы и ошибки. Здесь находятся `PageId`, `RelationId`, `TxId`, `Lsn`, `RowId`, `DbError`, `DbResult`, `Value`, `ColumnInfo`, `ExecResult`.

Этот crate не должен знать о файлах, WAL, страницах или SQL parser-е.

### `rdbms_vfs`

Слой файловой системы. Он прячет `std::fs` за простым контрактом: открыть файл, читать/писать по offset, узнать длину, вызвать `sync_data`.

Поверх него построен `PageFile`: `PageId(7)` читается и пишется по offset `7 * PAGE_SIZE`.

Этот слой нужен для переносимости и будущих fault-injection tests.

### `rdbms_page`

Физическая страница фиксированного размера. Страница содержит заголовок, тип страницы, checksum, LSN, slot directory и payload-зону.

Сейчас это базовый slotted-page layout: записи можно вставлять, читать по `SlotId`, помечать удалёнными и compact-ить без изменения живых `SlotId`.

### `rdbms_wal`

Журнал предзаписи. WAL v0 хранит бинарные записи с magic/version/checksum/LSN и типом записи.

Текущие типы записей: `BeginTx`, `PageImage`, `CommitTx`, `AbortTx`, `Checkpoint`.

WAL не открывает базу сам и не пишет data file напрямую. Он только даёт устойчивый append-only журнал, который потом читает recovery.

### `rdbms_recovery`

Минимальное восстановление. Оно открывает data file и WAL file через VFS, читает WAL с offset 0 и применяет только committed full-page images.

Это redo-only схема. `UNDO`, checkpoint state, pageLSN-skip и логическая перестройка индексов пока не реализованы.

### `rdbms_catalog`

Persistent catalog и heap table v0. Catalog хранится в page 0 как один encoded record. Он знает relation id, имя relation, kind, columns, heap pages, index root page и установленные static extensions.

Catalog умеет bootstrap, создать таблицу, создать index relation, выделить page id, вставить raw row bytes в heap и просканировать heap.

### `rdbms_tx`

Transactions v0. Слой держит `CatalogStore` и `WalWriter`, даёт `begin`, `commit`, `rollback` и autocommit helpers.

Правило commit:

```text
staged dirty pages → WAL PageImage records → WAL CommitTx → sync WAL → write data pages → sync data file
```

Rollback просто выбрасывает staged pages и пишет `AbortTx`. Physical undo пока нет.

### `rdbms_sql`

Маленький SQL subset поверх `TransactionalStore`.

Поддержано:

```text
CREATE TABLE name (column TYPE, ...)
INSERT INTO name VALUES (literal, ...)
CREATE INDEX name ON table(column)
LOAD EXTENSION stdlib
SELECT function(literal, ...)
SELECT * FROM name [WHERE column = literal]
SELECT column, ... FROM name [WHERE column = literal]
```

Нет binder-а, optimizer-а, prepared statements, SQL transactions, `JOIN`, `UPDATE`, `DELETE` и полноценного operator tree.

### `rdbms_index`

B+Tree index v0. Индекс хранится в обычных `PageType::Index` страницах. Сейчас он поддерживает вставку `(key, RowId)`, split страниц и equality lookup по `INT` и `TEXT` ключам.

Удаления, уникальность, range scan, NULL entries и MVCC visibility ещё не реализованы.

### `rdbms_ext_abi`

Набросок C-compatible ABI для будущих native extensions. Это не runtime loader, а стабильная внешняя форма, которую можно развивать без выдачи Rust trait-ов наружу.

### `rdbms_extension`

Static extension registry v0. Сейчас расширения встроены в бинарь и загружаются по имени. Встроенное расширение `stdlib` даёт scalar functions `upper(TEXT)` и `length(TEXT)`.

Dynamic native plugin loading пока отсутствует.

### `rdbms_android`

Android native-library smoke crate. Он собирается как `rlib` и `cdylib`, экспортирует JNI-shaped функции `stage`, `abiVersion`, `add` и линкуется с SQL/core stack.

Это не Android-приложение и не SQL JNI API.

### `rdbms_cli`

Пока это тонкая CLI-заглушка. Настоящий shell не является ближайшим архитектурным приоритетом.

## 4. Главные инварианты

1. Страница не считается валидной без проверки header/checksum.
2. `SlotId` живой записи не должен меняться после compact.
3. WAL record имеет LSN и проверяемый binary envelope.
4. Recovery применяет только committed page images.
5. Transaction v0 не пишет dirty pages в data file до durable WAL commit marker.
6. SQL error должен оставаться пользовательской ошибкой, а не превращаться в corruption.
7. Index page проходит через тот же staging/WAL путь, что catalog и heap page.
8. Платформенные различия изолируются за VFS и узкими FFI-boundary crate-ами.

## 5. Минимальный публичный API

Текущий пользовательский путь выглядит так:

```rust
let vfs = StdVfs;
let paths = DatabasePaths::new("data.dbonrs", "data.wal");
let page_file = open_database(&vfs, paths)?;
let mut store = TransactionalStore::new(page_file, wal_file)?;

rdbms_sql::execute(&mut store, "CREATE TABLE users (id INT, name TEXT)", &[])?;
rdbms_sql::execute(&mut store, "INSERT INTO users VALUES (1, 'Ada')", &[])?;
let result = rdbms_sql::execute(&mut store, "SELECT name FROM users WHERE id = 1", &[])?;
```

API пока не обещает стабильности для внешних пользователей. Он нужен для закрепления архитектурных слоёв.

## 6. Конкурентность

Текущая модель: many readers + single writer как будущая цель, но Этап 6 фактически даёт один active writer на handle. Это сделано специально, чтобы не смешивать первый storage/recovery stack с полноценным MVCC.

## 7. Граница с legacy

`docs/legacy/RDBMS-master` сохранён как исторический материал. Код оттуда не продолжается. Полезны только термины и ранняя предметная модель: database, table, column, row, value.

## 8. Что считать текущей архитектурной вершиной

После Этап 10 верхний слой — маленький SQL subset с persistent heap tables, equality indexes, static extensions и платформенные smoke-проверки. Ниже есть WAL-backed transactions.

Следующий крупный шаг — сделать SQL и storage менее демонстрационными: добавить более сильные транзакционные гарантии, расширить DML, улучшить recovery и начать нормальные crash/differential/property tests.
