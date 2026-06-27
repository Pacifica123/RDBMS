# Devctl-патчи для RDBMS

## 1. Зачем нужен devctl

RDBMS меняется через zip-патчи, а не через хаотичное редактирование архива проекта. Devctl даёт воспроизводимый поток:

```text
patch.zip → plan → apply → checks → commit → archive/report
```

Патч должен быть понятен человеку и безопасен для автоматического применения.

## 2. Структура патча

```text
patch_YYYYMMDD_HHMMSS_short_slug.zip
  manifest.json
  PATCH_SUMMARY.md
  files/
    relative/path/in/project.ext
```

`files/` содержит финальные версии изменённых файлов. Пути внутри `files/` всегда относительны к корню `project/`.

Нельзя класть:

```text
.git/
target/
build/
dist/
coverage/
__pycache__/
*.pyc
*.pyo
.env
.env.*
*.sqlite
*.db
*.dbonrs
```

## 3. Ветка проекта

Основная ветка RDBMS сейчас:

```text
master
```

В `manifest.json` должно быть:

```json
"base": {
  "branch": "master",
  "expectedHead": null
},
"push": {
  "remote": "origin",
  "branch": "master"
}
```

Не ставить `main`, пока репозиторий явно не переехал.

## 4. `apply.delete`

`apply.delete` всегда массив объектов.

Правильно:

```json
"apply": {
  "filesRoot": "files",
  "delete": [
    { "path": "old/file.md", "recursive": false, "required": false }
  ]
}
```

Если удалений нет:

```json
"delete": []
```

Неправильно:

```json
"delete": ["old/file.md"]
```

## 5. Минимальный manifest

```json
{
  "formatVersion": 1,
  "patchId": "2026-06-27-rdbms-docs-ru-refresh",
  "title": "Русификация и актуализация документации RDBMS",
  "summary": "Обновляет markdown-документацию проекта на русском языке и приводит её к состоянию Этап 10.",
  "kind": "documentation",
  "createdAt": "2026-06-27T00:00:00Z",
  "base": {
    "branch": "master",
    "expectedHead": null
  },
  "apply": {
    "filesRoot": "files",
    "delete": []
  },
  "checks": [
    {
      "name": "Rust workspace compiles",
      "cwd": ".",
      "command": "cargo check --workspace",
      "requiredCommands": ["cargo"],
      "timeoutSeconds": 300
    }
  ],
  "commit": {
    "message": "docs: русифицировать и актуализировать документацию"
  },
  "push": {
    "remote": "origin",
    "branch": "master"
  },
  "archive": {
    "nameSlug": "rdbms-docs-ru-refresh",
    "exclude": [
      ".git/",
      "target/",
      "dist/",
      "build/",
      "coverage/",
      "__pycache__/",
      ".env",
      ".env.*",
      "*.sqlite",
      "*.db",
      "*.dbonrs"
    ]
  }
}
```

## 6. Проверки

Обычные проверки для RDBMS:

```bash
cargo check --workspace
cargo test --workspace
python tools/devctl/validate_patch_manifest.py .devctl/templates/patch_manifest.template.json
```

Для документационного патча всё равно полезно прогнать `cargo check --workspace`, чтобы убедиться, что патч не сломал workspace случайно.

Manifest самого патча нужно проверить отдельно:

```bash
python tools/devctl/validate_patch_manifest.py path/to/manifest.json
```

## 7. `PATCH_SUMMARY.md`

Summary должен отвечать на вопросы:

- что меняется;
- зачем это нужно;
- какие файлы затронуты;
- какие риски есть;
- какие проверки прогнаны;
- есть ли особые инструкции применения.

## 8. Частые ошибки

Ошибка: `base.branch must be 'master'`.

Причина: в manifest указали `main`.

Ошибка: `apply.delete[0] must be an object`.

Причина: удаления записаны строками, а нужны объекты.

Ошибка: `checks must be a non-empty list`.

Причина: патч не объявил проверки.

Ошибка: `Файлов к копированию: 0`.

Причина: файлы положены не в тот `filesRoot` или архив собран неверно.

## 9. Практическое правило

Хороший patch.zip должен быть таким, чтобы другой человек мог открыть его без контекста и понять:

```text
какой проект меняется;
какая ветка ожидается;
какие файлы будут заменены;
какие команды проверят результат;
как откатиться, если проверка упадёт.
```
