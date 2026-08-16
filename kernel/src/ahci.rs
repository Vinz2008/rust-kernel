use core::hint;

use x86_64::{registers::control::Cr3, structures::{idt::InterruptStackFrame, paging::{FrameAllocator, Page, PageSize, PageTableFlags, PhysFrame, Size4KiB}}};

use crate::{allocator::MEMORY_MANAGER, apic::TIMER_HZ, interrupts::{InterruptIndex, end_of_interrupt, ticks}, mmio::{self, MmioRegister, Reserved}, paging::{PHYSICAL_MEMORY_OFFSET, map_page_phys_at_in}, pcie::{PciBarKind, PcieDevice, enable_msi}, serial_println};

// TODO : finish this

// HBA registers

// TODO : maybe rename these fields for better readability ?
#[repr(C)]
struct HBAMem {
    cap : MmioRegister<u32>, // 0x00, Host capability
    ghc : MmioRegister<u32>, // 0x04, Global host control
    is : MmioRegister<u32>, // 0x08, Interrupt status
    pi : MmioRegister<u32>, // 0x0C, Port implemented
    vs : MmioRegister<u32>, // 0x10, Version
    ccc_ctl : MmioRegister<u32>, // 0x14, Command completion coalescing control
    ccc_pts : MmioRegister<u32>, // 0x18, Command completion coalescing ports
    em_loc : MmioRegister<u32>, // 0x1C, Enclosure management location
    em_ctl : MmioRegister<u32>, // 0x20, Enclosure management control
    cap2 : MmioRegister<u32>, // 0x24, Host capabilities extended
    bohc : MmioRegister<u32>,

    reserved : Reserved<[u8; 116]>, // 0x2C - 0x9F, Reserved

    // TODO : will they be used ? do I need to keep MmioRegister or could I just put it in a Reserved<>?
    vendor: [MmioRegister<u8>; 96], // 0xA0 - 0xFF, Vendor specific registers

    // there could be less than 32 in the real struct, 32 is the maximum (it is variable length)
    ports : [HBAPort; 32], 
}

#[repr(C)]
struct HBAPort {
    command_list_base_lower : MmioRegister<u32>, // 0x00, 1K-byte aligned
    command_list_base_upper : MmioRegister<u32>, // 0x04
    fis_base_addr_lower : MmioRegister<u32>, // 0x08, 256-byte aligned
    fis_base_addr_upper : MmioRegister<u32>, // 0x0C
    interrupt_status : MmioRegister<u32>, // 0x10
    interrupt_enable : MmioRegister<u32>, // 0x14
    cmd : MmioRegister<u32>, // 0x18, command and status

    reserved1 : Reserved<u32>, // 0x1C, Reserved

    task_file_data : MmioRegister<u32>, // 0x20
    signature : MmioRegister<u32>, // 0x24
    sata_status : MmioRegister<u32>, // 0x28
    sata_control : MmioRegister<u32>, // 0x2C
    sata_err : MmioRegister<u32>, // 0x30
    sata_active : MmioRegister<u32>, // 0x34
    cmd_issue : MmioRegister<u32>, // 0x38
    sata_notification : MmioRegister<u32>, // 0x3C
    fis_based_switch_control : MmioRegister<u32>, // 0x40
    
    reserved2 : Reserved<[u32; 11]>,
    
    // TODO : will they be used ? do I need to keep MmioRegister or could I just put it in a Reserved<>?
    vendor : [MmioRegister<u32>; 4],
    
}

const HBA_PORT_IPM_ACTIVE : u8 = 1;
const HBA_PORT_DET_PRESENT : u8 = 3;
const SATA_SIG_ATA : u32 = 0x00000101;
const SATA_SIG_ATAPI : u32 = 0xEB140101;
const SATA_SIG_SEMB : u32 = 0xC33C0101;
const SATA_SIG_PM : u32 = 0x96690101;


enum SataType {
    Sata, // normal sata drive
    Satapi, // satapi drive (ex : cdrom)
    Semb, // enclosure management bridge
    PM, // port multiplier
}

fn get_port_type(port : &HBAPort) -> Option<SataType> {
    let ssts = port.sata_status.read();
    let ipm = (ssts >> 8) as u8;
    let det = ssts as u8;

    if det != HBA_PORT_DET_PRESENT {
        return None;
    }
    if ipm != HBA_PORT_IPM_ACTIVE {
        return None;
    }
    let sata_type = match port.signature.read(){
        SATA_SIG_ATA => SataType::Sata,
        SATA_SIG_ATAPI => SataType::Satapi,
        SATA_SIG_SEMB => SataType::Semb,
        SATA_SIG_PM => SataType::PM,
        sig => panic!("unkown port type : {}", sig),
    };
    Some(sata_type)
}

#[repr(C, align(1024))]
struct CommandList {
    headers: [CommandHeader; 32],
}

#[repr(C)]
struct CommandHeader {
    flags: u16,
    prdt_length: u16, // number of prdt entries in the command table
    prdbc: u32, // number of bytes transferred by hardware
    command_table_phys_base_lower: u32,
    command_table_phys_base_upper: u32,
    reserved: [u32; 4],
}

#[repr(C, align(256))]
struct ReceivedFis {
    data: [u8; 256],
}

#[repr(C)]
struct PrdtEntry {
    data_base_address: u32,
    data_base_address_upper: u32,
    reserved: u32,
    dbc_i: u32,
}

#[repr(C)]
struct CommandTable<const N: usize> {
    command_fis: [u8; 64],
    atapi_command: [u8; 16],
    reserved: [u8; 48],
    prdt: [PrdtEntry; N],
}

#[repr(C, align(4096))]
struct AhciPortDma {
    command_list: CommandList,
    received_fis: ReceivedFis,
    command_table: CommandTable<1>, // TODO : see how much needed, for now 1
    _padding: [u8; 2672],
}

struct DmaData {
    ahci_port_dma : &'static mut AhciPortDma,
    achi_dma_phys : PhysFrame,
}

const _: () = assert!(size_of::<AhciPortDma>() == 4096);

fn allocate_port_dma() -> DmaData {
    let frame : PhysFrame<Size4KiB> = MEMORY_MANAGER.get().unwrap().lock().frame_allocator.allocate_frame().unwrap();
    let phys = frame.start_address();
    let virt = PHYSICAL_MEMORY_OFFSET + phys.as_u64();
    let ahci_port_dma = unsafe {
        &mut *virt.as_mut_ptr::<AhciPortDma>()
    };
    DmaData { ahci_port_dma, achi_dma_phys: frame }
}

const PXCMD_ST: u32  = 1 << 0;
const PXCMD_FRE: u32 = 1 << 4;
const PXCMD_FR: u32  = 1 << 14;
const PXCMD_CR: u32  = 1 << 15;

fn stop_port(port: &HBAPort) {
    port.cmd.update(|cmd| cmd & !PXCMD_ST);
    if !wait_with_timeout(500, || port.cmd.read() & PXCMD_CR == 0){
        panic!("AHCI command engine did not stop");
    }
    port.cmd.update(|cmd| cmd & !PXCMD_FRE);
    if !wait_with_timeout(500, || port.cmd.read() & PXCMD_FR == 0) {
        panic!("AHCI FIS receive engine did not stop");
    }
}

const SCTL_DET_MASK: u32 = 0xF;
const SCTL_DET_INIT: u32 = 0x1;
const SCTL_DET_NONE: u32 = 0x0;

const SSTS_DET_MASK: u32 = 0xF;
const SSTS_IPM_MASK: u32 = 0xF << 8;
const SSTS_DET_NONE: u32 = 0x0;
const SSTS_DET_PRESENT: u32 = 0x3;
const SSTS_IPM_ACTIVE: u32 = 0x1 << 8;

fn reset_port(port : &HBAPort){
    serial_println!(
        "before reset: SSTS={:#x} SCTL={:#x} CMD={:#x}",
        port.sata_status.read(),
        port.sata_control.read(),
        port.cmd.read(),
    );
    port.sata_control.update(|sctl| (sctl & !SCTL_DET_MASK) | SCTL_DET_INIT);
    serial_println!(
        "DET=1: SSTS={:#x} SCTL={:#x}",
        port.sata_status.read(),
        port.sata_control.read(),
    );
    sleep_ms(1);
    port.sata_control.update(|sctl| (sctl & !SCTL_DET_MASK) | SCTL_DET_NONE);

    serial_println!(
        "DET=0: SSTS={:#x} SCTL={:#x}",
        port.sata_status.read(),
        port.sata_control.read(),
    );

    if !wait_with_timeout(1000, || port.sata_status.read() & SSTS_DET_MASK == SSTS_DET_PRESENT){
        serial_println!(
            "timeout: SSTS={:#x} SCTL={:#x} CMD={:#x} SERR={:#x}",
            port.sata_status.read(),
            port.sata_control.read(),
            port.cmd.read(),
            port.sata_err.read(),
        );
        panic!("AHCI port COMRESET timeout");
    }
    port.sata_err.write(u32::MAX);
}


fn start_port(port : &HBAPort){
    if !wait_with_timeout(500, || port.cmd.read() & PXCMD_CR == 0){
        panic!("AHCI port command engine did not become idle");
    }
    port.cmd.update(|v| v | PXCMD_FRE);
    port.cmd.update(|v| v | PXCMD_ST);
}

fn init_sata_drive(port_dma : DmaData, hba_port : &HBAPort){
    
}

fn init_port(hba_port : &HBAPort){
    let ssts = hba_port.sata_status.read();
    let det = ssts & SSTS_DET_MASK;

    if det == SSTS_DET_NONE {
        // Port exists in PI, but nothing is connected.
        return;
    }

    let port_dma = allocate_port_dma();
    stop_port(hba_port);

    let command_list_phys = port_dma.achi_dma_phys.start_address();
    hba_port.command_list_base_lower.write(command_list_phys.as_u64() as u32);
    hba_port.command_list_base_upper.write((command_list_phys.as_u64() >> 32) as u32);
            
    let received_fis_phys = command_list_phys + size_of::<CommandList>() as u64;
    hba_port.fis_base_addr_lower.write(received_fis_phys.as_u64() as u32);
    hba_port.fis_base_addr_upper.write((received_fis_phys.as_u64() >> 32) as u32);

    // TODO : if increase the CommandTable prdt size, need to write the commandHeader -> commandTable
    let cmd_list_header = &mut port_dma.ahci_port_dma.command_list.headers[0];
    let command_table_phys = received_fis_phys + size_of::<ReceivedFis>() as u64;
    debug_assert_eq!(command_table_phys.as_u64() & 0x7f, 0);
    cmd_list_header.command_table_phys_base_lower = command_table_phys.as_u64() as u32;
    cmd_list_header.command_table_phys_base_upper = (command_table_phys.as_u64() >> 32) as u32;
    cmd_list_header.flags = 0;
    cmd_list_header.prdt_length = 0;
    cmd_list_header.prdbc = 0;

    reset_port(hba_port);

    hba_port.interrupt_status.write(u32::MAX);
    hba_port.sata_err.write(u32::MAX);

    let ssts = hba_port.sata_status.read();

    if ssts & SSTS_DET_MASK != SSTS_DET_PRESENT {
        return;
    }

    if ssts & SSTS_IPM_MASK != SSTS_IPM_ACTIVE {
        // link isn't in active state
        return;
    }

    //hba_port.interrupt_enable.write(0); // TODO : enable interrupts ?

    //start_port(hba_port);

    if let Some(sata_type) = get_port_type(hba_port){
        match sata_type {
            SataType::Sata => {
                init_sata_drive(port_dma, hba_port);
            },
            SataType::Satapi => panic!("Satapi drive not supported"), // TODO ?
            SataType::Semb => panic!("Semb drive not supported"),
            SataType::PM => panic!("Port Multiplier drive not supported")
        }
    }
}

fn init_ports(abar : &HBAMem){
    let mut pi = abar.pi.read();
    for port in 0..32 {
        // TODO : also check if I support the device ?
        if (pi & 1) != 0 {
            let hba_port = &abar.ports[port];
            init_port(hba_port);
        }
        pi >>= 1;
    }
}

// busy wait, TODO ? better waiting API ? but should I use this better API here ?
pub fn wait_with_timeout(timeout_ms: u64, mut condition : impl FnMut() -> bool) -> bool {
    let ticks_needed = timeout_ms.saturating_mul(TIMER_HZ).div_ceil(1000);
    let start = ticks();

    loop {
        if condition(){
            return true;
        }

        if ticks().wrapping_sub(start) >= ticks_needed {
            return false;
        }
        
        hint::spin_loop();
    }
}

fn sleep_ms(time_ms : u64){
    wait_with_timeout(time_ms, || false);
}

const CAP2_BOH: u32 = 1 << 0;
const BOHC_BOS: u32 = 1 << 0;
const BOHC_OOS: u32 = 1 << 1;
const BOHC_BB: u32  = 1 << 4;

fn bios_os_handoff(hba: &HBAMem) {
    if hba.cap2.read() & CAP2_BOH == 0 {
        return;
    }

    hba.bohc.update(|bohc| bohc | BOHC_OOS);

    let bohc = hba.bohc.read();

    if bohc & BOHC_BOS == 0 {
        return;
    }

    if bohc & BOHC_BB != 0 {
        // BIOS is cleaning up, wait up to 2 seconds
        if !wait_with_timeout(2000, || hba.bohc.read() & BOHC_BOS == 0){
            panic!("BIOS handoff timeout");
        }
        return;
    }

    let condition = ||{
        let bohc = hba.bohc.read();
        bohc & BOHC_BOS == 0 || bohc & BOHC_BB != 0
    };

    if wait_with_timeout(25, condition){
        let bohc = hba.bohc.read();
        if bohc & BOHC_BOS == 0 {
            return;
        }

        if bohc & BOHC_BB != 0 {
            if !wait_with_timeout(2000, || hba.bohc.read() & BOHC_BOS == 0){
                panic!("BIOS handoff timeout");
            }
        }
    }
}

const GHC_HR: u32 = 1 << 0;

fn reset_controller(hba: &HBAMem) {
    hba.ghc.update(|ghc| ghc | GHC_HR);

    if !wait_with_timeout(1000, || hba.ghc.read() & GHC_HR == 0) {
        panic!("AHCI controller reset timeout");
    }
}

const GHC_IE: u32 = 1 << 1; // Interrupt enable
const GHC_AE: u32 = 1 << 31; // AHCI enable

fn enable_ahci(hba: &HBAMem){
    debug_assert_eq!(hba.ghc.read() & GHC_HR, 0); // need ghc to be cleared before this
    hba.ghc.update(|ghc| ghc | GHC_AE | GHC_IE);
}

const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1; // Allow accesses to the AHCI MMIO BAR.
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2; // Allow the AHCI controller to DMA to/from RAM.
const PCI_COMMAND_INTX_DISABLE: u16 = 1 << 10;

const CAP_S64A: u32 = 1 << 31; // supports 64 bit addressing

// TODO : use the checklist from here : https://wiki.osdev.org/AHCI (needs to also add code in the pcie part ?)
pub fn init(device : &PcieDevice){
    enable_msi(device, InterruptIndex::Ahci);
    let header = device.get_common_header();

    header.command.update(|cmd| cmd | PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_INTX_DISABLE);

    let bar5 = device.get_bar(5);
    let (bar5_addr, bar5_size) = match &bar5.kind {
        &PciBarKind::Memory { address, prefetchable, size } => {
            debug_assert!(!prefetchable);
            (address, size)
        },
        PciBarKind::Io { .. } => panic!("bar 5 is port instead of mem"), // TODO : better error handling ?
    };

    let bar5_page_nb = bar5_size.div_ceil(Size4KiB::SIZE) * Size4KiB::SIZE;
    let bar5_virt = mmio::alloc_virtual_pages(bar5_page_nb).unwrap();
    let page_start = Page::<Size4KiB>::containing_address(bar5_virt);
    let page_end = Page::containing_address(bar5_virt + bar5_size - 1);
    let page_range = Page::range_inclusive(page_start, page_end);

    let frame_start = PhysFrame::<Size4KiB>::containing_address(bar5_addr);
    let frame_end = PhysFrame::containing_address(bar5_addr + bar5_size - 1);
    let frame_range = PhysFrame::range_inclusive(frame_start, frame_end);

    let (level_4_table_frame, _) = Cr3::read();
    let page_table_phys = level_4_table_frame.start_address();

    for (page, phys_frame) in page_range.zip(frame_range) {
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE;
        match map_page_phys_at_in(page_table_phys, phys_frame, page.start_address(), flags){
            Ok(flush) => {
                flush.flush();
            }
            Err(e) => panic!("error when mapping page : {:?}", e), // TODO : better error handling ?
        }
    }

    let hba = unsafe {
        &*bar5_virt.as_ptr::<HBAMem>()
    };

    bios_os_handoff(hba);

    reset_controller(hba);

    enable_ahci(hba);


    let cap = hba.cap.read();
    let supports_64bit_dma = cap & CAP_S64A != 0;
    if !supports_64bit_dma {
        // TODO : in this case, allocate DMA memory below 4GiB
        panic!("for now, non 64 bits DMA not supported");
    }

    init_ports(hba);
}

pub extern "x86-interrupt" fn ahci_interrupt_handler(_stack_frame: InterruptStackFrame){
    // TODO
    end_of_interrupt();
}