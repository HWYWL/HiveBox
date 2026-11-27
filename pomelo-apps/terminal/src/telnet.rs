//! Telnet：把面板上那个终端，搬到任何一台能 `telnet` 过来的机器上。
//!
//! # 为什么是「同一套命令」，而不是另一套
//!
//! 这里没有第二个 shell。一行输入被拆成 [`KeyAction`]，喂给面板用的那个
//! [`TerminalModel::handle_key_action`] —— 和屏幕上那块虚拟键盘走的是**同一条路**。于是
//! `ls`、`cd`、`cat`、`rm`、`free`、`wifi`、`clear`……在这里的行为和在面板上逐字一致，
//! 连「命令写错了」那句提示都是同一句。要加新命令，只改 `commands/`，两边自动都有。
//!
//! # 一次连接一个会话
//!
//! 每条连接有自己的一份 [`TerminalModel`]，也就是自己的 cwd 和历史 —— 两个人在两台电脑上
//! 同时连进来，不会互相把对方 `cd` 走。命令本身共用全局注册表，那是只读的。
//!
//! # 协议只做必须做的那一点
//!
//! Telnet 的协商（IAC …）必须**至少不要当成正文**，否则第一个提示符前面会冒出一串乱码。
//! 所以：
//!
//! * 进来的 `IAC cmd opt` 三个字节被吃掉，不当输入；
//! * 我们主动声明 `WILL ECHO` 和 `WILL SUPPRESS-GO-AHEAD`，让服务端负责回显（客户端就不会
//!   再本地回显一遍，打一个字出现两个）；
//! * 除此之外一概拒绝，不去实现窗口大小、行模式这些用不上的选项。
//!
//! # 明文，没有口令
//!
//! 和网页那边一样：它只在局域网里，绑 `0.0.0.0`，**没有密码**。谁能连上 23 端口，谁就能跑
//! `rm`。这是这个项目的立场（见 `board_web.c` 顶部那段），用之前应该知道自己在什么网络上。

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::net::{TcpListener, TcpStream};

use crate::shell::{DEFAULT_USER, KeyAction, TerminalModel};

/// 23 是 telnet 的标准端口，客户端不用另外指定。
pub const PORT: u16 = 23;

/// 一条会话的线程栈。它只做字符串和几行 I/O，但 Rust 的格式化在上面还要占一层，
/// 所以给得比「刚好够」宽一点 —— 这块板子的内部 RAM 只剩三十来 KB，一万字节是个折中。
const SESSION_STACK: usize = 10 * 1024;

const IAC: u8 = 255;
const WILL: u8 = 251;
const WONT: u8 = 252;
const DO: u8 = 253;
const DONT: u8 = 254;
const OPT_ECHO: u8 = 1;
const OPT_SUPPRESS_GO_AHEAD: u8 = 3;

/// 同一时刻只服务一条连接。
///
/// 不是懒，是算术：每条会话一条线程、一份栈，这块板子的内部 RAM 只剩二十来 KB，第二条会话
/// 的线程根本开不出来 —— 实测第二个连接连招呼都收不到。与其让它卡在那儿，不如明说。
static BUSY: AtomicBool = AtomicBool::new(false);

/// 起服务：绑端口、开一条接受连接的线程，每条连接一条会话线程。
pub fn start(port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;

    std::thread::Builder::new()
        .name("telnet".into())
        .spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else {
                    continue;
                };

                if BUSY.swap(true, Ordering::SeqCst) {
                    // 说清楚，然后挂断。协商字节先发，免得客户端把它当正文。
                    let _ = stream.write_all(&[IAC, WILL, OPT_ECHO, IAC, WILL, OPT_SUPPRESS_GO_AHEAD]);
                    let _ = stream.write_all(
                        "\r\n这个终端一次只服务一台设备，已经有人连着了 —— 等他断开再来。\r\n".as_bytes(),
                    );
                    let _ = stream.flush();
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    continue;
                }

                let spawned = std::thread::Builder::new()
                    .name("telnet_session".into())
                    .stack_size(SESSION_STACK)
                    .spawn(move || {
                        if let Err(error) = session(stream) {
                            // 客户端那边直接关掉窗口是常态，不当成故障喊出来。
                            let _ = error;
                        }

                        BUSY.store(false, Ordering::SeqCst);
                    });

                if let Err(error) = spawned {
                    BUSY.store(false, Ordering::SeqCst);
                    eprintln!("[telnet] 开会话线程失败: {error}");
                }
            }
        })?;

    Ok(())
}

/// 一条连接的一生：欢迎语、提示符、读一行、跑一行、把新增的输出发回去。
fn session(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_nodelay(true)?;

    // 服务端回显 + 抑制 GA：客户端就不会自己再回显一遍。
    stream.write_all(&[
        IAC,
        WILL,
        OPT_ECHO,
        IAC,
        WILL,
        OPT_SUPPRESS_GO_AHEAD,
    ])?;

    let mut model = TerminalModel::new();
    let mut sent = 0usize;
    let mut typed = String::new();
    let mut pending: Vec<u8> = Vec::new();

    flush(&mut stream, &model, &mut sent)?;
    prompt(&mut stream, &model)?;

    let mut buffer = [0u8; 64];
    let mut negotiation = 0u8;
    let mut last_was_cr = false;

    loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(_) => return Ok(()),
        };

        for &byte in &buffer[..read] {
            // 协商字节：IAC + 命令 (+ 选项)。只吃掉，不解释。
            match negotiation {
                0 => {
                    if byte == IAC {
                        negotiation = 1;
                        continue;
                    }
                }
                1 => {
                    // `IAC IAC` 是一个真正的 0xFF 字节，不是协商；它不是文字，丢掉就好。
                    if byte == IAC {
                        negotiation = 0;
                        continue;
                    }

                    negotiation = if (WILL..=DONT).contains(&byte) { 2 } else { 0 };

                    continue;
                }
                _ => {
                    negotiation = 0;
                    continue;
                }
            }

            match byte {
                b'\r' => {
                    last_was_cr = true;
                    enter(&mut stream, &mut model, &mut sent, &mut typed, &mut pending)?;
                }
                b'\n' => {
                    if last_was_cr {
                        last_was_cr = false;
                        continue;
                    }

                    enter(&mut stream, &mut model, &mut sent, &mut typed, &mut pending)?;
                }
                0x7f | 0x08 => {
                    last_was_cr = false;

                    if typed.pop().is_some() {
                        stream.write_all(b"\x08 \x08")?;
                    }
                }
                _ => {
                    last_was_cr = false;

                    // 一个字节可能是半个汉字：凑够一个完整字符再回显、再进缓冲区。
                    pending.push(byte);

                    if let Ok(text) = std::str::from_utf8(&pending) {
                        typed.push_str(text);
                        stream.write_all(text.as_bytes())?;
                        pending.clear();
                    } else if pending.len() >= 4 {
                        // 不是 UTF-8 的字节，丢掉，别让它卡住后面所有输入。
                        pending.clear();
                    }
                }
            }
        }
    }
}

/// 回车：把这一行喂给模型，然后把新增的输出发回去，再给一个新提示符。
fn enter(
    stream: &mut TcpStream,
    model: &mut TerminalModel,
    sent: &mut usize,
    typed: &mut String,
    pending: &mut Vec<u8>,
) -> std::io::Result<()> {
    pending.clear();

    /* 抹掉刚回显的那一行：等一下模型会把它自己记下的提示符行发出来，不抹就是同一个命令两行。 */
    stream.write_all(b"\r\x1b[2K")?;

    for ch in typed.chars() {
        model.handle_key_action(if ch == ' ' {
            KeyAction::Space
        } else {
            KeyAction::Char(ch)
        });
    }

    model.handle_key_action(KeyAction::Enter);
    typed.clear();

    flush(stream, model, sent)?;
    prompt(stream, model)
}

/// 把模型里**还没发过**的行发出去。
///
/// `clear` 命令会把历史整个清空，问一句「还剩几行」就露馅了：行数变少，就当作屏幕被清过，
/// 发一个 ANSI 清屏，从头再发一遍。
fn flush(stream: &mut TcpStream, model: &TerminalModel, sent: &mut usize) -> std::io::Result<()> {
    let lines = model.history_lines();

    if lines.len() < *sent {
        stream.write_all(b"\x1b[2J\x1b[H")?;
        *sent = 0;
    }

    for line in &lines[*sent..] {
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\r\n")?;
    }

    *sent = lines.len();

    Ok(())
}

/// 提示符：和面板上那行一模一样（`pomelo:/internal$ `），只是没有输入框。
fn prompt(stream: &mut TcpStream, model: &TerminalModel) -> std::io::Result<()> {
    stream.write_all(format!("{}:{}$ ", DEFAULT_USER, model.short_cwd()).as_bytes())
}
