<script lang="ts">
  import { Sparkles, Inbox, Sunset } from "@lucide/svelte";
  import { api, SECTIONS, type Task } from "$lib/api";
  import { app, daysFromToday, personName } from "$lib/app.svelte";
  import { chat } from "$lib/chat.svelte";
  import { ACTIONS } from "$lib/actions";
  import TaskRow from "$lib/components/TaskRow.svelte";
  import JiraBadge from "$lib/components/JiraBadge.svelte";

  const now = new Date();
  const weekday = now.toLocaleDateString(undefined, { weekday: "long" });
  const date = now.toLocaleDateString(undefined, { day: "numeric", month: "long" });

  let newTask = $state("");
  let section = $state<string>("Now");

  const nowTasks = $derived(app.openTasks.filter((t) => t.file === "tasks.md" && t.section === "Now"));
  const waiting = $derived.by(() => {
    const groups = new Map<string, Task[]>();
    for (const t of app.openTasks.filter((t) => t.section === "Waiting on")) {
      const who = t.people[0] ?? "";
      groups.set(who, [...(groups.get(who) ?? []), t]);
    }
    return [...groups.entries()].sort(([a], [b]) => (a === "" ? 1 : b === "" ? -1 : a.localeCompare(b)));
  });
  const dueSoon = $derived(
    app.openTasks
      .filter((t) => t.due && daysFromToday(t.due) <= 7 && !(t.file === "tasks.md" && (t.section === "Now" || t.section === "Waiting on")))
      .sort((a, b) => a.due!.localeCompare(b.due!)),
  );
  const late = $derived(app.openTasks.filter((t) => t.due && daysFromToday(t.due) < 0).length);
  const inbox = $derived(app.notes.filter((n) => n.startsWith("inbox/") && !n.startsWith("inbox/filed/")));
  const myJira = $derived(
    app.jira ? (app.jira.queries.mine ?? Object.values(app.jira.queries)[0] ?? []).map((k) => app.jira!.issues[k]).filter(Boolean) : [],
  );

  const summary = $derived.by(() => {
    const parts: string[] = [];
    parts.push(nowTasks.length === 1 ? "One thing in Now" : `${nowTasks.length || "Nothing"} in Now`);
    if (late) parts.push(`${late} late`);
    const w = waiting.reduce((n, [, ts]) => n + ts.length, 0);
    if (w) parts.push(`waiting on ${w}`);
    return parts.join(", ") + ".";
  });

  async function add(e: Event) {
    e.preventDefault();
    if (!newTask.trim()) return;
    await api.taskAdd(section, newTask.trim());
    newTask = "";
    await app.refreshTasks();
  }

  const action = (id: string) => ACTIONS.find((a) => a.id === id)!;
</script>

<div class="today">
  <header class="masthead">
    <div class="date">
      <h1>{weekday}</h1>
      <p class="sub"><span class="d">{date}</span> {summary}</p>
    </div>
    <div class="actions">
      <button class="primary" onclick={() => chat.ask(action("plan-day").prompt({}), [], "Plan my day")} disabled={chat.busy}>
        <Sparkles size={15} /> Plan my day
      </button>
      <button onclick={() => chat.ask(action("end-of-day").prompt({}), [], "End-of-day summary")} disabled={chat.busy}>
        <Sunset size={15} /> End-of-day summary
      </button>
    </div>
  </header>

  <div class="grid">
    <section class="now">
      <h2>Now</h2>
      {#each nowTasks as task (task.raw + task.line)}
        <TaskRow {task} />
      {:else}
        <p class="empty">Nothing in Now. Pull something up from Next, or ask the agent to plan your day.</p>
      {/each}
      <form class="add" onsubmit={add}>
        <input bind:value={newTask} placeholder="Add a task" aria-label="New task" />
        <select bind:value={section} aria-label="Section">
          {#each SECTIONS as s}<option>{s}</option>{/each}
        </select>
      </form>

      {#if dueSoon.length}
        <h2 class="later">Due this week</h2>
        {#each dueSoon as task (task.file + task.line)}
          <TaskRow {task} showSource />
        {/each}
      {/if}
    </section>

    <aside class="side">
      <section>
        <h2>Waiting on</h2>
        {#each waiting as [who, tasks]}
          <div class="who">
            {#if who}
              <button class="person" onclick={() => app.followLink(who)}>
                <span class="avatar">{personName(who).slice(0, 1)}</span>{personName(who)}
              </button>
            {:else}
              <span class="person nobody">Unassigned</span>
            {/if}
            {#each tasks as task (task.file + task.line)}
              <TaskRow {task} hidePerson />
            {/each}
          </div>
        {:else}
          <p class="empty">Nobody owes you anything. Tasks under <em>Waiting on</em> with a [[people/…]] link appear here.</p>
        {/each}
      </section>

      {#if inbox.length}
        <section class="inbox">
          <div class="inbox-row">
            <Inbox size={17} />
            <span><strong>{inbox.length}</strong> {inbox.length === 1 ? "note" : "notes"} in the inbox</span>
            <button onclick={() => chat.ask(action("organize-inbox").prompt({}), [], "Organize inbox")} disabled={chat.busy}>Organize inbox</button>
          </div>
        </section>
      {/if}

      <section>
        <h2>My Jira</h2>
        {#if !app.jira}
          <p class="empty">Connect Jira in <button class="link" onclick={() => app.go({ kind: "settings" })}>Settings</button> to see your tickets here.</p>
        {:else}
          {#each myJira as issue (issue.key)}
            <div class="issue">
              <JiraBadge key={issue.key} />
              <span class="summary">{issue.summary}</span>
            </div>
          {:else}
            <p class="empty">No open tickets assigned to you.</p>
          {/each}
        {/if}
      </section>
    </aside>
  </div>
</div>

<style>
  .today {
    container-type: inline-size;
    height: 100%;
    overflow-y: auto;
    padding: 3.2rem clamp(1.5rem, 5vw, 4.5rem) 4rem;
  }
  .masthead {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 1.5rem;
    padding-bottom: 1.6rem;
    border-bottom: 2px solid var(--ink);
    margin-bottom: 2rem;
  }
  h1 {
    margin: 0;
    font-size: clamp(2.6rem, 6vw, 4.6rem);
    line-height: 0.95;
    font-weight: 800;
    letter-spacing: -0.035em;
  }
  .sub {
    margin: 0.7rem 0 0;
    font-size: var(--step-1);
    color: var(--muted);
  }
  .sub .d {
    color: var(--ink);
    font-weight: 700;
    margin-right: 0.4rem;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
  }
  .actions button {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    font-size: 0.875rem;
    padding: 0.5rem 0.95rem;
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr);
    gap: 3rem;
  }
  @container (max-width: 860px) {
    .grid {
      grid-template-columns: 1fr;
      gap: 2.2rem;
    }
  }
  h2 {
    margin: 0 0 0.5rem;
    font-size: var(--step-0);
    font-weight: 700;
    color: var(--ink);
  }
  h2.later {
    margin-top: 2.2rem;
  }
  .now :global(.task) {
    font-size: var(--step-1);
    border-bottom: 1px solid var(--rule);
    padding: 0.62rem 0;
  }
  .add {
    display: flex;
    gap: 0.4rem;
    margin-top: 0.8rem;
  }
  .add input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border-style: dashed;
  }
  .add input:focus {
    background: var(--sheet);
    border-style: solid;
  }
  .add select {
    background: transparent;
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: 2rem;
  }
  .who {
    margin-bottom: 0.9rem;
  }
  .who :global(.task) {
    font-size: 0.9rem;
    padding: 0.25rem 0 0.25rem 2.1rem;
  }
  .person {
    all: unset;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.55rem;
    font-weight: 700;
    font-size: 0.9rem;
  }
  .person:hover {
    color: var(--verdigris);
  }
  .person.nobody {
    color: var(--muted);
    cursor: default;
  }
  .avatar {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--ink);
    color: var(--paper);
    font-size: 0.75rem;
  }
  .inbox-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.75rem 0.9rem;
    border-radius: var(--radius-m);
    background: var(--brass-wash);
    color: var(--ink);
    font-size: 0.9rem;
  }
  .inbox-row span {
    flex: 1;
  }
  .inbox-row :global(svg) {
    color: var(--brass);
  }
  .issue {
    display: flex;
    gap: 0.6rem;
    align-items: baseline;
    padding: 0.32rem 0;
    font-size: 0.9rem;
    border-bottom: 1px solid var(--rule);
  }
  .summary {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-2);
  }
  .empty {
    color: var(--muted);
    font-size: 0.9rem;
    margin: 0.3rem 0;
  }
  .link {
    all: unset;
    cursor: pointer;
    color: var(--verdigris);
    text-decoration: underline;
  }
</style>
