# Политика unsafe

## 1. Базовое правило

Внутренние crates RDBMS должны по возможности оставаться без `unsafe`.

Storage, WAL, recovery, catalog, tx, SQL и index layers можно реализовать безопасным Rust-кодом. Если где-то появляется `unsafe`, нужно отдельно объяснить:

- почему без него нельзя;
- какие инварианты обязан держать вызывающий код;
- как это тестируется;
- почему unsafe boundary не протекает в остальной проект.

## 2. FFI boundary

Исключение — узкие FFI-boundary crates.

`rdbms_android` экспортирует JNI-shaped symbols. Сами функции принимают raw pointers как opaque handles и не dereference-ят их. Поэтому текущий Этап 10 не добавляет произвольные unsafe blocks в стек хранения.

Будущие native extensions тоже потребуют FFI boundary. Она должна быть изолирована в отдельном crate, а не размазана по catalog/tx/sql.

## 3. Что запрещено без отдельного решения

Нельзя незаметно добавлять unsafe для:

- ускорения parser-а;
- ручного управления памятью в page layer;
- обхода borrow checker-а;
- aliasing mutable references;
- чтения непроверенных bytes как struct через transmute;
- plugin loading без описанной ABI policy.

## 4. Проверка

Перед merge любого unsafe-изменения нужно обновить этот документ и добавить тесты вокруг boundary. Для обычных stage-патчей безопаснее держать `unsafe_code` запрещённым в crate settings там, где это возможно.
