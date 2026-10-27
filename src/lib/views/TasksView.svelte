<script lang="ts">
  import { api, SECTIONS, type Task } from "$lib/api";
  import { app, daysFromToday } from "$lib/app.svelte";
  import TaskRow from "$lib/components/TaskRow.svelte";

  let filter = $state("");
  let group = $state<"section" | "due">("section");
  let newTask = $state("");
  let newSection = $state("Now");

  const matches = (t: Task) => {
    const f = filter.trim().toLowerCase();
    if (!f) return true;
    return f.split(/\s+/).every((w) => t.text.toLowerCase().includes(w) || t.file.toLowerCase().includes(w));
  };
  const open = $derived(app.openTasks.filter(matches));
  const groups = $derived.by(() => {
    if (group === "due") {
      const buckets: [string, Task[]][] = [
        ["Late", []],
        ["Today", []],
        ["This week", []],
        ["Later", []],
        ["No date", []],
      ];
      for (const t of open) {
        const d = t.due ? daysFromToday(t.due) : null;
        const i = d == null ? 4 : d < 0 ? 0 : d === 0 ? 1 : d <= 7 ? 2 : 3;
        buckets[i][1].push(t);
      }
      buckets.forEach(([, ts]) => ts.sort((a, b) => (a.due ?? "9").localeCompare(b.due ?? "9")));
      return buckets.filter(([, ts]) => ts.length);
    }
    const main = SECTIONS.map((s) => [s, open.filter((t) => t.file === "tasks.md" && t.section === s)] as [string, Task[]]);
    const unsectioned = open.filter((t) => t.file === "tasks.md" && !SECTIONS.includes(t.section as never));
    const elsewhere = open.filter((t) => t.file !== "tasks.md");
    return [...main, ["Other sections", unsectioned] as [string, Task[]], ["In other notes", elsewhere] as [string, Task[]]].filter(
      ([name, ts]) => ts.length || SECTIONS.includes(name as never),
    );
  });

  async function move(t: Task, section: string) {
    try {
      await api.taskMove(t, section);
      await app.refreshTasks();
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function add(e: Event) {
    e.preventDefault();
    if (!newTask.trim()) return;
    await api.taskAdd(newSection, newTask.trim());
    newTask = "";
    await app.refreshTasks();
  }
</script>

<div class="tasks">
  <header>
    <h1>Tasks</h1>
    <input class="filter" type="search" bind:value={filter} placeholder="Filter by word, #tag, person or file" />
    <div class="seg" role="radiogroup" aria-label="Group by">
      <button role="radio" aria-checked={group === "section"} class:on={group === "section"} onclick={() => (group = "section")}>By section</button>
      <button role="radio" aria-checked={group === "due"} class:on={group === "due"} onclick={() => (group = "due")}>By due date</button>
    </div>
  </header>

  <form class="add" onsubmit={add}>
    <input bind:value={newTask} placeholder="New task" aria-label="New task" />
    <select bind:value={newSection} aria-label="Section">{#each SECTIONS as s}<option>{s}</option>{/each}</select>
    <button class="primary" disabled={!newTask.trim()}>Add</button>
  </form>

  {#each groups as [name, tasks] (name)}
    <section>
      <h2>{name} <span class="n">{tasks.length}</span></h2>
      {#each tasks as task (task.file + task.line + task.raw)}
        <div class="row">
          <TaskRow {task} showSource={group === "due" || task.file !== "tasks.md"} />
          {#if task.file === "tasks.md"}
            <select class="move" aria-label="Move to section" value="" onchange={(e) => move(task, e.currentTarget.value)}>
              <option value="" disabled>Move</option>
              {#each SECTIONS.filter((s) => s !== task.section) as s}<option value={s}>{s}</option>{/each}
            </select>
          {/if}
        </div>
      {:else}
        <p class="empty">Empty.</p>
      {/each}
    </section>
  {/each}
</div>

<style>
  .tasks {
    height: 100%;
    overflow-y: auto;
    padding: 2.4rem clamp(1.5rem, 5vw, 4.5rem) 4rem;
  }
  header {
    display: flex;
    align-items: center;
    gap: 1rem;
    flex-wrap: wrap;
    padding-bottom: 1rem;
    border-bottom: 2px solid var(--ink);
  }
  h1 {
    margin: 0 auto 0 0;
    font-size: var(--step-3);
    font-weight: 800;
    letter-spacing: -0.03em;
  }
  .filter {
    width: min(22rem, 100%);
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius-m);
    padding: 2px;
  }
  .seg button {
    border: none;
    background: none;
    color: var(--muted);
    padding: 0.28rem 0.7rem;
  }
  .seg button.on {
    background: var(--ink);
    color: var(--paper);
  }
  .add {
    display: flex;
    gap: 0.4rem;
    margin: 1.2rem 0 0.6rem;
  }
  .add input {
    flex: 1;
  }
  section {
    margin-top: 1.6rem;
  }
  h2 {
    font-size: var(--step-0);
    margin: 0 0 0.3rem;
  }
  .n {
    color: var(--faint);
    font-weight: 400;
    margin-left: 0.3rem;
  }
  .row {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
    border-bottom: 1px solid var(--rule);
  }
  .row :global(.task) {
    flex: 1;
  }
  .move {
    margin-top: 0.35rem;
    font-size: 0.78rem;
    padding: 0.15rem 0.3rem;
    opacity: 0;
    background: transparent;
  }
  .row:hover .move,
  .move:focus {
    opacity: 1;
  }
  .empty {
    color: var(--faint);
    font-size: 0.85rem;
    margin: 0.2rem 0;
  }
</style>
