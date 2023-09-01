use std::env;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use crate::dbg::{State, ZDbg};
use crate::helper::DynError;

mod dbg;
mod helper;

fn main() -> Result<(), DynError> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        let msg = format!("引数が必要です\n例: {} 実行ファイル [引数*]", args[0]).into();
        return Err(msg);
    }

    run_dbg(&args[1])?;
    Ok(())
}

fn run_dbg(filename: &str) -> Result<(), DynError> {
    let debugger = ZDbg::new(filename.to_string());
    let mut state = State::NotRunning(debugger);
    let mut rl = DefaultEditor::new()?;

    loop {
        match rl.readline("zdbg > ") {
            Ok(line) => {
                let trimmed = line.trim();
                let cmd: Vec<&str> = trimmed.split(" ")
                    .filter(|c| !c.is_empty()).collect();
                state = match state {
                    State::Running(r) => r.do_cmd(&cmd)?,
                    State::NotRunning(n) => n.do_cmd(&cmd)?,
                    _ => break,
                };
                // 代入？？
                if let State::Exit = state {
                    break;
                }
                let _ = rl.add_history_entry(line);
            }
            Err(ReadlineError::Interrupted) => {
                eprintln!("<<終了は CTRL-d>>");
            }
            _ => {
                if let State::Running(r) = state {
                    // 子プロセス実行中はkill
                    r.do_cmd(&["exit"])?;
                };
                break;
            }
        }
    }
    Ok(())
}