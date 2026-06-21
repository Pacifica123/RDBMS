# Android smoke

Stage 10 does not add an Android application. It adds the smallest Android-facing
library boundary:

```text
crates/rdbms_android -> cdylib/rlib
platform/android/app/src/main/java/dev/rdbms/NativeSmoke.java
```

The native library exports three JNI-shaped smoke functions:

```text
NativeSmoke.stage()      -> 10
NativeSmoke.abiVersion() -> 1
NativeSmoke.add(20, 22)  -> 42
```

The Rust side does not dereference JNI pointers. The functions only prove that a
stable symbol name exists and that the crate can be built as an Android native
library.

CI builds the library for `aarch64-linux-android`. A real Android application,
Gradle project, instrumented tests and device/emulator execution are later work.
