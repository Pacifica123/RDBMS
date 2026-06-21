# Platform ports

## 1. Stage 10 boundary

Stage 10 is not a new storage feature. It is a portability smoke layer around the
existing storage stack.

Implemented in this stage:

```text
Windows path/fsync smoke test in rdbms_vfs;
Android cdylib/rlib crate;
JNI-shaped smoke symbols;
Android Java smoke wrapper;
GitHub Actions CI matrix for Linux, Windows, macOS and Android library build.
```

Not implemented in this stage:

```text
Android app;
Gradle project;
instrumented Android emulator/device tests;
Windows installer;
platform-specific file locking;
mobile storage policy;
JNI query API for SQL execution.
```

## 2. Windows path/fsync smoke

The VFS already isolates random-access file IO behind `rdbms_vfs::VfsFile`. Stage
10 adds a smoke test that creates a database file with spaces and non-ASCII text
in the path, writes a page, calls `sync_data`, reopens the file and reads the
page back.

The Windows-only variant is guarded with `#[cfg(windows)]` and runs in the
Windows CI job. A cross-platform variant runs on all platforms as a simpler path
and sync smoke.

This does not prove all crash-consistency behavior on Windows. It only proves
that the current `StdVfs` path handling, random-access IO and sync boundary are
exercised on Windows.

## 3. Android library crate

Stage 10 adds:

```text
crates/rdbms_android
```

The crate is built as both:

```text
rlib;
cdylib.
```

The `cdylib` output is the native library shape expected by Android JNI loading.
The crate links against `rdbms_core` and `rdbms_sql`, so the Android target checks
more than an empty Rust library.

## 4. JNI smoke

The exported JNI-shaped symbols are:

```text
Java_dev_rdbms_NativeSmoke_stage
Java_dev_rdbms_NativeSmoke_abiVersion
Java_dev_rdbms_NativeSmoke_add
```

The corresponding Java wrapper is:

```text
platform/android/app/src/main/java/dev/rdbms/NativeSmoke.java
```

The native functions only return small integers. They do not dereference JNI
pointers. The Android crate is a narrow FFI-boundary exception to the default
workspace unsafe-code lint because exported native symbols are part of that
boundary; it still does not add arbitrary unsafe blocks.

## 5. CI matrix

The Stage 10 workflow is:

```text
.github/workflows/ci.yml
```

It runs the Rust workspace on:

```text
ubuntu-latest;
windows-latest;
macos-latest.
```

It also has an Android job that installs the Android target and NDK, then runs:

```bash
cargo build -p rdbms_android --target aarch64-linux-android --release
cargo test -p rdbms_android
```

The Android job builds the native library. It does not run it on an Android
emulator.

## 6. Current portability contract

The project now treats portability as a first-class boundary:

```text
storage code -> VFS -> platform file API
SQL/core code -> Rust library API -> Android JNI smoke crate
CI -> Linux/Windows/macOS/Android build coverage
```

Any future platform-specific storage behavior should stay behind `rdbms_vfs` or a
similarly narrow crate boundary.
