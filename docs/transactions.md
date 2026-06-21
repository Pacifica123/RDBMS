# Transactions v0

## 1. Назначение

Stage 6 добавляет первый transaction boundary поверх catalog/heap v0 и WAL v0.

Это не MVCC и не полноценный SQL transaction manager. Цель ниже:

```text
один writer на handle;
begin / commit / rollback;
autocommit helpers;
commit пишет full-page images в WAL до data file;
rollback отбрасывает staged catalog/heap pages;
recovery может восстановить committed catalog/heap pages из WAL.
```

## 2. Граница слоя

Новый crate:

```text
rdbms_tx
```

Основные типы:

```text
TransactionalStore<F: VfsFile>;
Transaction<'a, F>;
TransactionState;
open_transactional_store(vfs, data_path, wal_path).
```

`TransactionalStore` владеет двумя файлами:

```text
CatalogStore<F>  -> database file;
WalWriter<F>     -> WAL file.
```

## 3. Модель writer-а

Stage 6 принимает простое правило:

```text
на одном TransactionalStore может быть только одна активная write-транзакция.
```

Это enforced API-границей: `begin()` берёт `&mut self` и возвращает `Transaction`, который держит mutable borrow manager-а до `commit`, `rollback` или drop.

Это ещё не межпроцессный lock и не глобальная блокировка файла. Несколько процессов, открывших один и тот же файл, Stage 6 не координирует.

## 4. Staging

Транзакция не пишет dirty pages в data file сразу.

При `begin()` создаётся transaction-local копия catalog snapshot:

```text
working_catalog = committed catalog clone
```

При `create_table`:

```text
обновить working_catalog;
создать staged heap page;
создать staged catalog page.
```

При `insert_row`:

```text
читать heap page из dirty map или committed PageFile;
вставить row bytes в in-memory page;
положить page в dirty map;
если места нет — выделить новую page id в working_catalog и обновить staged catalog page.
```

Dirty pages хранятся в памяти:

```text
BTreeMap<PageId, Page>
```

## 5. Commit protocol v0

`commit()` делает:

```text
1. append PageImage(tx_id, page) для каждой dirty page;
2. append CommitTx(tx_id);
3. sync WAL file;
4. write dirty pages в PageFile;
5. sync data file;
6. заменить committed in-memory catalog snapshot на working_catalog.
```

Ключевое свойство Stage 6: data pages не пишутся до durable commit marker в WAL.

Если процесс падает после WAL sync, но до data write, Stage 4 recovery может восстановить committed full-page images из WAL.

## 6. Rollback protocol v0

`rollback()` делает:

```text
1. append AbortTx(tx_id);
2. sync WAL file;
3. очистить dirty pages;
4. не менять committed catalog snapshot;
5. не писать data file.
```

Так как Stage 6 использует no-steal staging, rollback не обязан делать physical undo в data file: uncommitted pages туда не попадали.

Если `Transaction` был dropped без явного rollback, staged pages тоже не попадают в data file. Abort WAL marker при drop не записывается, потому что `Drop` не может вернуть `DbResult`.

## 7. Autocommit

Autocommit helpers — это thin wrappers:

```text
create_table_autocommit = begin + create_table + commit;
insert_row_autocommit  = begin + insert_row + commit.
```

Они нужны до появления SQL executor-а, чтобы higher layer мог выполнять одиночные операции без ручного управления transaction handle.

## 8. Связь с recovery

Recovery не меняет свой формат. Оно уже умеет читать WAL v0 и применять committed `PageImage` records.

Stage 6 проверяет сценарий:

```text
commit записал WAL;
data file потерян/не содержит committed pages;
open_database(vfs, paths) применяет WAL;
CatalogStore::open видит committed catalog;
full_scan возвращает committed row bytes.
```

## 9. Текущие ограничения

Сейчас нет:

```text
MVCC;
read snapshots;
межпроцессный file lock;
несколько concurrent writers;
SQL BEGIN/COMMIT/ROLLBACK;
undo log;
savepoints;
deadlock detection;
transactional delete/update;
free-space reclamation on rollback;
WAL truncation/checkpoint;
stable TxId persisted across reopen.
```

`TxId` пока выдаётся с `1` при каждом open `TransactionalStore`. Это допустимо для skeleton-а, но должно быть заменено persistent allocator-ом до реальной эксплуатации.

## 10. Связь с SQL subset v0

Stage 7 использует `TransactionalStore` как write boundary для SQL statements:

```text
CREATE TABLE -> create_table_autocommit;
INSERT INTO ... VALUES -> insert_row_autocommit;
SELECT -> committed full_scan.
```

SQL-level `BEGIN`, `COMMIT` и `ROLLBACK` пока не реализованы. Поэтому каждое SQL write statement в Stage 7 является отдельной autocommit transaction.

## Stage 8 — index pages in transactions

B+Tree pages are staged exactly like heap and catalog pages.

```text
begin transaction
  create/modify index pages in dirty page map
  update catalog root page when root split happens
commit
  WAL PageImage records for dirty catalog/heap/index pages
  CommitTx
  WAL sync
  data writes
  data sync
```

Rollback discards staged index pages. There is still no MVCC, no delete maintenance and no index visibility map.
