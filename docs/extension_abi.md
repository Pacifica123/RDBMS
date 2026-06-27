# ABI расширений

## 1. Назначение

`rdbms_ext_abi` фиксирует направление для будущих внешних расширений. Внешним plugin-ам нельзя отдавать Rust trait как стабильный контракт: Rust ABI и layout не являются подходящей долгосрочной границей.

Поэтому внешняя граница должна быть C-compatible или изолированной, например через WASM.

## 2. Текущий ABI sketch

Сейчас crate содержит:

```text
RDBMS_EXT_ABI_VERSION = 1
abi_version_supported(version)
RdbmsStatus
RdbmsHost
RdbmsExtensionDescriptor
```

`RdbmsHost` — opaque handle. Extension не должна знать внутреннюю структуру host-а.

`RdbmsExtensionDescriptor` содержит ABI version, имя extension и init callback.

## 3. Что важно для ABI

Будущий ABI должен явно описывать:

- кто владеет памятью;
- как передаются строки;
- как возвращаются ошибки;
- как host вызывает functions;
- как extension регистрирует SQL objects;
- какие версии совместимы;
- что происходит при unload;
- какие функции доступны extension-у.

Без этих правил native plugin loading лучше не включать.

## 4. Почему static registry отдельно

`rdbms_extension` уже умеет static registry. Это рабочий runtime path для Этап 9.

`rdbms_ext_abi` — не runtime loader. Это контрактная заготовка для будущего этапа.

## 5. Ограничения

Пока нет:

- `dlopen`/`LoadLibrary`;
- проверки подписи plugin-а;
- sandbox;
- отдельного process boundary;
- стабильного value ABI;
- callback API для storage/catalog;
- документа compatibility promise.

## 6. Следующий разумный шаг

Сначала нужно сделать больше static extensions и стабилизировать SQL value boundary. Потом можно выбрать направление: native ABI или WASM. Для учебного проекта WASM может быть безопаснее, но native ABI полезен как инженерная практика.
