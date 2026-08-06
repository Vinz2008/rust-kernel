use raw_cpuid::{CpuId, CpuIdReaderNative};
use x86_64::registers::control::{Cr4, Cr4Flags};

use crate::serial_println;

// TODO : merge these Cr4 writes if possible (check if they are optimised out ?)

// enable Supervisor Mode Execution Prevention
fn enable_smep(cpu_id : CpuId<CpuIdReaderNative>){
    if !cpu_id.get_extended_feature_info().is_some_and(|features| features.has_smep()){
        return;
    }
    unsafe {
        Cr4::update(|flags| flags.insert(Cr4Flags::SUPERVISOR_MODE_EXECUTION_PROTECTION));
    }
    serial_println!("enable SMEP");
}


fn enable_umip(cpu_id : CpuId<CpuIdReaderNative>){
    if !cpu_id.get_extended_feature_info().is_some_and(|features| features.has_umip()){
        return;
    }
    unsafe {
        Cr4::update(|flags| flags.insert(Cr4Flags::USER_MODE_INSTRUCTION_PREVENTION));
    }
    serial_println!("enable UMIP");
}

// TODO : should I enable smap ? but would prevent easy code in syscall to access page user accessible, would need to copy in kernel memory buffers or have temp buffers for write and using the stac and clac to enable this copy between userspace/kernel, is the tradeoff worth it ?

pub fn enable_security_features(){
    let cpu_id = CpuId::new();
    enable_smep(cpu_id);
    enable_umip(cpu_id);
}