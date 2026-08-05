use lazy_static::lazy_static;
use spin::Mutex;

use crate::{fs::FileError, vga::WRITER};

pub trait DeviceOps : Sync {
    fn read(&self, offset : usize, buffer : &mut [u8]) -> Result<usize, FileError>;

    fn write(&self, offset : usize, data : &[u8]) -> Result<usize, FileError>;
}

lazy_static! {
    pub static ref STDOUT : Stdout = Stdout::new();
}


// TODO : do buffered IO, and have stderr which would be not buffered
struct StdoutInner;

pub struct Stdout {
    // TODO : implement a reantrant lock like in rust ?
    stdout_inner : Mutex<StdoutInner>,
}

impl Stdout {
    fn new() -> Stdout {
        Stdout { 
            stdout_inner: Mutex::new(StdoutInner),
        }
    }
}

impl DeviceOps for Stdout {
    fn read(&self, _offset : usize, _buffer : &mut [u8]) -> Result<usize, FileError> {
        Err(FileError::NotReadableFile)
    }

    fn write(&self, _offset : usize, data : &[u8]) -> Result<usize, FileError> {
        let stdout_lock = self.stdout_inner.lock();
        WRITER.lock().write_bytes(data);
        
        let _ = stdout_lock; // TODO : remove this after really using the lock for buffered io
        Ok(data.len())
    }
}
