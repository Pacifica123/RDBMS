# RDBMS — Rust Database Management System

Этот репозиторий больше не рассматривает ранний код `RDBMS-master` как основу будущей СУБД. Старый код сохранён как legacy-материал в `docs/legacy/RDBMS-master`, а новая работа начинается с архитектуры ядра: страницы, VFS, WAL, recovery, каталог, транзакционная граница, SQL-слой и API расширений.

Цель проекта — переносимое учебно-инженерное ядро СУБД на Rust, которое можно развивать для Linux, Windows и Android. Проект не пытается сразу стать смесью PostgreSQL, SQLite, RocksDB и ClickHouse. Ближайшая цель уже: малое корректное ядро с проверяемыми инвариантами.

## Текущее состояние

Статус: architecture-first reboot уже дошёл до минимального SQL subset поверх WAL-backed transactions. Проект умеет выполнять ограниченные `CREATE TABLE`, `INSERT INTO ... VALUES ...` и `SELECT` через `rdbms_sql`, но это ещё не полноценная SQL-СУБД.

Старый бакалаврский прототип был полезен как research spike: он нащупал слова `Database`, `Table`, `Column`, `Row`, `Value` и желание иметь SQL-facing API. Но он начинался со строкового SQL-диспетчера, JSON-снимков и in-memory `Vec<Table>`, поэтому не годится как фундамент storage/recovery/transaction architecture.

## Новая структура

```text
crates/
  rdbms_core/       общие типы, ошибки, идентификаторы
  rdbms_vfs/        VFS/IO-абстракция и будущая fault injection граница
  rdbms_page/       page ids, page header, checksum boundary, slotted page sketch
  rdbms_wal/        LSN, WAL records, writer/reader/recovery skeleton
  rdbms_catalog/    системный каталог и relation metadata
  rdbms_tx/         transactions v0 поверх catalog/heap/WAL
  rdbms_sql/        SQL subset v0: parser, row encoding, direct executor
  rdbms_ext_abi/    стабильная внешняя ABI-граница расширений
  rdbms_cli/        тонкий CLI поверх публичного API

docs/
  architecture.md
  non_goals.md
  format.md
  wal.md
  recovery.md
  transactions.md
  sql.md
  extension_abi.md
  roadmap.md
  unsafe.md
  legacy/
  development/
.devctl/
  templates/
tools/devctl/
  validate_patch_manifest.py
```

## Порядок разработки

Правильный порядок для этого проекта:

```text
байты → страницы → WAL → recovery → каталог → heap table → транзакция → SQL subset → индекс → расширения → переносимость
```

SQL shell не является первым milestone. Первый milestone — создать файл, записать страницу, прочитать страницу, проверить checksum, переоткрыть файл и обнаружить повреждение.


## Ближайший реализованный слой

Текущий верхний слой — `crates/rdbms_sql`. Он даёт маленький SQL subset поверх `rdbms_tx::TransactionalStore`: создание таблицы, вставку literal values и materialized SELECT с простым equality WHERE. Ниже уже есть страницы, VFS/page store, WAL, recovery, catalog, heap table и transactions v0.

Это всё ещё skeleton: нет SQL transactions, JOIN, UPDATE/DELETE, индексов, optimizer-а и prepared statements.

## Документы первого чтения

1. `docs/architecture.md`
2. `docs/format.md`
3. `docs/wal.md`
4. `docs/recovery.md`
5. `docs/roadmap.md`
6. `docs/non_goals.md`
7. `docs/extension_abi.md`
8. `docs/unsafe.md`
9. `docs/legacy/README.md`
10. `docs/development/devctl_patches.md`

## Процесс разработки

Изменения в проект вносятся devctl-патчами. Базовые правила зафиксированы в `docs/development/devctl_patches.md`, шаблон manifest лежит в `.devctl/templates/patch_manifest.template.json`, локальная проверка — в `tools/devctl/validate_patch_manifest.py`.

Текущая основная ветка для патчей: `master`. В `manifest.json` поля `base.branch` и `push.branch` должны указывать `master`, пока проект явно не переедет на другую ветку.

`apply.delete` в manifest всегда записывается массивом объектов: `{ "path": "...", "required": false }`. Массив строк невалиден для нашего devctl-конвейера.

## Проверки

```bash
cargo check --workspace
python tools/devctl/validate_patch_manifest.py .devctl/templates/patch_manifest.template.json
```

Теперь `cargo test -p rdbms_sql` проверяет SQL subset: parse, SQL row encoding, CREATE/INSERT/SELECT и простой WHERE. `cargo check --workspace` по-прежнему остаётся общей проверкой целостности workspace.
