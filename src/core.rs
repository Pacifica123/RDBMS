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
    pub struct RDBMS_E{
        pub message: String,
    }
}

mod sql_execute_machine {
// механизм обработки SQL-запросов
    use crate::core::core::Column;
    use crate::core::core::Database;
    use crate::core::core::RDBMS_E;

    pub fn sql_distribute(db: &mut Database, query: String){ //->Result<Database, RDBMS_E>
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

    fn get_query_type(query: String) -> QueryType {
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

    fn create_distribute(arg: String, name: String, db: Option<Database>)->Result<Database, RDBMS_E>{
        match arg {
            a if a.starts_with("DBRAM")  => Ok(create_database(name)),
            a if a.starts_with("TABLE") => {
                //провереки на существование и нахождение в БД
                match db {
                    Some(d) => {
                        let mut check: bool = false; //нахождение в БД: TODO()
                        if (check){
                            Ok(create_table(name, d))
                        }
                        else {
                            Err(RDBMS_E{message: String::from("создание таблицы вне выбранной БД")})
                        }
                        
                    },
                    None => Err(RDBMS_E{message: String::from("создание таблицы без БД") })
                }
                
            }
            _ => Err(RDBMS_E{message: String::from("некорректный аргумент запроса")}) 
        }
    }

    fn create_database(name: String)->Database {
        return Database::new();
    }

    fn create_table(name: String, mut current_db: Database)->Database{
        let mut fields: Vec<Column> = Vec::new();
        current_db.new_table(name, fields);
        return current_db;
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