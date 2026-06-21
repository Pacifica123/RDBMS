# Index v0

Stage 8 добавляет первый persistent index layer.

Индекс v0 — это B+Tree поверх уже существующих страниц. Он не является отдельным файлом: index pages лежат в том же database file, что catalog и heap pages.

```text
catalog page 0
  |
  +-- table users
  |     heap pages = [1, 2, ...]
  |
  +-- index users_id_idx
        table_id = users
        column = id
        root_page_id = N

index root page N
  |
  +-- internal/leaf B+Tree nodes
```

## Что реализовано

```text
rdbms_index crate;
PageType::Index;
B+Tree node format v0;
leaf pages with (key, RowId) entries;
internal pages with separator keys and child page ids;
leaf split;
internal split;
root split;
equality lookup;
INT and TEXT keys;
CREATE INDEX name ON table(column);
INSERT maintenance for existing indexes;
SELECT ... WHERE indexed_column = literal uses the index when possible.
```

## Что не реализовано

```text
unique indexes;
range scans;
delete from index;
UPDATE/DELETE SQL;
composite keys;
DOUBLE indexes;
NULL index entries;
index-only scans;
planner cost model;
background index build;
concurrent index build;
MVCC visibility checks in index entries.
```

## Physical format

Index node payload is stored as one record in slot 0 of a `PageType::Index` page.

```text
magic        = "RDBI"
version      = 1
node_kind    = leaf | internal
payload      = node-specific bytes
```

Leaf node:

```text
next_leaf flag
next_leaf page id
entry count
repeated entries:
  key
  row page id
  row slot id
```

Internal node:

```text
key count
separator keys
child count
child page ids
```

The current implementation uses small `MAX_KEYS` to force splits in unit tests. This is intentional for Stage 8 and not a performance target.

## Catalog integration

Catalog now has index relations:

```text
RelationKind::Index
StorageObject::BPlusTree
```

Index storage metadata contains:

```text
table_id
column_name
root_page_id
```

When the B+Tree root splits, catalog root metadata is updated in the same transaction.

## Transaction integration

Index pages are staged through the same transaction dirty-page map as catalog and heap pages.

Commit still follows the Stage 6 rule:

```text
write WAL PageImage records
write CommitTx
sync WAL
write data pages
sync data file
```

Rollback drops staged index pages together with other dirty pages.

## SQL integration

Supported syntax:

```sql
CREATE INDEX users_id_idx ON users(id);
```

Supported indexed lookup:

```sql
SELECT name FROM users WHERE id = 2;
```

The SQL executor still applies the WHERE predicate after reading candidate rows. This keeps the result correct even if the index path is not used or if a future stale-entry case appears.
