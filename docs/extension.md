# Расширения

## 1. Что есть сейчас

Этап 9 добавил безопасный static extension path.

Это значит:

- расширения не загружаются из `.so/.dll/.dylib`;
- extension descriptor встроен в Rust binary;
- registry проверяет ABI version;
- scalar functions вызываются через обычные Rust function pointers;
- metadata о загруженном extension сохраняется в catalog.

Текущий встроенный extension:

```text
stdlib
  upper(TEXT)  -> TEXT
  length(TEXT) -> INT
```

SQL пример:

```sql
LOAD EXTENSION stdlib;
SELECT upper('ada');
SELECT length('abc');
```

## 2. Почему сначала static path

Dynamic native plugins — рискованная граница. Там появляются вопросы ABI, lifetime, ownership, unsafe, symbol loading, platform differences и security policy.

Static path проще:

```text
extension descriptor известен на compile time;
функции вызываются безопасно;
ABI version уже проверяется;
SQL/catalog path можно отладить без dynamic loader.
```

Так проект получает расширяемость как архитектурный слой, но не тащит сразу весь риск native plugin loading.

## 3. Как работает LOAD EXTENSION

Упрощённо:

```text
1. parser читает LOAD EXTENSION name;
2. executor ищет built-in descriptor;
3. registry проверяет abi_version;
4. functions регистрируются по имени;
5. catalog сохраняет extension metadata;
6. commit делает изменение recoverable через WAL.
```

Повторная загрузка уже загруженного extension не должна создавать дубликаты функций.

## 4. Вызов scalar function

Для запроса:

```sql
SELECT upper('ada');
```

executor:

```text
1. разбирает function call без FROM;
2. проверяет, что function есть в registry;
3. проверяет arity;
4. вызывает Rust function pointer;
5. возвращает ExecResult с одной строкой и одной колонкой.
```

## 5. Catalog metadata

Catalog хранит:

```text
name
abi_version
kind
```

Сейчас `kind = static`.

Это нужно, чтобы после reopen можно было восстановить runtime registry из metadata.

## 6. Чего нет

Пока нет:

- dynamic loading native libraries;
- plugin search path;
- `CREATE EXTENSION` с файлами;
- sandbox;
- WASM extensions;
- table functions;
- aggregate functions;
- extension-owned catalog objects;
- unload/reload;
- permission model.

## 7. Куда развивать

Безопасный порядок:

```text
1. расширить static scalar functions;
2. добавить тесты reopen/recovery для loaded extensions;
3. описать ABI ownership rules;
4. добавить platform-specific loader только за feature flag;
5. рассмотреть WASM как более безопасную boundary;
6. только потом делать native plugin API публичным.
```
