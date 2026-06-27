# Индекс B+дерево v0

## 1. Зачем нужен индекс

Без индекса запрос вида:

```sql
SELECT name FROM users WHERE id = 10;
```

должен просканировать все строки таблицы. Индекс хранит отдельную структуру:

```text
key -> RowId
```

Тогда executor может быстро найти физические адреса строк, где `key = 10`, и прочитать только эти строки.

## 2. Что реализовано

`rdbms_index` реализует маленькое B+Tree:

- leaf и internal nodes;
- ordered keys;
- insert `(key, RowId)`;
- split leaf/internal nodes;
- root split;
- linked leaves через `next_leaf`;
- equality lookup;
- хранение node record внутри `PageType::Index` страницы.

Поддержанные key types:

```text
INT / INTEGER -> IndexKey::Integer
TEXT          -> IndexKey::Text
```

## 3. Что такое B+Tree простыми словами

B+Tree — это дерево, где:

- internal pages помогают выбрать путь вниз;
- leaf pages хранят реальные `(key, RowId)` пары;
- все leaf pages находятся на одном уровне;
- leaf pages связаны между собой ссылкой на следующую leaf page.

Пример:

```text
          [10 | 20]
         /    |    \
 [1,5,7] [10,15] [20,25,30]
```

Если ищем `15`, идём через root в среднюю leaf page и читаем entries с ключом `15`.

## 4. Почему это B+Tree, а не обычное B-Tree

В B+Tree реальные row pointers лежат только в leaves. Internal nodes хранят separator keys и child page ids.

Это удобно для базы данных:

- leaves можно связать между собой для будущих range scans;
- internal pages остаются компактнее;
- lookup всегда заканчивается в leaf page;
- index entry хранит физический `RowId`.

Range scans пока не реализованы, но `next_leaf` уже есть.

## 5. Физический формат

Каждая index page — обычная страница `rdbms_page::Page` с типом `PageType::Index`.

В slot 0 лежит один encoded B+Tree node:

```text
magic = RDBI
version = 1
node_kind = leaf/internal
payload
```

Leaf payload:

```text
next_leaf: Option<PageId>
entries: [(IndexKey, RowId), ...]
```

Internal payload:

```text
keys: [IndexKey, ...]
children: [PageId, ...]
```

## 6. Insert

Вставка идёт сверху вниз:

```text
1. найти leaf page для key;
2. вставить `(key, RowId)` в отсортированное место;
3. если page переполнена — split;
4. поднять separator key родителю;
5. если root split-ится — создать новый root.
```

`MAX_KEYS` сейчас маленький специально, чтобы тесты быстро проверяли split. Это не настройка производительности.

## 7. Lookup

Equality lookup:

```text
1. от root пройти к leaf page;
2. читать entries в leaf;
3. собрать все RowId, где key равен искомому;
4. при необходимости перейти в next_leaf;
5. остановиться, когда ключи стали больше искомого или leaf закончились.
```

Lookup возвращает `Vec<RowId>`.

## 8. Catalog integration

Index relation хранится в catalog:

```text
RelationKind::Index
StorageObject::BPlusTree {
  table_id,
  column_name,
  root_page_id
}
```

Если root split-ится, новый `root_page_id` обновляется в catalog в той же transaction.

## 9. Transaction integration

Index pages проходят через общий dirty-page staging:

```text
index page changed -> dirty_pages -> WAL PageImage -> CommitTx -> sync WAL -> data file
```

Rollback выбрасывает staged index pages вместе с catalog/heap pages.

## 10. SQL integration

SQL-команда:

```sql
CREATE INDEX users_id_idx ON users(id);
```

создаёт index relation и строит B+Tree по существующим строкам.

После этого запрос:

```sql
SELECT name FROM users WHERE id = 1;
```

может использовать index lookup, если predicate подходит под `column = literal` и тип literal можно превратить в `IndexKey`.

## 11. Ограничения

Пока нет:

- delete из индекса;
- unique index;
- range scan в SQL;
- composite keys;
- NULL entries;
- index-only scan;
- background/concurrent build;
- MVCC visibility;
- page fill factor;
- rebalancing/merge после delete;
- logical rebuild during recovery.

## 12. Что проверять дальше

Нужны тесты:

- много duplicate keys;
- split root;
- split internal pages;
- lookup через несколько linked leaves;
- recovery после split;
- insert в таблицу с несколькими indexes;
- fallback на full scan, если index не подходит.
