use std::{collections::HashMap, env, io};

use libsql::{Builder, Connection};

#[derive(Debug)]
struct Config {
    url_or_path: String,

    local: bool,

    token: String,

    migrations_path: String,
}

#[derive(Debug)]
enum AppError {
    DatabaseError(String),
    IOError(String),
}

fn get_configs() -> Config {
    let is_local = match env::var("TRSO_LOCAL") {
        Ok(val) => val
            .parse::<bool>()
            .expect("TRSO_LOCAL should be either true or false"),
        Err(_) => false,
    };

    let url_or_path = env::var("TRSO_PATH_URL").expect("TRSO_PATH_URL has to be set");
    let mut token = String::from("");
    if !is_local {
        token = env::var("TRSO_TOKEN").expect("if not TRSO_LOCAL=true, the TRSO_TOKEN must be set");
    }

    // Get current directory in case the path is not set
    let cwd = env::current_dir().unwrap().into_os_string().into_string().unwrap();
    let cwd = format!("{}/migrations", cwd);
    let migrations_path = env::var("TRSO_MIGRATIONS_PATH").unwrap_or(cwd);

    Config {
        local: is_local,
        url_or_path,
        token,
        migrations_path
    }
}

async fn get_connection(config: &Config) -> Result<libsql::Connection, libsql::Error> {
    let db = if config.local {
        Builder::new_local(&config.url_or_path).build().await?
    } else {
        Builder::new_remote(config.url_or_path.clone(), config.token.clone())
            .build()
            .await?
    };

    let conn = db.connect()?;

    return Ok(conn);
}

async fn migrate_database(conn: &Connection, path: String) -> Result<(), AppError> {
    let dir = match std::fs::read_dir(path.as_str()) {
        Ok(dir) => dir,
        Err(err) => return Err(AppError::IOError(err.to_string())),
    };

    let mut list_files = match dir
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()
    {
        Ok(list) => list,
        Err(e) => return Err(AppError::IOError(e.to_string())),
    };

    list_files.sort();

    let result = conn.execute(
        r#"
            CREATE TABLE IF NOT EXISTS migrations 
            (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_name TEXT);
        "#,
        (),
    )
    .await;

    match result {
        Ok(_) => (),
        Err(e) => return Err(AppError::DatabaseError(e.to_string()))
    }

    let mut in_database: HashMap<String, bool> = HashMap::new();
    let mut rows = match conn.query("SELECT * FROM migrations", ()).await {
        Ok(rows) => rows,
        Err(e) => return Err(AppError::DatabaseError(e.to_string())),
    };

    let mut name: String;
    while let Some(row) = rows.next().await.unwrap() {
        name = row.get_value(1).unwrap().as_text().unwrap().to_string();
        in_database.insert(name, true);
    }

    let mut name: String;
    let mut migration_content: String;
    for file in list_files {
        name = match file.file_name() {
            Some(n) => n.to_str().unwrap().to_string(),
            None => {
                println!("cannot find name in path");
                continue;
            }
        };

        if let Some(_) = in_database.get(&name) {
            println!("skipping file {}, it is already applied", name);
            continue;
        }

        migration_content = match std::fs::read_to_string(file) {
            Ok(content) => content,
            Err(e) => return Err(AppError::IOError(e.to_string())),
        };

        let transaction = conn.transaction().await.unwrap();
        match transaction.execute_batch(&migration_content).await {
            Ok(_) => {
                let _ = transaction.execute(
                    "INSERT INTO migrations (file_name) VALUES (?1)",
                    [name.as_str()],
                )
                .await;
                let _ = transaction.commit().await;
                println!("Migration applied for file {}", name);
            }
            Err(e) => {
                let _ = transaction.rollback().await;
                println!("Error while executing migration {}", name);
                return Err(AppError::DatabaseError(e.to_string()));
            }
        };
    }

    return Ok(());
}


#[tokio::main]
async fn main() {
    let configs = get_configs();
    let conn = get_connection(&configs).await.unwrap();
    
    println!("Migration is starting ...");
    let result = migrate_database(&conn, configs.migrations_path).await;

    match result {
        Ok(_) => {},
        Err(e) => println!("Error occured during the migration {:?}", e)
    }

    println!("Migration finished.");
}
