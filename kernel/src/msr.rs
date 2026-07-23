use core::arch::naked_asm;

use raw_cpuid::{CpuId, CpuIdReaderNative};
use x86_64::{VirtAddr, registers::{control::{Efer, EferFlags}, model_specific::{LStar, SFMask, Star}, rflags::RFlags}, structures::gdt::SegmentSelector};

use crate::{gdt::GDT, syscall::syscall_instr_entry};

fn has_msr(cpu_id : CpuId<CpuIdReaderNative>) -> bool {
    cpu_id.get_feature_info().is_some_and(|f| f.has_msr())
}

fn support_syscall(cpu_id : CpuId<CpuIdReaderNative>) -> bool {
    cpu_id.get_extended_processor_and_feature_identifiers().is_some_and(|f| f.has_syscall_sysret())
}

pub fn enable_syscall(){
    let user_code = GDT.1.user_code_selector;
    let user_data = GDT.1.user_data_selector;
    let kernel_code = GDT.1.kernel_code_selector;
    let kernel_data = GDT.1.kernel_data_selector;

    debug_assert_eq!(kernel_data.0, kernel_code.0 + 8);
    debug_assert_eq!(user_data.0 + 8, user_code.0);
    
    let cpu_id = CpuId::new();
    if !has_msr(cpu_id) || !support_syscall(cpu_id){
        panic!("Sycall not supported on this hardware");
    }
    Star::write(user_code, user_data, kernel_code, kernel_data).unwrap();
    LStar::write(VirtAddr::new(syscall_instr_entry as *const () as u64));

    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::TRAP_FLAG | RFlags::DIRECTION_FLAG);

    unsafe {
        Efer::update(|flags| flags.insert(EferFlags::SYSTEM_CALL_EXTENSIONS));
    }
}