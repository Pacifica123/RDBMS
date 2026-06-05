# .devctl

Этот каталог хранит проектные правила и шаблоны для devctl-патчей.

Главный шаблон: `.devctl/templates/patch_manifest.template.json`.

Перед упаковкой нового патча проверь manifest:

```bash
python tools/devctl/validate_patch_manifest.py path/to/manifest.json
```

Затем обязательно выполнить devctl dry-run. Локальный Python-скрипт ловит типовые ошибки, но не заменяет dry-run.
