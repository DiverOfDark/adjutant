<script lang="ts">
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { app, personName } from "$lib/app.svelte";

  let { text }: { text: string } = $props();

  // [[target|alias]] -> link the click handler resolves inside the vault.
  const withLinks = $derived(
    text.replace(/\[\[([^\]|#\n]+)(?:#[^\]|\n]*)?(?:\|([^\]\n]*))?\]\]/g, (_, target: string, alias?: string) => {
      const t = target.trim();
      const label = alias ?? (t.startsWith("people/") ? personName(t) : t.split("/").pop());
      return `[${label}](#wiki:${encodeURIComponent(t)})`;
    }),
  );
  const html = $derived(DOMPurify.sanitize(marked.parse(withLinks, { async: false, gfm: true }) as string));

  async function onclick(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest("a");
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute("href") ?? "";
    if (href.startsWith("#wiki:")) return app.followLink(decodeURIComponent(href.slice(6)));
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(href);
    } catch {
      window.open(href, "_blank");
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="md" {onclick}>{@html html}</div>

<style>
  .md :global(p) {
    margin: 0 0 0.65em;
  }
  .md :global(:last-child) {
    margin-bottom: 0;
  }
  .md :global(h1),
  .md :global(h2),
  .md :global(h3) {
    font-size: 1em;
    margin: 0.9em 0 0.35em;
  }
  .md :global(pre) {
    background: var(--sunken);
    padding: 0.6em 0.8em;
    border-radius: var(--radius-m);
    overflow-x: auto;
    font-size: 0.85em;
  }
  .md :global(code) {
    font-family: var(--font-mono);
    font-size: 0.86em;
  }
  .md :global(:not(pre) > code) {
    background: var(--sunken);
    padding: 0.05em 0.3em;
    border-radius: 4px;
  }
  .md :global(ul),
  .md :global(ol) {
    margin: 0 0 0.65em;
    padding-left: 1.3em;
  }
  .md :global(li) {
    margin: 0.15em 0;
  }
  .md :global(table) {
    border-collapse: collapse;
    margin: 0.4em 0;
    font-size: 0.9em;
  }
  .md :global(th),
  .md :global(td) {
    border-bottom: 1px solid var(--rule);
    padding: 0.25em 0.6em 0.25em 0;
    text-align: left;
  }
  .md :global(a) {
    color: var(--verdigris);
    font-weight: 600;
    text-decoration: none;
  }
  .md :global(a:hover) {
    text-decoration: underline;
  }
  .md :global(blockquote) {
    margin: 0 0 0.65em;
    padding-left: 0.8em;
    border-left: 3px solid var(--rule-strong);
    color: var(--ink-2);
  }
</style>
