<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Check, Copy } from "@lucide/svelte";
  import { api, onHotkeyChanged, onSetupLog, type SetupStatus } from "$lib/api";
  import ShortcutInfo from "$lib/components/ShortcutInfo.svelte";
  import { app } from "$lib/app.svelte";
  import { chat } from "$lib/chat.svelte";

  let s = $state<SetupStatus | null>(null);
  let busy = $state<string | null>(null);
  let log = $state<string[]>([]);
  let logBox: HTMLPreElement | undefined = $state();
  let agentId = $state<string | null>(null);
  let agentResult = $state<{ ok: boolean; text: string } | null>(null);
  let message = $state<string | null>(null);

  const SENSITIVE = ["people", "me"];

  async function refresh() {
    s = await api.setupStatus();
    agentId ??= s.discovery.defaultAgent;
  }

  onMount(() => {
    refresh();
    const un = onSetupLog(async (line) => {
      log = [...log.slice(-300), line];
      await tick();
      logBox?.scrollTo({ top: logBox.scrollHeight });
    });
    const unHotkey = onHotkeyChanged(refresh);
    return () => {
      un.then((f) => f());
      unHotkey.then((f) => f());
    };
  });

  async function step(name: string, fn: () => Promise<unknown>) {
    busy = name;
    message = null;
    try {
      await fn();
    } catch (e) {
      message = String(e);
    } finally {
      busy = null;
      await refresh();
    }
  }

  const miseInstall = $derived(
    s?.os === "windows" ? "winget install jdx.mise" : s?.os === "macos" ? "brew install mise" : "curl https://mise.run | sh",
  );
  const miseOk = $derived(!!s?.discovery.miseBin && !s.discovery.miseError);
  const toolsOk = $derived(miseOk && !s?.discovery.miseUntrusted && (s?.missingTools.length ?? 1) === 0);
  const installed = $derived(s?.discovery.agents.filter((a) => a.installed) ?? []);
  const hidden = $derived(SENSITIVE.every((f) => s?.hiddenFolders.includes(f)));

  async function copy(text: string) {
    await navigator.clipboard.writeText(text).catch(() => {});
    app.notify("Copied");
  }

  async function testAgent() {
    agentResult = null;
    await chat.connect(agentId);
    if (chat.sessionId) agentResult = { ok: true, text: `${chat.agentName} is ready. Nothing was sent, so no credits were used.` };
    else {
      app.agentOpen = true;
      agentResult = { ok: false, text: "The agent didn't open a session. The agent panel shows why and how to log in." };
    }
  }

  async function setAutostart(on: boolean) {
    const cur = await api.appSettings();
    await api.appSaveSettings({ ...cur, autostart: on });
    await refresh();
  }

  async function setUpdates(on: boolean) {
    const cur = await api.appSettings();
    await api.appSaveSettings({ ...cur, checkUpdates: on });
    await refresh();
  }

  async function finish() {
    await api.setupComplete();
    await app.refresh();
    app.go({ kind: "today" });
  }
</script>

<div class="setup">
  <header>
    <h1>Set up Adjutant</h1>
    <p class="lede">A few checks for this laptop and this vault. Everything stays in <code>{app.vault?.path}</code>; nothing here spends agent credits.</p>
  </header>

  {#if message}<p class="error">{message}</p>{/if}

  {#if s}
    <ol class="steps">
      <li class:done={s.initialized}>
        <div class="head"><span class="mark">{#if s.initialized}<Check size={15} />{:else}1{/if}</span><h2>Vault folders and files</h2></div>
        {#if s.initialized}
          <p>This folder has the task list, agent guidelines and config.</p>
        {:else}
          <p>Adds <code>tasks.md</code>, <code>AGENTS.md</code> (rules the agent follows), <code>mise.toml</code> (which agent and tools to use), folders like <code>people/</code> and <code>inbox/</code>, and templates. Existing files are left alone.</p>
          <button class="primary" disabled={!!busy} onclick={() => step("scaffold", () => api.vaultScaffold())}>Set up this folder</button>
        {/if}
      </li>

      <li class:done={miseOk}>
        <div class="head"><span class="mark">{#if miseOk}<Check size={15} />{:else}2{/if}</span><h2>mise</h2></div>
        {#if miseOk}
          <p>{s.miseVersion ?? "Installed"} at <code>{s.discovery.miseBin}</code>.</p>
        {:else}
          <p>Adjutant uses <a href="https://mise.jdx.dev" target="_blank" rel="noreferrer">mise</a> to install the agent and the command-line tools it may use, pinned per vault. Install it in a terminal, then check again:</p>
          <div class="cmd"><code>{miseInstall}</code><button class="icon" aria-label="Copy" onclick={() => copy(miseInstall)}><Copy size={14} /></button></div>
          <button disabled={!!busy} onclick={() => step("recheck", refresh)}>Check again</button>
        {/if}
      </li>

      <li class:done={toolsOk} class:blocked={!miseOk}>
        <div class="head"><span class="mark">{#if toolsOk}<Check size={15} />{:else}3{/if}</span><h2>Agent and tools</h2></div>
        {#if !miseOk}
          <p>Needs mise.</p>
        {:else if s.discovery.miseUntrusted}
          <p>mise asks before it uses a <code>mise.toml</code> it hasn't seen. This one is the vault's own.</p>
          <button class="primary" disabled={!!busy} onclick={() => step("trust", () => api.agentsTrustVault())}>Trust this vault's mise.toml</button>
        {:else if s.missingTools.length}
          <p>Not installed yet: {s.missingTools.join(", ")}. The first install downloads Node and the agent, which takes a few minutes.</p>
          <button class="primary" disabled={!!busy} onclick={() => { log = []; step("install", () => api.setupInstallTools()); }}>
            {busy === "install" ? "Installing" : `Install ${s.missingTools.length} ${s.missingTools.length === 1 ? "tool" : "tools"}`}
          </button>
        {:else}
          <p>Everything in <code>mise.toml</code> is installed.</p>
        {/if}
        {#if log.length}<pre class="log" bind:this={logBox}>{log.join("\n")}</pre>{/if}
      </li>

      <li class:done={agentResult?.ok} class:blocked={!toolsOk}>
        <div class="head"><span class="mark">{#if agentResult?.ok}<Check size={15} />{:else}4{/if}</span><h2>Agent login</h2></div>
        {#if installed.length === 0}
          <p>No ACP agent is installed. The vault's <code>mise.toml</code> lists them; Junie is <code>"npm:@jetbrains/junie-cli" = "latest"</code>.</p>
        {:else}
          <p>Starts the agent and opens a session without sending a prompt. Junie uses your JetBrains AI login; if it asks you to log in, a terminal opens for it.</p>
          <div class="row">
            <select bind:value={agentId} aria-label="Agent">
              {#each installed as a}<option value={a.id}>{a.name}{a.version ? ` ${a.version}` : ""}</option>{/each}
            </select>
            <button class="primary" disabled={!!busy || chat.connecting} onclick={testAgent}>{chat.connecting ? "Starting" : "Test the agent"}</button>
            <button disabled={!!busy} onclick={() => step("login", async () => { const c = await api.agentLoginTerminal(agentId, null); message = null; agentResult = { ok: false, text: `Opened a terminal running ${c}. Log in there, then test again.` }; })}>Log in from a terminal</button>
          </div>
          {#if agentResult}<p class="result" class:bad={!agentResult.ok}>{agentResult.text}</p>{/if}
        {/if}
      </li>

      <li class="done-ish">
        <div class="head"><span class="mark">5</span><h2>Privacy</h2></div>
        <label class="check">
          <input type="checkbox" checked={hidden} onchange={(e) => step("hide", () => api.setupHideFolders(SENSITIVE, e.currentTarget.checked))} />
          <span>Hide <code>people/</code> and <code>me/</code> from agents. Reviews and promotion notes stay out of cloud models; you can pin a local-model agent to them later.</span>
        </label>
        <label class="check">
          <input type="checkbox" checked={s.checkUpdates} onchange={(e) => setUpdates(e.currentTarget.checked)} />
          <span>Check GitHub for Adjutant updates. This is the only request the app itself makes.</span>
        </label>
        {#if s.gitAvailable && !s.gitRepo}
          <div class="row">
            <span class="grow">Track the vault with git, so every change the agent makes has history.</span>
            <button disabled={!!busy} onclick={() => step("git", async () => app.notify(await api.setupGitInit()))}>Start tracking</button>
          </div>
        {:else if s.gitRepo}
          <p class="ok">The vault is tracked with git.</p>
        {/if}
      </li>

      <li class="done-ish">
        <div class="head"><span class="mark">6</span><h2>In the background</h2></div>
        <label class="check">
          <input type="checkbox" checked={s.autostart} onchange={(e) => setAutostart(e.currentTarget.checked)} />
          <span>Start Adjutant when I log in. Schedules run only while it's open; closing the window keeps it in the tray.</span>
        </label>
        <p>Agent jobs spend credits, so they start off:</p>
        {#each s.agentSchedules as j (j.id)}
          <label class="check">
            <input type="checkbox" checked={j.enabled} onchange={(e) => step("sched", () => api.scheduleToggle(j.id, e.currentTarget.checked))} />
            <span>{j.name}</span>
          </label>
        {/each}
        <div class="capture">
          <strong>Quick capture</strong>
          <ShortcutInfo label={s.hotkeyLabel} managed={s.hotkeyManaged} error={s.hotkeyError} captureCommand={s.captureCommand} />
        </div>
      </li>

      <li class:done={s.jiraConfigured}>
        <div class="head"><span class="mark">{#if s.jiraConfigured}<Check size={15} />{:else}7{/if}</span><h2>Jira (optional)</h2></div>
        {#if s.jiraConfigured}
          <p>Connected. Tickets sync every 15 minutes into <code>jira/snapshot.md</code>.</p>
        {:else}
          <p>Jira Cloud with an API token from your Atlassian account. Check that a personal token is allowed for local tools at your company.</p>
          <button onclick={() => app.go({ kind: "settings" })}>Connect Jira</button>
        {/if}
      </li>
    </ol>

    <footer>
      <button class="primary big" onclick={finish} disabled={!s.initialized}>Done, show my day</button>
      <span class="hint">You can run this again from Settings.</span>
    </footer>
  {/if}
</div>

<style>
  .setup {
    height: 100%;
    overflow-y: auto;
    padding: 2.4rem clamp(1.5rem, 5vw, 4.5rem) 4rem;
  }
  header {
    padding-bottom: 1rem;
    border-bottom: 2px solid var(--ink);
  }
  h1 {
    margin: 0;
    font-size: var(--step-3);
    font-weight: 800;
    letter-spacing: -0.03em;
  }
  .lede {
    margin: 0.4rem 0 0;
    color: var(--muted);
    max-width: 44rem;
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
    max-width: 46rem;
  }
  .steps > li {
    padding: 1.2rem 0 1.3rem 2.6rem;
    border-bottom: 1px solid var(--rule);
    position: relative;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    margin-left: -2.6rem;
    margin-bottom: 0.35rem;
  }
  .mark {
    width: 28px;
    height: 28px;
    flex: none;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-weight: 800;
    font-size: 0.85rem;
    border: 1.6px solid var(--rule-strong);
    color: var(--muted);
  }
  li.done .mark {
    background: var(--verdigris);
    border-color: var(--verdigris);
    color: var(--verdigris-ink);
  }
  li.blocked {
    opacity: 0.55;
  }
  h2 {
    margin: 0;
    font-size: var(--step-1);
  }
  p {
    margin: 0 0 0.6rem;
    color: var(--ink-2);
    max-width: 42rem;
  }
  .cmd {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.35rem 0.4rem 0.35rem 0.75rem;
    margin-bottom: 0.6rem;
    border-radius: var(--radius-m);
    background: var(--sunken);
    max-width: 100%;
  }
  .cmd code {
    overflow-x: auto;
    white-space: nowrap;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }
  .grow {
    flex: 1;
    color: var(--ink-2);
  }
  .check {
    display: flex;
    gap: 0.6rem;
    align-items: flex-start;
    margin: 0 0 0.6rem;
    color: var(--ink-2);
    cursor: pointer;
  }
  .check input {
    margin-top: 0.25rem;
    accent-color: var(--verdigris);
    width: 16px;
    height: 16px;
    flex: none;
  }
  .log {
    margin: 0.6rem 0 0;
    max-height: 12rem;
    overflow: auto;
    padding: 0.6rem 0.8rem;
    border-radius: var(--radius-m);
    background: var(--sunken);
    font-size: 0.75rem;
    white-space: pre-wrap;
  }
  .result {
    margin-top: 0.6rem;
    color: var(--verdigris);
    font-weight: 700;
  }
  .result.bad {
    color: var(--brass);
  }
  .ok {
    color: var(--verdigris);
  }
  .capture {
    margin-top: 0.8rem;
  }
  .capture :global(p) {
    margin-top: 0.2rem;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-top: 1.8rem;
  }
  .big {
    font-size: var(--step-0);
    padding: 0.6rem 1.2rem;
  }
  .hint {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .error {
    color: var(--signal);
    white-space: pre-wrap;
  }
</style>
