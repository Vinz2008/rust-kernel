use shared_consts::RNG_SEED_SIZE;
use spin::Mutex;

use crate::syscall::syscall_get_random;

// TODO : instead, use the kernel random only as a seed, then use a normal algorithm (use the same chacha20 ? maybe a smaller chacha ? another algorithm ?)

pub fn kernel_random_u64() -> u64 {
    let mut buf = [0; 8];
    syscall_get_random(&mut buf);
    u64::from_ne_bytes(buf)
}

static RNG_SEED : Mutex<[u8; RNG_SEED_SIZE]> = Mutex::new([0; RNG_SEED_SIZE]);

pub fn init_rng_seed(rng_bytes : &[u8]){
    let mut rng_seed_lock = RNG_SEED.lock();
    rng_seed_lock.copy_from_slice(&rng_bytes[..RNG_SEED_SIZE]);
}