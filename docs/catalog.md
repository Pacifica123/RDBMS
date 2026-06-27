# Catalog и heap table v0

## 1. Роль catalog

Catalog — это системная таблица проекта. Он хранит metadata, без которой data file не имеет смысла:

- какие relations существуют;
- какой у них `RelationId`;
- какие columns у таблицы;
- какие heap pages принадлежат таблице;
- где root page у B+Tree index;
- какие static extensions установлены.

В текущей версии catalog хранится в page 0 как один encoded record.

## 2. Bootstrap

Пустая база начинается с catalog page:

```text
PageId(0) = catalog page
next_relation_id = 1
next_page_id = 1
relations = []
extensions = []
```

`next_page_id` стартует с 1, потому что page 0 занята catalog-ом.

## 3. Relation metadata

Relation описывается так:

```text
RelationInfo {
  id,
  name,
  kind,
  columns,
  storage
}
```

`kind` сейчас бывает:

```text
Table
Index
System
```

`System` зарезервирован для будущих внутренних relations.

## 4. Storage object

Storage object говорит, как физически хранится relation.

Heap table:

```text
Heap { pages: [PageId, ...] }
```

B+Tree index:

```text
BPlusTree {
  table_id,
  column_name,
  root_page_id
}
```

Если root page меняется после split, catalog metadata обновляется в той же transaction staging map.

## 5. Heap table

Heap table — это набор `PageType::Heap` страниц. Каждая строка лежит в slotted page как raw bytes.

Catalog не знает SQL row encoding. Он только хранит bytes и возвращает `RowId { page_id, slot_id }`.

SQL layer кодирует значения в bytes сам, через SQL row format v0.

## 6. Insert path

Упрощённо вставка выглядит так:

```text
найти relation по имени;
найти heap page со свободным местом;
если места нет — выделить новую page;
вставить record bytes в slotted page;
вернуть RowId;
обновить catalog metadata, если добавилась page.
```

В transaction path страница не пишется сразу в data file. Она попадает в dirty-page staging map и станет durable только после WAL commit.

## 7. Scan path

`full_scan` проходит по heap pages relation и читает живые slots.

Это простой путь для раннего SQL executor-а. Он не использует MVCC snapshot и не умеет predicate pushdown на уровне storage.

## 8. Extension metadata

Этап 9 добавил в catalog список установленных extensions:

```text
name
abi_version
kind
```

Сейчас `kind` фактически равен `static`. Это нужно, чтобы после reopen SQL layer мог собрать registry из catalog metadata и снова вызывать функции уже загруженных static extensions.

## 9. Что catalog пока не делает

Пока нет:

- системных SQL-таблиц вида `pg_class`;
- constraints;
- default values;
- nullability policy;
- schema namespaces;
- dependency tracking;
- privileges;
- statistics;
- catalog migrations;
- compatibility policy между версиями.

## 10. Что важно не сломать

Catalog page — центральная точка metadata. Любое изменение формата catalog должно сопровождаться:

- bump-ом version или явной migration policy;
- тестом decode старого/нового формата, если совместимость обещана;
- transaction test, где catalog update и heap/index page update commit-ятся вместе;
- recovery test после committed WAL.
