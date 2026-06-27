# Журнал изменений RDBMS

Проект использует devctl patch-quantum versioning.

Короткая форма версии:

```text
MAJOR.MINOR.MICRO.QUANTUM
```

Внешняя SemVer-совместимая форма:

```text
vMAJOR.MINOR.MICRO+devctl.QUANTUM
```

`QUANTUM` — сквозной номер успешно применённого devctl-патча после введения этой системы. Он не сбрасывается при изменении `MAJOR`, `MINOR` или `MICRO`.

## 0.10.0.2 — p000002-client-research-version-plan-v0-10-0-2

Дата: 2026-06-27.

Тип изменения: документация и планирование, compatible.

Что изменилось:

- добавлен документ `docs/development/client_research_version_plan.md`;
- в документ встроен вердикт по MVP-клиенту и теоретическая проверка гипотез по коду ядра;
- зафиксирован риск `TxId` reuse после reopen при append-only WAL и recovery grouping по `TxId`;
- уточнён ближайший порядок развития: safety baseline → metrics → checkpoint/WAL compaction → SQL transactions/batching → heap free-space → EXPLAIN/index confidence → crash hardening → client introspection;
- `docs/roadmap.md`, `docs/development/README.md` и `README.md` получили ссылку на новый версионный план;
- `.devctl/templates/patch_manifest.template.json` переведён на следующую базу `0.10.0.2 -> 0.10.0.3`.

Ограничения:

- этот патч не меняет код ядра;
- `cargo check --workspace` должен выполняться в нормальной Rust-среде, но сам документ не зависит от компиляции;
- номера будущих квантов в плане являются ориентировочными и должны сдвигаться, если появятся промежуточные devctl-патчи.

## 0.10.0.1 — p000001-versioning-bootstrap-v0-10-0-1

Дата: 2026-06-27.

Тип изменения: bootstrap versioning, compatible.

Что изменилось:

- добавлены `VERSION` и `VERSION.json` как источник истины версии в Git;
- добавлен `CHANGELOG.md`;
- добавлен документ `docs/development/versioning.md` с правилами devctl patch-quantum versioning для RDBMS;
- обновлены правила devctl-патчей и шаблон manifest;
- добавлены локальные проверки версии в `tools/devctl`;
- README теперь показывает текущую версию и порядок чтения документации по версионированию.

Почему версия начинается с `0.10.0.1`:

- `0.10.0` отражает текущее состояние проекта после Stage 10 platform ports;
- `1` — первый принятый devctl-квант после введения протокола;
- более ранние патчи остаются историей до версионирования и не получают номера задним числом.

Ограничения:

- `lastPatchSha256` пока равен `null`, потому что SHA-256 итогового zip известен только после сборки патча;
- полная автоматическая проверка `manifest.version.base -> VERSION.json.version -> files/VERSION.json.version` требует доработки внешнего devctl;
- текущие проверки проекта валидируют форму manifest.version и согласованность файлов версии внутри репозитория.
