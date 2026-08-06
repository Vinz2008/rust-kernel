use core::fmt;

use alloc::vec::Vec;
use shared_consts::{CREATE_FILE, Fd, StatMode};

use crate::syscall::{syscall_close, syscall_fstat, syscall_open, syscall_read, syscall_write};

pub enum FileError {
    FileExpected,
    FdNotFound,
    ReadError,
}

pub fn read_entire_file(file : &File) -> Result<Vec<u8>, FileError> {
    let fd = file.0;
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

// can represent a text file or dir, so TODO ? : cache if type of file (dir, file, device), to prevent some invalid usage before even doing the syscall, which will make a lot of operations faster, but will cost a syscall for each file open, is it worth it ?
pub struct File(Fd);

impl File {
    // TODO : improve the mode interface (with a struct with a builder pattern ?)
    pub fn open(path : &str, mode : u64) -> Option<File> {
        syscall_open(path, mode).map(|fd| File(fd))
    }

    pub fn create(path : &str, mode : u64) -> Option<File> {
        File::open(path, mode | CREATE_FILE)
    }

    // TODO : remove this ? or just keep it unsafe ?
    pub unsafe fn get_fd(&self) -> Fd {
        self.0
    }

    pub fn close(self){
        syscall_close(self.0);
    }
}

// TODO
pub struct IoError;

fn default_write_fmt<W : IoWrite + ?Sized>(this : &mut W, args : fmt::Arguments<'_>) -> Result<(), IoError> {
    struct Adapter<'a, T: ?Sized + 'a> {
        inner: &'a mut T,
        error: Result<(), IoError>,
    }
    impl <T: IoWrite + ?Sized> fmt::Write for Adapter<'_, T> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            match self.inner.write(s.as_bytes()){
                Ok(_) => Ok(()),
                Err(e) => {
                    self.error = Err(e);
                    Err(fmt::Error)
                }
            }
        }
    }
    let mut output = Adapter { inner: this, error: Ok(()) };
    match fmt::write(&mut output, args){
        Ok(()) => Ok(()),
        Err(_) => {
            if output.error.is_err(){
                output.error
            } else {
                panic!("a formatting trait implementation returned an error when the underlying stream did not");
            }
        }
    }
}

// TODO : trait IoRead ?

// inspired from std::io::Write
pub trait IoWrite {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError>;

    // TODO : add flush after adding buffered writes ? in kernel or userspace ?
    //fn flush(&mut self) -> Result<(), IoError>;

    // TODO : add write_all ?

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<(), IoError> {
        if let Some(s) = args.as_statically_known_str(){
            self.write(s.as_bytes()).map(|_| ()) // TODO : use write_all in the future ?
        } else {
            default_write_fmt(self, args)
        }
    }
}

impl IoWrite for File {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        syscall_write(self.0, buf).ok_or(IoError)
    }
}

impl Drop for File {
    fn drop(&mut self) {
        syscall_close(self.0);
    }
}