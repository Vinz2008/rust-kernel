pub struct KernelSymbol {
    addr : usize,
    name : &'static str,
}

include!(concat!(env!("OUT_DIR"), "/generated_symbols.rs"));

pub fn lookup_symbol(addr : usize) -> Option<(&'static str, usize)> {
    let ip = addr.saturating_sub(1);
    let symbols = KERNEL_SYMBOLS;

    let idx = symbols.partition_point(|sym| sym.addr <= ip);
    
    if idx == 0 {
        return None;
    }

    let sym = &symbols[idx -1];
    Some((sym.name, ip - sym.addr))
}