# Physical format

## 1. Назначение

Physical format описывает байты на диске. Это контракт между версиями программы, recovery и диагностикой. Его нельзя заменить сериализацией Rust-структур.

## 2. Начальный формат файла

MVP-файл должен иметь:

```text
file header page
page size
format version
database id
checksum mode
root catalog pointer
WAL/checkpoint pointer позднее
```

Пока это проектный документ, а не финальная спецификация.

## 3. Page size

Начальный размер страницы: 4096 байт.

Причина: это простой старт для учебного проекта, хорошо сочетается с обычными файловыми системами и не требует ранней оптимизации.

## 4. Page header sketch

```text
magic:        4 bytes
format_ver:   2 bytes
page_type:    2 bytes
page_id:      8 bytes
lsn:          8 bytes
checksum:     4 bytes
free_start:   2 bytes
free_end:     2 bytes
reserved:     ...
```

Для разных типов страниц payload может отличаться, но header должен оставаться проверяемым.

## 5. Slotted page sketch

Heap/table page должна хранить записи через slot directory:

```text
[page header][slot directory ->] ... free space ... [<- cell payloads]
```

Это позволяет перемещать payload внутри страницы без изменения внешнего `SlotId`.

## 6. Record identity

Логический адрес записи в heap MVP:

```text
RowId = PageId + SlotId
```

Индексы должны ссылаться на `RowId`, а не на позицию в `Vec<Row>`.

## 7. Checksum policy

MVP должен уметь:

1. вычислить checksum страницы;
2. записать страницу;
3. прочитать страницу;
4. обнаружить несовпадение checksum;
5. вернуть typed corruption error.

## 8. Версионирование

Любое изменение формата требует явного bump `format_ver` и описания миграционной политики. До стабильного формата миграции можно считать unsupported.
