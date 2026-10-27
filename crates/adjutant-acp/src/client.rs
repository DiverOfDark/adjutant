//! The ACP client connection: spawns the agent, serves its `fs/*`,
//! `terminal/*` and permission requests, and exposes a small command API.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AuthenticateRequest, CancelNotification, ClientCapabilities, ContentBlock,
    CreateTerminalRequest, CreateTerminalResponse, ErrorCode, FileSystemCapabilities,
    Implementation, InitializeRequest, KillTerminalRequest, KillTerminalResponse, McpServer,
    NewSessionRequest, PermissionOption, PermissionOptionKind, PromptRequest, ReadTextFileRequest,
    ReadTextFileResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse, ResourceLink,
    SelectedPermissionOutcome, SessionNotification, TerminalExitStatus, TerminalOutputRequest,
    TerminalOutputResponse, TextContent, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo, Error as AcpError};
use anyhow::{Context as _, anyhow};
use tokio::io::AsyncBufReadExt;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::events::{AgentEvent, EventSink, PermissionPolicy, PolicyDecision};
use crate::launch::AgentLaunch;
use crate::sandbox::Sandbox;
use crate::terminal::{ExitInfo, Terminals};

/// How long `initialize` may take. `mise exec` installs missing tools on
/// first run, so this is generous.
const INIT_TIMEOUT: Duration = Duration::from_secs(180);

/// One part of a user prompt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PromptPart {
    Text {
        text: String,
    },
    /// A file the agent should look at (sent as an ACP resource link).
    File {
        path: PathBuf,
    },
}

impl From<&str> for PromptPart {
    fn from(text: &str) -> Self {
        PromptPart::Text {
            text: text.to_string(),
        }
    }
}

/// What the agent reported during `initialize`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub agent_info: serde_json::Value,
    pub agent_capabilities: serde_json::Value,
    pub protocol_version: serde_json::Value,
    /// ACP auth methods; non-empty when the agent may need a login.
    pub auth_methods: serde_json::Value,
}

/// Marker in error messages when the agent needs a login first.
pub const AUTH_REQUIRED: &str = "auth_required";

enum Cmd {
    NewSession {
        mcp_servers: Vec<serde_json::Value>,
        reply: oneshot::Sender<anyhow::Result<String>>,
    },
    Prompt {
        session_id: String,
        parts: Vec<PromptPart>,
        reply: oneshot::Sender<anyhow::Result<serde_json::Value>>,
    },
    Cancel {
        session_id: String,
    },
    Authenticate {
        method_id: String,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
}

struct PendingPermission {
    session_id: String,
    reply: oneshot::Sender<Option<String>>,
}

struct Shared {
    launch: AgentLaunch,
    sandbox: Sandbox,
    sink: Arc<dyn EventSink>,
    policy: Arc<dyn PermissionPolicy>,
    terminals: Terminals,
    pending: Mutex<HashMap<u64, PendingPermission>>,
    next_permission: AtomicU64,
}

impl Shared {
    fn emit(&self, event: AgentEvent) {
        self.sink.emit(event);
    }

    /// Resolves every pending permission prompt of `session_id` as cancelled.
    fn cancel_pending(&self, session_id: &str) {
        let mut pending = self.pending.lock().unwrap();
        let ids: Vec<u64> = pending
            .iter()
            .filter(|(_, p)| p.session_id == session_id)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(p) = pending.remove(&id) {
                let _ = p.reply.send(None);
            }
        }
    }
}

/// A running agent. Dropping the handle shuts the agent down.
pub struct AgentHandle {
    cmd_tx: mpsc::UnboundedSender<Cmd>,
    shared: Arc<Shared>,
    info: AgentInfo,
    task: tokio::task::JoinHandle<()>,
    stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl AgentHandle {
    /// Spawns the agent and runs the ACP `initialize` handshake.
    pub async fn start(
        launch: AgentLaunch,
        sandbox: Sandbox,
        policy: Arc<dyn PermissionPolicy>,
        sink: Arc<dyn EventSink>,
    ) -> anyhow::Result<Self> {
        let mut cmd = launch.agent_command();
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0); // so the whole tree (mise -> npx -> node) can be killed
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn agent `{}`", launch.display()))?;
        let stdin = child.stdin.take().context("agent stdin")?;
        let stdout = child.stdout.take().context("agent stdout")?;
        let stderr = child.stderr.take().context("agent stderr")?;

        let shared = Arc::new(Shared {
            launch,
            sandbox,
            sink,
            policy,
            terminals: Terminals::default(),
            pending: Mutex::new(HashMap::new()),
            next_permission: AtomicU64::new(1),
        });

        {
            let shared = shared.clone();
            tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "agent_stderr", "{line}");
                    shared.emit(AgentEvent::Stderr { line });
                }
            });
        }

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());

        let task = {
            let shared = shared.clone();
            tokio::spawn(async move {
                let pid = child.id();
                let connection = run_connection(transport, shared.clone(), cmd_rx, ready_tx);
                let code = tokio::select! {
                    result = connection => {
                        if let Err(e) = result {
                            shared.emit(AgentEvent::Error { message: e.to_string() });
                        }
                        None
                    }
                    status = child.wait() => status.ok().and_then(|s| s.code()),
                    _ = stop_rx => None,
                };
                kill_tree(pid);
                let _ = child.kill().await;
                shared.emit(AgentEvent::Exited { code });
            })
        };

        let info = match tokio::time::timeout(INIT_TIMEOUT, ready_rx).await {
            Ok(Ok(Ok(info))) => info,
            Ok(Ok(Err(message))) => {
                task.abort();
                return Err(anyhow!("agent initialize failed: {message}"));
            }
            Ok(Err(_)) => {
                task.abort();
                return Err(anyhow!(
                    "agent exited before initialize completed (see stderr events)"
                ));
            }
            Err(_) => {
                task.abort();
                return Err(anyhow!(
                    "agent did not answer initialize within {INIT_TIMEOUT:?}"
                ));
            }
        };
        shared.emit(AgentEvent::Ready {
            agent_info: info.agent_info.clone(),
            agent_capabilities: info.agent_capabilities.clone(),
        });

        Ok(Self {
            cmd_tx,
            shared,
            info,
            task,
            stop_tx: Mutex::new(Some(stop_tx)),
        })
    }

    pub fn info(&self) -> &AgentInfo {
        &self.info
    }

    pub fn launch(&self) -> &AgentLaunch {
        &self.shared.launch
    }

    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }

    /// Opens a new session with the vault as working directory.
    /// `mcp_servers` are ACP `McpServer` objects as JSON.
    pub async fn new_session(&self, mcp_servers: Vec<serde_json::Value>) -> anyhow::Result<String> {
        let (reply, rx) = oneshot::channel();
        self.send(Cmd::NewSession { mcp_servers, reply })?;
        rx.await.map_err(|_| anyhow!("agent connection closed"))?
    }

    /// Sends a prompt and waits for the turn to end. Streamed output arrives
    /// as [`AgentEvent::SessionUpdate`] events meanwhile. Returns the stop reason.
    pub async fn prompt(
        &self,
        session_id: &str,
        parts: Vec<PromptPart>,
    ) -> anyhow::Result<serde_json::Value> {
        let (reply, rx) = oneshot::channel();
        self.send(Cmd::Prompt {
            session_id: session_id.to_string(),
            parts,
            reply,
        })?;
        rx.await.map_err(|_| anyhow!("agent connection closed"))?
    }

    /// Runs an ACP `authenticate` with one of [`AgentInfo::auth_methods`].
    pub async fn authenticate(&self, method_id: &str) -> anyhow::Result<()> {
        let (reply, rx) = oneshot::channel();
        self.send(Cmd::Authenticate {
            method_id: method_id.to_string(),
            reply,
        })?;
        rx.await.map_err(|_| anyhow!("agent connection closed"))?
    }

    /// Asks the agent to stop the current turn.
    pub fn cancel(&self, session_id: &str) -> anyhow::Result<()> {
        self.send(Cmd::Cancel {
            session_id: session_id.to_string(),
        })
    }

    /// Answers a [`AgentEvent::PermissionRequest`]. `None` cancels it.
    pub fn respond_permission(&self, request_id: u64, option_id: Option<String>) -> bool {
        let pending = self.shared.pending.lock().unwrap().remove(&request_id);
        match pending {
            Some(p) => {
                self.shared.emit(AgentEvent::PermissionDecided {
                    request_id,
                    option_id: option_id.clone(),
                    automatic: false,
                });
                p.reply.send(option_id).is_ok()
            }
            None => false,
        }
    }

    /// Stops the agent process tree. Pending prompts fail with
    /// "agent connection closed".
    pub fn stop(&self) {
        if let Some(tx) = self.stop_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }

    fn send(&self, cmd: Cmd) -> anyhow::Result<()> {
        self.cmd_tx
            .send(cmd)
            .map_err(|_| anyhow!("agent is not running"))
    }
}

async fn run_connection<T>(
    transport: T,
    shared: Arc<Shared>,
    mut cmd_rx: mpsc::UnboundedReceiver<Cmd>,
    ready_tx: oneshot::Sender<Result<AgentInfo, String>>,
) -> Result<(), AcpError>
where
    T: agent_client_protocol::ConnectTo<Client> + 'static,
{
    let s_notify = shared.clone();
    let s_perm = shared.clone();
    let s_read = shared.clone();
    let s_write = shared.clone();
    let s_term = shared.clone();
    let s_out = shared.clone();
    let s_wait = shared.clone();
    let s_kill = shared.clone();
    let s_release = shared.clone();
    let s_main = shared;

    Client
        .builder()
        .name("adjutant")
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                s_notify.emit(AgentEvent::SessionUpdate {
                    session_id: n.session_id.to_string(),
                    update: serde_json::to_value(&n.update).unwrap_or_default(),
                });
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, cx: ConnectionTo<Agent>| {
                let shared = s_perm.clone();
                let session_id = req.session_id.to_string();
                let tool_call = serde_json::to_value(&req.tool_call).unwrap_or_default();
                let request_id = shared.next_permission.fetch_add(1, Ordering::Relaxed);

                let auto = match shared.policy.decide(&shared.launch.id, &tool_call) {
                    PolicyDecision::Ask => None,
                    PolicyDecision::Allow => Some(pick_option(
                        &req.options,
                        &[
                            PermissionOptionKind::AllowOnce,
                            PermissionOptionKind::AllowAlways,
                        ],
                    )),
                    PolicyDecision::Reject => Some(pick_option(
                        &req.options,
                        &[
                            PermissionOptionKind::RejectOnce,
                            PermissionOptionKind::RejectAlways,
                        ],
                    )),
                };
                if let Some(option_id) = auto {
                    shared.emit(AgentEvent::PermissionDecided {
                        request_id,
                        option_id: option_id.clone(),
                        automatic: true,
                    });
                    return responder.respond(permission_response(option_id));
                }

                let (reply, rx) = oneshot::channel();
                shared.pending.lock().unwrap().insert(
                    request_id,
                    PendingPermission {
                        session_id: session_id.clone(),
                        reply,
                    },
                );
                shared.emit(AgentEvent::PermissionRequest {
                    request_id,
                    session_id,
                    tool_call,
                    options: serde_json::to_value(&req.options).unwrap_or_default(),
                });
                // Waiting for the user must not block the dispatch loop.
                cx.spawn(async move {
                    let option_id = rx.await.ok().flatten();
                    responder.respond(permission_response(option_id))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: ReadTextFileRequest, responder, _cx| {
                let session_id = req.session_id.to_string();
                let path = match s_read.sandbox.check(&req.path) {
                    Ok(p) => p,
                    Err(e) => {
                        s_read.emit(AgentEvent::FileAccessDenied {
                            session_id,
                            path: req.path.clone(),
                            reason: e.to_string(),
                        });
                        return responder.respond_with_result(Err(denied(e)));
                    }
                };
                match tokio::fs::read_to_string(&path).await {
                    Ok(content) => {
                        s_read.emit(AgentEvent::FileRead { session_id, path });
                        let content = slice_lines(&content, req.line, req.limit);
                        responder.respond(ReadTextFileResponse::new(content))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => responder
                        .respond_with_result(Err(AcpError::resource_not_found(Some(
                            path.display().to_string(),
                        )))),
                    Err(e) => responder.respond_with_result(Err(AcpError::into_internal_error(e))),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: WriteTextFileRequest, responder, _cx| {
                let session_id = req.session_id.to_string();
                let path = match s_write.sandbox.check(&req.path) {
                    Ok(p) => p,
                    Err(e) => {
                        s_write.emit(AgentEvent::FileAccessDenied {
                            session_id,
                            path: req.path.clone(),
                            reason: e.to_string(),
                        });
                        return responder.respond_with_result(Err(denied(e)));
                    }
                };
                let result = async {
                    if let Some(parent) = path.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::write(&path, req.content.as_bytes()).await
                }
                .await;
                match result {
                    Ok(()) => {
                        s_write.emit(AgentEvent::FileWritten { session_id, path });
                        responder.respond(WriteTextFileResponse::new())
                    }
                    Err(e) => responder.respond_with_result(Err(AcpError::into_internal_error(e))),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: CreateTerminalRequest, responder, _cx| {
                let shared = &s_term;
                let session_id = req.session_id.to_string();
                let cwd = match &req.cwd {
                    Some(cwd) => match shared.sandbox.check(cwd) {
                        Ok(p) => p,
                        Err(e) => return responder.respond_with_result(Err(denied(e))),
                    },
                    None => shared.launch.cwd.clone(),
                };
                let (program, args) = shell_split(&req.command, &req.args);
                let mut cmd = shared.launch.command_for(&program, &args);
                cmd.current_dir(&cwd);
                for var in &req.env {
                    cmd.env(&var.name, &var.value);
                }
                match shared.terminals.spawn(cmd, req.output_byte_limit) {
                    Ok(id) => {
                        shared.emit(AgentEvent::TerminalStarted {
                            session_id,
                            terminal_id: id.clone(),
                            command: std::iter::once(program)
                                .chain(args)
                                .collect::<Vec<_>>()
                                .join(" "),
                        });
                        responder.respond(CreateTerminalResponse::new(id))
                    }
                    Err(e) => responder.respond_with_result(Err(AcpError::into_internal_error(e))),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: TerminalOutputRequest, responder, _cx| match s_out
                .terminals
                .output(&req.terminal_id.to_string())
            {
                Some((output, truncated, exit)) => responder.respond(
                    TerminalOutputResponse::new(output, truncated)
                        .exit_status(exit.map(exit_status)),
                ),
                None => responder.respond_with_result(Err(unknown_terminal())),
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: WaitForTerminalExitRequest, responder, cx: ConnectionTo<Agent>| {
                let shared = s_wait.clone();
                cx.spawn(async move {
                    match shared.terminals.wait(&req.terminal_id.to_string()).await {
                        Some(exit) => {
                            responder.respond(WaitForTerminalExitResponse::new(exit_status(exit)))
                        }
                        None => responder.respond_with_result(Err(unknown_terminal())),
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: KillTerminalRequest, responder, _cx| {
                s_kill.terminals.kill(&req.terminal_id.to_string()).await;
                responder.respond(KillTerminalResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: ReleaseTerminalRequest, responder, _cx| {
                s_release
                    .terminals
                    .release(&req.terminal_id.to_string())
                    .await;
                responder.respond(ReleaseTerminalResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, async move |cx: ConnectionTo<Agent>| {
            let shared = s_main;
            let caps = ClientCapabilities::new()
                .fs(FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(true))
                .terminal(true);
            let init = cx
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(caps)
                        .client_info(
                            Implementation::new("adjutant", env!("CARGO_PKG_VERSION"))
                                .title("Adjutant"),
                        ),
                )
                .block_task()
                .await;
            let init = match init {
                Ok(init) => init,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return Err(e);
                }
            };
            let _ = ready_tx.send(Ok(AgentInfo {
                agent_info: serde_json::to_value(&init.agent_info).unwrap_or_default(),
                agent_capabilities: serde_json::to_value(&init.agent_capabilities)
                    .unwrap_or_default(),
                protocol_version: serde_json::to_value(init.protocol_version).unwrap_or_default(),
                auth_methods: serde_json::to_value(&init.auth_methods).unwrap_or_default(),
            }));

            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    Cmd::NewSession { mcp_servers, reply } => {
                        let cx2 = cx.clone();
                        let cwd = shared.launch.cwd.clone();
                        cx.spawn(async move {
                            let result = async {
                                let servers = mcp_servers
                                    .into_iter()
                                    .map(serde_json::from_value::<McpServer>)
                                    .collect::<Result<Vec<_>, _>>()
                                    .context("invalid MCP server config")?;
                                let resp = cx2
                                    .send_request(NewSessionRequest::new(cwd).mcp_servers(servers))
                                    .block_task()
                                    .await
                                    .map_err(|e| {
                                        if e.code == ErrorCode::AuthRequired {
                                            anyhow!("{AUTH_REQUIRED}: {}", e.message)
                                        } else {
                                            anyhow::Error::from(e)
                                        }
                                    })?;
                                Ok(resp.session_id.to_string())
                            }
                            .await;
                            let _ = reply.send(result);
                            Ok(())
                        })?;
                    }
                    Cmd::Prompt {
                        session_id,
                        parts,
                        reply,
                    } => {
                        let cx2 = cx.clone();
                        let shared = shared.clone();
                        cx.spawn(async move {
                            let blocks = parts.into_iter().map(to_content_block).collect();
                            let result = cx2
                                .send_request(PromptRequest::new(session_id.clone(), blocks))
                                .block_task()
                                .await;
                            let result = result
                                .map(|r| serde_json::to_value(r.stop_reason).unwrap_or_default())
                                .map_err(anyhow::Error::from);
                            if let Ok(stop_reason) = &result {
                                shared.emit(AgentEvent::PromptFinished {
                                    session_id,
                                    stop_reason: stop_reason.clone(),
                                });
                            }
                            let _ = reply.send(result);
                            Ok(())
                        })?;
                    }
                    Cmd::Authenticate { method_id, reply } => {
                        let cx2 = cx.clone();
                        cx.spawn(async move {
                            let result = cx2
                                .send_request(AuthenticateRequest::new(method_id))
                                .block_task()
                                .await
                                .map(|_| ())
                                .map_err(anyhow::Error::from);
                            let _ = reply.send(result);
                            Ok(())
                        })?;
                    }
                    Cmd::Cancel { session_id } => {
                        shared.cancel_pending(&session_id);
                        cx.send_notification(CancelNotification::new(session_id))?;
                    }
                }
            }
            Ok(())
        })
        .await
}

fn pick_option(options: &[PermissionOption], kinds: &[PermissionOptionKind]) -> Option<String> {
    kinds.iter().find_map(|kind| {
        options
            .iter()
            .find(|o| &o.kind == kind)
            .map(|o| o.option_id.to_string())
    })
}

fn permission_response(option_id: Option<String>) -> RequestPermissionResponse {
    RequestPermissionResponse::new(match option_id {
        Some(id) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
        None => RequestPermissionOutcome::Cancelled,
    })
}

fn to_content_block(part: PromptPart) -> ContentBlock {
    match part {
        PromptPart::Text { text } => ContentBlock::Text(TextContent::new(text)),
        PromptPart::File { path } => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            ContentBlock::ResourceLink(ResourceLink::new(name, file_uri(&path)))
        }
    }
}

fn file_uri(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

fn denied(e: impl std::fmt::Display) -> AcpError {
    AcpError::invalid_params().data(serde_json::Value::String(e.to_string()))
}

fn unknown_terminal() -> AcpError {
    AcpError::invalid_params().data(serde_json::Value::String("unknown terminal id".into()))
}

fn exit_status(exit: ExitInfo) -> TerminalExitStatus {
    TerminalExitStatus::new()
        .exit_code(exit.exit_code)
        .signal(exit.signal)
}

/// 1-based `line` and `limit` from `fs/read_text_file`.
fn slice_lines(content: &str, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return content.to_string();
    }
    let start = line.unwrap_or(1).saturating_sub(1) as usize;
    let lines = content.split_inclusive('\n').skip(start);
    match limit {
        Some(n) => lines.take(n as usize).collect(),
        None => lines.collect(),
    }
}

/// Agents sometimes send a whole shell command line in `command` with no
/// args. Run those through the platform shell.
fn shell_split(command: &str, args: &[String]) -> (String, Vec<String>) {
    if args.is_empty() && command.contains(char::is_whitespace) {
        if cfg!(windows) {
            ("cmd".into(), vec!["/C".into(), command.into()])
        } else {
            ("sh".into(), vec!["-c".into(), command.into()])
        }
    } else {
        (command.to_string(), args.to_vec())
    }
}

/// Terminates the agent's whole process tree (mise -> npx -> node).
fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    unsafe {
        // The agent was spawned as a process-group leader.
        libc::killpg(pid as libc::pid_t, libc::SIGTERM);
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl Drop for AgentHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_lines() {
        let text = "a\nb\nc\nd\n";
        assert_eq!(slice_lines(text, Some(2), Some(2)), "b\nc\n");
        assert_eq!(slice_lines(text, None, None), text);
        assert_eq!(slice_lines(text, Some(4), None), "d\n");
    }

    #[test]
    fn splits_shell_lines() {
        let (p, a) = shell_split("ls -la", &[]);
        assert!(p == "sh" || p == "cmd");
        assert_eq!(a.last().unwrap(), "ls -la");
        let (p, a) = shell_split("git", &["status".into()]);
        assert_eq!((p.as_str(), a.len()), ("git", 1));
    }
}
