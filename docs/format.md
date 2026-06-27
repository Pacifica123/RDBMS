# Физические форматы RDBMS

Этот документ описывает текущие форматы, которые уже есть в коде. Форматы пока версии `v0/v1` и не обещают долгосрочную совместимость. Их задача — зафиксировать, какие байты пишет проект сейчас.

## 1. Общие правила

Формат строится вокруг страниц фиксированного размера:

```text
PageId -> offset = PageId * PAGE_SIZE
```

Все долгоживущие данные должны проходить через проверяемый бинарный формат. JSON не используется как формат базы данных.

Главные единицы:

- data file — набор страниц;
- page — физическая единица чтения/записи;
- WAL file — append-only журнал;
- catalog page — page 0;
- heap page — страницы таблиц;
- index page — страницы B+Tree;
- SQL row — encoded record внутри heap page.

## 2. Page v1

Страница — это массив байт фиксированного размера `PAGE_SIZE`. В начале лежит header, дальше свободное место, slot directory и записи.

Смысл header:

```text
magic/version      защита от чужого формата
page_id            физический номер страницы
page_type          catalog / heap / index / free
page_lsn           будущая связь с WAL redo
slot_count         число slot-ов
free_start         начало свободной области
free_end           конец свободной области
checksum           проверка повреждения страницы
```

Сейчас checksum нужен, чтобы ошибка чтения не превращалась в молчаливую порчу данных.

## 3. Slotted page

Slotted page хранит записи так:

```text
[page header][record payload grows →][free space][← slot directory]
```

Slot хранит offset и length записи. Пользователь записи видит не offset, а `SlotId`.

Инвариант:

```text
живой SlotId не меняется после compact()
```

Это важно для `RowId { page_id, slot_id }`: индекс хранит именно такой физический адрес строки.

Удаление в текущем page layer только помечает slot как свободный. Полноценный SQL `DELETE` ещё не реализован.

## 4. WAL record v0

WAL — append-only файл. Каждая запись имеет envelope:

```text
magic
version
kind
lsn
tx_id
page_id или absent marker
payload_len
checksum
payload
```

Текущие kind:

```text
BeginTx
PageImage
CommitTx
AbortTx
Checkpoint
```

`PageImage` хранит полный образ страницы. Это просто и дорого по месту, но хорошо подходит для раннего recovery: не нужно восстанавливать отдельные логические операции.

## 5. LSN

В WAL v0 `Lsn` — это byte offset начала record header в WAL file.

Это простое правило даёт проверку:

```text
record.lsn должен совпадать с offset, по которому reader нашёл запись
```

Если LSN не совпадает, WAL считается повреждённым.

## 6. Catalog page

Catalog хранится в page 0 как один record с magic `RDBC` и version `1`.

Catalog содержит:

```text
next_relation_id
next_page_id
relations[]
extensions[]
```

Relation metadata содержит:

```text
relation_id
name
kind: table / index / system
columns[]
storage object
```

Storage object сейчас бывает двух видов:

```text
Heap { pages[] }
BPlusTree { table_id, column_name, root_page_id }
```

Catalog также хранит установленные static extensions:

```text
name
abi_version
kind
```

## 7. Heap table

Heap table — это relation kind `Table` со storage object `Heap`.

Catalog хранит список page id, которые принадлежат таблице. Каждая heap page — обычная slotted page с `PageType::Heap`.

Строка в heap page хранится как opaque bytes. SQL layer сам кодирует и декодирует значения.

## 8. SQL row v0

SQL row encoding используется `rdbms_sql` для записи значений в heap table.

Форма:

```text
magic = RDBR
version = 1
value_count
values[]
```

Каждое значение имеет tag:

```text
NULL
INTEGER(i64)
TEXT(utf8 bytes)
DOUBLE(f64)
```

Типы в `CREATE TABLE` сейчас проверяются на уровне SQL subset. Нет полноценной системы типов, constraints, default values и NULL policy.

## 9. Index page / B+Tree node v0

Index page — это `PageType::Index`. Внутри страницы сейчас лежит один encoded node record в slot 0.

Node имеет magic `RDBI`, version `1` и kind:

```text
Leaf
Internal
```

Leaf хранит:

```text
next_leaf
entries: (key, RowId)[]
```

Internal node хранит:

```text
separator keys[]
children page ids[]
```

Поддержанные key types:

```text
INTEGER
TEXT
```

`MAX_KEYS` намеренно маленький, чтобы unit tests быстро заставляли дерево split-иться. Это тестовый параметр, не performance target.

## 10. Extension ABI sketch

`rdbms_ext_abi` описывает C-compatible структуры для будущих native extensions:

```text
RDBMS_EXT_ABI_VERSION = 1
RdbmsStatus
RdbmsHost opaque handle
RdbmsExtensionDescriptor
```

Сейчас runtime не загружает native libraries. Рабочий механизм расширений — static registry в `rdbms_extension`.

## 11. Android boundary

`rdbms_android` собирается как native library и экспортирует JNI-shaped symbols:

```text
Java_dev_rdbms_NativeSmoke_stage
Java_dev_rdbms_NativeSmoke_abiVersion
Java_dev_rdbms_NativeSmoke_add
```

Эти функции не читают базу и не исполняют SQL. Они только проверяют форму native boundary.

## 12. Что ещё не стабилизировано

Не стабилизированы:

- совместимость файлов между версиями;
- database header;
- WAL file header;
- checkpoint format;
- pageLSN redo skip;
- SQL schema constraints;
- index rebuild protocol;
- dynamic extension ABI loading;
- формат backup/export.

До отдельного compatibility milestone любые эти форматы можно менять патчами.
