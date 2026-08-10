use std::{ffi::OsString, path::PathBuf, process::Stdio, time::Duration};

use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    sync::mpsc,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const DEFAULT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEvent {
    pub stream: OutputStream,
    pub line: String,
}

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: Option<PathBuf>,
    pub timeout: Option<Duration>,
    pub output_limit: usize,
}

impl ProcessSpec {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            args: Vec::new(),
            current_dir: None,
            timeout: None,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn current_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(path.into());
        self
    }

    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub const fn output_limit(mut self, bytes: usize) -> Self {
        self.output_limit = bytes;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl ProcessOutput {
    pub const fn success(&self) -> bool {
        matches!(self.exit_code, Some(0))
    }
}

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("the executable path is empty")]
    EmptyExecutable,
    #[error("could not start external tool `{executable}`: {source}")]
    Start {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("external tool I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("external tool task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("external tool execution was cancelled")]
    Cancelled,
    #[error("external tool exceeded its {0:?} timeout")]
    TimedOut(Duration),
}

#[derive(Debug, Clone, Default)]
pub struct ProcessRunner;

impl ProcessRunner {
    pub async fn run(
        &self,
        spec: ProcessSpec,
        cancellation: CancellationToken,
    ) -> Result<ProcessOutput, ProcessError> {
        self.run_streaming(spec, cancellation, None).await
    }

    pub async fn run_streaming(
        &self,
        spec: ProcessSpec,
        cancellation: CancellationToken,
        events: Option<mpsc::Sender<ProcessEvent>>,
    ) -> Result<ProcessOutput, ProcessError> {
        if spec.executable.as_os_str().is_empty() {
            return Err(ProcessError::EmptyExecutable);
        }

        let mut command = Command::new(&spec.executable);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(current_dir) = &spec.current_dir {
            command.current_dir(current_dir);
        }

        let mut child = command.spawn().map_err(|source| ProcessError::Start {
            executable: spec.executable.clone(),
            source,
        })?;
        let stdout = child.stdout.take().expect("stdout is configured as piped");
        let stderr = child.stderr.take().expect("stderr is configured as piped");

        let stdout_task = capture_stream(
            stdout,
            OutputStream::Stdout,
            spec.output_limit,
            events.clone(),
        );
        let stderr_task = capture_stream(stderr, OutputStream::Stderr, spec.output_limit, events);

        enum WaitResult {
            Exited(std::process::ExitStatus),
            Cancelled,
            TimedOut(Duration),
        }

        let mut wait = Box::pin(child.wait());
        let wait_result = if let Some(timeout) = spec.timeout {
            tokio::select! {
                result = &mut wait => WaitResult::Exited(result?),
                () = cancellation.cancelled() => WaitResult::Cancelled,
                () = tokio::time::sleep(timeout) => WaitResult::TimedOut(timeout),
            }
        } else {
            tokio::select! {
                result = &mut wait => WaitResult::Exited(result?),
                () = cancellation.cancelled() => WaitResult::Cancelled,
            }
        };
        drop(wait);

        let exit_status = match wait_result {
            WaitResult::Exited(status) => status,
            WaitResult::Cancelled => {
                terminate(&mut child).await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(ProcessError::Cancelled);
            }
            WaitResult::TimedOut(timeout) => {
                terminate(&mut child).await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(ProcessError::TimedOut(timeout));
            }
        };

        let stdout = stdout_task.await??;
        let stderr = stderr_task.await??;
        Ok(ProcessOutput {
            exit_code: exit_status.code(),
            stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        })
    }
}

async fn terminate(child: &mut tokio::process::Child) {
    if let Err(error) = child.kill().await {
        tracing::debug!(%error, "external tool was already stopped or could not be killed");
    }
    let _ = child.wait().await;
}

struct CapturedBytes {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture_stream<R>(
    stream: R,
    kind: OutputStream,
    limit: usize,
    events: Option<mpsc::Sender<ProcessEvent>>,
) -> JoinHandle<Result<CapturedBytes, std::io::Error>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
        let mut line = Vec::new();
        let mut truncated = false;

        loop {
            line.clear();
            let count = reader.read_until(b'\n', &mut line).await?;
            if count == 0 {
                break;
            }

            if let Some(sender) = &events {
                let text = String::from_utf8_lossy(&line)
                    .trim_end_matches(['\r', '\n'])
                    .to_owned();
                let _ = sender.try_send(ProcessEvent {
                    stream: kind,
                    line: text,
                });
            }

            let remaining = limit.saturating_sub(bytes.len());
            if remaining > 0 {
                bytes.extend_from_slice(&line[..line.len().min(remaining)]);
            }
            if line.len() > remaining {
                truncated = true;
            }
        }

        Ok(CapturedBytes { bytes, truncated })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_spec_preserves_argument_boundaries() {
        let spec = ProcessSpec::new("mkvmerge")
            .arg("--identify")
            .arg("a file; rm -rf never.mkv");

        assert_eq!(spec.args.len(), 2);
        assert_eq!(spec.args[1], OsString::from("a file; rm -rf never.mkv"));
    }

    #[test]
    fn process_output_success_requires_zero_exit_code() {
        let output = ProcessOutput {
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        };
        assert!(output.success());
    }
}
