#![no_std]
#![no_main]

use rt::{self as _, Args, alloc::{string::String, vec}, input::Reader, print, println, shared_consts::BACKSPACE, syscall::{syscall_change_cwd, syscall_exec, syscall_get_cwd, syscall_stat, syscall_wait_pid}};

// TODO : add a separate command for clear (use a separate exe ?), just prints a code to be handled by the cli driver

fn handle_cli(cli : &str){
    let mut cli_split = cli.split_whitespace(); // TODO : better parsing, for ex with quotess
    let command_name = cli_split.next();
    let command_name = match command_name {
        Some(cmd_name) => cmd_name,
        None => {
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
                None => println!("unknown command : {}", cli),
            }
            
        },
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn main(_args : Args<'_>) -> i32 {
    // TODO : fix the problem with writing being always at the end of the printing and not at the cursor position
    let mut cli = String::new();
    let mut last_cli = None; // TODO : use this (need to handle the arrow up in the kernel, need to pass an ansi sequence to the userspace, and handle it here)
    print!("> ");
    print::flush_stdout().unwrap(); // TODO : use stderr instead of flushing ?
    

    loop {
        let c = Reader::read_char();
        match c {
            '\n' => {
                //println!("\nentered : {}", &cli);
                println!();
                handle_cli(&cli);
                print!("> ");
                print::flush_stdout().unwrap();
                last_cli = Some(cli.clone());
                cli.clear();
            },
            BACKSPACE => {
                if !cli.is_empty(){
                    cli.pop();
                    print!("{}", BACKSPACE);
                    print::flush_stdout().unwrap();
                }
            }
            _ => {
                cli.push(c);
                print!("{}", c);
                print::flush_stdout().unwrap();
            }
        }
        
    }
}