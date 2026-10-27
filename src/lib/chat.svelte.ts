// Chat state: turns ACP session updates into a list of renderable items.
import {
  api,
  type AgentEvent,
  type PermissionOption,
  type PlanEntry,
  type PromptPart,
  type ToolCallUpdate,
} from "./api";
import { app } from "./app.svelte";

export type ChatItem =
  | { kind: "user"; id: string; text: string }
  | { kind: "agent"; id: string; text: string }
  | { kind: "thought"; id: string; text: string }
  | { kind: "tool"; id: string; call: ToolCallUpdate }
  | { kind: "plan"; id: string; entries: PlanEntry[] }
  | {
      kind: "permission";
      id: string;
      requestId: number;
      toolCall: ToolCallUpdate;
      options: PermissionOption[];
      decided: string | null | undefined;
    }
  | { kind: "notice"; id: string; text: string; level: "info" | "error" };

let seq = 0;
const nextId = () => `i${++seq}`;

export class ChatState {
  items = $state<ChatItem[]>([]);
  sessionId = $state<string | null>(null);
  busy = $state(false);
  agentId = $state<string | null>(null);
  agentName = $state<string | null>(null);
  connecting = $state(false);
  stderrTail = $state<string[]>([]);

  async connect(agentId: string | null) {
    this.connecting = true;
    this.items = [];
    this.sessionId = null;
    try {
      const info = await api.agentConnect(agentId);
      this.agentName = info.agentInfo?.title ?? info.agentInfo?.name ?? agentId;
      this.agentId = agentId ?? (await api.agentsDiscover()).defaultAgent;
      this.sessionId = await api.chatNewSession();
    } catch (e) {
      this.notice(String(e), "error");
      this.agentName = null;
    } finally {
      this.connecting = false;
    }
  }

  /** Opens the agent panel, starts the agent if needed and sends `text`. */
  async ask(text: string, files: string[] = [], label?: string) {
    app.agentOpen = true;
    if (!this.sessionId) await this.connect(this.agentId);
    if (this.sessionId) await this.send(text, files, label);
  }

  private async persist() {
    if (!this.sessionId) return;
    const first = this.items.find((i) => i.kind === "user");
    await api
      .sessionSave({
        id: this.sessionId,
        agentId: this.agentId ?? "",
        title: first && first.kind === "user" ? first.text.slice(0, 80) : "Untitled",
        updatedAt: new Date().toISOString(),
        items: $state.snapshot(this.items),
      })
      .catch(() => {});
  }

  /** Shows a saved conversation. Sending starts a new session. */
  showSaved(items: ChatItem[]) {
    this.items = items;
    this.sessionId = null;
  }

  async newSession() {
    this.items = [];
    this.sessionId = await api.chatNewSession();
  }

  /** `label` is what the conversation shows instead of a long canned prompt. */
  async send(text: string, files: string[] = [], label?: string) {
    if (!this.sessionId || this.busy || !text.trim()) return;
    const parts: PromptPart[] = [{ type: "text", text }, ...files.map((path) => ({ type: "file" as const, path }))];
    this.items.push({ kind: "user", id: nextId(), text: label ?? text });
    this.busy = true;
    try {
      const stop = await api.chatPrompt(this.sessionId, parts);
      if (stop !== "end_turn") this.notice(`Turn ended: ${stop}`, "info");
    } catch (e) {
      this.notice(String(e), "error");
    } finally {
      this.busy = false;
      this.persist();
      app.refreshTasks();
    }
  }

  cancel() {
    if (this.sessionId) api.chatCancel(this.sessionId);
  }

  async answerPermission(requestId: number, option: PermissionOption | null, rememberKind?: string) {
    const item = this.items.find((i) => i.kind === "permission" && i.requestId === requestId);
    if (item && item.kind === "permission") item.decided = option?.optionId ?? null;
    await api.permissionRespond(requestId, option?.optionId ?? null, rememberKind);
  }

  notice(text: string, level: "info" | "error") {
    this.items.push({ kind: "notice", id: nextId(), text, level });
  }

  handle(e: AgentEvent) {
    switch (e.type) {
      case "sessionUpdate":
        if (e.sessionId === this.sessionId) this.applyUpdate(e.update as any);
        break;
      case "permissionRequest":
        if (e.sessionId !== this.sessionId) break;
        this.items.push({
          kind: "permission",
          id: nextId(),
          requestId: e.requestId,
          toolCall: e.toolCall,
          options: e.options,
          decided: undefined,
        });
        break;
      case "permissionDecided": {
        const item = this.items.find((i) => i.kind === "permission" && i.requestId === e.requestId);
        if (item && item.kind === "permission") item.decided = e.optionId;
        break;
      }
      case "fileAccessDenied":
        this.notice(`Blocked file access outside the vault: ${e.path}`, "error");
        break;
      case "stderr":
        this.stderrTail = [...this.stderrTail.slice(-49), e.line];
        break;
      case "error":
        this.notice(e.message, "error");
        break;
      case "exited":
        this.notice(`Agent exited${e.code != null ? ` (code ${e.code})` : ""}`, "info");
        this.sessionId = null;
        this.agentName = null;
        this.busy = false;
        break;
    }
  }

  private applyUpdate(u: any) {
    switch (u.sessionUpdate) {
      case "agent_message_chunk":
        this.appendText("agent", u.content);
        break;
      case "agent_thought_chunk":
        this.appendText("thought", u.content);
        break;
      case "user_message_chunk":
        break;
      case "tool_call":
      case "tool_call_update": {
        const { sessionUpdate, ...fields } = u;
        const clean = Object.fromEntries(Object.entries(fields).filter(([, v]) => v != null)) as unknown as ToolCallUpdate;
        const existing = this.findTool(clean.toolCallId);
        if (existing) Object.assign(existing.call, clean);
        else this.items.push({ kind: "tool", id: `tool-${clean.toolCallId}`, call: clean });
        break;
      }
      case "plan": {
        // One plan per turn, updated in place.
        const lastUser = this.items.findLastIndex((i) => i.kind === "user");
        const plan = this.items.findLast((i, idx) => i.kind === "plan" && idx > lastUser);
        if (plan && plan.kind === "plan") plan.entries = u.entries;
        else this.items.push({ kind: "plan", id: nextId(), entries: u.entries });
        break;
      }
    }
  }

  private findTool(id: string) {
    return this.items.find((i) => i.kind === "tool" && i.call.toolCallId === id) as
      | Extract<ChatItem, { kind: "tool" }>
      | undefined;
  }

  private appendText(kind: "agent" | "thought", content: { type: string; text?: string }) {
    if (content?.type !== "text" || !content.text) return;
    const last = this.items.at(-1);
    if (last && last.kind === kind) last.text += content.text;
    else this.items.push({ kind, id: nextId(), text: content.text });
  }
}

export const chat = new ChatState();
