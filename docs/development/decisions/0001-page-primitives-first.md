# Решение 0001 — сначала slotted page

## Решение

Первым практическим слоем после architecture-first reboot реализуется `rdbms_page`: страница фиксированного размера, header, slot directory, insert/read/delete marker/compact и checksum validation.

## Почему не SQL

SQL shell выглядит заметнее, но не даёт гарантий хранения. Для этой СУБД правильный порядок другой:

```text
байты → страницы → WAL → catalog → heap table → transaction → index → query
```

Поэтому проект сначала получает физическую единицу хранения, а не parser команд.

## Инвариант

Живой `SlotId` не меняется после `compact()`.

Это нужно, чтобы будущий `RowId { page_id, slot_id }` оставался физическим адресом записи. Индекс может хранить такой адрес и не ломаться от compact внутри страницы.

## Последствия

После этого решения можно строить:

- `PageFile` поверх VFS;
- WAL `PageImage`;
- catalog page;
- heap table;
- B+Tree index pages;
- recovery tests для повреждённых страниц.

Это решение остаётся актуальным после Этап 10: все верхние слои всё ещё опираются на page/slot boundary.
