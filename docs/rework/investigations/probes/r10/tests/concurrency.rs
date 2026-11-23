//! I1-I2 and the process-kill part of H3. Every contender is a separate child process with its
//! own file descriptor on `state.lock`; synchronisation is by pipe lines, never by sleeping.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use r10::state::{HelperDir, HelperState};

const CHILD: &str = env!("CARGO_BIN_EXE_r10_child");

struct Kid {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
}

impl Kid {
    fn spawn(mode: &str, dir: &Path, cfg: &Path) -> Self {
        let mut child = Command::new(CHILD)
            .args([mode, &dir.to_string_lossy(), &cfg.to_string_lossy()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn child");
        let stdin = child.stdin.take().expect("stdin");
        let out = BufReader::new(child.stdout.take().expect("stdout"));
        Self { child, stdin, out }
    }
    fn line(&mut self) -> String {
        let mut l = String::new();
        self.out.read_line(&mut l).expect("read");
        l.trim().to_owned()
    }
    fn send(&mut self, s: &str) {
        writeln!(self.stdin, "{s}").expect("write");
        self.stdin.flush().expect("flush");
    }
}

impl Drop for Kid {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cfg(dir: &Path, name: &str, max_active: u32) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("[safety]\nmanagement_cidrs=[\"192.168.1.0/24\"]\n[safety.containment]\nmax_ttl=\"1h\"\nmax_active={max_active}\n")).expect("cfg");
    p
}

fn setup() -> (tempfile::TempDir, HelperDir) {
    let tmp = tempfile::TempDir::new().expect("tmp");
    let dir = HelperDir(tmp.path().join("h"));
    dir.init(None, "11111111-1111-4111-8111-111111111111").expect("init");
    (tmp, dir)
}

fn locked_elsewhere(dir: &HelperDir) -> bool {
    let f = fs::OpenOptions::new().read(true).write(true).open(dir.0.join("state.lock")).expect("open lock");
    matches!(f.try_lock(), Err(fs::TryLockError::WouldBlock))
}

#[test]
fn concurrent_attestations_serialize_generation_updates() {
    let (tmp, dir) = setup();
    let n = 8u32;
    let mut kids: Vec<Kid> = (0..n).map(|i| Kid::spawn("attest", &dir.0, &cfg(tmp.path(), &format!("c{i}.toml"), 10 + i))).collect();
    for k in &mut kids {
        assert_eq!(k.line(), "ready");
    }
    for k in &mut kids {
        k.send("go");
    }
    let mut gens: Vec<u64> = kids
        .iter_mut()
        .map(|k| k.line().strip_prefix("done ").and_then(|g| g.parse().ok()).expect("done <gen>"))
        .collect();
    gens.sort_unstable();
    println!("CONCURRENCY generations reported by {n} processes: {gens:?}");
    // Every process computed a different set, so each one is one increment: 2..=n+1, each once.
    assert_eq!(gens, (2..=u64::from(n) + 1).collect::<Vec<_>>());
    let st = HelperState::parse(&fs::read(dir.0.join("state")).unwrap()).unwrap();
    assert_eq!(st.generation, u64::from(n) + 1);
    assert!(!locked_elsewhere(&dir), "all children released the lock");
}

#[test]
fn state_cannot_change_between_comparison_and_apply_within_the_held_lock() {
    let (tmp, dir) = setup();
    let c = cfg(tmp.path(), "a.toml", 5);
    let mut holder = Kid::spawn("hold_between_compare_and_apply", &dir.0, &c);
    assert_eq!(holder.line(), "compared");
    // The helper is between compare and apply: nobody else can take the lock.
    assert!(locked_elsewhere(&dir));
    let at_compare = fs::read(dir.0.join("state")).unwrap();
    // A second helper instance wants to change the state; it cannot finish while the first holds the lock.
    let mut other = Kid::spawn("attest", &dir.0, &cfg(tmp.path(), "b.toml", 9));
    assert_eq!(other.line(), "ready");
    other.send("go");
    assert_eq!(fs::read(dir.0.join("state")).unwrap(), at_compare, "state unchanged while the first holds the lock");
    assert!(locked_elsewhere(&dir));
    holder.send("go");
    let r = holder.line();
    println!("I2 first instance: {r}");
    assert!(r.starts_with("result Ok"), "{r}");
    let done = other.line();
    assert!(done.starts_with("done "), "{done}");
    // The second instance only advanced the state after the first finished: its generation
    // follows the one the first compared against.
    let g_first: u64 = HelperState::parse(&at_compare).unwrap().generation;
    let g_other: u64 = done["done ".len()..].parse().unwrap();
    assert_eq!(g_other, g_first + 1);
}

#[test]
fn interrupted_state_write_kill_9_leaves_old_file_and_next_request_advances_once() {
    let (tmp, dir) = setup();
    let before = fs::read(dir.0.join("state")).unwrap();
    let g0 = HelperState::parse(&before).unwrap().generation;
    let mut kid = Kid::spawn("attest_block_after_tmp", &dir.0, &cfg(tmp.path(), "k.toml", 7));
    assert_eq!(kid.line(), "tmp_written");
    assert!(dir.0.join("state.tmp").exists());
    assert!(locked_elsewhere(&dir));
    kid.child.kill().expect("SIGKILL");
    kid.child.wait().expect("reap");
    // Kernel released the flock with the process; the old state file is intact; tmp is left over.
    assert!(!locked_elsewhere(&dir), "flock released on process death");
    assert_eq!(fs::read(dir.0.join("state")).unwrap(), before);
    assert!(dir.0.join("state.tmp").exists(), "leftover tmp after the kill");
    // Next helper instance: removes the leftover under the lock, advances exactly once.
    let mut next = Kid::spawn("attest", &dir.0, &cfg(tmp.path(), "k.toml", 7));
    assert_eq!(next.line(), "ready");
    next.send("go");
    assert_eq!(next.line(), format!("done {}", g0 + 1));
    assert!(!dir.0.join("state.tmp").exists());
    assert_eq!(HelperState::parse(&fs::read(dir.0.join("state")).unwrap()).unwrap().generation, g0 + 1);
}
