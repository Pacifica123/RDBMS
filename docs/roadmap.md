# План развития RDBMS

## Принцип

Проект развивается от проверяемого storage correctness, а не от ширины SQL-синтаксиса.

Основная цепочка:

```text
байты → страницы → VFS/page store → WAL → recovery → catalog → heap table → transactions → SQL subset → index → extensions → platform ports
```

## Текущая точка

Реализованы этапы 0–10. Текущий верхний слой — маленький SQL subset с persistent heap tables, B+Tree equality indexes, static extensions и платформенные smoke-проверки.

Проект всё ещё находится в учебно-инженерной стадии. Следующие этапы должны усиливать корректность, тестирование и понятность API, а не резко расширять SQL.

После MVP-клиента добавлен более точный версионный план: `docs/development/client_research_version_plan.md`. Он уточняет, что перед checkpoint и performance-работой нужно закрыть WAL/tx/recovery safety baseline: transaction identity после reopen, safe open path и crash assumptions.

## Stage 0 — architecture-first reboot

Статус: сделано.

Смысл:

- отказаться от продолжения старого `RDBMS-master` как кодовой базы;
- сохранить его как legacy/reference;
- зафиксировать архитектурный порядок;
- создать workspace с crates и документацией.

## Stage 1 — page primitives

Статус: сделано.

Результат:

- фиксированный `PAGE_SIZE`;
- `PageId`, `SlotId`, `RowId`;
- page header;
- checksum;
- slotted page;
- insert/read/delete marker/compact;
- tests для page invariants.

Ограничения:

- нет long records;
- нет overflow pages;
- нет compression.

## Stage 2 — VFS/page store

Статус: сделано.

Результат:

- `Vfs` и `VfsFile`;
- `StdVfs`;
- random-access `read_at/write_at`;
- `sync_data`;
- `PageFile`;
- mapping `PageId -> offset`;
- reopen tests.

Ограничения:

- нет fault injection;
- нет file locking;
- нет async IO.

## Stage 3 — WAL skeleton

Статус: сделано.

Результат:

- WAL record envelope;
- LSN allocator;
- append-only writer;
- sequential reader;
- checksum/length checks;
- truncated suffix detection;
- `PageImage` record.

Ограничения:

- нет WAL header;
- нет checkpoint;
- full-page images only.

## Stage 4 — recovery skeleton

Статус: сделано.

Результат:

- `rdbms_recovery`;
- `open_database`;
- WAL scan при open;
- redo committed page images;
- ignore uncommitted images;
- idempotent recovery tests.

Ограничения:

- redo-only;
- нет undo;
- нет checkpoint state;
- нет pageLSN skip.

## Stage 5 — catalog and heap table v0

Статус: сделано.

Результат:

- catalog page 0;
- `RelationInfo`;
- columns metadata;
- heap storage object;
- create table;
- raw row insert;
- full scan;
- page allocation через catalog.

Ограничения:

- нет SQL schema constraints;
- нет namespaces;
- нет catalog migrations.

## Этап 6 — transactions v0

Статус: сделано.

Результат:

- `TransactionalStore`;
- `begin/commit/rollback`;
- dirty-page staging;
- WAL commit protocol;
- autocommit helpers;
- recovery committed catalog/heap rows.

Ограничения:

- один writer;
- нет SQL-visible transactions;
- нет MVCC;
- rollback только отбрасывает staged pages.

## Stage 7 — SQL subset

Статус: сделано.

Результат:

- parser маленького subset;
- SQL row encoding;
- `CREATE TABLE`;
- `INSERT INTO ... VALUES ...`;
- `SELECT *`;
- `SELECT column list`;
- `WHERE column = literal`;
- direct executor поверх `TransactionalStore`.

Ограничения:

- нет binder/optimizer;
- нет prepared statements;
- нет `UPDATE/DELETE/JOIN`.

## Этап 8 — index v0

Статус: сделано.

Результат:

- `rdbms_index`;
- persistent B+Tree nodes в `PageType::Index`;
- integer/text keys;
- insert и equality lookup;
- node split и root split;
- catalog metadata для index relations;
- SQL `CREATE INDEX`;
- indexed lookup для `WHERE column = literal`.

Ограничения:

- нет delete;
- нет unique/range/composite indexes;
- нет MVCC visibility в index entries.

## Этап 9 — extension v0

Статус: сделано.

Результат:

- `rdbms_ext_abi` ABI sketch;
- `rdbms_extension` static registry;
- ABI version check;
- built-in `stdlib`;
- `upper(TEXT)` и `length(TEXT)`;
- SQL `LOAD EXTENSION`;
- catalog metadata для installed extensions;
- scalar `SELECT function(literal, ...)`.

Ограничения:

- нет dynamic plugin loading;
- нет WASM sandbox;
- нет table/aggregate functions.

## Этап 10 — platform ports

Статус: сделано.

Результат:

- Windows path/sync smoke;
- Android `rdbms_android` crate;
- JNI-shaped smoke symbols;
- Java wrapper `NativeSmoke`;
- CI matrix для Linux/macOS/Windows;
- Android aarch64 library build в CI.

Ограничения:

- нет Android app;
- нет emulator/device tests;
- нет SQL JNI API;
- нет platform crash matrix.

## Этап 11 — test hardening

Статус: следующий разумный этап.

Цель:

- fault-injection VFS;
- crash tests для WAL/recovery/tx/index;
- property tests для page/index/catalog invariants;
- differential tests для поддержанного SQL subset против SQLite там, где semantics совпадает;
- больше reopen/recovery scenarios.

## Этап 12 — SQL DML expansion

Цель:

- `DELETE`;
- `UPDATE`;
- basic `DROP TABLE`;
- SQL-visible `BEGIN/COMMIT/ROLLBACK`;
- prepared statements;
- better type checking.

Делать только после усиления тестов recovery.

## Этап 13 — checkpoint and WAL policy

Цель:

- database/WAL headers;
- checkpoint format;
- pageLSN redo skip;
- WAL truncation;
- recovery start position;
- sync policy documentation.

## Этап 14 — concurrency and MVCC research

Цель:

- reader/writer policy;
- snapshots;
- visibility metadata;
- lock/latch design;
- deadlock story;
- index visibility integration.

## Этап 15 — real extension boundary

Цель:

- выбрать native ABI или WASM;
- описать ownership/error/value ABI;
- реализовать loader за feature flag;
- добавить security policy;
- добавить platform tests.

## Что не делать раньше времени

Не стоит раньше Этап 11–13 делать:

- большой SQL parser;
- network server;
- ORM layer;
- сложный optimizer;
- dynamic plugins по умолчанию;
- обещания file-format compatibility;
- производительные benchmarks как главный критерий.
