# Правила `.devctl`

Каталог `.devctl` хранит проектные правила для devctl-патчей RDBMS.

Главный шаблон manifest:

```text
.devctl/templates/patch_manifest.template.json
```

Обязательные правила проекта:

- основная ветка — `master`;
- `base.branch` и `push.branch` в `manifest.json` должны быть равны `master`;
- `apply.filesRoot` должен быть `files`;
- `apply.delete` должен быть массивом объектов, даже если удалений нет;
- в патче должны быть проверки;
- в патче должен быть блок `version`;
- `VERSION`, `VERSION.json` и `CHANGELOG.md` должны обновляться каждым успешным devctl-квантом;
- в архив нельзя класть `.git/`, `target/`, `build/`, `.env*`, bytecode/cache и базы данных.

Быстрая проверка manifest:

```bash
python tools/devctl/validate_patch_manifest.py path/to/manifest.json
python tools/devctl/validate_version_files.py
```

Эта проверка ловит частые ошибки до запуска `devctl plan/start`. Она не заменяет dry-run через devctl.

## Версионирование

RDBMS использует формат `MAJOR.MINOR.MICRO.QUANTUM`. Последний разряд — номер успешного devctl-патча после введения протокола. Правила лежат в `docs/development/versioning.md`.
