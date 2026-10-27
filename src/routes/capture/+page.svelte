<script lang="ts">
  import { onMount } from "svelte";
  import { api, SECTIONS } from "$lib/api";

  // Quick capture window: Enter adds a task, Ctrl+Enter files a note to inbox/.
  let text = $state("");
  let section = $state<string>("Now");
  let done = $state<string | null>(null);
  let input: HTMLTextAreaElement | undefined = $state();

  async function hide() {
    text = "";
    done = null;
    await api.captureHide();
  }

  async function save(asNote: boolean) {
    const value = text.trim();
    if (!value) return hide();
    try {
      if (asNote || value.includes("\n")) {
        const path = await api.captureNote(value);
        done = `Saved to ${path}`;
      } else {
        await api.taskAdd(section, value);
        done = `Added to ${section}`;
      }
      text = "";
      setTimeout(hide, 650);
    } catch (e) {
      done = String(e);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") hide();
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      save(e.ctrlKey || e.metaKey);
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const i = SECTIONS.indexOf(section as never);
      section = SECTIONS[(i + (e.shiftKey ? SECTIONS.length - 1 : 1)) % SECTIONS.length];
    }
  }

  onMount(() => {
    input?.focus();
    setTimeout(() => input?.focus(), 50);
    const f = () => setTimeout(() => input?.focus(), 30);
    window.addEventListener("focus", f);
    return () => window.removeEventListener("focus", f);
  });
</script>

<main class="capture" data-tauri-drag-region>
  <textarea
    bind:this={input}
    bind:value={text}
    onkeydown={onKey}
    rows="2"
    placeholder="Capture a task or a note"
    aria-label="Capture"
  ></textarea>
  <footer data-tauri-drag-region>
    {#if done}
      <span class="done">{done}</span>
    {:else}
      <span><kbd>Enter</kbd> task in <strong>{section}</strong> <kbd>Tab</kbd> changes section</span>
      <span><kbd>Ctrl</kbd>+<kbd>Enter</kbd> note to inbox</span>
    {/if}
  </footer>
</main>

<style>
  :global(html),
  :global(body) {
    background: transparent;
  }
  .capture {
    height: 100vh;
    display: flex;
    flex-direction: column;
    background: var(--sheet);
    border: 1px solid var(--rule-strong);
    border-radius: 12px;
    overflow: hidden;
  }
  textarea {
    flex: 1;
    border: none;
    border-radius: 0;
    background: transparent;
    resize: none;
    font-size: var(--step-1);
    padding: 0.9rem 1.1rem;
  }
  textarea:focus {
    box-shadow: none;
  }
  footer {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.45rem 1.1rem;
    border-top: 1px solid var(--rule);
    background: var(--paper);
    font-size: 0.75rem;
    color: var(--muted);
  }
  kbd {
    font-size: 0.68rem;
  }
  .done {
    color: var(--verdigris);
    font-weight: 700;
  }
</style>
