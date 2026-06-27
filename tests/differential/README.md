# Differential tests

Этот каталог зарезервирован для будущих differential tests.

Идея: сравнивать поддержанный SQL subset с SQLite или PostgreSQL там, где semantics совпадает.

На текущем этапе сравнивать можно только маленькие сценарии:

- `CREATE TABLE`;
- `INSERT INTO ... VALUES ...`;
- `SELECT *`;
- `SELECT column list`;
- `WHERE column = literal`.

Нельзя сравнивать поведение, которого RDBMS пока не обещает: full SQL NULL semantics, joins, transactions, constraints, type coercion.
