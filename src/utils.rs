use chrono::NaiveDate;
use russh::{
    self, client,
    keys::{PrivateKeyWithHashAlg, load_secret_key, ssh_key},
};
use sqlx::{
    Executor, MySql, Pool,
    mysql::{MySqlPoolOptions, MySqlRow},
};
use std::sync::Arc;
use std::time::Duration;

#[allow(dead_code)]
#[derive(Clone)]
pub struct Database {
    pub pool: Pool<MySql>,
}

// Example of a service using the database with automatic reconnect
#[allow(dead_code)]
pub struct DatabaseService {
    db: Database,
    db_url: String,
    lifetime: u64,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum InputData {
    Unsent(NaiveDate),
    Wegabond(NaiveDate),
    Unknown,
}

pub struct Client {}

// More SSH event handlers
// can be defined in this trait
// In this example, we're only using Channel, so these aren't needed.
impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[allow(dead_code)]
pub fn parse_date_from_arg(argument: &str) -> InputData {
    let data = argument.split("_").collect::<Vec<&str>>();
    if data.len() < 3 {
        return InputData::Unknown;
    }
    let tbl_type = data.get(5).unwrap_or(&"");
    let year = data.get(3).unwrap_or(&"0").parse().unwrap_or(0);
    let month = data.get(4).unwrap_or(&"0").parse().unwrap_or(0);
    let day = data
        .get(data.len() - 2)
        .unwrap_or(&"-1")
        .parse()
        .unwrap_or(99);
    let parsed_date = match NaiveDate::from_ymd_opt(year, month, day) {
        Some(value) => value,
        None => {
            eprintln!("Unknown date: {}-{}-{}", year, month, day);
            return InputData::Unknown;
        }
    };

    if tbl_type == &"hypothesis" {
        InputData::Unsent(parsed_date)
    } else if tbl_type == &"wegabond" {
        InputData::Wegabond(parsed_date)
    } else {
        InputData::Unknown
    }
}

#[allow(dead_code)]
impl Database {
    pub async fn new(database_url: &str, lifetime: u64) -> Result<Self, sqlx::Error> {
        let pool = Self::create_pool(database_url, lifetime).await?;
        Ok(Database { pool })
    }

    pub async fn create_pool(
        database_url: &str,
        lifetime: u64,
    ) -> Result<Pool<MySql>, sqlx::Error> {
        MySqlPoolOptions::new()
            // .max_connections(10)
            // .min_connections(2)
            .acquire_timeout(Duration::from_secs(3))
            .idle_timeout(Duration::from_secs(30))
            .max_lifetime(Duration::from_secs(lifetime * 60))
            .test_before_acquire(true)
            .connect(database_url)
            .await
    }

    pub async fn reconnect(
        &mut self,
        database_url: &str,
        lifetime: u64,
    ) -> Result<(), sqlx::Error> {
        // Create new pool
        let new_pool = Self::create_pool(database_url, lifetime).await?;

        // Replace old pool
        self.pool = new_pool;
        Ok(())
    }

    pub async fn execute_query(&self, query: &str) -> Result<Vec<MySqlRow>, sqlx::Error> {
        let result = self.pool.fetch_all(query).await?;
        Ok(result)
    }
}

#[allow(dead_code)]
impl DatabaseService {
    pub async fn new(db_url: String, lifetime: u64) -> Result<Self, sqlx::Error> {
        let db = Database::new(&db_url, lifetime).await?;
        Ok(DatabaseService {
            db,
            db_url,
            lifetime,
        })
    }

    pub async fn execute_with_retry(&self, query: &str) -> Result<Vec<MySqlRow>, sqlx::Error> {
        // First attempt
        match self.db.execute_query(query).await {
            Ok(result) => Ok(result),
            Err(e) => {
                println!("Query failed: {:?}, attempting reconnect", e);

                // Attempt reconnect
                let mut new_db = self.db.clone();
                if let Err(reconnect_err) = new_db.reconnect(&self.db_url, self.lifetime).await {
                    eprintln!("Reconnection failed: {:?}", reconnect_err);
                    return Err(e);
                }

                // Retry query with new connection
                new_db.execute_query(query).await
            }
        }
    }
}

#[allow(dead_code)]
pub async fn connect_ssh_with_key(
    ssh_host: String,
    ssh_port: u16,
    ssh_user: String,
    ssh_key_path: String,
    ssh_key_password: Option<String>,
) -> Result<russh::client::Handle<Client>, Box<dyn std::error::Error>> {
    // 3. Загрузка приватного ключа
    let key = PrivateKeyWithHashAlg::new(
        Arc::new(
            load_secret_key(ssh_key_path, ssh_key_password.as_deref())
                .map_err(|e| format!("⛔ Failed to load private key: {}", e))?,
        ),
        None,
    );

    // 4. Подключение к SSH-серверу
    let sh = Client {};
    let addr = format!("{}:{}", ssh_host, ssh_port);
    let config = russh::client::Config::default();
    let config = Arc::new(config);

    let mut handle = client::connect(config, addr, sh)
        .await
        .map_err(|e| format!("⛔ Failed to connect to SSH server: {}", e))?;

    // 5. Аутентификация по ключу
    let auth_result = handle.authenticate_publickey(ssh_user, key).await;

    match auth_result {
        Ok(_) => println!("✅ SSH authentication successful"),
        Err(e) => return Err(format!("⛔ SSH authentication failed: {}", e).into()),
    }

    Ok(handle)
}
