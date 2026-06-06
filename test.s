
initrd/init:     format de fichier elf64-x86-64


Déassemblage de la section .text :

0000000000201120 <_start>:


static mut TEST_BSS: [u8; 4096] = [0; 4096];

#[unsafe(no_mangle)]
pub fn _start() {
  201120:	55                   	push   %rbp
  201121:	48 89 e5             	mov    %rsp,%rbp
    main();
  201124:	e8 07 00 00 00       	call   201130 <main>
    loop {} // TODO : after having syscalls, add the exit syscall here
  201129:	eb fe                	jmp    201129 <_start+0x9>
  20112b:	cc                   	int3
  20112c:	cc                   	int3
  20112d:	cc                   	int3
  20112e:	cc                   	int3
  20112f:	cc                   	int3

0000000000201130 <main>:
fn main() -> i32 {
  201130:	55                   	push   %rbp
  201131:	48 89 e5             	mov    %rsp,%rbp
}
  201134:	31 c0                	xor    %eax,%eax
  201136:	5d                   	pop    %rbp
  201137:	c3                   	ret
