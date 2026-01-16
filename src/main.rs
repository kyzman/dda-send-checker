use chrono::Local;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, poll, read};
use sqlx::{Column, Executor, Row, mysql::MySqlRow};
use std::time::Duration;
use tokio;

use crate::utils::InputData;

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
    let main_table_name = &args[0].replace("`", "");

    let _orig_date = match main_table {
        InputData::Unknown => {
            eprintln!("⛔ Unknown date!");
            std::process::exit(1)
        }
        InputData::Unsent(d) => d,
        InputData::Wegabond(d) => d,
    };

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

    let schema = std::env::var("SCHEMA")
        .map_err(|e| format!("SCHEMA not set: {}", e))?
        .replace("`", "");

    let table1 = std::env::var("TABLE1")
        .map_err(|e| format!("TABLE1 not set: {}", e))?
        .replace("`", "");
    let table2 = std::env::var("TABLE2")
        .map_err(|e| format!("TABLE2 not set: {}", e))?
        .replace("`", "");
    let req_interval: u64 = std::env::var("INTERVAL")
        .map_err(|e| format!("INTERVAL not set: {}", e))?
        .parse()
        .map_err(|e| format!("INTERVAL is not a number: {}", e))?;
    let lifetime: u64 = std::env::var("LIFETIME")
        .map_err(|e| format!("LIFETIME not set: {}", e))?
        .parse()
        .map_err(|e| format!("LIFETIME is not a number: {}", e))?;

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
        "mysql://{}:{}@localhost:{}/{}",
        mysql_user, mysql_password, mysql_local_port, mysql_db
    );

    // let pool = utils::create_pool(&database_url, lifetime).await?;
    let mut db = utils::Database::new(&database_url, lifetime).await?;

    // MySqlPoolOptions::new()
    // .idle_timeout(timeout)
    // .max_connections(3)
    // .acquire_timeout(Duration::from_secs(20))
    // .max_lifetime(Some(Duration::from_secs(lifetime * 60)))
    // .connect(&database_url)
    // .await?;

    // println!("{:?}", pool.connect_options());
    // println!("Pool is closed? {:?}", pool.is_closed());
    // println!("IDLE connections {:?}", pool.num_idle());
    // println!("Total connections {:?}", pool.size());

    let qry: &str = &format!(
        "with tenant_ids as (select tbl.TENANT_ID from
    `seeneco_dwh`.`{main_table_name}` tbl
    )
    select
    (select count(*) from tenant_ids
    ) as 'TOTAL'
    ,
    (select count(bsr.ID) from `{schema}`.`{table1}` bsr
    where
    bsr.tenant_id in (select * from tenant_ids)
    and bsr.CREATE_DATE_TIME between current_date() and DATE_ADD(CURDATE(), INTERVAL 1 DAY)
    ) as 'BSR'
    ,
    (select count(bpd.ID) from `{schema}`.`{table2}` bpd where
    bpd.tenant_id in (select * from tenant_ids)
    and bpd.payment_demand_date_time between current_date() and DATE_ADD(CURDATE(), INTERVAL 1 DAY)
    ) as 'BPD'
    "
    );

    // 6. Пример запроса к БД.
    loop {
        // tokio::time::sleep(Duration::from_secs(req_interval)).await;
        if poll(Duration::from_secs(req_interval))? {
            let event_occurs = read()?;
            match event_occurs {
                Event::Key(
                    KeyEvent {
                        code: KeyCode::Char('q') | KeyCode::Char('й'),
                        modifiers: KeyModifiers::CONTROL,
                        kind: crossterm::event::KeyEventKind::Press,
                        state: _,
                    }
                    | KeyEvent {
                        code: KeyCode::Esc,
                        modifiers: KeyModifiers::NONE,
                        kind: crossterm::event::KeyEventKind::Press,
                        state: _,
                    },
                ) => break,
                _ => (),
            }
        }

        // let res = query(qry).fetch_all(&pool).await?;
        let res: Vec<MySqlRow> = match db.execute_query(qry).await {
            Ok(res) => res,
            Err(sqlx::Error::PoolTimedOut) => {
                println!("PoolTimedOut error!");
                println!("Pool is closed? {:?}", db.pool.is_closed());
                println!("IDLE connections {:?}", db.pool.num_idle());
                println!("Total connections {:?}", db.pool.size());
                // Рабочее, но возвращается вместо Pool<MySql> структура PoolConnection<MySql>, с которой, впрочем, можно работать как с Pool,
                // однако они не равны и это создаёт проблему expected Pool<MySql>, found PoolConnection<MySql> (rust-analyzer E0308),
                // если пытаться использовать одну и ту-же переменную.
                // Возможный вариант решения, изначально после connect вызывать acquire(), которая вернёт PoolConnection
                // и далее работать с PoolConnection, а не Pool, однако это не тривиальное решение.
                if db.pool.num_idle() > 0 {
                    let mut new_pool = match db.pool.try_acquire() {
                        Some(conn) => conn,
                        None => {
                            println!("Can't acquire pool");
                            break;
                        }
                    };
                    let new_res = new_pool.fetch_all(qry).await?;
                    println!("{:?}", new_res);
                } else {
                    println!("No free connection!");
                }
                break;
            }
            Err(e) => {
                println!("Error: {:?}", e);
                break;
            }
        };

        // 7. Вывод результата.

        if res.len() > 0 {
            print!("[{}] ", Local::now().format("%Y-%m-%d %H:%M:%S"));
            // println!(
            //     "{:?}",
            //     res.get(0)
            //         .unwrap()
            //         .columns()
            //         .iter()
            //         .map(|n| n.name())
            //         .collect::<Vec<&str>>()
            // ); // вывод списка названий колонок

            for row in res {
                for col in 0..row.len() {
                    match row.try_get_unchecked::<String, usize>(col) {
                        Ok(value) => print!("{}: {}", row.column(col).name(), value),
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
