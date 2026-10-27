<script lang="ts">
  import { app, relTime } from "$lib/app.svelte";
  import { chat } from "$lib/chat.svelte";

  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now()), 30000);
    return () => clearInterval(t);
  });
  const synced = $derived.by(() => {
    void now;
    return app.jira ? relTime(app.jira.syncedAt) : null;
  });
</script>

<footer class="status">
  <span class="agent" class:live={!!chat.agentName}>
    <span class="led"></span>{chat.agentName ?? "No agent"}{chat.busy ? ", working" : ""}
  </span>
  <span class="spacer"></span>
  {#if synced}<button class="link" onclick={() => app.go({ kind: "jira" })}>Jira synced {synced}</button>{/if}
  <span>{app.openTasks.length} open tasks</span>
  <span class="kbd"><kbd>Ctrl</kbd> <kbd>K</kbd> commands</span>
</footer>

<style>
  .status {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 1.1rem;
    padding: 0 0.9rem;
    height: 26px;
    border-top: 1px solid var(--rule);
    font-size: 0.75rem;
    color: var(--muted);
  }
  .agent {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
  }
  .led {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--faint);
  }
  .live .led {
    background: var(--verdigris);
    box-shadow: 0 0 0 3px var(--verdigris-wash);
  }
  .spacer {
    flex: 1;
  }
  .link {
    all: unset;
    cursor: pointer;
  }
  .link:hover {
    color: var(--ink);
  }
  kbd {
    font-size: 0.68rem;
  }
</style>
