# WAL — журнал предзаписи

## 1. Зачем нужен WAL

WAL нужен, чтобы база могла восстановить committed изменения после сбоя. Идея простая:

```text
сначала записать намерение/образ в журнал → надёжно сбросить журнал → потом писать data file
```

Если процесс упал между этими шагами, recovery читает WAL и доводит data file до committed состояния.

## 2. Что реализовано сейчас

`rdbms_wal` даёт WAL v0:

- бинарный envelope записи;
- `Lsn` как offset в WAL file;
- append-only writer;
- sequential reader;
- checksum и проверку длины;
- обнаружение truncated suffix;
- записи `BeginTx`, `PageImage`, `CommitTx`, `AbortTx`, `Checkpoint`;
- helper для redo committed page images.

WAL v0 хранит full-page images. Это не самый эффективный формат, но он простой и хорошо подходит для раннего correctness-контурa.

## 3. Типы записей

```text
BeginTx     начало транзакции
PageImage   полный образ страницы, созданный транзакцией
CommitTx    транзакция считается committed
AbortTx     транзакция отменена
Checkpoint  marker для будущего checkpoint protocol
```

`PageImage` содержит `tx_id`, `page_id` и полный массив байт страницы.

## 4. Commit protocol в Этап 6+

Transactions v0 используют такой порядок:

```text
1. собрать dirty pages в памяти;
2. записать PageImage для каждой dirty page;
3. записать CommitTx;
4. вызвать sync_data для WAL;
5. записать dirty pages в data file;
6. вызвать sync_data для data file.
```

До шага 4 data file не должен получать dirty pages этой транзакции.

## 5. Что считает recovery

Recovery применяет только page images тех `tx_id`, для которых в WAL найден `CommitTx`.

Если есть `PageImage`, но нет `CommitTx`, этот image игнорируется.

Это правило защищает от ситуации, когда незавершённая транзакция появилась в WAL, но не должна стать видимой после reopen.

## 6. Что проверяет reader

WAL reader проверяет:

- magic;
- version;
- длину payload;
- checksum;
- соответствие LSN текущему offset;
- что запись не обрезана посередине.

Повреждённый WAL возвращает ошибку, а не молча пропускается.

## 7. Ограничения текущего WAL

Пока нет:

- WAL file header;
- checkpoint с позицией recovery;
- truncation после checkpoint;
- pageLSN redo skip;
- logical records;
- partial-page redo;
- group commit;
- segment rotation;
- sync policy tuning.

Также full-page images дорогие по размеру. Это допустимо для раннего этапа, но не для производительного storage engine.

## 8. Что должно появиться дальше

Ближайшие улучшения:

- database/WAL headers с version и compatibility policy;
- checkpoint state;
- использование `page_lsn`;
- crash tests с fault-injection VFS;
- более мелкие WAL records для heap/index операций;
- понятная политика обрезки WAL.
