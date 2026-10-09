//! In-process SSH transport for sandboxed clients. No subprocesses or local PTYs.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use russh::{
    client,
    keys::{known_hosts, PrivateKeyWithHashAlg, PublicKey},
    Channel, ChannelMsg, Disconnect,
};
use russh_sftp::{
    client::SftpSession,
    protocol::{FileAttributes, OpenFlags},
};
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use zeroize::Zeroizing;

pub use russh::keys;
pub type Session = client::Handle<PinnedHost>;
pub type SessionChannel = Channel<client::Msg>;
pub use russh::ChannelMsg as Message;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("SSH operation timed out")]
    Timeout,
    #[error("SSH authentication was rejected")]
    Authentication,
    #[error("SSH failure: {0}")]
    Ssh(#[from] russh::Error),
    #[error("SSH key failure: {0}")]
    Key(#[from] russh::keys::Error),
    #[error("File transfer failure: {0}")]
    Sftp(#[from] russh_sftp::client::error::Error),
    #[error("I/O failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    InvalidInput(String),
}

#[derive(Clone)]
pub struct Connection {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub private_key: Arc<keys::PrivateKey>,
    pub known_hosts: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecOutput {
    pub command: String,
    pub status_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

static HOST_KEY_GATE: Mutex<()> = Mutex::new(());

pub struct PinnedHost {
    host: String,
    port: u16,
    path: PathBuf,
}

impl client::Handler for PinnedHost {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKey) -> Result<bool, Self::Error> {
        // Serialize TOFU enrollment, including concurrent provisioning sessions.
        let _guard = HOST_KEY_GATE
            .lock()
            .map_err(|_| russh::Error::Inconsistent)?;
        match known_hosts::check_known_hosts_path(&self.host, self.port, key, &self.path) {
            Ok(true) => Ok(true),
            Ok(false) => {
                known_hosts::learn_known_hosts_path(&self.host, self.port, key, &self.path)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))?;
                }
                Ok(true)
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl Connection {
    pub async fn connect(&self) -> Result<Session, Error> {
        tokio::time::timeout(Duration::from_secs(10), async {
            let config = Arc::new(client::Config {
                inactivity_timeout: None,
                keepalive_interval: Some(Duration::from_secs(30)),
                keepalive_max: 3,
                nodelay: true,
                ..Default::default()
            });
            if let Some(parent) = self.known_hosts.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let mut session = client::connect(
                config,
                (self.host.as_str(), self.port),
                PinnedHost {
                    host: self.host.clone(),
                    port: self.port,
                    path: self.known_hosts.clone(),
                },
            )
            .await?;
            let hash = session.best_supported_rsa_hash().await?.flatten();
            let auth = session
                .authenticate_publickey(
                    &self.user,
                    PrivateKeyWithHashAlg::new(self.private_key.clone(), hash),
                )
                .await?;
            if !auth.success() {
                return Err(Error::Authentication);
            }
            Ok(session)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    /// Apply one deadline to connect, authenticate, stdin, and command completion.
    /// Never replay a command after dispatch: a dropped reply may follow a committed mutation.
    pub async fn exec(
        &self,
        command: &str,
        stdin: Option<Vec<u8>>,
        limit: Option<Duration>,
    ) -> Result<ExecOutput, Error> {
        let started = Instant::now();
        let operation = async {
            let session = self.connect().await?;
            let result = async {
                let channel = session.channel_open_session().await?;
                channel.exec(true, command).await?;
                let (mut reader, writer) = channel.split();
                // Read output while writing input: commands may fill their output
                // window before consuming all stdin (clipboard and storage payloads).
                let write_input = async {
                    if let Some(bytes) = stdin {
                        let bytes = Zeroizing::new(bytes);
                        writer.data(&bytes[..]).await?;
                    }
                    writer.eof().await?;
                    Ok::<_, Error>(())
                };
                let read_output = async {
                    let mut stdout = Vec::new();
                    let mut stderr = Vec::new();
                    let mut status = -1;
                    while let Some(message) = reader.wait().await {
                        match message {
                            ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                            ChannelMsg::ExtendedData { data, .. } => {
                                stderr.extend_from_slice(&data)
                            }
                            ChannelMsg::ExitStatus { exit_status } => status = exit_status as i32,
                            ChannelMsg::ExitSignal { .. } => status = 255,
                            ChannelMsg::Close => break,
                            // EOF can precede ExitStatus. Continue draining.
                            _ => {}
                        }
                    }
                    Ok::<_, Error>((status, stdout, stderr))
                };
                let (_, (status_code, stdout, stderr)) =
                    tokio::try_join!(write_input, read_output)?;
                Ok(ExecOutput {
                    command: format!(
                        "ssh {}@{}:{} <remote-command>",
                        self.user, self.host, self.port
                    ),
                    status_code,
                    stdout: String::from_utf8_lossy(&stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                    duration_ms: started.elapsed().as_millis(),
                })
            }
            .await;
            let _ = session
                .disconnect(Disconnect::ByApplication, "", "en")
                .await;
            result
        };
        match limit {
            Some(limit) => tokio::time::timeout(limit, operation)
                .await
                .map_err(|_| Error::Timeout)?,
            None => operation.await,
        }
    }

    pub async fn terminal(&self, rows: u16, cols: u16) -> Result<(Session, SessionChannel), Error> {
        tokio::time::timeout(Duration::from_secs(15), async {
            let session = self.connect().await?;
            let mut channel = session.channel_open_session().await?;
            channel
                .request_pty(
                    true,
                    "xterm-256color",
                    cols.max(1).into(),
                    rows.max(1).into(),
                    0,
                    0,
                    &[],
                )
                .await?;
            expect_success(&mut channel).await?;
            channel.request_shell(true).await?;
            expect_success(&mut channel).await?;
            Ok((session, channel))
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    /// The destination is the exact target path, matching the existing upload API.
    pub async fn upload(
        &self,
        source: &Path,
        destination: &str,
        recursive: bool,
        limit: Duration,
    ) -> Result<ExecOutput, Error> {
        let started = Instant::now();
        tokio::time::timeout(limit, async {
            let session = self.connect().await?;
            let result = async {
                let mut channel = session.channel_open_session().await?;
                channel.request_subsystem(true, "sftp").await?;
                expect_success(&mut channel).await?;
                let sftp = SftpSession::new(channel.into_stream()).await?;
                let result = upload_tree(&sftp, source, destination, recursive).await;
                let close_result = sftp.close().await;
                result?;
                close_result?;
                Ok(ExecOutput {
                    command: format!("sftp {}@{}:{} <upload>", self.user, self.host, self.port),
                    status_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                    duration_ms: started.elapsed().as_millis(),
                })
            }
            .await;
            let _ = session
                .disconnect(Disconnect::ByApplication, "", "en")
                .await;
            result
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}

async fn expect_success(channel: &mut SessionChannel) -> Result<(), Error> {
    match channel.wait().await {
        Some(ChannelMsg::Success) => Ok(()),
        _ => Err(Error::InvalidInput(
            "SSH server rejected the channel request".into(),
        )),
    }
}

async fn upload_tree(
    sftp: &SftpSession,
    source: &Path,
    destination: &str,
    recursive: bool,
) -> Result<(), Error> {
    let mut pending = vec![(
        source.to_path_buf(),
        destination.to_owned(),
        Vec::<PathBuf>::new(),
    )];
    while let Some((local, remote, mut ancestors)) = pending.pop() {
        // OpenSSH scp follows local symlinks. Detect ancestor cycles without
        // rejecting independent links to the same file/directory.
        let canonical = tokio::fs::canonicalize(&local).await?;
        let metadata = tokio::fs::metadata(&local).await?;
        if metadata.is_dir() {
            if !recursive {
                return Err(Error::InvalidInput(
                    "Directory upload requires recursive mode".into(),
                ));
            }
            if ancestors.contains(&canonical) {
                return Err(Error::InvalidInput(
                    "Upload contains a directory symlink cycle".into(),
                ));
            }
            ancestors.push(canonical);
            match sftp.metadata(&remote).await {
                Ok(attrs) if attrs.is_dir() => {}
                Ok(_) => {
                    return Err(Error::InvalidInput(
                        "Remote directory target is a file".into(),
                    ))
                }
                Err(russh_sftp::client::error::Error::Status(status))
                    if status.status_code == russh_sftp::protocol::StatusCode::NoSuchFile =>
                {
                    sftp.create_dir(&remote).await?
                }
                Err(error) => return Err(error.into()),
            }
            let mut children = tokio::fs::read_dir(&local).await?;
            while let Some(child) = children.next_entry().await? {
                let name = child
                    .file_name()
                    .into_string()
                    .map_err(|_| Error::InvalidInput("Upload filename is not UTF-8".into()))?;
                pending.push((
                    child.path(),
                    format!("{}/{name}", remote.trim_end_matches('/')),
                    ancestors.clone(),
                ));
            }
        } else if metadata.is_file() {
            let mut input = tokio::fs::File::open(&local).await?;
            let mut attrs = FileAttributes::empty();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                attrs.permissions = Some(metadata.permissions().mode() & 0o777);
            }
            let mut output = sftp
                .open_with_flags_and_attributes(
                    &remote,
                    OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
                    attrs,
                )
                .await?;
            tokio::io::copy(&mut input, &mut output).await?;
            output.shutdown().await?;
        } else {
            return Err(Error::InvalidInput(
                "Upload source is not a regular file or directory".into(),
            ));
        }
    }
    Ok(())
}

/// Streaming UTF-8 decoder: preserve characters split across SSH packets.
#[derive(Default)]
pub struct TerminalText {
    pending: Vec<u8>,
}

impl TerminalText {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    output.push_str(text);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    output.push_str(
                        std::str::from_utf8(&self.pending[..valid]).expect("validated prefix"),
                    );
                    self.pending.drain(..valid);
                    match error.error_len() {
                        Some(length) => {
                            output.push('\u{fffd}');
                            self.pending.drain(..length);
                        }
                        None => break,
                    }
                }
            }
        }
        output
    }

    pub fn finish(&mut self) -> String {
        let tail = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        tail
    }
}
