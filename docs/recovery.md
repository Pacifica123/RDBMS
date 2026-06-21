# Recovery

## 1. Цель

Recovery отвечает на один вопрос: какое состояние базы допустимо после падения процесса или ОС.

Для текущего MVP применяется простая модель:

```text
committed changes survive;
uncommitted changes do not become visible through recovery;
corrupted WAL/page bytes are detected, not silently accepted.
```

## 2. Stage 4 recovery loop

Stage 4 добавляет `rdbms_recovery` — первый redo-only loop, который связывает `rdbms_vfs` и `rdbms_wal`.

Текущий open path:

```text
1. открыть data file через VFS;
2. открыть WAL file через VFS;
3. просканировать WAL с offset 0;
4. проверить WAL magic/version/checksum/length/LSN;
5. выбрать PageImage records только для транзакций с CommitTx и без AbortTx;
6. проверить каждую page image через Page::from_bytes;
7. записать committed full-page image в PageFile;
8. sync data file;
9. вернуть RecoveredDatabase с PageFile и RecoveryReport.
```

Публичная граница Stage 4:

```text
DatabasePaths;
RecoveryReport;
RecoveredDatabase<F: VfsFile>;
open_database(vfs, paths);
recover_page_file(page_file, wal_file).
```

## 3. Идемпотентность

Recovery можно запускать несколько раз подряд. В WAL v0 redo работает через full-page image. Поэтому повторная запись того же committed image должна оставлять тот же page state.

Текущий тест проверяет повторный `open_database` на одном и том же WAL и data file.

## 4. Что считается committed

Stage 4 использует правило из `rdbms_wal::redo_committed_page_images`:

```text
PageImage применяется, если tx_id имеет CommitTx и не имеет AbortTx.
PageImage игнорируется, если tx_id не имеет CommitTx.
PageImage игнорируется, если tx_id имеет AbortTx.
```

Это ещё не полноценная transaction visibility model. Это только минимальная граница, чтобы uncommitted WAL records не попадали в data file при recovery.

## 5. Текущие ограничения

Сейчас нет:

```text
undo;
ARIES;
checkpoint state;
recovery start position;
page_lsn-based skip;
WAL truncation после checkpoint;
commit protocol между WAL sync и data page flush;
fault-injection VFS;
file header bootstrap;
catalog bootstrap.
```

`page_lsn` уже есть в page header, но Stage 4 не обновляет и не использует его для пропуска redo. Это отдельный шаг после стабилизации commit protocol.

## 6. Crash testing

Тесты отказов должны использовать fault-injection VFS, а не ручное выключение компьютера. Минимальная будущая модель: VFS умеет оборвать write, потерять suffix, вернуть short write или ошибку sync.

Stage 4 пока проверяет только обычный reopen/recovery path, WAL corruption propagation и идемпотентность повторного recovery.

## 7. Граница с транзакциями

Полноценный undo/MVCC отложены. Но уже сейчас WAL records имеют `TxId`, commit marker и page image redo path, чтобы будущий transaction layer не приклеивался поверх storage вслепую.
