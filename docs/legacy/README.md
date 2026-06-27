# Исторический прототип RDBMS-master

Этот каталог хранит ранний бакалаврский прототип. Он не является основой новой архитектуры.

Статус:

```text
legacy / archived / not used as current codebase
```

Что в нём полезно:

- ранние сущности `Database`, `Table`, `Column`, `Row`, `Value`;
- список желаемых SQL-команд;
- первая попытка отделить логику от `main.rs`;
- историческая фиксация интереса к теме СУБД.

Почему код не продолжается напрямую:

- SQL dispatcher появился раньше storage engine;
- нет page format;
- нет WAL;
- нет recovery;
- нет catalog;
- нет transaction architecture;
- JSON snapshot Rust-структуры не является форматом базы;
- `Vec<Table>` не даёт физического адреса строк;
- переносимость Linux/Windows/Android не выделена в отдельный IO layer.

Старый `task.txt` можно читать только как историческую заметку, не как roadmap новой версии.
