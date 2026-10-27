<script lang="ts">
  import type { PermissionOption, ToolCallUpdate } from "$lib/api";
  import { chat } from "$lib/chat.svelte";
  import ToolCallCard from "./ToolCallCard.svelte";

  let {
    requestId,
    toolCall,
    options,
    decided,
  }: { requestId: number; toolCall: ToolCallUpdate; options: PermissionOption[]; decided: string | null | undefined } =
    $props();

  const allowOnce = $derived(options.find((o) => o.kind === "allow_once"));
  const allowAlways = $derived(options.find((o) => o.kind === "allow_always") ?? allowOnce);
  const reject = $derived(options.find((o) => o.kind === "reject_once") ?? options.find((o) => o.kind === "reject_always"));
  const decidedLabel = $derived(
    decided === undefined ? null : decided === null ? "Cancelled" : (options.find((o) => o.optionId === decided)?.name ?? decided),
  );
  const kind = $derived(toolCall.kind ?? "other");
</script>

<div class="perm" class:done={decided !== undefined}>
  <div class="q">The agent wants to run:</div>
  <ToolCallCard call={toolCall} />
  {#if decided === undefined}
    <div class="actions">
      {#if allowOnce}
        <button class="primary" onclick={() => chat.answerPermission(requestId, allowOnce)}>Allow once</button>
      {/if}
      {#if allowAlways}
        <button
          title={`Always allow "${kind}" tools for this agent (saved in .vault/config.toml)`}
          onclick={() => chat.answerPermission(requestId, allowAlways, kind)}>Always allow {kind}</button
        >
      {/if}
      <button class="danger" onclick={() => chat.answerPermission(requestId, reject ?? null)}>Deny</button>
    </div>
  {:else}
    <div class="decided">→ {decidedLabel}</div>
  {/if}
</div>

<style>
  .perm {
    border: 1px solid var(--verdigris);
    border-radius: 8px;
    padding: 0.6em;
    display: flex;
    flex-direction: column;
    gap: 0.5em;
    background: color-mix(in srgb, var(--verdigris) 6%, var(--paper));
  }
  .perm.done { border-color: var(--rule); background: none; }
  .q { font-size: 0.85em; color: var(--muted); }
  .actions { display: flex; gap: 0.4em; flex-wrap: wrap; }
  .decided { font-size: 0.85em; color: var(--muted); }
</style>
