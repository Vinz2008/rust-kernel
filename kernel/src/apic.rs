use acpi::{AcpiError, platform::{InterruptModel, interrupt::Apic}};
use spin::{Mutex, Once};
use x86_64::{VirtAddr, instructions::{interrupts::without_interrupts, port::Port}, registers::model_specific::{ApicBase, ApicBaseFlags}};

use crate::{acpi::ACPI_PLATFORM, interrupts::InterruptIndex, mmio::MmioRegister, paging::PHYSICAL_MEMORY_OFFSET, serial_println};

//unsafe impl<T> Sync for MmioRegister<T> {}

type ApicMmioReg = MmioRegister<u32>;

#[repr(C)]
struct LocalApicRegisters {
    _reserved_000: [u8; 0x20],

    id: ApicMmioReg,

    _reserved_024: [u8; 0x30 - 0x24],

    version: ApicMmioReg,

    _reserved_034: [u8; 0x80 - 0x34],

    task_priority: ApicMmioReg,

    _reserved_084: [u8; 0xB0 - 0x84],

    end_of_interrupt: ApicMmioReg,

    _reserved_0b4: [u8; 0xF0 - 0xB4],

    spurious_interrupt_vector: ApicMmioReg,

    _reserved_0f4: [u8; 0x320 - 0xF4],

    timer_lvt: ApicMmioReg,

    _reserved_324: [u8; 0x330 - 0x324],

    thermal_lvt: ApicMmioReg,

    _reserved_334: [u8; 0x340 - 0x334],

    performance_lvt: ApicMmioReg,

    _reserved_344: [u8; 0x350 - 0x344],

    lint0_lvt: ApicMmioReg,

    _reserved_354: [u8; 0x360 - 0x354],

    lint1_lvt: ApicMmioReg,

    _reserved_364: [u8; 0x370 - 0x364],

    error_lvt: ApicMmioReg,

    _reserved_374: [u8; 0x380 - 0x374],

    initial_count: ApicMmioReg,

    _reserved_384: [u8; 0x390 - 0x384],

    current_count: ApicMmioReg,

    _reserved_394: [u8; 0x3e0 - 0x394],

    divide_configuration: ApicMmioReg,
}

impl LocalApicRegisters {
    fn id(&self) -> u8 {
        ((self.id.read() >> 24) & 0xFF) as u8
    }

    fn end_of_interrupt(&mut self){
        self.end_of_interrupt.write(0);
        let _ = self.id.read();    
    }
}

// TODO : make this apic really core/thread local
#[derive(Clone)]
pub struct LocalApic(VirtAddr);


const MASKED: u32 = 1 << 16;
const LAPIC_ENABLE: u32 = 1 << 8;
const SPURIOUS_VECTOR: u8 = 0xff;

const TIMER_PERIODIC: u32 = 1 << 17;

// Divide configuration encoding:
// 0b0011 = divide by 16
const TIMER_DIVIDE_16: u32 = 0b0011;

const PIT_FREQUENCY: u64 = 1_193_182;

struct PitWait {
    old_control : u8,
}

fn prepare_pit_wait_ms(ms: u64) -> PitWait {
    let count = PIT_FREQUENCY * ms / 1000;

    debug_assert!(count > 0 && count <= u16::MAX as u64);

    unsafe {
        let mut command = Port::<u8>::new(0x43);
        let mut channel2 = Port::<u8>::new(0x42);
        let mut control = Port::<u8>::new(0x61);

        let old_control = control.read();

        // Gate LOW: channel 2 not running yet.
        // Speaker output also disabled.
        control.write(old_control & !0b11);
        
        // channel 2, low+high bytes, mode 0, binary
        command.write(0b1011_0000);

        let count = count as u16;

        channel2.write(count as u8);
        channel2.write((count >> 8) as u8);
        
        PitWait { old_control }
    }
}

#[inline(always)]
fn pit_wait_ms(pit_wait : PitWait) {
    unsafe {
        let mut control = Port::<u8>::new(0x61);
        let value = control.read();
        control.write((value | 0x01) & !0x02);
        while control.read() & (1 << 5) == 0 {
            core::hint::spin_loop();
        }
        control.write(pit_wait.old_control);
    }
}

impl LocalApic {
    fn get_regs(&mut self) -> &mut LocalApicRegisters {
        unsafe {
            &mut *self.0.as_mut_ptr::<LocalApicRegisters>()
        }
    }

    pub fn end_of_interrupt(&mut self){
        self.get_regs().end_of_interrupt();
    }

    fn start_timer(&mut self, initial_count : u32){
        let regs = self.get_regs();

        // stop timer
        regs.timer_lvt.write(InterruptIndex::Timer as u32 | MASKED);

        // APIC timer clock divided by 16, changes how fast it ticks
        regs.divide_configuration.write(TIMER_DIVIDE_16);

        regs.timer_lvt.write(InterruptIndex::Timer as u32 | TIMER_PERIODIC);

        // this starts the timer, initial count = nb of tick before interrupt
        regs.initial_count.write(initial_count);
    }

    fn enable(&mut self){

        let regs = self.get_regs();
        
        regs.task_priority.write(0);
            
        regs.thermal_lvt.write(MASKED);
        regs.performance_lvt.write(MASKED);
        regs.lint0_lvt.write(MASKED);
        regs.lint1_lvt.write(MASKED); // TODO ? could need it for some NMI events
        regs.error_lvt.write(MASKED);

        regs.spurious_interrupt_vector.write(LAPIC_ENABLE | SPURIOUS_VECTOR as u32);

        let pit_wait = prepare_pit_wait_ms(50);

        regs.timer_lvt.write(InterruptIndex::Timer as u32 | MASKED);

        regs.divide_configuration.write(TIMER_DIVIDE_16);

        // starts counting down
        regs.initial_count.write(u32::MAX);

        pit_wait_ms(pit_wait);
    
        let current = regs.current_count.read();
        let ticks_50ms = u32::MAX - current;
        let initial_count = ticks_50ms / 5; // 10ms = 100Hz
        self.start_timer(initial_count);

        let _ = self.get_regs().id.read();
    }
}

pub static LOCAL_APIC : Once<Mutex<LocalApic>> = Once::new(); 


struct IoApic {
    base: VirtAddr,
    gsi_base: u32,
}

impl IoApic {

    fn get_regs(&mut self) -> &mut IoApicMmio {
        unsafe {
            &mut *self.base.as_mut_ptr::<IoApicMmio>()
        }
    }

    fn read_register(&mut self, reg : u8) -> u32 {
        let regs = self.get_regs();
        regs.register_select.write(reg as u32);
        regs.register_window.read()
    }

    fn write_register(&mut self, reg : u8, val : u32){
        let regs = self.get_regs();
        regs.register_select.write(reg as u32);
        regs.register_window.write(val);
    }
    fn route(&mut self, gsi : u32, vec : u8, dest_apic_id : u8){
        debug_assert!(gsi >= self.gsi_base);

        let idx = gsi - self.gsi_base;
        let low_reg = (0x10 + idx * 2) as u8;
        let high_reg = low_reg + 1;
        self.write_register(low_reg, vec as u32 | MASKED);
        self.write_register(high_reg, (dest_apic_id as u32) << 24);
        self.write_register(low_reg, vec as u32);
    }
}


#[repr(C)]
struct IoApicMmio {
    register_select: ApicMmioReg,
    _reserved: [u8; 0x0c],
    register_window: ApicMmioReg,
}

enum Gsi {
    Timer = 0,
    Keyboard = 1,
}

static IO_APIC : Once<Mutex<IoApic>> = Once::new();

fn irq_to_gsi(apic : &Apic, irq : u8) -> u32 {
    apic.interrupt_source_overrides.iter()
        .find(|iso| iso.isa_source == irq)
        .map(|iso| iso.global_system_interrupt)
        .unwrap_or(irq as u32)
}

fn _init_apic() -> Result<(), AcpiError> {

    let apic = match &ACPI_PLATFORM.get().ok_or(AcpiError::HostUnimplemented)?.interrupt_model {
        InterruptModel::Apic(apic) => apic,
        InterruptModel::Unknown => panic!("unknown interrupt model"),
        _ => unreachable!(),
    };

    let (frame, mut flags) = ApicBase::read();

    // TODO : support for X2APIC ? (only need it for large smp systems)
    assert!(!flags.contains(ApicBaseFlags::X2APIC_ENABLE), "x2APIC enabled; x2APIC is not supported yet");

    if !flags.contains(ApicBaseFlags::LAPIC_ENABLE){
        flags.insert(ApicBaseFlags::LAPIC_ENABLE);

        unsafe {
            ApicBase::write(frame, flags);
        }
    }
    

    let lapic_physical_addr = frame.start_address();

    let lapic_virtal_addr = PHYSICAL_MEMORY_OFFSET + lapic_physical_addr.as_u64();

    LOCAL_APIC.call_once(|| Mutex::new(LocalApic(lapic_virtal_addr)));

    let local_apid_id = {
        let mut lock = LOCAL_APIC.get().unwrap().lock();
        lock.enable();
        lock.get_regs().id()
    };
    

    for io_apic in &apic.io_apics {
        serial_println!("apic io : id = {}, address = 0x{:x}, global_system_interrupt_base = {}", io_apic.id, io_apic.address, io_apic.global_system_interrupt_base);
    }

    for iso in &apic.interrupt_source_overrides {
        serial_println!(
            "ISO: ISA IRQ {} -> GSI {}, polarity={:?}, trigger={:?}",
            iso.isa_source,
            iso.global_system_interrupt,
            iso.polarity,
            iso.trigger_mode,
        );
    }

    let io_apic_info = *apic.io_apics.first().ok_or(AcpiError::HostUnimplemented)?;

    let io_apic_virt_addr = PHYSICAL_MEMORY_OFFSET + io_apic_info.address as u64;

    IO_APIC.call_once(|| Mutex::new(IoApic { base: io_apic_virt_addr, gsi_base: io_apic_info.global_system_interrupt_base }));
    
    {
        let mut io_apic_lock = IO_APIC.get().unwrap().lock();
        let gsi_timer = irq_to_gsi(apic, Gsi::Timer as u8);
        serial_println!("gsi_timer : {}", gsi_timer);
        io_apic_lock.route(gsi_timer, InterruptIndex::Timer as u8, local_apid_id);
        let gsi_keyboard = irq_to_gsi(apic, Gsi::Keyboard as u8);
        serial_println!("gsi_keyboard : {}", gsi_keyboard);
        io_apic_lock.route(gsi_keyboard, InterruptIndex::Keyboard as u8, local_apid_id);
    }

    Ok(())
}


pub fn init_apic() -> Result<(), AcpiError> {
    // TODO : only do it if supported
    without_interrupts(_init_apic)
}