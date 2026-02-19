use log::{error, info, warn};
use russh_sftp::protocol::{Attrs, StatusCode};
use tokio::fs;

use crate::sftp::utils::metadata::MetadataConverter;
use crate::sftp::SessionState;
use crate::sftp::utils::path_resolver::PathResolver;

pub async fn handle_stat(
    _state: &SessionState,
    path_resolver: &PathResolver,
    id: u32,
    path: String,
) -> Result<Attrs, StatusCode> {
    info!("stat: {}", path);
    let resolved_path = path_resolver.resolve_path(&path)?;
    match fs::metadata(&resolved_path).await {
        Ok(metadata) => {
            let attrs = MetadataConverter::to_file_attributes(&metadata).await;
            info!(
                "stat result for {:?}: size={:?}, is_file={}",
                resolved_path,
                attrs.size,
                metadata.is_file()
            );
            Ok(Attrs { id, attrs })
        }
        Err(e) => {
            warn!("Failed to stat {:?}: {}", resolved_path, e);
            match e.kind() {
                std::io::ErrorKind::NotFound => Err(StatusCode::NoSuchFile),
                std::io::ErrorKind::PermissionDenied => Err(StatusCode::PermissionDenied),
                _ => Err(StatusCode::Failure),
            }
        }
    }
}

pub async fn handle_lstat(
    _state: &SessionState,
    path_resolver: &PathResolver,
    id: u32,
    path: String,
) -> Result<Attrs, StatusCode> {
    info!("lstat: {}", path);
    let resolved_path = path_resolver.resolve_path(&path)?;
    match fs::symlink_metadata(&resolved_path).await {
        Ok(metadata) => {
            let attrs = MetadataConverter::to_file_attributes(&metadata).await;
            Ok(Attrs { id, attrs })
        }
        Err(e) => {
            warn!("Failed to lstat {:?}: {}", resolved_path, e);
            match e.kind() {
                std::io::ErrorKind::NotFound => Err(StatusCode::NoSuchFile),
                std::io::ErrorKind::PermissionDenied => Err(StatusCode::PermissionDenied),
                _ => Err(StatusCode::Failure),
            }
        }
    }
}

pub async fn handle_fstat(
    state: &SessionState,
    id: u32,
    handle: String,
) -> Result<Attrs, StatusCode> {
    info!("fstat handle: {}", handle);
    if let Some(open_file) = state.open_files.get(&handle) {
        match open_file.file.metadata().await {
            Ok(metadata) => {
                let attrs = MetadataConverter::to_file_attributes(&metadata).await;
                Ok(Attrs { id, attrs })
            }
            Err(e) => {
                error!("Failed to get metadata for handle {}: {}", handle, e);
                Err(StatusCode::Failure)
            }
        }
    } else {
        warn!("Invalid file handle for fstat: {}", handle);
        Err(StatusCode::BadMessage)
    }
}
