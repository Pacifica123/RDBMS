pub mod core {
    use crate::core::sql_execute_machine::sql_distribute;
    //use serde::Serialize;
    // use serde_json::json;
    use serde_derive::Serialize;
    use serde_derive::Deserialize;
    // use serde::{Serialize, Deserialize};

    #[derive(Serialize, Deserialize)]
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
            sql_distribute(self, String::from(query));

            Ok(())
        }
        
    }
    #[derive(Serialize, Deserialize)]
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
    #[derive(Serialize, Deserialize)]
    pub struct Column{
        name: String,
        data_type: DataType,
    }
    #[derive(Serialize, Deserialize)]
    pub struct Row{
        values: Vec<Value>,
    }
    #[derive(Serialize, Deserialize)]
    pub enum DataType {
        Text,
        Integer,
        Double,
    }
    #[derive(Serialize, Deserialize)]
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
    use crate::core::core::{Database, Column, Row, Table};
    use crate::core::core::RDBMS_E;
    use serde_derive::Serialize;
    use serde_derive::Deserialize;
    use std::fs::{File, OpenOptions};
    use std::fs::write;
    use std::io::Write;
    use serde_json::json;
    use serde::Serialize;

    pub enum SqlResult{
        DB(Database),
        Rows(Vec<Row>),
        Columns(Vec<Column>),
        Table(Table),
        Error(RDBMS_E)
    }

    pub fn sql_distribute(db: &mut Database, query: String) -> SqlResult{ 
        let query_type = get_query_type(query.clone());
        match query_type {
            // TODO: осталось-то всего лишь реализовать все это, ха!
            QueryType::CREATE => {
                let parts: Vec<&str> = query.split_terminator(" ") .collect::<Vec<&str>>();
                if parts.len() > 3 || parts.len() < 2 {
                    return SqlResult::Error(RDBMS_E { message: String::from("Некорректный CREATE запрос") })
                }
                if parts.len() == 2 {
                    let res_default = create_distribute(String::from(parts[1]), String::from("defDBname"), Option::None);
                    return res_default;
                };
                let res = create_distribute(String::from(parts[1]), String::from(parts[2]), Option::None); 
                return res;
            }
            QueryType::DELETE => {
                let parts: Vec<&str> = query.split_terminator(" ").collect::<Vec<&str>>();
                if parts.len() != 2 { return SqlResult::Error(RDBMS_E { message: String::from("Некорректный DELETE запрос") });}

                todo!()
            }
            QueryType::DROP => {
                todo!()
            }
            QueryType::INSERT => {
                todo!()
            }
            QueryType::SELECT => {
                todo!()
            }
            QueryType::SHOW => {
                todo!()
            }
            QueryType::USE => {
                todo!()
            }
            QueryType::ERROR => { SqlResult::Error(RDBMS_E { message: String::from("Ошибка распознования запроса") })}
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

    fn create_distribute(arg: String, name: String, db: Option<Database>)->SqlResult{
        match arg {
            a if a == "DBRAM"  => create_database(name), // временная БД хранящаяся в ОЗУ которая сотрется с закрытием программы.
            a if a == "TABLE" => {
                //провереки на существование и нахождение в БД
                match db {
                    Some(d) => {
                        let mut check: bool = false; //нахождение в БД: TODO()
                        if (check){
                            create_table(name, d)
                        }
                        else {
                            SqlResult::Error(RDBMS_E{message: String::from("создание таблицы вне выбранной БД")})
                        }
                        
                    },
                    None => SqlResult::Error(RDBMS_E{message: String::from("создание таблицы без БД") })
                }
                
            }
            a if a == "DB" => {
                // создание полноценного файла БД
                if (true) {
                    // если создание файла прошло успешно
                    create_database(name)
                }
                else {
                    SqlResult::Error(RDBMS_E { message: String::from("Не удалось создать БД") })
                }
            }
            _ => SqlResult::Error(RDBMS_E{message: String::from("некорректный аргумент запроса")}) 
        }
    }

    fn create_database(name: String)->SqlResult {
        return SqlResult::DB(Database::new());
    }

    fn create_table(name: String, mut current_db: Database)->SqlResult{
        //TODO: не хватает обработки ошибок
        let mut fields: Vec<Column> = Vec::new();
        current_db.new_table(name, fields);
        return SqlResult::DB(current_db);
    }

    fn create_db_file(db: &Database, name: String)->bool{
        let file_path = format!("{}.dbonrs", name);
        // создание файла
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&file_path) {
            Ok(file) => file,
            Err(_) => {
                eprintln!("Файл {} уже существует", file_path);
                return false;
            }
        };
        // запись БД в файл (пока в формате json)
        
        let ser_db = serde_json::to_string_pretty(db).unwrap();
        if let Err(err) = file.write_all(ser_db.as_bytes()){
            eprintln!("Не удалось записать полностью файл");
            return false;
        }

        true
    }

    fn delete_execute(){
        todo!()
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