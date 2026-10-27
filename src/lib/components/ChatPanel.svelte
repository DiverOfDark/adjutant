<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api, onAgentEvent, type Discovery } from "$lib/api";
  import { chat } from "$lib/chat.svelte";
  import Markdown from "./Markdown.svelte";
  import ToolCallCard from "./ToolCallCard.svelte";
  import PermissionCard from "./PermissionCard.svelte";

  let { vaultPath }: { vaultPath: string } = $props();

  let discovery = $state<Discovery | null>(null);
  let selected = $state<string | null>(null);
  let input = $state("");
  let scroller: HTMLDivElement | undefined = $state();
  let showStderr = $state(false);

  export async function sendPrompt(text: string) {
    if (!chat.sessionId) await connect();
    await chat.send(text);
  }

  async function discover() {
    discovery = await api.agentsDiscover();
    selected = chat.agentId ?? discovery.defaultAgent;
  }

  async function trust() {
    discovery = await api.agentsTrustVault();
  }

  async function connect() {
    await chat.connect(selected);
  }

  async function submit(e?: Event) {
    e?.preventDefault();
    const text = input;
    if (!text.trim()) return;
    input = "";
    if (!chat.sessionId) await connect();
    await chat.send(text);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) submit(e);
  }

  $effect(() => {
    chat.items.length;
    const last = chat.items.at(-1);
    if (last && "text" in last) last.text;
    tick().then(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  });

  $effect(() => {
    vaultPath;
    discover();
  });

  onMount(() => {
    const un = onAgentEvent((e) => chat.handle(e));
    return () => un.then((f) => f());
  });
</script>

<section class="chat">
  <header>
    <select bind:value={selected} disabled={chat.connecting || chat.busy} title="Agent for this session">
      {#each discovery?.agents ?? [] as a}
        <option value={a.id} disabled={!a.installed}>
          {a.name}{a.version ? ` ${a.version}` : ""}{a.installed ? "" : " — not installed"}
        </option>
      {/each}
    </select>
    {#if chat.sessionId && chat.agentId === selected}
      <button onclick={() => chat.newSession()} disabled={chat.busy}>New chat</button>
    {:else}
      <button class="primary" onclick={connect} disabled={chat.connecting || !selected}>
        {chat.connecting ? "Starting…" : "Start"}
      </button>
    {/if}
    <span class="who" title={vaultPath}>{chat.agentName ? `● ${chat.agentName}` : "○ not connected"}</span>
  </header>

  {#if discovery?.miseUntrusted}
    <div class="banner">
      mise does not trust this vault's <code>mise.toml</code> yet.
      <button onclick={trust}>Trust it</button>
    </div>
  {:else if discovery?.miseError}
    <div class="banner error">{discovery.miseError}</div>
  {/if}
  {#if selected && discovery && !discovery.agents.find((a) => a.id === selected)?.installed && !discovery.miseError}
    {@const a = discovery.agents.find((a) => a.id === selected)}
    <div class="banner">Add to the vault's <code>mise.toml</code>: <code>{a?.miseLine}</code></div>
  {/if}

  <div class="log" bind:this={scroller}>
    {#if chat.items.length === 0}
      <div class="empty">Ask about your notes, plan your day, or paste a repo URL.</div>
    {/if}
    {#each chat.items as item (item.id)}
      {#if item.kind === "user"}
        <div class="msg user">{item.text}</div>
      {:else if item.kind === "agent"}
        <div class="msg agent"><Markdown text={item.text} /></div>
      {:else if item.kind === "thought"}
        <details class="thought"><summary>Thinking</summary>{item.text}</details>
      {:else if item.kind === "tool"}
        <ToolCallCard call={item} />
      {:else if item.kind === "plan"}
        <div class="plan">
          {#each item.entries as e}
            <div class="entry" data-status={e.status}>
              {e.status === "completed" ? "☑" : e.status === "in_progress" ? "◐" : "☐"} {e.content}
            </div>
          {/each}
        </div>
      {:else if item.kind === "permission"}
        <PermissionCard requestId={item.requestId} toolCall={item.toolCall} options={item.options} decided={item.decided} />
      {:else if item.kind === "notice"}
        <div class="notice {item.level}">{item.text}</div>
      {/if}
    {/each}
    {#if chat.busy}<div class="working">working…</div>{/if}
  </div>

  <form onsubmit={submit}>
    <textarea
      bind:value={input}
      onkeydown={onKey}
      rows="3"
      placeholder={chat.sessionId ? "Message the agent (Enter to send, Shift+Enter for newline)" : "Start an agent, or just type to start the default one"}
    ></textarea>
    <div class="row">
      {#if chat.stderrTail.length}
        <button type="button" class="link" onclick={() => (showStderr = !showStderr)}>agent log</button>
      {/if}
      <span class="spacer"></span>
      {#if chat.busy}
        <button type="button" class="danger" onclick={() => chat.cancel()}>Stop</button>
      {:else}
        <button type="submit" class="primary" disabled={!input.trim() || chat.connecting}>Send</button>
      {/if}
    </div>
    {#if showStderr}
      <pre class="stderr">{chat.stderrTail.join("\n")}</pre>
    {/if}
  </form>
</section>

<style>
  .chat { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  header {
    display: flex; gap: 0.4em; align-items: center; padding: 0.5em; border-bottom: 1px solid var(--border);
  }
  header select { flex: 1; min-width: 0; }
  .who { font-size: 0.8em; color: var(--fg-muted); white-space: nowrap; }
  .banner { margin: 0.5em; padding: 0.5em 0.6em; border-radius: 6px; background: var(--bg-raised); font-size: 0.85em; }
  .banner.error { color: var(--danger); }
  .log { flex: 1; overflow-y: auto; padding: 0.8em; display: flex; flex-direction: column; gap: 0.6em; }
  .empty { color: var(--fg-muted); text-align: center; margin-top: 30%; font-size: 0.9em; }
  .msg { line-height: 1.5; }
  .msg.user {
    align-self: flex-end; background: var(--bg-raised); padding: 0.45em 0.7em; border-radius: 10px 10px 2px 10px;
    max-width: 85%; white-space: pre-wrap;
  }
  .thought { font-size: 0.82em; color: var(--fg-muted); white-space: pre-wrap; }
  .plan { border-left: 2px solid var(--accent); padding-left: 0.6em; font-size: 0.88em; }
  .entry[data-status="completed"] { color: var(--fg-muted); text-decoration: line-through; }
  .notice { font-size: 0.82em; color: var(--fg-muted); text-align: center; }
  .notice.error { color: var(--danger); white-space: pre-wrap; text-align: left; }
  .working { font-size: 0.8em; color: var(--accent); animation: pulse 1.2s infinite; }
  @keyframes pulse { 50% { opacity: 0.4; } }
  form { border-top: 1px solid var(--border); padding: 0.5em; display: flex; flex-direction: column; gap: 0.4em; }
  textarea { resize: none; width: 100%; box-sizing: border-box; }
  .row { display: flex; gap: 0.4em; align-items: center; }
  .spacer { flex: 1; }
  .stderr { max-height: 160px; overflow: auto; font-size: 0.75em; font-family: var(--font-mono); margin: 0; }
</style>
