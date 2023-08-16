mod database {

    pub struct Database{
        //поля хранения таблиц, индексов, и т.д.
        tables: Vec<Table>,
    }
    impl Database {
        pub fn new() -> Self {
            Database {
                //init DB
                tables: Vec::new(), 
            }
        }
        pub fn sql_execute(&mut self, query: &str) ->Result<(), String> {
            // Обработка выполнения запроса
            // Разбираем запрос, выявляем тип, параметры
            // выполняем в др. функциях соответственные операции
            // Обрабатываем ошибки и возвращаем результат
            Ok(())
        }
        
    }
    pub struct Table{
        //TODO: как представить таблицу?
    }
    impl Table {
        
    }


}
fn main() {
    println!("Тестинг СУБД!");
    //создаем БД, создаем внутри БД таблицу, выполняем запрос
}
