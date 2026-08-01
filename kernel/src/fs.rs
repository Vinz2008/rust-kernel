use core::{cmp, sync::atomic::{AtomicU64, Ordering}};

use alloc::{borrow::Cow, boxed::Box, collections::BTreeMap, string::{String, ToString}, sync::Arc, vec::Vec};
use shared_consts::{DIRENT_DEVICE, DIRENT_DIR, DIRENT_FILE, DirChild, Fd, PATH_NAME_MAX, Stat, StatMode};
use spin::mutex::Mutex;

use crate::{device::DeviceOps, initrd::{INITRD_BYTES, TarInitrd}, process::OpenedFile, scheduler::with_scheduler_no_int, serial_println};
use lazy_static::lazy_static;

pub fn process_open_file(path : &str, is_readable : bool, is_writable : bool) -> Option<Fd> {
    with_scheduler_no_int(|scheduler|{
        let canonicalized_path = {
            let current_cwd = &scheduler.current_process.unwrap().get_process(&scheduler.processes).cwd_path;
            canonicalize_path(path, current_cwd)?
        };
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        let fd = current_proc.fd_list.len();
        let opened_file = OpenedFile::new(&canonicalized_path, is_readable, is_writable).ok()?;
        current_proc.fd_list.push(Some(opened_file));
        Some(Fd(fd))
    })
}

pub fn process_close_file(fd : Fd) -> Option<()> {
    with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        let idx = fd.0;
        current_proc.fd_list.get_mut(idx)?.take();
        Some(())
    })
}

pub fn process_get_dir_children(fd : Fd, out : &mut [DirChild]) -> Result<usize, FileError> {
    let (inode, offset) = with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process(&scheduler.processes);
        let opened_dir = current_proc.fd_list.get(fd.0).ok_or(FileError::FdNotFound)?.as_ref().unwrap();
        let path = opened_dir.inode.clone();
        let offset = opened_dir.offset;
        Ok((path, offset))
    })?;

    let children_nb = inode.read_dir_children(offset, out)?;

    with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        let opened_dir = current_proc.fd_list.get_mut(fd.0).ok_or(FileError::FdNotFound)?.as_mut().unwrap();
        opened_dir.offset += children_nb;
        Ok(())
    })?;

    Ok(children_nb)
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



/*pub enum FileContent<'a> {
    Directory {
        children : Vec<FileNode<'a>>,
    },
    File {
        content : &'a [u8], // TODO : replace with vec ? Cow ?
    },
    Device {
        device_ops : &'static dyn DeviceOps,
    }
}

pub struct FileNode<'a> {
    name : String, // TODO : replace these with Box<str> to lower memory usage ?
    content : FileContent<'a>,
}*/

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
}

const EMPTY_CONTENT : &[u8] = &[];

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

    fn new_mem_file() -> Arc<Inode> {
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
            InodeKind::Device { .. } => todo!(), // TODO : read a device
            InodeKind::Directory { .. } => return Err(FileError::FileExpected { path: Box::default() }),
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

/*impl<'a> FileNode<'a> {
    fn _create_node<'b>(&mut self, current_part : &'b str, mut rest_path : impl Iterator<Item = &'b str>, content : FileContent<'a>, create_parents : bool) -> Result<(), FileError>{
        let children = match &mut self.content {
            FileContent::Directory { children } => children,
            FileContent::File { .. } | FileContent::Device { .. } => return Err(FileError::DirExpected { file_should_be_dir: Some(Box::from(self.name.as_str())), path: Box::default() }),
        };
        
        match rest_path.next(){
            Some(next_part) => {
                let child = match children.iter_mut().find(|f| f.name == current_part) {
                    Some(c) => c,
                    None => {
                        if create_parents {
                            let next_idx = children.len();
                            children.push(FileNode::new_dir(current_part.to_string()));
                            &mut children[next_idx]
                        } else {
                            return Err(FileError::DirPathNotFound { dir_not_found: Box::from(current_part), path: Box::default() })
                        }
                    },
                };
                child._create_node(next_part, rest_path, content, create_parents)?;
            },
            None => {
                let new_file_name = current_part.to_string();
                let new_file = match content {
                    FileContent::Directory { children } => FileNode::new_dir_with_children(new_file_name, children),
                    FileContent::File { content } => FileNode::new_file_with_content(new_file_name, content),
                    FileContent::Device { device_ops } => FileNode::new_device(new_file_name, device_ops),
                };
                if children.iter().find(|f| f.name == current_part).is_some() {
                    return Err(FileError::FileAlreadyExists { path: Box::default() });
                }
                children.push(new_file);
            }
        }
        
        Ok(())
    }

    pub fn create_node(&mut self, path : &str, content : FileContent<'a>, create_parents : bool) -> Result<(), FileError>{
        let mut split_path = path.split('/').filter(|part| !part.is_empty());
        let first_part = match split_path.next() {
            Some(first_part) => first_part,
            None => return Err(FileError::InvalidPath { path: Box::from(path) }),
        };
        let res = self._create_node(first_part, split_path, content, create_parents);
        fix_error_with_path(res, Box::from(path))
    }

    fn create_file_with_content(&mut self, path : &str, content : &'a [u8], create_parents : bool) -> Result<(), FileError>{
        self.create_node(path, FileContent::File { content }, create_parents)
    }

    fn create_file(&mut self, path : &str, create_parents : bool) -> Result<(), FileError> {
        self.create_file_with_content(path, EMPTY_CONTENT, create_parents)
    }

    fn create_dir(&mut self, path : &str, create_parents : bool) -> Result<(), FileError>{
        self.create_node(path, FileContent::Directory { children: Vec::new() }, create_parents)
    }


    fn _get_file_node<'b>(&self, current_part : &'b str, mut rest_path : impl Iterator<Item = &'b str>) -> Result<&FileNode<'a>, FileError> {
        match &self.content {
            FileContent::Directory { children } => {
                match rest_path.next(){
                    Some(next_part) => {
                        let child = match children.iter().find(|f| f.name == current_part && f.is_dir()) {
                            Some(c) => c,
                            None => return Err(FileError::DirPathNotFound { dir_not_found: Box::from(current_part), path: Box::default() }), // the String::new() will be replaced in the wrapper
                        };
                        child._get_file_node(next_part, rest_path)
                    },
                    None => {
                        match children.iter().find(|f| f.name == current_part){
                            Some(file) => Ok(file),
                            None => Err(FileError::FileNotFound { path: Box::default() }),
                        }
                    }
                }
            }
            FileContent::File { .. } | FileContent::Device { .. } => Err(FileError::DirExpected { file_should_be_dir: Some(self.name.clone().into()), path: Box::default() }),
        }
    }


    fn get_file_node(&self, path : &str) -> Result<&FileNode<'a>, FileError> {
        if path.is_empty() {
            return Ok(self);
        }

        let mut split_path = path.split('/').filter(|part| !part.is_empty());
        let first_part = match split_path.next() {
            Some(first_part) => first_part,
            None => return Ok(self),
        };
        let res = self._get_file_node(first_part, split_path);
        fix_error_with_path(res, Box::from(path))
    }

    fn _get_file_node_mut<'b>(&mut self, current_part : &'b str, mut rest_path : impl Iterator<Item = &'b str>) -> Result<&mut FileNode<'a>, FileError> {
        match &mut self.content {
            FileContent::Directory { children } => {
                match rest_path.next(){
                    Some(next_part) => {
                        let child = match children.iter_mut().find(|f| f.name == current_part && f.is_dir()) {
                            Some(c) => c,
                            None => return Err(FileError::DirPathNotFound { dir_not_found: current_part.to_string().into(), path: Box::default() }),
                        };
                        child._get_file_node_mut(next_part, rest_path)
                    },
                    None => {
                        match children.iter_mut().find(|f| f.name == current_part){
                            Some(file) => Ok(file),
                            None => Err(FileError::FileNotFound { path: Box::default() }),
                        }
                    }
                }
            }
            FileContent::File { .. }| FileContent::Device { .. } => Err(FileError::DirExpected { file_should_be_dir: Some(Box::from(self.name.as_str())), path: Box::default() }), 
        }
    }


    fn get_file_node_mut(&mut self, path : &str) -> Result<&mut FileNode<'a>, FileError> {
        if path.is_empty() {
            return Ok(self);
        }

        let mut split_path = path.split('/').filter(|part| !part.is_empty());
        let first_part = match split_path.next() {
            Some(first_part) => first_part,
            None => return Ok(self),
        };

        let res = self._get_file_node_mut(first_part, split_path);
        fix_error_with_path(res, Box::from(path))
    }
}*/

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


pub fn file_stat(path : &str) -> Result<Stat, FileError> {
    serial_println!("file stat on {}", path);
    let file_node = get_inode(path)?;
    let stat = file_node.stat();

    Ok(stat)
}

lazy_static! {
    pub static ref ROOT_NODE : Arc<Inode> = {
        let tar_initrd = TarInitrd::new(INITRD_BYTES).expect("invalid tar");
        let root_node = fs_create_root_node(tar_initrd);
        root_node
    };
}

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
    
    root_node
}