# ABI

syscall nb : rax
arg 1 : rdi
arg 2 : rsi
arg 3 : rdx
arg 4 : r10
arg 5 : r8
arg 6 : r9

# types in syscall list
! = never type (never returns)
Other types are standard C types

# Syscalls list

## exit - syscall 0
! exit(uint64_t code)

Returns:  
    nothing (never returns)  

Description:
Exits the current process with the code given.

Description:
Prints to the standard output of the terminal.

## exec - syscall 1

uint64_t exec(const uint8_t* path_ptr, uint64_t path_len, const uint8_t args_ptr, uint64_t args_len)

Returns:  
    0xFFFFFFFFFFFFFFFF if error  
    pid of created process if success  

Description:
Creates a process, launch it and return the pid.

## wait_pid - syscall 2

TODO

## stat - syscall 3

TODO

## open - syscall 4

TODO

## close - syscall 5

TODO

## get_cwd - syscall 6

TODO

## get_dir_children - syscall 7

TODO

## sbrk - syscall 8

TODO

## shutdown - syscall 9

TODO

## change_cwd - syscall 10

TODO

## fstat - syscall 11

TODO

## read - syscall 12

TODO

## write - syscall 13