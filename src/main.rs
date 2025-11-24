use russh::{self, client, keys::PrivateKeyWithHashAlg, keys::load_secret_key, keys::ssh_key};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Client {}

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

    let local_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        mysql_local_port.into(),
    );

    let listener = tokio::net::TcpListener::bind(local_addr).await?;
    let (socket, o_addr) = listener.accept().await?;
    let mut stream = socket;
    let mut channel = handle
        .channel_open_direct_tcpip(
            mysql_remote_host.to_string(),
            mysql_remote_port.into(),
            o_addr.ip().to_string(),
            o_addr.port().into(),
        )
        .await?;
    
    let mut stream_closed = false;
    let mut buf = vec![0; 65536];
    loop {
        // Handle one of the possible events:
        tokio::select! {
            // There's socket input available from the client
            r = stream.read(&mut buf), if !stream_closed => {
                match r {
                    Ok(0) => {
                        stream_closed = true;
                        channel.eof().await?;
                    },
                    // Send it to the server
                    Ok(n) => channel.data(&buf[..n]).await?,
                    Err(e) => return Err(e.into()),
                };
            },
            // There's an event available on the session channel
            Some(msg) = channel.wait() => {
                match msg {
                    // Write data to the client
                    russh::ChannelMsg::Data { ref data } => {
                        stream.write_all(data).await?;
                    }
                   russh::ChannelMsg::Eof => {
                        if !stream_closed {
                            channel.eof().await?;
                        }
                        break;
                    }
                    russh::ChannelMsg::WindowAdjusted { new_size:_ }=> {
                        // Ignore this message type
                    }
                    _ => {todo!()}
                }
            },
        }
    }

    // Отключение
    match handle
        .disconnect(russh::Disconnect::ByApplication, "finished", "English")
        .await
    {
        Ok(_) => println!("✅ Successfully disconnected from host {}", ssh_host),
        Err(e) => return Err(format!("⛔ disconnection failed: {}", e).into()),
    }

    Ok(())
}
