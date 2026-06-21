# Recovery

## 1. Цель

Recovery должен отвечать на один вопрос: какое состояние базы допустимо после падения процесса или ОС.

Для MVP допустима простая модель:

```text
committed changes survive;
uncommitted changes do not become visible;
corrupted pages are detected, not silently accepted.
```

## 2. Стартовая стратегия

Первый recovery loop:

1. открыть файл базы;
2. прочитать file header;
3. прочитать WAL с последней checkpoint-позиции или с начала;
4. проверить checksum WAL records;
5. применить redo для committed records;
6. проверить page checksums;
7. вернуть typed error при corruption.

На текущем Stage 3 реализованы только WAL scan и `PageImageRedo` hook. Сам recovery loop при `Database::open` ещё не реализован.

## 3. Идемпотентность

Recovery можно запускать несколько раз подряд. Второй запуск не должен менять состояние иначе, чем первый.

## 4. Crash testing

Тесты отказов должны использовать fault-injection VFS, а не ручное выключение компьютера. Минимальная модель: VFS умеет оборвать write, потерять suffix, вернуть short write или ошибку sync.

## 5. Граница с транзакциями

Полноценный undo/MVCC можно отложить. Но уже в MVP нельзя писать код так, будто транзакции будут “приклеены потом” без LSN, tx id и visibility rules.
