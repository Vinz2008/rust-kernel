use core::cmp;

use alloc::{boxed::Box, string::{String, ToString}, vec::Vec};
use shared_consts::{DIRENT_DEVICE, DIRENT_DIR, DIRENT_FILE, DirChild, Fd, PATH_NAME_MAX, Stat, StatMode};
use spin::mutex::Mutex;

use crate::{initrd::{INITRD_BYTES, TarInitrd}, process::OpenedFile, scheduler::with_scheduler_no_int, serial_println};
use lazy_static::lazy_static;

pub fn process_open_file(path : &str, is_readable : bool, is_writable : bool) -> Option<Fd> {
    with_scheduler_no_int(|scheduler|{
        let canonicalized_path = {
            let current_cwd = &scheduler.current_process.unwrap().get_process(&scheduler.processes).cwd_path;
            canonicalize_path(path, current_cwd)?
        };
        file_stat(&canonicalized_path).ok()?;
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        let fd = current_proc.fd_list.len();
        current_proc.fd_list.push(Some(OpenedFile::new(canonicalized_path, is_readable, is_writable)));
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
    let (path, offset) = with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process(&scheduler.processes);
        let opened_dir = current_proc.fd_list.get(fd.0).ok_or(FileError::FdNotFound)?.as_ref().unwrap();
        let path = opened_dir.path.clone();
        let offset = opened_dir.offset;
        Ok((path, offset))
    })?;

    let children_nb = file_read_dir_children(&path, offset, out)?;

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
                components.pop()?;
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

pub enum FileContent<'a> {
    Directory {
        children : Vec<FileNode<'a>>,
    },
    File {
        content : &'a [u8], // TODO : replace with vec ? Cow ?
    },
    Device {

    }
}

pub struct FileNode<'a> {
    name : String, // TODO : replace these with Box<str> to lower memory usage ?
    content : FileContent<'a>,
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

impl<'a> FileNode<'a> {
    fn new_dir_with_children(name : String, children : Vec<FileNode<'a>>) -> FileNode<'a> {
        let content = FileContent::Directory { children };
        FileNode { 
            name, 
            content, 
        }
    }

    pub fn new_dir(name : String) -> FileNode<'a> {
        Self::new_dir_with_children(name, Vec::new())
    }

    fn new_file_with_content(name : String, content : &'a [u8]) -> FileNode<'a> {
        let content = FileContent::File { content };
        FileNode { 
            name,
            content 
        }
    }

    fn new_device(name : String, ) -> FileNode<'a> {
        let content = FileContent::Device {  };
        FileNode { 
            name, 
            content 
        }
    }

    fn new_file(name : String) -> FileNode<'a> {
        Self::new_file_with_content(name, EMPTY_CONTENT)
    }

    fn is_dir(&self) -> bool {
        matches!(self.content, FileContent::Directory { .. })
    }

    fn is_file(&self) -> bool {
        matches!(self.content, FileContent::File { .. })
    }

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
                    FileContent::Device {  } => FileNode::new_device(new_file_name),
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

    pub fn read_dir_children(&self, path : &str, start_offset : usize, out : &mut [DirChild]) -> Result<usize, FileError> {
        let dir_node = self.get_file_node(path)?;

        let children = match &dir_node.content {
            FileContent::Directory { children } => children,
            FileContent::File { .. } | FileContent::Device { .. } => {
                return Err(FileError::DirExpected { file_should_be_dir: None, path: path.to_string().into() })
            }
        };

        let mut written = 0;
        
        for child in children.iter().skip(start_offset).take(out.len()){
            let kind = match child.content {
                FileContent::Directory { .. } => DIRENT_DIR,
                FileContent::File { .. } => DIRENT_FILE,
                FileContent::Device { .. } => DIRENT_DEVICE,
            };

            let name_len = cmp::min(child.name.len(), PATH_NAME_MAX);

            let mut entry = DirChild {
                kind,
                name_len: name_len as u8,
                name: [0; PATH_NAME_MAX],
            };

            entry.name[..name_len].copy_from_slice(&child.name.as_bytes()[..name_len]);
            out[written] = entry;
            written += 1;
        }

        Ok(written)
    }

    pub fn get_file_content(&self, path : &str) -> Result<&'a [u8], FileError> {
        match &self.get_file_node(path)?.content {
            FileContent::File { content } => Ok(content),
            FileContent::Directory { .. } | FileContent::Device { .. } => Err(FileError::FileExpected { path: Box::from(path) }),
        }
    }
}


pub fn get_file_content<'a>(path : &str) -> Result<&'a [u8], FileError> {
    let root_node = ROOT_NODE.lock();
    root_node.get_file_content(path)
}



pub fn file_stat(path : &str) -> Result<Stat, FileError> {
    serial_println!("file stat on {}", path);
    let root_node = ROOT_NODE.lock();
    let file_node = root_node.get_file_node(path)?;
    let mode = match file_node.content {
        FileContent::File { content } => StatMode::File {
            size: content.len(),
        },
        FileContent::Directory { .. } => StatMode::Directory,
        FileContent::Device { .. } => StatMode::Device,
    };
    Ok(Stat {
        mode
    })
}

pub fn file_read_dir_children(path : &str, start_offset : usize, out : &mut [DirChild]) -> Result<usize, FileError> {
    let root = ROOT_NODE.lock();
    root.read_dir_children(path, start_offset, out)
}

lazy_static! {
    pub static ref ROOT_NODE : Mutex<FileNode<'static>> = {
        let tar_initrd = TarInitrd::new(INITRD_BYTES).expect("invalid tar");
        let root_node = fs_create_root_node(tar_initrd);
        Mutex::new(root_node)
    };
}

fn fs_create_root_node(tar_initrd : TarInitrd<'static>) -> FileNode<'static> {
    let mut root_node = FileNode::new_dir("<ROOT NODE>".to_string());
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap().as_ref(), file.size().unwrap());
    }

    serial_println!("TEST");
    
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap().as_ref(), file.size().unwrap());
        let path = &file.get_filename().unwrap()[1..];
        serial_println!("path : {}", path);
        if path != "/" {
            let node_content = match file.get_typeflag() {
                b'0' | 0 => FileContent::File { content: file.content().unwrap() },
                b'5' => FileContent::Directory { children: Vec::new() },
                typeflag => panic!("unsupported tag in initrd : {:?}", typeflag),
            };
            root_node.create_node(path, node_content, true).unwrap();  // TODO : better error handling ?
            
        }
    }
    
    root_node
}