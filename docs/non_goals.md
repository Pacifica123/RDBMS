# Что проект пока не обещает

## 1. Не строим SQL-first СУБД

SQL-консоль сама по себе не доказывает, что есть СУБД. До широкого SQL нужны страницы, WAL, recovery, catalog и transaction boundary.

## 2. Не используем JSON как формат базы

JSON можно использовать как debug export или временный тестовый snapshot. Он не является page format, WAL, recovery protocol или стабильным форматом базы.

## 3. Не продолжаем старый `RDBMS-master`

Старый код сохранён как legacy. Его полезно читать как исторический материал, но не как фундамент нового ядра.

## 4. Не делаем “всё сразу”

PostgreSQL + SQLite + RocksDB + ClickHouse в одном учебном проекте — плохая цель. Проект идёт маленькими слоями с проверяемыми инвариантами.

## 5. Не выдаём Rust traits как plugin ABI

Внутренний Rust trait удобен внутри workspace, но не является стабильной внешней ABI. Для plugin-ов нужна C-compatible boundary или WASM.

## 6. Не начинаем с mmap

`mmap` может быть полезен позже, но сначала нужен простой VFS/page store с понятными sync и fault-injection tests.

## 7. Не обещаем промышленную надёжность

Текущий проект — учебно-инженерная база. Он уже имеет WAL-backed transactions v0, но ещё не имеет всей инфраструктуры промышленной СУБД: crash matrix, MVCC, checkpoint, backup, monitoring, security model и compatibility policy.

## 8. Не обещаем стабильный файл между версиями

Пока нет отдельного compatibility milestone, физические форматы можно менять. Любое такое изменение должно быть явно описано в документации и тестах.
