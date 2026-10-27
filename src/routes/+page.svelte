<script lang="ts">
  import { onMount } from "svelte";
  import { api, inTauri, onJiraSynced, onScheduleRun, onVaultChanged } from "$lib/api";
  import { app } from "$lib/app.svelte";
  import { updater } from "$lib/updater.svelte";
  import Rail from "$lib/components/Rail.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ChatPanel from "$lib/components/ChatPanel.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import TodayView from "$lib/views/TodayView.svelte";
  import NoteView from "$lib/views/NoteView.svelte";
  import TasksView from "$lib/views/TasksView.svelte";
  import JiraView from "$lib/views/JiraView.svelte";
  import SchedulesView from "$lib/views/SchedulesView.svelte";
  import SettingsView from "$lib/views/SettingsView.svelte";
  import SetupView from "$lib/views/SetupView.svelte";

  let error = $state<string | null>(null);
  let loading = $state(true);

  async function open(path: string) {
    try {
      await app.openVault(path);
      error = null;
      const status = await api.setupStatus().catch(() => null);
      if (status && !status.done) app.go({ kind: "setup" });
    } catch (e) {
      error = String(e);
    }
  }

  async function pick() {
    if (!inTauri) return open("/home/demo/work-vault");
    const { open: dialog } = await import("@tauri-apps/plugin-dialog");
    const dir = await dialog({ directory: true, title: "Choose a folder for your vault" });
    if (typeof dir === "string") await open(dir);
  }

  function onKey(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "k") {
      e.preventDefault();
      app.paletteOpen = !app.paletteOpen;
    } else if (mod && e.key.toLowerCase() === "p") {
      e.preventDefault();
      app.paletteOpen = true;
    } else if (mod && e.key.toLowerCase() === "j") {
      e.preventDefault();
      app.agentOpen = !app.agentOpen;
    } else if (mod && e.shiftKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      app.sidebar = "search";
    } else if (mod && e.key === "1") {
      e.preventDefault();
      app.go({ kind: "today" });
    } else if (e.altKey && e.key === "ArrowLeft") {
      app.back();
    }
  }

  onMount(() => {
    updater.start();
    (async () => {
      const last = await api.vaultLast();
      if (last) await open(last);
      loading = false;
    })();
    let timer: ReturnType<typeof setTimeout>;
    const subs = [
      onVaultChanged(() => {
        clearTimeout(timer);
        timer = setTimeout(async () => {
          if (!app.vault) return;
          await app.refresh();
          app.revision++;
        }, 250);
      }),
      onJiraSynced((cache) => (app.jira = cache)),
      onScheduleRun(({ record }) => {
        if (record.status === "ok" && record.output) app.notify(`${record.message ?? "Job finished"}: ${record.output}`);
        if (record.status === "failed") app.notify(`Scheduled job failed: ${record.message}`, "error");
      }),
    ];
    return () => subs.forEach((s) => s.then((f) => f()));
  });
</script>

<svelte:window onkeydown={onKey} />

{#if loading}
  <div class="loading"></div>
{:else if !app.vault}
  <main class="welcome">
    <div class="card">
      <img class="mark" src="/icon.svg" alt="" />
      <h1>Adjutant</h1>
      <p>Pick a folder of Markdown notes, or an empty folder to start one. Your notes stay plain files on this machine.</p>
      <button class="primary big" onclick={pick}>Choose a folder</button>
      {#if error}<p class="error">{error}</p>{/if}
    </div>
  </main>
{:else}
  <div class="shell" class:agent={app.agentOpen}>
    <Rail />
    <Sidebar />
    <main class="center">
      {#key app.view.kind === "note" ? "note" : app.view.kind}
        {#if app.view.kind === "today"}<TodayView />
        {:else if app.view.kind === "note"}<NoteView path={app.view.path} line={app.view.line} />
        {:else if app.view.kind === "tasks"}<TasksView />
        {:else if app.view.kind === "jira"}<JiraView />
        {:else if app.view.kind === "schedules"}<SchedulesView />
        {:else if app.view.kind === "settings"}<SettingsView />
        {:else if app.view.kind === "setup"}<SetupView />
        {/if}
      {/key}
    </main>
    {#if app.agentOpen}
      <aside class="agentpane"><ChatPanel /></aside>
    {/if}
    <StatusBar />
  </div>
  <CommandPalette />
{/if}

{#if app.toast}
  {@const toast = app.toast}
  <div
    class="toast"
    class:bad={toast.kind === "error"}
    role="status"
    onpointerenter={() => app.holdToast()}
    onpointerleave={() => app.releaseToast()}
  >
    <span>{toast.text}</span>
    {#if toast.action}
      <button
        class="toast-action"
        onclick={() => {
          const run = toast.action?.run;
          app.toast = null;
          run?.();
        }}>{toast.action.label}</button
      >
    {/if}
  </div>
{/if}

<style>
  .loading {
    height: 100vh;
  }
  .welcome {
    height: 100vh;
    display: grid;
    place-items: center;
    padding: 2rem;
  }
  .card {
    max-width: 30rem;
  }
  .mark {
    width: 72px;
    height: 72px;
    margin-left: -6px;
  }
  .welcome h1 {
    font-size: var(--step-4);
    font-weight: 800;
    letter-spacing: -0.04em;
    line-height: 1;
    margin: 1.4rem 0 0.8rem;
  }
  .welcome p {
    color: var(--ink-2);
    font-size: var(--step-1);
    margin: 0 0 1.6rem;
  }
  .big {
    font-size: var(--step-0);
    padding: 0.65rem 1.3rem;
  }
  .error {
    color: var(--signal) !important;
    font-size: 0.9rem !important;
    margin-top: 1rem !important;
  }
  .shell {
    display: grid;
    grid-template-columns: 52px 248px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr) auto;
    height: 100vh;
  }
  .shell.agent {
    grid-template-columns: 52px 248px minmax(0, 1fr) minmax(320px, 400px);
  }
  @media (max-width: 1100px) {
    .shell {
      grid-template-columns: 52px 210px minmax(0, 1fr);
    }
    .shell.agent {
      grid-template-columns: 52px 0 minmax(0, 1fr) 340px;
    }
    .shell.agent :global(.sidebar) {
      display: none;
    }
  }
  .center {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--sheet);
  }
  .center > :global(*) {
    flex: 1;
    min-height: 0;
  }
  .agentpane {
    min-height: 0;
    border-left: 1px solid var(--rule);
  }
  .toast {
    position: fixed;
    bottom: 40px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 100;
    max-width: min(40rem, 90vw);
    padding: 0.6rem 1rem;
    border-radius: var(--radius-m);
    background: var(--ink);
    color: var(--paper);
    font-size: 0.875rem;
    box-shadow: var(--shadow-pop);
    animation: rise 0.18s ease-out;
    display: flex;
    align-items: center;
    gap: 0.9rem;
  }
  .toast-action {
    flex: none;
    padding: 0.15rem 0.55rem;
    border: 1px solid color-mix(in srgb, var(--paper) 35%, transparent);
    border-radius: var(--radius-s);
    background: transparent;
    color: inherit;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .toast-action:hover {
    background: color-mix(in srgb, var(--paper) 15%, transparent);
  }
  .toast.bad {
    background: var(--signal);
    color: #fff;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, 6px);
    }
  }
</style>
