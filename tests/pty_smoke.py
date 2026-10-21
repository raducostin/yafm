#!/usr/bin/env python3
"""Drive a yafm binary in a 100x30 pseudo-terminal and check the main features.

Usage: python3 tests/pty_smoke.py path/to/yafm
Prints PASS/FAIL per step; exits 1 if any step fails. Uses temp dirs only.
"""
import fcntl, glob, os, pty, select, shutil, struct, subprocess, sys, tempfile, termios, time

binary = os.path.abspath(sys.argv[1])
work = tempfile.mkdtemp(prefix="yafm-smoke-")
files = os.path.join(work, "files")
os.makedirs(os.path.join(files, "src/in"))
open(os.path.join(files, "src/in/f"), "w").write("F")
subprocess.run("tar cf - * | gzip > ../a.tar.gz", shell=True, cwd=os.path.join(files, "src"), check=True)
shutil.rmtree(os.path.join(files, "src"))
open(os.path.join(files, "note.txt"), "w").write("needle\n")
cwd_file = os.path.join(work, "cwd")
env = dict(os.environ, XDG_CONFIG_HOME=os.path.join(work, "cfg"), HOME=os.path.join(work, "home"), TERM="xterm-256color")
os.makedirs(env["HOME"])
before_tmp = set(glob.glob(os.path.join(tempfile.gettempdir(), "yafm-*.yafm")))

pid, fd = pty.fork()
if pid == 0:
    os.chdir(files)
    os.execve(binary, [binary, "--cwd-file", cwd_file], env)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))

out = b""
def pump(t=0.5):
    global out
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                out += os.read(fd, 65536)
            except OSError:
                return

def send(keys, t=0.5):
    """Send keys; return the screen output they produced."""
    mark = len(out)
    os.write(fd, keys.encode())
    pump(t)
    return out[mark:].decode("utf-8", "replace")

results = []
def check(name, ok):
    results.append((name, bool(ok)))

F7, F8, F12, LEFT = "\x1b[18~", "\x1b[19~", "\x1b[24~", "\x1b[D"
pump(1.0)
check("starts and draws the panes", "a.tar.gz" in out.decode("utf-8", "replace"))
check("tabs: new and close", "files" in send(":") and send("!") is not None)
send(F7); send("newdir\r", 0.8)
check("F7 makes a directory", os.path.isdir(os.path.join(files, "newdir")))
send("\x1b[H"); send("j")  # home, then down to the first dir: a.tar.gz sorts after dirs, newdir is index 1
send(F8); send("y", 0.8)
check("F8 deletes after confirmation", not os.path.isdir(os.path.join(files, "newdir")))
send("\x1b[H"); send("j")
screen = send("\r", 1.0)
during = set(glob.glob(os.path.join(tempfile.gettempdir(), "yafm-*.yafm"))) - before_tmp
check("Enter opens the archive as a folder", "#vfs://" in screen and len(during) == 1)
send(LEFT); send("n", 0.8)
after = set(glob.glob(os.path.join(tempfile.gettempdir(), "yafm-*.yafm"))) - before_tmp
check("leaving the archive removes its temp folder", len(after) == 0)
screen = send(F12) + send("i", 0.8)
check("file info", "Inode" in screen and "Links" in screen)
send("q")
check("tree view", " Tree " in send("\x14", 0.8))
send("q")
send("/"); screen = send("\x17note*\r", 1.0)
check("find shows results", "[ Go ]" in screen)
send("q")
send("\x1b[F")  # end: last entry (note.txt)
screen = send("\x18echo $f\r", 1.0) + send("y", 0.8)
# single words: the terminal diff can split phrases with cursor moves
check("PowerCLI runs and shows output", "Output" in screen and "note.txt" in screen)
send("q")
send("q"); send("y", 1.0)
try:
    _, status = os.waitpid(pid, 0)
    code = os.WEXITSTATUS(status)
except ChildProcessError:
    code = -1
check("quits with exit code 0", code == 0)
check("terminal restored (alternate screen left)", b"\x1b[?1049l" in out)
check("--cwd-file holds the last directory", os.path.exists(cwd_file) and open(cwd_file).read() == files)

shutil.rmtree(work)
for name, ok in results:
    print(("PASS" if ok else "FAIL") + "  " + name)
sys.exit(0 if all(ok for _, ok in results) else 1)
