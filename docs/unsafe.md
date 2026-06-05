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
