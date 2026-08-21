#![no_std]
#![no_main]

use crate::memory_map::create_memory_map;
use crate::page_table::allocate_level4_page_table;
use crate::rsdp::find_rsdp;

use bootloader::bootinfo::MemoryMap;
use bootloader::printer::Printer;
use bootloader::{self, common_boot::bootloader_main, printer::FramebufferInfo};

use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::{Status, boot};

use x86_64::PhysAddr;
use x86_64::structures::paging::{PageSize, Size4KiB};

static KERNEL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/kernel.bin"));

mod memory_map;
mod page_table;
mod rsdp;

#[uefi::entry]
fn main() -> Status {
    uefi::helpers::init().expect("failed to initialize UEFI helpers");
    
    let image_handle = boot::image_handle();
    let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(image_handle).expect("failed to open LoadedImage");
    let (image_base, image_size) = loaded_image.info();
    let bootloader_start = PhysAddr::new(image_base as u64);
    let bootloader_end = bootloader_start + image_size;

    drop(loaded_image);

    let memory_map_pages_nb = size_of::<MemoryMap>().div_ceil(Size4KiB::SIZE as usize);

    let map_storage = boot::allocate_pages(boot::AllocateType::AnyPages, boot::MemoryType::LOADER_DATA, memory_map_pages_nb).expect("failed to allocate memory map storage");

    let new_p4_addr = boot::allocate_pages(boot::AllocateType::AnyPages, boot::MemoryType::LOADER_DATA, 1).expect("failed to allocate bootloader P4");


    let gop_handle = boot::get_handle_for_protocol::<GraphicsOutput>().expect("failed to find GOP");

    let mut gop = boot::open_protocol_exclusive::<GraphicsOutput>(gop_handle).expect("failed to open GOP");

    let mode = gop.current_mode_info();
    let (width, height) = mode.resolution();

    let mut fb = gop.frame_buffer();

    let framebuffer = FramebufferInfo {
        addr: fb.as_mut_ptr() as usize,
        size: fb.size(),
        width,
        height,
        stride: mode.stride(),
        pixel_format: mode.pixel_format(),
    };

    drop(fb);
    drop(gop);

    Printer::init(framebuffer);

    let rsdp = find_rsdp().expect("RSDP not found");

    let uefi_memory_map = unsafe { uefi::boot::exit_boot_services(None) };

    let memory_map = create_memory_map(uefi_memory_map, map_storage.as_ptr().cast::<MemoryMap>());

    let new_p4_phys = allocate_level4_page_table(new_p4_addr);

    bootloader_main(KERNEL, memory_map, None, None, bootloader_start, bootloader_end, new_p4_phys, Some(rsdp))
}