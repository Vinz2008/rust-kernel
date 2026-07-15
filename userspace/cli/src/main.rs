#![no_std]
#![no_main]

use rt::{self as _, Args, alloc::{string::String, vec}, print, println, shared_consts::BACKSPACE, syscall::{syscall_change_cwd, syscall_exec, syscall_get_char, syscall_get_cwd, syscall_print, syscall_stat, syscall_wait_pid}};

fn handle_cli(cli : &str){
    let mut cli_split = cli.split_whitespace(); // TODO : better parsing, for ex with quotess
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
            let mut path = String::new();
            path.push('/');
            path.push_str(cmd_name);
            match syscall_stat(&path){
                Some(_) => {
                    let mut argv = vec![path.as_str()];
                    argv.extend(cli_split);
                    let pid = syscall_exec(&path, &argv);
                    syscall_wait_pid(pid);
                },
                None => println!("unknown command : {}", cli),
            }
            
        },
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn main(_args : Args<'_>) -> i32 {
    let mut cli = String::new();
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
                cli.push(c);
                print!("{}", c);
            }
        }
        
    }
}