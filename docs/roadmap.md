# Roadmap

Roadmap построен так, чтобы прогресс измерялся гарантиями, а не числом распознанных SQL-команд.

## Milestone 0 — repository reboot

Готово, когда:

- старый код перенесён в `docs/legacy`;
- корень стал Rust workspace;
- есть документы architecture/format/WAL/recovery/roadmap;
- `cargo check --workspace` проходит.

## Milestone 0.1 — devctl workflow

Готово, когда:

- правила devctl-патчей описаны в `docs/development/devctl_patches.md`;
- есть шаблон `.devctl/templates/patch_manifest.template.json`;
- есть локальная проверка `tools/devctl/validate_patch_manifest.py`;
- в документации зафиксированы нюансы: `apply.delete` как массив объектов и текущая ветка `master`;
- каждый следующий патч содержит checks.

## Milestone 1 — page store

Готово, когда:

- создаётся файл базы;
- пишется страница 4096 байт;
- страница читается обратно;
- checksum проверяется;
- повреждённая страница обнаруживается typed error;
- тесты не зависят от SQL.

## Milestone 2 — VFS + fault injection

Готово, когда:

- storage использует VFS trait;
- есть std implementation;
- есть fault-injection implementation для тестов;
- можно моделировать short write, lost write и sync error.

## Milestone 3 — WAL + recovery skeleton

Готово, когда:

- WAL records имеют LSN;
- commit проходит через sync boundary;
- recovery делает redo committed page changes;
- repeated recovery идемпотентен;
- crash tests покрывают основные точки сбоя.

## Milestone 4 — catalog v0

Готово, когда:

- catalog хранится в страницах базы;
- relation metadata переживает переоткрытие;
- catalog changes проходят через WAL/recovery.

## Milestone 5 — heap table v0

Готово, когда:

- есть slotted page для строк;
- `RowId = PageId + SlotId`;
- insert/read/delete marker работают без SQL;
- property tests проверяют layout invariants.

## Milestone 6 — transaction manager v0

Готово, когда:

- есть tx id;
- committed/uncommitted состояние различается;
- MVP ограничен many readers + single writer;
- rollback/abort имеет ясную модель хотя бы для поддержанных операций.

## Milestone 7 — SQL subset

Готово, когда:

- SQL parser не основан на `starts_with()`;
- binder проверяет catalog;
- executor возвращает typed `ExecResult`;
- поддерживается малое подмножество: create table, insert, select by full scan.

## Milestone 8 — B+Tree index

Готово, когда:

- index pages имеют собственный формат;
- index changes журналируются;
- recovery сохраняет согласованность индекса и heap;
- differential tests сравнивают SQL-подмножество с SQLite.

## Milestone 9 — extension v0

Готово, когда:

- есть versioned ABI descriptor;
- есть безопасная host boundary;
- panic не пересекает FFI boundary;
- capability model документирован и проверяется.

## Milestone 10 — platform readiness

Готово, когда:

- Linux/Windows/Android сборки описаны;
- VFS sync semantics задокументированы по платформам;
- CI хотя бы компилирует поддержанные targets.
