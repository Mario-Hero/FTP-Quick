use std::{collections::HashMap, sync::Arc};

use log::{error, info};
use russh_sftp::protocol::{
    Data, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};

use crate::server::ServerConfig;

use super::{SessionState, handlers, utils::path_resolver::PathResolver};

pub struct SftpSession {
    pub(crate) state: SessionState,
    pub(crate) path_resolver: PathResolver,
}

impl SftpSession {
    pub fn new(config: Arc<ServerConfig>) -> Self {
        Self {
            state: SessionState {
                version: None,
                _root_dir: config.root_dir.clone(),
                open_files: HashMap::new(),
                open_dirs: HashMap::new(),
                handle_counter: 0,
                max_read_size: config.max_read_size,
            },
            path_resolver: PathResolver::new(config.root_dir.clone()),
        }
    }

    pub fn next_handle(&mut self) -> String {
        self.state.handle_counter += 1;
        format!("handle_{}", self.state.handle_counter)
    }
}

impl russh_sftp::server::Handler for SftpSession {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        version: u32,
        extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        if self.state.version.is_some() {
            error!("duplicate SSH_FXP_VERSION packet");
            return Err(StatusCode::ConnectionLost);
        }

        self.state.version = Some(version);
        info!(
            "version: {:?}, extensions: {:?}",
            self.state.version, extensions
        );
        Ok(Version::new())
    }

    async fn open(
        &mut self,
        id: u32,
        path: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        handlers::file_ops::handle_open(self, id, path, pflags, _attrs).await
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        handlers::file_ops::hanlde_close(self, id, handle).await
    }

    /// Implementazione migliorata del comando READ per supportare download di file di testo e binari
    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        handlers::file_ops::handle_read(self, id, handle, offset, len).await
    }

    /// Implementazione del comando WRITE per supportare upload di file
    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        handlers::file_ops::handle_write(self, id, handle, offset, data).await
    }

    async fn lstat(
        &mut self,
        id: u32,
        path: String,
    ) -> Result<russh_sftp::protocol::Attrs, Self::Error> {
        handlers::stat_ops::handle_lstat(&self.state, &self.path_resolver, id, path).await
    }

    async fn fstat(
        &mut self,
        id: u32,
        handle: String,
    ) -> Result<russh_sftp::protocol::Attrs, Self::Error> {
        handlers::stat_ops::handle_fstat(&self.state, id, handle).await
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        handlers::dir_ops::opendir(self, id, path).await
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        handlers::dir_ops::hanle_readdir(self, id, handle).await
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        handlers::file_ops::handle_realpath(self, id, path).await
    }

    async fn stat(
        &mut self,
        id: u32,
        path: String,
    ) -> Result<russh_sftp::protocol::Attrs, Self::Error> {
        handlers::stat_ops::handle_stat(&self.state, &self.path_resolver, id, path).await
    }
}
