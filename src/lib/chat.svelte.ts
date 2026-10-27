// Chat state: turns ACP session updates into a list of renderable items.
import {
  api,
  type AgentEvent,
  type PermissionOption,
  type PlanEntry,
  type PromptPart,
  type ToolCallUpdate,
} from "./api";

export type ChatItem =
  | { kind: "user"; id: string; text: string }
  | { kind: "agent"; id: string; text: string }
  | { kind: "thought"; id: string; text: string }
  | ({ kind: "tool"; id: string } & ToolCallUpdate)
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
      this.agentId = agentId;
      this.sessionId = await api.chatNewSession();
    } catch (e) {
      this.notice(String(e), "error");
      this.agentName = null;
    } finally {
      this.connecting = false;
    }
  }

  async newSession() {
    this.items = [];
    this.sessionId = await api.chatNewSession();
  }

  async send(text: string, files: string[] = []) {
    if (!this.sessionId || this.busy || !text.trim()) return;
    const parts: PromptPart[] = [{ type: "text", text }, ...files.map((path) => ({ type: "file" as const, path }))];
    this.items.push({ kind: "user", id: nextId(), text });
    this.busy = true;
    try {
      const stop = await api.chatPrompt(this.sessionId, parts);
      if (stop !== "end_turn") this.notice(`Turn ended: ${stop}`, "info");
    } catch (e) {
      this.notice(String(e), "error");
    } finally {
      this.busy = false;
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
      case "tool_call": {
        const { sessionUpdate, ...call } = u;
        const existing = this.findTool(call.toolCallId);
        if (existing) Object.assign(existing, call);
        else this.items.push({ kind: "tool", id: call.toolCallId, ...call });
        break;
      }
      case "tool_call_update": {
        const { sessionUpdate, ...fields } = u;
        const existing = this.findTool(fields.toolCallId);
        const clean = Object.fromEntries(Object.entries(fields).filter(([, v]) => v != null));
        if (existing) Object.assign(existing, clean);
        else this.items.push({ kind: "tool", id: fields.toolCallId, ...clean } as any);
        break;
      }
      case "plan": {
        const plan = this.items.findLast((i) => i.kind === "plan");
        if (plan && plan.kind === "plan" && this.items.at(-1) === plan) plan.entries = u.entries;
        else this.items.push({ kind: "plan", id: nextId(), entries: u.entries });
        break;
      }
    }
  }

  private findTool(id: string) {
    return this.items.find((i) => i.kind === "tool" && i.id === id) as
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
