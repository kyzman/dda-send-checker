#[path = "../utils.rs"]
mod utils;
use arboard::Clipboard;
use chrono::Local;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, poll, read};
use sqlx::{
    Column, Executor, Row,
    mysql::{MySqlPoolOptions, MySqlRow},
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let main_table_name = &args[0].replace("`", "");

    dotenv::dotenv().map_err(|e| format!("Failed to load .env: {}", e))?;

    let mysql_local_port: u16 = std::env::var("MYSQL_LOCAL_PORT")
        .map_err(|e| format!("MYSQL_LOCAL_PORT not set: {}", e))?
        .parse()
        .map_err(|e| format!("MYSQL_LOCAL_PORT is not a number: {}", e))?;
    let mysql_user =
        std::env::var("MYSQL_USER").map_err(|e| format!("MYSQL_USER not set: {}", e))?;
    let mysql_password =
        std::env::var("MYSQL_PASSWORD").map_err(|e| format!("MYSQL_PASSWORD not set: {}", e))?;
    let mysql_db =
        std::env::var("MYSQL_DATABASE").map_err(|e| format!("MYSQL_DATABASE not set: {}", e))?;
    let req_interval: u64 = std::env::var("INTERVAL")
        .map_err(|e| format!("INTERVAL not set: {}", e))?
        .parse()
        .map_err(|e| format!("INTERVAL is not a number: {}", e))?;
    let lifetime: u64 = std::env::var("LIFETIME")
        .map_err(|e| format!("LIFETIME not set: {}", e))?
        .parse()
        .map_err(|e| format!("LIFETIME is not a number: {}", e))?;
    let clip_buffer_size: usize = std::env::var("HISTORY_SIZE")
        .unwrap_or("7".to_string())
        .parse()
        .map_err(|e| format!("HISTORY_SIZE is not a number: {}", e))?;

    let schema = std::env::var("SCHEMA")
        .map_err(|e| format!("SCHEMA not set: {}", e))?
        .replace("`", "");

    let table1 = std::env::var("TABLE1")
        .map_err(|e| format!("TABLE1 not set: {}", e))?
        .replace("`", "");
    let table2 = std::env::var("TABLE2")
        .map_err(|e| format!("TABLE2 not set: {}", e))?
        .replace("`", "");
    let table3 = std::env::var("TABLE3")
        .map_err(|e| format!("TABLE3 not set: {}", e))?
        .replace("`", "");

    // 1. Создаём экземпляр Clipboard
    let mut clipboard = Clipboard::new()
        .map_err(|e| format!("Не удалось получить доступ к буферу обмена: {}", e))?;

    let database_url = format!(
        "mysql://{}:{}@localhost:{}/{}",
        mysql_user, mysql_password, mysql_local_port, mysql_db
    );
    let pool = MySqlPoolOptions::new()
        // .idle_timeout(timeout)
        .max_connections(3)
        .acquire_timeout(Duration::from_secs(3))
        .idle_timeout(Duration::from_secs(30))
        .max_lifetime(Duration::from_secs(lifetime * 60))
        .connect(&database_url)
        .await?;

    // 5. Формирование запроса;
    // let qry: &str = &format!("select version();");

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
    ,
    (select count(*) from `{schema}`.`{table3}` bp where bp.tenant_id in (select * from tenant_ids) and date(bp.PAYMENT_DATE_TIME) = CURDATE()
    ) as 'SUCCESS'
    ,
    (select round(sum(bp.AMOUNT), 2) from `{schema}`.`{table3}` bp where bp.tenant_id in (select * from tenant_ids) and date(bp.PAYMENT_DATE_TIME) = CURDATE()
    ) as 'INCOME'
    "
    );

    let res: Vec<MySqlRow> = pool.fetch_all(qry).await?;
    let columns = res.get(0).unwrap().columns();

    // Формируем для буфера обмена заголовок таблицы
    let mut history: Vec<String> = Vec::new();
    history.push(format!(
        "| TIME |{}|",
        columns
            .iter()
            .map(|n| format!(" {} ", n.name()))
            .collect::<Vec<String>>()
            .join("|")
    )); // вывод списка названий колонок);
    history.push(format!("|{}", " --- |".repeat(columns.len() + 1)));

    println!("ESC - прервать, Enter - скопировать последние несколько строк в буфер обмена\n");

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
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                    kind: crossterm::event::KeyEventKind::Press,
                    state: _,
                }) => match clipboard.set_text(history.join("\n")) {
                    Ok(_) => println!(
                        "{} строк{} успешно скопировано в буфер обмена!",
                        history.len(),
                        utils::ending(history.len())
                    ),
                    Err(e) => eprintln!("Ошибка при копировании: {}", e),
                },
                Event::Key(KeyEvent {
                    code: _,
                    modifiers: _,
                    kind: _,
                    state: _,
                }) => continue,
                _ => (),
            }
        };

        // 6. Запрос в базу.
        let res: Vec<MySqlRow> = match pool.fetch_all(qry).await {
            Ok(res) => res,
            Err(sqlx::Error::PoolTimedOut) => {
                println!("PoolTimedOut error!");
                println!("Pool is closed? {:?}", pool.is_closed());
                println!("IDLE connections {:?}", pool.num_idle());
                println!("Total connections {:?}", pool.size());
                println!("Config {:?}", pool.options());
                break;
            }
            Err(e) => {
                println!("Error: {:?}", e);
                break;
            }
        };

        // 7. Вывод результата.

        if res.len() > 0 {
            let timestump = Local::now().format("%Y-%m-%d %H:%M:%S");
            let mut out_res: String = String::from(&format!("[{}] ", timestump));
            let mut data_res: Vec<String> = Vec::new();

            for row in res {
                for col in 0..row.len() {
                    match row.try_get_unchecked::<String, usize>(col) {
                        Ok(value) => {
                            data_res.push(format!(
                                "{}",
                                // row.column(col).name(),
                                value
                            ));
                            out_res.push_str(&format!("{}: {}", row.column(col).name(), value));
                        }
                        Err(sqlx::Error::ColumnDecode { index: _, source }) => {
                            if source.is::<sqlx::error::UnexpectedNullError>() {
                                data_res.push("<null>".to_string());
                                out_res.push_str("<null>");
                            } else {
                                data_res.push(format!("{:?}", source))
                            }
                        }
                        Err(err) => eprint!("{:?}", err),
                        //     Error::ColumnDecode { index, source }
                        // }
                    }
                    if col < row.len() - 1 {
                        out_res.push_str(" | ")
                    } else {
                        out_res.push_str(" |")
                    }
                }
            }
            println!("{}", out_res);
            history.push(format!("| {} | {} |", timestump, data_res.join(" | ")));
            if history.len() > clip_buffer_size + 2 {
                history.remove(2);
            }
        } else {
            println!("Empty result")
        }
    }

    println!("finish!");
    pool.close().await;
    Ok(())
}
