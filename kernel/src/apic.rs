use core::{cell::UnsafeCell, ptr::{read_volatile, write_volatile}, sync::atomic::{AtomicBool, Ordering}};

use acpi::{AcpiError, AcpiTables, platform::{AcpiPlatform, InterruptModel}};
use spin::{Mutex, Once};
use x86_64::{VirtAddr, instructions::interrupts::without_interrupts, registers::model_specific::{ApicBase, ApicBaseFlags}};

use crate::{acpi::MapHandler, interrupts::InterruptIndex, paging::PHYSICAL_MEMORY_OFFSET, pic, serial_println};

// TODO : finish this

#[repr(transparent)]
struct MmioRegister {
    val : UnsafeCell<u32>,
}

impl MmioRegister {
    #[inline]
    fn read(&self) -> u32 {
        unsafe { read_volatile(self.val.get()) }
    }

    // after calling this, read the id to ensure the write
    #[inline]
    fn write(&self, val : u32){
        unsafe { write_volatile(self.val.get(), val); }
    }
}

unsafe impl Sync for MmioRegister {}

#[repr(C)]
struct LocalApicRegisters {
    _reserved_000: [u8; 0x20],

    id: MmioRegister,

    _reserved_024: [u8; 0x30 - 0x24],

    version: MmioRegister,

    _reserved_034: [u8; 0x80 - 0x34],

    task_priority: MmioRegister,

    _reserved_084: [u8; 0xB0 - 0x84],

    end_of_interrupt: MmioRegister,

    _reserved_0b4: [u8; 0xF0 - 0xB4],

    spurious_interrupt_vector: MmioRegister,

    _reserved_0f4: [u8; 0x320 - 0xF4],

    timer_lvt: MmioRegister,

    _reserved_324: [u8; 0x330 - 0x324],

    thermal_lvt: MmioRegister,

    _reserved_334: [u8; 0x340 - 0x334],

    performance_lvt: MmioRegister,

    _reserved_344: [u8; 0x350 - 0x344],

    lint0_lvt: MmioRegister,

    _reserved_354: [u8; 0x360 - 0x354],

    lint1_lvt: MmioRegister,

    _reserved_364: [u8; 0x370 - 0x364],

    error_lvt: MmioRegister,
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

impl LocalApic {
    fn get_regs(&mut self) -> &mut LocalApicRegisters {
        unsafe {
            &mut *self.0.as_mut_ptr::<LocalApicRegisters>()
        }
    }

    pub fn end_of_interrupt(&mut self){
        self.get_regs().end_of_interrupt();
    }

    fn enable(&mut self){
        let regs = self.get_regs();
        
        regs.task_priority.write(0);

        regs.spurious_interrupt_vector.write(LAPIC_ENABLE | SPURIOUS_VECTOR as u32);
        regs.thermal_lvt.write(MASKED); // TODO ?
        regs.performance_lvt.write(MASKED); // TODO ?
        regs.lint0_lvt.write(MASKED); // TODO ?
        regs.lint1_lvt.write(MASKED); // TODO ?

        regs.timer_lvt.write(MASKED); // TODO : instead of routing the pic timer to the normal pit, use the real apic timer (need LAPIC time regs)
        let _ = regs.id.read();
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

        let idx = (gsi - self.gsi_base);
        let low_reg = (0x10 + idx * 2) as u8;
        let high_reg = low_reg + 1;
        self.write_register(low_reg, vec as u32 | MASKED);
        self.write_register(high_reg, (dest_apic_id as u32) << 24);
        self.write_register(low_reg, vec as u32);
    }
}


#[repr(C)]
struct IoApicMmio {
    register_select: MmioRegister,
    _reserved: [u8; 0x0c],
    register_window: MmioRegister,
}

static IO_APIC : Once<Mutex<IoApic>> = Once::new();

enum GSI {
    Timer = 0,
    Keyboard = 1,
}

fn _init_apic(acpi_tables : AcpiTables<MapHandler>) -> Result<(), AcpiError> {
    

    let handler = MapHandler;
    let platform = AcpiPlatform::new(acpi_tables, handler)?;

    let apic = match platform.interrupt_model {
        InterruptModel::Apic(apic) => apic,
        InterruptModel::Unknown => panic!("unknown interrupt model"),
        _ => unreachable!(),
    };

    let (frame, mut flags) = ApicBase::read();

    // TODO : support for X2APIC
    assert!(!flags.contains(ApicBaseFlags::X2APIC_ENABLE), "x2APIC enabled; x2APIC is not supported yet");

    if !flags.contains(ApicBaseFlags::LAPIC_ENABLE){
        flags.insert(ApicBaseFlags::LAPIC_ENABLE);

        unsafe {
            ApicBase::write(frame, flags);
        }
    }
    

    let lapic_physical_addr = frame.start_address();

    let lapic_virtal_addr = *PHYSICAL_MEMORY_OFFSET.get().unwrap() + lapic_physical_addr.as_u64();

    LOCAL_APIC.call_once(|| Mutex::new(LocalApic(lapic_virtal_addr)));

    let local_apid_id = {
        let mut lock = LOCAL_APIC.get().unwrap().lock();
        lock.enable();
        lock.get_regs().id()
    };
    

    for io_apic in &apic.io_apics {
        serial_println!("apic io : id = {}, address = 0x{:x}, global_system_interrupt_base = {}", io_apic.id, io_apic.address, io_apic.global_system_interrupt_base);
    }

    let io_apic_info = *apic.io_apics.first().ok_or(AcpiError::HostUnimplemented)?;

    let io_apic_virt_addr = *PHYSICAL_MEMORY_OFFSET.get().unwrap() + io_apic_info.address as u64;

    IO_APIC.call_once(|| Mutex::new(IoApic { base: io_apic_virt_addr, gsi_base: io_apic_info.global_system_interrupt_base }));
    
    {
        let mut io_apic_lock = IO_APIC.get().unwrap().lock();
        io_apic_lock.route(GSI::Timer as u32, InterruptIndex::Timer as u8, local_apid_id);
        io_apic_lock.route(GSI::Keyboard as u32, InterruptIndex::Keyboard as u8, local_apid_id);
    }

    unsafe { pic::PICS.lock().disable() };
    HAS_ENABLED_APIC.store(true, Ordering::Relaxed);

    Ok(())
}

// TODO : remove this after removing PIC support ?
pub static HAS_ENABLED_APIC : AtomicBool = AtomicBool::new(false);

pub fn init_apic(acpi_tables : AcpiTables<MapHandler>) -> Result<(), AcpiError> {
    // TODO : only do it if supported
    without_interrupts(|| _init_apic(acpi_tables))
}