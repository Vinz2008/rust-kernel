use core::fmt::{self, Display};

use spin::{Mutex, Once};
use x86_64::instructions::port::{PortReadOnly, PortWriteOnly};

struct Rtc {
    port_addr :PortWriteOnly<u8>,
    port_data : PortReadOnly<u8>,
}

const CMOS_ADDR_PORT : u16 = 0x70;
const CMOS_DATA_PORT : u16 = 0x71;

impl Rtc {
    const fn new() -> Rtc {
        let port_addr = PortWriteOnly::<u8>::new(CMOS_ADDR_PORT);
        let port_data = PortReadOnly::<u8>::new(CMOS_DATA_PORT);
        Rtc {
            port_addr,
            port_data,
        }
    }

    fn read(&mut self, reg : u8) -> u8 {
        unsafe {
            self.port_addr.write(reg);
            return self.port_data.read();
        }
    }
}

static RTC : Mutex<Rtc> = Mutex::new(Rtc::new());

fn is_update_in_progress(rtc : &mut Rtc) -> bool {
    (rtc.read(0x0A) & 0x80) != 0
}

#[inline(always)]
pub fn cpu_relax() {
    core::hint::spin_loop();
}

fn wait_for_update(rtc : &mut Rtc){
    while is_update_in_progress(rtc){
        cpu_relax();
    }
}

#[derive(Clone, PartialEq)]
pub struct RTCTime {
    second : u8,
    minute : u8,
    hour : u8,
    day : u8,
    month : u8,
    year : u32,
}


impl Display for RTCTime {
	/// Prints a `RTCDateTime` formatted according to the [ISO 8601](https://en.wikipedia.org/wiki/ISO_8601) standard.
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}-{}-{}T{}:{}:{}Z", self.year, self.month, self.day, self.hour, self.minute, self.second)
	}
}

const REG_SECOND : u8 = 0x00;
const REG_MINUTE : u8 = 0x02;
const REG_HOUR : u8 = 0x04;
const REG_DAY : u8 = 0x07;
const REG_MONTH : u8 = 0x08;
const REG_YEAR : u8 = 0x09;


// TODO : add a date command

// TODO : add timezone support (use an offset)

// TODO : read century (need to find it with acpi)


fn read_rtc_snapshot(rtc : &mut Rtc, rtc_time : &mut RTCTime){
    wait_for_update(rtc);

    rtc_time.second = rtc.read(REG_SECOND);
    rtc_time.minute = rtc.read(REG_MINUTE);
    rtc_time.hour = rtc.read(REG_HOUR);
    rtc_time.day = rtc.read(REG_DAY);
    rtc_time.month = rtc.read(REG_MONTH);
    rtc_time.year = rtc.read(REG_YEAR) as u32;
    // TODO : handle century
}

fn read_rtc() -> RTCTime {
    let mut rtc_lock = RTC.lock();
    let mut rtc_time = RTCTime { second: 0, minute: 0, hour: 0, day: 0, month: 0, year: 0 };

    read_rtc_snapshot(&mut rtc_lock, &mut rtc_time);

    let last_rtc_time = rtc_time.clone();
    let mut is_first = true;
    while is_first || last_rtc_time != rtc_time {
        is_first = false;
        read_rtc_snapshot(&mut rtc_lock, &mut rtc_time);
    }

    let register_b = rtc_lock.read(0x0B);
    let is_bcd = (register_b & 0x04) == 0;
    if is_bcd {
        rtc_time.second = (rtc_time.second & 0x0F) + ((rtc_time.second / 16) * 10);
        rtc_time.minute = (rtc_time.minute & 0x0F) + ((rtc_time.minute / 16) * 10);
        rtc_time.hour = ( (rtc_time.hour & 0x0F) + (((rtc_time.hour & 0x70) / 16) * 10) ) | (rtc_time.hour & 0x80);
        rtc_time.day = (rtc_time.day & 0x0F) + ((rtc_time.day / 16) * 10);
        rtc_time.month = (rtc_time.month & 0x0F) + ((rtc_time.month / 16) * 10);
        rtc_time.year = (rtc_time.year & 0x0F) + ((rtc_time.year / 16) * 10);
        // TODO : handle century
    }

    let is_12_hours_clock = (register_b & 0x02) == 0 && (rtc_time.hour & 0x80) != 0;

    if is_12_hours_clock {
        rtc_time.hour = ((rtc_time.hour & 0x7F) + 12) % 24;
    }

    // TODO : use the century register instead ? for now use the const
    const THIS_YEAR : u32 = 2026;

    rtc_time.year += (THIS_YEAR / 100) * 100;
    if rtc_time.year < THIS_YEAR {
        rtc_time.year += 100;
    }

    rtc_time
}

pub static BOOT_TIME : Once<RTCTime> = Once::new();

pub fn init_rtc(){
    BOOT_TIME.call_once(|| read_rtc());
}