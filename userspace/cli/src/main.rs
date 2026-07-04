#![no_std]
#![no_main]

use arrayvec::ArrayString;
use rt::{self as _, print, println, syscall::{syscall_get_char, syscall_print}};
use shared_consts::BACKSPACE;


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
        _ => println!("unknown command : {}", cli),
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