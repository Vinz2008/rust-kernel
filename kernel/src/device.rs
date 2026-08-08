//use arrayvec::ArrayVec;
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

// TODO : think about the buffering strategy
// TODO : add the buffering in the userspace, and make this not the stdout, but something like /dev/console, or /dev/console_output, then it will be assigned to the right fd, and have a stdout link/device that will select the right one depending on the process running

//const STDOUT_BUF_SIZE : usize = 4096;

// TODO : have stderr which would be not buffered
struct StdoutInner {
 //   buf : ArrayVec<u8, STDOUT_BUF_SIZE>,
}

/*impl StdoutInner {
    fn flush(&mut self){
        if self.buf.is_empty(){
            return;
        }
        WRITER.lock().write_bytes(&self.buf);
        self.buf.clear();
    }
}*/

pub struct Stdout {
    // TODO : implement a reantrant lock like in rust ?
    stdout_inner : Mutex<StdoutInner>,
}

impl Stdout {
    fn new() -> Stdout {
        Stdout { 
            stdout_inner: Mutex::new(StdoutInner {
//                buf: ArrayVec::new(),
            }),
        }
    }

    /*pub fn flush(&self){
        self.stdout_inner.lock().flush();
    }*/
}

impl DeviceOps for Stdout {
    fn read(&self, _offset : usize, _buffer : &mut [u8]) -> Result<usize, FileError> {
        Err(FileError::NotReadableFile)
    }

    fn write(&self, _offset : usize, data : &[u8]) -> Result<usize, FileError> {
        let stdout_lock = self.stdout_inner.lock();
        
        // TODO : should I transform the bytes to utf str instead ?
        /*for &byte in data {
            if stdout_lock.buf.is_full(){
                stdout_lock.flush();
            }

            stdout_lock.buf.push(byte);
            if byte == b'\n' {
                stdout_lock.flush();
            }
        }*/


        WRITER.lock().write_bytes(data);

        let _ = stdout_lock;
        Ok(data.len())
    }
}
