use crate::{fs::FileError, vga::WRITER};

pub trait DeviceOps : Sync {
    fn read(&self, offset : usize, buffer : &mut [u8]) -> Result<usize, FileError>;

    fn write(&self, offset : usize, data : &[u8]) -> Result<usize, FileError>;
}



// TODO : make this not the stdout, but something like /dev/console, or /dev/console_output, then it will be assigned to the right fd, and have a stdout link/device that will select the right one depending on the process running


// TODO : have stderr for errors

pub struct Stdout;

impl DeviceOps for Stdout {
    fn read(&self, _offset : usize, _buffer : &mut [u8]) -> Result<usize, FileError> {
        Err(FileError::NotReadableFile)
    }

    fn write(&self, _offset : usize, data : &[u8]) -> Result<usize, FileError> {
        WRITER.lock().write_bytes(data);
        Ok(data.len())
    }
}
