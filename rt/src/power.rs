use shared_consts::{SHUTDOWN_FAILURE, SHUTDOWN_REBOOT, SHUTDOWN_SUCCESS};

use crate::syscall::syscall_shutdown;

// TODO : maybe remove the failure/sucess shutdown concept althogether from the kernel and userspace
pub enum ShutdownResult {
    Failure,
    Success,
}

pub fn shutdown(shutdown_res : ShutdownResult) -> ! {
    let flags = match shutdown_res {
        ShutdownResult::Success => SHUTDOWN_SUCCESS,
        ShutdownResult::Failure => SHUTDOWN_FAILURE,
    };
    syscall_shutdown(flags)
}

pub fn reboot() -> ! {
    syscall_shutdown(SHUTDOWN_REBOOT)
}