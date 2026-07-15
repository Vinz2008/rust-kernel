use core::cmp;

use alloc::{boxed::Box, slice, string::{String, ToString}, vec::Vec};
use lazy_static::lazy_static;
use shared_consts::{DIRENT_DIR, DIRENT_FILE, DirChild, PATH_NAME_MAX, Stat, StatMode};
use spin::Mutex;

use crate::{elf::load_elf, process::Process, scheduler::{SCHEDULER, start_first_process}, serial_println};

#[repr(C)]
pub struct TarHeader {
    filename : [u8; 100],
    mode : [u8; 8],
    uid : [u8; 8],
    gid : [u8; 8],
    size : [u8; 12],
    mtime : [u8; 12],
    chksum : [u8; 8], // TODO : check chksum ?
    typeflag : [u8; 1],
    linkname : [u8; 100],
    // ustar part
    magic : [u8; 6], // TODO : check magic
    version : [u8; 2],
    uname : [u8; 32],
    gname : [u8; 32],
    dev_major: [u8; 8],
    dev_minor: [u8; 8],
    prefix: [u8; 155],
    pad: [u8; 12],
}

#[derive(Debug)]
pub enum TarError {
    InvalidUtf8,
    InvalidOctal,
    NoEnd,
}

fn trim_right_nul(buf : &[u8]) -> &[u8] {
    let end_pos = buf.iter().position(|e| *e == 0).unwrap_or(buf.len());
    &buf[..end_pos]
}

fn parse_octal(buf : &[u8]) -> Result<u64, TarError> {
    let mut res : u64 = 0;
    for &b in buf {
        match b {
            0 | b' ' => break,
            b'0'..=b'7' => {
                res = res * 8 + (b - b'0') as u64;
            }
            _ => return Err(TarError::InvalidOctal),
        }
    }
    Ok(res)
}

impl TarHeader {
    // filename is max 256 chars
    // TODO : use also the prefix ?
    pub fn get_filename(&self) -> Result<&str, TarError> { 
        str::from_utf8(trim_right_nul(&self.filename)).map_err(|_| TarError::InvalidUtf8)
    }

    pub fn size(&self) -> Result<usize, TarError> {
        parse_octal(&self.size).map(|s| s as usize)
    }

    fn get_mode(&self) -> Result<u32, TarError> {
        parse_octal(&self.mode).map(|m| m as u32)
    }

    pub fn content(&self) -> Result<&[u8], TarError> {
        let size = self.size()?;
        let file_content_ptr = unsafe { (self as *const TarHeader as *const u8).add(512) };
        let slice = unsafe { slice::from_raw_parts(file_content_ptr, size) };
        Ok(slice)
    }
}

/*struct TarHeader {
    filename : ArrayString<256>,
    mode : u32,
    uid : u32,
    gid : u32,
    size : u64,
    mtime : u64,
    chksum : u32,
}*/

pub struct TarInitrd<'a> {
    content : &'a [u8],
    pub headers : Vec<&'a TarHeader>, // for now, find all headers, in the future lazy iterate ? (TODO ?)
}

fn get_headers(content : &[u8]) -> Result<Vec<&TarHeader>, TarError> {
    // TODO : maybe reserve the size of the vec to not waste memory ? (would need to do 2 times traversal, is it a good idea ?)
    let mut headers = Vec::new();
    let mut header_ptr = content.as_ptr();
    loop {
        if header_ptr as usize + 512 >= content.as_ptr_range().end as usize {
            return Err(TarError::NoEnd);
        }
        let header = unsafe { &*(header_ptr as *const TarHeader) };
        if header.filename[0] == b'\0' {
            break;
        }
        let size = parse_octal(&header.size)? as usize;
        headers.push(header);
        unsafe {
            header_ptr = header_ptr.add(((size/512) + 1) * 512);
        }
        if !size.is_multiple_of(512) {
            unsafe {
                header_ptr = header_ptr.add(512);
            }
        }
    }
    Ok(headers)
}

impl<'a> TarInitrd<'a> {
    pub fn new(content : &'a [u8]) -> Result<TarInitrd<'a>, TarError> {
        let headers = get_headers(content)?;
        Ok(TarInitrd { 
            content, 
            headers, 
        })
    }
}


const INITRD_BYTES : &[u8] = include_bytes!("../initrd.tar");

// TODO : make the children be not owned to be able to duplicate them in the tree ? (do I really need that ?), also would help with ownership by letting easily copy a filenode

enum FileContent<'a> {
    Directory {
        children : Vec<FileNode<'a>>,
    },
    File {
        content : &'a [u8], // TODO : replace with vec ? Cow ?
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

    fn new_dir(name : String) -> FileNode<'a> {
        Self::new_dir_with_children(name, Vec::new())
    }

    fn new_file_with_content(name : String, content : &'a [u8]) -> FileNode<'a> {
        let content = FileContent::File { content };
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
            FileContent::File { .. } => return Err(FileError::DirExpected { file_should_be_dir: Some(Box::from(self.name.as_str())), path: Box::default() }),
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
                let new_file = match content {
                    FileContent::Directory { children } => FileNode::new_dir_with_children(current_part.to_string(), children),
                    FileContent::File { content } => FileNode::new_file_with_content(current_part.to_string(), content),
                };
                if children.iter().find(|f| f.name == current_part).is_some() {
                    return Err(FileError::FileAlreadyExists { path: Box::default() });
                }
                children.push(new_file);
            }
        }
        
        Ok(())
    }

    fn create_node(&mut self, path : &str, content : FileContent<'a>, create_parents : bool) -> Result<(), FileError>{
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
            FileContent::File { .. } => Err(FileError::DirExpected { file_should_be_dir: Some(self.name.clone().into()), path: Box::default() }),
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
            FileContent::File { .. } => Err(FileError::DirExpected { file_should_be_dir: Some(Box::from(self.name.as_str())), path: Box::default() }), 
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
            FileContent::File { .. } => {
                return Err(FileError::DirExpected { file_should_be_dir: None, path: path.to_string().into() })
            }
        };

        let mut written = 0;
        
        for child in children.iter().skip(start_offset).take(out.len()){
            let kind = match child.content {
                FileContent::Directory { .. } => DIRENT_DIR,
                FileContent::File { .. } => DIRENT_FILE,
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

    fn get_file_content(&self, path : &str) -> Result<&'a [u8], FileError> {
        match &self.get_file_node(path)?.content {
            FileContent::File { content } => Ok(content),
            FileContent::Directory { .. } => Err(FileError::FileExpected { path: Box::from(path) }),
        }
    }
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
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
    }

    serial_println!("TEST");
    
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
        let path = &file.get_filename().unwrap()[1..];
        serial_println!("path : {}", path);
        if path != "/" {
            let node_content = match file.typeflag[0] {
                b'0' | 0 => FileContent::File { content: file.content().unwrap() },
                b'5' => FileContent::Directory { children: Vec::new() },
                _ => panic!("unsupported tag in initrd : {:?}", file.typeflag),
            };
            root_node.create_node(path, node_content, true).unwrap();  // TODO : better error handling ?
            
        }
    }
    
    root_node
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
    };
    Ok(Stat {
        mode
    })
}

pub fn file_read_dir_children(path : &str, start_offset : usize, out : &mut [DirChild]) -> Result<usize, FileError> {
    let root = ROOT_NODE.lock();
    root.read_dir_children(path, start_offset, out)
}

pub fn load_initrd_init() -> ! {
    let init_path = "/init";
    let (entrypoint, process_pid) = {
        /*let tar_initrd = TAR_INITRD.lock();
        for (idx, &file) in tar_initrd.headers.iter().enumerate() {
            serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
        }*/

        let root_node = ROOT_NODE.lock();
        
        let init_content = root_node.get_file_content(init_path).unwrap();
        

        let process_pid = Process::empty_process("/".to_string());


        let elf = {
            let scheduler_lock = SCHEDULER.lock();
            load_elf(init_content, process_pid.get_process(&scheduler_lock.processes))
        };
        
        (elf.ehdr.e_entry, process_pid)
    };
    

    {
        let mut scheduler_lock = SCHEDULER.lock();
        let process = process_pid.get_process_mut(&mut scheduler_lock.processes);
        let args = &[init_path];
        process.init_process(entrypoint as usize, args);
    };
    
    start_first_process(process_pid)
}