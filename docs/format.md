# FORMAT — физический формат RDBMS

## Статус

Документ описывает текущий экспериментальный формат страницы, database file v0 и WAL record v0. Это не обещание вечной совместимости. До полноценного recovery формат можно менять, но каждое изменение должно быть явно отражено здесь и в тестах соответствующего crate.

## Базовая единица хранения

Минимальная единица физического хранения — страница фиксированного размера.

```text
PAGE_SIZE = 4096 bytes
byte order = little-endian
page_id = u64
lsn = u64
slot_id = u16
```

Сейчас реализованы in-memory page buffer в crate `rdbms_page`, первый disk-backed слой в crate `rdbms_vfs` и WAL record stream v0 в crate `rdbms_wal`. VFS/page store записывает и читает страницы через random-access файл.

## Layout страницы v1

```text
[page header][slot directory → ... free space ... ← record bytes]
```

Header занимает 34 байта:

```text
offset  size  field
0       4     magic = "RDBP"
4       2     version = 1
6       2     page_type
8       8     page_id
16      8     page_lsn
24      2     free_start
26      2     free_end
28      2     slot_count
30      4     checksum
```

`free_start` всегда равен `HEADER_SIZE + slot_count * SLOT_SIZE`. Это упрощает первый вариант: каталог слотов растёт только вправо, а payload-зона растёт слева от конца страницы.

## Slot directory

Один slot занимает 6 байт:

```text
offset  size  field
0       2     record_offset
2       2     record_len
4       2     flags
```

Флаги:

```text
0 = unused
1 = live
2 = dead
```

`row_id = (page_id, slot_id)` остаётся стабильным для живой записи даже после `compact()`: payload может быть переложен внутри страницы, но номер слота не меняется.

## Операции страницы

Реализовано:

```text
Page::new(page_id, page_type)
Page::from_bytes(bytes)
Page::insert_record(bytes) -> SlotId
Page::read_record(slot_id) -> Option<&[u8]>
Page::delete_record(slot_id) -> bool
Page::compact()
Page::validate()
```

`delete_record` не стирает payload немедленно. Он помечает slot как dead. `compact()` перепаковывает только живые записи и освобождает дырки.

## Checksum

Checksum пока простой: сумма байтов с wraparound. Поле checksum при расчёте считается нулевым.

Это слабый алгоритм. Его назначение сейчас — зафиксировать саму границу проверки повреждений. Перед реальным recovery нужно заменить алгоритм на более сильный и явно описать совместимость формата.


## Database file v0

Первый файл базы — это простой массив страниц фиксированного размера. Страница с `page_id = N` хранится по смещению:

```text
offset = N * PAGE_SIZE
```

На этом этапе ещё нет allocator, free-space map, file header bootstrap и catalog bootstrap. `PageType::FileHeader` уже зарезервирован в формате страницы, но `rdbms_vfs` пока не создаёт специальную header page автоматически.

`PageFile::write_page` перед записью проверяет checksum и совпадение `PageId` в заголовке. `PageFile::read_page` читает ровно `PAGE_SIZE` байт, строит `Page::from_bytes`, проверяет checksum и отдельно проверяет, что запрошенный `PageId` совпал с `page_id` в заголовке. Повреждённая страница после reopen должна возвращать `DbError::Corruption`.

## WAL record v0

WAL v0 — append-only поток records без общего file header. Один record состоит из fixed header и payload:

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

`WAL_HEADER_SIZE = 40`. Все числа little-endian. В WAL v0 `LSN` равен byte offset начала record header.

Record kinds:

```text
1 = BeginTx
2 = PageImage, payload = PAGE_SIZE bytes
3 = CommitTx
4 = AbortTx
5 = Checkpoint
```

Reader обязан обнаруживать обрезанный suffix: неполный header или payload в конце WAL возвращает `DbError::Corruption`.

## Что ещё не является форматом БД

Сейчас нет:

```text
file header bootstrap;
segment layout;
free-space map;
record schema layout;
WAL file header;
page_lsn update API;
checkpoint state;
catalog bootstrap pages.
```

Следующий практический слой — recovery skeleton: сканировать WAL, применять committed page images и сделать повторный recovery идемпотентным.
