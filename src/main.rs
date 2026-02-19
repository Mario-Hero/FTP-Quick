#![windows_subsystem = "windows"]

mod server;
mod sftp;
mod ssh;

use slint::SharedString;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

slint::include_modules!();

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = AppWindow::new()?;
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<ServerCommand>(32);
    let current_task: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));

    // 浏览按钮回调
    let app_weak = app.as_weak();
    app.on_browse_directory(move || {
        let app_weak = app_weak.clone(); // 克隆弱引用
        tokio::spawn(async move {
            let folder = rfd::AsyncFileDialog::new().pick_folder().await;
            if let Some(path) = folder {
                let path_str = path.path().display().to_string();
                slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak.upgrade() {
                        app.set_directory(path_str.into());
                    }
                })
                .unwrap();
            }
        });
    });

    // 启动服务器回调
    let app_weak = app.as_weak();
    let task_handle = current_task.clone();
    app.on_start_server(
        move |protocol: SharedString,
              username: SharedString,
              password: SharedString,
              port_str: SharedString,
              directory: SharedString| {
            let app = app_weak.unwrap();
            let cmd_tx = cmd_tx.clone();
            let task_handle = task_handle.clone();

            // 解析端口
            let port: u16 = match port_str.parse() {
                Ok(p) => p,
                Err(_) => {
                    eprintln!("无效端口号: {}", port_str);
                    return;
                }
            };

            // 目录不能为空
            let directory = directory.trim().to_string();
            if directory.is_empty() {
                eprintln!("目录不能为空");
                return;
            }

            app.set_server_running(true);

            tokio::spawn(async move {
                // 停止当前正在运行的服务器
                let mut guard = task_handle.lock().await;
                if let Some(handle) = guard.take() {
                    handle.abort();
                    let _ = handle.await;
                }

                let cmd = ServerCommand::Start {
                    protocol: protocol.to_string(),
                    username: username.to_string(),
                    password: password.to_string(),
                    port,
                    directory,
                };
                let _ = cmd_tx.send(cmd).await;
            });
        },
    );

    // 停止服务器回调
    let app_weak = app.as_weak();
    let task_handle = current_task.clone();
    app.on_stop_server(move || {
        let app = app_weak.unwrap();
        let task_handle = task_handle.clone();
        app.set_server_running(false);

        tokio::spawn(async move {
            let mut guard = task_handle.lock().await;
            if let Some(handle) = guard.take() {
                handle.abort();
                let _ = handle.await;
            }
        });
    });

    // 后台命令处理：启动对应的服务器
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                ServerCommand::Start {
                    protocol,
                    username,
                    password,
                    port,
                    directory,
                } => {
                    println!(
                        "启动 {} 服务器 (用户: {}, 端口: {}, 目录: {})",
                        protocol, username, port, directory
                    );
                    let task = match protocol.as_str() {
                        "FTP" => server::run_ftp_server(username, password, port, directory).await,
                        "SFTP" => {
                            server::run_sftp_server(username, password, port, directory).await
                        }
                        "TFTP" => server::run_tftp_server(port, directory).await,
                        _ => continue,
                    };
                    let mut guard = current_task.lock().await;
                    *guard = Some(task);
                }
            }
        }
    });

    app.run()?;
    Ok(())
}

enum ServerCommand {
    Start {
        protocol: String,
        username: String,
        password: String,
        port: u16,
        directory: String,
    },
}
