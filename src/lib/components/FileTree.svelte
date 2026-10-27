<script lang="ts">
  import { ChevronRight, FileText, Folder, FolderOpen } from "@lucide/svelte";
  import type { TreeNode } from "$lib/api";
  import { app } from "$lib/app.svelte";
  import FileTree from "./FileTree.svelte";

  let { node, depth = 0, oncontext }: { node: TreeNode; depth?: number; oncontext?: (e: MouseEvent, n: TreeNode) => void } = $props();

  const expanded = $state<Record<string, boolean>>(
    typeof localStorage !== "undefined" ? JSON.parse(localStorage.getItem("tree-open") ?? "{}") : {},
  );
  function toggle(path: string) {
    expanded[path] = !expanded[path];
    localStorage.setItem("tree-open", JSON.stringify(expanded));
  }
  const current = $derived(app.view.kind === "note" ? app.view.path : null);
  const inboxCount = (n: TreeNode) => (n.path === "inbox" ? n.children.filter((c) => !c.isDir).length : 0);
</script>

{#each node.children as child (child.path)}
  {#if child.isDir}
    <button
      class="row dir"
      style="--depth: {depth}"
      onclick={() => toggle(child.path)}
      oncontextmenu={(e) => oncontext?.(e, child)}
      aria-expanded={!!expanded[child.path]}
    >
      <span class="chev" class:open={expanded[child.path]}><ChevronRight size={13} /></span>
      {#if expanded[child.path]}<FolderOpen size={15} />{:else}<Folder size={15} />{/if}
      <span class="name">{child.name}</span>
      {#if inboxCount(child) > 0}<span class="count">{inboxCount(child)}</span>{/if}
    </button>
    {#if expanded[child.path]}
      <FileTree node={child} depth={depth + 1} {oncontext} />
    {/if}
  {:else}
    <button
      class="row"
      class:current={current === child.path}
      style="--depth: {depth}"
      onclick={() => app.openNote(child.path)}
      oncontextmenu={(e) => oncontext?.(e, child)}
    >
      <span class="chev"></span>
      <FileText size={15} />
      <span class="name">{child.name.replace(/\.md$/, "")}</span>
    </button>
  {/if}
{/each}

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.22rem 0.5rem 0.22rem calc(0.4rem + var(--depth) * 0.95rem);
    border: none;
    border-radius: var(--radius-s);
    background: none;
    color: var(--ink-2);
    font-weight: 400;
    font-size: 0.875rem;
    text-align: left;
  }
  .row :global(svg) {
    flex: none;
    color: var(--faint);
  }
  .row:hover:not(:disabled) {
    background: var(--sunken);
  }
  .row.current {
    background: var(--verdigris-wash);
    color: var(--ink);
  }
  .row.current :global(svg) {
    color: var(--verdigris);
  }
  .dir {
    color: var(--ink);
  }
  .chev {
    width: 13px;
    display: inline-grid;
    transition: transform 0.12s;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 0.72rem;
    font-weight: 700;
    color: var(--brass);
    background: var(--brass-wash);
    padding: 0 0.4rem;
    border-radius: 99px;
  }
</style>
