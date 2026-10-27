<script lang="ts">
  import { marked } from "marked";
  import DOMPurify from "dompurify";

  let { text }: { text: string } = $props();
  const html = $derived(DOMPurify.sanitize(marked.parse(text, { async: false, gfm: true, breaks: false }) as string));
</script>

<div class="md">{@html html}</div>

<style>
  .md :global(p) { margin: 0 0 0.6em; }
  .md :global(p:last-child) { margin-bottom: 0; }
  .md :global(pre) {
    background: var(--bg-sunken);
    padding: 0.6em 0.8em;
    border-radius: 6px;
    overflow-x: auto;
    font-size: 0.86em;
  }
  .md :global(code) { font-family: var(--font-mono); font-size: 0.92em; }
  .md :global(:not(pre) > code) { background: var(--bg-sunken); padding: 0.05em 0.3em; border-radius: 4px; }
  .md :global(ul), .md :global(ol) { margin: 0 0 0.6em; padding-left: 1.4em; }
  .md :global(table) { border-collapse: collapse; margin: 0.4em 0; }
  .md :global(th), .md :global(td) { border: 1px solid var(--border); padding: 0.2em 0.5em; }
  .md :global(a) { color: var(--accent); }
</style>
