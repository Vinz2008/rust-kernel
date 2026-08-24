use raw_cpuid::{CpuId, ExtendedFeatures};
use x86_64::registers::control::{Cr4, Cr4Flags};

use crate::serial_println;

// enable Supervisor Mode Execution Prevention
fn enable_smep(flags : &mut Cr4Flags, features : Option<&ExtendedFeatures>){
    if !features.is_some_and(|features| features.has_smep()){
        return;
    }
    flags.insert(Cr4Flags::SUPERVISOR_MODE_EXECUTION_PROTECTION);
    serial_println!("enable SMEP");
}

// enable Supervisor Mode Access Prevention
fn enable_smap(flags : &mut Cr4Flags, features : Option<&ExtendedFeatures>){
    if !features.is_some_and(|features| features.has_smap()){
        panic!("SMAP needed")
    }
    flags.insert(Cr4Flags::SUPERVISOR_MODE_ACCESS_PREVENTION);
    serial_println!("enable SMAP");
}


fn enable_umip(flags : &mut Cr4Flags, features : Option<&ExtendedFeatures>){
    if !features.is_some_and(|features| features.has_umip()){
        return;
    }
    flags.insert(Cr4Flags::USER_MODE_INSTRUCTION_PREVENTION);
    serial_println!("enable UMIP");
}


// TODO : should I enable smap ? but would prevent easy code in syscall to access page user accessible, would need to copy in kernel memory buffers or have temp buffers for write and using the stac and clac to enable this copy between userspace/kernel, is the tradeoff worth it ?
// TODO : enable LASS ? (need to change 0xb8000 usage and enable the same stac and clac for smap)
// TODO : should I enable CET ?


pub fn enable_security_features(){
    let cpu_id = CpuId::new();
    let extended_features = cpu_id.get_extended_feature_info();
    unsafe {
        Cr4::update(|flags|{
            enable_smep(flags, extended_features.as_ref());
            enable_smap(flags, extended_features.as_ref());
            enable_umip(flags, extended_features.as_ref());
        });
    }
    
}