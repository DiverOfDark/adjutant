<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, type VaultInfo } from "$lib/api";
  import ChatPanel from "$lib/components/ChatPanel.svelte";

  let vault = $state<VaultInfo | null>(null);
  let error = $state<string | null>(null);

  async function openVault(path: string) {
    try {
      vault = await api.vaultOpen(path);
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function pickVault() {
    const dir = await open({ directory: true, title: "Open a folder as vault" });
    if (typeof dir === "string") await openVault(dir);
  }

  onMount(async () => {
    const last = await api.vaultLast();
    if (last) await openVault(last);
  });
</script>

{#if !vault}
  <main class="welcome">
    <h1>Adjutant</h1>
    <p>Open a folder of Markdown notes. It becomes your vault: notes, tasks and an agent that knows them.</p>
    <button class="primary" onclick={pickVault}>Open vault…</button>
    {#if error}<p class="error">{error}</p>{/if}
  </main>
{:else}
  <main class="app">
    <aside class="side">
      <div class="vault" title={vault.path}>{vault.name}</div>
      <button class="link" onclick={pickVault}>switch vault</button>
    </aside>
    <section class="center">
      <div class="placeholder">Editor arrives in Phase 1.</div>
    </section>
    <aside class="right">
      <ChatPanel vaultPath={vault.path} />
    </aside>
  </main>
{/if}

<style>
  .welcome {
    height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 0.8em;
    text-align: center; padding: 2em;
  }
  .welcome h1 { margin: 0; font-weight: 600; letter-spacing: 0.02em; color: var(--accent); }
  .welcome p { color: var(--fg-muted); max-width: 32em; margin: 0; }
  .error { color: var(--danger) !important; }
  .app { display: grid; grid-template-columns: 240px 1fr 420px; height: 100vh; }
  .side { border-right: 1px solid var(--border); padding: 0.7em; display: flex; flex-direction: column; gap: 0.3em; }
  .vault { font-weight: 600; }
  .center { display: flex; align-items: center; justify-content: center; color: var(--fg-muted); }
  .right { border-left: 1px solid var(--border); min-height: 0; overflow: hidden; }
</style>
