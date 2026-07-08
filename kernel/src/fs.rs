use alloc::string::ToString;
use shared_consts::{DirChild, Fd};

use crate::{initrd::{FileError, file_read_dir_children, file_stat}, process::OpenedFile, scheduler::with_scheduler_no_int};


pub fn process_open_file(path : &str, is_readable : bool, is_writable : bool) -> Option<Fd> {
    with_scheduler_no_int(|scheduler|{
        file_stat(path).ok()?;
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        let fd = current_proc.fd_list.len();
        current_proc.fd_list.push(Some(OpenedFile::new(path.to_string(), is_readable, is_writable)));
        Some(Fd(fd))
    })
}

pub fn process_close_file(fd : Fd) -> Option<()> {
    with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        let idx = fd.0;
        current_proc.fd_list.get_mut(idx)?.take();
        Some(())
    })
}

pub fn process_get_dir_children(fd : Fd, out : &mut [DirChild]) -> Result<usize, FileError> {
    let (path, offset) = with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process(&scheduler.processes);
        let opened_dir = current_proc.fd_list.get(fd.0).ok_or(FileError::FdNotFound)?.as_ref().unwrap();
        let path = opened_dir.path.clone();
        let offset = opened_dir.offset;
        Ok((path, offset))
    })?;

    let children_nb = file_read_dir_children(&path, offset, out)?;

    with_scheduler_no_int(|scheduler|{
        let current_proc = scheduler.current_process.unwrap();
        let current_proc = current_proc.get_process_mut(&mut scheduler.processes);
        let opened_dir = current_proc.fd_list.get_mut(fd.0).ok_or(FileError::FdNotFound)?.as_mut().unwrap();
        opened_dir.offset += children_nb;
        Ok(())
    })?;

    Ok(children_nb)
}