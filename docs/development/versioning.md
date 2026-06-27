# Версионирование RDBMS через devctl-кванты

## Назначение

Версия проекта должна отвечать на простой вопрос: какой успешно применённый devctl-патч привёл проект в это состояние.

Обычный SemVer отвечает на вопрос о совместимости API. Для RDBMS этого мало, потому что проект меняется через devctl-патчи. В этом workflow патч — атомарная поставка изменения: `plan`, `start`, проверки, commit, архив и запись в devctl-состоянии.

Поэтому версия RDBMS состоит из смысловой версии и номера принятого devctl-кванта.

```text
MAJOR.MINOR.MICRO.QUANTUM
```

Пример:

```text
0.10.0.1
```

SemVer-совместимая форма для внешних инструментов:

```text
v0.10.0+devctl.1
```

Внутри проекта основной считается короткий формат `0.10.0.1`.

## Текущая версия

Текущая начальная версия после включения протокола:

```text
0.10.0.1
```

Это bootstrap-версия.

Почему не `0.0.1.1`: проект уже прошёл Stage 10 и имеет storage, WAL, recovery, catalog, tx, SQL subset, index, extension и platform smoke. Поэтому смысловая линия начинается с `0.10.0`.

Почему quantum равен `1`: счётчик начинается только после введения этого протокола. Старые патчи не перенумеровываются задним числом.

## Разряды версии

`MAJOR` — крупный несовместимый перелом. Для RDBMS это может быть смена формата страниц, WAL, catalog или devctl-workflow без совместимого перехода.

`MINOR` — новая заметная возможность без разрушения старого поведения. Примеры: новый SQL DML слой, checkpoint v0, fault-injection VFS, новый стабильный API.

`MICRO` — совместимое исправление или уточнение поведения. Примеры: bugfix recovery, улучшение ошибки parser-а, расширение проверки без нового пользовательского режима.

`QUANTUM` — сквозной номер успешного devctl-патча. Он увеличивается на `+1` после каждого `devctl start -> applied`.

`QUANTUM` не сбрасывается. Даже если `0.10.2.14` переходит в `0.11.0.15`, последний разряд продолжает расти.

## Что считается квантом

Квант — только успешный devctl-патч:

```text
devctl start -> status=applied
```

Не увеличивают quantum:

- `devctl plan`;
- `devctl inspect`;
- `devctl inbox grab`;
- неуспешный `devctl start`;
- `invalid_patch`;
- `push_failed`, если локальный commit требует ручного разбора;
- ручная правка без patch.zip.

Правило жёсткое: изменение, которое должно попасть в историю версий, должно пройти через devctl-патч.

## Источник истины

Версия хранится в Git, а не только в локальном `.devctl/state.json`.

Файлы:

```text
VERSION
VERSION.json
CHANGELOG.md
```

`VERSION` нужен человеку и простым скриптам.

`VERSION.json` нужен инструментам.

`CHANGELOG.md` объясняет, что изменилось и почему был выбран bump.

## VERSION.json

Минимальная структура:

```json
{
  "schema": "devctl-version-v1",
  "version": "0.10.0.1",
  "semver": "0.10.0",
  "major": 0,
  "minor": 10,
  "micro": 0,
  "quantum": 1,
  "released": false,
  "updatedAt": "2026-06-27T00:00:00Z",
  "lastPatchId": "p000001-versioning-bootstrap-v0-10-0-1",
  "lastPatchSha256": null
}
```

`lastPatchSha256` может быть `null`, пока devctl не умеет сам дописывать SHA-256 итогового архива в версионные файлы.

## Manifest-поле version

Каждый новый patch.zip должен содержать блок `version` в `manifest.json`.

Пример обычного quantum-bump:

```json
"version": {
  "schema": "devctl-version-intent-v1",
  "base": "0.10.0.1",
  "next": "0.10.0.2",
  "bump": "quantum",
  "quantum": 2,
  "reason": "Документационный патч без изменения поведения проекта.",
  "publicSurface": ["docs"],
  "compatibility": "compatible"
}
```

Для первого патча, который вводит версионирование, допустим `bootstrap`:

```json
"version": {
  "schema": "devctl-version-intent-v1",
  "base": null,
  "next": "0.10.0.1",
  "bump": "bootstrap",
  "quantum": 1,
  "reason": "Первое включение VERSION/VERSION.json/CHANGELOG.",
  "publicSurface": ["versioning", "docs", "tools/devctl"],
  "compatibility": "compatible"
}
```

После bootstrap `base` больше не должен быть `null`.

## Правила bump

Только quantum:

```text
0.10.0.1 -> 0.10.0.2
```

Использовать для документации, комментариев, тестов без изменения поведения, внутреннего refactor и чистки.

Micro:

```text
0.10.0.2 -> 0.10.1.3
```

Использовать для совместимых bugfix-ов и малых UX/diagnostics исправлений.

Minor:

```text
0.10.1.3 -> 0.11.0.4
```

Использовать для новой возможности без разрушения старого поведения.

Major:

```text
0.11.0.4 -> 1.0.0.5
```

Использовать для несовместимого изменения формата, workflow или публичного API.

## Имена patch.zip

Рекомендуемый формат:

```text
patch_YYYYMMDD_HHMMSS_pNNNNNN_slug_vMAJOR_MINOR_MICRO_QUANTUM.zip
```

Пример:

```text
patch_20260627_130000_p000002_test_hardening_plan_v0_10_0_2.zip
```

`pNNNNNN` должен совпадать с quantum в `VERSION.json` и `manifest.version.quantum`.

## Commit message

Commit message должен включать версию.

Пример:

```text
docs(versioning): add patch-quantum protocol v0.10.0.1

Version-Before: none
Version-After: 0.10.0.1
Version-Bump: bootstrap
Patch-Quantum: 1
```

Для обычного патча:

```text
docs: update recovery notes v0.10.0.2

Version-Before: 0.10.0.1
Version-After: 0.10.0.2
Version-Bump: quantum
Patch-Quantum: 2
```

## Проверки

Минимальные проверки для версионного патча:

```bash
python tools/devctl/validate_version_files.py
python tools/devctl/validate_patch_manifest.py path/to/manifest.json
python tools/devctl/validate_patch_manifest.py .devctl/templates/patch_manifest.template.json
```

Обычные Rust-проверки проекта остаются:

```bash
cargo check --workspace
cargo test --workspace
```

## Конфликт двух патчей

Если два patch.zip собраны от `0.10.0.1` и оба предлагают `0.10.0.2`, применить можно только первый.

Второй патч устарел. Его нужно пересобрать от новой базы и выдать ему следующий quantum.

Это лучше, чем пытаться автоматически склеивать две истории. Версия должна защищать порядок причин.

## Чего пока нет

Текущее внедрение не меняет внешний devctl. Поэтому часть правил проверяется локальными скриптами и глазами при `devctl plan`.

Пока нет автоматической блокировки `devctl start`, если:

- `manifest.version.base` не совпадает с текущим `VERSION.json.version`;
- payload меняет `VERSION.json`, но `manifest.version.next` не совпадает с ним;
- `CHANGELOG.md` не обновлён;
- quantum не равен `current.quantum + 1`.

Это следующий логичный шаг для доработки devctl.

## Короткое правило

Каждый успешный devctl-патч получает один новый quantum.

Старшие разряды объясняют смысл изменения.

Последний разряд доказывает факт принятого изменения.
