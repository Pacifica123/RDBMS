# Android-проверка

Этап 10 не добавляет Android-приложение. Он добавляет минимальную native-library boundary для Android.

Файлы:

```text
crates/rdbms_android/
platform/android/app/src/main/java/dev/rdbms/NativeSmoke.java
```

Rust crate собирается как `rlib` и `cdylib`. `cdylib` — форма, подходящая для JNI loading.

Экспортированные JNI-shaped функции:

```text
NativeSmoke.stage()      -> 10
NativeSmoke.abiVersion() -> 1
NativeSmoke.add(20, 22)  -> 42
```

Rust side не dereference-ит JNI pointers. Функции принимают raw pointers как opaque handles и только возвращают простые значения.

Что это проверяет:

- native symbol names существуют;
- crate можно собрать как Android native library;
- Android crate линкуется с `rdbms_core` и `rdbms_sql`;
- CI может собрать `aarch64-linux-android` target.

Чего здесь нет:

- Android app;
- Gradle project;
- emulator/device tests;
- SQL execution через JNI;
- mobile storage policy.
