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
    // // 機械語レベルでステップ実行を行う
    // fn do_stepi(self) -> Result<State, DynError> {
    //     // TODO:
    // }

    pub fn do_cmd(mut self, cmd: &[&str]) -> Result<State, DynError> {
        if cmd.is_empty() {
            return Ok(State::Running(self));
        }

        match cmd[0] {
            "break" | "b" => self.do_break(cmd)?,
            "continue" | "c" => return self.do_continue(),
            // "register" | "regs" => {
            //     let regs = ptrace::getregs(self.info.pid)?;
            //     print_regs(&regs);
            // }
            "run" | "r" => eprintln!("<<ターゲットは既に実行中です>>"),
            "exit" => {
                self.do_exit()?;
                return Ok(State::Exit);
            }
            _ => self.do_cmd_common(cmd),
        }


        Ok(State::Running(self))
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
        let addr = if let Some(addr) = self.info.brk_addr {
            // ブレークポイントが設定されている場合はそのアドレスを取得
            addr
        } else {
            return Ok(());
        };

        // ブレークするアドレスにあるメモリ上の位置を取得
        // 8バイト単位で取得ｄけいる
        let val = match ptrace::read(self.info.pid, addr) {
            Ok(val) => val,
            Err(e) => {
                eprintln!("<<ブレークポイントの設定に失敗: {e}, addr = {:p}>>", addr);
                return Ok(());
            }
        };

        // メモリ上の値を表示する補助関数
        fn print_val(addr: usize, val: i64) {
            print!("{:x}", addr);
            for n in (0..8).map(|n| ((val >> (n*8)) & 0xff) as u8) {
                print!(" {:02x}", n);
            }
        }

        println!("<<以下のようにメモリを書き換えます>>");
        println!("<<before: "); // 元の値
        print_val(addr as usize, val as i64);
        println!(">>");

        // まず下位8ビットを0クリアして、 0xcc を書き込む
        // x86_64 では リトルエンディアン採用
        // ビッグエンディアンは多くのインターネットプロトコルで採用されるため「ネットワークバイトオーダー」と呼ばれる
        let val_int3 = (val & !0xff) | 0xcc; // 0xcc は INT3 命令
        print!("<<after: "); // 書き換え後の値
        print_val(addr as usize, val_int3 as i64);
        println!(">>");

        // "int 3" をメモリに書き込み
        match unsafe {
            ptrace::write(self.info.pid, addr, val_int3 as *mut c_void)
        } {
            Ok(_) => {
                self.info.brk_addr = Some(addr);
                self.info.brk_val = val; // 元の値を保存
            }
            Err(e) => {
                eprintln!("<<ブレークポイントの設定に失敗: {e}, addr = {:p}>>", addr);
            }
        }

        Ok(())
    }

    fn do_continue(self) -> Result<State, DynError> {
        // ブレークポイントで停止している場合は1ステップ実行後再開
        // step_and_break や wait_child を実行すると子プロセスが終了すう可能性がある？
        //   終わりってこと？
        // self で値を取得して、遷移後の状態を返すようにしている？
        //   どこのこと？なんで？
        match self.step_and_break()? {
            State::Running(r) => {
                ptrace::cont(r.info.pid, None)?;
                r.wait_child()
            }
            n => Ok(n),
        }
    }

    fn step_and_break(mut self) -> Result<State, DynError> {
        // レジスタを取得する
        let regs = ptrace::getregs(self.info.pid)?;
        // ブレークポイントのアドレスかチェック
        if Some((regs.rip) as *mut c_void) == self.info.brk_addr {
            // ブレークポイントアドレスなら1ステップ実行
            ptrace::step(self.info.pid, None)?;
            // 止まったら終了
            match waitpid(self.info.pid, None)? {
                WaitStatus::Exited(..) | WaitStatus::Signaled(..) => {
                    println!("<<子プロセスが終了しました>>");
                    return Ok(State::NotRunning(ZDbg::<NotRunning>{
                        info: self.info,
                        _state: NotRunning,
                    }))
                }
                _ => (),
            }
        }

        Ok(State::Running(self))
    }

    fn wait_child(self) -> Result<State, DynError> {
        match waitpid(self.info.pid, None)? {
            WaitStatus::Exited(..) | WaitStatus::Signaled(..) => {
                println!("<<子プロセスが終了しました>>");
                let not_run = ZDbg::<NotRunning> {
                    info: self.info,
                    _state: NotRunning,
                };
                Ok(State::NotRunning(not_run))
            }
            WaitStatus::Stopped(..) => {
                let mut regs = ptrace::getregs(self.info.pid)?;
                if Some((regs.rip - 1) as *mut c_void) == self.info.brk_addr {
                    // 書き換えたメモリを元に戻す
                    unsafe {
                        ptrace::write(
                            self.info.pid,
                            self.info.brk_addr.unwrap(),
                            self.info.brk_val as *mut c_void,
                        )?
                    };

                    // ブレークポイントで停止したアドレスから1つ戻す
                    regs.rip -= 1;
                    ptrace::setregs(self.info.pid, regs)?;
                }
                println!("<<子プロセスが停止しました: PC = {:#x}>>", regs.rip);

                Ok(State::Running(self))
            }
            _ => Err("waitpid の返り値が不正です".into()),
        }
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