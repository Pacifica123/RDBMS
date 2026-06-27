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
