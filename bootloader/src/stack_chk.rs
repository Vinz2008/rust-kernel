use core::arch::x86_64::_rdrand64_step;
use raw_cpuid::CpuId;

pub fn stack_chk_random() -> Option<u64> {
    let cpuid = CpuId::new();

    if cpuid.get_feature_info().is_some_and(|features| features.has_rdrand()){
        for _ in 0..10 {
            let mut value = 0;
            if unsafe { _rdrand64_step(&mut value) } == 1 {
                return Some(value);
            }
        }
    }
        
    None
}