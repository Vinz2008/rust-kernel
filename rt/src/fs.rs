use alloc::vec::Vec;
use shared_consts::{Fd, StatMode};

use crate::syscall::{syscall_fstat, syscall_read};

pub enum FileError {
    FileExpected,
    FdNotFound,
    ReadError,
}

pub fn read_entire_file(fd : Fd) -> Result<Vec<u8>, FileError> { 
    let stat = syscall_fstat(fd).ok_or(FileError::FdNotFound)?;
    let size = match stat.mode {
        StatMode::File { size } => size,
        _ => return Err(FileError::FileExpected),
    };
    let mut res = Vec::with_capacity(size);

    let read = syscall_read(fd, &mut res).ok_or(FileError::ReadError)?;
    if read != size {
        return Err(FileError::ReadError);
    }

    Ok(res)
}