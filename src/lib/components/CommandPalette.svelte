<script lang="ts">
  import { api } from "$lib/api";
  import { app, noteTitle } from "$lib/app.svelte";
  import { chat } from "$lib/chat.svelte";
  import { ACTIONS } from "$lib/actions";

  type Item = { label: string; hint?: string; run: () => unknown };

  let query = $state("");
  let index = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let templates = $state<string[]>([]);

  $effect(() => {
    if (app.paletteOpen) {
      query = "";
      index = 0;
      setTimeout(() => input?.focus(), 0);
      api.templatesList().then((t) => (templates = t)).catch(() => {});
    }
  });

  const openNote = $derived(app.view.kind === "note" ? app.view.path : null);

  const commands = $derived<Item[]>([
    { label: "Go to Today", run: () => app.go({ kind: "today" }) },
    { label: "Go to Tasks", run: () => app.go({ kind: "tasks" }) },
    { label: "Go to Jira", run: () => app.go({ kind: "jira" }) },
    { label: "Go to Schedules", run: () => app.go({ kind: "schedules" }) },
    { label: "Open settings", run: () => app.go({ kind: "settings" }) },
    { label: "Search notes", hint: "Ctrl+Shift+F", run: () => (app.sidebar = "search") },
    { label: "Toggle agent panel", hint: "Ctrl+J", run: () => (app.agentOpen = !app.agentOpen) },
    ...ACTIONS.filter((a) => !a.needsFile || openNote).map((a) => ({
      label: `Agent: ${a.label}`,
      run: () => {
        if (a.needsPerson) {
          const person = openNote?.startsWith("people/") ? openNote.replace(/\.md$/, "") : prompt("Person note, e.g. people/alex");
          if (person) chat.ask(a.prompt({ person }), [`${person}.md`], a.label);
        } else chat.ask(a.prompt({ file: openNote ?? undefined }), a.needsFile && openNote ? [openNote] : [], a.label);
      },
    })),
    ...templates.map((t) => ({
      label: `New note from template: ${t}`,
      run: async () => {
        const name = prompt(`Path for the new ${t} note`, t === "person" ? "people/" : t === "one-on-one" ? `people/` : "");
        if (!name) return;
        const path = await api.noteCreate(name.endsWith(".md") ? name : `${name}.md`, t);
        await app.refresh();
        app.openNote(path);
      },
    })),
  ]);

  function score(text: string, q: string) {
    const t = text.toLowerCase();
    let pos = 0;
    let s = 0;
    for (const ch of q.toLowerCase()) {
      const i = t.indexOf(ch, pos);
      if (i < 0) return -1;
      s += i === pos ? 2 : 1;
      pos = i + 1;
    }
    return s - t.length * 0.01;
  }

  const items = $derived.by<Item[]>(() => {
    const q = query.trim();
    const notes: Item[] = app.notes.map((n) => ({ label: noteTitle(n), hint: n, run: () => app.openNote(n) }));
    if (!q) return [...commands.slice(0, 7), ...notes.slice(0, 8)];
    const all = q.startsWith(">") ? commands : [...notes, ...commands];
    const qq = q.replace(/^>/, "").trim();
    return all
      .map((i) => ({ i, s: Math.max(score(i.label, qq), score(i.hint ?? "", qq) - 1) }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 30)
      .map((x) => x.i);
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      index = Math.min(index + 1, items.length - 1);
      e.preventDefault();
    } else if (e.key === "ArrowUp") {
      index = Math.max(index - 1, 0);
      e.preventDefault();
    } else if (e.key === "Enter") {
      choose(items[index]);
    } else if (e.key === "Escape") {
      app.paletteOpen = false;
    }
  }

  async function choose(item?: Item) {
    if (!item) {
      const q = query.trim();
      if (q && !q.startsWith(">")) {
        app.paletteOpen = false;
        const path = await api.noteCreate(q.endsWith(".md") ? q : `${q}.md`);
        await app.refresh();
        app.openNote(path);
      }
      return;
    }
    app.paletteOpen = false;
    await item.run();
  }
</script>

{#if app.paletteOpen}
  <div class="scrim" role="presentation" onclick={() => (app.paletteOpen = false)}></div>
  <div class="palette" role="dialog" aria-label="Command palette">
    <input bind:this={input} bind:value={query} onkeydown={onKey} oninput={() => (index = 0)} placeholder="Open a note, or type > for commands" aria-label="Command" />
    <ul role="listbox">
      {#each items as item, i}
        <li role="option" aria-selected={i === index}>
          <button class:sel={i === index} onmouseenter={() => (index = i)} onclick={() => choose(item)}>
            <span>{item.label}</span>{#if item.hint}<span class="hint">{item.hint}</span>{/if}
          </button>
        </li>
      {:else}
        <li class="none">Press Enter to create “{query}”</li>
      {/each}
    </ul>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgb(16 22 28 / 0.25);
    z-index: 80;
  }
  .palette {
    position: fixed;
    z-index: 81;
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(36rem, 92vw);
    background: var(--sheet);
    border-radius: 12px;
    box-shadow: var(--shadow-pop);
    overflow: hidden;
  }
  input {
    width: 100%;
    border: none;
    border-bottom: 1px solid var(--rule);
    border-radius: 0;
    padding: 0.9rem 1.1rem;
    font-size: var(--step-1);
    background: transparent;
  }
  input:focus {
    box-shadow: none;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0.35rem;
    max-height: 50vh;
    overflow-y: auto;
  }
  li button {
    all: unset;
    box-sizing: border-box;
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    width: 100%;
    padding: 0.45rem 0.75rem;
    border-radius: var(--radius-m);
    cursor: pointer;
    font-size: 0.92rem;
  }
  li button.sel {
    background: var(--verdigris);
    color: var(--verdigris-ink);
  }
  .hint {
    opacity: 0.6;
    font-size: 0.8rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .none {
    padding: 0.6rem 0.75rem;
    color: var(--muted);
    font-size: 0.9rem;
  }
</style>
