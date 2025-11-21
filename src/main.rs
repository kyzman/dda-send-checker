use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Загрузка переменных из .env
    dotenv::dotenv().map_err(|e| format!("Failed to load .env: {}", e))?;

    // 2. Обработка и проверка переменных для использования их в программе.
    let ssh_host = std::env::var("SSH_HOST").map_err(|e| format!("SSH_HOST not set: {}", e))?;
    let ssh_port: u16 = std::env::var("SSH_PORT")
        .map_err(|e| format!("SSH_PORT not set: {}", e))?
        .parse()
        .map_err(|e| format!("SSH_PORT is not a number: {}", e))?;
    let ssh_user = std::env::var("SSH_USER").map_err(|e| format!("SSH_USER not set: {}", e))?;
    let ssh_key_path =
        std::env::var("SSH_KEY_PATH").map_err(|e| format!("SSH_KEY_PATH not set: {}", e))?;
    let ssh_key_password = std::env::var("SSH_KEY_PASSWORD").ok(); // None, если не задан

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

    println!("Finished successfully.");
    Ok(())
}
