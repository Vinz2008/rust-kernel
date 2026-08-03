use spin::Once;

// TODO : replace it with a dynamically allocated buf after enabling xsave to enable newer instructions than sse like avx instructions
#[derive(Clone)]
#[repr(C, align(16))]
pub struct FxState {
    bytes : [u8; 512],
}

impl FxState {
    fn new() -> FxState {
        FxState { bytes: [0; 512] }
    }

    pub fn save(&mut self){
        unsafe {
            core::arch::asm!(
                "fxsave64 [{ptr}]",
                ptr = in(reg) self.bytes.as_mut_ptr(),
                options(nostack),
            );
        }
    }

    pub fn restore(&self){
        unsafe {
            core::arch::asm!(
                "fxrstor64 [{ptr}]",
                ptr = in(reg) self.bytes.as_ptr(),
                options(nostack),
            )
        }
    }
}

pub static DEFAULT_FXSTATE : Once<FxState> = Once::new();

pub fn init_fpu_template(){
    let state = unsafe {
        core::arch::asm!("fninit", options(nostack));
        let mut state = FxState::new();
        let mxcsr = 0x1f80 as u32;
        core::arch::asm!(
            "ldmxcsr [{ptr}]",
            ptr = in(reg) &mxcsr,
            options(nostack, readonly),
        );

        state.save();
        state
    };

    DEFAULT_FXSTATE.call_once(|| state);
}