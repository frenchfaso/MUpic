//! Small in-memory filesystem with std-like file/seek semantics. Compiled on
//! native hosts for tests, and used in production only on bare browser WASM.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

fn error(kind: ErrorKind, message: &str) -> io::Error { io::Error::new(kind, message) }
fn lock<T>(mutex: &Mutex<T>) -> io::Result<MutexGuard<'_, T>> {
    mutex.lock().map_err(|_| error(ErrorKind::Other, "virtual file lock poisoned"))
}

#[derive(Debug)]
struct Data { bytes: Vec<u8>, readonly: bool }

#[derive(Clone, Debug)]
enum Node {
    Directory { readonly: bool },
    File(Arc<Mutex<Data>>),
}

impl Node {
    fn metadata(&self) -> io::Result<Metadata> {
        Ok(match self {
            Self::Directory { readonly } => Metadata { len: 0, directory: true, permissions: Permissions { readonly: *readonly } },
            Self::File(data) => {
                let data = lock(data)?;
                Metadata { len: data.bytes.len() as u64, directory: false, permissions: Permissions { readonly: data.readonly } }
            }
        })
    }
}

struct Filesystem { nodes: BTreeMap<PathBuf, Node> }
impl Filesystem {
    fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(PathBuf::from("/"), Node::Directory { readonly: false });
        Self { nodes }
    }
    fn node(&self, path: &Path) -> io::Result<&Node> {
        self.nodes.get(path).ok_or_else(|| self.missing_path(path))
    }
    fn missing_path(&self, path: &Path) -> io::Error {
        if path.ancestors().skip(1).any(|parent| matches!(self.nodes.get(parent), Some(Node::File(_)))) {
            error(ErrorKind::NotADirectory, "virtual path traverses a file")
        } else {
            error(ErrorKind::NotFound, "virtual path not found")
        }
    }
    fn writable_parent(&self, path: &Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| error(ErrorKind::PermissionDenied, "cannot replace filesystem root"))?;
        match self.node(parent)? {
            Node::Directory { readonly: false } => Ok(()),
            Node::Directory { readonly: true } => Err(error(ErrorKind::PermissionDenied, "parent directory is read-only")),
            Node::File(_) => Err(error(ErrorKind::NotADirectory, "parent is not a directory")),
        }
    }
}

thread_local! { static FS: RefCell<Filesystem> = RefCell::new(Filesystem::new()); }

/// Relative paths resolve from /. Lexical components are normalized because the
/// VFS has no symlinks; non-UTF8 host paths are not accepted by the browser ABI.
fn path<P: AsRef<Path>>(input: P) -> io::Result<PathBuf> {
    let input = input.as_ref();
    if input.as_os_str().is_empty() || input.to_str().is_none() || input.to_str().is_some_and(|s| s.contains('\0')) {
        return Err(error(ErrorKind::InvalidInput, "invalid virtual path"));
    }
    let mut out = PathBuf::from("/");
    for part in input.components() {
        match part {
            Component::RootDir => out = PathBuf::from("/"),
            Component::CurDir => {},
            Component::ParentDir => { if out != Path::new("/") { out.pop(); } },
            Component::Normal(name) => out.push(name),
            Component::Prefix(_) => return Err(error(ErrorKind::InvalidInput, "unsupported virtual path prefix")),
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct Permissions { readonly: bool }
impl Permissions {
    pub fn readonly(&self) -> bool { self.readonly }
    pub fn set_readonly(&mut self, readonly: bool) { self.readonly = readonly; }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileType { directory: bool }
impl FileType {
    pub fn is_dir(&self) -> bool { self.directory }
    pub fn is_file(&self) -> bool { !self.directory }
    pub fn is_symlink(&self) -> bool { false }
}

#[derive(Clone, Debug)]
pub struct Metadata { len: u64, directory: bool, permissions: Permissions }
impl Metadata {
    pub fn len(&self) -> u64 { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
    pub fn is_dir(&self) -> bool { self.directory }
    pub fn is_file(&self) -> bool { !self.directory }
    pub fn file_type(&self) -> FileType { FileType { directory: self.directory } }
    pub fn permissions(&self) -> Permissions { self.permissions.clone() }
}

#[derive(Debug)]
pub struct File {
    data: Arc<Mutex<Data>>,
    // Duplicated handles share the offset, separate opens have independent ones.
    position: Arc<Mutex<u64>>,
    readable: bool,
    writable: bool,
    append: bool,
}

impl File {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> { OpenOptions::new().read(true).open(path) }
    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<Self> { OpenOptions::new().write(true).create(true).truncate(true).open(path) }
    pub fn options() -> OpenOptions { OpenOptions::new() }
    pub fn metadata(&self) -> io::Result<Metadata> { Node::File(self.data.clone()).metadata() }
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self { data: self.data.clone(), position: self.position.clone(), readable: self.readable, writable: self.writable, append: self.append })
    }
    pub fn set_len(&self, size: u64) -> io::Result<()> {
        if !self.writable { return Err(error(ErrorKind::PermissionDenied, "file not opened for writing")); }
        let size = usize::try_from(size).map_err(|_| error(ErrorKind::InvalidInput, "virtual file length exceeds address space"))?;
        let mut data = lock(&self.data)?;
        reserve(&mut data.bytes, size)?;
        data.bytes.resize(size, 0);
        Ok(())
    }
    pub fn set_permissions(&self, permissions: Permissions) -> io::Result<()> { lock(&self.data)?.readonly = permissions.readonly; Ok(()) }
    pub fn sync_all(&self) -> io::Result<()> { Ok(()) }
    pub fn sync_data(&self) -> io::Result<()> { Ok(()) }
}

fn reserve(bytes: &mut Vec<u8>, size: usize) -> io::Result<()> {
    if size > bytes.len() {
        bytes.try_reserve(size - bytes.len()).map_err(|_| error(ErrorKind::OutOfMemory, "virtual file allocation failed"))?;
    }
    Ok(())
}

impl Read for File {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.readable { return Err(error(ErrorKind::PermissionDenied, "file not opened for reading")); }
        let mut position = lock(&self.position)?;
        let data = lock(&self.data)?;
        let start = usize::try_from(*position).unwrap_or(usize::MAX).min(data.bytes.len());
        let len = buffer.len().min(data.bytes.len() - start);
        buffer[..len].copy_from_slice(&data.bytes[start..start + len]);
        *position += len as u64;
        Ok(len)
    }
}

impl Write for File {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.writable { return Err(error(ErrorKind::PermissionDenied, "file not opened for writing")); }
        let mut position = lock(&self.position)?;
        let mut data = lock(&self.data)?;
        if bytes.is_empty() { return Ok(0); }
        let at = if self.append { data.bytes.len() } else {
            usize::try_from(*position).map_err(|_| error(ErrorKind::InvalidInput, "virtual file offset exceeds address space"))?
        };
        let end = at.checked_add(bytes.len()).ok_or_else(|| error(ErrorKind::InvalidInput, "virtual file write overflows"))?;
        reserve(&mut data.bytes, end)?;
        if end > data.bytes.len() { data.bytes.resize(end, 0); }
        data.bytes[at..end].copy_from_slice(bytes);
        *position = end as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

impl Seek for File {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let mut position = lock(&self.position)?;
        let value = match from {
            SeekFrom::Start(value) => Some(value),
            SeekFrom::Current(offset) => position.checked_add_signed(offset),
            SeekFrom::End(offset) => (lock(&self.data)?.bytes.len() as u64).checked_add_signed(offset),
        };
        // SeekFrom::Start has no signed addition but follows the same validation.
        *position = value.ok_or_else(|| error(ErrorKind::InvalidInput, "seek before file start or beyond u64"))?;
        Ok(*position)
    }
}

#[derive(Clone, Debug, Default)]
pub struct OpenOptions { read: bool, write: bool, append: bool, truncate: bool, create: bool, create_new: bool }
impl OpenOptions {
    pub fn new() -> Self { Self::default() }
    pub fn read(&mut self, value: bool) -> &mut Self { self.read = value; self }
    pub fn write(&mut self, value: bool) -> &mut Self { self.write = value; self }
    pub fn append(&mut self, value: bool) -> &mut Self { self.append = value; self }
    pub fn truncate(&mut self, value: bool) -> &mut Self { self.truncate = value; self }
    pub fn create(&mut self, value: bool) -> &mut Self { self.create = value; self }
    pub fn create_new(&mut self, value: bool) -> &mut Self { self.create_new = value; self }
    pub fn open<P: AsRef<Path>>(&self, input: P) -> io::Result<File> {
        let writable = self.write || self.append;
        if (!self.read && !writable) || ((self.truncate || self.create || self.create_new) && !writable) {
            return Err(error(ErrorKind::InvalidInput, "invalid file access options"));
        }
        let path = path(input)?;
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            let data = match fs.nodes.get(&path) {
                Some(_) if self.create_new => return Err(error(ErrorKind::AlreadyExists, "virtual path already exists")),
                Some(Node::Directory { .. }) => return Err(error(ErrorKind::IsADirectory, "cannot open directory as a file")),
                Some(Node::File(data)) => data.clone(),
                None if self.create || self.create_new => {
                    fs.writable_parent(&path)?;
                    let data = Arc::new(Mutex::new(Data { bytes: Vec::new(), readonly: false }));
                    fs.nodes.insert(path, Node::File(data.clone()));
                    data
                },
                None => return Err(fs.missing_path(&path)),
            };
            if writable {
                let mut data = lock(&data)?;
                if data.readonly { return Err(error(ErrorKind::PermissionDenied, "virtual file is read-only")); }
                if self.truncate { data.bytes.clear(); }
            }
            Ok(File { data, position: Arc::new(Mutex::new(0)), readable: self.read, writable, append: self.append })
        })
    }
}

pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let data = lock(&file.data)?;
    let mut result = Vec::new();
    reserve(&mut result, data.bytes.len())?;
    result.extend_from_slice(&data.bytes);
    Ok(result)
}
pub fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|_| error(ErrorKind::InvalidData, "file is not UTF-8"))
}
pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, bytes: C) -> io::Result<()> { File::create(path)?.write_all(bytes.as_ref()) }
pub fn register<P: AsRef<Path>>(input: P, bytes: &[u8]) -> io::Result<()> {
    let path = path(input)?;
    if let Some(parent) = path.parent() { create_dir_all(parent)?; }
    write(path, bytes)
}
pub fn metadata<P: AsRef<Path>>(input: P) -> io::Result<Metadata> {
    let path = path(input)?;
    FS.with(|fs| fs.borrow().node(&path)?.metadata())
}
pub fn symlink_metadata<P: AsRef<Path>>(input: P) -> io::Result<Metadata> { metadata(input) }
pub fn canonicalize<P: AsRef<Path>>(input: P) -> io::Result<PathBuf> {
    let path = path(input)?;
    FS.with(|fs| { fs.borrow().node(&path)?; Ok(path) })
}
pub fn exists<P: AsRef<Path>>(input: P) -> io::Result<bool> {
    match metadata(input) { Ok(_) => Ok(true), Err(e) if e.kind() == ErrorKind::NotFound => Ok(false), Err(e) => Err(e) }
}
pub fn set_permissions<P: AsRef<Path>>(input: P, permissions: Permissions) -> io::Result<()> {
    let path = path(input)?;
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        match fs.nodes.get_mut(&path).ok_or_else(|| error(ErrorKind::NotFound, "virtual path not found"))? {
            Node::Directory { readonly } => *readonly = permissions.readonly,
            Node::File(data) => lock(data)?.readonly = permissions.readonly,
        }
        Ok(())
    })
}
pub fn create_dir<P: AsRef<Path>>(input: P) -> io::Result<()> {
    let path = path(input)?;
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        if fs.nodes.contains_key(&path) { return Err(error(ErrorKind::AlreadyExists, "virtual path already exists")); }
        fs.writable_parent(&path)?;
        fs.nodes.insert(path, Node::Directory { readonly: false });
        Ok(())
    })
}
pub fn create_dir_all<P: AsRef<Path>>(input: P) -> io::Result<()> {
    let path = path(input)?;
    let mut prefix = PathBuf::from("/");
    for component in path.components().skip(1) {
        prefix.push(component.as_os_str());
        match create_dir(&prefix) {
            Ok(()) => {},
            Err(e) if e.kind() == ErrorKind::AlreadyExists && metadata(&prefix)?.is_dir() => {},
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct DirEntry { path: PathBuf }
impl DirEntry {
    pub fn path(&self) -> PathBuf { self.path.clone() }
    pub fn file_name(&self) -> OsString { self.path.file_name().unwrap_or_default().to_os_string() }
    pub fn metadata(&self) -> io::Result<Metadata> { metadata(&self.path) }
    pub fn file_type(&self) -> io::Result<FileType> { Ok(self.metadata()?.file_type()) }
}
#[derive(Debug)]
pub struct ReadDir { entries: std::vec::IntoIter<DirEntry> }
impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;
    fn next(&mut self) -> Option<Self::Item> { self.entries.next().map(Ok) }
}
pub fn read_dir<P: AsRef<Path>>(input: P) -> io::Result<ReadDir> {
    let path = path(input)?;
    FS.with(|fs| {
        let fs = fs.borrow();
        if !fs.node(&path)?.metadata()?.is_dir() { return Err(error(ErrorKind::NotADirectory, "virtual path is not a directory")); }
        let entries = fs.nodes.keys().filter(|candidate| candidate.parent() == Some(path.as_path()))
            .map(|path| DirEntry { path: path.clone() }).collect::<Vec<_>>();
        Ok(ReadDir { entries: entries.into_iter() })
    })
}
pub fn remove_file<P: AsRef<Path>>(input: P) -> io::Result<()> {
    let path = path(input)?;
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        if !fs.node(&path)?.metadata()?.is_file() { return Err(error(ErrorKind::IsADirectory, "virtual path is a directory")); }
        fs.writable_parent(&path)?;
        fs.nodes.remove(&path);
        Ok(())
    })
}
pub fn remove_dir<P: AsRef<Path>>(input: P) -> io::Result<()> {
    let path = path(input)?;
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        if !fs.node(&path)?.metadata()?.is_dir() { return Err(error(ErrorKind::NotADirectory, "virtual path is not a directory")); }
        fs.writable_parent(&path)?;
        if fs.nodes.keys().any(|p| p.parent() == Some(path.as_path())) { return Err(error(ErrorKind::DirectoryNotEmpty, "virtual directory not empty")); }
        fs.nodes.remove(&path);
        Ok(())
    })
}
pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
    let from = path(from)?;
    let to = path(to)?;
    FS.with(|fs| {
        let mut fs = fs.borrow_mut();
        let source_dir = fs.node(&from)?.metadata()?.is_dir();
        if from == to { return Ok(()); }
        fs.writable_parent(&from)?;
        fs.writable_parent(&to)?;
        if source_dir && to.starts_with(&from) { return Err(error(ErrorKind::InvalidInput, "cannot move directory into itself")); }
        if let Some(existing) = fs.nodes.get(&to) {
            let target_dir = existing.metadata()?.is_dir();
            if source_dir != target_dir {
                return Err(error(if target_dir { ErrorKind::IsADirectory } else { ErrorKind::NotADirectory }, "rename changes file type"));
            }
            if target_dir && fs.nodes.keys().any(|p| p.parent() == Some(to.as_path())) {
                return Err(error(ErrorKind::DirectoryNotEmpty, "rename target directory not empty"));
            }
        }
        let moved = fs.nodes.keys().filter(|p| *p == &from || (source_dir && p.starts_with(&from))).cloned().collect::<Vec<_>>();
        fs.nodes.remove(&to);
        for old in moved {
            let new = if old == from { to.clone() } else { to.join(old.strip_prefix(&from).unwrap()) };
            let node = fs.nodes.remove(&old).unwrap();
            fs.nodes.insert(new, node);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() { FS.with(|fs| *fs.borrow_mut() = Filesystem::new()); }

    #[test]
    fn registered_assets_have_real_directory_and_utf8_semantics() {
        fresh();
        register("roms/./nested/../basic.bin", &[0x00, 0xff, 0x42]).unwrap();
        register("/roms/name.txt", b"C64\n").unwrap();
        assert_eq!(read("/roms/basic.bin").unwrap(), [0x00, 0xff, 0x42]);
        assert_eq!(canonicalize("roms/../roms/basic.bin").unwrap(), PathBuf::from("/roms/basic.bin"));
        assert!(metadata("/roms").unwrap().is_dir());
        assert_eq!(metadata("roms/basic.bin").unwrap().len(), 3);
        let names = read_dir("/roms").unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>();
        assert_eq!(names, [OsString::from("basic.bin"), OsString::from("name.txt")]);
        assert_eq!(read_to_string("roms/name.txt").unwrap(), "C64\n");
        assert_eq!(read_to_string("roms/basic.bin").unwrap_err().kind(), ErrorKind::InvalidData);
        assert_eq!(read_dir("roms/name.txt").unwrap_err().kind(), ErrorKind::NotADirectory);
        assert_eq!(read("missing").unwrap_err().kind(), ErrorKind::NotFound);
        assert!(!exists("missing").unwrap());
        assert_eq!(register("bad\0name", b"").unwrap_err().kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn flash_padding_and_dirty_sector_writeback_survive_reopening() {
        fresh();
        register("/flash.bin", &[0x12, 0x34]).unwrap();
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open("/flash.bin").unwrap();
        let mut image = vec![0xff; 8192];
        file.read_exact(&mut image[..2]).unwrap();
        file.write_all(&image[2..]).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 8192);
        image[4096..4100].copy_from_slice(&[1, 2, 3, 4]);
        file.seek(SeekFrom::Start(4096)).unwrap();
        file.write_all(&image[4096..8192]).unwrap();
        file.flush().unwrap();
        drop(file);
        assert_eq!(read("/flash.bin").unwrap(), image);
        assert_eq!(metadata("/flash.bin").unwrap().len(), 8192);
    }

    #[test]
    fn readonly_images_reject_rw_open_and_keep_existing_contents() {
        fresh();
        register("/sd.img", &[1, 2, 3]).unwrap();
        let mut writable = OpenOptions::new().write(true).open("/sd.img").unwrap();
        let mut permissions = metadata("/sd.img").unwrap().permissions();
        permissions.set_readonly(true);
        set_permissions("/sd.img", permissions.clone()).unwrap();
        assert_eq!(OpenOptions::new().read(true).write(true).open("/sd.img").unwrap_err().kind(), ErrorKind::PermissionDenied);
        assert_eq!(File::create("/sd.img").unwrap_err().kind(), ErrorKind::PermissionDenied);
        // Changing path permissions affects future opens, not an already-open fd.
        writable.write_all(&[9]).unwrap();
        let mut ro = File::open("/sd.img").unwrap();
        let mut bytes = [0; 3];
        ro.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [9, 2, 3]);
        assert_eq!(ro.write(&[9]).unwrap_err().kind(), ErrorKind::PermissionDenied);
        assert_eq!(ro.set_len(0).unwrap_err().kind(), ErrorKind::PermissionDenied);
        permissions.set_readonly(false);
        set_permissions("/sd.img", permissions).unwrap();
        File::create("/sd.img").unwrap();
        assert!(read("/sd.img").unwrap().is_empty());
    }

    #[test]
    fn seek_eof_sparse_writes_and_set_len_preserve_cursor() {
        fresh();
        register("/data", b"abc").unwrap();
        let mut file = OpenOptions::new().read(true).write(true).open("/data").unwrap();
        assert_eq!(file.seek(SeekFrom::End(-1)).unwrap(), 2);
        let mut b = [0];
        file.read_exact(&mut b).unwrap();
        assert_eq!(b, [b'c']);
        assert_eq!(file.read(&mut b).unwrap(), 0);
        assert_eq!(file.seek(SeekFrom::Current(-4)).unwrap_err().kind(), ErrorKind::InvalidInput);
        assert_eq!(file.stream_position().unwrap(), 3);
        file.seek(SeekFrom::End(2)).unwrap();
        file.write_all(b"z").unwrap();
        assert_eq!(read("/data").unwrap(), b"abc\0\0z");
        file.set_len(2).unwrap();
        assert_eq!(file.stream_position().unwrap(), 6);
        file.set_len(4).unwrap();
        assert_eq!(read("/data").unwrap(), b"ab\0\0");
        file.seek(SeekFrom::Start(u64::MAX)).unwrap();
        assert_eq!(file.read(&mut b).unwrap(), 0);
        assert_eq!(file.write(&[]).unwrap(), 0);
        assert!(file.write(b"x").is_err());
        assert_eq!(read("/data").unwrap(), b"ab\0\0");
    }

    #[test]
    fn clone_append_and_unlinked_handles_follow_inode_semantics() {
        fresh();
        register("/first", b"abcd").unwrap();
        let mut file = File::open("/first").unwrap();
        let mut clone = file.try_clone().unwrap();
        let mut separate = File::open("/first").unwrap();
        let mut b = [0];
        file.read_exact(&mut b).unwrap();
        clone.read_exact(&mut b).unwrap();
        assert_eq!(b, [b'b']);
        separate.read_exact(&mut b).unwrap();
        assert_eq!(b, [b'a']);
        let mut append = OpenOptions::new().append(true).open("/first").unwrap();
        append.seek(SeekFrom::Start(0)).unwrap();
        append.write_all(b"e").unwrap();
        assert_eq!(read("/first").unwrap(), b"abcde");
        rename("/first", "/second").unwrap();
        assert_eq!(clone.metadata().unwrap().len(), 5);
        remove_file("/second").unwrap();
        assert_eq!(read("/second").unwrap_err().kind(), ErrorKind::NotFound);
        file.read_exact(&mut b).unwrap();
        assert_eq!(b, [b'c']);
    }

    #[test]
    fn open_options_report_creation_and_access_errors() {
        fresh();
        assert_eq!(OpenOptions::new().open("/file").unwrap_err().kind(), ErrorKind::InvalidInput);
        assert_eq!(OpenOptions::new().read(true).truncate(true).open("/file").unwrap_err().kind(), ErrorKind::InvalidInput);
        assert_eq!(File::create("/absent/file").unwrap_err().kind(), ErrorKind::NotFound);
        File::create("/file").unwrap().write_all(b"old").unwrap();
        assert_eq!(OpenOptions::new().write(true).create_new(true).open("/file").unwrap_err().kind(), ErrorKind::AlreadyExists);
        assert_eq!(read("/file").unwrap(), b"old");
        assert_eq!(File::open("/").unwrap_err().kind(), ErrorKind::IsADirectory);
        assert_eq!(File::create("/file/child").unwrap_err().kind(), ErrorKind::NotADirectory);
        assert_eq!(File::open("/file/child").unwrap_err().kind(), ErrorKind::NotADirectory);
        assert_eq!(metadata("/file/child").unwrap_err().kind(), ErrorKind::NotADirectory);
        let mut write_only = OpenOptions::new().write(true).open("/file").unwrap();
        assert_eq!(write_only.read(&mut [0]).unwrap_err().kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn directory_rename_is_recursive_and_invalid_moves_are_atomic() {
        fresh();
        register("/old/nested/file", b"saved").unwrap();
        create_dir_all("/old/nested").unwrap();
        let opened = File::open("/old/nested/file").unwrap();
        assert_eq!(remove_dir("/old").unwrap_err().kind(), ErrorKind::DirectoryNotEmpty);
        assert_eq!(rename("/old", "/old/inside").unwrap_err().kind(), ErrorKind::InvalidInput);
        assert_eq!(read("/old/nested/file").unwrap(), b"saved");
        rename("/old", "/new").unwrap();
        assert_eq!(read("/new/nested/file").unwrap(), b"saved");
        assert_eq!(opened.metadata().unwrap().len(), 5);
        assert!(!exists("/old").unwrap());
        assert_eq!(read_dir("/new").unwrap().next().unwrap().unwrap().file_name(), "nested");
        remove_file("/new/nested/file").unwrap();
        remove_dir("/new/nested").unwrap();
        remove_dir("/new").unwrap();
        assert!(read_dir("/").unwrap().next().is_none());
    }

    #[test]
    fn re_registering_an_asset_updates_existing_handles_and_truncates() {
        fresh();
        register("/load.prg", b"old content").unwrap();
        let mut handle = File::open("/load.prg").unwrap();
        handle.seek(SeekFrom::Start(1)).unwrap();
        register("/load.prg", b"new").unwrap();
        let mut tail = Vec::new();
        handle.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, b"ew");
        assert_eq!(metadata("/load.prg").unwrap().len(), 3);
    }

    #[test]
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn random_access_operations_match_native_files_byte_for_byte() {
        fresh();
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ue2-vfs-{}-{index}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        struct Temp(PathBuf);
        impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let _temp = Temp(root.clone());
        let actual_path = root.join("image");
        let mut actual = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&actual_path).unwrap();
        let mut virtual_file = OpenOptions::new().read(true).write(true).create_new(true).open("/image").unwrap();
        let mut seed = 0x12345678_u32;
        for step in 0..240 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            match step % 6 {
                0 => {
                    let at = u64::from(seed % 4096);
                    assert_eq!(actual.seek(SeekFrom::Start(at)).unwrap(), virtual_file.seek(SeekFrom::Start(at)).unwrap());
                }
                1 => {
                    let bytes = seed.to_le_bytes().repeat((seed as usize & 7) + 1);
                    actual.write_all(&bytes).unwrap();
                    virtual_file.write_all(&bytes).unwrap();
                }
                2 => {
                    let mut a = [0; 24]; let mut b = [0; 24];
                    assert_eq!(actual.read(&mut a).unwrap(), virtual_file.read(&mut b).unwrap());
                    assert_eq!(a, b);
                }
                3 | 5 => {
                    let offset = i64::from(seed % 65) - 32;
                    let from = if step % 6 == 3 { SeekFrom::End(offset) } else { SeekFrom::Current(offset) };
                    assert_eq!(actual.seek(from).map_err(|e| e.kind()), virtual_file.seek(from).map_err(|e| e.kind()));
                }
                _ => {
                    let size = u64::from(seed % 4096);
                    actual.set_len(size).unwrap();
                    virtual_file.set_len(size).unwrap();
                }
            }
            assert_eq!(actual.stream_position().unwrap(), virtual_file.stream_position().unwrap(), "cursor after operation {step}");
            assert_eq!(std::fs::read(&actual_path).unwrap(), read("/image").unwrap(), "contents after operation {step}");
        }
    }
}
