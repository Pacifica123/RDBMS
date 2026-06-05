# FORMAT — физический формат RDBMS

## Статус

Документ описывает текущий экспериментальный формат страницы. Это не обещание вечной совместимости. До появления WAL/recovery формат можно менять, но каждое изменение должно быть явно отражено здесь и в тестах `rdbms_page`.

## Базовая единица хранения

Минимальная единица физического хранения — страница фиксированного размера.

```text
PAGE_SIZE = 4096 bytes
byte order = little-endian
page_id = u64
lsn = u64
slot_id = u16
```

Сейчас реализован только in-memory page buffer в crate `rdbms_page`. VFS и файл базы будут подключаться следующим слоем.

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

## Что ещё не является форматом БД

Сейчас нет:

```text
file header page;
segment layout;
free-space map;
record schema layout;
WAL binding;
checkpoint state;
catalog bootstrap pages.
```

Следующий практический слой — VFS/page store: создать файл, записать страницу, прочитать страницу, проверить checksum после reopen и обнаружить повреждённую страницу.
