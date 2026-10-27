<script lang="ts">
  import { dialog } from "$lib/dialog.svelte";

  let value = $state("");
  let touched = $state(false);

  const d = $derived(dialog.current);
  const error = $derived(d?.kind === "prompt" ? (d.validate?.(value) ?? null) : null);
  const preview = $derived(d?.kind === "prompt" && !(touched && error) ? (d.preview?.(value) ?? null) : null);

  // Reset whenever a new dialog opens.
  $effect.pre(() => {
    const c = dialog.current;
    if (!c) return;
    value = c.kind === "prompt" ? (c.value ?? "") : "";
    touched = false;
  });

  /** Focuses the input on mount and selects what the user most likely retypes. */
  function focusInput(el: HTMLInputElement) {
    const c = dialog.current;
    el.value = c?.kind === "prompt" ? (c.value ?? "") : "";
    el.focus();
    const v = el.value;
    if (v.endsWith("/")) el.setSelectionRange(v.length, v.length);
    else if (c?.kind === "prompt" && c.selectName) {
      const start = v.lastIndexOf("/") + 1;
      const dot = v.lastIndexOf(".");
      el.setSelectionRange(start, dot > start ? dot : v.length);
    } else el.select();
  }

  function focusButton(el: HTMLButtonElement) {
    if (dialog.current?.kind === "confirm") el.focus();
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    touched = true;
    if (error) return;
    dialog.submit(value.trim());
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      dialog.cancel();
    }
  }
</script>

{#if d}
  <div class="scrim" role="presentation" onclick={() => dialog.cancel()}></div>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title" tabindex="-1" {onkeydown}>
    <form onsubmit={submit}>
      <h2 id="dialog-title">{d.title}</h2>
      {#if d.kind === "prompt"}
        {#if d.label}<label class="label" for="dialog-input">{d.label}</label>{/if}
        <input
          id="dialog-input"
          use:focusInput
          bind:value
          oninput={() => (touched = true)}
          placeholder={d.placeholder}
          spellcheck="false"
          autocomplete="off"
          class:invalid={touched && error}
        />
        <p class="note" class:bad={touched && error}>
          {#if touched && error}{error}{:else if preview}{preview}{:else}&nbsp;{/if}
        </p>
      {:else if d.body}
        <p class="body">{d.body}</p>
      {/if}
      <div class="actions">
        <button type="button" class="quiet" onclick={() => dialog.cancel()}>Cancel</button>
        <button
          use:focusButton
          type="submit"
          class={d.kind === "confirm" && d.danger ? "danger" : "primary"}
          disabled={d.kind === "prompt" && !!error && touched}>{d.confirm ?? "OK"}</button
        >
      </div>
    </form>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgb(16 22 28 / 0.25);
    z-index: 90;
    animation: fade 0.12s ease-out;
  }
  .dialog {
    position: fixed;
    z-index: 91;
    top: 18vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(28rem, 92vw);
    padding: 1.1rem 1.25rem 1rem;
    background: var(--sheet);
    border-radius: 12px;
    box-shadow: var(--shadow-pop);
    animation: rise 0.16s ease-out;
  }
  h2 {
    margin: 0 0 0.75rem;
    font-size: var(--step-1);
    font-weight: 650;
  }
  .label {
    display: block;
    margin-bottom: 0.35rem;
    font-size: var(--step--1);
    color: var(--muted);
  }
  input {
    width: 100%;
    font-family: var(--font-mono);
    font-size: 0.875rem;
  }
  input.invalid {
    border-color: var(--signal);
  }
  .note {
    margin: 0.4rem 0 0;
    min-height: 1.2em;
    font-size: var(--step--1);
    color: var(--muted);
  }
  .note.bad {
    color: var(--signal);
  }
  .body {
    margin: 0;
    color: var(--ink-2);
    line-height: 1.5;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1rem;
  }
  .actions .danger {
    background: var(--signal);
    border-color: var(--signal);
    color: #fff;
  }
  .actions .danger:hover:not(:disabled) {
    filter: brightness(1.08);
    border-color: var(--signal);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translate(-50%, 6px);
    }
  }
</style>
