use x86_64::instructions::interrupts;

use crate::{fs::FileError, interrupts::KEYBOARD_RINGBUF, scheduler::{SCHEDULER, SchedulerState, WaitReason}, vga::WRITER};

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

pub struct Stderr;

// TODO : better handling of stderr ? (need to be able to replace the implementation, need to depend on the process ?)
impl DeviceOps for Stderr {
    fn read(&self, offset : usize, buffer : &mut [u8]) -> Result<usize, FileError> {
        Stdout.read(offset, buffer)
    }

    fn write(&self, offset : usize, data : &[u8]) -> Result<usize, FileError> {
        Stdout.write(offset, data)
    }
}

pub struct Stdin;

impl DeviceOps for Stdin {
    fn read(&self, _offset : usize, buffer : &mut [u8]) -> Result<usize, FileError> {
        interrupts::without_interrupts(||{
            let mut keyboard_ringbuf_lock = KEYBOARD_RINGBUF.lock();
            let count = keyboard_ringbuf_lock.len();
            if count == 0 {
                let mut scheduler_lock = SCHEDULER.lock();
                let current_pid = scheduler_lock.current_process.unwrap();
                current_pid.get_process_mut(&mut scheduler_lock.processes).state = SchedulerState::Wait(WaitReason::WaitRead);
                
                scheduler_lock.processes_waiting_keyboard.add_process(current_pid);
                return Err(FileError::NoDataYet);
            }
            let mut off = 0;
            while off < buffer.len() {
                let c = match keyboard_ringbuf_lock.pop(){
                    Some(c) => c,
                    None => break,
                };
                buffer[off] = c;
                off += 1;
            }
            
            Ok(off)
        })
        
    }
    
    fn write(&self, _offset : usize, _data : &[u8]) -> Result<usize, FileError> {
        Err(FileError::NotWritableFile)
    }
}