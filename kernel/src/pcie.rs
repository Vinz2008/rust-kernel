// TODO : maybe add the old config mechanism with ports for legacy pci support

use acpi::sdt::mcfg::Mcfg;
use alloc::vec::Vec;
use arrayvec::ArrayVec;
use spin::Once;
use x86_64::{PhysAddr, VirtAddr};

use crate::{acpi::ACPI_PLATFORM, ahci, apic::get_lapic_id, interrupts::InterruptIndex, mmio::MmioRegister, paging::PHYSICAL_MEMORY_OFFSET, serial_println};


pub enum PciBarKind {
    Io {
        port : u16,
    },
    Memory {
        address : PhysAddr,
        size : u64,
        prefetchable : bool,
    },
}

pub struct PciBar {
    idx: u8,
    pub kind : PciBarKind,
}

pub struct PcieDevice {
    bus : u8,
    dev : u8,
    function : u8,

    base_addr : PhysAddr,
    start_bus : u8,

    vendor_id: u16,
    device_id : u16,
    class_code : u8,
    subclass : u8,
    prog_if : u8,

    header_type : u8,
    bars : ArrayVec<PciBar, 6>,
}

impl PcieDevice {
    pub fn get_common_header(&self) -> &'static PciCommonHeader {
        get_common_header(self.base_addr, self.start_bus, self.bus, self.dev, self.function)
    }

    pub fn get_type_0_header(&self) -> &'static PciType0Header {
        get_type_0_header(self.base_addr, self.start_bus, self.bus, self.dev, self.function)
    }

    pub fn get_bar(&self, idx : u8) -> &PciBar {
        debug_assert!(idx < 6);

        self.bars.iter().find(|&bar| bar.idx == idx).unwrap()
    }
}

#[repr(C)]
pub struct PciCommonHeader {
    pub vendor_id: MmioRegister<u16>,       // 0x00
    pub device_id: MmioRegister<u16>,       // 0x02

    pub command: MmioRegister<u16>,         // 0x04
    pub status: MmioRegister<u16>,          // 0x06

    pub revision_id: MmioRegister<u8>,      // 0x08
    pub prog_if: MmioRegister<u8>,          // 0x09
    pub subclass: MmioRegister<u8>,         // 0x0A
    pub class_code: MmioRegister<u8>,       // 0x0B

    pub cache_line_size: MmioRegister<u8>,  // 0x0C
    pub latency_timer: MmioRegister<u8>,    // 0x0D
    pub header_type: MmioRegister<u8>,      // 0x0E
    pub bist: MmioRegister<u8>,             // 0x0F
}

#[repr(C)]
struct PciType0Header {
    common: PciCommonHeader,

    bars: [MmioRegister<u32>; 6],          // 0x10..0x27
    cardbus_cis_ptr: MmioRegister<u32>,    // 0x28
    subsystem_vendor_id: MmioRegister<u16>,// 0x2C
    subsystem_id: MmioRegister<u16>,       // 0x2E
    expansion_rom_base: MmioRegister<u32>, // 0x30
    capabilities_ptr: MmioRegister<u8>,    // 0x34
    _reserved_35: [u8; 7],                 // 0x35..0x3B
    interrupt_line: MmioRegister<u8>,      // 0x3C
    interrupt_pin: MmioRegister<u8>,       // 0x3D
    min_grant: MmioRegister<u8>,           // 0x3E
    max_latency: MmioRegister<u8>,         // 0x3F
}

#[repr(C)]
struct PciCapabilityHeader {
    id: MmioRegister<u8>,
    next: MmioRegister<u8>,
}

#[repr(C)]
struct MsiCapabilityHeader {
    id: MmioRegister<u8>,              // +0x00
    next: MmioRegister<u8>,            // +0x01
    message_control: MmioRegister<u16>,// +0x02
    message_address_low: MmioRegister<u32>,
}

#[repr(C)]
struct MsiCapability32 {
    header: MsiCapabilityHeader,
    message_data: MmioRegister<u16>, // +0x08
}

#[repr(C)]
struct MsiCapability64 {
    header: MsiCapabilityHeader,
    message_address_high: MmioRegister<u32>, // +0x08
    message_data: MmioRegister<u16>,         // +0x0C
}

const PCI_CAP_ID_MSI: u8 = 0x05;

const PCI_STATUS_CAPABILITIES_LIST : u16 = 1 << 4;

fn get_addr_space_addr(base_addr : PhysAddr) -> VirtAddr {
    // TODO : maybe map manually the pages as not cached instead of using the physmap, which is cached
    let virt_base_addr = PHYSICAL_MEMORY_OFFSET + base_addr.as_u64();
    virt_base_addr
}

fn get_header_at<T>(base_addr : PhysAddr, start_bus: u8, bus : u8, dev : u8, function : u8) -> &'static T {
    debug_assert!(bus >= start_bus);
    debug_assert!(dev < 32);
    debug_assert!(function < 8);

    let virt_base_addr = get_addr_space_addr(base_addr);
    let addr = virt_base_addr + (((bus -start_bus) as u64) << 20) + ((dev as u64) << 15) + ((function as u64) << 12);
    unsafe {
        &*addr.as_ptr::<T>()
    }
}

fn get_common_header(base_addr : PhysAddr, start_bus: u8, bus : u8, dev : u8, function : u8) -> &'static PciCommonHeader {
    get_header_at::<PciCommonHeader>(base_addr, start_bus, bus, dev, function)
}

fn get_type_0_header(base_addr : PhysAddr, start_bus: u8, bus : u8, dev : u8, function : u8) -> &'static PciType0Header {
    get_header_at::<PciType0Header>(base_addr, start_bus, bus, dev, function)
}

fn create_normal_device(bars : &mut ArrayVec<PciBar, 6>, base_addr : PhysAddr, start_bus : u8, bus : u8, dev : u8, function : u8){    
    let header_type0 = get_type_0_header(base_addr, start_bus, bus, dev, function);
    let mut idx = 0 as u8;
    while idx < header_type0.bars.len() as u8 {
        let bar_ref = &header_type0.bars[idx as usize];
        let bar = bar_ref.read();
        if bar == 0 {
            // unused
        } else if bar & 1 != 0 {
            // IO port BAR
            let port = (bar & !0x3) as u16;
            bars.push(PciBar { 
                idx, 
                kind: PciBarKind::Io { port }, 
            });
        } else {
            // memory BAR
            let mem_type = (bar >> 1) & 0x3;
            let prefetchable = bar & (1 << 3) != 0;
            // TODO : if prefetchable is false, would not not cached memory, for now just ignore it, then map the memory in a non cached way

            let current_idx = idx;
            let (phys, size) = match mem_type {
                0x0 => {
                    // 32-bit MMIO BAR
                    let phys = bar & !0xf;
                    let phys = PhysAddr::new(phys as u64);
                            
                    // find the size, by writing 0xffff_ffff, then looking at how it is corrected to be aligned
                    bar_ref.write(0xffff_ffff);
                    let aligned = bar_ref.read();
                    bar_ref.write(bar);
                    let mask = aligned & !0xf; // mask the flags bits
                    debug_assert!(mask != 0);
                    let size = (!mask).wrapping_add(1) as u64;
                    (phys, size)
                }
                0x2 => {
                    // 64-bit BAR
                    let high_idx = (idx + 1) as usize;
                    let high_bar = header_type0.bars[high_idx].read();
                    let phys_low = (bar as u64) & 0xffff_fff0;
                    let phys_high = high_bar as u64;
                            
                    let phys = (phys_high << 32) | phys_low;
                    let phys = PhysAddr::new(phys);
                            
                    bar_ref.write(0xffff_ffff);
                    header_type0.bars[high_idx].write(0xffff_ffff);
                    let aligned_low = bar_ref.read() as u64;
                    let aligned_high = header_type0.bars[high_idx].read() as u64;
                    bar_ref.write(bar);
                    header_type0.bars[high_idx].write(high_bar);
                            
                    let mask = (aligned_high << 32) | (aligned_low & !0xf);
                    let size = (!mask).wrapping_add(1);

                    idx += 1;
                    (phys, size)
                },
                _ => {
                    panic!("unsupported/reserved/legacy pcie bar");
                }
            };
            bars.push(PciBar { 
                idx: current_idx, 
                kind: PciBarKind::Memory { 
                    address: phys,
                    size, 
                    prefetchable,
                }, 
            });
        }
        idx += 1;
    }
}

fn create_device(header : &PciCommonHeader, base_addr : PhysAddr, start_bus : u8, bus : u8, dev : u8, function : u8, header_type : u8) -> PcieDevice {
    let mut bars = ArrayVec::new();
    let layout = header_type & 0x7f;
    match layout {
        0x00 => {
            // normal device
            create_normal_device(&mut bars, base_addr, start_bus, bus, dev, function);
        }
        0x01 => {
            // PCI-to-PCI bridge
            todo!() // TODO
        }
        0x02 => panic!("CardBus pci/pcie device not supported"),
        _ => {}
    }

    PcieDevice {
        bus,
        dev,
        function,
        base_addr,
        start_bus,
        vendor_id: header.vendor_id.read(),
        device_id: header.device_id.read(),
        class_code: header.class_code.read(),
        subclass: header.subclass.read(),
        prog_if: header.prog_if.read(),
        header_type,
        bars,
    }
}


const MSI_ENABLE: u16 = 1 << 0;
const MSI_64BIT_CAPABLE: u16 = 1 << 7;

pub fn enable_msi(device : &PcieDevice, vector : InterruptIndex){
    let header_type0 = device.get_type_0_header();
    let has_capabilities = header_type0.common.status.read() & PCI_STATUS_CAPABILITIES_LIST != 0;
    // TODO, instead of this, return a err if capabilites list is not supported ?
    if has_capabilities {
        // TODO : make the searching into the capabilites generic using an iterator
        let mut offset = header_type0.capabilities_ptr.read() & !0b11;
        let config_addr = VirtAddr::from_ptr(header_type0 as *const PciType0Header);
        while offset != 0 {
            let pci_capability = unsafe { &*(config_addr + offset as u64).as_ptr::<PciCapabilityHeader>() };
            if pci_capability.id.read() == PCI_CAP_ID_MSI {
                break;
            }
            offset = pci_capability.next.read() & !0b11;
        }
        let has_msi = offset != 0;
        if !has_msi {
            panic!("msi unsupported");
        }
        let msi_addr: VirtAddr = config_addr + offset as u64;
        let msi_capability = unsafe { &*(msi_addr).as_ptr::<MsiCapabilityHeader>() };
        let is_64_bit = msi_capability.message_control.read() & MSI_64BIT_CAPABLE != 0;
        let apic_id = get_lapic_id();
        let message_address = 0xfee0_0000u32 | ((apic_id as u32) << 12);
        let message_data = vector as u16;

        msi_capability.message_address_low.write(message_address);

        if is_64_bit {
            let msi64 = unsafe {
                &*msi_addr.as_ptr::<MsiCapability64>()
            };
            msi64.message_address_high.write(0);
            msi64.message_data.write(message_data);
        } else {
            let msi32 = unsafe {
                &*msi_addr.as_ptr::<MsiCapability32>()  
            };
            msi32.message_data.write(message_data);
        }
        let mut control = msi_capability.message_control.read();

        // Request only one MSI message.
        control &= !(0b111 << 4);
        control |= MSI_ENABLE;
        msi_capability.message_control.write(control);
    }
}

fn init_device(device : &PcieDevice){

    match (device.class_code, device.subclass, device.prog_if){
        (0x01, 0x06, 0x01) => {
            ahci::init(device);
        }
        // TODO : (0x01, 0x08, 0x02) = nvme
        // TODO : (0x0c, 0x03, 0x30) = xhci/usb
        // TODO : (0x02, 0x00, _) = ethernet
        _ => serial_println!("unsupported device {} {} {}", device.class_code, device.subclass, device.prog_if), // TODO : should I panic ?
    }
}

static PCIE_DEVICES : Once<Vec<PcieDevice>> = Once::new();

 // TODO instead of brute force scanning, use the recusive scan method

pub fn init_pcie(){
    let acpi_platform = ACPI_PLATFORM.get().unwrap();
    let mcfg = acpi_platform.tables.find_table::<Mcfg>().unwrap();
    let mut pcie_devices = Vec::new();
    for entry in mcfg.entries(){
        let base_address = entry.base_address;
        let segment_group = entry.pci_segment_group;
        serial_println!("pcie conf space, base address = {:#x}, in pci segment group {}, from bus number {} to {}", base_address, segment_group, entry.bus_number_start, entry.bus_number_end);
        let base_address = PhysAddr::new(base_address);
        for bus in entry.bus_number_start..=entry.bus_number_end {
            for dev in 0..32 {

                let header = get_common_header(base_address, entry.bus_number_start, bus, dev, 0);
                let vendor_id = header.vendor_id.read();
                if vendor_id == 0xffff {
                    continue;
                }
                serial_println!("pcie device bus {} dev {} found (function 0), {:04x}:{:04x} class {:02x}:{:02x}:{:02x}", bus, dev, header.vendor_id.read(), header.device_id.read(), header.class_code.read(), header.subclass.read(), header.prog_if.read());
                
                let header_type = header.header_type.read();
                let has_multiple_functions = header_type & 0x80 != 0;

                let pcie_device = create_device(header, base_address, entry.bus_number_start, bus, dev, 0, header_type);
                pcie_devices.push(pcie_device);
                
                if  has_multiple_functions {
                    for function in 1..8 {
                        let header = get_common_header(base_address, entry.bus_number_start, bus, dev, function);
                        let vendor_id = header.vendor_id.read();
                        if vendor_id == 0xffff {
                            continue;
                        }
                        let header_type = header.header_type.read();
                        serial_println!("pcie device bus {} dev {}, function {} found, {:04x}:{:04x} class {:02x}:{:02x}:{:02x}", bus, dev, function, header.vendor_id.read(), header.device_id.read(), header.class_code.read(), header.subclass.read(), header.prog_if.read());
                        let pcie_device = create_device(header, base_address, entry.bus_number_start, bus, dev, function, header_type);
                        pcie_devices.push(pcie_device);
                    }
                }
            }
        }
    }

    for device in &pcie_devices {
        init_device(device);
    }

    PCIE_DEVICES.call_once(|| pcie_devices);
    
}