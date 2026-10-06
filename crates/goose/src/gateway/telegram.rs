use super::{
    Gateway, GatewayConfig, GatewayHandler, IncomingMessage, OutgoingMessage, PlatformUser,
};
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, Response};
#[cfg(unix)]
use rustix::fs::{AtFlags, Mode, OFlags};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
const TELEGRAM_API_BASE: &str = "https://api.telegram.org";
const POLL_TIMEOUT_SECS: u64 = 30;
const MAX_MESSAGE_LENGTH: usize = 4096;
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(5);
/// Maximum voice file size we'll attempt to download (20 MB, Telegram's bot API limit).
const MAX_VOICE_FILE_SIZE: i64 = 20 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VoiceFileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume: u32,
    #[cfg(windows)]
    index_high: u32,
    #[cfg(windows)]
    index_low: u32,
}

struct VoiceTempFile {
    path: tempfile::TempPath,
    identity: VoiceFileIdentity,
    created_at: std::time::SystemTime,
    cleanup_on_drop: bool,
}

impl VoiceTempFile {
    fn remove(&mut self) -> io::Result<bool> {
        self.path.disable_cleanup(true);
        let removed = remove_voice_file_if_unchanged(&self.path, Some(self.identity), |_| {})?;
        self.cleanup_on_drop = false;
        Ok(removed)
    }
}

impl Drop for VoiceTempFile {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let path = self.path.to_path_buf();
            self.path.disable_cleanup(true);
            let _ = remove_voice_file_if_unchanged(&path, Some(self.identity), |_| {});
        }
    }
}

struct VoiceTempFiles {
    parent: PathBuf,
    files: Mutex<Vec<VoiceTempFile>>,
}

impl VoiceTempFiles {
    fn new_in(parent: impl Into<PathBuf>) -> Self {
        Self {
            parent: parent.into(),
            files: Mutex::new(Vec::new()),
        }
    }

    fn save(&self, bytes: &[u8], extension: &str) -> io::Result<PathBuf> {
        let mut file = tempfile::Builder::new()
            .prefix("goose_voice_")
            .suffix(&format!(".{extension}"))
            .tempfile_in(&self.parent)?;
        file.write_all(bytes)?;
        let identity = voice_file_identity(file.as_file())?;
        let path = file.path().to_path_buf();
        let path_owner = file.into_temp_path();
        self.files
            .lock()
            .map_err(|_| io::Error::other("Telegram voice file registry is unavailable"))?
            .push(VoiceTempFile {
                path: path_owner,
                identity,
                created_at: std::time::SystemTime::now(),
                cleanup_on_drop: true,
            });
        Ok(path)
    }

    fn cleanup(&self, max_age: std::time::Duration) -> io::Result<u32> {
        self.cleanup_with_hook(max_age, |_| {})
    }

    fn cleanup_with_hook(
        &self,
        max_age: std::time::Duration,
        mut after_opened_candidate: impl FnMut(&std::path::Path),
    ) -> io::Result<u32> {
        let cutoff = std::time::SystemTime::now()
            .checked_sub(max_age)
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let mut files = self
            .files
            .lock()
            .map_err(|_| io::Error::other("Telegram voice file registry is unavailable"))?;
        let mut removed_tracked = 0;
        let mut retained = Vec::with_capacity(files.len());
        for mut file in std::mem::take(&mut *files) {
            if file.created_at > cutoff {
                retained.push(file);
                continue;
            }
            match file.remove() {
                Ok(true) => removed_tracked += 1,
                Ok(false) => {}
                Err(_) => retained.push(file),
            }
        }
        *files = retained;
        let active_paths: std::collections::HashSet<PathBuf> =
            files.iter().map(|file| file.path.to_path_buf()).collect();
        drop(files);

        let removed_orphans = cleanup_orphaned_voice_files(
            &self.parent,
            cutoff,
            &active_paths,
            &mut after_opened_candidate,
        )?;
        let removed_legacy =
            cleanup_legacy_voice_files(&self.parent, cutoff, &mut after_opened_candidate)?;
        Ok(removed_tracked + removed_orphans + removed_legacy)
    }
}

fn cleanup_orphaned_voice_files(
    parent: &std::path::Path,
    cutoff: std::time::SystemTime,
    active_paths: &std::collections::HashSet<PathBuf>,
    after_opened_candidate: &mut impl FnMut(&std::path::Path),
) -> io::Result<u32> {
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0;
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        if !is_goose_voice_file_name(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        if active_paths.contains(&path) {
            continue;
        }
        let Ok(file) = open_owned_voice_file(&path) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if metadata
            .modified()
            .map_or(true, |modified| modified > cutoff)
        {
            continue;
        }
        let Ok(identity) = voice_file_identity(&file) else {
            continue;
        };
        after_opened_candidate(&path);
        if remove_voice_file_if_unchanged(&path, Some(identity), |_| {})? {
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_goose_voice_file_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(rest) = name.strip_prefix("goose_voice_") else {
        return false;
    };
    let Some((random, extension)) = rest.split_once('.') else {
        return false;
    };
    random.len() == 6
        && random.bytes().all(|byte| byte.is_ascii_alphanumeric())
        && is_voice_file_extension(extension)
}

fn is_legacy_voice_file_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(rest) = name.strip_prefix("voice_") else {
        return false;
    };
    let Some((uuid, extension)) = rest.split_once('.') else {
        return false;
    };
    let uuid_bytes = uuid.as_bytes();
    uuid_bytes.len() == 36
        && uuid_bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && *byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23) && byte.is_ascii_hexdigit()
        })
        && is_voice_file_extension(extension)
}

fn is_voice_file_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 16
        && extension.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'-' | b'_'))
        })
}

fn cleanup_legacy_voice_files(
    parent: &std::path::Path,
    cutoff: std::time::SystemTime,
    after_opened_candidate: &mut impl FnMut(&std::path::Path),
) -> io::Result<u32> {
    let root_path = parent.join("goose_voice");
    let Ok(root) = open_legacy_voice_root(&root_path) else {
        return Ok(0);
    };
    let entries = match std::fs::read_dir(&root_path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0;
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        if !is_legacy_voice_file_name(&name) {
            continue;
        }
        let Ok(file) = open_owned_voice_file_at(&root, &name) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if metadata
            .modified()
            .map_or(true, |modified| modified > cutoff)
        {
            continue;
        }
        let Ok(identity) = voice_file_identity(&file) else {
            continue;
        };
        let path = root_path.join(&name);
        after_opened_candidate(&path);
        let Ok(current) = open_owned_voice_file_at(&root, &name) else {
            continue;
        };
        if voice_file_identity(&current)? != identity {
            continue;
        }
        delete_open_legacy_voice_file(&root, &name, current)?;
        removed += 1;
    }
    Ok(removed)
}

fn remove_voice_file_if_unchanged(
    path: &std::path::Path,
    expected_identity: Option<VoiceFileIdentity>,
    mut after_opened: impl FnMut(&std::path::Path),
) -> io::Result<bool> {
    let file = match open_owned_voice_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Ok(false),
    };
    let identity = voice_file_identity(&file)?;
    if expected_identity.is_some_and(|expected| expected != identity) {
        return Ok(false);
    }
    after_opened(path);
    let current = match open_owned_voice_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Ok(false),
    };
    if voice_file_identity(&current)? != identity {
        return Ok(false);
    }
    delete_open_voice_file(current, path)?;
    Ok(true)
}

#[cfg(unix)]
fn delete_open_voice_file(_file: std::fs::File, path: &std::path::Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

#[cfg(unix)]
fn open_legacy_voice_root(path: &std::path::Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let directory = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW).bits() as i32)
        .open(path)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o700
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an owned legacy Telegram voice directory",
        ));
    }
    Ok(directory)
}

#[cfg(unix)]
fn open_owned_voice_file_at(
    directory: &std::fs::File,
    name: &std::ffi::OsStr,
) -> io::Result<std::fs::File> {
    let descriptor = rustix::fs::openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    let file = std::fs::File::from(descriptor);
    validate_owned_voice_file(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn delete_open_legacy_voice_file(
    directory: &std::fs::File,
    name: &std::ffi::OsStr,
    _file: std::fs::File,
) -> io::Result<()> {
    rustix::fs::unlinkat(directory, name, AtFlags::empty())?;
    Ok(())
}

#[cfg(unix)]
fn open_owned_voice_file(path: &std::path::Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    validate_owned_voice_file(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn validate_owned_voice_file(file: &std::fs::File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an owned Telegram voice tempfile",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn voice_file_identity(file: &std::fs::File) -> io::Result<VoiceFileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(VoiceFileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn open_owned_voice_file(path: &std::path::Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use winapi::um::winbase::FILE_FLAG_OPEN_REPARSE_POINT;
    use winapi::um::winnt::{
        DELETE, FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    validate_owned_voice_file(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn validate_owned_voice_file(file: &std::fs::File) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an owned Telegram voice tempfile",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn open_legacy_voice_root(path: &std::path::Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use winapi::um::winbase::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};
    use winapi::um::winnt::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    };

    let directory = std::fs::OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not an owned legacy Telegram voice directory",
        ));
    }
    Ok(directory)
}

#[cfg(windows)]
fn open_owned_voice_file_at(
    directory: &std::fs::File,
    name: &std::ffi::OsStr,
) -> io::Result<std::fs::File> {
    use ntapi::ntioapi::{
        FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, IO_STATUS_BLOCK,
        NtCreateFile,
    };
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use winapi::shared::ntdef::{
        HANDLE, NT_SUCCESS, OBJ_CASE_INSENSITIVE, OBJECT_ATTRIBUTES, UNICODE_STRING,
    };
    use winapi::um::winnt::{
        DELETE, FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let mut name: Vec<u16> = name.encode_wide().collect();
    let name_bytes = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Telegram voice filename is too long",
            )
        })?;
    let mut unicode_name = UNICODE_STRING {
        Length: name_bytes,
        MaximumLength: name_bytes,
        Buffer: name.as_mut_ptr(),
    };
    let mut attributes = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: directory.as_raw_handle() as HANDLE,
        ObjectName: &mut unicode_name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null_mut(),
        SecurityQualityOfService: std::ptr::null_mut(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    let mut io_status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_GENERIC_READ | DELETE,
            &mut attributes,
            &mut io_status,
            std::ptr::null_mut(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null_mut(),
            0,
        )
    };
    if !NT_SUCCESS(status) {
        let error = unsafe { ntapi::ntrtl::RtlNtStatusToDosError(status) };
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let file = unsafe { std::fs::File::from_raw_handle(handle.cast()) };
    validate_owned_voice_file(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn delete_open_legacy_voice_file(
    _directory: &std::fs::File,
    _name: &std::ffi::OsStr,
    file: std::fs::File,
) -> io::Result<()> {
    delete_open_voice_file(file, std::path::Path::new(""))
}

#[cfg(windows)]
fn delete_open_voice_file(file: std::fs::File, _path: &std::path::Path) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use winapi::um::fileapi::{FILE_DISPOSITION_INFO, SetFileInformationByHandle};
    use winapi::um::minwinbase::FileDispositionInfo;
    let mut disposition = FILE_DISPOSITION_INFO { DeleteFile: 1 };
    // SAFETY: the handle is live and the information buffer matches FileDispositionInfo.
    let result = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileDispositionInfo,
            (&mut disposition as *mut FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn voice_file_identity(file: &std::fs::File) -> io::Result<VoiceFileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use winapi::um::fileapi::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};

    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let result =
        unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(VoiceFileIdentity {
        volume: information.dwVolumeSerialNumber,
        index_high: information.nFileIndexHigh,
        index_low: information.nFileIndexLow,
    })
}

pub struct TelegramGateway {
    bot_token: String,
    client: Client,
    api_base: String,
    voice_temp_files: Arc<VoiceTempFiles>,
}

#[derive(Debug, Serialize)]
struct SendRichMessageRequest<'a> {
    chat_id: i64,
    rich_message: InputRichMessage<'a>,
}

#[derive(Debug, Serialize)]
struct InputRichMessage<'a> {
    markdown: &'a str,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    from: Option<TelegramUser>,
    chat: TelegramChat,
    text: Option<String>,
    voice: Option<TelegramVoice>,
    audio: Option<TelegramAudio>,
}

#[derive(Debug, Deserialize)]
struct TelegramVoice {
    file_id: String,
    #[allow(dead_code)]
    duration: Option<i32>,
    #[allow(dead_code)]
    mime_type: Option<String>,
    file_size: Option<i64>,
}

/// Audio files sent as documents (not inline voice notes).
#[derive(Debug, Deserialize)]
struct TelegramAudio {
    file_id: String,
    #[allow(dead_code)]
    duration: Option<i32>,
    #[allow(dead_code)]
    mime_type: Option<String>,
    file_size: Option<i64>,
}

/// Metadata extracted from a Telegram voice note or audio attachment.
struct VoiceInfo<'a> {
    file_id: &'a str,
    file_size: Option<i64>,
    duration: Option<i32>,
    mime_type: Option<&'a str>,
}

/// Response from the Telegram `getFile` API.
#[derive(Debug, Deserialize)]
struct TelegramFile {
    #[allow(dead_code)]
    file_id: String,
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    first_name: String,
    last_name: Option<String>,
    #[allow(dead_code)]
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    chat_type: String,
}

#[derive(Debug, Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

impl TelegramGateway {
    pub fn new(config: &GatewayConfig) -> anyhow::Result<Self> {
        Self::new_with_voice_temp_parent(config, std::env::temp_dir())
    }

    fn new_with_voice_temp_parent(
        config: &GatewayConfig,
        voice_temp_parent: PathBuf,
    ) -> anyhow::Result<Self> {
        let bot_token = config.platform_config["bot_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing bot_token in platform_config"))?
            .to_string();

        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .http1_only()
            .build()?;
        Ok(Self {
            bot_token,
            client,
            api_base: TELEGRAM_API_BASE.to_string(),
            voice_temp_files: Arc::new(VoiceTempFiles::new_in(voice_temp_parent)),
        })
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base, self.bot_token, method)
    }

    async fn send_request(request: RequestBuilder) -> reqwest::Result<Response> {
        request.send().await.map_err(reqwest::Error::without_url)
    }

    async fn response_json<T: DeserializeOwned>(response: Response) -> reqwest::Result<T> {
        response.json().await.map_err(reqwest::Error::without_url)
    }

    async fn response_bytes(response: Response) -> reqwest::Result<Vec<u8>> {
        response
            .bytes()
            .await
            .map(Vec::from)
            .map_err(reqwest::Error::without_url)
    }

    async fn get_updates(&self, offset: Option<i64>) -> anyhow::Result<Vec<TelegramUpdate>> {
        let mut params = serde_json::json!({
            "timeout": POLL_TIMEOUT_SECS,
            "allowed_updates": ["message"],
        });
        if let Some(offset) = offset {
            params["offset"] = serde_json::json!(offset);
        }

        let response = Self::send_request(
            self.client
                .post(self.api_url("getUpdates"))
                .json(&params)
                .timeout(std::time::Duration::from_secs(POLL_TIMEOUT_SECS + 10)),
        )
        .await?;
        let resp: TelegramResponse<Vec<TelegramUpdate>> = Self::response_json(response).await?;

        resp.result.ok_or_else(|| {
            anyhow::anyhow!(
                "Telegram API error: {}",
                resp.description.unwrap_or_default()
            )
        })
    }

    async fn send_text(&self, chat_id: i64, text: &str) -> anyhow::Result<()> {
        let chunks = split_message(text, MAX_MESSAGE_LENGTH);
        for (index, chunk) in chunks.iter().enumerate() {
            let resp = Self::send_request(self.client.post(self.api_url("sendRichMessage")).json(
                &SendRichMessageRequest {
                    chat_id,
                    rich_message: InputRichMessage { markdown: chunk },
                },
            ))
            .await?;

            if let Ok(body) = Self::response_json::<TelegramResponse<serde_json::Value>>(resp).await
            {
                if !body.ok {
                    tracing::warn!(
                        error = body.description.as_deref().unwrap_or("unknown"),
                        "Telegram rejected rich markdown, falling back to plain text"
                    );
                    for plain_chunk in &chunks[index..] {
                        let plain_response =
                            Self::send_request(self.client.post(self.api_url("sendMessage")).json(
                                &serde_json::json!({
                                    "chat_id": chat_id,
                                    "text": plain_chunk,
                                }),
                            ))
                            .await?;
                        let plain_resp: TelegramResponse<serde_json::Value> =
                            Self::response_json(plain_response).await?;
                        if !plain_resp.ok {
                            anyhow::bail!(
                                "Telegram sendMessage failed: {}",
                                plain_resp.description.unwrap_or_default()
                            );
                        }
                    }
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    async fn send_chat_action(&self, chat_id: i64, action: &str) -> anyhow::Result<()> {
        Self::send_request(self.client.post(self.api_url("sendChatAction")).json(
            &serde_json::json!({
                "chat_id": chat_id,
                "action": action,
            }),
        ))
        .await?;
        Ok(())
    }

    /// Download a file from Telegram by its `file_id`.
    ///
    /// This is a two-step process:
    /// 1. Call `getFile` to obtain the server-side `file_path`.
    /// 2. Fetch the raw bytes from `https://api.telegram.org/file/bot<TOKEN>/<file_path>`.
    async fn download_file(&self, file_id: &str) -> anyhow::Result<Vec<u8>> {
        // Step 1 – resolve file_id → file_path
        let response = Self::send_request(
            self.client
                .post(self.api_url("getFile"))
                .json(&serde_json::json!({ "file_id": file_id })),
        )
        .await?;
        let resp: TelegramResponse<TelegramFile> = Self::response_json(response).await?;

        let tg_file = resp.result.ok_or_else(|| {
            anyhow::anyhow!(
                "Telegram getFile error: {}",
                resp.description.unwrap_or_default()
            )
        })?;

        let file_path = tg_file
            .file_path
            .ok_or_else(|| anyhow::anyhow!("Telegram getFile returned no file_path"))?;

        // Step 2 – download raw bytes
        let download_url = format!(
            "{}/file/bot{}/{}",
            TELEGRAM_API_BASE, self.bot_token, file_path
        );
        let response = Self::send_request(self.client.get(&download_url)).await?;
        Ok(Self::response_bytes(response).await?)
    }

    /// Save voice bytes to a temporary file and return the path.
    ///
    /// Files are stored as protected, exclusively created temporary files so
    /// Goose can access them via its shell tools. The extension is derived from
    /// the MIME type when available, falling back to `.ogg` for voice notes.
    ///
    /// On Unix files are created with mode `0600` so other local users cannot
    /// read private voice content.
    fn save_voice_file(&self, bytes: &[u8], mime_type: Option<&str>) -> anyhow::Result<PathBuf> {
        let ext = Self::voice_file_extension(mime_type);
        Ok(self.voice_temp_files.save(bytes, &ext)?)
    }

    fn voice_file_extension(mime_type: Option<&str>) -> String {
        let media_type = mime_type
            .and_then(|mime| mime.split(';').next())
            .map(str::trim)
            .map(str::to_ascii_lowercase);
        let subtype = media_type
            .as_deref()
            .and_then(|mime| mime.strip_prefix("audio/"));

        let Some(subtype) = subtype else {
            return "ogg".to_string();
        };

        match subtype {
            "mpeg" => "mp3".to_string(),
            "mp4" | "x-m4a" => "m4a".to_string(),
            "ogg" => "ogg".to_string(),
            "wav" | "x-wav" | "vnd.wave" => "wav".to_string(),
            other
                if other.len() <= 16
                    && other.bytes().enumerate().all(|(index, byte)| {
                        byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'-' | b'_'))
                    }) =>
            {
                other.to_string()
            }
            _ => "ogg".to_string(),
        }
    }

    /// Build the text prompt that tells Goose about a voice message file.
    fn voice_prompt(
        path: &std::path::Path,
        duration: Option<i32>,
        mime_type: Option<&str>,
    ) -> String {
        let duration_hint = duration
            .map(|d| format!(" (duration: {d}s)"))
            .unwrap_or_default();
        let format_hint = mime_type
            .map(|m| format!(" The file format is {m}."))
            .unwrap_or_default();
        format!(
            "The user sent a voice message{duration_hint}. \
             The audio file is saved at: {}{format_hint}\n\n\
             Please transcribe this audio file using available command-line tools \
             (e.g. whisper, ffmpeg, sox, or any STT utility you can find on this system) \
             and then respond to what the user said. \
             If no transcription tool is available, let the user know and ask them to type their message instead.",
            path.display()
        )
    }

    /// Extract metadata from either a voice note or an audio attachment.
    /// Returns `None` when neither is present.
    fn voice_info(msg: &TelegramMessage) -> Option<VoiceInfo<'_>> {
        if let Some(ref v) = msg.voice {
            return Some(VoiceInfo {
                file_id: &v.file_id,
                file_size: v.file_size,
                duration: v.duration,
                mime_type: v.mime_type.as_deref(),
            });
        }
        if let Some(ref a) = msg.audio {
            return Some(VoiceInfo {
                file_id: &a.file_id,
                file_size: a.file_size,
                duration: a.duration,
                mime_type: a.mime_type.as_deref(),
            });
        }
        None
    }

    fn to_platform_user(tg_msg: &TelegramMessage) -> PlatformUser {
        PlatformUser {
            platform: "telegram".to_string(),
            user_id: tg_msg.chat.id.to_string(),
            display_name: tg_msg.from.as_ref().map(|u| {
                let mut name = u.first_name.clone();
                if let Some(ref last) = u.last_name {
                    name.push(' ');
                    name.push_str(last);
                }
                name
            }),
        }
    }
}

#[async_trait]
impl Gateway for TelegramGateway {
    fn gateway_type(&self) -> &str {
        "telegram"
    }

    async fn start(
        &self,
        handler: GatewayHandler,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        let mut offset: Option<i64> = None;

        tracing::info!("Telegram gateway starting long-poll loop");

        // Spawn a background task that periodically removes stale voice files
        // (older than 1 hour) so they don't accumulate on disk.
        let cleanup_cancel = cancel.clone();
        let voice_temp_files = Arc::clone(&self.voice_temp_files);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(600));
            loop {
                tokio::select! {
                    _ = cleanup_cancel.cancelled() => break,
                    _ = interval.tick() => {
                        if let Err(error) = voice_temp_files.cleanup(std::time::Duration::from_secs(3600)) {
                            tracing::warn!(%error, "failed to clean up Telegram voice files");
                        }
                    }
                }
            }
        });

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("Telegram gateway shutting down");
                    break;
                }
                result = self.get_updates(offset) => {
                    match result {
                        Ok(updates) => {
                            for update in updates {
                                offset = Some(update.update_id + 1);

                                let Some(tg_msg) = update.message else {
                                    continue;
                                };

                                // Determine the text to send to the handler.
                                // Voice/audio messages are downloaded, saved to
                                // disk, and converted into a prompt that asks
                                // Goose to transcribe the file using CLI tools.
                                let text = if let Some(voice) = Self::voice_info(&tg_msg) {
                                    // Reject files that exceed the Telegram bot
                                    // download limit.
                                    if voice.file_size.unwrap_or(0) > MAX_VOICE_FILE_SIZE {
                                        tracing::warn!(
                                            file_size = voice.file_size,
                                            "voice file exceeds size limit, skipping"
                                        );
                                        continue;
                                    }

                                    match self.download_file(voice.file_id).await {
                                        Ok(bytes) => match self.save_voice_file(&bytes, voice.mime_type) {
                                            Ok(path) => Self::voice_prompt(&path, voice.duration, voice.mime_type),
                                            Err(e) => {
                                                tracing::error!(
                                                    error = %e,
                                                    "failed to save voice file"
                                                );
                                                continue;
                                            }
                                        },
                                        Err(e) => {
                                            tracing::error!(
                                                error = %e,
                                                "failed to download voice file from Telegram"
                                            );
                                            continue;
                                        }
                                    }
                                } else if let Some(ref t) = tg_msg.text {
                                    t.clone()
                                } else {
                                    // Neither text nor voice — skip.
                                    continue;
                                };

                                let user = Self::to_platform_user(&tg_msg);
                                let incoming = IncomingMessage {
                                    user,
                                    text,
                                    platform_message_id: Some(tg_msg.message_id.to_string()),
                                    attachments: vec![],
                                };

                                let handler = handler.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handler.handle_message(incoming).await {
                                        tracing::error!(error = %e, "error handling Telegram message");
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = ?e, "Telegram poll error");
                            tokio::time::sleep(RETRY_DELAY).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn send_message(
        &self,
        user: &PlatformUser,
        message: OutgoingMessage,
    ) -> anyhow::Result<()> {
        let chat_id: i64 = user
            .user_id
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid chat_id: {}", user.user_id))?;

        match message {
            OutgoingMessage::Text { body } => {
                self.send_text(chat_id, &body).await?;
            }
            OutgoingMessage::Typing => {
                self.send_chat_action(chat_id, "typing").await?;
            }
        }

        Ok(())
    }

    async fn validate_config(&self) -> anyhow::Result<()> {
        let response = Self::send_request(self.client.get(self.api_url("getMe"))).await?;
        let resp: TelegramResponse<serde_json::Value> = Self::response_json(response).await?;

        if !resp.ok {
            anyhow::bail!(
                "invalid Telegram bot token: {}",
                resp.description.unwrap_or_default()
            );
        }

        if let Some(result) = &resp.result {
            if let Some(username) = result.get("username").and_then(|v| v.as_str()) {
                tracing::info!(bot = %username, "Telegram bot verified");
            }
        }

        Ok(())
    }
}

#[allow(clippy::string_slice)]
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let mut cut = max_len;
        while cut > 0 && !remaining.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == 0 {
            cut = remaining
                .char_indices()
                .nth(1)
                .map(|(i, _)| i)
                .unwrap_or(remaining.len());
        }

        let split_at = remaining[..cut]
            .rfind('\n')
            .or_else(|| remaining[..cut].rfind(' '))
            .map(|pos| pos + 1)
            .unwrap_or(cut);

        chunks.push(remaining[..split_at].to_string());
        remaining = &remaining[split_at..];
    }

    chunks
}
