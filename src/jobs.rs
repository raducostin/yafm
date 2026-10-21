//! Running external commands: quoting, one-shot runs, background runs, and
//! jobs that can be paused, resumed and killed. Port of `escape_*`, `run_*`
//! and `ProcessCommand` in lfm's utils.py.

use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;

use nix::sys::signal::{killpg, Signal};
use nix::unistd::Pid;

/// POSIX single-quote quoting: `'` + s with each `'` as `'\''` + `'`.
pub fn shell_quote(s: &OsStr) -> OsString {
    let mut out = Vec::with_capacity(s.len() + 2);
    out.push(b'\'');
    for &b in s.as_bytes() {
        if b == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(b);
        }
    }
    out.push(b'\'');
    OsString::from_vec(out)
}

/// `{program} {quoted file}`, detached with output discarded when `background`.
pub fn command_on_file(program: &str, file: &Path, background: bool) -> OsString {
    let mut cmd = OsString::from(program);
    cmd.push(" ");
    cmd.push(shell_quote(file.as_os_str()));
    if background {
        cmd.push(" >/dev/null 2>&1 &");
    }
    cmd
}

fn shell(cmd: &OsStr, cwd: &Path) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd).current_dir(cwd).stdin(Stdio::null());
    c
}

/// Runs `cmd` with `sh -c` in `cwd` and waits; returns (stdout, stderr).
pub fn run_in_cli(cmd: &OsStr, cwd: &Path) -> std::io::Result<(String, String)> {
    let out = shell(cmd, cwd).output()?;
    Ok((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

/// Starts `cmd` detached with its output discarded and returns at once.
pub fn run_in_background(cmd: &OsStr, cwd: &Path) -> std::io::Result<()> {
    let mut detached = cmd.to_os_string();
    detached.push(" >/dev/null 2>&1 &");
    shell(&detached, cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    Ok(())
}

/// A shell command in its own process group, so signals reach every process it starts.
pub struct Job {
    child: Child,
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
    killed: bool,
    paused: Cell<bool>,
    /// Reaped (or finished): its pid may be reused, so no more signals.
    exited: bool,
}

pub struct JobOutput {
    /// `None` when the job was killed.
    pub status: Option<i32>,
    pub stopped: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Drains a pipe on its own thread so a chatty command can't block on a full pipe.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> Option<JoinHandle<Vec<u8>>> {
    pipe.map(|mut r| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            buf
        })
    })
}

fn joined(h: Option<JoinHandle<Vec<u8>>>) -> String {
    h.and_then(|h| h.join().ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

impl Job {
    pub fn spawn(cmd: &OsStr, cwd: &Path) -> std::io::Result<Job> {
        let mut child = shell(cmd, cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        Ok(Job {
            child,
            stdout,
            stderr,
            killed: false,
            paused: Cell::new(false),
            exited: false,
        })
    }

    /// Exit code once finished (-1 if ended by a signal), `None` while running.
    pub fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
        let status = self.child.try_wait()?;
        if status.is_some() {
            self.exited = true;
        }
        Ok(status.map(|s| s.code().unwrap_or(-1)))
    }

    fn signal_group(&self, sig: Signal) -> std::io::Result<()> {
        if self.exited {
            return Ok(());
        }
        killpg(Pid::from_raw(self.child.id() as i32), sig).map_err(std::io::Error::from)
    }

    pub fn pause(&self) -> std::io::Result<()> {
        self.signal_group(Signal::SIGSTOP)?;
        self.paused.set(!self.exited);
        Ok(())
    }

    pub fn resume(&self) -> std::io::Result<()> {
        self.signal_group(Signal::SIGCONT)?;
        self.paused.set(false);
        Ok(())
    }

    /// Kills the whole process group; a job that already exited keeps its real status.
    pub fn kill(&mut self) -> std::io::Result<()> {
        if self.exited {
            return Ok(());
        }
        self.killed = true;
        self.signal_group(Signal::SIGKILL)
    }

    pub fn finish(mut self) -> JobOutput {
        if self.paused.get() && !self.killed {
            let _ = self.resume();
        }
        let status = self.child.wait().ok().and_then(|s| s.code());
        self.exited = true;
        JobOutput {
            status: if self.killed { None } else { status },
            stopped: self.killed,
            stdout: joined(self.stdout.take()),
            stderr: joined(self.stderr.take()),
        }
    }
}

impl Drop for Job {
    /// A job dropped while running is killed and reaped, never left as a zombie.
    fn drop(&mut self) {
        if !self.exited {
            let _ = self.signal_group(Signal::SIGKILL);
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(shell_quote(OsStr::new("a b")), OsString::from("'a b'"));
        assert_eq!(
            shell_quote(OsStr::new("it's $x \"q\"")),
            OsString::from("'it'\\''s $x \"q\"'")
        );
        assert_eq!(
            command_on_file("less", Path::new("/t/a b"), false),
            OsString::from("less '/t/a b'")
        );
        assert_eq!(
            command_on_file("eog", Path::new("/p.png"), true),
            OsString::from("eog '/p.png' >/dev/null 2>&1 &")
        );
    }

    #[test]
    fn quoted_names_survive_the_shell() {
        let t = tempfile::tempdir().unwrap();
        let name = "it's $HOME \"q\"";
        std::fs::write(t.path().join(name), b"hello").unwrap();
        let cmd = command_on_file("cat", &t.path().join(name), false);
        assert_eq!(run_in_cli(&cmd, t.path()).unwrap().0, "hello");
    }

    #[test]
    fn cli_output_and_cwd() {
        let t = tempfile::tempdir().unwrap();
        let (out, err) = run_in_cli(OsStr::new("pwd; echo oops >&2"), t.path()).unwrap();
        assert_eq!(
            std::path::PathBuf::from(out.trim()).canonicalize().unwrap(),
            t.path().canonicalize().unwrap()
        );
        assert_eq!(err.trim(), "oops");
    }

    #[test]
    fn job_finishes_with_large_output() {
        let mut j = Job::spawn(OsStr::new("seq 1 200000; exit 3"), Path::new("/")).unwrap();
        while j.try_wait().unwrap().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let o = j.finish();
        assert_eq!(o.status, Some(3));
        assert!(!o.stopped);
        assert!(o.stdout.ends_with("200000\n"));
    }

    #[test]
    fn job_pause_resume_kill() {
        let mut j = Job::spawn(OsStr::new("sleep 30"), Path::new("/")).unwrap();
        j.pause().unwrap();
        j.resume().unwrap();
        j.kill().unwrap();
        let o = j.finish();
        assert!(o.stopped);
        assert_eq!(o.status, None);
    }

    fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || tx.send(f()).unwrap());
        rx.recv_timeout(std::time::Duration::from_secs(secs))
            .expect("timed out")
    }

    #[test]
    fn kill_stops_compound_commands_quickly() {
        let o = within(2, || {
            let mut j = Job::spawn(OsStr::new("sleep 4; echo done"), Path::new("/")).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(200));
            j.kill().unwrap();
            j.finish()
        });
        assert!(o.stopped && !o.stdout.contains("done"));
    }

    #[test]
    fn finish_after_pause_does_not_hang() {
        let o = within(3, || {
            let j = Job::spawn(OsStr::new("sleep 0.3; echo ok"), Path::new("/")).unwrap();
            j.pause().unwrap();
            j.finish()
        });
        assert_eq!((o.status, o.stdout.as_str()), (Some(0), "ok\n"));
    }

    #[test]
    fn kill_after_exit_keeps_real_status() {
        let mut j = Job::spawn(OsStr::new("exit 4"), Path::new("/")).unwrap();
        while j.try_wait().unwrap().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        j.kill().unwrap();
        let o = j.finish();
        assert_eq!((o.status, o.stopped), (Some(4), false));
    }

    #[test]
    fn background_returns_immediately() {
        let t = std::time::Instant::now();
        run_in_background(OsStr::new("sleep 5"), Path::new("/")).unwrap();
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
    }
}
