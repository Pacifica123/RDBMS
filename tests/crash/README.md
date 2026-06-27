# Crash tests

Этот каталог зарезервирован для будущих crash tests.

Такие тесты должны проверять не “обычный happy path”, а сбои в опасных местах:

- WAL record обрезан посередине;
- сбой до `CommitTx`;
- сбой после `CommitTx`, но до записи data file;
- ошибка `sync_data`;
- повреждённый page checksum;
- split B+Tree index page во время commit.

Правильный инструмент для этого — fault-injection VFS. Тесты не должны зависеть от случайных sleep/kill.
