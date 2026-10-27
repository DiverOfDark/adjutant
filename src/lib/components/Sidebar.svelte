<script lang="ts">
  import { FilePlus, FolderPlus, RefreshCw, GitBranch } from "@lucide/svelte";
  import { api, type RepoInfo, type SearchResults, type TreeNode } from "$lib/api";
  import { app, personName, relTime } from "$lib/app.svelte";
  import { checkPath, dialog, withMd } from "$lib/dialog.svelte";
  import FileTree from "./FileTree.svelte";

  let query = $state("");
  let results = $state<SearchResults | null>(null);
  let searchInput: HTMLInputElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout>;
  let menu = $state<{ x: number; y: number; node: TreeNode } | null>(null);
  let repos = $state<RepoInfo[]>([]);
  let pulling = $state<string | null>(null);

  $effect(() => {
    app.revision;
    if (app.vault) api.reposList().then((r) => (repos = r)).catch(() => (repos = []));
  });

  async function pull(name: string) {
    pulling = name;
    try {
      const out = await api.repoPull(name);
      app.notify(`${name}: ${out.split("\n").at(-1)}`);
      repos = await api.reposList();
    } catch (e) {
      app.notify(String(e), "error");
    } finally {
      pulling = null;
    }
  }

  $effect(() => {
    if (app.sidebar === "search") setTimeout(() => searchInput?.focus(), 0);
  });

  function onQuery() {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      results = query.trim().length > 1 ? await api.search(query.trim()) : null;
    }, 160);
  }

  async function newNote(dir = "") {
    const name = await dialog.prompt({
      title: "New note",
      label: "Name or path in the vault",
      value: dir ? `${dir}/` : "",
      placeholder: "people/alex",
      confirm: "Create",
      validate: checkPath,
      preview: (v) => `Creates ${withMd(v)}`,
    });
    if (!name) return;
    try {
      const path = await api.noteCreate(withMd(name));
      await app.refresh();
      app.openNote(path);
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function newFolder(dir = "") {
    const name = await dialog.prompt({
      title: "New folder",
      label: "Name or path in the vault",
      value: dir ? `${dir}/` : "",
      placeholder: "teams/platform",
      confirm: "Create",
      validate: checkPath,
    });
    if (!name) return;
    try {
      await api.dirCreate(name.replace(/\/+$/, ""));
      await app.refresh();
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function rename(node: TreeNode) {
    const to = await dialog.prompt({
      title: node.isDir ? "Rename folder" : "Rename note",
      label: "New name; change the folder part to move it",
      value: node.path,
      confirm: "Rename",
      selectName: !node.isDir,
      validate: (v) => checkPath(v) ?? (v.trim() === node.path ? "That's the current name" : null),
      preview: (v) => (node.isDir ? null : "Links to it in other notes are updated"),
    });
    if (!to) return;
    try {
      const n = await api.noteRename(node.path, to);
      await app.refresh();
      app.notify(n ? `Renamed; updated links in ${n} notes` : "Renamed");
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  async function trash(node: TreeNode) {
    const ok = await dialog.confirm({
      title: `Delete ${node.path.split("/").pop()}?`,
      body: `${node.path} moves to the vault trash (.vault/trash). You can restore it from there.`,
      confirm: "Move to trash",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.noteTrash(node.path);
      await app.refresh();
    } catch (e) {
      app.notify(String(e), "error");
    }
  }

  const people = $derived(
    app.notes
      .filter((n) => n.startsWith("people/"))
      .map((path) => {
        const stem = path.replace(/\.md$/, "");
        const waiting = app.openTasks.filter((t) => t.section === "Waiting on" && t.people.includes(stem)).length;
        const open = app.openTasks.filter((t) => t.people.includes(stem)).length;
        return { path, name: personName(stem), waiting, open };
      }),
  );

  function highlight(text: string, ranges: [number, number][]) {
    let out = "";
    let pos = 0;
    const esc = (s: string) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]!);
    for (const [a, b] of ranges) {
      out += esc(text.slice(pos, a)) + "<mark>" + esc(text.slice(a, b)) + "</mark>";
      pos = b;
    }
    return out + esc(text.slice(pos));
  }
</script>

<svelte:window onclick={() => (menu = null)} />

<aside class="sidebar">
  {#if app.sidebar === "files"}
    <header>
      <h2 title={app.vault?.path}>{app.vault?.name}</h2>
      <button class="icon" title="New note" aria-label="New note" onclick={() => newNote()}><FilePlus size={16} /></button>
      <button class="icon" title="New folder" aria-label="New folder" onclick={() => newFolder()}><FolderPlus size={16} /></button>
      <button class="icon" title="Reload" aria-label="Reload" onclick={() => app.refresh()}><RefreshCw size={15} /></button>
    </header>
    <div class="scroll">
      {#if app.tree}
        <FileTree node={app.tree} oncontext={(e, node) => { e.preventDefault(); menu = { x: e.clientX, y: e.clientY, node }; }} />
      {/if}
      {#if repos.length}
        <h3 class="sub">Repositories</h3>
        {#each repos as r (r.name)}
          <div class="repo" title={r.remote ?? r.path}>
            <GitBranch size={14} />
            <span class="rname">{r.name}</span>
            <span class="pulled">{r.lastPulled ? relTime(r.lastPulled) : ""}</span>
            <button class="icon" aria-label="Pull {r.name}" title="git pull" onclick={() => pull(r.name)} disabled={pulling === r.name}>
              <RefreshCw size={13} class={pulling === r.name ? "spin" : ""} />
            </button>
          </div>
        {/each}
      {/if}
    </div>
  {:else if app.sidebar === "search"}
    <header><h2>Search</h2></header>
    <div class="pad">
      <input bind:this={searchInput} bind:value={query} oninput={onQuery} placeholder="Search notes" type="search" />
    </div>
    <div class="scroll">
      {#if results}
        {#each results.fileMatches as f}
          <button class="hit file" onclick={() => app.openNote(f)}>{f}</button>
        {/each}
        {#each results.hits as hit}
          <button class="hit" onclick={() => app.openNote(hit.file, hit.line)}>
            <span class="where">{hit.file}<span class="ln">:{hit.line + 1}</span></span>
            <span class="text">{@html highlight(hit.text, hit.ranges)}</span>
          </button>
        {/each}
        {#if results.hits.length === 0 && results.fileMatches.length === 0}
          <p class="empty">Nothing matches “{query}”.</p>
        {/if}
        {#if results.truncated}<p class="empty">Showing the first {results.hits.length} matches.</p>{/if}
      {:else}
        <p class="empty">Type at least two characters. Matches every word, ignoring case unless you type a capital.</p>
      {/if}
    </div>
  {:else}
    <header><h2>People</h2></header>
    <div class="scroll">
      {#each people as p}
        <button class="person" class:current={app.view.kind === "note" && app.view.path === p.path} onclick={() => app.openNote(p.path)}>
          <span class="avatar">{p.name.slice(0, 1)}</span>
          <span class="pname">{p.name}</span>
          {#if p.waiting}<span class="waiting" title="Waiting on {p.name}">{p.waiting} waiting</span>{/if}
        </button>
      {:else}
        <p class="empty">Add a note under <code>people/</code> for each person you work with.</p>
      {/each}
      <button class="quiet add" onclick={() => newNote("people")}>Add person</button>
    </div>
  {/if}
</aside>

{#if menu}
  <div class="menu" style="left: {menu.x}px; top: {menu.y}px" role="menu">
    {#if menu.node.isDir}
      <button role="menuitem" onclick={() => newNote(menu!.node.path)}>New note here</button>
      <button role="menuitem" onclick={() => newFolder(menu!.node.path)}>New folder here</button>
    {/if}
    <button role="menuitem" onclick={() => rename(menu!.node)}>Rename or move</button>
    <button role="menuitem" class="danger" onclick={() => trash(menu!.node)}>Move to trash</button>
  </div>
{/if}

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--rule);
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.1rem;
    padding: 0.85rem 0.6rem 0.5rem 0.9rem;
  }
  h2 {
    flex: 1;
    margin: 0;
    font-size: var(--step-0);
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    margin: 1.2rem 0.5rem 0.3rem;
    font-size: 0.78rem;
    font-weight: 700;
    color: var(--muted);
  }
  .repo {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.1rem 0.2rem 0.1rem 0.5rem;
    font-size: 0.85rem;
    color: var(--ink-2);
  }
  .repo :global(svg) {
    color: var(--faint);
    flex: none;
  }
  .rname {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pulled {
    font-size: 0.72rem;
    color: var(--faint);
  }
  .pad {
    padding: 0 0.6rem 0.5rem;
  }
  .pad input {
    width: 100%;
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 0.4rem 1rem;
  }
  .hit {
    display: flex;
    flex-direction: column;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    padding: 0.35rem 0.5rem;
    border-radius: var(--radius-s);
    font-weight: 400;
    gap: 0.05rem;
  }
  .hit:hover:not(:disabled) {
    background: var(--sunken);
  }
  .hit.file {
    font-weight: 700;
    font-size: 0.875rem;
  }
  .where {
    font-size: 0.75rem;
    color: var(--muted);
  }
  .ln {
    color: var(--faint);
  }
  .text {
    font-size: 0.85rem;
    color: var(--ink-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .text :global(mark) {
    background: var(--brass-wash);
    color: var(--ink);
    border-radius: 2px;
  }
  .empty {
    color: var(--muted);
    font-size: 0.85rem;
    padding: 0.4rem 0.6rem;
    margin: 0;
  }
  .person {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
    border: none;
    background: none;
    padding: 0.4rem 0.5rem;
    font-weight: 400;
    font-size: 0.9rem;
    text-align: left;
  }
  .person:hover:not(:disabled),
  .person.current {
    background: var(--sunken);
  }
  .avatar {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--ink);
    color: var(--paper);
    font-weight: 700;
    font-size: 0.8rem;
    flex: none;
  }
  .pname {
    flex: 1;
  }
  .waiting {
    font-size: 0.72rem;
    font-weight: 700;
    color: var(--brass);
  }
  .add {
    margin: 0.4rem 0.3rem;
  }
  .menu {
    position: fixed;
    z-index: 50;
    background: var(--sheet);
    box-shadow: var(--shadow-pop);
    border-radius: var(--radius-m);
    padding: 0.25rem;
    display: flex;
    flex-direction: column;
    min-width: 170px;
  }
  .menu button {
    border: none;
    background: none;
    text-align: left;
    font-weight: 400;
    font-size: 0.875rem;
  }
  .menu button:hover {
    background: var(--sunken);
  }
</style>
