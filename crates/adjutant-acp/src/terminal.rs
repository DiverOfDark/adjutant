//! Client-side terminals for ACP `terminal/*` requests.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExitInfo {
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}

struct Output {
    data: String,
    truncated: bool,
    limit: Option<usize>,
}

impl Output {
    fn push(&mut self, chunk: &str) {
        self.data.push_str(chunk);
        if let Some(limit) = self.limit
            && self.data.len() > limit
        {
            let mut cut = self.data.len() - limit;
            while !self.data.is_char_boundary(cut) {
                cut += 1;
            }
            self.data.drain(..cut);
            self.truncated = true;
        }
    }
}

struct Terminal {
    output: Mutex<Output>,
    exit: watch::Receiver<Option<ExitInfo>>,
    kill: mpsc::Sender<()>,
}

#[derive(Default)]
pub struct Terminals {
    map: Mutex<HashMap<String, Arc<Terminal>>>,
    next: AtomicU64,
}

impl Terminals {
    /// Spawns `cmd` (already configured with program, args, cwd, env).
    pub fn spawn(
        &self,
        mut cmd: tokio::process::Command,
        output_byte_limit: Option<u64>,
    ) -> std::io::Result<String> {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (exit_tx, exit_rx) = watch::channel(None);
        let (kill_tx, mut kill_rx) = mpsc::channel(1);
        let term = Arc::new(Terminal {
            output: Mutex::new(Output {
                data: String::new(),
                truncated: false,
                limit: output_byte_limit.map(|l| l as usize),
            }),
            exit: exit_rx,
            kill: kill_tx,
        });

        let readers = [
            stdout.map(|s| pump(s, term.clone())),
            stderr.map(|s| pump(s, term.clone())),
        ];
        tokio::spawn(async move {
            let status = tokio::select! {
                status = child.wait() => status,
                _ = kill_rx.recv() => {
                    let _ = child.kill().await;
                    child.wait().await
                }
            };
            for reader in readers.into_iter().flatten() {
                let _ = reader.await;
            }
            let info = match status {
                Ok(status) => exit_info(status),
                Err(_) => ExitInfo::default(),
            };
            let _ = exit_tx.send(Some(info));
        });

        let id = format!("term-{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
        self.map.lock().unwrap().insert(id.clone(), term);
        Ok(id)
    }

    /// Current output, whether it was truncated, and exit status if finished.
    pub fn output(&self, id: &str) -> Option<(String, bool, Option<ExitInfo>)> {
        let term = self.get(id)?;
        let out = term.output.lock().unwrap();
        let exit = term.exit.borrow().clone();
        Some((out.data.clone(), out.truncated, exit))
    }

    pub async fn wait(&self, id: &str) -> Option<ExitInfo> {
        let mut rx = self.get(id)?.exit.clone();
        rx.wait_for(|e| e.is_some()).await.ok()?.clone()
    }

    pub async fn kill(&self, id: &str) {
        if let Some(term) = self.get(id) {
            let _ = term.kill.try_send(());
        }
    }

    /// Kills (if still running) and forgets the terminal.
    pub async fn release(&self, id: &str) {
        let term = self.map.lock().unwrap().remove(id);
        if let Some(term) = term {
            let _ = term.kill.try_send(());
        }
    }

    fn get(&self, id: &str) -> Option<Arc<Terminal>> {
        self.map.lock().unwrap().get(id).cloned()
    }
}

fn pump(
    mut stream: impl AsyncRead + Unpin + Send + 'static,
    term: Arc<Terminal>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    term.output.lock().unwrap().push(&chunk);
                }
            }
        }
    })
}

fn exit_info(status: std::process::ExitStatus) -> ExitInfo {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|s| s.to_string())
    };
    #[cfg(not(unix))]
    let signal = None;
    ExitInfo {
        exit_code: status.code().map(|c| c as u32),
        signal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_truncates_from_front() {
        let mut out = Output {
            data: String::new(),
            truncated: false,
            limit: Some(5),
        };
        out.push("hello world");
        assert_eq!(out.data, "world");
        assert!(out.truncated);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_command_and_collects_output() {
        let terms = Terminals::default();
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", "echo hi; echo err >&2; exit 3"]);
        let id = terms.spawn(cmd, None).unwrap();
        let exit = terms.wait(&id).await.unwrap();
        assert_eq!(exit.exit_code, Some(3));
        let (out, truncated, _) = terms.output(&id).unwrap();
        assert!(out.contains("hi") && out.contains("err"));
        assert!(!truncated);
    }
}
