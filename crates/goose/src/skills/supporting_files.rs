#[cfg(unix)]
use rustix::fs::{Mode, OFlags};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path};

const LOADED_FILE_PREFIX: &str = "# Loaded: ";
const LOADED_FILE_SEPARATOR: &str = "\n\n";
const LOADED_FILE_SUFFIX: &str = "\n\n---\nFile loaded into context.";
const MAX_SOURCE_FILE_BYTES: usize = crate::scheduler::MAX_SCHEDULE_RECIPE_BYTES as usize;

#[derive(Clone, Copy)]
enum ReadLimit {
    Characters(usize),
    Bytes(usize),
}

pub(crate) fn load_supporting_file(
    skill_dir: &Path,
    relative: &Path,
    skill_name: &str,
) -> io::Result<String> {
    load_supporting_file_with_limit(
        skill_dir,
        relative,
        skill_name,
        crate::agents::max_tool_response_size(),
    )
}

fn load_supporting_file_with_limit(
    skill_dir: &Path,
    relative: &Path,
    skill_name: &str,
    max_characters: usize,
) -> io::Result<String> {
    let wrapper_characters = LOADED_FILE_PREFIX.chars().count()
        + skill_name.chars().count()
        + LOADED_FILE_SEPARATOR.chars().count()
        + LOADED_FILE_SUFFIX.chars().count();
    let content_limit = max_characters
        .checked_sub(wrapper_characters)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "maximum tool response size of {max_characters} characters is too small to load '{skill_name}'"
                ),
            )
        })?;
    let content = read_supporting_file_with_limit(skill_dir, relative, content_limit)?;
    Ok(format!(
        "{LOADED_FILE_PREFIX}{skill_name}{LOADED_FILE_SEPARATOR}{content}{LOADED_FILE_SUFFIX}"
    ))
}

fn read_supporting_file_with_limit(
    skill_dir: &Path,
    relative: &Path,
    max_characters: usize,
) -> io::Result<String> {
    read_supporting_file_with_hook(skill_dir, relative, max_characters, |_| {})
}

pub(crate) fn read_source_file(source_dir: &Path, relative: &Path) -> io::Result<String> {
    read_confined_file_with_hook(
        source_dir,
        relative,
        ReadLimit::Bytes(MAX_SOURCE_FILE_BYTES),
        |_| {},
    )
}

pub(crate) fn write_source_file(
    source_dir: &Path,
    relative: &Path,
    content: &[u8],
) -> io::Result<()> {
    write_confined_file_with_hook(source_dir, relative, content, false, |_| {})
}

pub(crate) fn create_source_file(
    source_dir: &Path,
    relative: &Path,
    content: &[u8],
) -> io::Result<()> {
    write_confined_file_with_hook(source_dir, relative, content, true, |_| {})
}

fn read_supporting_file_with_hook(
    skill_dir: &Path,
    relative: &Path,
    max_characters: usize,
    after_opened_component: impl FnMut(&Path),
) -> io::Result<String> {
    read_confined_file_with_hook(
        skill_dir,
        relative,
        ReadLimit::Characters(max_characters),
        after_opened_component,
    )
}

fn max_utf8_bytes(max_characters: usize) -> io::Result<usize> {
    max_characters.checked_mul(4).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configured supporting file size limit is too large",
        )
    })
}

fn read_utf8_with_limit(mut reader: impl io::Read, max_characters: usize) -> io::Result<String> {
    let max_bytes = max_utf8_bytes(max_characters)?;
    let read_size = max_bytes.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configured supporting file size limit is too large",
        )
    })?;
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(read_size as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(file_encoding_too_large(max_bytes));
    }
    let content = String::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if content.chars().count() > max_characters {
        return Err(file_too_large(max_characters));
    }
    Ok(content)
}

fn read_utf8_with_byte_limit(mut reader: impl io::Read, max_bytes: usize) -> io::Result<String> {
    let read_size = max_bytes.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "configured source file size limit is too large",
        )
    })?;
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(read_size as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(file_encoding_too_large(max_bytes));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn file_too_large(max_characters: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("supporting file exceeds the maximum size of {max_characters} characters"),
    )
}

fn file_encoding_too_large(max_bytes: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("supporting file exceeds the maximum encoded size of {max_bytes} bytes"),
    )
}

fn read_opened_file(file: fs::File, limit: ReadLimit) -> io::Result<String> {
    let max_bytes = match limit {
        ReadLimit::Characters(max_characters) => max_utf8_bytes(max_characters)?,
        ReadLimit::Bytes(max_bytes) => max_bytes,
    };
    if file.metadata()?.len() > max_bytes as u64 {
        return Err(file_encoding_too_large(max_bytes));
    }
    match limit {
        ReadLimit::Characters(max_characters) => read_utf8_with_limit(file, max_characters),
        ReadLimit::Bytes(max_bytes) => read_utf8_with_byte_limit(file, max_bytes),
    }
}

fn validated_relative_components(path: &Path) -> io::Result<Vec<&std::ffi::OsStr>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(component) => components.push(component),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "supporting file path must stay within the skill directory",
                ));
            }
        }
    }
    if components.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "supporting file path must name a file",
        ));
    }
    Ok(components)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn directory_traversal_flags() -> OFlags {
    OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn directory_traversal_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

#[cfg(unix)]
fn open_skill_root(
    skill_dir: &Path,
    after_opened_component: &mut impl FnMut(&Path),
) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(directory_traversal_flags().bits() as i32);
    let mut directory = options.open(Path::new("/"))?;
    let mut opened_path = std::path::PathBuf::from("/");
    let mut saw_root = false;
    for component in skill_dir.components() {
        match component {
            Component::RootDir if !saw_root => saw_root = true,
            Component::Normal(component) if saw_root => {
                directory = open_at(&directory, component, directory_traversal_flags())?;
                opened_path.push(component);
                after_opened_component(&opened_path);
            }
            Component::CurDir if saw_root => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "skill path must be an absolute normalized path",
                ));
            }
        }
    }
    if !saw_root || opened_path != skill_dir {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill path must be an absolute normalized path",
        ));
    }
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill path is not a directory",
        ));
    }
    Ok(directory)
}

#[cfg(unix)]
fn read_confined_file_with_hook(
    skill_dir: &Path,
    relative: &Path,
    limit: ReadLimit,
    mut after_opened_component: impl FnMut(&Path),
) -> io::Result<String> {
    let components = validated_relative_components(relative)?;
    let (file_name, ancestors) = components.split_last().unwrap();
    let mut directory = open_skill_root(skill_dir, &mut after_opened_component)?;

    let mut opened_path = std::path::PathBuf::new();
    for ancestor in ancestors {
        directory = open_at(&directory, ancestor, directory_traversal_flags())?;
        opened_path.push(ancestor);
        after_opened_component(&opened_path);
    }

    let file = open_at(
        &directory,
        file_name,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
    )?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "supporting file is not a regular file",
        ));
    }

    read_opened_file(file, limit)
}

#[cfg(unix)]
fn write_confined_file_with_hook(
    source_dir: &Path,
    relative: &Path,
    content: &[u8],
    create_new: bool,
    mut after_opened_component: impl FnMut(&Path),
) -> io::Result<()> {
    let components = validated_relative_components(relative)?;
    let (file_name, ancestors) = components.split_last().unwrap();
    let mut directory = open_skill_root(source_dir, &mut after_opened_component)?;

    let mut opened_path = std::path::PathBuf::new();
    for ancestor in ancestors {
        directory = open_at(&directory, ancestor, directory_traversal_flags())?;
        opened_path.push(ancestor);
        after_opened_component(&opened_path);
    }

    let mut flags =
        OFlags::WRONLY | OFlags::CREATE | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    if create_new {
        flags |= OFlags::EXCL;
    }
    let mut file = open_at_with_mode(&directory, file_name, flags, Mode::from_raw_mode(0o666))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source path is not a regular file",
        ));
    }
    ensure_source_file_has_single_link(&file, &metadata)?;
    if !create_new {
        file.set_len(0)?;
    }
    file.write_all(content)
}

#[cfg(unix)]
fn ensure_source_file_has_single_link(_file: &fs::File, metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    if metadata.nlink() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source path must have exactly one hard link",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_at(directory: &fs::File, name: &std::ffi::OsStr, flags: OFlags) -> io::Result<fs::File> {
    open_at_with_mode(directory, name, flags, Mode::empty())
}

#[cfg(unix)]
fn open_at_with_mode(
    directory: &fs::File,
    name: &std::ffi::OsStr,
    flags: OFlags,
    mode: Mode,
) -> io::Result<fs::File> {
    let descriptor = rustix::fs::openat(directory, name, flags, mode)?;
    Ok(fs::File::from(descriptor))
}

#[cfg(windows)]
fn open_skill_root(
    skill_dir: &Path,
    after_opened_component: &mut impl FnMut(&Path),
) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use winapi::um::winbase::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};
    use winapi::um::winnt::{
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
        SYNCHRONIZE,
    };

    let root_anchor = skill_dir
        .ancestors()
        .last()
        .filter(|path| path.has_root())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "skill path must be an absolute normalized path",
            )
        })?;
    let relative = skill_dir.strip_prefix(root_anchor).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill path must be an absolute normalized path",
        )
    })?;
    let components = if relative.as_os_str().is_empty() {
        Vec::new()
    } else {
        validated_relative_components(relative)?
    };

    let mut options = fs::OpenOptions::new();
    options
        .access_mode(FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    let mut directory = options.open(root_anchor)?;
    let root_metadata = directory.metadata()?;
    if windows_metadata_is_reparse_point(&root_metadata) || !root_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill path is not a directory",
        ));
    }
    let mut opened_path = root_anchor.to_path_buf();
    for component in components {
        directory = windows_open_at(&directory, component, true)?;
        let metadata = directory.metadata()?;
        if windows_metadata_is_reparse_point(&metadata) || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "skill path ancestor is not a regular directory",
            ));
        }
        opened_path.push(component);
        after_opened_component(&opened_path);
    }
    if opened_path != skill_dir {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill path must be an absolute normalized path",
        ));
    }
    Ok(directory)
}

#[cfg(windows)]
fn read_confined_file_with_hook(
    skill_dir: &Path,
    relative: &Path,
    limit: ReadLimit,
    mut after_opened_component: impl FnMut(&Path),
) -> io::Result<String> {
    let components = validated_relative_components(relative)?;
    let (file_name, ancestors) = components.split_last().unwrap();
    let mut directory = open_skill_root(skill_dir, &mut after_opened_component)?;

    let mut opened_path = std::path::PathBuf::new();
    for ancestor in ancestors {
        directory = windows_open_at(&directory, ancestor, true)?;
        let metadata = directory.metadata()?;
        if windows_metadata_is_reparse_point(&metadata) || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "supporting file path ancestor is not a regular directory",
            ));
        }
        opened_path.push(ancestor);
        after_opened_component(&opened_path);
    }

    let file = windows_open_at(&directory, file_name, false)?;
    let metadata = file.metadata()?;
    if windows_metadata_is_reparse_point(&metadata) || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "supporting file is not a regular file",
        ));
    }

    read_opened_file(file, limit)
}

#[cfg(windows)]
fn write_confined_file_with_hook(
    source_dir: &Path,
    relative: &Path,
    content: &[u8],
    create_new: bool,
    mut after_opened_component: impl FnMut(&Path),
) -> io::Result<()> {
    let components = validated_relative_components(relative)?;
    let (file_name, ancestors) = components.split_last().unwrap();
    let mut directory = open_skill_root(source_dir, &mut after_opened_component)?;

    let mut opened_path = std::path::PathBuf::new();
    for ancestor in ancestors {
        directory = windows_open_at(&directory, ancestor, true)?;
        let metadata = directory.metadata()?;
        if windows_metadata_is_reparse_point(&metadata) || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source path ancestor is not a regular directory",
            ));
        }
        opened_path.push(ancestor);
        after_opened_component(&opened_path);
    }

    let mut file = windows_open_file_at(&directory, file_name, create_new)?;
    let metadata = file.metadata()?;
    if windows_metadata_is_reparse_point(&metadata) || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source path is not a regular file",
        ));
    }
    ensure_source_file_has_single_link(&file, &metadata)?;
    if !create_new {
        file.set_len(0)?;
    }
    file.write_all(content)
}

#[cfg(windows)]
fn ensure_source_file_has_single_link(file: &fs::File, _metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use winapi::um::fileapi::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};

    // SAFETY: BY_HANDLE_FILE_INFORMATION is a plain C data structure initialized before the call.
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the file owns a valid handle and information points to writable initialized storage.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if information.nNumberOfLinks != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source path must have exactly one hard link",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_open_at(
    directory: &fs::File,
    name: &std::ffi::OsStr,
    directory_only: bool,
) -> io::Result<fs::File> {
    use ntapi::ntioapi::{
        FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use winapi::um::winnt::{FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_TRAVERSE, SYNCHRONIZE};

    let mut create_options = FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT;
    if directory_only {
        create_options |= FILE_DIRECTORY_FILE;
    }
    let desired_access = if directory_only {
        FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE
    } else {
        FILE_GENERIC_READ
    };
    windows_open_at_with_options(directory, name, desired_access, FILE_OPEN, create_options)
}

#[cfg(windows)]
fn windows_open_file_at(
    directory: &fs::File,
    name: &std::ffi::OsStr,
    create_new: bool,
) -> io::Result<fs::File> {
    use ntapi::ntioapi::{
        FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_IF, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use winapi::um::winnt::{FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, SYNCHRONIZE};

    let create_disposition = if create_new {
        FILE_CREATE
    } else {
        FILE_OPEN_IF
    };
    windows_open_at_with_options(
        directory,
        name,
        FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        create_disposition,
        FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
    )
}

#[cfg(windows)]
fn windows_open_at_with_options(
    directory: &fs::File,
    name: &std::ffi::OsStr,
    desired_access: u32,
    create_disposition: u32,
    create_options: u32,
) -> io::Result<fs::File> {
    use ntapi::ntioapi::{IO_STATUS_BLOCK, NtCreateFile};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use winapi::shared::ntdef::{
        HANDLE, NT_SUCCESS, OBJ_CASE_INSENSITIVE, OBJECT_ATTRIBUTES, UNICODE_STRING,
    };
    use winapi::um::winnt::{FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE};

    let mut name: Vec<u16> = name.encode_wide().collect();
    let name_bytes = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "supporting file path component is too long",
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
    // SAFETY: IO_STATUS_BLOCK is a plain C data structure initialized before the synchronous call.
    let mut io_status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // SAFETY: all pointers reference initialized values for the duration of the synchronous call.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &mut attributes,
            &mut io_status,
            std::ptr::null_mut(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            create_disposition,
            create_options,
            std::ptr::null_mut(),
            0,
        )
    };
    if !NT_SUCCESS(status) {
        return Err(windows_nt_status_error(status));
    }
    // SAFETY: NtCreateFile returned a new owned handle on success.
    Ok(unsafe { fs::File::from_raw_handle(handle.cast()) })
}

#[cfg(windows)]
fn windows_metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use winapi::um::winnt::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(windows)]
fn windows_nt_status_error(status: winapi::shared::ntdef::NTSTATUS) -> io::Error {
    // SAFETY: RtlNtStatusToDosError accepts every NTSTATUS value.
    let error = unsafe { ntapi::ntrtl::RtlNtStatusToDosError(status) };
    io::Error::from_raw_os_error(error as i32)
}

#[cfg(not(any(unix, windows)))]
fn read_confined_file_with_hook(
    _skill_dir: &Path,
    relative: &Path,
    _limit: ReadLimit,
    _after_opened_component: impl FnMut(&Path),
) -> io::Result<String> {
    validated_relative_components(relative)?;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure supporting file reads are not supported on this platform",
    ))
}

#[cfg(not(any(unix, windows)))]
fn write_confined_file_with_hook(
    _source_dir: &Path,
    relative: &Path,
    _content: &[u8],
    _create_new: bool,
    _after_opened_component: impl FnMut(&Path),
) -> io::Result<()> {
    validated_relative_components(relative)?;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure source file writes are not supported on this platform",
    ))
}
