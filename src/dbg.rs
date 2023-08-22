use crate::helper::DynError;
use nix::sys::personality::{self, Persona};
use nix::{
    libc::user_regs_struct,
    sys::{
        ptrace,
        wait::{waitpid, WaitStatus},
    },
    unistd::{execvp, fork, ForkResult, Pid},
};
use std::ffi::{c_void, CString};

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

/// デバッガの状態、サイズは0
/// 幽霊型(phantom type)と呼ばれる
pub struct Running;

pub struct NotRunning;

/// デバッガの状態の列挙型表現。Exitの場合、終了
pub enum State {
    Running(ZDbg<Running>),
    NotRunning(ZDbg<NotRunning>),
    Exit,
}

/// Running と NotRunning の共通実装
impl<T> ZDbg<T> {
    /// ブレークポイントのアドレスを設定する関数。子プロセスのメモリ上には反映しない
    /// アドレス設定に成功した場合は true を返す
    fn set_break_addr(&mut self, cmd: &[&str]) -> bool {
        if self.info.brk_addr.is_some() {
            println!("ブレークポイントは設定済み: Addr = {:p}>>", self.info.brk_addr.unwrap());
            false
        } else if Some(addr) = get_break_addr(cmd) {
            self.info.brk_addr = Some(addr); // ブレークポイントのアドレスを設定
            true
        } else {
            false
        }
    }

    /// 共通コマンドの実行
    fn do_cmd_common(&self, cmd: &[&str]) {
        match cmd[0] {
            "help" | "h" => do_help(),
            _ => (),
        }
    }
}

/// NotRunning の実装
impl ZDbg<NotRunning> {
    pub fn new(filename: String) -> Self {
        ZDbg {
            info: Box::new(DbgInfo {
                pid: Pid::from_raw(0),
                brk_addr: None,
                brk_val: 0,
                filename,
            }),
            _state: NotRunning,
        }
    }

    pub fn do_cmd(mut self, cmd: &[&str]) -> Result<State, DynError> {
        if cmd.is_empty() {
            return Ok(State::NotRunning(self));
        }

        let _ = match cmd[0] {
            "run" | "r" => self.do_run(cmd),
            "break" | "b" => {
                self.do_break(cmd);
            }
            "exit" => return Ok(State::Exit),
            "continue" | "c" | "stepi" | "s" | "registers" | "regs" => {
                eprintln!("<<ターゲットを実行していません。run で実行してください>>")
            }
            _ => self.do_cmd_common(cmd),
        };

        Ok(State::NotRunning(self))
    }

    fn do_break(&mut self, cmd: &[&str]) -> bool {
        self.set_break_addr(cmd)
    }

    /// 子プロセスを生成し、成功した場合は Running に遷移する
    fn do_run(mut self, cmd: &[&str]) -> Result<State, DynError> {
        // 子プロセスに渡すコマンドライン引数
        let args: Vec<CString> = cmd.iter().map(|s| CString::new(*s).unwrap()).collect();

        match unsafe { fork()? } {
            ForkResult::Child => {
                // ASLR(Address space layout randomization) を無効化
                // デバッグ時に不便
                let p = personality::get().unwrap();
                personality::set(p | Persona::ADDR_NO_RANDOMIZE).unwrap();
                // 自身がデバッガによるトレース対象であることを宣言
                //   exec で即座にプロセスが停止する
                ptrace::traceme().unwrap();

                // exec
                // 子プロセスをデバッグ対象のプログラムに置き換える
                execvp(
                    &CString::new(self.info.filenme.as_str()).unwrap(),
                    &args,
                ).unwrap();
                unreachable!();
            }
            // 親プロセスであれば waitpid で子プロセスの終了を待つ
            ForkResult::Parent { child, .. } => match waitpid(child, None)? {
                WaitStatus::Stopped(..) => {
                    println!("<<子プロセスの実行に成功: PID = {child}>>");
                    self.info.pid = child;
                    // Runningに遷移
                    let mut dbg = ZDbg::<Running> {
                        info: self.info,
                        _state: Running,
                    };
                    // ブレークポイントを子プロセスのメモリ上に実際に設定
                    dbg.set_break()?;
                    // 子プロセスを再開
                    dbg.do_continue()
                }
                WaitStatus::Exited(..) | WaitStatus::Signaled(..) => {
                    Err("子プロセスの実行に失敗".into())
                }
                _ => Err("子プロセスが不正な状態です".into()),
            }
        }
    }
}

/// Running の実装
impl ZDbg<Running> {
    // 機械語レベルでステップ実行を行う
    fn do_stepi(self) -> Result<State, DynError> {
        // TODO: 
    }

    fn do_exit(self) -> Result<(), DynError> {
        // ?? なんでループするのか ??
        loop {
            ptrace::kill(self.info.pid)?;
            match waitpid(self.info.pid, None)? {
                WaitStatus::Exited(..) | WaitStatus::Signaled(..) => return Ok(()),
                _ => (), // ?? 何が来る ??
            }
        }
    }

    fn do_break(&mut self, cmd: &[&str]) -> Result<(), DynError> {
        if self.set_break_addr(cmd) {
            self.set_break()?;
        }
        Ok(())
    }

    fn set_break(&mut self) -> Result<(), DynError> {
        let addr = if leet Some(addr) = self.info.brk_addr {
            addr
        } else {
            return Ok(());
        }

        // ブレークするアドレスにあるメモリ上の位置を取得
        if val = match ptrace::read(self.info.pid, addr) {
            Ok(val) => val,
            Err(e) => {
                eprintln!("<<ブレークポイントの設定に失敗: {e}, addr = {:p}>>", addr);
                return Ok(());
            }
        };

        // メモリ上の値を表示する補助関数
        fn print_val(afddr: usize, val: i64) {
            print!("{:x}", addr);
            for n in (0..8).map(|n| ((val >> (n*8)) & 0xff) as u8) {
                print!(" {:02x}", n);
            }
        }

        println!("<<以下のようにメモリを書き換えます>>");
        println!("<<before: "); // 元の値
        print_val(addr as usize, val);
        println!(">>");

        let val_int3 = (val & !0xff) | 0xcc; // 0xcc は INT3 命令
        // 続き

        Ok(())
    }
}

/// ヘルプを表示
fn do_help() {
    println!(
        r#"コマンド一覧(カッコ内は省略記法)
        break 0x8000 : ブレークポイントを 0x8000 番地に設定 (b 0x8000)
        run          : プログラムを実行 (r)
        continue     : プログラムを再開 (c)
        stepi        : 機械語レベルで1ステップ実行 (s)
        registers    : レジスタを表示 (regs)
        exit         : 終了
        help         : このヘルプを表示 (h)
        "#
    );
}