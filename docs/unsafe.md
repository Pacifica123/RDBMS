# Unsafe policy

По умолчанию unsafe запрещён на уровне workspace lint.

Разрешённые будущие зоны:

1. FFI boundary для расширений;
2. низкоуровневый parsing page bytes после проверки bounds/alignment;
3. platform-specific VFS;
4. performance-critical участок после тестов и ревью.

Каждый unsafe block должен иметь комментарий с инвариантом:

```rust
// SAFETY: buffer length was checked against PAGE_SIZE and the pointer is aligned for Header.
```

Если инвариант нельзя написать простым языком, unsafe block не готов.


## Stage 10 JNI smoke

`rdbms_android` exports JNI-shaped `extern "system"` functions, but does not dereference raw JNI pointers. The pointer arguments are opaque handles in this stage.

The crate does not opt into workspace `unsafe_code = "deny"` yet because exported native symbols such as `#[no_mangle]` are treated as an unsafe-code surface by the Rust lint. This is a narrow FFI-boundary exception, not permission to add arbitrary unsafe blocks.

A future JNI SQL API may need explicit unsafe handling for strings, buffers and object lifetimes. That work must document ownership and release rules before crossing the boundary.
