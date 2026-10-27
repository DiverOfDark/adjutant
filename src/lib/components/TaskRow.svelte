<script lang="ts">
  import type { Task } from "$lib/api";
  import { app, daysFromToday, dueLabel, personName } from "$lib/app.svelte";
  import JiraBadge from "./JiraBadge.svelte";

  let { task, showSource = false, hidePerson = false }: { task: Task; showSource?: boolean; hidePerson?: boolean } = $props();
  let leaving = $state(false);

  type Piece = { t: "text"; v: string } | { t: "link"; v: string } | { t: "tag"; v: string } | { t: "jira"; v: string };
  const pieces = $derived.by(() => {
    const out: Piece[] = [];
    const re = /\[\[([^\]|#]+)(?:[#|][^\]]*)?\]\]|(?:^|(?<=\s))#([\p{L}\p{N}_/-]+)|\b([A-Z][A-Z0-9]+-\d+)\b/gu;
    let pos = 0;
    for (const m of task.title.matchAll(re)) {
      if (m.index! > pos) out.push({ t: "text", v: task.title.slice(pos, m.index) });
      if (m[1]) out.push({ t: "link", v: m[1].trim() });
      else if (m[2]) out.push({ t: "tag", v: m[2] });
      else out.push({ t: "jira", v: m[3] });
      pos = m.index! + m[0].length;
    }
    if (pos < task.title.length) out.push({ t: "text", v: task.title.slice(pos) });
    if (hidePerson) {
      // "Load-test results from [[people/sam]]" reads "Load-test results" next to Sam's name.
      const visible = out.filter((p) => !(p.t === "link" && p.v.startsWith("people/")));
      const last = visible.at(-1);
      if (last?.t === "text") last.v = last.v.replace(/\s+(from|for|by|with|to|on)\s*$/i, "");
      return visible;
    }
    return out;
  });
  const dueIn = $derived(task.due ? daysFromToday(task.due) : null);

  async function tick() {
    leaving = true;
    await new Promise((r) => setTimeout(r, 260));
    await app.completeTask(task);
  }
</script>

<div class="task" class:leaving>
  <button class="box" role="checkbox" aria-checked={leaving} aria-label="Mark done: {task.title}" onclick={tick}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.5 8.5l3 3 6-7" /></svg>
  </button>
  <div class="body">
    <span class="title">
      {#each pieces as p}
        {#if p.t === "text"}{p.v}{:else if p.t === "link"}
          {#if !(hidePerson && p.v.startsWith("people/"))}
            <button class="chip person" onclick={() => app.followLink(p.v)}>{p.v.startsWith("people/") ? personName(p.v) : p.v}</button>
          {/if}
        {:else if p.t === "tag"}<span class="tag">#{p.v}</span>{:else}<JiraBadge key={p.v} />{/if}
      {/each}
    </span>
    {#if task.due || showSource}
      <span class="meta">
        {#if task.due}
          <span class="due" class:late={dueIn! < 0} class:soon={dueIn! >= 0 && dueIn! <= 2}>{dueLabel(task.due)}</span>
        {/if}
        {#if showSource && task.file !== "tasks.md"}
          <button class="src" onclick={() => app.openNote(task.file, task.line)}>{task.file}</button>
        {/if}
      </span>
    {/if}
  </div>
</div>

<style>
  .task {
    display: flex;
    align-items: flex-start;
    gap: 0.7rem;
    padding: 0.42rem 0;
    transition: opacity 0.25s, transform 0.25s;
  }
  .task.leaving {
    opacity: 0;
    transform: translateX(12px);
  }
  .box {
    flex: none;
    width: 19px;
    height: 19px;
    margin-top: 0.12rem;
    padding: 0;
    border: 1.6px solid var(--rule-strong);
    border-radius: 6px;
    background: var(--sheet);
    display: grid;
    place-items: center;
  }
  .box svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: var(--verdigris-ink);
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0;
  }
  .box:hover:not(:disabled) {
    border-color: var(--verdigris);
  }
  .box:hover svg {
    opacity: 0.35;
    stroke: var(--verdigris);
  }
  .leaving .box {
    background: var(--verdigris);
    border-color: var(--verdigris);
  }
  .leaving .box svg {
    opacity: 1;
    stroke: var(--verdigris-ink);
  }
  .body {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: 0.7rem;
    min-width: 0;
  }
  .title {
    line-height: 1.5;
  }
  .chip {
    all: unset;
    cursor: pointer;
    font-weight: 600;
    color: var(--verdigris);
  }
  .chip:hover {
    text-decoration: underline;
  }
  .tag {
    color: var(--muted);
    font-size: 0.9em;
  }
  .meta {
    display: inline-flex;
    gap: 0.6rem;
    font-size: 0.8rem;
    color: var(--muted);
  }
  .due.soon {
    color: var(--brass);
    font-weight: 700;
  }
  .due.late {
    color: var(--signal);
    font-weight: 700;
  }
  .src {
    all: unset;
    cursor: pointer;
    color: var(--faint);
  }
  .src:hover {
    color: var(--ink);
  }
</style>
