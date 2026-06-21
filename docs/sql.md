# SQL subset v0

## 1. Назначение

Stage 7 добавляет первый SQL-facing слой поверх `rdbms_tx::TransactionalStore`.

Это не полный SQL engine. Цель этапа — связать уже готовые catalog/heap/transaction primitives с маленьким пользовательским языком и зафиксировать минимальную границу parser/executor.

Поддерживаемый поток:

```text
SQL text
  -> lexer/parser
  -> Statement AST
  -> direct executor
  -> TransactionalStore autocommit
  -> catalog/heap pages
  -> WAL-backed commit
```

## 2. Новый активный слой

Crate:

```text
rdbms_sql
```

Главные API:

```text
parse_statement(sql) -> Statement
execute(store, sql, params) -> ExecResult
SqlSession::execute(sql, params) -> ExecResult
encode_row(values) -> Vec<u8>
decode_row(bytes) -> Vec<Value>
```

`params` пока должны быть пустыми. Параметры относятся к будущему binder/executor этапу.

## 3. Поддерживаемые statements

Stage 7 поддерживает только одну SQL statement за вызов:

```sql
CREATE TABLE users (id INT, name TEXT);
INSERT INTO users VALUES (1, 'Ada');
SELECT * FROM users;
SELECT name FROM users WHERE id = 1;
```

Поддержано:

```text
CREATE TABLE name (column TYPE, ...)
INSERT INTO name VALUES (literal, ...)
SELECT * FROM name
SELECT column, ... FROM name
SELECT ... FROM name WHERE column = literal
```

Не поддержано:

```text
SQL BEGIN/COMMIT/ROLLBACK;
INSERT column list;
UPDATE;
DELETE;
JOIN;
ORDER BY;
GROUP BY;
aggregates;
expressions;
prepared statements;
quoted identifiers;
multiple statements in one execute call.
```

## 4. Идентификаторы и типы

Идентификаторы Stage 7 простые:

```text
[A-Za-z_][A-Za-z0-9_]*
```

Parser нормализует имена таблиц и колонок в lower-case. Quoted identifiers пока отсутствуют.

Поддерживаемые SQL-типы:

```text
INT
INTEGER
TEXT
DOUBLE
REAL
FLOAT
```

Типы в catalog сохраняются в upper-case.

## 5. Литералы

Поддерживаемые literals:

```text
NULL
123
-123
1.5
'text'
```

В строковых литералах одинарная кавычка экранируется удвоением:

```sql
INSERT INTO quotes VALUES ('it''s ok');
```

## 6. SQL row encoding v0

Heap table v0 по-прежнему хранит raw row bytes. Stage 7 вводит SQL row payload v0 внутри этих raw bytes.

SQL row v0:

```text
offset  size      field
0       4         magic = "RDBR"
4       2         version = 1
6       2         value_count
8       variable  values
```

Value entry:

```text
tag = 0  NULL, no payload
tag = 1  INTEGER, i64 little-endian
tag = 2  TEXT, u32 byte_len + UTF-8 bytes
tag = 3  DOUBLE, f64 little-endian
```

SQL executor проверяет, что число значений совпадает с числом колонок в catalog.

## 7. Execution model

`CREATE TABLE` выполняется через:

```text
TransactionalStore::create_table_autocommit
```

`INSERT` выполняется через:

```text
coerce literals to catalog column types;
encode SQL row v0;
TransactionalStore::insert_row_autocommit.
```

`SELECT` выполняется через:

```text
lookup relation in committed catalog;
full_scan(relation_id);
decode SQL row v0;
optional equality WHERE;
projection;
materialized ExecResult::Query.
```

Это прямой executor. Отдельного logical plan, optimizer и physical operator tree пока нет.

## 8. Связь с транзакциями

Stage 7 сам не добавляет новый transaction manager. Все write statements идут через Stage 6 autocommit helpers. Поэтому `CREATE TABLE` и `INSERT`, выполненные через SQL API, получают тот же WAL-backed commit порядок:

```text
PageImage records;
CommitTx;
WAL sync;
data page writes;
data sync.
```

SQL-level explicit transaction statements пока отсутствуют.

## 9. Ограничения

Stage 7 не умеет читать arbitrary raw heap rows, созданные прямым `CatalogStore::insert_row`. SQL `SELECT` ожидает SQL row v0 bytes с magic `RDBR`.

Текущий `WHERE` — только equality predicate вида:

```sql
WHERE column = literal
```

Без boolean expressions, comparison operators, indexes и NULL-semantics уровня SQL standard.
