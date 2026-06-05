# Extension ABI

## 1. Зачем отдельный ABI

Внутренние Rust trait удобны внутри workspace, но они не являются стабильным внешним контрактом для динамических расширений. Версии компилятора, layout, panic behavior и ownership делают такой контракт хрупким.

## 2. MVP-подход

Первая версия расширений — проектный контракт, а не обязательная runtime-фича. Документ нужен сейчас, чтобы не замкнуть ядро на нестабильные внутренние типы.

## 3. Native ABI sketch

```rust
#[repr(C)]
pub struct RdbmsExtensionDescriptor {
    pub abi_version: u32,
    pub name_ptr: *const u8,
    pub name_len: usize,
    pub init: Option<extern "C" fn(*mut RdbmsHost) -> RdbmsStatus>,
}
```

Во внешней ABI нет `String`, `Vec<T>`, `Box<dyn Trait>` и Rust panic через границу.

## 4. Capabilities

Расширение должно явно объявлять capabilities:

```text
scalar_function
aggregate_function
table_provider
index_provider
storage_adapter
unsafe_native
```

Чем ближе расширение к storage, тем строже требования.

## 5. Ownership

Правило по умолчанию: сторона, которая выделила память, её и освобождает. Для буферов нужна явная функция release.

## 6. WASM-track

WASM может стать более безопасной альтернативой native plugins для части расширений. Но он не отменяет need в host API, versioning и capabilities.
