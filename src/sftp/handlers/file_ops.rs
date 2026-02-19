use log::{error, info, warn};
use russh_sftp::protocol::{Data, File, Name, Status, StatusCode, Handle, OpenFlags, FileAttributes};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::fs;

use crate::sftp::SftpSession;
use crate::sftp::utils::file_info::FileInfo;

pub async fn hanlde_close(
    session: &mut SftpSession,
    id: u32,
    handle: String,
) -> Result<Status, StatusCode> {
    info!("close handle: {}", handle);

    // Rimuovi il file o directory dal tracking
    if let Some(open_file) = session.state.open_files.remove(&handle) {
        info!(
            "Closed file handle: {} (path: {:?}, binary: {})",
            handle, open_file.path, open_file.is_binary
        );
    } else if session.state.open_dirs.remove(&handle).is_some() {
        info!("Closed directory handle: {}", handle);
    }

    Ok(Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_string(),
        language_tag: "en-US".to_string(),
    })
}

pub async fn handle_read(
    session: &mut SftpSession,
    id: u32,
    handle: String,
    offset: u64,
    len: u32,
) -> Result<Data, StatusCode> {
    info!(
        "read handle: {}, offset: {}, requested len: {}",
        handle, offset, len
    );

    if let Some(open_file) = session.state.open_files.get_mut(&handle) {
        // Limita la dimensione della lettura al massimo configurato
        let actual_len = std::cmp::min(len, session.state.max_read_size);

        match open_file.file.seek(std::io::SeekFrom::Start(offset)).await {
            Ok(actual_offset) => {
                if actual_offset != offset {
                    warn!("Seek to {} resulted in position {}", offset, actual_offset);
                }

                let mut buffer = vec![0u8; actual_len as usize];
                match open_file.file.read(&mut buffer).await {
                    Ok(bytes_read) => {
                        if bytes_read == 0 {
                            info!("End of file reached for handle: {}", handle);
                            Err(StatusCode::Eof)
                        } else {
                            buffer.truncate(bytes_read);

                            info!(
                                "Successfully read {} bytes from handle: {} (binary: {}, offset: {})",
                                bytes_read, handle, open_file.is_binary, offset
                            );

                            // Log aggiuntivo per file di testo (primi 100 caratteri se non binario)
                            if !open_file.is_binary && bytes_read > 0 {
                                let preview = String::from_utf8_lossy(
                                    &buffer[..std::cmp::min(100, bytes_read)],
                                );
                                info!(
                                    "Text file preview: {}",
                                    preview.chars().take(50).collect::<String>()
                                );
                            }

                            Ok(Data { id, data: buffer })
                        }
                    }
                    Err(e) => {
                        error!("Failed to read from file handle {}: {}", handle, e);
                        Err(StatusCode::Failure)
                    }
                }
            }
            Err(e) => {
                error!("Failed to seek in file handle {}: {}", handle, e);
                Err(StatusCode::Failure)
            }
        }
    } else {
        warn!("Invalid file handle: {}", handle);
        Err(StatusCode::BadMessage)
    }
}

pub async fn handle_realpath(session: &mut SftpSession, id: u32, path: String) -> Result<Name, StatusCode> {
    info!("realpath: {}", path);

        let resolved_path = session.path_resolver.resolve_path(&path)?;

        match resolved_path.canonicalize() {
            Ok(canonical) => {
                // Converti il path canonico in un path relativo alla root
                let relative_path =
                    if let Ok(rel) = canonical.strip_prefix(&session.path_resolver.get_root_dir()) {
                        format!("/{}", rel.to_string_lossy())
                    } else {
                        "/".to_string()
                    };

                Ok(Name {
                    id,
                    files: vec![File::dummy(&relative_path)],
                })
            }
            Err(_) => Ok(Name {
                id,
                files: vec![File::dummy("/")],
            }),
        }
}

pub async fn handle_open(
    session: &mut SftpSession,
    id: u32,
    path: String,
    pflags: OpenFlags,
    _attrs: FileAttributes,
) -> Result<Handle, StatusCode> {
    info!("open file: {} with flags: {:?}", path, pflags);

    let resolved_path = session.path_resolver.resolve_path(&path)?;

    // Determina se è un'operazione di scrittura
    let is_write = pflags.intersects(
        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::APPEND,
    );

    if is_write {
        // Per operazioni di scrittura, assicurati che la directory parent esista
        if let Some(parent) = resolved_path.parent() {
            if !parent.exists() {
                warn!("Parent directory does not exist: {:?}", parent);
                return Err(StatusCode::NoSuchFile);
            }
        }

        // Crea o apri il file per scrittura
        let open_options = {
            let mut opts = fs::OpenOptions::new();

            if pflags.contains(OpenFlags::READ) {
                opts.read(true);
            }

            if pflags.contains(OpenFlags::WRITE) {
                opts.write(true);
            }

            if pflags.contains(OpenFlags::CREATE) {
                opts.create(true);
            }

            if pflags.contains(OpenFlags::TRUNCATE) {
                opts.truncate(true);
            }

            if pflags.contains(OpenFlags::APPEND) {
                opts.append(true);
            }

            opts
        };

        match open_options.open(&resolved_path).await {
            Ok(file) => match FileInfo::from_file(file, resolved_path).await {
                Ok(open_file) => {
                    let handle = session.next_handle();
                    info!(
                        "Successfully opened file for write with handle: {} (binary: {})",
                        handle, open_file.is_binary
                    );
                    session.state.open_files.insert(handle.clone(), open_file);
                    Ok(Handle { id, handle })
                }
                Err(e) => {
                    warn!("Failed to create FileInfo: {}", e);
                    Err(StatusCode::Failure)
                }
            },
            Err(e) => {
                warn!("Failed to open/create file {:?}: {}", resolved_path, e);
                match e.kind() {
                    std::io::ErrorKind::NotFound => Err(StatusCode::NoSuchFile),
                    std::io::ErrorKind::PermissionDenied => Err(StatusCode::PermissionDenied),
                    std::io::ErrorKind::AlreadyExists => Err(StatusCode::Failure),
                    _ => Err(StatusCode::Failure),
                }
            }
        }
    } else {
        // Per operazioni di lettura, il file deve esistere
        if !resolved_path.exists() {
            warn!("File does not exist: {:?}", resolved_path);
            return Err(StatusCode::NoSuchFile);
        }

        // Controlla se è un file regolare
        if !resolved_path.is_file() {
            warn!("Path is not a regular file: {:?}", resolved_path);
            return Err(StatusCode::Failure);
        }

        match FileInfo::new(resolved_path).await {
            Ok(open_file) => {
                let handle = session.next_handle();
                info!(
                    "Successfully opened file for read with handle: {} (binary: {})",
                    handle, open_file.is_binary
                );
                session.state.open_files.insert(handle.clone(), open_file);
                Ok(Handle { id, handle })
            }
            Err(e) => {
                warn!("Failed to open file: {}", e);
                match e.kind() {
                    std::io::ErrorKind::NotFound => Err(StatusCode::NoSuchFile),
                    std::io::ErrorKind::PermissionDenied => Err(StatusCode::PermissionDenied),
                    _ => Err(StatusCode::Failure),
                }
            }
        }
    }
}

pub async fn handle_write(
    session: &mut SftpSession,
    id: u32,
    handle: String,
    offset: u64,
    data: Vec<u8>,
) -> Result<Status, StatusCode> {
    info!(
        "write handle: {}, offset: {}, data len: {}",
        handle,
        offset,
        data.len()
    );

    if let Some(open_file) = session.state.open_files.get_mut(&handle) {
        match open_file.file.seek(std::io::SeekFrom::Start(offset)).await {
            Ok(actual_offset) => {
                if actual_offset != offset {
                    warn!("Seek to {} resulted in position {}", offset, actual_offset);
                }

                match open_file.file.write_all(&data).await {
                    Ok(_) => {
                        // Assicurati che i dati siano scritti su disco
                        if let Err(e) = open_file.file.flush().await {
                            warn!("Failed to flush file handle {}: {}", handle, e);
                        }

                        info!(
                            "Successfully wrote {} bytes to handle: {} at offset: {}",
                            data.len(),
                            handle,
                            offset
                        );

                        Ok(Status {
                            id,
                            status_code: StatusCode::Ok,
                            error_message: "Ok".to_string(),
                            language_tag: "en-US".to_string(),
                        })
                    }
                    Err(e) => {
                        error!("Failed to write to file handle {}: {}", handle, e);
                        Err(StatusCode::Failure)
                    }
                }
            }
            Err(e) => {
                error!("Failed to seek in file handle {}: {}", handle, e);
                Err(StatusCode::Failure)
            }
        }
    } else {
        warn!("Invalid file handle for write: {}", handle);
        Err(StatusCode::BadMessage)
    }
}