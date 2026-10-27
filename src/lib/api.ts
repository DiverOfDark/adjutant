// Typed wrappers around Tauri commands and events.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface VaultInfo {
  path: string;
  name: string;
}

export interface AgentEntry {
  id: string;
  name: string;
  command: string;
  args: string[];
  useMise: boolean;
  installed: boolean;
  version: string | null;
  miseLine: string | null;
  builtin: boolean;
}

export interface Discovery {
  agents: AgentEntry[];
  defaultAgent: string | null;
  miseBin: string | null;
  miseError: string | null;
  miseUntrusted: boolean;
}

export interface AgentInfo {
  agentInfo: { name?: string; title?: string; version?: string } | null;
  agentCapabilities: Record<string, unknown>;
  protocolVersion: unknown;
}

export type PromptPart = { type: "text"; text: string } | { type: "file"; path: string };

/** ACP permission option. */
export interface PermissionOption {
  optionId: string;
  name: string;
  kind: "allow_once" | "allow_always" | "reject_once" | "reject_always";
}

// Mirrors adjutant_acp::AgentEvent (tag = "type", camelCase).
export type AgentEvent = { agentId: string } & (
  | { type: "ready"; agentInfo: unknown; agentCapabilities: unknown }
  | { type: "sessionUpdate"; sessionId: string; update: SessionUpdate }
  | {
      type: "permissionRequest";
      requestId: number;
      sessionId: string;
      toolCall: ToolCallUpdate;
      options: PermissionOption[];
    }
  | { type: "permissionDecided"; requestId: number; optionId: string | null; automatic: boolean }
  | { type: "fileRead"; sessionId: string; path: string }
  | { type: "fileWritten"; sessionId: string; path: string }
  | { type: "fileAccessDenied"; sessionId: string; path: string; reason: string }
  | { type: "terminalStarted"; sessionId: string; terminalId: string; command: string }
  | { type: "promptFinished"; sessionId: string; stopReason: string }
  | { type: "stderr"; line: string }
  | { type: "error"; message: string }
  | { type: "exited"; code: number | null }
);

export interface ContentBlock {
  type: string;
  text?: string;
  uri?: string;
  name?: string;
}

export interface ToolCallContent {
  type: "content" | "diff" | "terminal";
  content?: ContentBlock;
  path?: string;
  oldText?: string | null;
  newText?: string;
  terminalId?: string;
}

export interface ToolCallUpdate {
  toolCallId: string;
  title?: string;
  kind?: string;
  status?: "pending" | "in_progress" | "completed" | "failed";
  content?: ToolCallContent[];
  locations?: { path: string; line?: number }[];
  rawInput?: unknown;
}

export interface PlanEntry {
  content: string;
  priority: "high" | "medium" | "low";
  status: "pending" | "in_progress" | "completed";
}

export type SessionUpdate =
  | { sessionUpdate: "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk"; content: ContentBlock }
  | ({ sessionUpdate: "tool_call" } & ToolCallUpdate)
  | ({ sessionUpdate: "tool_call_update" } & ToolCallUpdate)
  | { sessionUpdate: "plan"; entries: PlanEntry[] }
  | { sessionUpdate: string; [key: string]: unknown };

export const api = {
  vaultOpen: (path: string) => invoke<VaultInfo>("vault_open", { path }),
  vaultLast: () => invoke<string | null>("vault_last"),
  agentsDiscover: () => invoke<Discovery>("agents_discover"),
  agentsTrustVault: () => invoke<Discovery>("agents_trust_vault"),
  agentConnect: (agentId: string | null) => invoke<AgentInfo>("agent_connect", { agentId }),
  agentDisconnect: () => invoke<void>("agent_disconnect"),
  chatNewSession: () => invoke<string>("chat_new_session"),
  chatPrompt: (sessionId: string, parts: PromptPart[]) =>
    invoke<string>("chat_prompt", { sessionId, parts }),
  chatCancel: (sessionId: string) => invoke<void>("chat_cancel", { sessionId }),
  permissionRespond: (requestId: number, optionId: string | null, alwaysToolKind?: string) =>
    invoke<void>("permission_respond", { requestId, optionId, alwaysToolKind }),
};

export function onAgentEvent(cb: (e: AgentEvent) => void): Promise<UnlistenFn> {
  return listen<AgentEvent>("agent-event", (e) => cb(e.payload));
}
