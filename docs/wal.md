# WAL

## 1. Назначение

WAL нужен не “для галочки”, а для восстановления после сбоя. Если страница могла быть частично записана или данные могли попасть на диск не в том порядке, recovery должен иметь достаточно информации для приведения базы к согласованному состоянию.

## 2. Правило

```text
Write-Ahead Logging: log record должен быть durable раньше страницы данных, которую он описывает.
```

## 3. Базовые понятия

```text
LSN      — позиция записи в WAL
page_lsn — последний LSN, применённый к странице
redo     — повторить изменение, если оно ещё не отражено на странице
undo     — отменить изменение незавершённой транзакции; не MVP первой итерации
```

## 4. MVP WAL records

Минимальный набор для первых этапов:

```text
BeginTx
PageUpdate { page_id, before_checksum, after_image_or_delta }
CommitTx
AbortTx
Checkpoint
```

На раннем milestone можно начать с page image WAL, а не с оптимальных дельт. Это проще для recovery tests.

## 5. Sync boundary

Commit считается durable только после sync WAL до commit record. Точная реализация зависит от VFS и платформы, поэтому должна проходить через `rdbms_vfs`.

## 6. Что не делаем сразу

Не начинаем с полного ARIES, nested top actions, сложного checkpointing и fine-grained undo. Сначала нужен воспроизводимый маленький recovery loop.

## 7. Тесты

Каждый WAL milestone должен иметь crash tests:

1. сбой до записи WAL;
2. сбой после WAL до data page;
3. сбой после частичной data page;
4. сбой после commit;
5. повторный recovery должен быть идемпотентным.
