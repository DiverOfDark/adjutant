<script lang="ts">
  import { onMount, tick } from "svelte";
  import { ArrowUp, Square, Plus, GitBranch, X, History, Globe } from "@lucide/svelte";
  import { api, onAgentEvent, type Discovery, type SavedSession, type Visibility } from "$lib/api";
  import { app } from "$lib/app.svelte";
  import { chat } from "$lib/chat.svelte";
  import { ACTIONS, importPage, repoQuestion } from "$lib/actions";
  import Markdown from "./Markdown.svelte";
  import ToolCallCard from "./ToolCallCard.svelte";
  import PermissionCard from "./PermissionCard.svelte";

  let discovery = $state<Discovery | null>(null);
  let selected = $state<string | null>(null);
  let input = $state("");
  let url = $state("");
  let mode = $state<"chat" | "repo" | "import">("chat");
  let history = $state<SavedSession[] | null>(null);
  let visibility = $state<Visibility | null>(null);
  let scroller: HTMLDivElement | undefined = $state();
  let textarea: HTMLTextAreaElement | undefined = $state();
  let showLog = $state(false);

  const openNote = $derived(app.view.kind === "note" ? app.view.path : null);
  const selectedAgent = $derived(discovery?.agents.find((a) => a.id === selected));
  const pinnedAgent = $derived(
    visibility?.pinned && visibility.agent && visibility.agent !== selected ? discovery?.agents.find((a) => a.id === visibility!.agent) : null,
  );

  $effect(() => {
    const p = openNote;
    visibility = null;
    if (p) api.noteVisibility(p).then((v) => (visibility = v)).catch(() => {});
  });

  async function toggleHistory() {
    history = history ? null : await api.sessionsList();
  }

  async function openSaved(id: string) {
    const s = await api.sessionLoad(id);
    chat.showSaved(s.items as never);
    history = null;
  }

  function setMode(m: typeof mode) {
    mode = mode === m ? "chat" : m;
    url = "";
  }

  async function discover() {
    discovery = await api.agentsDiscover().catch(() => null);
    selected = chat.agentId ?? discovery?.defaultAgent ?? null;
  }

  async function connect() {
    await chat.connect(selected);
  }

  async function submit(e?: Event) {
    e?.preventDefault();
    let text = input.trim();
    if (mode === "import") {
      if (!url.trim()) return;
      const label = `Import ${url.trim()}${text ? `: ${text}` : ""}`;
      text = importPage(url.trim()) + (text ? `\n\nAlso: ${text}` : "");
      input = "";
      mode = "chat";
      return chat.ask(text, [], label);
    }
    if (!text) return;
    let label: string | undefined;
    if (mode === "repo" && url.trim()) {
      label = `${text}\n(${url.trim()})`;
      text = repoQuestion(url.trim(), text);
    }
    input = "";
    if (!chat.sessionId) await chat.connect(selected);
    await chat.send(text, openNote && text.includes("this note") ? [openNote] : [], label);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) submit(e);
  }

  function autosize() {
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = Math.min(textarea.scrollHeight, 220) + "px";
  }

  function runAction(id: string) {
    const a = ACTIONS.find((x) => x.id === id)!;
    chat.ask(a.prompt({ file: openNote ?? undefined }), a.needsFile && openNote ? [openNote] : [], a.label);
  }

  $effect(() => {
    chat.items.length;
    const last = chat.items.at(-1);
    if (last && "text" in last) last.text;
    tick().then(() => scroller?.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" }));
  });

  $effect(() => {
    app.vault?.path;
    discover();
  });

  onMount(() => {
    const un = onAgentEvent((e) => chat.handle(e));
    return () => un.then((f) => f());
  });
</script>

<section class="chat" aria-label="Agent">
  <header>
    <div class="who">
      <span class="led" class:live={!!chat.agentName} class:busy={chat.busy}></span>
      <select bind:value={selected} disabled={chat.connecting || chat.busy} aria-label="Agent">
        {#each discovery?.agents ?? [] as a}
          <option value={a.id} disabled={!a.installed}>{a.name}{a.installed ? "" : " (not installed)"}</option>
        {/each}
      </select>
    </div>
    <button class="icon" title="Past conversations" aria-label="Past conversations" aria-expanded={!!history} onclick={toggleHistory}><History size={16} /></button>
    {#if chat.sessionId && chat.agentId === selected}
      <button class="icon" title="New conversation" aria-label="New conversation" onclick={() => chat.newSession()} disabled={chat.busy}><Plus size={17} /></button>
    {:else}
      <button class="primary small" onclick={connect} disabled={chat.connecting || !selectedAgent?.installed}>{chat.connecting ? "Starting" : "Start"}</button>
    {/if}
    <button class="icon" title="Hide (Ctrl+J)" aria-label="Hide agent panel" onclick={() => (app.agentOpen = false)}><X size={17} /></button>
  </header>

  {#if history}
    <div class="history">
      {#each history as h (h.id)}
        <button onclick={() => openSaved(h.id)}><span>{h.title}</span><span class="when">{new Date(h.updatedAt).toLocaleDateString()}</span></button>
      {:else}
        <p>No saved conversations yet. They are kept in <code>.vault/sessions/</code>.</p>
      {/each}
    </div>
  {/if}

  {#if pinnedAgent}
    <div class="banner">
      This folder is pinned to {pinnedAgent.name}.
      <button class="small" onclick={() => { selected = pinnedAgent!.id; connect(); }}>Switch to {pinnedAgent.name}</button>
    </div>
  {/if}

  {#if discovery?.miseUntrusted}
    <div class="banner">
      mise needs your OK to use this vault's <code>mise.toml</code>.
      <button class="small" onclick={async () => (discovery = await api.agentsTrustVault())}>Trust this vault</button>
    </div>
  {:else if discovery?.miseError}
    <div class="banner bad">{discovery.miseError}</div>
  {:else if selectedAgent && !selectedAgent.installed}
    <div class="banner">To install {selectedAgent.name}, add <code>{selectedAgent.miseLine}</code> to the vault's <code>mise.toml</code>.</div>
  {/if}

  <div class="log" bind:this={scroller}>
    {#if chat.items.length === 0}
      <div class="empty">
        <p class="hello">What do you need?</p>
        <p>The agent works inside this vault. It reads your notes, follows <code>AGENTS.md</code>, and asks before it changes anything.</p>
      </div>
    {/if}
    {#each chat.items as item (item.id)}
      {#if item.kind === "user"}
        <div class="user">{item.text}</div>
      {:else if item.kind === "agent"}
        <div class="agent"><Markdown text={item.text} /></div>
      {:else if item.kind === "thought"}
        <details class="thought"><summary>Thinking</summary><p>{item.text}</p></details>
      {:else if item.kind === "tool"}
        <ToolCallCard call={item.call} />
      {:else if item.kind === "plan"}
        <ol class="plan">
          {#each item.entries as e}
            <li data-status={e.status}>{e.content}</li>
          {/each}
        </ol>
      {:else if item.kind === "permission"}
        <PermissionCard requestId={item.requestId} toolCall={item.toolCall} options={item.options} decided={item.decided} />
      {:else if item.kind === "auth"}
        <div class="auth">
          <p><strong>{item.message}</strong></p>
          <p class="sub">Most agents log in once with your account (Junie uses your JetBrains login). Then press Retry.</p>
          <div class="authbtns">
            {#each item.methods as m}
              <button class="primary small" title={m.description ?? ""} onclick={() => chat.login(m)}>{m.name}</button>
            {/each}
            <button class="small" onclick={() => chat.login(null)}>Log in from a terminal</button>
            <button class="small quiet" onclick={() => chat.retry()}>Retry</button>
          </div>
        </div>
      {:else if item.kind === "notice"}
        <div class="notice {item.level}">{item.text}</div>
      {/if}
    {/each}
    {#if chat.busy}<div class="working" aria-live="polite"><span></span><span></span><span></span></div>{/if}
  </div>

  <div class="compose">
    <div class="chips">
      {#each ACTIONS.filter((a) => !a.needsPerson && (!a.needsFile || (openNote && /\.(md|markdown)$/i.test(openNote)))) as a}
        <button class="chip" onclick={() => runAction(a.id)} disabled={chat.busy || chat.connecting}>{a.label}</button>
      {/each}
      <button class="chip" class:on={mode === "repo"} onclick={() => setMode("repo")}><GitBranch size={13} /> Ask about a repo</button>
      <button class="chip" class:on={mode === "import"} onclick={() => setMode("import")}><Globe size={13} /> Import a page</button>
    </div>
    {#if mode !== "chat"}
      <input
        class="repo"
        bind:value={url}
        placeholder={mode === "repo" ? "Repository URL, e.g. git@github.com:org/service.git" : "Page URL (Confluence, docs, anything readable)"}
        aria-label={mode === "repo" ? "Repository URL" : "Page URL"}
      />
    {/if}
    <form onsubmit={submit}>
      <textarea
        bind:this={textarea}
        bind:value={input}
        oninput={autosize}
        onkeydown={onKey}
        rows="1"
        placeholder={mode === "repo" ? "Where is index rebalancing triggered?" : mode === "import" ? "Optional: what to focus on, or import the whole page tree" : "Ask, or tell it what to file"}
        aria-label="Message"
      ></textarea>
      {#if chat.busy}
        <button type="button" class="send stop" aria-label="Stop" title="Stop" onclick={() => chat.cancel()}><Square size={14} /></button>
      {:else}
        <button type="submit" class="send" aria-label="Send" title="Send (Enter)" disabled={(mode === "import" ? !url.trim() : !input.trim()) || chat.connecting}><ArrowUp size={17} /></button>
      {/if}
    </form>
    {#if chat.stderrTail.length}
      <button class="logbtn" onclick={() => (showLog = !showLog)}>{showLog ? "Hide" : "Show"} agent output</button>
      {#if showLog}<pre class="stderr">{chat.stderrTail.join("\n")}</pre>{/if}
    {/if}
  </div>
</section>

<style>
  .chat {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--paper);
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.55rem 0.6rem 0.55rem 0.9rem;
    border-bottom: 1px solid var(--rule);
  }
  .who {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }
  .who select {
    border: none;
    background: transparent;
    font-weight: 700;
    padding: 0.2rem;
    min-width: 0;
    max-width: 100%;
  }
  .led {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--faint);
  }
  .led.live {
    background: var(--verdigris);
  }
  .led.busy {
    animation: pulse 1s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      box-shadow: 0 0 0 4px var(--verdigris-wash);
    }
  }
  .small {
    padding: 0.25rem 0.7rem;
  }
  .banner {
    margin: 0.6rem 0.8rem 0;
    padding: 0.55rem 0.7rem;
    border-radius: var(--radius-m);
    background: var(--brass-wash);
    font-size: 0.82rem;
    line-height: 1.45;
  }
  .history {
    margin: 0.5rem 0.8rem 0;
    max-height: 40%;
    overflow-y: auto;
    border-radius: var(--radius-m);
    background: var(--sheet);
    box-shadow: var(--shadow-pop);
    padding: 0.3rem;
  }
  .history button {
    all: unset;
    box-sizing: border-box;
    cursor: pointer;
    display: flex;
    justify-content: space-between;
    gap: 0.8rem;
    width: 100%;
    padding: 0.4rem 0.6rem;
    border-radius: var(--radius-s);
    font-size: 0.85rem;
  }
  .history button:hover {
    background: var(--sunken);
  }
  .history span:first-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .history .when {
    flex: none;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .history p {
    margin: 0.4rem 0.6rem;
    font-size: 0.82rem;
    color: var(--muted);
  }
  .banner.bad {
    background: var(--signal-wash);
  }
  .banner button {
    margin-top: 0.4rem;
  }
  .log {
    flex: 1;
    overflow-y: auto;
    padding: 1rem 0.9rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    font-size: 0.92rem;
  }
  .empty {
    margin: auto 0.5rem;
    color: var(--muted);
    font-size: 0.875rem;
  }
  .hello {
    font-size: var(--step-2);
    font-weight: 800;
    letter-spacing: -0.02em;
    color: var(--ink);
    margin: 0 0 0.4rem;
  }
  .user {
    align-self: flex-end;
    max-width: 88%;
    padding: 0.5rem 0.8rem;
    border-radius: 14px 14px 4px 14px;
    background: var(--ink);
    color: var(--paper);
    white-space: pre-wrap;
    line-height: 1.45;
  }
  .agent {
    line-height: 1.6;
  }
  .thought {
    font-size: 0.8rem;
    color: var(--muted);
  }
  .thought summary {
    cursor: pointer;
  }
  .thought p {
    white-space: pre-wrap;
    margin: 0.3rem 0 0;
  }
  .plan {
    margin: 0;
    padding: 0.5rem 0.7rem 0.5rem 2rem;
    border-radius: var(--radius-m);
    background: var(--sunken);
    font-size: 0.85rem;
  }
  .plan li[data-status="completed"] {
    color: var(--muted);
    text-decoration: line-through;
  }
  .plan li[data-status="in_progress"] {
    font-weight: 700;
  }
  .auth {
    padding: 0.7rem 0.8rem;
    border-radius: var(--radius-m);
    background: var(--brass-wash);
    font-size: 0.85rem;
    white-space: pre-wrap;
  }
  .auth p {
    margin: 0 0 0.4rem;
  }
  .auth .sub {
    color: var(--ink-2);
  }
  .authbtns {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }
  .notice {
    font-size: 0.8rem;
    color: var(--muted);
    text-align: center;
  }
  .notice.error {
    color: var(--signal);
    text-align: left;
    white-space: pre-wrap;
    background: var(--signal-wash);
    padding: 0.5rem 0.7rem;
    border-radius: var(--radius-m);
  }
  .working {
    display: flex;
    gap: 4px;
  }
  .working span {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--verdigris);
    animation: bounce 1.2s infinite ease-in-out;
  }
  .working span:nth-child(2) {
    animation-delay: 0.15s;
  }
  .working span:nth-child(3) {
    animation-delay: 0.3s;
  }
  @keyframes bounce {
    0%,
    80%,
    100% {
      opacity: 0.25;
    }
    40% {
      opacity: 1;
    }
  }
  .compose {
    padding: 0.6rem 0.8rem 0.8rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.22rem 0.6rem;
    border-radius: 99px;
    font-size: 0.78rem;
    font-weight: 600;
    background: transparent;
    color: var(--ink-2);
  }
  .chip:hover:not(:disabled) {
    border-color: var(--verdigris);
    color: var(--verdigris);
  }
  .chip.on {
    background: var(--verdigris-wash);
    border-color: var(--verdigris);
    color: var(--verdigris);
  }
  .repo {
    font-size: 0.85rem;
  }
  form {
    display: flex;
    align-items: flex-end;
    gap: 0.4rem;
    padding: 0.35rem 0.35rem 0.35rem 0.75rem;
    border: 1px solid var(--rule-strong);
    border-radius: 12px;
    background: var(--sheet);
  }
  form:focus-within {
    border-color: var(--verdigris);
    box-shadow: 0 0 0 3px var(--verdigris-wash);
  }
  textarea {
    flex: 1;
    border: none;
    background: none;
    resize: none;
    padding: 0.35rem 0;
    max-height: 220px;
    line-height: 1.45;
  }
  textarea:focus {
    box-shadow: none;
  }
  .send {
    width: 32px;
    height: 32px;
    padding: 0;
    display: grid;
    place-items: center;
    border-radius: 9px;
    border: none;
    background: var(--verdigris);
    color: var(--verdigris-ink);
  }
  .send:disabled {
    background: var(--rule-strong);
    color: var(--sheet);
    opacity: 1;
  }
  .send.stop {
    background: var(--ink);
    color: var(--paper);
  }
  .logbtn {
    all: unset;
    cursor: pointer;
    font-size: 0.75rem;
    color: var(--faint);
  }
  .logbtn:hover {
    color: var(--ink);
  }
  .stderr {
    margin: 0;
    max-height: 140px;
    overflow: auto;
    font-size: 0.7rem;
    font-family: var(--font-mono);
    color: var(--muted);
  }
</style>
