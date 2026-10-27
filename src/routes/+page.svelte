<script lang="ts">
  import { onMount } from "svelte";
  import { api, inTauri, onJiraSynced, onScheduleRun, onVaultChanged } from "$lib/api";
  import { app } from "$lib/app.svelte";
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

  let error = $state<string | null>(null);
  let loading = $state(true);
  let needsSetup = $state(false);

  async function open(path: string) {
    try {
      await app.openVault(path);
      needsSetup = !app.vault?.initialized;
      error = null;
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

  async function setup() {
    const created = await api.vaultScaffold();
    needsSetup = false;
    await app.refresh();
    app.notify(`Set up the vault: ${created.length} files and folders added`);
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
      <div class="mark">A</div>
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
      {#if needsSetup}
        <div class="setup">
          <p><strong>This folder isn't set up as a vault yet.</strong> Add the task list, folders, agent guidelines and default schedules? Existing files are left alone.</p>
          <button class="primary" onclick={setup}>Set up vault</button>
          <button class="quiet" onclick={() => (needsSetup = false)}>Not now</button>
        </div>
      {/if}
      {#key app.view.kind === "note" ? "note" : app.view.kind}
        {#if app.view.kind === "today"}<TodayView />
        {:else if app.view.kind === "note"}<NoteView path={app.view.path} line={app.view.line} />
        {:else if app.view.kind === "tasks"}<TasksView />
        {:else if app.view.kind === "jira"}<JiraView />
        {:else if app.view.kind === "schedules"}<SchedulesView />
        {:else if app.view.kind === "settings"}<SettingsView />
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
  <div class="toast" class:bad={app.toast.kind === "error"} role="status">{app.toast.text}</div>
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
    width: 52px;
    height: 52px;
    border-radius: 15px;
    display: grid;
    place-items: center;
    background: var(--ink);
    color: var(--paper);
    font-weight: 800;
    font-size: 1.7rem;
    letter-spacing: -0.04em;
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
  .center > :global(*:not(.setup)) {
    flex: 1;
    min-height: 0;
  }
  .agentpane {
    min-height: 0;
    border-left: 1px solid var(--rule);
  }
  .setup {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.7rem 1.4rem;
    background: var(--brass-wash);
    font-size: 0.9rem;
  }
  .setup p {
    flex: 1;
    margin: 0;
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
