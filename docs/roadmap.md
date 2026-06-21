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

Следующий хороший патч.

Нужно реализовать:

```text
WAL record binary envelope;
LSN allocator;
writer/reader;
commit marker;
truncated WAL detection;
redo hook для page image.
```

## Stage 4 — recovery skeleton

```text
open database;
scan WAL;
redo committed page images;
ignore uncommitted changes;
idempotent recovery test.
```

## Stage 5 — catalog and heap table v0

```text
bootstrap catalog;
relation_id → storage object;
internal create_table API;
insert row bytes;
full scan;
reopen schema test.
```

## Stage 6 — transactions v0

```text
TxId;
autocommit;
begin/commit/rollback;
single writer + many readers;
rollback uncommitted inserts.
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
