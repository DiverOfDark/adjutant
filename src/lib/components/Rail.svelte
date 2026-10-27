<script lang="ts">
  import { Sun, FolderClosed, Search, Users, ListChecks, Ticket, CalendarClock, Settings2, Bot } from "@lucide/svelte";
  import { app, type SidebarMode, type View } from "$lib/app.svelte";

  const sidebarItems: { mode: SidebarMode; label: string; icon: typeof Sun }[] = [
    { mode: "files", label: "Files", icon: FolderClosed },
    { mode: "search", label: "Search (Ctrl+Shift+F)", icon: Search },
    { mode: "people", label: "People", icon: Users },
  ];
  const viewItems: { kind: View["kind"]; label: string; icon: typeof Sun }[] = [
    { kind: "today", label: "Today", icon: Sun },
    { kind: "tasks", label: "Tasks", icon: ListChecks },
    { kind: "jira", label: "Jira", icon: Ticket },
    { kind: "schedules", label: "Schedules", icon: CalendarClock },
  ];
  const lateCount = $derived(app.openTasks.filter((t) => t.due && t.due < new Date().toISOString().slice(0, 10)).length);
</script>

<nav class="rail" aria-label="Main">
  <div class="mark" aria-hidden="true">A</div>
  <div class="group">
    {#each viewItems as item}
      <button
        class="item"
        class:active={app.view.kind === item.kind}
        title={item.label}
        aria-label={item.label}
        onclick={() => app.go({ kind: item.kind } as View)}
      >
        <item.icon size={19} strokeWidth={1.8} />
        {#if item.kind === "tasks" && lateCount > 0}<span class="dot" aria-label="{lateCount} late"></span>{/if}
      </button>
    {/each}
  </div>
  <div class="sep"></div>
  <div class="group">
    {#each sidebarItems as item}
      <button
        class="item"
        class:on={app.sidebar === item.mode}
        title={item.label}
        aria-label={item.label}
        onclick={() => (app.sidebar = item.mode)}
      >
        <item.icon size={19} strokeWidth={1.8} />
      </button>
    {/each}
  </div>
  <div class="spacer"></div>
  <button class="item" class:on={app.agentOpen} title="Agent panel (Ctrl+J)" aria-label="Toggle agent panel" onclick={() => (app.agentOpen = !app.agentOpen)}>
    <Bot size={19} strokeWidth={1.8} />
  </button>
  <button class="item" class:active={app.view.kind === "settings"} title="Settings" aria-label="Settings" onclick={() => app.go({ kind: "settings" })}>
    <Settings2 size={19} strokeWidth={1.8} />
  </button>
</nav>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    padding: 0.7rem 0 0.6rem;
    width: 52px;
    border-right: 1px solid var(--rule);
  }
  .mark {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: 9px;
    background: var(--ink);
    color: var(--paper);
    font-weight: 800;
    font-size: 1rem;
    letter-spacing: -0.03em;
    margin-bottom: 0.6rem;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .sep {
    width: 20px;
    height: 1px;
    background: var(--rule-strong);
    margin: 0.45rem 0;
  }
  .spacer {
    flex: 1;
  }
  .item {
    position: relative;
    width: 36px;
    height: 36px;
    padding: 0;
    display: grid;
    place-items: center;
    border: none;
    border-radius: 9px;
    background: transparent;
    color: var(--muted);
  }
  .item:hover:not(:disabled) {
    color: var(--ink);
    background: var(--sunken);
  }
  .item.on {
    color: var(--ink);
  }
  .item.active {
    background: var(--verdigris);
    color: var(--verdigris-ink);
  }
  .dot {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--signal);
    box-shadow: 0 0 0 2px var(--paper);
  }
</style>
