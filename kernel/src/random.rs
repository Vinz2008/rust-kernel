use core::arch::x86_64::_rdseed64_step;

use raw_cpuid::CpuId;
use spin::{Mutex, Once};

struct KernelRng {
    key: [u8; 32],
    counter: u64,
}

const CONSTANTS: [u32; 4] = [
    0x6170_7865,
    0x3320_646e,
    0x7962_2d32,
    0x6b20_6574,
];

fn quarter_round(mut a: u32, mut b: u32, mut c: u32, mut d: u32) -> (u32, u32, u32, u32) {
    a = a.wrapping_add(b);
    d ^= a;
    d = d.rotate_left(16);

    c = c.wrapping_add(d);
    b ^= c;
    b = b.rotate_left(12);

    a = a.wrapping_add(b);
    d ^= a;
    d = d.rotate_left(8);

    c = c.wrapping_add(d);
    b ^= c;
    b = b.rotate_left(7);

    (a, b, c, d)
}

fn quarter_round_state(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize){
    let (new_a, new_b, new_c, new_d) = quarter_round(state[a], state[b], state[c], state[d]);

    state[a] = new_a;
    state[b] = new_b;
    state[c] = new_c;
    state[d] = new_d;
}

fn double_round(state: &mut [u32; 16]) {
    // columns
    quarter_round_state(state, 0, 4, 8, 12);
    quarter_round_state(state, 1, 5, 9, 13);
    quarter_round_state(state, 2, 6, 10, 14);
    quarter_round_state(state, 3, 7, 11, 15);

    // diagonals
    quarter_round_state(state, 0, 5, 10, 15);
    quarter_round_state(state, 1, 6, 11, 12);
    quarter_round_state(state, 2, 7, 8, 13);
    quarter_round_state(state, 3, 4, 9, 14);
}

fn make_state(key: &[u8; 32], counter: u64) -> [u32; 16]{
    let mut state = [0; 16];
    state[0..4].copy_from_slice(&CONSTANTS);
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([
            key[i*4],
            key[i*4 + 1],
            key[i*4 + 2],
            key[i*4 + 3],
        ]);
    }
    state[12] = counter as u32;
    state[13] = (counter >> 32) as u32;

    state[14] = 0;
    state[15] = 0;

    state
}

fn chacha20_block(key: &[u8; 32], counter: u64) -> [u8; 64] {
    let initial = make_state(key, counter);
    let mut working = initial;
    for _ in 0..10 {
        double_round(&mut working);
    }

    for i in 0..16 {
        working[i] = working[i].wrapping_add(initial[i]);
    }
    let mut output = [0; 64];
    for i in 0..16 {
        output[i * 4 .. i * 4 + 4].copy_from_slice(&working[i].to_le_bytes());
    }

    output
}

fn rdseed64() -> Option<u64> {
    let mut value = 0u64;

    let success = unsafe {
        _rdseed64_step(&mut value)
    };

    if success == 1 {
        Some(value)
    } else {
        None
    }
}

fn retry_rdseed64() -> Option<u64 >{
    for _ in 0..64 {
        if let Some(value) = rdseed64(){
            return Some(value);
        }
        core::hint::spin_loop();
    }
    None
}

static KERNEL_RNG : Once<Mutex<KernelRng>> = Once::new();

pub fn init_kernel_rng(){
    let mut seed = [0; 32];
    let cpu_id = CpuId::new();
    if !cpu_id.get_extended_feature_info().is_some_and(|features| features.has_rdseed()){
        panic!("rsseed unsupported, couldn't seed the rng");
    }

    for chunk in seed.chunks_exact_mut(8) {
        let value = retry_rdseed64().expect("rdseed retry failed, couldn't seed the rng");
        chunk.copy_from_slice(&value.to_le_bytes());
    }

    // TODO : also mix rdrand, maybe interrupt timing, device timing, etc (for some, need to reseed for this)
    
    let kernel_rng = KernelRng { key: seed, counter: 0 };
    KERNEL_RNG.call_once(|| Mutex::new(kernel_rng));
}

pub fn random_bytes(out : &mut [u8]){
    KERNEL_RNG.get().unwrap().lock().fill(out);
}

pub fn random_u64() -> u64 {
    let mut buf = [0; 8];
    random_bytes(&mut buf);
    u64::from_ne_bytes(buf)
}

impl KernelRng {
    fn fill(&mut self, output: &mut [u8]) {
        for chunk in output.chunks_mut(64) {
            let block = chacha20_block(&self.key, self.counter);
            self.counter = self.counter.checked_add(1).expect("ChaCha20 counter exhausted");

            chunk.copy_from_slice(&block[..chunk.len()]);
        }
        self.rekey();
    }

    fn rekey(&mut self){
        let block = chacha20_block(&self.key, self.counter);
        self.key.copy_from_slice(&block[..32]);
        self.counter = 0;
    }
}

// TODO : fix tests
#[test_case]
fn test_quarter_round() {
    let result = quarter_round(0x1111_1111, 0x0102_0304, 0x9b8d_6f43, 0x0123_4567,);

    assert_eq!(
        result,
        (
            0xea2a_92f4,
            0xcb1c_f8ce,
            0x4581_472e,
            0x5881_c4bb,
        )
    );
}