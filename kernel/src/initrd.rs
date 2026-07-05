use core::str::Split;

use alloc::{format, slice, string::{String, ToString}, vec::Vec};
use lazy_static::lazy_static;
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
    name : String,
    content : FileContent<'a>,
}

const EMPTY_CONTENT : &[u8] = &[];

impl<'a> FileNode<'a> {
    fn new_dir(name : String) -> FileNode<'a> {
        let content = FileContent::Directory { children: Vec::new() };
        FileNode { 
            name, 
            content, 
        }
    }

    fn new_file_with_content(name : String, content : &'a [u8]) -> FileNode<'a> {
        let content = FileContent::File { content: content };
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

    // TODO : support dir
    fn _create_file_with_content<'b>(&mut self, current_part : &'b str, mut rest_path : impl Iterator<Item = &'b str>, content : &'a [u8]){
        match rest_path.next() {
            Some(next_part) => {
                match &mut self.content {
                    FileContent::Directory { children } => {
                        let child = match children.iter_mut().find(|f| f.name == current_part && f.is_dir()) {
                            Some(c) => c,
                            None => panic!("couldn't find {}", current_part), // TODO : better error handling
                        };
                        child._create_file_with_content(next_part, rest_path, content);
                    }
                    FileContent::File { .. } => panic!("expected a dir"), // TODO : replace with good error handling 
                }
            }
            None => {
                match &mut self.content {
                    FileContent::Directory { children } => {
                        let new_file = FileNode::new_file_with_content(current_part.to_string(), content);
                        if children.iter().find(|f| &f.name == current_part).is_some() {
                            panic!("file already exists"); // TODO : better error handling
                        }
                        children.push(new_file);
                    }
                    FileContent::File { .. } => panic!("can't create a file in a file"), // TODO : better error handling
                }
            }
        }
    }

    fn create_file_with_content(&mut self, path : &str, content : &'a [u8]){
        let mut split_path = path.split('/').filter(|part| !part.is_empty());
        let first_part = match split_path.next() {
            Some(first_part) => first_part,
            None => panic!("empty path"), // TODO : better error handling
        };
        self._create_file_with_content(first_part, split_path, content);
    }

    fn create_file(&mut self, path : &str){
        self.create_file_with_content(path, EMPTY_CONTENT);
    }

    fn _get_file_node<'b>(&self, current_part : &'b str, mut rest_path : impl Iterator<Item = &'b str>) -> &FileNode<'a> {
        match rest_path.next() {
            Some(next_part) => {
                match &self.content {
                    FileContent::Directory { children } => {
                        let child = match children.iter().find(|f| f.name == current_part && f.is_dir()) {
                            Some(c) => c,
                            None => panic!("couldn't find {}", current_part), // TODO : better error handling
                        };
                        child._get_file_node(next_part, rest_path)
                    }
                    FileContent::File { .. } => panic!("expected a dir"), // TODO : replace with good error handling 
                }
            }
            None => {
                match &self.content {
                    FileContent::Directory { children } => {
                        children.iter().find(|f| &f.name == current_part).unwrap() // TODO : better error handling
                    }
                    FileContent::File { .. } => panic!("can't create a file in a file"), // TODO : better error handling
                }
            }
        }
    }


    // TODO : get_file_mut
    fn get_file_node(&self, path : &str) -> &FileNode<'a> {
        if path == "" {
            return self;
        }

        let mut split_path = path.split('/').filter(|part| !part.is_empty());
        let first_part = match split_path.next() {
            Some(first_part) => first_part,
            None => panic!("error in path"), // TODO : better error handling
        };

        self._get_file_node(first_part, split_path)
    }

    fn _get_file_node_mut<'b>(&mut self, current_part : &'b str, mut rest_path : impl Iterator<Item = &'b str>) -> &mut FileNode<'a> {
        match rest_path.next() {
            Some(next_part) => {
                match &mut self.content {
                    FileContent::Directory { children } => {
                        let child = match children.iter_mut().find(|f| f.name == current_part && f.is_dir()) {
                            Some(c) => c,
                            None => panic!("couldn't find {}", current_part), // TODO : better error handling
                        };
                        child._get_file_node_mut(next_part, rest_path)
                    }
                    FileContent::File { .. } => panic!("expected a dir"), // TODO : replace with good error handling 
                }
            }
            None => {
                match &mut self.content {
                    FileContent::Directory { children } => {
                        children.iter_mut().find(|f| &f.name == current_part).unwrap() // TODO : better error handling
                    }
                    FileContent::File { .. } => panic!("can't create a file in a file"), // TODO : better error handling
                }
            }
        }
    }


    // TODO : get_file_mut
    fn get_file_node_mut(&mut self, path : &str) -> &mut FileNode<'a> {
        if path == "" {
            return self;
        }

        let mut split_path = path.split('/').filter(|part| !part.is_empty());
        let first_part = match split_path.next() {
            Some(first_part) => first_part,
            None => panic!("error in path"), // TODO : better error handling
        };

        self._get_file_node_mut(first_part, split_path)
    }

    fn get_file_content(&self, path : &str) -> &'a [u8] {
        match &self.get_file_node(path).content {
            FileContent::File { content } => *content,
            FileContent::Directory { .. } => panic!("can't get file content of dir"),
        }
    }
}

lazy_static! {
    /*pub static ref TAR_INITRD : Mutex<TarInitrd<'static>> = {
        let tar_initrd = TarInitrd::new(INITRD_BYTES).expect("invalid tar");
        Mutex::new(tar_initrd)
    };*/

    pub static ref ROOT_NODE : Mutex<FileNode<'static>> = {
        let tar_initrd = TarInitrd::new(INITRD_BYTES).expect("invalid tar");
        let root_node = fs_create_root_node(tar_initrd);
        Mutex::new(root_node)
    };
}

fn fs_create_root_node(tar_initrd : TarInitrd<'static>) -> FileNode<'static> {
    let mut file_node = FileNode::new_dir("<ROOT NODE>".to_string());
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
    }

    serial_println!("TEST");
    
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
        let path = &file.get_filename().unwrap()[1..];
        serial_println!("path : {}", path);
        if path != "/" {
            file_node.create_file_with_content(path, file.content().unwrap());
        }
    }
    
    file_node
}


pub fn get_file_content<'a>(path : &str) -> &'a [u8] {
    let root_node = ROOT_NODE.lock();
    root_node.get_file_content(path)
}

pub fn load_initrd_init() -> ! {
    let (entrypoint, process_pid) = {
        /*let tar_initrd = TAR_INITRD.lock();
        for (idx, &file) in tar_initrd.headers.iter().enumerate() {
            serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
        }*/

        let root_node = ROOT_NODE.lock();

        let init_content = root_node.get_file_content("/init");
        

        let process_pid = Process::empty_process();


        let elf = {
            let scheduler_lock = SCHEDULER.lock();
            load_elf(init_content, process_pid.get_process(&scheduler_lock.processes))
        };
        
        (elf.ehdr.e_entry, process_pid)
    };
    

    {
        let mut scheduler_lock = SCHEDULER.lock();
        let process = process_pid.get_process_mut(&mut scheduler_lock.processes);
        process.init_process(entrypoint as usize);
    };
    
    start_first_process(process_pid)
}