pub mod core {
    use crate::core::sql_execute_machine;
    pub struct Database{
        //поля хранения таблиц, индексов, и т.д.
        pub tables: Vec<Table>,
    }
    impl Database {
        pub fn new() -> Self {
            Database {
                //init DB
                tables: Vec::new(), 
            }
        }
        pub fn new_table(&mut self, name:String, fields:Vec<Column>) {
            let new_table = Table::new(name, fields);
            self.tables.push(new_table);
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
        name: String,
        columns: Vec<Column>,
        rows: Vec<Row>,
    }
    impl Table {
        pub fn new(name: String, fields: Vec<Column>) ->Self {
            Table { name: (name), columns: fields, rows: Vec::new(), }
        }
    }
    pub struct Column{
        name: String,
        data_type: DataType,
    }
    pub struct Row{
        values: Vec<Value>,
    }
    pub enum DataType {
        Text,
        Integer,
        Double,
    }
    pub enum Value {
        Text(String),
        Integer(i64),
        Double(f64),
    }
}

mod sql_execute_machine {
// механизм обработки SQL-запросов
    use crate::core::core::Database;
    pub fn sql_distribute(db: &mut Database, query: &str){
        let query_type = get_query_type(query);
        match query_type {
            // TODO: осталось-то всего лишь реализовать все это, ха!
            QueryType::CREATE => {}
            QueryType::DELETE => {}
            QueryType::DROP => {}
            QueryType::INSERT => {}
            QueryType::SELECT => {}
            QueryType::SHOW => {}
            QueryType::USE => {}
            QueryType::ERROR => {/*ошибка распознования запроса*/}
        }
    }

    fn get_query_type(query: &str) -> QueryType {
        match query {
            q if q.starts_with("SELECT") => QueryType::SELECT,
            q if q.starts_with("SHOW") => QueryType::SHOW,
            q if q.starts_with("USE") => QueryType::USE,
            q if q.starts_with("DROP") => QueryType::DROP,
            q if q.starts_with("DELETE") => QueryType::DELETE,
            q if q.starts_with("INSERT") => QueryType::INSERT,
            _ => QueryType::ERROR
        }
    }

    enum QueryType {
        SELECT, 
        SHOW, 
        CREATE,
        USE,
        DROP,
        DELETE,
        INSERT,
        ERROR
    }
}