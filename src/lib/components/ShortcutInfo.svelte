<script lang="ts">
  import { Copy } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { app } from "$lib/app.svelte";

  // What the backend knows about the quick-capture shortcut.
  let {
    label,
    managed,
    error,
    captureCommand = null,
  }: { label: string | null | undefined; managed: boolean | undefined; error: string | null | undefined; captureCommand?: string | null } = $props();

  async function change() {
    try {
      await api.captureShortcutConfigure();
    } catch (e) {
      app.notify(String(e), "error");
    }
  }
</script>

{#if label}
  <p>
    Press {#each label.split("+") as key, i}{#if i}&thinsp;{/if}<kbd>{key}</kbd>{/each} anywhere to add a task or a note.
    {#if managed}Your desktop owns this shortcut. <button class="link" onclick={change}>Change it…</button>{/if}
  </p>
{:else if error}
  <p class="bad">{error}</p>
  {#if captureCommand}
    <div class="cmd">
      <code>{captureCommand}</code>
      <button class="icon" aria-label="Copy" onclick={() => navigator.clipboard.writeText(captureCommand!).then(() => app.notify("Copied"))}><Copy size={14} /></button>
    </div>
  {/if}
{:else if managed}
  <p>Your desktop is asking you to confirm the shortcut; accept it in the dialog it shows.</p>
{:else}
  <p>No shortcut set.</p>
{/if}

<style>
  .bad {
    color: var(--signal);
  }
  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--verdigris);
    font-size: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .cmd {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.3rem 0.3rem 0.3rem 0.7rem;
    border-radius: var(--radius-m);
    background: var(--sunken);
  }
  code {
    font-family: var(--font-mono);
    font-size: 0.8125rem;
  }
</style>
