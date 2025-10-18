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

fn parse_dsn(dsn: &str) -> Result<(String, String), String> {
    let mut parts = dsn.splitn(2, '?');
    let base = parts
        .next()
        .filter(|part| !part.is_empty())
        .ok_or_else(|| "TRSO_DSN must include the database URL before '?'".to_string())?
        .to_string();

    let query = parts
        .next()
        .ok_or_else(|| "TRSO_DSN must include query parameters".to_string())?;

    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next().unwrap_or("");
        let value = kv.next().unwrap_or("");

        if key == "authToken" {
            if value.is_empty() {
                return Err("authToken in TRSO_DSN cannot be empty".to_string());
            }

            return Ok((base, value.to_string()));
        }
    }

    Err("TRSO_DSN must include authToken query parameter".to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_dsn;

    #[test]
    fn parse_dsn_extracts_url_and_token() {
        let dsn = "libsql://example.turso.io?authToken=abc123&project=myproj";
        let (url, token) = parse_dsn(dsn).unwrap();

        assert_eq!(url, "libsql://example.turso.io");
        assert_eq!(token, "abc123");
    }

    #[test]
    fn parse_dsn_errors_when_missing_token() {
        let err = parse_dsn("libsql://example.turso.io").unwrap_err();
        assert!(err.contains("query"));

        let err = parse_dsn("libsql://example.turso.io?project=myproj").unwrap_err();
        assert!(err.contains("authToken"));
    }
}

fn get_configs() -> Config {
    // Get current directory in case the path is not set
    let cwd = env::current_dir()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap();
    let cwd = format!("{}/migrations", cwd);
    let migrations_path = env::var("TRSO_MIGRATIONS_PATH").unwrap_or(cwd);

    if let Ok(dsn) = env::var("TRSO_DSN") {
        let (url_or_path, token) = parse_dsn(&dsn)
            .expect("TRSO_DSN should follow 'libsql://<path>?authToken=<token>' format");

        return Config {
            local: false,
            url_or_path,
            token,
            migrations_path,
        };
    }

    let is_local = match env::var("TRSO_LOCAL") {
        Ok(val) => val
            .parse::<bool>()
            .expect("TRSO_LOCAL should be either true or false"),
        Err(_) => false,
    };

    let url_or_path = env::var("TRSO_PATH_URL").expect("TRSO_PATH_URL has to be set");
    let token = if is_local {
        String::new()
    } else {
        env::var("TRSO_TOKEN").expect("if not TRSO_LOCAL=true, the TRSO_TOKEN must be set")
    };

    Config {
        local: is_local,
        url_or_path,
        token,
        migrations_path,
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

    let result = conn
        .execute(
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
        Err(e) => return Err(AppError::DatabaseError(e.to_string())),
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
                let _ = transaction
                    .execute(
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
        Ok(_) => {}
        Err(e) => println!("Error occured during the migration {:?}", e),
    }

    println!("Migration finished.");
}
