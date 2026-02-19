use log::warn;
use russh_sftp::protocol::StatusCode;
use std::path::{Path, PathBuf};

pub struct PathResolver {
    root_dir: PathBuf,
}

impl PathResolver {
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    pub fn resolve_path(&self, path: &str) -> Result<PathBuf, StatusCode> {
        let path = if path.starts_with('/') {
            Path::new(path).strip_prefix("/").unwrap_or(Path::new(path))
        } else {
            Path::new(path)
        };

        let resolved = self.root_dir.join(path);

        match resolved.canonicalize() {
            Ok(canonical) => {
                if canonical.starts_with(&self.root_dir) ||
                    // 长路径支持
                    canonical.starts_with("\\\\?\\".to_string() + self.root_dir.to_str().unwrap()){
                    Ok(canonical)
                } else {
                    warn!(
                        "尝试访问根目录以外的位置: {:?} 根目录: {:?}",
                        canonical, self.root_dir
                    );
                    Err(StatusCode::PermissionDenied)
                }
            }
            Err(_) => {
                if let Some(parent) = resolved.parent() {
                    match parent.canonicalize() {
                        Ok(canonical_parent) => {
                            if canonical_parent.starts_with(&self.root_dir) {
                                Ok(resolved)
                            } else {
                                warn!(
                                    "尝试访问根目录以外的位置: {:?} 根目录: {:?}",
                                    resolved, self.root_dir
                                );
                                Err(StatusCode::PermissionDenied)
                            }
                        }
                        Err(_) => Err(StatusCode::NoSuchFile),
                    }
                } else {
                    Err(StatusCode::NoSuchFile)
                }
            }
        }
    }

    pub fn get_root_dir(&self) -> &PathBuf {
        &self.root_dir
    }
}
