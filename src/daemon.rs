use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("failed to bind ephemeral port: {0}")]
    PortBind(std::io::Error),
    #[error("failed to spawn daemon: {0}")]
    Spawn(std::io::Error),
    #[error("daemon did not become healthy within {timeout}s")]
    HealthTimeout { timeout: u64 },
}

pub struct Daemon {
    pub port: u16,
    pub auth_token: String,
    child: Option<Child>,
    #[allow(dead_code)]
    stdout_path: String,
    #[allow(dead_code)]
    stderr_path: String,
}

impl Daemon {
    /// Spawns the DeepSeek-TUI daemon with an ephemeral port and auth token.
    ///
    /// Uses a `TcpListener` bound to `:0` to discover a free port, drops the
    /// listener (accepting a small TOCTOU race — acceptable for v0.1), and
    /// passes the port + token to `deepseek-tui serve`.
    ///
    /// Stdout and stderr are captured to files in a per-run temp directory for
    /// debugging when things go wrong.
    pub fn spawn(binary: &Path) -> Result<Self, DaemonError> {
        // Pick an ephemeral port.
        let listener = TcpListener::bind("127.0.0.1:0").map_err(DaemonError::PortBind)?;
        let port = listener
            .local_addr()
            .map_err(|e| DaemonError::PortBind(std::io::Error::new(std::io::ErrorKind::Other, e)))?
            .port();
        drop(listener); // TOCTOU race — acceptable for v0.1.

        let auth_token = uuid::Uuid::new_v4().to_string();

        // Create a temp dir for daemon logs.
        let run_dir = std::env::temp_dir().join(format!("deepseek-tui-modes-{}", &auth_token[..8]));
        std::fs::create_dir_all(&run_dir).ok();
        let stdout_path = run_dir.join("daemon.stdout").to_string_lossy().to_string();
        let stderr_path = run_dir.join("daemon.stderr").to_string_lossy().to_string();

        let stdout_file = std::fs::File::create(&stdout_path)
            .map_err(|e| DaemonError::Spawn(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let stderr_file = std::fs::File::create(&stderr_path)
            .map_err(|e| DaemonError::Spawn(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        let child = Command::new(binary)
            .args([
                "serve",
                "--http",
                "--port",
                &port.to_string(),
                "--auth-token",
                &auth_token,
            ])
            .stdin(Stdio::piped())
            .stdout(stdout_file)
            .stderr(stderr_file)
            .spawn()
            .map_err(DaemonError::Spawn)?;

        Ok(Daemon {
            port,
            auth_token,
            child: Some(child),
            stdout_path,
            stderr_path,
        })
    }

    /// Kills the daemon process and waits for it to exit.
    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Polls `GET /health` against the daemon until it returns 200 or the timeout
/// elapses. Exponential backoff: 50ms initial, 5s ceiling, 10s total budget.
pub fn wait_for_health(port: u16) -> Result<(), DaemonError> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client build should never fail");

    let url = format!("http://127.0.0.1:{port}/health");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut delay = Duration::from_millis(50);

    while std::time::Instant::now() < deadline {
        match client.get(&url).send() {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => {}
        }
        std::thread::sleep(delay);
        delay = std::cmp::min(delay * 2, Duration::from_secs(5));
    }

    Err(DaemonError::HealthTimeout { timeout: 10 })
}
