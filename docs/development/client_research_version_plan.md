# Вердикт по MVP-клиенту и версионный план ядра

Дата: 2026-06-27.

Статус: рабочий план после исследования MVP-клиента.

Связанные материалы:

- внешняя заметка `RDBMS_MVP_Client_Research_Notes.md`;
- внешний прототип `rdbms-client-mvp`;
- текущий код ядра RDBMS версии `0.10.0.1`;
- текущий roadmap `docs/roadmap.md`;
- правила версионирования `docs/development/versioning.md`.

Этот документ не заменяет `docs/roadmap.md`. Он уточняет ближайшую линию развития после появления внешнего MVP-клиента и первых ручных стресс-проверок.

## 1. Исходный сигнал от клиента

MVP-клиент показал полезную вещь: проект уже можно дергать не только тестами, но и внешним инструментом разработки. Это сразу проявило не синтаксические, а ядровые вопросы:

- большой рост `.wal` при большом количестве маленьких `INSERT`;
- рост времени одиночных `INSERT` по мере увеличения таблицы;
- необходимость benchmark-режима без браузерной отрисовки всех строк;
- необходимость file stats и WAL stats;
- вопрос, использует ли SQL path индекс;
- вопрос, что происходит с recovery и WAL при reopen.

Главный вывод: клиент не говорит, что ядро плохое. Он показывает, что следующие этапы должны быть не про расширение SQL вширь, а про управляемость durability/write path.

## 2. Короткий вердикт

Вердикт: гипотеза про раздутый WAL подтверждается по коду. В текущей архитектуре это ожидаемое следствие full-page-image WAL и autocommit на каждый SQL `INSERT`.

Вердикт: гипотеза про дорогие одиночные `INSERT` частично подтверждается по коду. Есть минимум три теоретических причины: отдельная транзакция на каждый `INSERT`, sync WAL + sync data на каждый commit, линейный поиск heap-страницы с местом.

Вердикт: гипотеза про обязательную проверку index path подтверждается. Код уже пытается использовать B+Tree для `WHERE indexed_column = literal`, но нет `EXPLAIN` и нет метрик, которые показывают выбранный путь. Поэтому пользователь видит только результат, а не план.

Вердикт: в ядре есть более важный риск, чем размер WAL. `TxId` начинается с `1` при каждом открытии `TransactionalStore`, а recovery определяет committed transaction по одному `TxId` на всём WAL. При append-only WAL это может стать crash-correctness проблемой после reopen и повторного использования `TxId`.

Вердикт: стандартный SQL/tx open path сейчас не делает recovery автоматически. `open_transactional_store` открывает catalog store и WAL writer, но не прогоняет `rdbms_recovery::open_database`. Это нормально для текущей учебной стадии, но плохо как основа для внешнего клиента.

Следствие: перед `CHECKPOINT`, bulk insert и широким SQL надо стабилизировать safety baseline для WAL/tx/recovery.

## 3. Проверка гипотез по коду

### 3.1. Почему WAL примерно по странице на INSERT

В `rdbms_wal` запись `PageImage` хранит полный образ страницы `PAGE_SIZE = 4096` байт. Заголовок WAL record имеет размер `40` байт. Поэтому один `PageImage` занимает около `4136` байт без учёта begin/commit records.

Кодовая основа:

- `crates/rdbms_wal/src/lib.rs`: `WAL_HEADER_SIZE = 40`;
- `crates/rdbms_wal/src/lib.rs`: `WalRecordKind::PageImage { image: Box<[u8; PAGE_SIZE]> }`;
- `crates/rdbms_wal/src/lib.rs`: `payload()` для `PageImage` возвращает полный `image`;
- `crates/rdbms_wal/src/lib.rs`: `encoded_len_for_kind = WAL_HEADER_SIZE + payload_len`.

Обычный SQL `INSERT` проходит так:

1. `rdbms_sql::execute_insert` делает `store.begin()`;
2. `transaction.insert_row(...)` меняет heap page;
3. `transaction.commit()` пишет dirty pages как full-page images;
4. затем пишет `CommitTx`;
5. затем делает `wal.sync_data()`;
6. затем пишет страницы в data file;
7. затем делает `store.sync_data()`.

Кодовая основа:

- `crates/rdbms_sql/src/lib.rs:292-319`;
- `crates/rdbms_tx/src/lib.rs:125-134`;
- `crates/rdbms_tx/src/lib.rs:355-371`.

Для одиночного `INSERT` в уже существующую heap page минимальный WAL близок к:

```text
BeginTx       40 bytes
PageImage   4136 bytes
CommitTx      40 bytes
----------------------
Итого       4216 bytes
```

Наблюдение из клиента около `4269 bytes/INSERT` хорошо совпадает с этой моделью. Отклонение объясняется create table, новыми heap pages, catalog page images и округлениями измерения.

Если `INSERT` затрагивает индекс, WAL может стать больше: `insert_index_entry` меняет index pages и дополнительно помечает catalog page dirty. Это значит, что indexed insert потенциально пишет несколько full-page images за одну строку.

### 3.2. Почему одиночные INSERT дорожают

Текущий SQL layer не имеет SQL-visible transaction state. Каждый `INSERT INTO ... VALUES (...)` выполняется как самостоятельный statement и создаёт отдельную transaction.

Кодовая основа:

- `crates/rdbms_sql/src/lib.rs:121-137` — `Statement::Insert` ведёт в `execute_insert`;
- `crates/rdbms_sql/src/lib.rs:292-319` — `execute_insert` делает `store.begin()`, `insert_row`, `commit`;
- `crates/rdbms_sql/src/lib.rs:766-783` — parser принимает только `CREATE`, `INSERT`, `LOAD`, `SELECT`; `BEGIN/COMMIT/ROLLBACK` нет;
- `crates/rdbms_tx/src/lib.rs:355-371` — commit делает WAL sync и data sync.

Отдельно insert path ищет страницу для вставки линейно по списку heap pages:

```text
for page_id in heap_pages {
    read/load page;
    try insert_record;
    if no space: continue;
}
```

Кодовая основа:

- `crates/rdbms_tx/src/lib.rs:296-319`;
- `crates/rdbms_catalog/src/lib.rs:578-600`.

Поэтому при росте таблицы возможны два независимых эффекта:

1. каждая строка платит цену отдельного commit;
2. чем больше заполненных страниц, тем дороже найти страницу с местом, если нет free-space map или last-page hint.

На текущем уровне нельзя честно сказать, какой вклад больше. Для этого нужны метрики страниц, WAL records и sync count.

### 3.3. Что именно меряет MVP-клиент

В клиенте `elapsedMs` снимается вокруг `session.execute(statement, &[])`. JSON-сериализация результата идёт уже после получения `ExecResult`.

Кодовая основа во внешнем клиенте:

- `rdbms-client-mvp/src/main.rs:212-214` — замер вокруг `session.execute`;
- `rdbms-client-mvp/src/main.rs:510-545` — JSON результата собирается после получения elapsed;
- `rdbms-client-mvp/src/main.rs:1132-1142` — browser grid рендерит rows отдельно.

Значит, показанные миллисекунды ближе к времени SQL engine + материализации `ExecResult`, а не к полной стоимости HTTP/JSON/browser. Но `SELECT *` всё равно материализует все строки в `Vec<Vec<Value>>`, поэтому это не чистый storage benchmark.

### 3.4. Должен ли SELECT увеличивать WAL

Теоретически обычный `SELECT` не должен увеличивать WAL.

Кодовая основа:

- `execute_select` не вызывает `store.begin()`;
- `candidate_rows` либо делает `store.full_scan`, либо `store.lookup_index`;
- `WalWriter::append` вызывается в transaction begin/commit/rollback, но не в select path.

Кодовые участки:

- `crates/rdbms_sql/src/lib.rs:348-414`;
- `crates/rdbms_tx/src/lib.rs:88-101`;
- `crates/rdbms_tx/src/lib.rs:355-385`.

Если `.wal` растёт от чистого `SELECT`, это будет отдельный баг или побочный эффект не самого SQL select path.

### 3.5. Используется ли индекс в SQL path

Да, код уже содержит index lookup path для простого equality predicate.

Условие:

```text
WHERE indexed_column = literal
```

Путь:

1. `candidate_rows` ищет index relation по колонке;
2. значение predicate переводится в `IndexKey`;
3. вызывается `store.lookup_index`;
4. найденные `RowId` читаются из heap;
5. predicate всё равно перепроверяется после чтения строки.

Кодовая основа:

- `crates/rdbms_sql/src/lib.rs:391-414`;
- `crates/rdbms_sql/src/lib.rs:447-462`;
- `crates/rdbms_tx/src/lib.rs:66-85`.

Ограничение: нет `EXPLAIN`, нет счётчика выбранного плана и нет теста, который доказывает именно отсутствие full scan. Текущие функциональные тесты проверяют результат, но не план выполнения.

### 3.6. Recovery сейчас линейно зависит от WAL

`recover_page_file` читает весь WAL через `WalReader::read_all()`, затем `redo_committed_page_images` проходит records, потом применяет committed page images.

Кодовая основа:

- `crates/rdbms_recovery/src/lib.rs:89-98`;
- `crates/rdbms_wal/src/lib.rs:207-253`;
- `crates/rdbms_wal/src/lib.rs:274-320`.

Без checkpoint state, recovery start position, pageLSN skip и WAL truncation рост `.wal` прямо ухудшает recovery path. Даже если обычный клиентский open сейчас не вызывает recovery автоматически, полноценный safe open должен будет столкнуться с этой ценой.

### 3.7. Важный риск: TxId reuse после reopen

`TransactionalStore::new` всегда ставит `next_tx_id = FIRST_TX_ID.0`, где `FIRST_TX_ID = TxId(1)`. Значит после reopen новые transaction id начинаются заново.

Кодовая основа:

- `crates/rdbms_tx/src/lib.rs:19`;
- `crates/rdbms_tx/src/lib.rs:40-48`;
- `crates/rdbms_tx/src/lib.rs:162-168`.

Recovery при этом строит множества `committed` и `aborted` только по `TxId`, без epoch, без transaction begin LSN и без уникального transaction identity на всём WAL.

Кодовая основа:

- `crates/rdbms_wal/src/lib.rs:282-320`.

Опасный сценарий:

```text
1. В старом запуске TxId(1) committed.
2. Проект закрыли и открыли снова.
3. Новый запуск снова выдаёт TxId(1).
4. Во время commit нового TxId(1) WAL успел получить PageImage, но не получил CommitTx.
5. Recovery видит, что TxId(1) где-то в старом WAL committed.
6. Новый uncommitted PageImage может быть ошибочно принят как committed.
```

Да, текущий commit делает sync только после commit marker, но crash-сценарии нельзя доказывать надеждой на то, что unsynced bytes не окажутся на диске. Для учебной версии это допустимый известный долг. Для следующей durability-линии это надо закрыть раньше checkpoint.

### 3.8. Standard open path не делает recovery

`open_transactional_store` открывает `CatalogStore` и `WalWriter`, но не вызывает `rdbms_recovery::open_database`.

Кодовая основа:

- `crates/rdbms_tx/src/lib.rs:176-188`.

Следствие: внешний клиент, который напрямую использует `open_transactional_store`, не получает automatic recovery on open. Для текущего MVP это объяснимо, но дальнейший клиентский workflow должен иметь один безопасный open API.

## 4. Что считать подтверждённым, а что нет

| Наблюдение / гипотеза | Вердикт | Комментарий |
| --- | --- | --- |
| WAL раздут из-за full-page images | Подтверждено | Один обычный heap insert пишет минимум один 4 KiB page image плюс begin/commit records. |
| Каждый SQL INSERT — отдельная transaction | Подтверждено | SQL `execute_insert` сам делает `begin` и `commit`. |
| Нет checkpoint/truncate | Подтверждено | Есть enum marker `Checkpoint`, но нет checkpoint protocol и нет truncation. |
| Recovery замедлится от роста WAL | Теоретически подтверждено | Recovery читает весь WAL и replay-ит committed page images. |
| INSERT замедляется из-за поиска свободного места | Вероятно | Код сканирует heap pages линейно; точный вклад надо мерить. |
| INSERT замедляется из-за fsync | Вероятно | Commit делает WAL sync и data sync на каждый autocommit statement. Точный вклад надо мерить. |
| SELECT увеличивает WAL | Скорее нет | Select path не пишет WAL. Если рост есть, это отдельный баг. |
| SQL path не использует index | Скорее нет | Код index path есть, но нужен `EXPLAIN` и plan test. |
| Клиентский elapsed включает browser rendering | Нет | `elapsedMs` снимается до JSON/browser, но включает materialized SQL result. |
| Самая срочная engine-фича — checkpoint | Частично | Checkpoint важен, но перед ним надо закрыть TxId/recovery/open safety baseline. |

## 5. Глобальные цели и целевые версии

Ниже версии указаны как ориентировочные якоря. Реальные `QUANTUM`-номера могут сдвинуться, если между этими задачами появятся внеплановые devctl-патчи. Семантическая линия и порядок важнее конкретного номера.

Текущая точка после принятия этого документа должна стать `0.10.0.2`.

### 5.1. `0.11.0.x` — WAL/tx/recovery safety baseline

Цель: убрать самые опасные неопределённости перед checkpoint и performance work.

Почему первая: нельзя строить checkpoint, compaction и safe client open поверх неоднозначной transaction identity и ручного recovery path.

Главные результаты:

- transaction identity не переиспользуется опасно после reopen;
- recovery tests покрывают reused TxId или новая схема делает reuse невозможным;
- появляется safe open path, который явно решает вопрос recovery;
- dev docs фиксируют crash assumptions текущей версии.

### 5.2. `0.12.0.x` — measurements and diagnostics v0

Цель: перестать гадать, что именно тормозит и сколько WAL records создаётся.

Почему после safety baseline: метрики должны измерять корректный путь, а не временную неопределенность recovery.

Главные результаты:

- engine-level counters для page read/write, WAL append, WAL bytes, sync calls;
- reproducible benchmark harness без browser UI;
- recovery benchmark smoke;
- file-size stats для `.db` и `.wal`;
- базовые performance docs.

### 5.3. `0.13.0.x` — checkpoint + WAL compaction v0

Цель: сделать так, чтобы подтверждённая загрузка данных не оставляла бесконечно растущий WAL.

Почему здесь: размер WAL уже подтверждён как архитектурное следствие текущего write path.

Главные результаты:

- `CHECKPOINT` API v0;
- safe WAL truncation после checkpoint;
- recovery после checkpoint;
- тест, что WAL не остаётся десятками размеров DB после checkpoint;
- документированная sync policy.

### 5.4. `0.14.0.x` — explicit SQL transactions and batching v0

Цель: дать пользователю и клиенту возможность выполнить много inserts в одной transaction.

Почему после checkpoint: batching уменьшит количество commit records и sync calls, но сам WAL всё равно должен быть управляемым.

Главные результаты:

- SQL `BEGIN`, `COMMIT`, `ROLLBACK`;
- `SqlSession` хранит active transaction state;
- batch insert сценарии проходят recovery/reopen;
- autocommit остаётся совместимым поведением по умолчанию.

### 5.5. `0.15.0.x` — heap allocation and free-space tracking v0

Цель: убрать линейный поиск подходящей heap page на каждую вставку.

Почему после batching: сначала надо снизить commit overhead, затем отдельно лечить поиск места внутри heap.

Главные результаты:

- last-page insert hint или free-space map v0;
- тесты на многостраничную таблицу;
- метрика количества прочитанных страниц на insert;
- сравнение до/после для 1k/5k/10k rows.

### 5.6. `0.16.0.x` — planner transparency and index confidence v0

Цель: сделать выбранный путь выполнения видимым и проверяемым.

Почему здесь: index path уже есть, но без `EXPLAIN` и plan metrics он плохо проверяется внешним клиентом.

Главные результаты:

- `EXPLAIN SELECT ...`;
- явные plan nodes: `SeqScan`, `IndexLookup`;
- тест, который доказывает, что indexed equality predicate не делает full scan;
- клиент и dev scripts могут показывать план.

### 5.7. `0.17.0.x` — recovery/crash hardening v1

Цель: перейти от smoke recovery к небольшой, но явной crash matrix.

Почему после checkpoint/batching/free-space: к этому моменту write path уже сложнее, и его надо закрепить fault-injection тестами.

Главные результаты:

- fault-injection VFS для write/sync failures;
- crash points around WAL append/sync/data write/data sync/checkpoint;
- recovery idempotency matrix;
- pageLSN redo skip decision: реализовать или явно отложить;
- документация, где заканчивается учебная гарантия.

### 5.8. `0.18.0.x` — client-facing introspection API v0

Цель: дать внешнему MVP-клиенту стабильные источники диагностической информации.

Почему после core hardening: клиент не должен парсить внутренности файлов и угадывать состояние ядра.

Главные результаты:

- system tables или internal API для tables/indexes/pages/wal_stats;
- безопасные file stats;
- query history hooks или execution diagnostics;
- отдельная граница между dev diagnostics и будущим public API.

## 6. План квантов-патчей

Номера ниже предполагают, что этот документ будет принят как `p000002` и версия станет `0.10.0.2`. Если появятся промежуточные патчи, номера надо сдвинуть, но порядок целей сохранить.

### 6.1. `0.11.0.x` — WAL/tx/recovery safety baseline

#### `p000003` → `0.11.0.3` — transaction identity research and failing tests

Тип: `minor`.

Чеклист:

- добавить документ `docs/development/tx_identity.md`;
- добавить тест или ignored-test, который моделирует reused `TxId` после reopen;
- вручную построить WAL sequence: old `TxId(1)` committed, later `TxId(1)` page image without commit;
- показать, что текущий `redo_committed_page_images` не различает эти transaction instances;
- описать выбранное решение: persistent next_tx_id, WAL epoch, begin LSN identity или другой вариант.

Критерий приёмки:

- проблема воспроизводится как тестовый сценарий или явно зафиксирована как failing/ignored test;
- решение выбрано до изменения формата WAL.

#### `p000004` → `0.11.0.4` — persistent transaction identity v0

Тип: `quantum`.

Чеклист:

- реализовать выбранную схему уникальности transaction identity;
- если меняется WAL record format, поднять WAL format version и добавить migration/compat note;
- обновить recovery grouping так, чтобы committed marker относился к конкретной transaction instance;
- добавить test: committed old tx не коммитит new incomplete tx с тем же старым номером;
- обновить `docs/wal.md`, `docs/recovery.md`, `docs/transactions.md`.

Критерий приёмки:

- reused-id сценарий не приводит к replay uncommitted page image;
- старые tests WAL/recovery/tx проходят.

#### `p000005` → `0.11.0.5` — safe open path v0

Тип: `quantum`.

Чеклист:

- добавить API уровня engine: `open_database_transactional` или аналог;
- новый API перед выдачей `TransactionalStore` выполняет recovery или явно проверяет clean state;
- старый `open_transactional_store` пометить как low-level/open-without-recovery в docs;
- добавить test: committed WAL-only data становится видима через safe open;
- обновить пример использования для клиента.

Критерий приёмки:

- внешний клиент может открыть базу через один безопасный API;
- unsafe/low-level open path не выглядит обычным рекомендуемым входом.

#### `p000006` → `0.11.0.6` — durability assumptions document

Тип: `quantum`.

Чеклист:

- добавить `docs/durability.md`;
- перечислить текущую последовательность commit: page images, commit marker, WAL sync, data write, data sync;
- описать, какие crash-сценарии уже покрыты тестами;
- описать, какие crash-сценарии пока являются долгом;
- добавить ссылки из `docs/architecture.md`, `docs/roadmap.md`.

Критерий приёмки:

- проект не обещает больше durability, чем реально проверяет.

### 6.2. `0.12.0.x` — measurements and diagnostics v0

#### `p000007` → `0.12.0.7` — engine metrics counters v0

Тип: `minor`.

Чеклист:

- ввести lightweight counters: page reads, page writes, wal records, wal bytes, wal syncs, data syncs;
- не ломать обычный API;
- добавить возможность получить snapshot metrics после statement;
- покрыть unit tests без привязки к точным OS timings.

Критерий приёмки:

- можно доказать, сколько WAL records и sync calls породил один `INSERT`.

#### `p000008` → `0.12.0.8` — benchmark harness v0

Тип: `quantum`.

Чеклист:

- добавить dev benchmark binary или test-harness без browser UI;
- сценарии: 1k/5k/10k inserts, select full scan, select indexed lookup;
- замерять row count, elapsed, db size, wal size, metrics snapshot;
- документировать, что это не production benchmark.

Критерий приёмки:

- можно повторить клиентский эксперимент без UI и JSON.

#### `p000009` → `0.12.0.9` — recovery benchmark smoke

Тип: `quantum`.

Чеклист:

- сценарий: создать N rows, закрыть, открыть через recovery path, проверить rows;
- замерить scanned WAL records и redone page images;
- сохранить формат отчёта в docs;
- не делать выводов про production performance.

Критерий приёмки:

- recovery cost видна в числах, а не обсуждается на глаз.

### 6.3. `0.13.0.x` — checkpoint + WAL compaction v0

#### `p000010` → `0.13.0.10` — checkpoint design and file policy

Тип: `minor`.

Чеклист:

- описать checkpoint state: что считается durable, что можно truncation;
- решить, нужен ли WAL header сейчас;
- решить, как хранить recovery start point;
- описать порядок sync;
- добавить failing tests для будущего checkpoint behavior.

Критерий приёмки:

- checkpoint не начинается с кода без протокола.

#### `p000011` → `0.13.0.11` — checkpoint API v0

Тип: `quantum`.

Чеклист:

- добавить engine-level `checkpoint()`;
- записывать checkpoint marker или state по выбранной схеме;
- убедиться, что committed dirty state уже в data file;
- не делать unsafe truncate в этом же патче, если протокол не доказан.

Критерий приёмки:

- checkpoint можно вызвать и увидеть в WAL/state;
- recovery после checkpoint даёт те же rows.

#### `p000012` → `0.13.0.12` — WAL truncate after checkpoint v0

Тип: `quantum`.

Чеклист:

- реализовать безопасное сокращение WAL после checkpoint;
- добавить Windows/Linux smoke;
- добавить test: 5000 autocommit inserts + checkpoint уменьшает WAL ratio;
- документировать, что compaction v0 не равна промышленному fuzzy checkpoint.

Критерий приёмки:

- после checkpoint WAL не остаётся в десятки раз больше DB на базовом сценарии.

#### `p000013` → `0.13.0.13` — SQL-visible CHECKPOINT v0

Тип: `quantum`.

Чеклист:

- добавить `CHECKPOINT` в SQL subset или dev command layer;
- вернуть понятный `ExecResult::StatementComplete` или diagnostics;
- добавить тест SQL path;
- обновить клиентские рекомендации.

Критерий приёмки:

- MVP-клиент может вызвать checkpoint без прямого Rust API.

### 6.4. `0.14.0.x` — explicit SQL transactions and batching v0

#### `p000014` → `0.14.0.14` — SqlSession transaction state design

Тип: `minor`.

Чеклист:

- описать состояние session: autocommit, active transaction, failed transaction;
- решить, как `SELECT` видит staged changes;
- решить поведение errors внутри transaction;
- добавить parser tests для `BEGIN`, `COMMIT`, `ROLLBACK` как failing/ignored до реализации.

Критерий приёмки:

- semantics explicit transactions не придумывается по ходу кодинга.

#### `p000015` → `0.14.0.15` — BEGIN/COMMIT/ROLLBACK v0

Тип: `quantum`.

Чеклист:

- добавить statements в parser;
- `SqlSession` хранит active `Transaction` или безопасный эквивалент;
- `INSERT` внутри active transaction не делает autocommit;
- `COMMIT` применяет staged pages;
- `ROLLBACK` отбрасывает staged changes;
- tests: rollback drops multiple inserts, commit survives reopen/recovery.

Критерий приёмки:

- 5000 inserts можно выполнить как одну transaction через SQL layer.

#### `p000016` → `0.14.0.16` — batch insert smoke and docs

Тип: `quantum`.

Чеклист:

- добавить dev script для сравнения autocommit vs single transaction;
- показать WAL records/pages/sync calls через metrics;
- обновить `docs/sql.md`, `docs/transactions.md`, `docs/roadmap.md`.

Критерий приёмки:

- преимущество batching подтверждено тестом/метриками, а не только ожиданием.

### 6.5. `0.15.0.x` — heap allocation and free-space tracking v0

#### `p000017` → `0.15.0.17` — heap insert metrics and last-page hint

Тип: `minor`.

Чеклист:

- добавить счётчик heap pages tried per insert;
- реализовать last-page hint в catalog metadata или runtime state;
- test: последовательные inserts не перечитывают все старые full pages;
- сохранить fallback на полный scan при подозрении на inconsistent hint.

Критерий приёмки:

- insert path перестаёт быть слепым линейным поиском в обычном append workload.

#### `p000018` → `0.15.0.18` — free-space map v0 research or implementation

Тип: `quantum`.

Чеклист:

- решить: нужен ли отдельный FreeMap page или достаточно catalog-level summary;
- если реализуется FreeMap, зафиксировать формат в `docs/format.md`;
- tests: delete marker/compact/free-space reuse если scope включает delete later;
- не вводить сложный allocator без тестов.

Критерий приёмки:

- есть понятный путь от last-page hint к настоящему free-space tracking.

### 6.6. `0.16.0.x` — planner transparency and index confidence v0

#### `p000019` → `0.16.0.19` — EXPLAIN v0 parser and result

Тип: `minor`.

Чеклист:

- добавить `EXPLAIN SELECT ...`;
- использовать уже существующий `ExecResult::Explain`;
- plan string v0: `SeqScan`, `IndexLookup`, table, index, predicate;
- tests parser/executor.

Критерий приёмки:

- пользователь видит, почему point lookup быстрый или медленный.

#### `p000020` → `0.16.0.20` — index plan tests and metrics

Тип: `quantum`.

Чеклист:

- добавить test, который отличает full scan от index lookup по счётчикам pages read или explicit plan;
- проверить `INT` и `TEXT` equality;
- проверить fallback при неподдержанном key type;
- обновить `docs/index.md` и `docs/sql.md`.

Критерий приёмки:

- фраза “SQL использует B+Tree index” становится проверяемой гарантией для поддержанного subset.

### 6.7. `0.17.0.x` — recovery/crash hardening v1

#### `p000021` → `0.17.0.21` — fault-injection VFS v0

Тип: `minor`.

Чеклист:

- добавить in-memory или wrapper VFS с fail points;
- fail before/after WAL append, WAL sync, data write, data sync;
- tests не должны зависеть от реального kill process.

Критерий приёмки:

- crash-like сценарии можно проверять в обычном test suite.

#### `p000022` → `0.17.0.22` — crash matrix for commit/checkpoint

Тип: `quantum`.

Чеклист:

- матрица состояний вокруг commit protocol;
- матрица состояний вокруг checkpoint/truncate;
- recovery idempotency assertions;
- документ с тем, что ещё не покрыто.

Критерий приёмки:

- durability claims проекта опираются на crash matrix, а не на общие слова.

#### `p000023` → `0.17.0.23` — pageLSN skip decision

Тип: `quantum`.

Чеклист:

- либо реализовать pageLSN redo skip;
- либо явно отложить и объяснить цену;
- tests: repeated recovery не делает лишний redo там, где skip заявлен;
- обновить `docs/recovery.md`, `docs/format.md`.

Критерий приёмки:

- recovery после большого WAL/checkpoint имеет понятную политику replay.

### 6.8. `0.18.0.x` — client-facing introspection API v0

#### `p000024` → `0.18.0.24` — system tables design

Тип: `minor`.

Чеклист:

- выбрать форму: SQL pseudo tables или Rust diagnostics API;
- минимальный набор: tables, indexes, pages, wal_stats, metrics;
- отделить dev diagnostics от future stable public API;
- добавить docs для MVP-клиента.

Критерий приёмки:

- клиенту не надо угадывать состояние ядра по файлам.

#### `p000025` → `0.18.0.25` — rdbms_wal_stats / metrics exposure v0

Тип: `quantum`.

Чеклист:

- показать WAL size, record count, approximate checkpoint state;
- показать db size/page count;
- вернуть данные через выбранный API;
- добавить tests на пустую и непустую базу.

Критерий приёмки:

- MVP-клиент может показывать file/WAL stats через engine boundary.

## 7. Что не делать раньше этого плана

До `0.13.0.x` не стоит делать:

- большой SQL parser;
- сетевой server protocol;
- ORM-like layer;
- dynamic plugin loader;
- сложный optimizer;
- публичные обещания file-format compatibility.

До `0.11.0.x` не стоит делать checkpoint/truncate. Сначала надо закрыть transaction identity и safe open path.

До `0.12.0.x` не стоит спорить о performance на глаз. Нужны метрики.

## 8. Минимальный ближайший порядок

Если нужен самый короткий порядок без всей таблицы, он такой:

```text
1. Зафиксировать TxId/recovery/open safety baseline.
2. Добавить метрики и benchmark harness.
3. Сделать checkpoint + WAL truncate.
4. Добавить SQL BEGIN/COMMIT/ROLLBACK.
5. Ускорить heap insert через hints/free-space tracking.
6. Добавить EXPLAIN и доказуемый index path.
7. Усилить crash matrix.
8. Дать клиенту introspection API.
```

## 9. Контрольные вопросы для следующих патчей

Перед каждым новым engine-патчем надо ответить:

1. Меняется ли WAL/data format?
2. Требуется ли bump выше `quantum`?
3. Есть ли recovery/reopen test?
4. Есть ли тест на rollback или crash-like scenario?
5. Метрика показывает улучшение или мы только предполагаем его?
6. Не выдаём ли учебную гарантию за промышленную?
7. Может ли MVP-клиент проверить эту фичу без доступа к приватным внутренностям?

## 10. Итог

Следующее развитие RDBMS должно идти не от “добавить ещё SQL”, а от “сделать write path объяснимым, измеримым и восстанавливаемым”.

MVP-клиент уже выполнил свою первую функцию: он вынес наружу реальные боли ядра. Самая важная из них — не UI и не скорость браузера, а политика WAL/tx/recovery.
