<script lang="ts">
  import { app } from "$lib/app.svelte";
  let { key }: { key: string } = $props();
  const issue = $derived(app.jira?.issues[key]);
  const href = $derived(issue?.url ?? (app.jira ? `${app.jira.baseUrl}/browse/${key}` : null));

  async function open(e: MouseEvent) {
    e.preventDefault();
    if (!href) return;
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(href);
    } catch {
      window.open(href, "_blank");
    }
  }
</script>

<a class="jira" data-cat={issue?.statusCategory ?? "unknown"} href={href ?? undefined} onclick={open} title={issue ? `${issue.summary}\n${issue.status}${issue.assignee ? `, ${issue.assignee}` : ""}` : "Not in the Jira snapshot"}>
  <span class="key">{key}</span>{#if issue}<span class="st">{issue.status}</span>{/if}
</a>

<style>
  .jira {
    display: inline-flex;
    align-items: baseline;
    gap: 0.35rem;
    padding: 0 0.4rem;
    border-radius: var(--radius-s);
    background: var(--sunken);
    color: var(--ink);
    text-decoration: none;
    font-size: 0.85em;
    line-height: 1.55;
    white-space: nowrap;
  }
  .key {
    font-weight: 700;
  }
  .st {
    color: var(--muted);
  }
  [data-cat="indeterminate"] .st {
    color: var(--verdigris);
  }
  [data-cat="done"] {
    opacity: 0.7;
  }
  [data-cat="done"] .key {
    text-decoration: line-through;
  }
  .jira:hover {
    background: var(--rule);
  }
</style>
