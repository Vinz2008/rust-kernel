#![no_std]
#![no_main]

use rt::{self as _, Args, alloc::{string::String, vec}, arrayvec::ArrayVec, input::Reader, print, println, shared_consts::BACKSPACE_BYTE, syscall::{syscall_change_cwd, syscall_exec, syscall_get_cwd, syscall_stat, syscall_wait_pid}};

// TODO : add a separate command for clear (use a separate exe ?), just prints a code to be handled by the cli driver

fn handle_cli(state : &CliState){
    let mut cli_split = state.cli_line.split_whitespace(); // TODO : better parsing, for ex with quotess
    let command_name = cli_split.next();
    let command_name = match command_name {
        Some(cmd_name) => cmd_name,
        None => {
            return;
        }
    };
    match command_name {
        "echo" => {
            let mut first = true;
            for cli_part in cli_split {
                if first {
                    first = false;
                } else {
                    print!(" ");
                }
                print!("{}", cli_part);
            }
            println!();
        }
        "pwd" => {
            let cwd = syscall_get_cwd().unwrap();
            println!("{}", cwd);
        }
        "cd" => {
            let dir = match cli_split.next(){
                Some(dir) => dir,
                None => {
                    println!("expected dir after cd");
                    return;
                }
            };
            syscall_change_cwd(dir);
        }
        cmd_name => {
            // TODO : handle paths for exes
            let mut path = String::new();
            path.push_str("/bin/");
            path.push_str(cmd_name);
            match syscall_stat(&path){
                Some(_) => {
                    let mut argv = vec![path.as_str()];
                    argv.extend(cli_split);
                    let pid = syscall_exec(&path, &argv).unwrap();
                    syscall_wait_pid(pid);
                },
                None => println!("unknown command : {}", &state.cli_line),
            }
            
        },
    }
}


fn handle_escape(state : &mut CliState){
    let c = Reader::read_byte();
    if c != b'[' {
        // TODO : maybe use a state machine instead like in vga.rs in the kernel ?
        // TODO : pass this c in the normal handling (could be for ex ESC\n, so would need to handle it, or ESCa) 
        return;
    }
    let mut csi_buf = ArrayVec::<u8, 16>::new();
    let mut csi_c = Reader::read_byte();
    while !(0x40..=0x7e).contains(&csi_c){
        csi_buf.push(csi_c); // TODO : use try_push and return error if not enough size
        csi_c = Reader::read_byte();
    }
    csi_buf.push(csi_c);
    match csi_buf.as_ref() {
        b"D" => {
            if state.cursor == 0 {
                return;
            }
            // arrow left (TODO : handle the pattern xD for ex 2D for moving 2 times left ?)
            // TODO : move left in the representation
            print!("\x1B[D");
            print::flush_stdout().unwrap();
            state.cursor -= 1;
        }, 
        b"C" => {
            // arrow right
            if state.cursor >= state.cli_line.len() {
                return;
            }
            print!("\x1B[C");
            print::flush_stdout().unwrap();
            state.cursor += 1;
        }
        b"B" => {
            // arrow down (TODO)
        }
        b"A" => {
            // arrow up (TODO)
        }
        _ => {}
    }
}

struct CliState {
    cli_line : String,
    last_cli : Option<String>,
    cursor : usize,
}

fn redraw_cli(state : &CliState){
    print!("\r> {}", state.cli_line);
    print!("\x1B[K");

    let move_left = state.cli_line.len() - state.cursor;

    for _ in 0..move_left {
        print!("\x1B[D");
    }

    print::flush_stdout().unwrap();
}

#[unsafe(no_mangle)]
pub extern "Rust" fn main(_args : Args<'_>) -> i32 {
    let mut cli_state = CliState {
        cli_line: String::new(),
        last_cli: None, // TODO : use this (need to handle the arrow up in the kernel, need to pass an ansi sequence to the userspace, and handle it here)
        cursor: 0,
    };
    
    print!("> ");
    print::flush_stdout().unwrap(); // TODO : use stderr instead of flushing ?
    

    loop {
        let c = Reader::read_byte();
        match c {
            b'\x1B' => {
                handle_escape(&mut cli_state);
            }
            b'\n' => {
                //println!("\nentered : {}", &cli);
                println!();
                handle_cli(&cli_state);
                print!("> ");
                print::flush_stdout().unwrap();
                cli_state.last_cli = Some(cli_state.cli_line.clone());
                cli_state.cli_line.clear();
                cli_state.cursor = 0;
            },
            BACKSPACE_BYTE => {
                if cli_state.cursor > 0 {
                    cli_state.cursor -= 1;
                    cli_state.cli_line.remove(cli_state.cursor);

                    redraw_cli(&cli_state);
                }
            }
            _ => {
                // TODO : do I need to decode utf8 instead ?
                let c_char = c as char;
                cli_state.cli_line.insert(cli_state.cursor, c_char);
                cli_state.cursor += 1;
                redraw_cli(&cli_state);
            }
        }
        
    }
}