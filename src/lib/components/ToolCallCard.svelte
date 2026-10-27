<script lang="ts">
  import type { ToolCallUpdate } from "$lib/api";

  let { call }: { call: ToolCallUpdate } = $props();
  let open = $state(false);

  const icons: Record<string, string> = {
    read: "◱", edit: "✎", delete: "⌫", move: "⇄", search: "⌕",
    execute: "❯", think: "◌", fetch: "⇣", switch_mode: "⇆", other: "•",
  };
  const status = $derived(call.status ?? "pending");
  const diffs = $derived((call.content ?? []).filter((c) => c.type === "diff"));
  const texts = $derived(
    (call.content ?? []).filter((c) => c.type === "content" && c.content?.type === "text").map((c) => c.content!.text!),
  );
  const hasDetail = $derived(diffs.length > 0 || texts.length > 0 || call.rawInput != null);

  function diffLines(oldText: string | null | undefined, newText: string | undefined) {
    // Minimal line diff: common prefix/suffix, then removed/added block.
    const a = (oldText ?? "").split("\n");
    const b = (newText ?? "").split("\n");
    let start = 0;
    while (start < a.length && start < b.length && a[start] === b[start]) start++;
    let endA = a.length, endB = b.length;
    while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) { endA--; endB--; }
    const ctx = (lines: string[]) => lines.map((l) => ({ t: " ", l }));
    return [
      ...ctx(a.slice(Math.max(0, start - 2), start)),
      ...a.slice(start, endA).map((l) => ({ t: "-", l })),
      ...b.slice(start, endB).map((l) => ({ t: "+", l })),
      ...ctx(a.slice(endA, endA + 2)),
    ];
  }
</script>

<div class="tool" data-status={status}>
  <button class="head" onclick={() => (open = !open)} disabled={!hasDetail}>
    <span class="icon">{icons[call.kind ?? "other"] ?? "•"}</span>
    <span class="title">{call.title ?? call.toolCallId}</span>
    <span class="status">{status.replace("_", " ")}</span>
  </button>
  {#if open}
    <div class="body">
      {#each diffs as d}
        <div class="path">{d.path}</div>
        <pre class="diff">{#each diffLines(d.oldText, d.newText) as line}<span class="l{line.t === '+' ? ' add' : line.t === '-' ? ' del' : ''}">{line.t} {line.l}
</span>{/each}</pre>
      {/each}
      {#each texts as t}
        <pre>{t}</pre>
      {/each}
      {#if diffs.length === 0 && texts.length === 0 && call.rawInput != null}
        <pre>{JSON.stringify(call.rawInput, null, 2)}</pre>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tool {
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 0.85em;
    background: var(--bg-raised);
  }
  .head {
    all: unset;
    box-sizing: border-box;
    display: flex;
    gap: 0.5em;
    align-items: center;
    width: 100%;
    padding: 0.35em 0.6em;
    cursor: pointer;
  }
  .head:disabled { cursor: default; }
  .icon { font-family: var(--font-mono); color: var(--fg-muted); width: 1em; text-align: center; }
  .title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono); }
  .status { color: var(--fg-muted); font-size: 0.85em; }
  [data-status="in_progress"] .status { color: var(--accent); }
  [data-status="failed"] .status { color: var(--danger); }
  [data-status="completed"] .status { color: var(--ok); }
  .body { border-top: 1px solid var(--border); padding: 0.4em 0.6em; max-height: 320px; overflow: auto; }
  .path { font-family: var(--font-mono); color: var(--fg-muted); margin-bottom: 0.2em; }
  pre { margin: 0 0 0.4em; font-family: var(--font-mono); font-size: 0.92em; white-space: pre-wrap; }
  .diff .l { display: block; }
  .diff .add { background: color-mix(in srgb, var(--ok) 18%, transparent); }
  .diff .del { background: color-mix(in srgb, var(--danger) 18%, transparent); }
</style>
