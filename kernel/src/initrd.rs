use alloc::{borrow::Cow, format, slice, string::ToString, vec::Vec};

use crate::{elf::load_elf, fs::{ROOT_NODE, get_inode}, process::Process, scheduler::{SCHEDULER, start_first_process}};

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
    pub fn get_filename(&self) -> Result<Cow<'_, str>, TarError> { 
        let filename = str::from_utf8(trim_right_nul(&self.filename)).map_err(|_| TarError::InvalidUtf8)?;
        let prefix = str::from_utf8(trim_right_nul(&self.prefix)).map_err(|_| TarError::InvalidUtf8)?;
        let res = if prefix.is_empty(){
            Cow::Borrowed(filename)
        } else {
            Cow::Owned(format!("{}/{}", prefix, filename))
        };
        Ok(res)
    }

    pub fn size(&self) -> Result<usize, TarError> {
        parse_octal(&self.size).map(|s| s as usize)
    }

    fn get_mode(&self) -> Result<u32, TarError> {
        parse_octal(&self.mode).map(|m| m as u32)
    }

    pub fn get_typeflag(&self) -> u8 {
        self.typeflag[0]
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


pub const INITRD_BYTES : &[u8] = include_bytes!("../initrd.tar");


pub fn load_initrd_init() -> ! {
    let init_path = "/init";
    let (entrypoint, process_pid) = {
        /*let tar_initrd = TAR_INITRD.lock();
        for (idx, &file) in tar_initrd.headers.iter().enumerate() {
            serial_println!("file {} {} {}", idx, file.get_filename().unwrap(), file.size().unwrap());
        }*/
        
        let init_node = get_inode(init_path).unwrap();
        let init_content = init_node.read_entire_file_in_mem().unwrap();
        

        let process_pid = Process::empty_process("/".to_string());


        let elf = {
            let scheduler_lock = SCHEDULER.lock();
            load_elf(&init_content, process_pid.get_process(&scheduler_lock.processes))
        }.expect("failed loading init");
        
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