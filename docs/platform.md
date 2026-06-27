# Переносимость и платформенные проверки

## 1. Что добавил Этап 10

Этап 10 не добавляет новую storage-фичу. Он проверяет, что текущий стек можно собирать и частично проверять на разных платформах.

Добавлено:

- Windows path/sync smoke в `rdbms_vfs`;
- Android native library crate `rdbms_android`;
- JNI-shaped smoke symbols;
- Java wrapper `NativeSmoke`;
- CI matrix для Linux, Windows, macOS;
- Android `aarch64-linux-android` library build в CI.

Это не Android-приложение и не мобильный SQL API.

## 2. Windows

VFS уже скрывает file IO за `rdbms_vfs::VfsFile`. Этап 10 добавляет smoke tests для путей и `sync_data`.

Проверяется:

- файл можно создать по обычному platform path;
- random-access write/read работает;
- `sync_data` проходит через VFS boundary;
- Windows job в CI компилирует workspace.

Это не доказывает полную crash-consistency на Windows. Для этого нужны отдельные crash tests и platform-specific fsync исследования.

## 3. Android crate

`crates/rdbms_android` собирается как:

```text
rlib
cdylib
```

`cdylib` — форма native library, которую можно загрузить через JNI.

Crate линкуется с `rdbms_core` и `rdbms_sql`, поэтому Android build проверяет, что верхняя часть Rust stack компилируется под Android target.

## 4. JNI smoke

Экспортированные функции:

```text
Java_dev_rdbms_NativeSmoke_stage()      -> 10
Java_dev_rdbms_NativeSmoke_abiVersion() -> 1
Java_dev_rdbms_NativeSmoke_add(20, 22)  -> 42
```

Java wrapper:

```text
platform/android/app/src/main/java/dev/rdbms/NativeSmoke.java
```

Rust функции не dereference-ят JNI pointers. Они принимают их как opaque handles и возвращают маленькие значения. Это узкая FFI-boundary проверка, не полноценная интеграция.

## 5. CI matrix

Обычный workflow проверяет:

```bash
cargo check --workspace
cargo test --workspace
```

на Linux/macOS/Windows.

Android job устанавливает target и NDK, затем собирает native library для:

```text
aarch64-linux-android
```

Android job не запускает приложение на emulator/device.

## 6. Текущий portability contract

Граница такая:

```text
storage code -> VFS -> platform file API
SQL/core code -> Rust library API -> Android JNI smoke crate
CI -> Linux/Windows/macOS/Android build coverage
```

Будущее platform-specific поведение должно оставаться за `rdbms_vfs` или отдельным platform crate. Storage, catalog, tx и SQL не должны напрямую разрастаться `cfg(windows)`/`cfg(android)` ветками без причины.

## 7. Чего ещё нет

Пока нет:

- Android Gradle project;
- Android instrumentation tests;
- SQL API через JNI;
- mobile storage directory policy;
- file locking;
- platform crash matrix;
- packaging/release native library;
- iOS target.

## 8. Следующие шаги

Разумный порядок:

```text
1. добавить больше VFS tests для platform paths;
2. сделать fault-injection VFS;
3. добавить Android host-side API только после стабилизации SQL/session API;
4. добавить emulator/device smoke;
5. отдельно исследовать fsync/file-locking semantics для каждой platform.
```
