#![no_std]
#![no_main]

use arrayvec::ArrayString;
use rt::{self as _, print, println, shared_consts::BACKSPACE, syscall::{syscall_exec, syscall_get_char, syscall_print, syscall_stat, syscall_wait_pid, PATH_MAX}};

fn handle_cli(cli : &str){
    let mut cli_split = cli.split_whitespace();
    let command_name = cli_split.next();
    let command_name = match command_name {
        Some(cmd_name) => cmd_name,
        None => {
            println!("empty command");
            return;
        }
    };
    //println!("command name : {}", command_name);
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
        cmd_name => {
            let mut path = ArrayString::<PATH_MAX>::new();
            path.push_str("/");
            path.push_str(cmd_name);
            match syscall_stat(&path){
                Some(_) => {
                    let pid = syscall_exec(&path);
                    syscall_wait_pid(pid);
                },
                None => println!("unknown command : {}", cli),
            }
            
        },
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn main() -> i32 {
    let mut cli : ArrayString<10000> = ArrayString::new();
    syscall_print("> ");

    loop {
        let c = syscall_get_char();
        match c {
            '\n' => {
                //println!("\nentered : {}", &cli);
                println!();
                handle_cli(&cli);
                print!("> ");
                cli.clear();
            },
            BACKSPACE => {
                cli.pop();
                print!("{}", BACKSPACE);
            }
            _ => {
                if let Ok(_) = cli.try_push(c) {
                    print!("{}", c);
                }
            }
        }
        
    }
}