//! Local shells through the OS pseudo terminal (ConPTY on Windows, openpty on
//! Unix).

use std::io::{Read, Write};
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::error::{Error, Result};
use crate::profile::LocalShell;
use crate::session::{Control, EventSink, SessionEvent, SessionHandle, TermSize};

fn pty_size(size: TermSize) -> PtySize {
    let size = size.clamped();
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn pty_err(e: impl std::fmt::Display) -> Error {
    Error::Pty(e.to_string())
}

pub fn spawn(shell: &LocalShell, size: TermSize, sink: EventSink) -> Result<SessionHandle> {
    let pair = native_pty_system().openpty(pty_size(size)).map_err(pty_err)?;

    let mut cmd = CommandBuilder::new(&shell.program);
    cmd.args(&shell.args);
    match &shell.cwd {
        Some(dir) if dir.is_dir() => cmd.cwd(dir),
        _ => {
            if let Some(home) = directories::BaseDirs::new() {
                cmd.cwd(home.home_dir());
            }
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "Termdock");

    let mut child = pair.slave.spawn_command(cmd).map_err(pty_err)?;
    // The child owns the slave side now; keeping it open here would stop us
    // from seeing EOF when the shell exits.
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(pty_err)?;
    let writer = pair.master.take_writer().map_err(pty_err)?;
    let mut killer = child.clone_killer();
    let (handle, mut rx) = SessionHandle::new();
    let (exit_tx, exit_rx) = std_mpsc::channel();

    // Waiter: reaps the child and then asks the control thread to release the
    // PTY (needed on Windows, where ConPTY keeps the pipe open otherwise).
    let control_tx = handle.clone();
    thread::Builder::new().name("pty-wait".into()).spawn(move || {
        let status = child.wait().ok();
        let _ = exit_tx.send(status);
        control_tx.close();
    })?;

    // Control: input, resize and shutdown.
    thread::Builder::new().name("pty-control".into()).spawn(move || {
        let master = pair.master;
        let mut writer = writer;
        while let Some(msg) = rx.blocking_recv() {
            match msg {
                Control::Input(data) => {
                    if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                        break;
                    }
                }
                Control::Resize(size) => {
                    let _ = master.resize(pty_size(size));
                }
                Control::Close => break,
            }
        }
        let _ = killer.kill();
        drop(writer);
        drop(master);
    })?;

    // Reader: forwards output until EOF, then reports the exit status.
    thread::Builder::new().name("pty-read".into()).spawn(move || {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => sink(SessionEvent::Data(buf[..n].to_vec())),
            }
        }
        let status = exit_rx.recv_timeout(Duration::from_secs(5)).ok().flatten();
        sink(SessionEvent::Exit {
            code: status.as_ref().map(|s| s.exit_code()),
            message: status.and_then(|s| s.signal().map(str::to_owned)),
        });
    })?;

    Ok(handle)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test(flavor = "multi_thread")]
    async fn runs_a_shell() {
        let out = Arc::new(Mutex::new(Vec::new()));
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let done_tx = Mutex::new(Some(done_tx));
        let out2 = out.clone();
        let sink: EventSink = Arc::new(move |ev| match ev {
            SessionEvent::Data(d) => out2.lock().unwrap().extend(d),
            SessionEvent::Exit { code, .. } => {
                if let Some(tx) = done_tx.lock().unwrap().take() {
                    let _ = tx.send(code);
                }
            }
        });

        let shell = LocalShell {
            program: "/bin/sh".into(),
            args: vec![],
            cwd: None,
        };
        let h = spawn(&shell, TermSize { cols: 100, rows: 30 }, sink).unwrap();
        h.resize(TermSize { cols: 120, rows: 40 }).unwrap();
        h.write(b"stty size; echo hello-$((40+2)); exit 3\n".to_vec()).unwrap();

        let code = tokio::time::timeout(Duration::from_secs(10), done_rx)
            .await
            .expect("shell did not exit")
            .unwrap();
        assert_eq!(code, Some(3));
        let text = String::from_utf8_lossy(&out.lock().unwrap()).to_string();
        assert!(text.contains("hello-42"), "{text}");
        assert!(text.contains("40 120"), "{text}");
    }
}
