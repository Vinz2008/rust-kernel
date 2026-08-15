use core::{hint, mem::offset_of};

use x86_64::{registers::control::Cr3, structures::paging::{Page, PageSize, PageTableFlags, PhysFrame, Size4KiB}};

use crate::{apic::TIMER_HZ, interrupts::ticks, mmio::{self, MmioRegister, Reserved}, paging::{active_level_4_table, map_page_at_in, map_page_phys_at_in}, pcie::{PciBarKind, PcieDevice}, serial_println};

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
    clb : MmioRegister<u32>, // 0x00, command list base address, 1K-byte aligned
    clbu : MmioRegister<u32>, // 0x04, command list base address upper 32 bits
    fb : MmioRegister<u32>, // 0x08, FIS base address, 256-byte aligned
    fbu : MmioRegister<u32>, // 0x0C, FIS base address upper 32 bits
    is : MmioRegister<u32>, // 0x10, interrupt status
    ie : MmioRegister<u32>, // 0x14, interrupt enable
    cmd : MmioRegister<u32>, // 0x18, command and status

    reserved1 : Reserved<u32>, // 0x1C, Reserved

    tfd : MmioRegister<u32>, // 0x20, task file data
    sig : MmioRegister<u32>, // 0x24, signature
    ssts : MmioRegister<u32>, // 0x28, SATA status (SCR0:SStatus)
    sctl : MmioRegister<u32>, // 0x2C, SATA control (SCR2:SControl)
    serr : MmioRegister<u32>, // 0x30, SATA error (SCR1:SError)
    sact : MmioRegister<u32>, // 0x34, SATA active (SCR3:SActive)
    ci : MmioRegister<u32>, // 0x38, command issue
    sntf : MmioRegister<u32>, // 0x3C, SATA notification (SCR4:SNotification)
    fbs : MmioRegister<u32>, // 0x40, FIS-based switch control
    
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
    let ssts = port.ssts.read();
    let ipm = (ssts >> 8) as u8;
    let det = ssts as u8;

    if det != HBA_PORT_DET_PRESENT {
        return None;
    }
    if ipm != HBA_PORT_IPM_ACTIVE {
        return None;
    }
    let sata_type = match port.sig.read(){
        SATA_SIG_ATA => SataType::Sata,
        SATA_SIG_ATAPI => SataType::Satapi,
        SATA_SIG_SEMB => SataType::Semb,
        SATA_SIG_PM => SataType::PM,
        sig => panic!("unkown port type : {}", sig),
    };
    Some(sata_type)
}

fn init_ports(abar : &HBAMem){
    let mut pi = abar.pi.read();
    for port in 0..32 {
        if (pi & 1) != 0 {
            if let Some(sata_type) = get_port_type(&abar.ports[port]){
                // TODO ?
            }
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

const CAP2_BOH: u32 = 1 << 0;
const BOHC_BOS: u32 = 1 << 0;
const BOHC_OOS: u32 = 1 << 1;
const BOHC_BB: u32  = 1 << 4;

fn bios_os_handoff(hba: &HBAMem) {
    if hba.cap2.read() & CAP2_BOH == 0 {
        return;
    }

    let mut bohc = hba.bohc.read();
    bohc |= BOHC_OOS;
    hba.bohc.write(bohc);

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
            return;
        }
    }
}

const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1; // Allow accesses to the AHCI MMIO BAR.
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2; // Allow the AHCI controller to DMA to/from RAM.
const PCI_COMMAND_INTX_DISABLE: u16 = 1 << 10;

// TODO : use the checklist from here : https://wiki.osdev.org/AHCI (needs to also add code in the pcie part ?)
pub fn init(device : &PcieDevice){
    let header = device.get_common_header();

    let mut cmd = header.command.read();
    cmd |= PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_INTX_DISABLE; // disables intx, TODO : need to enable MSI instead

    header.command.write(cmd);

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
}