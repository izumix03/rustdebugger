// use crate::Helper::DynError;
// use nix::{
//     libc::user_regs_struct,
//     sys::{
//         personality::{self, Persona},
//         ptrace,
//         wait::{waitpid, WaitStatus},
//     },
//     unistd::{execvp, fork, ForkResult, Pid},
// };
// use std::ffi::{c_void, CString};
use nix::unistd::Pid;
use std::ffi::c_void;

/// デバッガ内の情報
pub struct DbgInfo {
    pid: Pid,
    // Breakpoint address
    brk_addr: Option<*mut c_void>,
    // Breakpoint を設定した memory の元の値
    brk_val: i64,
    // 実行ファイル
    filename: String,
}

/// デバッガ
/// ZDbg<Running> は子プロセス実行中
/// ZDbg<NotRunning> は子プロセス実行していない
pub struct ZDbg<T> {
    info: Box<DbgInfo>,
    _state: T,
}

/// デバッガの状態
pub struct Running;
pub struct NotRunning;

/// デバッガの状態の列挙型表現。Exitの場合、終了
pub enum State {
    Running(ZDbg<Running>),
    NotRunning(ZDbg<NotRunning>),
    Exit,
}

