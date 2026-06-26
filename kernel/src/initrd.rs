use core::ops::DerefMut;

use alloc::{slice, vec::Vec};

use crate::{elf::{USER_STACK_TOP, get_elf_entrypoint, load_elf}, process::{CURRENT_PROCESS, Pid, Process}, serial_println, userspace::switch_to_userspace};

#[repr(C)]
pub struct TarHeader {
    filename : [u8; 100],
    mode : [u8; 8],
    uid : [u8; 8],
    gid : [u8; 8],
    size : [u8; 12],
    mtime : [u8; 12],
    chksum : [u8; 8],
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

fn get_headers<'a>(content : &'a [u8]) -> Result<Vec<&'a TarHeader>, TarError> {
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
        if size % 512 != 0 {
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

// TODO : refactor this code, to have the exe loading part in a separate elf.rs

pub fn load_initrd(){

    let tar_initrd = TarInitrd::new(INITRD_BYTES).expect("invalid tar");
    for (idx, &file) in tar_initrd.headers.iter().enumerate() {
        serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
    }

    let init_file_header = *tar_initrd.headers.iter().find(|e| e.get_filename().unwrap() == "./init").unwrap();
    let init_content = init_file_header.content().unwrap();

    let file = load_elf(init_content);

    //let common_data = file.find_common_data().expect("error when getting common data of init elf");
    
    let entrypoint_fun = get_elf_entrypoint(&file);

    *CURRENT_PROCESS.lock().deref_mut() = Some(Process::new_process());

    //serial_println!("main : 0x{:x}", entrypoint_fun as usize);

    //entrypoint_fun();
    switch_to_userspace(entrypoint_fun, USER_STACK_TOP);
}