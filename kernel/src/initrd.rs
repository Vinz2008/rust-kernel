use core::{intrinsics::copy_nonoverlapping, mem, ptr};

use alloc::{slice, vec::Vec};
use elf::{ElfBytes, endian::AnyEndian};
use x86_64::{VirtAddr, structures::paging::{Page, PageTableFlags, Size4KiB}};

use crate::{allocator::map_page_at, serial_println};

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

    let file = ElfBytes::<AnyEndian>::minimal_parse(init_content).expect("Error when parsing init elf");

    let text_section_header = file.section_header_by_name(".text").unwrap().unwrap();
    let text_section_content = file.section_data(&text_section_header).unwrap().0;
    //serial_println!("text section content : {:?}", text_section_content);

    for prog_header in file.segments().unwrap() {
        serial_println!("type={} offset={:#x} vaddr={:#x} filesz={:#x} memsz={:#x}", prog_header.p_type, prog_header.p_offset, prog_header.p_vaddr, prog_header.p_filesz, prog_header.p_memsz);
        match prog_header.p_type {
            elf::abi::PT_LOAD => {
                let virtual_addr = prog_header.p_vaddr;
                let start = VirtAddr::new(virtual_addr);
                let end = VirtAddr::new(virtual_addr + prog_header.p_memsz as u64 - 1);

                let start_page = Page::<Size4KiB>::containing_address(start);
                let end_page = Page::<Size4KiB>::containing_address(end);
                for page in Page::range_inclusive(start_page, end_page){
                    map_page_at(page.start_address(), PageTableFlags::PRESENT | PageTableFlags::WRITABLE);
                }
                
                let segment_off = prog_header.p_offset as usize;
                let bytes = &init_content[segment_off..(segment_off + prog_header.p_filesz as usize)];
                let virtual_addr_ptr = virtual_addr as usize as *mut u8;
                unsafe {
                    ptr::copy_nonoverlapping(bytes.as_ptr(), virtual_addr_ptr, prog_header.p_filesz as usize);
                }
                if prog_header.p_filesz < prog_header.p_memsz {
                    // zero the rest
                    unsafe {
                        ptr::write_bytes(virtual_addr_ptr.add(prog_header.p_filesz as usize), 0, (prog_header.p_memsz - prog_header.p_filesz) as usize);
                    }
                }
            },

            elf::abi::PT_GNU_RELRO | elf::abi::PT_PHDR | elf::abi::PT_GNU_STACK => {},
            p_type => serial_println!("unknown p_type : {}", p_type),
        }
    }

    let common_data = file.find_common_data().expect("error when getting common data of init elf");
    let (sym_table, str_table) = file.symbol_table().expect("symbol table not found in init elf").unwrap();

    let s = sym_table.iter().find(|sym| str_table.get(sym.st_name as usize).unwrap() == "main").expect("main not found");
    
    let main_virt_address = s.st_value as usize;
    let main_fun : extern "C" fn() -> i32 = unsafe { core::mem::transmute(main_virt_address) };
    main_fun();
}