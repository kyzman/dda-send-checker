fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Загрузка переменных из .env
    dotenv::dotenv().map_err(|e| format!("Failed to load .env: {}", e))?;

    Ok(())
}
