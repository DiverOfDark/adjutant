//! Phase 0 spike: spawn an ACP agent in a vault, send one prompt, print the
//! streamed updates, and answer permission prompts on stdin.
//!
//! ```sh
//! cargo run -p adjutant-acp --example acp_spike -- \
//!     --vault examples/demo-vault --agent claude-agent-acp \
//!     "List the folders in this vault and what they are for"
//! ```
//!
//! `--agent junie` runs `junie --acp=true`. `--dump-init` prints the full
//! `initialize` response, which is how docs/acp-spike.md was produced.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use adjutant_acp::{AgentEvent, AgentHandle, AgentLaunch, AskAlways, PromptPart, Sandbox};
use tokio::sync::mpsc;

struct Args {
    vault: PathBuf,
    agent: String,
    extra_args: Vec<String>,
    dump_init: bool,
    no_mise: bool,
    prompt: String,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut out = Args {
        vault: PathBuf::from("."),
        agent: "claude-agent-acp".into(),
        extra_args: Vec::new(),
        dump_init: false,
        no_mise: false,
        prompt: String::new(),
    };
    let mut rest = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--vault" => out.vault = args.next().expect("--vault <dir>").into(),
            "--agent" => out.agent = args.next().expect("--agent <name>"),
            "--arg" => out.extra_args.push(args.next().expect("--arg <value>")),
            "--dump-init" => out.dump_init = true,
            "--no-mise" => out.no_mise = true,
            _ => rest.push(a),
        }
    }
    out.prompt = rest.join(" ");
    out
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".into()))
        .with_writer(std::io::stderr)
        .init();
    let args = parse_args();
    let vault = dunce::canonicalize(&args.vault)?;

    let default_args: Vec<String> = match args.agent.as_str() {
        "junie" => vec!["--acp=true".into()],
        "gemini" => vec!["--acp".into()],
        _ => vec![],
    };
    let launch = AgentLaunch {
        id: args.agent.clone(),
        program: args.agent.clone(),
        args: default_args.into_iter().chain(args.extra_args).collect(),
        env: Default::default(),
        cwd: vault.clone(),
        use_mise: !args.no_mise,
        mise_bin: None,
    };
    eprintln!(
        "» spawning: {}  (cwd {})",
        launch.display(),
        vault.display()
    );

    let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
    let sink = Arc::new(move |e: AgentEvent| {
        let _ = tx.send(e);
    });
    let agent = Arc::new(
        AgentHandle::start(launch, Sandbox::new([vault]), Arc::new(AskAlways), sink).await?,
    );

    let info = agent.info();
    eprintln!("» agent: {}", info.agent_info);
    if args.dump_init {
        println!("{}", serde_json::to_string_pretty(info)?);
    }
    if args.prompt.is_empty() {
        return Ok(());
    }

    let session = agent.new_session(vec![]).await?;
    eprintln!("» session {session}");

    let prompt_agent = agent.clone();
    let prompt = args.prompt.clone();
    let sid = session.clone();
    let mut turn = tokio::spawn(async move {
        prompt_agent
            .prompt(&sid, vec![PromptPart::Text { text: prompt }])
            .await
    });

    loop {
        tokio::select! {
            result = &mut turn => {
                println!();
                eprintln!("» stop reason: {}", result??);
                break;
            }
            Some(event) = rx.recv() => print_event(&agent, event).await,
        }
    }
    Ok(())
}

async fn print_event(agent: &AgentHandle, event: AgentEvent) {
    match event {
        AgentEvent::SessionUpdate { update, .. } => {
            let kind = update["sessionUpdate"].as_str().unwrap_or("?");
            match kind {
                "agent_message_chunk" => {
                    print!("{}", update["content"]["text"].as_str().unwrap_or(""));
                    let _ = std::io::stdout().flush();
                }
                "agent_thought_chunk" => {}
                "tool_call" => eprintln!(
                    "\n» tool [{}] {} ({})",
                    update["kind"].as_str().unwrap_or("other"),
                    update["title"].as_str().unwrap_or(""),
                    update["status"].as_str().unwrap_or("pending"),
                ),
                "tool_call_update" => {
                    if let Some(status) = update["status"].as_str() {
                        eprintln!(
                            "» tool {} → {status}",
                            update["toolCallId"].as_str().unwrap_or("")
                        );
                    }
                }
                "plan" => {
                    eprintln!("» plan:");
                    for e in update["entries"].as_array().into_iter().flatten() {
                        eprintln!(
                            "   [{}] {}",
                            e["status"].as_str().unwrap_or(""),
                            e["content"].as_str().unwrap_or("")
                        );
                    }
                }
                other => eprintln!("» update {other}: {update}"),
            }
        }
        AgentEvent::PermissionRequest {
            request_id,
            tool_call,
            options,
            ..
        } => {
            eprintln!(
                "\n» permission: {}",
                tool_call["title"].as_str().unwrap_or("tool call")
            );
            let opts: Vec<_> = options.as_array().cloned().unwrap_or_default();
            for (i, o) in opts.iter().enumerate() {
                eprintln!(
                    "   {}) {} [{}]",
                    i + 1,
                    o["name"].as_str().unwrap_or(""),
                    o["kind"].as_str().unwrap_or("")
                );
            }
            eprint!("   choose (empty = cancel): ");
            let choice = tokio::task::spawn_blocking(|| {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).ok();
                line
            })
            .await
            .unwrap_or_default();
            let picked = choice
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|n| opts.get(n.wrapping_sub(1)))
                .and_then(|o| o["optionId"].as_str().map(String::from));
            agent.respond_permission(request_id, picked);
        }
        AgentEvent::FileRead { path, .. } => eprintln!("» fs read  {}", path.display()),
        AgentEvent::FileWritten { path, .. } => eprintln!("» fs write {}", path.display()),
        AgentEvent::FileAccessDenied { path, reason, .. } => {
            eprintln!("» fs DENIED {}: {reason}", path.display())
        }
        AgentEvent::TerminalStarted { command, .. } => eprintln!("» terminal: {command}"),
        AgentEvent::Stderr { line } => eprintln!("  [agent stderr] {line}"),
        AgentEvent::Error { message } => eprintln!("» error: {message}"),
        AgentEvent::Exited { code } => eprintln!("» agent exited ({code:?})"),
        AgentEvent::Ready { .. }
        | AgentEvent::PromptFinished { .. }
        | AgentEvent::PermissionDecided { .. } => {}
    }
}
