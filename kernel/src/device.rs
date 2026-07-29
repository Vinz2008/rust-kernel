use crate::fs::FileError;

pub trait DeviceOps : Sync {
    fn read(&self, offset : usize, buffer : &mut [u8]) -> Result<usize, FileError>;

    fn write(&self, offset : usize, data : &[u8]) -> Result<usize, FileError>;
}
