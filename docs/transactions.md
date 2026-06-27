# Transactions v0

## 1. Цель Этап 6

Transactions v0 нужны, чтобы catalog/heap/index изменения проходили через один понятный commit protocol:

```text
изменить страницы в памяти → записать WAL → sync WAL → записать data file
```

Это не полноценные SQL transactions и не MVCC. Это первый слой, который делает storage operations recoverable.

## 2. Основные сущности

`TransactionalStore` владеет:

- `CatalogStore`;
- `WalWriter`;
- счётчиком `TxId`;
- флагом active writer.

`Transaction` содержит:

- `tx_id`;
- рабочую копию catalog;
- dirty-page staging map;
- состояние `Active/Committed/RolledBack`.

## 3. Dirty-page staging

Все изменённые страницы сначала лежат в памяти:

```text
PageId -> Page
```

В эту map попадают:

- catalog page;
- heap pages;
- index pages;
- новые страницы, выделенные во время операции.

Пока transaction не committed, data file не должен видеть эти pages.

## 4. Commit

Commit делает:

```text
1. для каждой dirty page записать WAL PageImage(tx_id, page);
2. записать WAL CommitTx(tx_id);
3. sync_data WAL;
4. записать dirty pages в data file;
5. sync_data data file;
6. заменить committed catalog в памяти;
7. освободить active writer.
```

Главная гарантия Этап 6:

```text
если data file получил страницу после commit, WAL уже содержит committed image этой страницы
```

## 5. Rollback

Rollback делает:

```text
1. записать AbortTx;
2. sync_data WAL;
3. очистить dirty pages;
4. освободить active writer.
```

Physical undo не нужен, потому что uncommitted dirty pages не писались в data file.

## 6. Autocommit helpers

Для текущего SQL subset используются autocommit операции:

```text
create_table_autocommit
insert_row_autocommit
create_index_autocommit
load_extension_autocommit
```

Они открывают transaction, выполняют одно действие и commit-ят его.

SQL-команды `BEGIN`, `COMMIT`, `ROLLBACK` пока не поддержаны.

## 7. Индексы в транзакциях

B+Tree index pages stage-ятся так же, как heap pages.

При `CREATE INDEX`:

```text
создаётся index relation;
выделяется root page;
строится пустой B+Tree root;
существующие heap rows вставляются в index;
root metadata сохраняется в catalog.
```

При `INSERT`:

```text
строка вставляется в heap;
для подходящих indexes создаются index entries;
изменённые index pages попадают в dirty-page map;
commit пишет heap/catalog/index pages через WAL.
```

## 8. Ограничения

Пока нет:

- нескольких writer-ов;
- SQL-visible transaction statements;
- isolation levels;
- MVCC;
- locks/latches;
- deadlock detection;
- savepoints;
- undo log;
- group commit;
- background checkpoint.

Drop активной transaction освобождает writer flag, но не делает полноценный rollback protocol для частично выполненных операций. Поэтому публичный API должен явно завершать transaction.

## 9. Что проверять дальше

Нужны тесты:

- rollback не меняет catalog/heap/index;
- committed insert восстанавливается после reopen;
- uncommitted WAL page images не применяются;
- index split recoverable;
- ошибка во время commit не оставляет store в ложном successful состоянии;
- второй writer блокируется, пока первый active.
