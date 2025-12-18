// use chrono::NaiveDate;
use sqlx::{Column, Executor, MySqlPool, Row, mysql::MySqlRow};
use tokio;

mod utils;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Парсинг аргумента из командной строки.
    let args: Vec<String> = std::env::args().skip(1).collect();

    let main_table = match args.get(0) {
        Some(idate) => utils::parse_date_from_arg(idate),
        None => {
            eprintln!("⛔ No arguments given!");
            std::process::exit(1)
        }
    };

    println!("{:?}", main_table);

    // 2. Загрузка переменных из .env
    dotenv::dotenv().map_err(|e| format!("Failed to load .env: {}", e))?;

    // 3. Обработка и проверка переменных для использования их в программе.
    let use_ssh = match std::env::var("USE_SSH") {
        Ok(_) => true,
        Err(_) => false,
    };
    let ssh_host = std::env::var("SSH_HOST").map_err(|e| format!("SSH_HOST not set: {}", e))?;
    let ssh_port: u16 = std::env::var("SSH_PORT")
        .map_err(|e| format!("SSH_PORT not set: {}", e))?
        .parse()
        .map_err(|e| format!("SSH_PORT is not a number: {}", e))?;
    let ssh_user = std::env::var("SSH_USER").map_err(|e| format!("SSH_USER not set: {}", e))?;
    let ssh_key_path =
        std::env::var("SSH_KEY_PATH").map_err(|e| format!("SSH_KEY_PATH not set: {}", e))?;
    let ssh_key_password = match std::env::var("SSH_KEY_PASSWORD") {
        Ok(pass) => Some(pass),
        Err(_) => None,
    }; // None, если не задан

    let mysql_local_port: u16 = std::env::var("MYSQL_LOCAL_PORT")
        .map_err(|e| format!("MYSQL_LOCAL_PORT not set: {}", e))?
        .parse()
        .map_err(|e| format!("MYSQL_LOCAL_PORT is not a number: {}", e))?;
    let mysql_remote_host = std::env::var("MYSQL_REMOTE_HOST")
        .map_err(|e| format!("MYSQL_REMOTE_HOST not set: {}", e))?;
    let mysql_remote_port: u16 = std::env::var("MYSQL_REMOTE_PORT")
        .map_err(|e| format!("MYSQL_REMOTE_PORT not set: {}", e))?
        .parse()
        .map_err(|e| format!("MYSQL_REMOTE_PORT is not a number: {}", e))?;
    let mysql_user =
        std::env::var("MYSQL_USER").map_err(|e| format!("MYSQL_USER not set: {}", e))?;
    let mysql_password =
        std::env::var("MYSQL_PASSWORD").map_err(|e| format!("MYSQL_PASSWORD not set: {}", e))?;
    let mysql_db =
        std::env::var("MYSQL_DATABASE").map_err(|e| format!("MYSQL_DATABASE not set: {}", e))?;

    let table1 = std::env::var("TABLE1").map_err(|e| format!("TABLE1 not set: {}", e))?;

    // 4. Если используем SSH, то подключаемся и создаём туннель.
    if use_ssh {
        // 1. Подключаемся по SSH
        let handle = utils::connect_ssh_with_key(
            ssh_host,
            ssh_port,
            ssh_user,
            ssh_key_path,
            ssh_key_password,
        )
        .await?;

        // 2. Запуск TCP-слушателя на локальном порту
        let listener =
            tokio::net::TcpListener::bind(format!("127.0.0.1:{}", mysql_local_port)).await?;
        println!("✅ Listening local port {}", mysql_local_port);

        // 3. Открываем канал от локальной сессии до сервиса.
        tokio::spawn(async move {
            let (mut local_socket, _) = listener
                .accept()
                .await
                .expect("⛔ Cannot process local client");

            let ssh_channel = handle
                .channel_open_direct_tcpip(
                    &mysql_remote_host.clone(),
                    mysql_remote_port as u32,
                    "127.0.0.1",
                    mysql_local_port as u32,
                )
                .await
                .expect("⛔ Cannot open SSH forwarding channel");

            let mut ssh_stream = ssh_channel.into_stream();

            // Копирование в обе стороны данных.
            tokio::io::copy_bidirectional(&mut local_socket, &mut ssh_stream)
                .await
                .expect("⛔ Copy error between local socket and SSH stream");
        });
    }

    // 5. Подключаемся к Базе данных.
    let database_url = format!(
        "postgres://{}:{}@localhost:{}/{}",
        mysql_user, mysql_password, mysql_local_port, mysql_db
    );

    let pool = MySqlPool::connect(&database_url).await?;

    // 6. Пример запроса к БД.
    let qry: &str = &format!(
        "select * from {} tbl where tbl.OPERATION_ID = 13007 and tbl.START_DATE_TIME >= '2025-11-26' order by tbl.START_DATE_TIME desc limit 10",
        table1
    ); // current_date()
    // let qry: &str = &format!("select version()");

    // let mut rows = query(qry).fetch(&pool);
    let res: Vec<MySqlRow> = pool.fetch_all(qry).await.unwrap();

    // 7. Тестовый вывод результата.

    if res.len() > 0 {
        println!(
            "{:?}",
            res.get(0)
                .unwrap()
                .columns()
                .iter()
                .map(|n| n.name())
                .collect::<Vec<&str>>()
        ); // вывод списка названий колонок

        for row in res {
            for col in 0..row.len() {
                match row.try_get_unchecked::<String, usize>(col) {
                    Ok(value) => print!("{}", value),
                    Err(sqlx::Error::ColumnDecode { index: _, source }) => {
                        if source.is::<sqlx::error::UnexpectedNullError>() {
                            print!("<null>")
                        } else {
                            print!("{:?}", source)
                        }
                    }
                    Err(err) => print!("{:?}", err),
                    //     Error::ColumnDecode { index, source }
                    // }
                }
                if col < row.len() - 1 {
                    print!(" | ")
                } else {
                    println!("")
                }
            }
        }
    } else {
        println!("Empty result")
    }
    // Отключение канала.
    // handle
    //     .cancel_tcpip_forward(&mysql_remote_host, mysql_remote_port.into())
    //     .await?;

    // Отключение SSH
    // match handle
    //     .disconnect(russh::Disconnect::ByApplication, "finished", "English")
    //     .await
    // {
    //     Ok(_) => println!("✅ Successfully disconnected from host {}", ssh_host),
    //     Err(e) => return Err(format!("⛔ disconnection failed: {}", e).into()),
    // }

    Ok(())
}
