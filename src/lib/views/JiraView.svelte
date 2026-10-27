<script lang="ts">
  import { RefreshCw, Plus, Check } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { app, relTime } from "$lib/app.svelte";
  import JiraBadge from "$lib/components/JiraBadge.svelte";

  let tab = $state<string | null>(null);
  let syncing = $state(false);
  let error = $state<string | null>(null);

  const queries = $derived(app.jira ? Object.keys(app.jira.queries) : []);
  const active = $derived(tab && queries.includes(tab) ? tab : (queries.includes("mine") ? "mine" : queries[0]));
  const issues = $derived(app.jira && active ? app.jira.queries[active].map((k) => app.jira!.issues[k]).filter(Boolean) : []);
  const tracked = $derived(new Set(app.openTasks.flatMap((t) => t.jiraKeys)));

  async function sync() {
    syncing = true;
    error = null;
    try {
      app.jira = await api.jiraSync();
    } catch (e) {
      error = String(e);
    } finally {
      syncing = false;
    }
  }

  async function track(key: string, summary: string) {
    await api.taskAdd("Now", `${key} ${summary}`);
    await app.refreshTasks();
    app.notify(`Added ${key} to Now`);
  }

  const label = (q: string) => (q === "mine" ? "Assigned to me" : q.charAt(0).toUpperCase() + q.slice(1).replace(/[-_]/g, " "));
</script>

<div class="jira">
  <header>
    <h1>Jira</h1>
    {#if app.jira}<span class="when">Synced {relTime(app.jira.syncedAt)}</span>{/if}
    <button onclick={sync} disabled={syncing}><RefreshCw size={14} class={syncing ? "spin" : ""} /> {syncing ? "Syncing" : "Sync now"}</button>
  </header>

  {#if error}<p class="error">{error}</p>{/if}

  {#if !app.jira}
    <div class="empty">
      <p>Adjutant keeps a snapshot of your Jira tickets in <code>jira/snapshot.md</code>, refreshed every 15 minutes, so you and the agent can read it without spending credits on Jira calls.</p>
      <button class="primary" onclick={() => app.go({ kind: "settings" })}>Connect Jira</button>
    </div>
  {:else}
    <nav class="tabs" aria-label="Queries">
      {#each queries as q}
        <button class:on={q === active} onclick={() => (tab = q)}>{label(q)} <span class="n">{app.jira.queries[q].length}</span></button>
      {/each}
    </nav>
    <table>
      <thead>
        <tr><th>Ticket</th><th>Summary</th><th>Assignee</th><th>Updated</th><th></th></tr>
      </thead>
      <tbody>
        {#each issues as issue (issue.key)}
          <tr>
            <td><JiraBadge key={issue.key} /></td>
            <td class="summary">{issue.summary}</td>
            <td class="muted">{issue.assignee ?? "Unassigned"}</td>
            <td class="muted">{issue.updated ? relTime(issue.updated) : ""}</td>
            <td class="act">
              {#if tracked.has(issue.key)}
                <span class="tracked"><Check size={14} /> In tasks</span>
              {:else}
                <button class="quiet" onclick={() => track(issue.key, issue.summary)}><Plus size={14} /> Add task</button>
              {/if}
            </td>
          </tr>
        {:else}
          <tr><td colspan="5" class="muted">No tickets in this query.</td></tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .jira {
    height: 100%;
    overflow-y: auto;
    padding: 2.4rem clamp(1.5rem, 5vw, 4.5rem) 4rem;
  }
  header {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding-bottom: 1rem;
    border-bottom: 2px solid var(--ink);
  }
  h1 {
    margin: 0 auto 0 0;
    font-size: var(--step-3);
    font-weight: 800;
    letter-spacing: -0.03em;
  }
  .when {
    color: var(--muted);
    font-size: 0.85rem;
  }
  header button,
  .act button {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }
  :global(.spin) {
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .tabs {
    display: flex;
    gap: 1.4rem;
    margin: 1.2rem 0 0.4rem;
    border-bottom: 1px solid var(--rule);
  }
  .tabs button {
    border: none;
    border-radius: 0;
    background: none;
    padding: 0.4rem 0;
    color: var(--muted);
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }
  .tabs button.on {
    color: var(--ink);
    border-bottom-color: var(--verdigris);
  }
  .n {
    color: var(--faint);
    font-weight: 400;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
  }
  th {
    text-align: left;
    font-size: 0.78rem;
    font-weight: 700;
    color: var(--muted);
    padding: 0.6rem 0.6rem 0.4rem 0;
  }
  td {
    padding: 0.5rem 0.6rem 0.5rem 0;
    border-top: 1px solid var(--rule);
    vertical-align: baseline;
  }
  .summary {
    width: 55%;
  }
  .muted {
    color: var(--muted);
    white-space: nowrap;
  }
  .act {
    text-align: right;
    white-space: nowrap;
  }
  .tracked {
    display: inline-flex;
    gap: 0.3rem;
    align-items: center;
    color: var(--verdigris);
    font-size: 0.8rem;
    font-weight: 700;
  }
  .empty {
    max-width: 34rem;
    margin-top: 2rem;
    color: var(--ink-2);
  }
  .error {
    color: var(--signal);
    white-space: pre-wrap;
  }
</style>
