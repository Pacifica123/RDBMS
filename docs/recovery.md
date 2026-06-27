# Восстановление после сбоя

## 1. Цель

Recovery нужно, чтобы после reopen база увидела committed данные и не увидела uncommitted данные.

Минимальные правила:

```text
committed изменения восстанавливаются;
uncommitted изменения не становятся видимыми;
повреждённый WAL или page bytes дают ошибку, а не тихую порчу.
```

## 2. Текущий recovery loop

`rdbms_recovery::open_database` связывает VFS, data file и WAL file.

Порядок работы:

```text
1. открыть data file через VFS;
2. открыть WAL file через VFS;
3. прочитать WAL с offset 0;
4. проверить WAL envelope/checksum/LSN;
5. найти committed tx_id;
6. применить PageImage только для committed tx_id;
7. вернуть PageFile для дальнейшего открытия CatalogStore/TransactionalStore.
```

Это redo-only recovery. Оно не делает undo, не строит MVCC snapshot и не выполняет логическую перестройку таблиц.

## 3. Почему full-page image

WAL v0 пишет полный образ страницы. Поэтому redo простое:

```text
если tx committed → записать этот page image в data file
```

Такой подход проще проверить. Минус — большой WAL и отсутствие оптимизаций.

## 4. Идемпотентность

Recovery можно запускать повторно. Повторная запись того же committed page image должна оставлять тот же page state.

Это важно для сценария:

```text
open database → recovery → process dies → open database again → recovery again
```

## 5. Связь с transactions v0

`rdbms_tx` делает изменения recoverable так:

```text
dirty catalog/heap/index pages живут в памяти;
commit пишет их в WAL как PageImage records;
commit пишет CommitTx;
commit sync-ит WAL;
только потом commit пишет страницы в data file.
```

Если сбой произошёл после sync WAL, но до записи data file, recovery восстановит страницы из WAL.

Если сбой произошёл до `CommitTx`, recovery проигнорирует эти page images.

## 6. Индексы и recovery

Для Этап 8/10 индекс не требует отдельного алгоритма восстановления. Index pages проходят через тот же путь:

```text
B+Tree page → staged dirty page → WAL PageImage → recovery redo
```

После recovery catalog знает root page index relation, а index pages восстановлены как обычные committed pages.

Пока нет логического index rebuild. Если в будущем появятся операции, где heap и index можно чинить отдельно, понадобится отдельный protocol.

## 7. Ограничения

Пока нет:

- undo;
- MVCC visibility;
- checkpoint state;
- pageLSN skip;
- WAL truncation;
- fuzzy checkpoint;
- crash matrix;
- repair tool;
- логического rebuild индексов.

## 8. Что проверять дальше

Нужны crash tests:

- падение до `CommitTx`;
- падение после `CommitTx`, но до data sync;
- обрезанный WAL record;
- повреждённый checksum;
- повторный recovery;
- сбой при index page split;
- сбой между catalog update и index page write.

Такие тесты должны идти через fault-injection VFS, а не через случайные sleep/kill.
