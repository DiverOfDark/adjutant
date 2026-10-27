<script lang="ts">
  import { onMount } from "svelte";
  import { Play, Bot, RefreshCw, Archive } from "@lucide/svelte";
  import { api, onScheduleRun, type ScheduleView } from "$lib/api";
  import { app, relTime } from "$lib/app.svelte";

  let schedules = $state<ScheduleView[]>([]);
  let open = $state<string | null>(null);
  let draft = $state<{ cron: string; prompt: string; output: string }>({ cron: "", prompt: "", output: "" });
  let error = $state<string | null>(null);

  async function load() {
    try {
      schedules = await api.schedulesList();
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    load();
    const un = onScheduleRun(() => load());
    const t = setInterval(load, 30000);
    return () => {
      clearInterval(t);
      un.then((f) => f());
    };
  });

  function edit(s: ScheduleView) {
    if (open === s.id) return (open = null);
    open = s.id;
    draft = { cron: s.cron, prompt: s.prompt ?? "", output: s.output ?? "" };
  }

  async function save(s: ScheduleView) {
    try {
      await api.scheduleUpdate(s.id, draft.cron !== s.cron ? draft.cron : null, s.kind === "agent" ? draft.prompt : null, s.kind === "agent" ? draft.output : null);
      open = null;
      await load();
      app.notify(`Saved ${s.name}`);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function toggle(s: ScheduleView) {
    await api.scheduleToggle(s.id, !s.enabled);
    await load();
  }

  async function runNow(s: ScheduleView) {
    await api.scheduleRun(s.id);
    app.notify(`Started ${s.name}`);
    setTimeout(load, 500);
  }

  const icon = (k: ScheduleView["kind"]) => (k === "agent" ? Bot : k === "jira-sync" ? RefreshCw : Archive);
  const describe = (cron: string) => {
    const f = cron.trim().split(/\s+/);
    const [sec, min, hour, , , dow] = f.length === 6 ? f : ["0", ...f];
    void sec;
    if (min.startsWith("*/") && hour === "*") return `Every ${min.slice(2)} minutes`;
    if (/^\d+$/.test(min) && /^\d+$/.test(hour)) {
      const time = `${hour.padStart(2, "0")}:${min.padStart(2, "0")}`;
      const days: Record<string, string> = { "*": "Every day", "MON-FRI": "Weekdays", "1-5": "Weekdays", FRI: "Fridays", MON: "Mondays", "5": "Fridays", "1": "Mondays" };
      return `${days[dow?.toUpperCase()] ?? dow} at ${time}`;
    }
    return cron;
  };
</script>

<div class="sched">
  <header>
    <h1>Schedules</h1>
    <p class="lede">Jobs run while Adjutant is open, including from the tray. Missed runs catch up once when the laptop wakes. Agent jobs use your normal agent login and credits.</p>
  </header>
  {#if error}<p class="error">{error}</p>{/if}

  <ul class="list">
    {#each schedules as s (s.id)}
      {@const Icon = icon(s.kind)}
      <li class:off={!s.enabled}>
        <div class="row">
          <span class="ic" data-kind={s.kind}><Icon size={16} /></span>
          <button class="name" onclick={() => edit(s)} aria-expanded={open === s.id}>
            <strong>{s.name}</strong>
            <span class="when">{s.error ? s.error : describe(s.cron)}{s.output ? `, writes ${s.output}` : ""}</span>
          </button>
          <span class="last">
            {#if s.lastRun}
              <span class="st" data-status={s.lastRun.status}>{s.lastRun.status === "ok" ? "Ran" : s.lastRun.status === "failed" ? "Failed" : s.lastRun.status === "running" ? "Running" : "Skipped"}</span>
              {relTime(s.lastRun.startedAt)}
            {:else}Never ran{/if}
            {#if s.enabled && s.nextRun}<span class="next">Next {relTime(s.nextRun)}</span>{/if}
          </span>
          <button class="icon" title="Run now" aria-label="Run {s.name} now" onclick={() => runNow(s)} disabled={s.lastRun?.status === "running"}><Play size={15} /></button>
          <button class="switch" role="switch" aria-checked={s.enabled} aria-label="Enable {s.name}" onclick={() => toggle(s)}><span></span></button>
        </div>
        {#if open === s.id}
          <div class="detail">
            <label>Schedule (cron, local time)
              <input bind:value={draft.cron} />
            </label>
            {#if s.kind === "agent"}
              <label>Output file <input bind:value={draft.output} placeholder="reports/{'{week}'}.md" /></label>
              <label>Prompt <textarea rows="5" bind:value={draft.prompt}></textarea></label>
            {/if}
            <div class="btns">
              <button class="primary" onclick={() => save(s)}>Save</button>
              <button class="quiet" onclick={() => (open = null)}>Cancel</button>
              {#if s.lastRun?.output}<button class="quiet" onclick={() => app.openNote(s.lastRun!.output!)}>Open last output</button>{/if}
            </div>
            {#if s.lastRun}
              <div class="log">
                <div class="log-head">Last run {new Date(s.lastRun.startedAt).toLocaleString()}{s.lastRun.message ? `: ${s.lastRun.message}` : ""}</div>
                {#if s.lastRun.log}<pre>{s.lastRun.log}</pre>{/if}
              </div>
            {/if}
          </div>
        {/if}
      </li>
    {:else}
      <li class="empty">No schedules. Add <code>[[schedules]]</code> entries to <code>.vault/config.toml</code>.</li>
    {/each}
  </ul>
</div>

<style>
  .sched {
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
    max-width: 42rem;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    border-bottom: 1px solid var(--rule);
  }
  li.off .name,
  li.off .ic {
    opacity: 0.5;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.9rem;
    padding: 0.85rem 0;
  }
  .ic {
    width: 32px;
    height: 32px;
    border-radius: 9px;
    display: grid;
    place-items: center;
    background: var(--sunken);
    color: var(--ink-2);
    flex: none;
  }
  .ic[data-kind="agent"] {
    background: var(--verdigris-wash);
    color: var(--verdigris);
  }
  .name {
    all: unset;
    cursor: pointer;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .when {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .last {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    font-size: 0.8rem;
    color: var(--muted);
    white-space: nowrap;
  }
  .st {
    font-weight: 700;
    color: var(--ink-2);
  }
  .st[data-status="ok"] {
    color: var(--verdigris);
  }
  .st[data-status="failed"] {
    color: var(--signal);
  }
  .st[data-status="running"] {
    color: var(--brass);
  }
  .next {
    color: var(--faint);
  }
  .switch {
    width: 36px;
    height: 21px;
    padding: 2px;
    border-radius: 99px;
    border: none;
    background: var(--rule-strong);
    display: flex;
  }
  .switch span {
    width: 17px;
    height: 17px;
    border-radius: 50%;
    background: var(--sheet);
    transition: transform 0.15s;
  }
  .switch[aria-checked="true"] {
    background: var(--verdigris);
  }
  .switch[aria-checked="true"] span {
    transform: translateX(15px);
  }
  .detail {
    padding: 0 0 1.2rem 2.9rem;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    font-size: 0.8rem;
    font-weight: 700;
    color: var(--muted);
  }
  label input,
  label textarea {
    font-weight: 400;
    color: var(--ink);
  }
  .btns {
    display: flex;
    gap: 0.4rem;
  }
  .log {
    border-radius: var(--radius-m);
    background: var(--sunken);
    padding: 0.6rem 0.8rem;
  }
  .log-head {
    font-size: 0.8rem;
    color: var(--muted);
  }
  pre {
    margin: 0.4rem 0 0;
    max-height: 14rem;
    overflow: auto;
    font-size: 0.78rem;
    white-space: pre-wrap;
  }
  .empty {
    padding: 1rem 0;
    color: var(--muted);
  }
  .error {
    color: var(--signal);
  }
</style>
