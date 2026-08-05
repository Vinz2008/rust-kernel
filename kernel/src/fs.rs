use core::{cmp, sync::atomic::{AtomicU64, Ordering}};

use alloc::{borrow::Cow, boxed::Box, collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use shared_consts::{DIRENT_DEVICE, DIRENT_DIR, DIRENT_FILE, DirChild, Fd, PATH_NAME_MAX, Stat, StatMode};
use spin::mutex::Mutex;

use crate::{device::{DeviceOps, STDOUT}, initrd::{INITRD_BYTES, TarInitrd}, process::OpenedFile, scheduler::with_scheduler_no_int, serial_println};
use lazy_static::lazy_static;

// TODO : file permissions (first need users, maybe root/admin user ? search about it)

pub fn process_open_file(path : &str, is_readable : bool, is_writable : bool, create_file : bool) -> Option<Fd> {
    with_scheduler_no_int(|scheduler|{
        let canonicalized_path = {
            let current_cwd = &scheduler.current_process.unwrap().get_process(&scheduler.processes).cwd_path;
            canonicalize_path(path, current_cwd)?
        };
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        
        let opened_file = OpenedFile::new(&canonicalized_path, is_readable, is_writable, create_file).ok()?;
        let fd = current_proc.add_opened_file(opened_file);
        Some(fd)
    })
}

pub fn process_close_file(fd : Fd) -> Option<()> {
    with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        current_proc.remove_opened_file(fd)
    })
}

pub fn process_get_dir_children(fd : Fd, out : &mut [DirChild]) -> Result<usize, FileError> {
    let opened_dir = with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process(&scheduler.processes);
        let opened_dir = current_proc.fd_list.get(fd.0).ok_or(FileError::FdNotFound)?.as_ref().cloned().ok_or(FileError::FdNotFound)?;
        Ok(opened_dir)
    })?;

    if !opened_dir.readable {
        return Err(FileError::NotReadableFile);
    }

    let mut offset_lock = opened_dir.offset.lock();
    let children_nb = opened_dir.inode.read_dir_children(*offset_lock, out)?;
    *offset_lock += children_nb;

    Ok(children_nb)
}

pub fn process_fstat(fd : Fd) -> Result<Stat, FileError> {
    let inode = with_scheduler_no_int(|scheduler|{
        let current_pid = scheduler.current_process.unwrap();
        let current_proc = current_pid.get_process(&scheduler.processes);
        let opened_file = current_proc.fd_list.get(fd.0).ok_or(FileError::FdNotFound)?.as_ref().ok_or(FileError::FdNotFound)?;
        Ok(opened_file.inode.clone())
    })?;
    file_stat_inode(&inode)
}

pub fn process_read(fd : Fd, buf : &mut [u8]) -> Result<usize, FileError> {
    let opened_file = with_scheduler_no_int(|scheduler|{
        let current_pid = scheduler.current_process.unwrap();
        let current_proc = current_pid.get_process(&scheduler.processes);
        let opened_file = current_proc.fd_list.get(fd.0).ok_or(FileError::FdNotFound)?.as_ref().cloned().ok_or(FileError::FdNotFound)?;
        Ok(opened_file)
    })?;
    if !opened_file.readable {
        return Err(FileError::NotReadableFile);
    }
    let mut offset_lock = opened_file.offset.lock();
    let read = opened_file.inode.read_at(*offset_lock, buf)?;
    *offset_lock += read;
    Ok(read)
}

pub fn process_write(fd : Fd, buf : &[u8]) -> Result<usize, FileError> {
    let opened_file = with_scheduler_no_int(|scheduler|{
        let current_pid = scheduler.current_process.unwrap();
        let current_proc = current_pid.get_process(&scheduler.processes);
        let opened_file = current_proc.fd_list.get(fd.0).ok_or(FileError::FdNotFound)?.as_ref().cloned().ok_or(FileError::FdNotFound)?;
        Ok(opened_file)
    })?;
    if !opened_file.writable {
        return Err(FileError::NotWritableFile);
    }
    let mut offset_lock = opened_file.offset.lock();
    let written = opened_file.inode.write_at(*offset_lock, buf)?;
    *offset_lock += written;
    Ok(written)
}

// TODO : if it uses a lot perf, use cow instead ?
// TODO : optimize performance ?
pub fn canonicalize_path(path : &str, cwd : &str) -> Option<String> {

    let mut components = Vec::new();
    if !path.starts_with('/'){
        for component in cwd.split('/'){
            match component {
                "" | "." => {}
                ".." => {
                    components.pop()?;
                },
                comp => components.push(comp),
            }
        }
    }

    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            name => components.push(name),
        }
    }

    let mut result = String::from("/");
    for (idx, &component) in components.iter().enumerate() {
        if idx != 0 {
            result.push('/');
        }
        result.push_str(component);
    }
    Some(result)
}

// TODO : make the children be not owned to be able to duplicate them in the tree ? (do I really need that ?), also would help with ownership by letting easily copy a filenode
// TODO : use trait instead, to abstract from where is the data (to replace the part with the content)

static NEXT_INODE_ID : AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub struct InodeIdx(u64);

fn next_inode_idx() -> InodeIdx {
    InodeIdx(NEXT_INODE_ID.fetch_add(1, Ordering::Relaxed))
}

pub struct Inode {
    idx : InodeIdx,
    kind : InodeKind,
}

pub enum FileData {
    Initrd(&'static [u8]),
    Memory(Mutex<Vec<u8>>), // TODO : test RwLock ?
}

impl FileData {
    fn len(&self) -> usize {
        match self {
            FileData::Initrd(buf) => buf.len(),
            FileData::Memory(buf) => buf.lock().len(),
        }
    }
}

pub enum InodeKind {
    Directory {
        entries : Mutex<BTreeMap<Box<str>, Arc<Inode>>>, // TODO : test Rwlock ? (more important than for others case, because should be able to traverse fs in concurrency, while reading concurrently is less important for now)
    },
    File {
        data: FileData
    },
    Device {
        device_ops : &'static dyn DeviceOps,
    }
}


#[derive(Debug)]
pub enum FileError {
    DirPathNotFound {
        dir_not_found : Box<str>,
        path : Box<str>,
    },
    DirExpected {
        file_should_be_dir : Option<Box<str>>,
        path : Box<str>,
    },
    FileExpected {
        path : Box<str>,
    },
    FileNotFound {
        path : Box<str>,
    },
    FileAlreadyExists {
        path: Box<str>,
    },
    InvalidPath {
        path: Box<str>,
    },
    FdNotFound,
    NotReadableFile,
    NotWritableFile,
}

//const EMPTY_CONTENT : &[u8] = &[];

// TODO : have a fd to not have to resolve path for each file operation

fn fix_error_with_path<T>(res : Result<T, FileError>, path : Box<str>) -> Result<T, FileError>{
    match res {
        Err(FileError::DirPathNotFound { dir_not_found, path: _ }) => Err(FileError::DirPathNotFound { dir_not_found, path }),
        Err(FileError::DirExpected { file_should_be_dir, path: _ }) => Err(FileError::DirExpected { file_should_be_dir, path }),
        Err(FileError::FileNotFound { path: _ }) => Err(FileError::FileNotFound { path }),
        Err(FileError::FileAlreadyExists { path: _ }) => Err(FileError::FileAlreadyExists { path }),
        f => f,
    }
}

impl Inode {
    fn new_dir() -> Arc<Inode> {
        Arc::new(Inode { 
            idx: next_inode_idx(), 
            kind: InodeKind::Directory { entries: Mutex::new(BTreeMap::new()) },
        })
    }

    fn new_initrd_file(data : &'static [u8]) -> Arc<Inode> {
        Arc::new(Inode { 
            idx: next_inode_idx(), 
            kind: InodeKind::File { data: FileData::Initrd(data) }, 
        })
    }

    pub fn new_mem_file() -> Arc<Inode> {
        Arc::new(Inode { 
            idx: next_inode_idx(), 
            kind: InodeKind::File { data: FileData::Memory(Mutex::new(Vec::new())) }, 
        })
    }

    fn new_device(device_ops : &'static dyn DeviceOps) -> Arc<Inode> {
        Arc::new(Inode {
            idx: next_inode_idx(),
            kind: InodeKind::Device { device_ops },
        })
    }

    fn is_dir(&self) -> bool {
        matches!(self.kind, InodeKind::Directory { .. })
    }

    fn read_dir_children(&self, start_offset : usize, out : &mut [DirChild]) -> Result<usize, FileError> {
        let entries_lock = match &self.kind {
            InodeKind::Directory { entries } => entries.lock(),
            InodeKind::File { .. } | InodeKind::Device { .. } => {
                return Err(FileError::DirExpected { file_should_be_dir: None, path: Box::default() }); // TODO : put the inode instead ?
            }
        };

        let mut written = 0;


        for (name, inode) in entries_lock.iter().skip(start_offset).take(out.len()){
            let kind = match inode.kind {
                InodeKind::Directory { .. } => DIRENT_DIR,
                InodeKind::File { .. } => DIRENT_FILE,
                InodeKind::Device { .. } => DIRENT_DEVICE,
            };

            let name_len = cmp::min(name.len(), PATH_NAME_MAX);

            let mut entry = DirChild {
                kind,
                name_len: name_len as u8,
                name: [0; PATH_NAME_MAX],
            };

            entry.name[..name_len].copy_from_slice(&name.as_bytes()[..name_len]);
            out[written] = entry;
            written += 1;
        }

        Ok(written)
    }

    pub fn initrd_content(&self) -> Option<&'static [u8]> {
        match &self.kind {
            InodeKind::File { data: FileData::Initrd(content) } => Some(*content),
            _ => None,
        }
    }

    fn stat(&self) -> Stat {
        let mode = match &self.kind {
            InodeKind::File { data } => StatMode::File {
                size: data.len(),
            },
            InodeKind::Directory { .. } => StatMode::Directory,
            InodeKind::Device { .. } => StatMode::Device,
        };
        Stat {
            mode
        }
    }

    fn read_at(&self, offset : usize, out : &mut [u8]) -> Result<usize, FileError> {
        match &self.kind {
            InodeKind::File { data } => match data {
                FileData::Initrd(data) => {
                    if offset >= data.len() {
                        return Ok(0);
                    }
                    let count = cmp::min(out.len(), data.len()-offset);
                    out[..count].copy_from_slice(&data[offset..offset+count]);
                    Ok(count)
                },
                FileData::Memory(data) => {
                    let data_lock = data.lock();
                    if offset >= data_lock.len(){
                        return Ok(0);
                    }
                    let count = cmp::min(out.len(), data_lock.len()-offset);
                    out[..count].copy_from_slice(&data_lock[offset..offset+count]);
                    Ok(count)
                },
            }, 
            InodeKind::Device { device_ops } => device_ops.read(offset, out),
            InodeKind::Directory { .. } => Err(FileError::FileExpected { path: Box::default() }),
        }
    }

    // TODO : use this (implement syscall)
    fn write_at(&self, offset : usize, input : &[u8]) -> Result<usize, FileError> {
        match &self.kind {
            InodeKind::File { data } => match data {
                FileData::Initrd(_) => {
                    Err(FileError::NotWritableFile)
                },
                FileData::Memory(data) => {
                    let mut data_lock = data.lock();

                    if offset > data_lock.len() {
                        // fill hole if offset is after the end of file
                        data_lock.resize(offset, 0);
                    }

                    let overwrite_len = input.len().min(data_lock.len()-offset);

                    data_lock[offset..offset+overwrite_len].copy_from_slice(&input[..overwrite_len]);
                    data_lock.extend_from_slice(&input[overwrite_len..]);

                    Ok(input.len())
                },
            }, 
            InodeKind::Device { device_ops } => device_ops.write(offset, input),
            InodeKind::Directory { .. } => Err(FileError::FileExpected { path: Box::default() }),
        }
    }

    pub fn read_entire_file_in_mem(&self) -> Result<Cow<'_, [u8]>, FileError> {
        if let Some(content) = self.initrd_content() {
            return Ok(Cow::Borrowed(content));
        }
        let size = match self.stat().mode {
            StatMode::File { size } => size,
            _ => return Err(FileError::FileExpected { path: Box::default() }), // TODO : put Inode instead ?
        };
        let mut content = Vec::with_capacity(size);
        let read_amount = self.read_at(0, &mut content)?; // TODO : check wrote ? or retry if not everything read ?
        Ok(Cow::Owned(content))
    }
}


fn path_components(path : &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|part| !part.is_empty())
}

fn find_inode_from(start : Arc<Inode>, path : &str) -> Result<Arc<Inode>, FileError> {
    let mut current = start;

    for component in path_components(path){
        current = match &current.kind {
            InodeKind::Directory { entries } => {
                entries.lock().get(component).cloned().ok_or_else(||{
                    FileError::FileNotFound { path: Box::from(path) }
                })?
            },
            InodeKind::Device { .. } | InodeKind::File { .. } => return Err(FileError::DirExpected { file_should_be_dir: Some(Box::from(component)), path: Box::from(path) }),
        };
    }

    Ok(current)
}

pub fn get_inode(path : &str) -> Result<Arc<Inode>, FileError> {
    find_inode_from(ROOT_NODE.clone(), path)
}

fn add_child_to_inode_dir(parent : &Arc<Inode>, name : &str, child : Arc<Inode>) -> Result<(), FileError> {
    if name.is_empty() || name.contains('/') {
        return Err(FileError::InvalidPath { path: Box::from(name) });
    }

    let entries = match &parent.kind {
        InodeKind::Directory { entries } => entries,
        InodeKind::File { .. } | InodeKind::Device { .. } => return Err(FileError::DirExpected { file_should_be_dir: Some(Box::from(name)), path: Box::from("") })
    };

    let mut entries_lock = entries.lock();

    if entries_lock.contains_key(name){
        return Err(FileError::FileAlreadyExists { path: Box::from(name) });
    }

    entries_lock.insert(Box::from(name), child);
    Ok(())
}

fn add_inode_to_vfs_tree(root : Arc<Inode>, path : &str, node : Arc<Inode>, create_parents : bool) -> Result<(), FileError> {
    let components = path_components(path).collect::<Vec<_>>();
    let (name, parents) = match components.split_last(){
        Some((name, parents)) => (*name, parents),
        None => return Err(FileError::InvalidPath { path: Box::from(path) }),
    };

    let mut current = root;

    for &component in parents {
        current = match &current.kind {
            InodeKind::Directory { entries } => {
                let mut entries_lock = entries.lock();
                let existing = entries_lock.get(component).cloned();
                match existing {
                    Some(inode) => {
                        if !inode.is_dir() {
                            return Err(FileError::DirExpected { file_should_be_dir: Some(Box::from(component)), path: Box::from(path) });
                        }
                        inode
                    },
                    None => {
                        let component_str = Box::from(component); 
                        if create_parents {
                            let dir = Inode::new_dir();
                            entries_lock.insert(component_str, dir.clone());
                            dir
                        } else {
                            return Err(FileError::DirPathNotFound { dir_not_found: component_str, path: Box::from(path) });
                        }
                    }
                }
            }
            InodeKind::File { .. } | InodeKind::Device { .. } => {
                return Err(FileError::DirExpected {
                    file_should_be_dir: None,
                    path: Box::from(path),
                });
            }
        };
    }

    let res = add_child_to_inode_dir(&current, name, node);
    fix_error_with_path(res, Box::from(path))
}


pub fn file_stat_inode(inode : &Arc<Inode>) -> Result<Stat, FileError> {
    let stat = inode.stat();
    Ok(stat)
}

pub fn file_stat(path : &str) -> Result<Stat, FileError> {
    serial_println!("file stat on {}", path);
    let file_node = get_inode(path)?;
    let stat = file_stat_inode(&file_node)?;

    Ok(stat)
}

lazy_static! {
    pub static ref ROOT_NODE : Arc<Inode> = {
        let tar_initrd = TarInitrd::new(INITRD_BYTES).expect("invalid tar");
        let root_node = fs_create_root_node(tar_initrd);
        root_node
    };
}

pub fn add_inode(path : &str, inode : Arc<Inode>) -> Result<(), FileError> {
    add_inode_to_vfs_tree(ROOT_NODE.clone(), path, inode, false)
}

// TODO : better error handling ? (need to use once for the root node ?)
fn fs_create_root_node(tar_initrd : TarInitrd<'static>) -> Arc<Inode> {
    let root_node = Inode::new_dir();
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap().as_ref(), file.size().unwrap());
    }

    serial_println!("TEST");
    
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap().as_ref(), file.size().unwrap());
        let path = &file.get_filename().unwrap()[1..];
        serial_println!("path : {}", path);
        if path != "/" {
            let inode = match file.get_typeflag(){
                b'0' | 0 => Inode::new_initrd_file(file.content().unwrap()),
                b'5' => Inode::new_dir(),
                typeflag => panic!("unsupported tag in initrd : {:?}", typeflag),
            };
            add_inode_to_vfs_tree(root_node.clone(), path, inode, true).unwrap(); // TODO : better error handling ?
        }
    }

    add_inode_to_vfs_tree(root_node.clone(), "/dev", Inode::new_dir(), false).unwrap();
    add_inode_to_vfs_tree(root_node.clone(), "/dev/stdout", Inode::new_device(&*STDOUT), false).unwrap();
    
    root_node
}