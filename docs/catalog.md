# Catalog and heap table v0

## 1. Назначение

Stage 5 добавляет первый persistent catalog и минимальную heap table API. Stage 7 уже умеет использовать этот catalog через маленький SQL subset, но catalog crate сам остаётся внутренним storage-слоем.

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
SQL ALTER/DROP TABLE;
SQL UPDATE/DELETE;
general record schema encoding beyond SQL row v0;
type checking;
NULL bitmap;
free-space map;
page allocator;
transactional direct CatalogStore writes;
WAL protocol inside CatalogStore::create_table/insert_row;
rollback inside direct CatalogStore API;
indexes;
system tables as normal queryable relations.
```

Direct `CatalogStore` changes сейчас durable только как обычные page writes. Для WAL-backed create/insert нужно использовать `rdbms_tx::TransactionalStore`.

## 8. Связь с transactions v0

Stage 6 не меняет catalog record v0 и heap page layout. Он добавляет staging path поверх уже существующего формата.

Для этого `rdbms_catalog` отдаёт несколько низкоуровневых helpers слою `rdbms_tx`:

```text
Catalog::to_page();
Catalog::create_table_metadata();
Catalog::allocate_page_id();
Catalog::append_heap_page();
CatalogStore::replace_catalog().
```

Эти функции нужны не SQL-слою, а transaction manager-у. Он должен уметь собрать новый catalog page image в памяти, записать его в WAL как `PageImage`, а затем установить в data file только после durable commit marker.

Обычные Stage 5 методы `CatalogStore::create_table` и `CatalogStore::insert_row` сохранены для низкоуровневых тестов и прямого storage-доступа. Новый код, которому нужна transaction boundary, должен использовать `rdbms_tx::TransactionalStore`.

## 9. Связь с SQL subset v0

Stage 7 хранит SQL-visible строки как raw heap bytes со своим payload magic `RDBR`. Catalog metadata используется для проверки числа значений, имён колонок и простых SQL type names.

`rdbms_catalog` не парсит SQL и не знает о SQL expressions. Он только хранит relation/column metadata и page ids.

## Index catalog metadata v0

Stage 8 extends catalog storage objects with B+Tree indexes.

```text
RelationKind::Index
StorageObject::BPlusTree
```

The index storage object stores:

```text
table_id      heap table being indexed
column_name   indexed column
root_page_id  current B+Tree root page
```

The catalog is the only owner of the root page id. When root split happens, transaction code updates catalog metadata and marks the catalog page dirty.

Stage 8 supports one-column indexes only. Duplicate indexes on the same table column are rejected by catalog metadata checks.
