# Подмножество SQL v0

## 1. Назначение SQL layer

`rdbms_sql` — маленький SQL-facing слой поверх `rdbms_tx::TransactionalStore`. Он нужен, чтобы проверять стек хранения через понятный интерфейс.

Это не полноценный SQL parser, binder, planner или optimizer.

## 2. Поддержанные команды

Текущий subset:

```sql
CREATE TABLE name (column TYPE, ...);
INSERT INTO name VALUES (literal, ...);
CREATE INDEX index_name ON table_name(column_name);
LOAD EXTENSION stdlib;
SELECT function(literal, ...);
SELECT * FROM table_name;
SELECT column, ... FROM table_name;
SELECT * FROM table_name WHERE column = literal;
SELECT column, ... FROM table_name WHERE column = literal;
```

Поддержанные literals:

```text
NULL
integer
text string
float/double
```

Поддержанные типы в `CREATE TABLE` пока ограничены тем, что умеет SQL row encoding:

```text
INT / INTEGER
TEXT
DOUBLE / FLOAT / REAL
```

## 3. Пример

```sql
CREATE TABLE users (id INT, name TEXT, score DOUBLE);
INSERT INTO users VALUES (1, 'Ada', 10.5);
INSERT INTO users VALUES (2, 'Grace', 20.0);
SELECT name, score FROM users WHERE id = 2;
```

Ожидаемый результат — materialized rows с колонками `name`, `score`.

## 4. Индексированный путь

Если есть индекс по колонке из `WHERE column = literal`, executor может использовать B+Tree lookup:

```sql
CREATE INDEX users_id_idx ON users(id);
SELECT name FROM users WHERE id = 1;
```

Даже если index path найден, executor всё равно проверяет predicate после чтения candidate rows. Это защитное правило: результат остаётся корректным, даже если в будущем появятся stale entries или fallback path.

## 5. Расширения

Static extension загружается так:

```sql
LOAD EXTENSION stdlib;
SELECT upper('ada');
SELECT length('abc');
```

`stdlib` сейчас встроен в бинарь. Dynamic plugin loading отсутствует.

После `LOAD EXTENSION` metadata сохраняется в catalog, поэтому при reopen registry можно восстановить из catalog extension list.

## 6. Parser

Parser намеренно простой:

- принимает один statement;
- требует, чтобы после statement не оставалось мусора;
- не поддерживает quoted identifiers;
- не поддерживает параметры;
- не поддерживает выражения, кроме literal arguments в scalar function call;
- не строит полноценное дерево операторов.

## 7. Executor

Executor напрямую вызывает `TransactionalStore`:

```text
CREATE TABLE  -> create_table_autocommit
INSERT        -> insert_row_autocommit + index maintenance
CREATE INDEX  -> create_index_autocommit
LOAD EXTENSION -> load_extension_autocommit
SELECT        -> full scan или equality index lookup
SELECT func   -> static extension registry call
```

Результат возвращается как `ExecResult`.

## 8. Чего нет

Пока нет:

- `BEGIN`, `COMMIT`, `ROLLBACK` в SQL;
- `UPDATE`, `DELETE`, `DROP TABLE`;
- `ALTER TABLE`;
- `JOIN`;
- `ORDER BY`, `GROUP BY`, aggregate functions;
- prepared statements;
- positional parameters;
- type coercion;
- constraints;
- primary key / unique;
- NULL semantics как в SQL standard;
- query optimizer;
- cost model;
- server protocol.

## 9. Как расширять дальше

Безопасный порядок развития:

```text
1. отделить parser AST от binder output;
2. добавить schema/type checking;
3. добавить prepared statements и params;
4. добавить logical plan;
5. добавить physical executor nodes;
6. добавить UPDATE/DELETE;
7. добавить SQL-visible transactions;
8. добавить optimizer только после стабильных operators.
```

Не стоит сразу писать большой SQL parser. В этом проекте storage correctness важнее ширины синтаксиса.
