<script lang="ts">
  import { ChevronRight, EyeOff, Eye, ListPlus, FileSignature, RefreshCcw } from "@lucide/svelte";
  import { api, type Backlink, type Visibility } from "$lib/api";
  import { app, personName } from "$lib/app.svelte";
  import { chat } from "$lib/chat.svelte";
  import { ACTIONS, resyncSource } from "$lib/actions";
  import Editor from "$lib/editor/Editor.svelte";
  import TaskRow from "$lib/components/TaskRow.svelte";

  let { path, line }: { path: string; line?: number } = $props();

  let content = $state<string | null>(null);
  let loadedPath = "";
  let dirty = $state(false);
  let saving = $state(false);
  let saveTimer: ReturnType<typeof setTimeout>;
  let latest = "";
  let backlinks = $state<Backlink[]>([]);
  let visibility = $state<Visibility | null>(null);
  let error = $state<string | null>(null);
  let editor: Editor | undefined = $state();

  const crumbs = $derived(path.split("/"));
  const stem = $derived(path.replace(/\.md$/, ""));
  const isMarkdown = $derived(/\.(md|markdown)$/i.test(path));
  const sourceUrl = $derived(content?.match(/^---\n[\s\S]*?^source:\s*(\S+)[\s\S]*?^---/m)?.[1] ?? null);
  const isPerson = $derived(isMarkdown && path.startsWith("people/") && path.split("/").length === 2);
  const personTasks = $derived(isPerson ? app.openTasks.filter((t) => t.people.includes(stem) || (t.file === path)) : []);
  const waitingOn = $derived(personTasks.filter((t) => t.section === "Waiting on"));
  const otherTasks = $derived(personTasks.filter((t) => t.section !== "Waiting on"));

  async function load(p: string) {
    error = null;
    try {
      const text = await api.noteRead(p);
      if (p !== path) return;
      content = text;
      latest = text;
      loadedPath = p;
      dirty = false;
      editor?.setContent(text);
      [backlinks, visibility] = await Promise.all([api.backlinks(p), api.noteVisibility(p).catch(() => null)]);
    } catch (e) {
      error = String(e);
    }
  }

  async function save() {
    clearTimeout(saveTimer);
    if (!dirty || loadedPath !== path) return;
    saving = true;
    try {
      await api.noteWrite(path, latest);
      dirty = false;
      if (latest.includes("- [")) app.refreshTasks();
    } catch (e) {
      app.notify(`Could not save ${path}: ${e}`, "error");
    } finally {
      saving = false;
    }
  }

  function onchange(text: string) {
    latest = text;
    dirty = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(save, 700);
  }

  $effect(() => {
    const p = path;
    if (p !== loadedPath) {
      if (dirty) save();
      content = null;
      load(p);
    }
  });

  // Reload when the file changes on disk and there are no local edits.
  $effect(() => {
    app.revision;
    if (loadedPath === path && !dirty) {
      api.noteRead(path).then((text) => {
        if (!dirty && text !== latest) {
          latest = text;
          editor?.setContent(text);
        }
      });
      api.backlinks(path).then((b) => (backlinks = b));
    }
  });

  $effect(() => () => {
    if (dirty) api.noteWrite(loadedPath, latest);
  });

  function tidy(line: string) {
    return line
      .replace(/^\s*(?:[-*+]\s+(?:\[[ xX]\]\s+)?|>\s*|#+\s+)/, "")
      .replace(/\[\[([^\]|#]+)(?:#[^\]|]*)?(?:\|([^\]]*))?\]\]/g, (_, t: string, a?: string) => a ?? personName(t.trim()))
      .replace(/@due\(([^)]*)\)/g, "due $1");
  }

  const extract = ACTIONS.find((a) => a.id === "extract-tasks")!;
  const review = ACTIONS.find((a) => a.id === "draft-review")!;
</script>

<div class="note">
  <header class="bar">
    <nav class="crumbs" aria-label="Path">
      {#each crumbs as c, i}
        {#if i > 0}<ChevronRight size={13} />{/if}
        <span class:last={i === crumbs.length - 1}>{c}</span>
      {/each}
    </nav>
    <span class="state">{saving ? "Saving" : dirty ? "Edited" : "Saved"}</span>
    {#if visibility}
      <span class="vis" class:hidden={visibility.hidden} title={visibility.hidden ? "Agents can't read this folder (.vault/config.toml)" : `Readable by ${visibility.agent}`}>
        {#if visibility.hidden}<EyeOff size={14} /> Hidden from agents{:else}<Eye size={14} /> {visibility.agent}{/if}
      </span>
    {/if}
    {#if sourceUrl}
      <button class="quiet" title={sourceUrl} onclick={() => chat.ask(resyncSource(path, sourceUrl!), [path], `Re-sync ${path} from source`)} disabled={chat.busy || visibility?.hidden}>
        <RefreshCcw size={15} /> Re-sync from source
      </button>
    {/if}
    {#if isMarkdown}<button class="quiet" onclick={() => chat.ask(extract.prompt({ file: path }), [path], `Extract tasks from ${path}`)} disabled={chat.busy || visibility?.hidden}>
      <ListPlus size={15} /> Extract tasks
    </button>{/if}
  </header>

  {#if isPerson}
    <section class="prep">
      <div class="prep-head">
        <h3>For your next 1:1 with {personName(stem)}</h3>
        <button class="quiet" onclick={() => chat.ask(review.prompt({ person: stem }), [path], `Draft review for ${personName(stem)}`)} disabled={chat.busy || visibility?.hidden}>
          <FileSignature size={15} /> Draft review
        </button>
      </div>
      <div class="cols">
        <div>
          <h4>You're waiting on</h4>
          {#each waitingOn as task (task.file + task.line)}<TaskRow {task} hidePerson showSource />{:else}<p class="none">Nothing.</p>{/each}
        </div>
        <div>
          <h4>Open tasks mentioning them</h4>
          {#each otherTasks as task (task.file + task.line)}<TaskRow {task} hidePerson showSource />{:else}<p class="none">Nothing.</p>{/each}
        </div>
      </div>
    </section>
  {/if}

  <div class="sheet">
    {#if error}
      <p class="error">{error}</p>
    {:else if content !== null}
      <Editor bind:this={editor} {path} {content} {onchange} onsave={save} {line} />
    {/if}
  </div>

  {#if backlinks.length}
    <footer class="backlinks">
      <h4>Linked from {backlinks.length} {backlinks.length === 1 ? "place" : "places"}</h4>
      <ul>
        {#each backlinks as b}
          <li>
            <button onclick={() => app.openNote(b.file, b.line)}>
              <span class="from">{b.file}</span>
              <span class="ctx">{tidy(b.text)}</span>
            </button>
          </li>
        {/each}
      </ul>
    </footer>
  {/if}
</div>

<style>
  .note {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 0.9rem;
    padding: 0.55rem 1rem 0.55rem 1.4rem;
    border-bottom: 1px solid var(--rule);
    font-size: 0.85rem;
  }
  .crumbs {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.25rem;
    color: var(--muted);
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }
  .crumbs :global(svg) {
    color: var(--faint);
    flex: none;
  }
  .crumbs .last {
    color: var(--ink);
    font-weight: 700;
  }
  .state {
    color: var(--faint);
    font-size: 0.78rem;
  }
  .vis {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .vis.hidden {
    color: var(--brass);
    font-weight: 700;
  }
  .bar button {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }
  .sheet {
    flex: 1;
    min-height: 0;
    background: var(--sheet);
    padding: 2.2rem 1.5rem 0;
  }
  .prep {
    padding: 1rem 1.4rem 1.1rem;
    border-bottom: 1px solid var(--rule);
    background: var(--paper);
  }
  .prep-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.4rem;
  }
  .prep-head button {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }
  h3 {
    margin: 0;
    font-size: var(--step-0);
  }
  h4 {
    margin: 0.3rem 0 0.2rem;
    font-size: 0.8rem;
    color: var(--muted);
    font-weight: 700;
  }
  .cols {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1.5rem;
    font-size: 0.9rem;
  }
  .none {
    margin: 0.2rem 0;
    color: var(--faint);
    font-size: 0.85rem;
  }
  .backlinks {
    max-height: 30%;
    overflow-y: auto;
    padding: 0.8rem 1.4rem 1rem;
    border-top: 1px solid var(--rule);
    background: var(--paper);
  }
  .backlinks ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .backlinks button {
    all: unset;
    cursor: pointer;
    display: flex;
    gap: 0.8rem;
    width: 100%;
    padding: 0.25rem 0;
    font-size: 0.85rem;
  }
  .backlinks button:hover .ctx {
    color: var(--ink);
  }
  .from {
    flex: none;
    font-weight: 700;
    color: var(--verdigris);
  }
  .ctx {
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .error {
    color: var(--signal);
  }
</style>
