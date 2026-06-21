# Catalog and heap table v0

## 1. Назначение

Stage 5 добавляет первый persistent catalog и минимальную heap table API. Это всё ещё внутренний слой, без SQL, транзакций, индексов и схемного binder-а.

Текущий поток:

```text
open PageFile через VFS;
load_or_bootstrap catalog page 0;
create_table пишет catalog record и первый heap page;
insert_row пишет raw row bytes в slotted heap page;
full_scan возвращает live row bytes в heap-page order.
```

## 2. Catalog page

Page 0 зарезервирована под системный каталог:

```text
page_id = 0
page_type = Catalog
record slot 0 = encoded catalog record v0
```

Если database file пустой, `CatalogStore::open` создаёт пустой catalog page. Если файл короче одной страницы, но не пустой, это считается corruption.

Catalog record v0 начинается с:

```text
magic = "RDBC"
version = 1
next_relation_id: u64
next_page_id: u64
relation_count: u32
```

Дальше идут relation entries. У каждого relation entry есть:

```text
relation_id: u64
relation_kind: u8
storage_object
name: string16
column_count: u16
columns...
```

Строка `string16` кодируется как `u16 byte_len + UTF-8 bytes`.

## 3. Relation storage object

Stage 5 фиксирует первую связь:

```text
RelationId -> StorageObject::Heap { pages: Vec<PageId> }
```

Heap pages не обязаны быть непрерывными. Это намеренно: без allocator/free-space map проще безопасно добавлять новую страницу через глобальный `next_page_id`, не рискуя пересечься с другой relation.

## 4. Heap table v0

Heap table хранит только raw row bytes. Схема пока сохраняется как metadata, но не используется для проверки payload.

Реализовано:

```text
CatalogStore::create_table(name, columns) -> RelationId;
CatalogStore::insert_row(relation_id, bytes) -> RowId;
CatalogStore::full_scan(relation_id) -> Vec<HeapRow>;
reopen schema test;
additional heap page allocation when current pages are full.
```

`RowId` остаётся физическим адресом:

```text
RowId = (PageId, SlotId)
```

Стабильность `SlotId` обеспечивается слоем `rdbms_page`.

## 5. Ограничения

Stage 5 не реализует:

```text
SQL CREATE TABLE;
SQL INSERT/SELECT;
record schema encoding;
type checking;
NULL bitmap;
free-space map;
page allocator;
transactional catalog changes;
WAL protocol for create_table/insert_row;
rollback;
indexes;
system tables as normal queryable relations.
```

Catalog changes сейчас durable только как обычные page writes. Полная crash-consistency будет доводиться вместе с transaction layer и WAL protocol.
