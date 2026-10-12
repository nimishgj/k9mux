use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// How a child process is launched. Tests substitute a stub for k9s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl Launch {
    pub fn new(program: &str) -> Launch {
        Launch {
            program: program.to_string(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }
    pub fn arg(mut self, a: &str) -> Launch {
        self.args.push(a.to_string());
        self
    }
    pub fn env(mut self, k: &str, v: &str) -> Launch {
        self.env.push((k.to_string(), v.to_string()));
        self
    }
}

/// One child process on a pty, with its screen parsed and ready to render.
pub struct Session {
    parser: Arc<Mutex<vt100::Parser>>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    alive: Arc<AtomicBool>,
    exit: Arc<Mutex<Option<i32>>>,
    size: (u16, u16),
}

impl Session {
    pub fn spawn(launch: &Launch, rows: u16, cols: u16) -> Result<Session, String> {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(&launch.program);
        for a in &launch.args {
            cmd.arg(a);
        }
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        if let Ok(dir) = std::env::current_dir() {
            cmd.cwd(dir);
        }

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let alive = Arc::new(AtomicBool::new(true));

        let sink = parser.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => sink.lock().unwrap().process(&buf[..n]),
                }
            }
        });

        let child = Arc::new(Mutex::new(child));
        let exit: Arc<Mutex<Option<i32>>> = Arc::new(Mutex::new(None));
        let watched = child.clone();
        let flag = alive.clone();
        let code = exit.clone();
        std::thread::spawn(move || {
            loop {
                let status = match watched.lock().unwrap().try_wait() {
                    Ok(Some(s)) => Some(s.exit_code() as i32),
                    Err(_) => Some(-1),
                    Ok(None) => None,
                };
                if let Some(s) = status {
                    *code.lock().unwrap() = Some(s);
                    flag.store(false, Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });

        Ok(Session {
            parser,
            master: pair.master,
            writer,
            child,
            alive,
            exit,
            size: (rows, cols),
        })
    }
    /// Feeds bytes to the child as if typed.
    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser
            .lock()
            .unwrap()
            .screen_mut()
            .set_size(rows, cols);
    }
    pub fn size(&self) -> (u16, u16) {
        self.size
    }
    /// False once the child has exited.
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
    /// The child's exit code, once it has one.
    pub fn exit_code(&self) -> Option<i32> {
        *self.exit.lock().unwrap()
    }
    pub fn parser(&self) -> &Arc<Mutex<vt100::Parser>> {
        &self.parser
    }
    /// Screen contents as text, for tests and diagnostics.
    pub fn text(&self) -> String {
        let p = self.parser.lock().unwrap();
        p.screen().contents()
    }
    pub fn kill(&mut self) {
        let _ = self.child.lock().unwrap().kill();
        self.alive.store(false, Ordering::SeqCst);
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("size", &self.size)
            .field("alive", &self.alive())
            .finish()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// `TERM` is set explicitly because CI runners often have none, the way
    /// k9s is given one in production. The stubs that ask how big the
    /// terminal is use `stty`, which needs no terminfo database and so runs
    /// on a bare image too - busybox, for one, has no `tput`.
    fn sh(script: &str) -> Launch {
        Launch::new("sh")
            .arg("-c")
            .arg(script)
            .env("TERM", "xterm-256color")
    }

    /// Waits for the child's output to satisfy `f`, so tests never sleep blindly.
    fn wait_for(s: &Session, f: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let text = s.text();
            if f(&text) {
                return text;
            }
            assert!(Instant::now() < deadline, "timed out; screen was:\n{text}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_child_writes_onto_the_screen() {
        let s = Session::spawn(&sh("printf 'hello world\\n'"), 10, 40).unwrap();
        let text = wait_for(&s, |t| t.contains("hello world"));
        assert!(text.contains("hello world"));
    }

    #[test]
    fn the_screen_is_the_size_we_asked_for() {
        let s = Session::spawn(&sh("sleep 5"), 24, 80).unwrap();
        assert_eq!(s.size(), (24, 80));
        let p = s.parser().lock().unwrap();
        assert_eq!(p.screen().size(), (24, 80));
    }

    #[test]
    fn the_child_is_told_the_terminal_size() {
        let s = Session::spawn(&sh("stty size"), 17, 61).unwrap();
        let text = wait_for(&s, |t| t.contains("17") && t.contains("61"));
        assert!(text.contains("17"), "{text}");
        assert!(text.contains("61"), "{text}");
    }

    #[test]
    fn input_reaches_the_child() {
        let mut s = Session::spawn(&sh("read line; printf 'got:%s\\n' \"$line\""), 10, 40).unwrap();
        s.write(b"ping\r");
        let text = wait_for(&s, |t| t.contains("got:ping"));
        assert!(text.contains("got:ping"), "{text}");
    }

    #[test]
    fn escape_sequences_are_interpreted_not_printed() {
        let s = Session::spawn(&sh("printf '\\033[2J\\033[HTOP\\n'"), 6, 20).unwrap();
        let text = wait_for(&s, |t| t.contains("TOP"));
        assert!(!text.contains("\u{1b}"), "raw escape leaked: {text:?}");
        assert!(text.starts_with("TOP"), "{text:?}");
    }

    #[test]
    fn resizing_tells_the_child_and_the_parser() {
        let mut s = Session::spawn(&sh("sleep 5"), 10, 40).unwrap();
        s.resize(30, 100);
        assert_eq!(s.size(), (30, 100));
        let p = s.parser().lock().unwrap();
        assert_eq!(p.screen().size(), (30, 100));
    }

    #[test]
    fn a_resize_makes_the_child_see_the_new_size() {
        let mut s = Session::spawn(&sh("trap 'stty size' WINCH; sleep 5 & wait"), 10, 40).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        s.resize(10, 123);
        let text = wait_for(&s, |t| t.contains("123"));
        assert!(text.contains("123"), "{text}");
    }

    #[test]
    fn a_session_knows_when_its_child_exits() {
        let s = Session::spawn(&sh("exit 0"), 10, 40).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while s.alive() {
            assert!(Instant::now() < deadline, "child never reported exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!s.alive());
    }

    #[test]
    fn a_clean_exit_is_reported_as_zero() {
        let s = Session::spawn(&sh("exit 0"), 10, 40).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while s.exit_code().is_none() {
            assert!(Instant::now() < deadline, "no exit code");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.exit_code(), Some(0));
    }

    #[test]
    fn a_failure_keeps_its_exit_code() {
        let s = Session::spawn(&sh("exit 3"), 10, 40).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while s.exit_code().is_none() {
            assert!(Instant::now() < deadline, "no exit code");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.exit_code(), Some(3));
    }

    #[test]
    fn a_running_child_has_no_exit_code_yet() {
        let s = Session::spawn(&sh("sleep 5"), 10, 40).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(s.exit_code(), None);
    }

    #[test]
    fn a_long_running_child_stays_alive() {
        let s = Session::spawn(&sh("sleep 5"), 10, 40).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(s.alive());
    }

    #[test]
    fn killing_a_session_stops_the_child() {
        let mut s = Session::spawn(&sh("sleep 30"), 10, 40).unwrap();
        s.kill();
        let deadline = Instant::now() + Duration::from_secs(10);
        while s.alive() {
            assert!(Instant::now() < deadline, "child survived kill");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn environment_is_passed_to_the_child() {
        let s = Session::spawn(
            &sh("printf 'ctx=%s\\n' \"$K9MUX_TEST_CTX\"").env("K9MUX_TEST_CTX", "use1-a"),
            10,
            40,
        )
        .unwrap();
        let text = wait_for(&s, |t| t.contains("ctx=use1-a"));
        assert!(text.contains("ctx=use1-a"), "{text}");
    }

    #[test]
    fn sessions_do_not_leak_ptys() {
        // The pty pool is small; a pane that comes and goes must give its back.
        for i in 0..150 {
            let s = Session::spawn(&sh("sleep 30"), 10, 40)
                .unwrap_or_else(|e| panic!("ran out of ptys after {i} sessions: {e}"));
            drop(s);
        }
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_panic() {
        let err = Session::spawn(&Launch::new("k9mux-no-such-binary-xyz"), 10, 40).unwrap_err();
        assert!(!err.is_empty());
    }
}
