use core::fmt::{self, Write};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

use uefi::proto::console::gop::PixelFormat;
use font8x8::legacy::BASIC_LEGACY;

#[derive(Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: usize,
    pub size: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub pixel_format: PixelFormat,
}

pub struct Printer;

static mut FRAMEBUFFER_INFO : UnsafeCell<Option<FramebufferInfo>> = UnsafeCell::new(None);

static X_POS: AtomicUsize = AtomicUsize::new(0);
static Y_POS: AtomicUsize = AtomicUsize::new(0);

pub unsafe fn put_pixel(fb: &FramebufferInfo, x: usize, y: usize, r: u8, g: u8, b: u8) {
    if x >= fb.width || y >= fb.height {
        return;
    }

    let offset = (y * fb.stride + x) * 4;

    if offset + 3 >= fb.size {
        return;
    }

    let ptr = (fb.addr + offset) as *mut u8;

    match fb.pixel_format {
        PixelFormat::Rgb => {
            ptr.add(0).write_volatile(r);
            ptr.add(1).write_volatile(g);
            ptr.add(2).write_volatile(b);
            ptr.add(3).write_volatile(0);
        }

        PixelFormat::Bgr => {
            ptr.add(0).write_volatile(b);
            ptr.add(1).write_volatile(g);
            ptr.add(2).write_volatile(r);
            ptr.add(3).write_volatile(0);
        }

        _ => {}
    }
}

pub fn write_char(framebuffer: &FramebufferInfo, x: usize, y: usize, c: char,) {
    let index = if c.is_ascii() {
        c as usize
    } else {
        b'?' as usize
    };

    let glyph = BASIC_LEGACY[index];

    for (row, bits) in glyph.iter().copied().enumerate() {
        for col in 0..8 {
            if bits & (1 << col) != 0 {
                unsafe { put_pixel(framebuffer, x + col, y + row, 255, 255, 255); }
            }
        }
    }
}

pub fn write_string(framebuffer: &FramebufferInfo, mut x: usize, mut y: usize, s: &str){
    for c in s.chars() {
        match c {
            '\n' => {
                x = 0;
                y += 8;
            }

            '\r' => {
                x = 0;
            }

            c => {
                write_char(framebuffer, x, y, c);
                x += 8;

                if x + 8 > framebuffer.width {
                    x = 0;
                    y += 8;
                }
            }
        }
        X_POS.store(x, Ordering::Relaxed);
        Y_POS.store(y, Ordering::Relaxed);

        if y + 8 > framebuffer.height {
            break;
        }
    }
}

impl Printer {
    pub fn init(infos : FramebufferInfo){
        unsafe {
            *FRAMEBUFFER_INFO.get() = Some(infos);
        }
    }

    pub fn clear_screen(&self) {
        let infos = unsafe { &*FRAMEBUFFER_INFO.get() }.as_ref().unwrap();
        for x in 0..infos.width {
            for y in 0..infos.height {
                unsafe {
                    put_pixel(infos, x, y, 0, 0, 0);
                }
            }
        }
    }
}

impl Write for Printer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let infos = unsafe { &*FRAMEBUFFER_INFO.get() }.as_ref().unwrap();
        let x = X_POS.load(Ordering::Relaxed);
        let y = Y_POS.load(Ordering::Relaxed);
        write_string(infos, x, y, s);
        fmt::Result::Ok(())
    }
}
