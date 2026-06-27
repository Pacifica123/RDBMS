# Документация для разработки

Этот каталог хранит документацию для разработки RDBMS.

Сейчас здесь важны два вида документов:

```text
devctl_patches.md                       правила подготовки devctl-патчей
versioning.md                          правила devctl patch-quantum versioning
client_research_version_plan.md        вердикт после MVP-клиента и план версий ядра
decisions/                             архитектурные решения
```

Основное правило: изменения в проект должны приходить маленькими проверяемыми devctl-патчами. Патч должен менять только связанные файлы, содержать `manifest.json`, `PATCH_SUMMARY.md`, финальные версии файлов в `files/` и проверки.

Текущая основная ветка проекта — `master`.

Перед упаковкой патча проверяй manifest:

```bash
python tools/devctl/validate_patch_manifest.py path/to/manifest.json
```
