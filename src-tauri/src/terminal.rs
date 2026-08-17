//! The built-in terminal.
//!
//! Each session is a real pty — a login shell running under `portable-pty`, so
//! prompts, colour, job control and curses programs behave exactly as they do
//! in a native terminal. Bytes flow out on the `term:data` event and in through
//! `term_write`; the frontend end is xterm.js.
//!
//! Output is base64 so a chunk that splits a UTF-8 sequence (or carries raw
//! bytes that are not text at all) survives the JSON hop intact — xterm
//! reassembles the stream on its side.

use base64::Engine;
use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TermData {
    pub id: String,
    /// base64 of the raw pty bytes.
    pub chunk: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TermExit {
    pub id: String,
    pub code: Option<i32>,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

type Registry = Arc<Mutex<HashMap<String, Session>>>;

fn sessions() -> &'static Registry {
    static SESSIONS: OnceLock<Registry> = OnceLock::new();
    SESSIONS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// Shells to try, best first. Spawning falls through the list, so a machine
/// without the preferred shell still gets a terminal.
///
/// On Unix `new_default_prog` is the right answer on both Linux and macOS: it
/// resolves `$SHELL`, falls back to the passwd database — which matters for a
/// macOS app launched from Finder, where `$SHELL` is often unset — and starts
/// the shell as a real login shell by prefixing argv0 with `-`. That is what
/// loads `.zprofile` and therefore the user's real `PATH`; a GUI process on
/// macOS otherwise inherits a bare `/usr/bin:/bin` and nothing they installed
/// with Homebrew would be on it.
fn shell_candidates() -> Vec<CommandBuilder> {
    #[cfg(windows)]
    {
        // PowerShell 7, then Windows PowerShell, then whatever ComSpec says
        // (cmd.exe). `CommandBuilder` resolves bare names against PATH+PATHEXT.
        vec![
            CommandBuilder::new("pwsh.exe"),
            CommandBuilder::new("powershell.exe"),
            CommandBuilder::new_default_prog(),
        ]
    }

    #[cfg(not(windows))]
    {
        vec![CommandBuilder::new_default_prog()]
    }
}

fn prepare(mut cmd: CommandBuilder, cwd: &str) -> CommandBuilder {
    let dir = Path::new(cwd);
    if dir.is_dir() {
        cmd.cwd(dir);
    } else if let Some(home) = dirs::home_dir() {
        cmd.cwd(home);
    }

    // Unix only, deliberately. xterm.js speaks xterm-256color, so telling the
    // shell lines `ls` colours and ncurses key handling up with what the
    // frontend renders. On Windows nothing sets `TERM` natively and ConPTY
    // negotiates VT on its own; inventing one confuses ported tools.
    #[cfg(not(windows))]
    {
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
    }

    cmd
}

pub fn open(app: &AppHandle, id: String, cwd: String, cols: u16, rows: u16) -> Result<(), String> {
    if sessions().lock().unwrap().contains_key(&id) {
        return Ok(());
    }

    let pty = NativePtySystem::default()
        .openpty(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let mut child = None;
    let mut last_error = String::from("No usable shell found");
    for candidate in shell_candidates() {
        match pty.slave.spawn_command(prepare(candidate, &cwd)) {
            Ok(spawned) => {
                child = Some(spawned);
                break;
            }
            Err(e) => last_error = e.to_string(),
        }
    }
    let child = child.ok_or(last_error)?;
    // Dropping the slave lets the reader see EOF once the shell exits.
    drop(pty.slave);

    let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;

    sessions().lock().unwrap().insert(
        id.clone(),
        Session {
            master: pty.master,
            writer,
            child,
        },
    );

    // Blocking pty reads need their own thread; tokio would only park a worker.
    let handle = app.clone();
    let reader_id = id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let chunk = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    if handle
                        .emit(
                            "term:data",
                            TermData {
                                id: reader_id.clone(),
                                chunk,
                            },
                        )
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }

        let code = sessions()
            .lock()
            .unwrap()
            .remove(&reader_id)
            .and_then(|mut s| s.child.wait().ok())
            .map(|status| status.exit_code() as i32);
        let _ = handle.emit(
            "term:exit",
            TermExit {
                id: reader_id,
                code,
            },
        );
    });

    Ok(())
}

pub fn write(id: &str, data: &str) -> Result<(), String> {
    let mut all = sessions().lock().unwrap();
    let session = all.get_mut(id).ok_or("Terminal session is not running")?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

pub fn resize(id: &str, cols: u16, rows: u16) -> Result<(), String> {
    let all = sessions().lock().unwrap();
    let Some(session) = all.get(id) else {
        // A resize can race a closing tab; that is not an error worth showing.
        return Ok(());
    };
    session
        .master
        .resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

pub fn close(id: &str) -> Result<(), String> {
    let Some(mut session) = sessions().lock().unwrap().remove(id) else {
        return Ok(());
    };
    let _ = session.child.kill();
    let _ = session.child.wait();
    Ok(())
}
