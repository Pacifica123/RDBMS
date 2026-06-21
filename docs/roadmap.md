# ROADMAP — порядок развития RDBMS

## Принцип

Проект развивается не от красивого SQL shell, а от проверяемого correctness-контура:

```text
байты → страницы → VFS/page store → WAL → recovery → каталог → heap table → транзакция → индекс → SQL → расширения → переносимость
```

## Stage 0 — architecture-first reboot

Статус: выполнено.

Сделано:

```text
workspace skeleton;
architecture docs;
legacy archive;
devctl patch workflow;
базовые newtype-id и error boundary.
```

## Stage 1 — page primitives

Статус: выполнено.

Цель: получить первый настоящий кирпич storage engine — slotted page.

Сделано:

```text
фиксированный PAGE_SIZE = 4096;
page header v1;
page type;
slot directory;
insert/read/delete variable-size record;
compact без смены live slot id;
checksum validation;
unit tests на основные инварианты.
```

Что это даёт:

```text
строка больше не является абстрактным Vec<Value>;
появляется физический адрес row_id = (page_id, slot_id);
можно строить heap table и индексы поверх стабильных slot id;
можно проверять повреждение страницы до подключения WAL.
```

## Stage 2 — VFS/page store

Статус: выполнено.

Реализовано:

```text
StdVfs поверх std::fs;
read_at/write_at/sync_data;
page file abstraction;
create/open database file;
write_page/read_page;
reopen test;
corrupt page detection after reopen.
```

Запрещено на этом этапе:

```text
SQL;
индексы;
плагины;
server mode.
```

## Stage 3 — WAL skeleton

Статус: выполнено.

Реализовано:

```text
WAL record binary envelope v0;
LSN allocator на byte offsets;
WalWriter/WalReader поверх VfsFile;
commit marker;
truncated WAL detection;
redo hook для committed page image.
```

Ограничения:

```text
нет WAL file header;
нет page_lsn update API;
нет recovery при open database;
нет checkpoint state;
нет fault-injection VFS.
```

## Stage 4 — recovery skeleton

Статус: выполнено.

Реализовано:

```text
rdbms_recovery crate;
DatabasePaths;
open_database через VFS;
scan WAL при open;
redo committed page images в PageFile;
ignore uncommitted page images;
idempotent recovery test;
propagate WAL corruption during recovery.
```

Ограничения:

```text
нет undo;
нет checkpoint state;
нет page_lsn-based skip;
нет commit protocol между WAL и data file;
нет fault-injection VFS;
нет catalog/bootstrap database header.
```

## Stage 5 — catalog and heap table v0

Статус: выполнено.

Реализовано:

```text
bootstrap catalog page 0;
relation_id → heap storage object;
internal create_table API;
insert raw row bytes;
full scan;
reopen schema test;
heap page extension when current pages are full.
```

Ограничения:

```text
нет SQL CREATE TABLE/INSERT/SELECT;
нет record schema encoding;
нет transactional catalog changes;
нет WAL protocol for heap inserts;
нет rollback;
нет indexes.
```

## Stage 6 — transactions v0

Статус: выполнено.

Реализовано:

```text
rdbms_tx crate;
TransactionalStore;
begin/commit/rollback;
autocommit helpers;
single active writer на handle;
transaction-local dirty page staging;
WAL PageImage records before data file writes;
rollback discards uncommitted create_table/insert_row;
recovery test for committed catalog/heap pages from WAL.
```

Ограничения:

```text
нет MVCC;
нет межпроцессного file lock;
нет SQL BEGIN/COMMIT/ROLLBACK;
нет transactional delete/update;
нет persistent TxId allocator;
нет checkpoint/WAL truncation;
нет savepoints.
```

## Stage 7 — SQL subset

SQL начинается только после storage/catalog/transaction skeleton.

```text
parser adapter;
binder;
logical plan;
SeqScan/Filter/Project;
INSERT;
SELECT;
simple WHERE.
```

## Stage 8 — index v0

```text
B+Tree page format;
insert/delete;
equality scan;
range scan;
index verifier.
```

## Stage 9 — extension v0

```text
static scalar function registry;
extension catalog metadata;
ABI version check;
Linux native plugin experiment;
Android static registry path.
```

## Stage 10 — platform ports

```text
Windows path/fsync smoke;
Android library build;
JNI smoke;
CI matrix.
```
