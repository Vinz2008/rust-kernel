#[repr(C)]
struct TarHeader {
    filename : [u8; 100],
    mode : [u8; 8],
    uid : [u8; 8],
    gid : [u8; 8],
    size : [u8; 12],
    mtime : [u8; 12],
    chksum : [u8; 8],
    typeflag : [u8; 1],
    linkname : [u8; 100],
    // ustar part
    magic : [u8; 6], // TODO : check magic
    version : [u8; 2],
    uname : [u8; 32],
    gname : [u8; 32],
    dev_major: [u8; 8],
    dev_minor: [u8; 8],
    prefix: [u8; 155],
    pad: [u8; 12],
}

enum TarError {
    InvalidUtf8,
    InvalidOctal,
}

fn trim_right_nul(buf : &[u8]) -> &[u8] {
    let end_pos = buf.iter().position(|e| *e == 0).unwrap_or(buf.len());
    &buf[..end_pos]
}

fn parse_octal(buf : &[u8]) -> Result<u64, TarError> {
    let mut res : u64 = 0;
    for &b in buf {
        match b {
            0 | b' ' => break,
            b'0'..=b'7' => {
                res = res * 8 + (b - b'0') as u64;
            }
            _ => return Err(TarError::InvalidOctal),
        }
    }
    Ok(res)
}

impl TarHeader {
    // filename is max 256 chars
    fn get_filename(&self) -> Result<&str, TarError> { 
        str::from_utf8(trim_right_nul(&self.filename)).map_err(|_| TarError::InvalidUtf8)
    }

    fn size(&self) -> Result<usize, TarError> {
        parse_octal(&self.size).map(|s| s as usize)
    }

    fn get_mode(&self) -> Result<u32, TarError> {
        parse_octal(&self.mode).map(|m| m as u32)
    }
}

/*struct TarHeader {
    filename : ArrayString<256>,
    mode : u32,
    uid : u32,
    gid : u32,
    size : u64,
    mtime : u64,
    chksum : u32,
}*/