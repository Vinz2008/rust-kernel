use crate::{mmio::{MmioRegister, Reserved}, pcie::PcieDevice};

// TODO : finish this

// HBA registers

// TODO : maybe rename these fields for better readability ?
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

// TODO : use the checklist from here : https://wiki.osdev.org/AHCI (needs to also add code in the pcie part)
pub fn init(device : &PcieDevice){
    
}