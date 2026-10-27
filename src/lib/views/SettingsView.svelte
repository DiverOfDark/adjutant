<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AppSettings, type Discovery, type JiraSettings } from "$lib/api";
  import { app } from "$lib/app.svelte";
  import { updater } from "$lib/updater.svelte";

  let jira = $state<JiraSettings>({ baseUrl: "", email: "", queries: {}, hasToken: false });
  let token = $state("");
  let queries = $state<{ name: string; jql: string }[]>([]);
  let jiraStatus = $state<{ ok: boolean; text: string } | null>(null);
  let busy = $state(false);
  let settings = $state<AppSettings | null>(null);
  let discovery = $state<Discovery | null>(null);

  onMount(async () => {
    const [j, s, d] = await Promise.all([api.jiraSettings().catch(() => null), api.appSettings(), api.agentsDiscover().catch(() => null)]);
    if (j) {
      jira = j;
      queries = Object.entries(j.queries).map(([name, jql]) => ({ name, jql }));
    }
    if (!queries.length) queries = [{ name: "mine", jql: "assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC" }];
    settings = s;
    discovery = d;
  });

  async function saveJira(test: boolean) {
    busy = true;
    jiraStatus = null;
    try {
      await api.jiraSaveSettings(
        { baseUrl: jira.baseUrl.trim(), email: jira.email.trim(), queries: Object.fromEntries(queries.filter((q) => q.name && q.jql).map((q) => [q.name, q.jql])) },
        token || null,
      );
      if (token) jira.hasToken = true;
      token = "";
      if (test) {
        const who = await api.jiraTest();
        jiraStatus = { ok: true, text: `Connected as ${who}. Syncing now…` };
        app.jira = await api.jiraSync();
        jiraStatus = { ok: true, text: `Connected as ${who}. Synced ${Object.keys(app.jira.issues).length} tickets.` };
      } else {
        jiraStatus = { ok: true, text: "Saved." };
      }
    } catch (e) {
      jiraStatus = { ok: false, text: String(e) };
    } finally {
      busy = false;
    }
  }

  async function saveApp() {
    if (settings) await api.appSaveSettings($state.snapshot(settings));
    app.notify("Saved");
  }
</script>

<div class="settings">
  <h1>Settings</h1>

  <section>
    <div class="intro">
      <h2>Jira</h2>
      <p>Jira Cloud. The API token is stored in your system keyring, never in the vault. Create one at <a href="https://id.atlassian.com/manage-profile/security/api-tokens" target="_blank" rel="noreferrer">id.atlassian.com</a>.</p>
    </div>
    <div class="form">
      <label>Site URL <input bind:value={jira.baseUrl} placeholder="https://your-company.atlassian.net" /></label>
      <label>Email <input bind:value={jira.email} type="email" placeholder="you@company.com" /></label>
      <label>API token
        <input bind:value={token} type="password" placeholder={jira.hasToken ? "Stored in keyring. Type to replace." : "Paste your API token"} autocomplete="off" />
      </label>
      <fieldset>
        <legend>Queries</legend>
        {#each queries as q, i}
          <div class="q">
            <input class="qname" bind:value={q.name} placeholder="name" aria-label="Query name" />
            <input class="qjql" bind:value={q.jql} placeholder="JQL" aria-label="JQL" />
            <button class="icon" aria-label="Remove query" onclick={() => queries.splice(i, 1)}>×</button>
          </div>
        {/each}
        <button class="quiet" onclick={() => queries.push({ name: "", jql: "" })}>Add query</button>
        <p class="hint">The query named <code>mine</code> fills “My Jira” on Today. Add one per team board, for example <code>project = PLAT AND resolution = Unresolved</code>.</p>
      </fieldset>
      <div class="btns">
        <button class="primary" onclick={() => saveJira(true)} disabled={busy || !jira.baseUrl || !jira.email || (!token && !jira.hasToken)}>Save and test</button>
        <button onclick={() => saveJira(false)} disabled={busy}>Save</button>
      </div>
      {#if jiraStatus}<p class="status" class:bad={!jiraStatus.ok}>{jiraStatus.text}</p>{/if}
    </div>
  </section>

  {#if settings}
    <section>
      <div class="intro">
        <h2>App</h2>
        <p>Scheduled jobs only run while Adjutant is open, so keep it in the tray and start it on login.</p>
      </div>
      <div class="form">
        <div class="version">
          <span>Adjutant {updater.version ?? "dev"}</span>
          {#if updater.available?.ready}
            <span class="hint">{updater.available.version} is downloaded and installs when you quit from the tray.</span>
            <button class="primary" onclick={() => updater.installNow()}>Restart to update</button>
          {:else if updater.available}
            <span class="hint">Downloading {updater.available.version}…</span>
          {:else}
            <span class="hint">{updater.note || "Updates come from GitHub releases."}</span>
            <button onclick={() => updater.check()}>Check now</button>
          {/if}
        </div>
        <label class="check"><input type="checkbox" bind:checked={settings.autostart} /> Start Adjutant when I log in</label>
        <label class="check"><input type="checkbox" bind:checked={settings.closeToTray} /> Closing the window keeps Adjutant running in the tray</label>
        <label class="check"><input type="checkbox" bind:checked={settings.checkUpdates} /> Check GitHub for updates</label>
        <label>Quick capture shortcut <input bind:value={settings.captureHotkey} placeholder="CommandOrControl+Shift+Space" /></label>
        {#if settings.hotkeyError}<p class="status bad">{settings.hotkeyError}</p>{/if}
        <div class="btns"><button class="primary" onclick={saveApp}>Save</button></div>
      </div>
    </section>
  {/if}

  <section>
    <div class="intro">
      <h2>Agents</h2>
      <p>Agents come from the vault's <code>mise.toml</code>. Add a line there to install one; per-agent settings live in <code>.vault/config.toml</code>.</p>
    </div>
    <div class="form">
      {#if discovery?.miseError}<p class="status bad">{discovery.miseError}</p>{/if}
      <ul class="agents">
        {#each discovery?.agents ?? [] as a}
          <li class:missing={!a.installed}>
            <strong>{a.name}</strong>
            {#if a.installed}
              <span class="ok">{a.version ? `Installed ${a.version}` : "Configured"}</span>
              {#if discovery?.defaultAgent === a.id}<span class="def">Default</span>{/if}
            {:else}
              <code>{a.miseLine}</code>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  </section>

  <section>
    <div class="intro">
      <h2>Vault</h2>
      <p>{app.vault?.path}</p>
    </div>
    <div class="form">
      <button onclick={() => app.go({ kind: "setup" })}>Run setup again</button>
      <button onclick={async () => { const { open } = await import("@tauri-apps/plugin-dialog"); const d = await open({ directory: true }); if (typeof d === "string") await app.openVault(d); }}>Open another vault</button>
    </div>
  </section>
</div>

<style>
  .settings {
    height: 100%;
    overflow-y: auto;
    padding: 2.4rem clamp(1.5rem, 5vw, 4.5rem) 4rem;
  }
  h1 {
    margin: 0;
    padding-bottom: 1rem;
    border-bottom: 2px solid var(--ink);
    font-size: var(--step-3);
    font-weight: 800;
    letter-spacing: -0.03em;
  }
  section {
    display: grid;
    grid-template-columns: minmax(0, 16rem) minmax(0, 32rem);
    gap: 2.5rem;
    padding: 2rem 0;
    border-bottom: 1px solid var(--rule);
  }
  @media (max-width: 900px) {
    section {
      grid-template-columns: 1fr;
      gap: 1rem;
    }
  }
  h2 {
    margin: 0 0 0.3rem;
    font-size: var(--step-1);
  }
  .intro p {
    margin: 0;
    color: var(--muted);
    font-size: 0.875rem;
    overflow-wrap: anywhere;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 0.9rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.8rem;
    font-weight: 700;
    color: var(--ink-2);
  }
  label input {
    font-weight: 400;
    color: var(--ink);
  }
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 0.5rem;
    font-weight: 400;
    font-size: 0.9rem;
  }
  label.check input {
    accent-color: var(--verdigris);
    width: 16px;
    height: 16px;
  }
  fieldset {
    border: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  legend {
    font-size: 0.8rem;
    font-weight: 700;
    color: var(--ink-2);
    margin-bottom: 0.25rem;
  }
  .q {
    display: flex;
    gap: 0.4rem;
  }
  .qname {
    width: 7rem;
  }
  .qjql {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 0.8rem;
  }
  fieldset > button {
    align-self: flex-start;
  }
  .hint {
    margin: 0;
    font-size: 0.8rem;
    color: var(--muted);
  }
  .version {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.8rem;
    font-weight: 700;
  }
  .version .hint {
    flex-basis: 100%;
  }
  .btns {
    display: flex;
    gap: 0.4rem;
  }
  .status {
    margin: 0;
    color: var(--verdigris);
    font-size: 0.875rem;
    white-space: pre-wrap;
  }
  .status.bad {
    color: var(--signal);
  }
  .agents {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .agents li {
    display: flex;
    align-items: baseline;
    gap: 0.7rem;
    padding: 0.45rem 0;
    border-bottom: 1px solid var(--rule);
    font-size: 0.9rem;
  }
  .agents li.missing strong {
    color: var(--muted);
  }
  .agents code {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .ok {
    color: var(--verdigris);
    font-size: 0.8rem;
  }
  .def {
    font-size: 0.72rem;
    font-weight: 700;
    padding: 0 0.4rem;
    border-radius: 99px;
    background: var(--ink);
    color: var(--paper);
  }
</style>
