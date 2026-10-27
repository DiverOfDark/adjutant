// In-app replacements for window.prompt/confirm, rendered by Dialog.svelte.

export interface PromptOptions {
  title: string;
  label?: string;
  value?: string;
  placeholder?: string;
  confirm?: string;
  /** Select only the file name (without extension) instead of the whole value. */
  selectName?: boolean;
  /** A line under the input for the current value, e.g. the path it creates. */
  preview?: (value: string) => string | null;
  /** An error message to block submitting, or null when the value is fine. */
  validate?: (value: string) => string | null;
}

export interface ConfirmOptions {
  title: string;
  body?: string;
  confirm?: string;
  danger?: boolean;
}

export type Pending =
  | ({ kind: "prompt"; resolve: (v: string | null) => void } & PromptOptions)
  | ({ kind: "confirm"; resolve: (v: boolean) => void } & ConfirmOptions);

class DialogState {
  current = $state<Pending | null>(null);

  prompt(opts: PromptOptions): Promise<string | null> {
    this.cancel();
    return new Promise((resolve) => (this.current = { kind: "prompt", resolve, ...opts }));
  }

  confirm(opts: ConfirmOptions): Promise<boolean> {
    this.cancel();
    return new Promise((resolve) => (this.current = { kind: "confirm", resolve, ...opts }));
  }

  /** Closes with the given answer (a string for prompts, true for confirms). */
  submit(value: string) {
    const c = this.current;
    this.current = null;
    if (c?.kind === "prompt") c.resolve(value);
    else c?.resolve(true);
  }

  cancel() {
    const c = this.current;
    this.current = null;
    if (c?.kind === "prompt") c.resolve(null);
    else c?.resolve(false);
  }
}

export const dialog = new DialogState();

/** Common checks for a vault-relative path typed by the user. */
export function checkPath(value: string): string | null {
  const v = value.trim();
  if (!v || v.endsWith("/")) return "Enter a name";
  if (v.startsWith("/") || /^[a-zA-Z]:[\\/]/.test(v)) return "Use a path inside the vault, without a leading /";
  if (v.split("/").some((p) => p === "..")) return "“..” isn't allowed";
  if (/[<>:"|?*\\]/.test(v)) return "Avoid < > : \" | ? * \\ — they break on some systems";
  return null;
}

/** "people/alex" -> "people/alex.md"; common text extensions are kept as typed. */
export function withMd(value: string): string {
  const v = value.trim();
  return /\.(md|markdown|txt|toml|json|ya?ml|csv)$/i.test(v) ? v : `${v}.md`;
}
