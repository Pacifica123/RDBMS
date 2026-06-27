# RDBMS — Rust Database Management System

RDBMS — учебно-инженерная СУБД на Rust. Проект строится снизу вверх: сначала байты, страницы, журнал предзаписи и восстановление, потом каталог, транзакции, SQL, индексы, расширения и переносимость.

Это не обёртка над SQLite/PostgreSQL и не попытка сразу сделать промышленную СУБД. Текущая цель проще: маленькое ядро, где каждый слой можно проверить тестами и где понятно, какие гарантии уже есть, а каких ещё нет.

## Версия

Текущая версия проекта после введения devctl patch-quantum versioning:

```text
0.10.0.1
```

`0.10.0` отражает состояние после Этапа 10. Последний разряд `1` — первый успешный devctl-квант после включения версионирования. Источник истины: `VERSION`, `VERSION.json` и `CHANGELOG.md`. Правила описаны в `docs/development/versioning.md`.

## Текущий статус

Проект дошёл до Этап 10. Сейчас есть:

- файловый слой `rdbms_vfs` и страничный файл;
- страница фиксированного размера с заголовком, слотами и checksum;
- WAL v0 с бинарными записями, LSN, commit-marker и чтением журнала;
- recovery v0, который применяет committed full-page images;
- persistent catalog и heap-таблицы;
- transactions v0 поверх catalog/heap/WAL;
- небольшой SQL subset;
- B+Tree index v0 для equality lookup;
- static extension registry v0;
- платформенные smoke-проверки для Windows, Android, Linux/macOS CI.

Это уже можно использовать как учебную базу и как основу для pet-проектов, где допустимы ограничения. Это ещё нельзя считать полноценной SQL-СУБД: нет конкурентного MVCC, SQL-транзакций, `UPDATE`, `DELETE`, `JOIN`, prepared statements, optimizer-а, нормальных checkpoint-ов, `UNDO`, динамических plugin-ов и полноценного Android API.

## Что уже можно выполнить через SQL layer

Пример поддержанного сценария:

```sql
CREATE TABLE users (id INT, name TEXT);
CREATE INDEX users_id_idx ON users(id);
INSERT INTO users VALUES (1, 'Ada');
SELECT name FROM users WHERE id = 1;
LOAD EXTENSION stdlib;
SELECT upper('ada');
SELECT length('abc');
```

Поддержка SQL намеренно маленькая. Сейчас это API для проверки стек хранения, а не самостоятельный SQL server.

## Структура workspace

```text
crates/
  rdbms_core/       общие типы, ошибки, идентификаторы, Value, ExecResult
  rdbms_vfs/        VFS, random-access IO, sync_data, PageFile
  rdbms_page/       физическая страница, header, checksum, slotted page
  rdbms_wal/        WAL records, LSN, writer/reader, commit marker
  rdbms_recovery/   redo-only recovery loop
  rdbms_catalog/    persistent catalog, heap table metadata, extension metadata
  rdbms_tx/         transactions v0, dirty-page staging, commit/rollback
  rdbms_sql/        parser/executor маленького SQL subset
  rdbms_index/      B+Tree index v0 для равенства
  rdbms_ext_abi/    C-compatible ABI sketch для будущих расширений
  rdbms_extension/  static extension registry v0
  rdbms_android/    Android cdylib/JNI smoke boundary
  rdbms_cli/        пока тонкий CLI-заглушка

docs/               основная документация проекта
platform/android/   Java wrapper для JNI smoke
tests/              заготовки будущих crash/differential/property tests
.devctl/            правила devctl-патчей
tools/devctl/       локальная проверка manifest.json
```

## Правильный порядок развития

Главное правило проекта:

```text
байты → страницы → VFS/page store → WAL → recovery → каталог → heap table → транзакция → SQL subset → индекс → расширения → переносимость
```

SQL shell не является первым доказательством СУБД. Для этого проекта важнее уметь записать страницу, прочитать её после reopen, обнаружить повреждение, восстановить committed данные после WAL и только потом поднимать SQL-слой.

## Документы первого чтения

Начинать лучше в таком порядке:

1. `docs/architecture.md` — общая схема слоёв.
2. `docs/format.md` — физические форматы страницы, WAL, catalog, row, index.
3. `docs/wal.md` — как устроен журнал предзаписи.
4. `docs/recovery.md` — что делает восстановление.
5. `docs/transactions.md` — commit protocol Этап 6.
6. `docs/sql.md` — текущий SQL subset.
7. `docs/index.md` — B+Tree v0 простыми словами.
8. `docs/extension.md` и `docs/extension_abi.md` — расширения.
9. `docs/platform.md` — Windows/Android/CI boundary.
10. `docs/roadmap.md` — что уже сделано и что дальше.
11. `docs/non_goals.md` — что проект сознательно не обещает.
12. `docs/development/versioning.md` — как читать и bump-ать версию.
13. `docs/development/devctl_patches.md` — как готовить патчи.

## Проверки

Основные команды:

```bash
cargo check --workspace
cargo test --workspace
python tools/devctl/validate_version_files.py
python tools/devctl/validate_patch_manifest.py .devctl/templates/patch_manifest.template.json
```

Для SQL-слоя отдельно полезно:

```bash
cargo test -p rdbms_sql
```

Для platform stage:

```bash
cargo test -p rdbms_vfs
cargo test -p rdbms_android
```

Android cross-build зависит от установленного Android target/NDK и обычно проверяется в CI.

## Как вносить изменения

Проект обновляется через devctl-патчи. Патч должен быть zip-архивом с `manifest.json`, `PATCH_SUMMARY.md` и каталогом `files/`, где лежат финальные версии изменённых файлов.

Ветка проекта: `master`. В `manifest.json` поля `base.branch` и `push.branch` должны указывать `master`.

`apply.delete` всегда пишется как массив объектов, а не строк:

```json
"delete": [
  { "path": "old/file.md", "recursive": false, "required": false }
]
```

Перед упаковкой патча нужно проверить manifest:

```bash
python tools/devctl/validate_patch_manifest.py path/to/manifest.json
```
