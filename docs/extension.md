# Extension v0

## 1. Назначение

Stage 9 добавляет первый безопасный путь расширений. Это не native plugin loader и не `.so`/`.dll` runtime. Текущий слой нужен, чтобы зафиксировать контракт:

```text
extension descriptor
  -> ABI version check
  -> static registry
  -> scalar functions
  -> SQL LOAD EXTENSION
  -> persisted catalog metadata
```

## 2. Активные crates

```text
rdbms_ext_abi       C-compatible ABI sketch and ABI version constant
rdbms_extension     safe static extension registry v0
rdbms_catalog       persisted extension metadata in catalog page 0
rdbms_tx            WAL-backed install of extension metadata
rdbms_sql           LOAD EXTENSION and SELECT scalar_function(...)
```

`rdbms_ext_abi` остаётся низкоуровневым контрактом для будущих native plugins. `rdbms_extension` — текущий рабочий слой для безопасных built-in extensions.

## 3. Static registry v0

Расширение описывается descriptor-ом:

```text
name
abi_version
kind = static
scalar_functions[]
```

Каждая scalar function описывается так:

```text
name
arity
return_type
eval(args) -> Value
```

Сейчас поддержан только `ScalarArity::Exact(N)`. Функции принимают и возвращают `rdbms_core::Value`.

## 4. Встроенное расширение stdlib

Stage 9 добавляет built-in static extension:

```text
stdlib
```

Функции:

```text
length(TEXT) -> INT
lower(TEXT) -> TEXT
upper(TEXT) -> TEXT
abs(INT|DOUBLE) -> INT|DOUBLE
typeof(Value) -> TEXT
rdbms_version() -> TEXT
```

`NULL` для `length/lower/upper/abs` возвращает `NULL`. Остальные ошибки считаются user error, а не corruption.

## 5. SQL surface

Установка расширения:

```sql
LOAD EXTENSION stdlib;
```

Вызов scalar function без `FROM`:

```sql
SELECT upper('ada');
SELECT length('abc');
SELECT rdbms_version();
```

В Stage 9 функции принимают только literal arguments. Вызовы вида `SELECT upper(name) FROM users` ещё не поддержаны. Это оставлено для planner/binder/expression этапа.

## 6. Catalog metadata

После `LOAD EXTENSION stdlib` catalog page хранит extension metadata:

```text
name = stdlib
abi_version = 1
kind = static
```

Metadata пишется через `rdbms_tx`, поэтому установка расширения получает тот же commit порядок, что catalog/heap/index pages:

```text
PageImage(catalog page)
CommitTx
WAL sync
data page write
data sync
```

При выполнении scalar function SQL executor строит runtime registry из extension metadata, сохранённой в catalog. Если extension не загружен, функция не находится.

## 7. ABI version check

`rdbms_ext_abi::RDBMS_EXT_ABI_VERSION` сейчас равен `1`. Registry отказывает descriptor-у, если его `abi_version` не поддерживается текущей сборкой.

Stage 9 не разыменовывает raw pointers и не вызывает native entry point. `RdbmsExtensionDescriptor` остаётся sketch-ем внешней ABI, но runtime использует безопасный static path.

## 8. Ограничения

```text
нет dynamic loading;
нет Linux .so/.dll plugin loader;
нет WASM runtime;
нет aggregate functions;
нет table-valued functions;
нет functions over table columns;
нет function volatility/security model;
нет extension unload;
нет dependency tracking between extensions.
```
