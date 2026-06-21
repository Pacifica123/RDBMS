# WAL

## 1. Назначение

WAL нужен не “для галочки”, а для восстановления после сбоя. Если страница могла быть частично записана или данные могли попасть на диск не в том порядке, recovery должен иметь достаточно информации для приведения базы к согласованному состоянию.

## 2. Правило

```text
Write-Ahead Logging: log record должен быть durable раньше страницы данных, которую он описывает.
```

## 3. Базовые понятия

```text
LSN      — позиция записи в WAL
page_lsn — последний LSN, применённый к странице
redo     — повторить изменение, если оно ещё не отражено на странице
undo     — отменить изменение незавершённой транзакции; не MVP первой итерации
```

В WAL v0 `LSN` — это byte offset начала record header в WAL-файле. Первый record получает `Lsn(0)`, следующий record получает offset сразу после предыдущего encoded record.

## 4. WAL file v0

WAL v0 — append-only поток независимых records. Общего file header пока нет. Reader сканирует файл с offset `0` до `file_len`. Если в конце файла остаётся неполный header или неполный payload, это `DbError::Corruption`, а не молчаливый EOF.

Формат record header:

```text
offset  size  field
0       4     magic = "RDBW"
4       2     version = 1
6       2     kind
8       8     lsn
16      8     tx_id, u64::MAX если не применяется
24      8     page_id, u64::MAX если не применяется
32      4     payload_len
36      4     checksum
```

Все числа пишутся little-endian. `WAL_HEADER_SIZE = 40`.

Checksum пока простой: сумма байтов с wraparound. Поле checksum при расчёте считается нулевым. Это слабый алгоритм, но он фиксирует саму границу проверки WAL corruption до выбора настоящего checksum.

## 5. Record kinds v0

```text
1 = BeginTx    { tx_id }
2 = PageImage  { tx_id, page_id, payload = PAGE_SIZE bytes }
3 = CommitTx   { tx_id }
4 = AbortTx    { tx_id }
5 = Checkpoint
```

`PageImage` хранит полный serialized page image из `rdbms_page`. Payload должен иметь размер ровно `PAGE_SIZE`. Во время redo hook image дополнительно проходит `Page::from_bytes`, то есть проверяются page magic, версия, slot boundaries и page checksum.

## 6. Writer/reader boundary

`rdbms_wal` работает поверх `rdbms_vfs::VfsFile`:

```text
WalWriter<F: VfsFile>
WalReader<F: VfsFile>
LsnAllocator
```

Для корректного сканирования WAL в `VfsFile` есть `len()`. Без длины файла нельзя отличить clean EOF на границе record от обрезанного suffix.

`WalWriter::append` назначает LSN, кодирует record, пишет bytes по offset `lsn.0`. `WalWriter::sync_data` даёт sync boundary для будущего commit protocol. Stage 3 только предоставляет эту границу; полноценное правило “commit durable после WAL sync” будет собрано в transaction/recovery слое.

## 7. Redo hook

Stage 3 не реализует recovery loop. Он предоставляет только hook:

```text
PageImageRedo::redo_page_image(lsn, tx_id, page_id, image)
redo_committed_page_images(records, redo)
```

Hook replay-ит только page images транзакций, у которых есть `CommitTx` и нет `AbortTx`. Checkpoint, file bootstrap, idempotent database open и применение к `PageFile` остаются Stage 4.

## 8. Что не делаем сразу

Не начинаем с полного ARIES, nested top actions, сложного checkpointing и fine-grained undo. Сначала нужен воспроизводимый маленький recovery loop.

Сейчас ещё нет:

```text
WAL file header;
checkpoint state;
page_lsn update API;
commit protocol между WAL и data file;
fault-injection VFS;
recovery при Database::open.
```

## 9. Тесты

Для Stage 3 зафиксированы unit tests:

```text
binary envelope round-trip;
LSN allocator offset progression;
writer append + reader scan commit marker;
truncated WAL suffix detection;
redo hook replays only committed page images.
```

Следующие recovery milestones должны добавить crash tests:

1. сбой до записи WAL;
2. сбой после WAL до data page;
3. сбой после частичной data page;
4. сбой после commit;
5. повторный recovery должен быть идемпотентным.
