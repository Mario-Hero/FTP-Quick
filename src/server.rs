use libunftp::ServerBuilder;
use log::{LevelFilter, error, info};
use std::path::PathBuf;
use tokio::task::JoinHandle;

pub(crate) use crate::ssh::server::{Server, ServerConfig};
use russh::keys::ssh_key::rand_core::OsRng;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::Server as _;
use std::sync::Arc;
use std::time::Duration;

// ---------- FTP 服务器 ----------

pub async fn run_ftp_server(
    username: String,
    password: String,
    port: u16,
    directory: String,
) -> JoinHandle<()> {
    use unftp_sbe_fs::Filesystem;
    let user_pass_json = format!(
        "[{{\"username\": \"{}\",\"password\": \"{}\"}}]",
        username, password
    );
    let ftp_home = PathBuf::from(directory);
    let authenticator =
        Arc::new(unftp_auth_jsonfile::JsonFileAuthenticator::from_json(user_pass_json).unwrap());
    let server = if username.len() > 0 && password.len() > 0 {
        ServerBuilder::with_authenticator(
            Box::new(move || Filesystem::new(ftp_home.clone()).unwrap()),
            authenticator,
        )
    } else {
        ServerBuilder::new(Box::new(move || Filesystem::new(ftp_home.clone()).unwrap()))
    }
    .greeting("Welcome to my FTP server")
    .passive_ports(50000..=65535)
    .build()
    .unwrap();
    tokio::spawn(async move {
        if let Err(e) = server.listen(format!("0.0.0.0:{port}")).await {
            eprintln!("ftp server error: {:?}", e);
        }
    })
}

pub async fn run_sftp_server(
    username: String,
    password: String,
    port: u16,
    directory: String,
) -> JoinHandle<()> {
    use std::path::Path;
    env_logger::builder().filter_level(LevelFilter::Info).init();
    let root_dir = Path::new(&directory);

    if !root_dir.exists() {
        error!("Root directory {:?} does not exist", directory);
        std::process::exit(1);
    }

    if !root_dir.is_dir() {
        error!("Root directory {:?} is not a directory", directory);
        std::process::exit(1);
    }

    let server_config = Arc::new(ServerConfig {
        username,
        password,
        root_dir: root_dir.to_path_buf(),
        max_read_size: 32768,
    });

    let config = russh::server::Config {
        auth_rejection_time: Duration::from_secs(3),
        auth_rejection_time_initial: Some(Duration::from_secs(0)),
        keys: vec![
            PrivateKey::random(&mut OsRng, Algorithm::Ed25519).unwrap(),
            PrivateKey::random(&mut OsRng, Algorithm::Rsa { hash: None }).unwrap(),
            //PrivateKey::random(&mut OsRng, Algorithm::Dsa).unwrap(),
        ],
        ..Default::default()
    };

    let mut server = Server {
        config: server_config,
    };

    info!("Starting SFTP server on 0.0.0.0:{}", port);
    info!(
        "Use credentials: username='{}', password='***'",
        server.config.username
    );
    tokio::spawn(async move {
        server
            .run_on_address(Arc::new(config), ("0.0.0.0", port))
            .await
            .unwrap();
    })
}

pub async fn run_tftp_server(port: u16, directory: String) -> JoinHandle<()> {
    use async_tftp::server::TftpServerBuilder;
    tokio::spawn(async move {
        let tftpd = TftpServerBuilder::with_dir_rw(directory)
            .unwrap()
            .build()
            .await
            .unwrap();
        if let Err(e) = tftpd.serve().await {
            eprintln!("tftp server error: {:?}", e);
        }
    })
}
