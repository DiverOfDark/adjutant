// Live preview for Markdown in CodeMirror 6: syntax marks are hidden except
// on lines that hold the cursor, so the text stays plain Markdown while
// reading like a rendered page.
import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from "@codemirror/view";

export interface PreviewHooks {
  followLink: (target: string) => void;
  jiraStatus: (key: string) => { status: string; category: string; summary: string } | null;
  openUrl: (url: string) => void;
}

class CheckboxWidget extends WidgetType {
  constructor(readonly checked: boolean, readonly pos: number) {
    super();
  }
  eq(other: CheckboxWidget) {
    return other.checked === this.checked && other.pos === this.pos;
  }
  toDOM(view: EditorView) {
    const box = document.createElement("span");
    box.className = "cm-task-box" + (this.checked ? " checked" : "");
    box.setAttribute("role", "checkbox");
    box.setAttribute("aria-checked", String(this.checked));
    box.innerHTML = '<svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7"/></svg>';
    box.addEventListener("mousedown", (e) => {
      e.preventDefault();
      const line = view.state.doc.lineAt(this.pos);
      const m = line.text.match(/^(\s*[-*+] \[)( |x|X)(\])/);
      if (!m) return;
      const at = line.from + m[1].length;
      let insert = this.checked ? " " : "x";
      const changes = [{ from: at, to: at + 1, insert }];
      const today = new Date().toISOString().slice(0, 10);
      const done = line.text.match(/\s@done\(\d{4}-\d{2}-\d{2}\)/);
      if (!this.checked && !done) changes.push({ from: line.to, to: line.to, insert: ` @done(${today})` });
      if (this.checked && done) {
        const start = line.from + line.text.indexOf(done[0]);
        changes.push({ from: start, to: start + done[0].length, insert: "" });
      }
      view.dispatch({ changes });
      insert = "";
    });
    return box;
  }
  ignoreEvent() {
    return false;
  }
}

class JiraWidget extends WidgetType {
  constructor(readonly status: string, readonly category: string, readonly summary: string) {
    super();
  }
  eq(o: JiraWidget) {
    return o.status === this.status && o.category === this.category;
  }
  toDOM() {
    const el = document.createElement("span");
    el.className = `cm-jira-status cat-${this.category}`;
    el.textContent = this.status;
    el.title = this.summary;
    return el;
  }
}

class RuleWidget extends WidgetType {
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-hr";
    return el;
  }
}

const hide = Decoration.replace({});
const mark = (cls: string, attrs?: Record<string, string>) => Decoration.mark({ class: cls, attributes: attrs });
const lineDeco = (cls: string) => Decoration.line({ class: cls });

const WIKILINK = /\[\[([^\]|#\n]+)((?:#[^\]|\n]*)?)(?:\|([^\]\n]*))?\]\]/g;
const JIRA = /\b[A-Z][A-Z0-9]+-\d+\b/g;
const DUE = /@(due|done)\((\d{4}-\d{2}-\d{2})\)/g;
const TAG = /(^|\s)(#[\p{L}\p{N}_/-]*[\p{L}_/-][\p{L}\p{N}_/-]*)/gu;

function activeLines(state: EditorState): Set<number> {
  const lines = new Set<number>();
  for (const r of state.selection.ranges) {
    const a = state.doc.lineAt(r.from).number;
    const b = state.doc.lineAt(r.to).number;
    for (let i = a; i <= b; i++) lines.add(i);
  }
  return lines;
}

function build(view: EditorView, hooks: PreviewHooks): DecorationSet {
  const { state } = view;
  const active = view.hasFocus ? activeLines(state) : new Set<number>();
  const decos: { from: number; to: number; deco: Decoration }[] = [];
  const add = (from: number, to: number, deco: Decoration) => decos.push({ from, to, deco });
  const isActive = (pos: number) => active.has(state.doc.lineAt(pos).number);
  const codeRanges: [number, number][] = [];
  const today = new Date().toISOString().slice(0, 10);

  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;
        const heading = /^ATXHeading(\d)$/.exec(name);
        if (heading) {
          const line = state.doc.lineAt(node.from);
          add(line.from, line.from, lineDeco(`cm-h cm-h${heading[1]}`));
          return;
        }
        switch (name) {
          case "HeaderMark": {
            if (!isActive(node.from)) {
              const next = state.sliceDoc(node.to, node.to + 1);
              add(node.from, next === " " ? node.to + 1 : node.to, hide);
            }
            break;
          }
          case "Emphasis":
            add(node.from, node.to, mark("cm-em"));
            break;
          case "StrongEmphasis":
            add(node.from, node.to, mark("cm-strong"));
            break;
          case "Strikethrough":
            add(node.from, node.to, mark("cm-strike"));
            break;
          case "EmphasisMark":
          case "StrikethroughMark":
          case "CodeMark":
            if (name === "CodeMark" && node.node.parent?.name === "FencedCode") break;
            if (!isActive(node.from)) add(node.from, node.to, hide);
            break;
          case "InlineCode":
            add(node.from, node.to, mark("cm-inline-code"));
            codeRanges.push([node.from, node.to]);
            break;
          case "FencedCode": {
            codeRanges.push([node.from, node.to]);
            for (let pos = node.from; pos <= node.to; ) {
              const line = state.doc.lineAt(pos);
              add(line.from, line.from, lineDeco("cm-codeblock"));
              pos = line.to + 1;
            }
            return false;
          }
          case "Blockquote": {
            for (let pos = node.from; pos <= node.to; ) {
              const line = state.doc.lineAt(pos);
              add(line.from, line.from, lineDeco("cm-quote"));
              pos = line.to + 1;
            }
            break;
          }
          case "QuoteMark":
            if (!isActive(node.from)) add(node.from, Math.min(node.to + 1, state.doc.lineAt(node.from).to), hide);
            break;
          case "HorizontalRule":
            if (!isActive(node.from)) add(node.from, node.to, Decoration.replace({ widget: new RuleWidget() }));
            break;
          case "Link": {
            const text = node.node.getChildren("LinkMark");
            const url = node.node.getChild("URL");
            if (text.length >= 2) {
              const label = { from: text[0].to, to: text[1].from };
              const href = url ? state.sliceDoc(url.from, url.to) : "";
              add(label.from, label.to, mark("cm-link", { "data-url": href }));
              if (!isActive(node.from)) {
                add(node.from, text[0].to, hide);
                add(text[1].from, node.to, hide);
              }
            }
            return false;
          }
          case "URL":
            if (node.node.parent?.name !== "Link") add(node.from, node.to, mark("cm-link", { "data-url": state.sliceDoc(node.from, node.to) }));
            break;
          case "Task": {
            const line = state.doc.lineAt(node.from);
            const marker = node.node.getChild("TaskMarker");
            if (!marker) break;
            const checked = /x/i.test(state.sliceDoc(marker.from, marker.to));
            if (checked) add(marker.to, line.to, mark("cm-task-done"));
            if (!isActive(node.from)) {
              const listMark = node.node.parent?.getChild("ListMark");
              const start = listMark ? listMark.from : marker.from;
              const end = Math.min(marker.to + 1, line.to);
              add(start, end, Decoration.replace({ widget: new CheckboxWidget(checked, line.from) }));
            }
            break;
          }
        }
      },
    });

    // Regex-based inline syntax that the Markdown grammar doesn't know.
    const text = state.sliceDoc(from, to);
    const inCode = (pos: number) => codeRanges.some(([a, b]) => pos >= a && pos < b);
    for (const m of text.matchAll(WIKILINK)) {
      const start = from + m.index!;
      const end = start + m[0].length;
      if (inCode(start)) continue;
      const target = m[1].trim();
      const alias = m[3];
      if (isActive(start)) {
        add(start, end, mark("cm-wikilink raw", { "data-link": target }));
      } else {
        const labelFrom = alias != null ? start + 2 + m[1].length + m[2].length + 1 : start + 2;
        const labelTo = alias != null ? end - 2 : start + 2 + m[1].length;
        add(start, labelFrom, hide);
        const shown = state.sliceDoc(labelFrom, labelTo);
        add(labelFrom, labelTo, mark("cm-wikilink", { "data-link": target, title: target }));
        if (alias == null && target.includes("/")) {
          // Show "people/alex" as "alex".
          const slash = shown.lastIndexOf("/");
          if (slash >= 0) add(labelFrom, labelFrom + slash + 1, hide);
        }
        add(labelTo, end, hide);
      }
    }
    for (const m of text.matchAll(JIRA)) {
      const start = from + m.index!;
      if (inCode(start)) continue;
      const end = start + m[0].length;
      const info = hooks.jiraStatus(m[0]);
      add(start, end, mark("cm-jira-key" + (info ? ` cat-${info.category}` : ""), { "data-jira": m[0], title: info?.summary ?? "Not in the Jira snapshot" }));
      if (info) add(end, end, Decoration.widget({ widget: new JiraWidget(info.status, info.category, info.summary), side: 1 }));
    }
    for (const m of text.matchAll(DUE)) {
      const start = from + m.index!;
      if (inCode(start)) continue;
      const date = m[2];
      const cls = m[1] === "done" ? "cm-done-date" : date < today ? "cm-due late" : date <= addDays(today, 2) ? "cm-due soon" : "cm-due";
      add(start, start + m[0].length, mark(cls));
    }
    for (const m of text.matchAll(TAG)) {
      const start = from + m.index! + m[1].length;
      if (inCode(start)) continue;
      const line = state.doc.lineAt(start);
      if (line.text.startsWith("#") && start === line.from) continue;
      add(start, start + m[2].length, mark("cm-tag"));
    }
  }

  decos.sort((a, b) => a.from - b.from || startSide(a.deco) - startSide(b.deco));
  const builder = new RangeSetBuilder<Decoration>();
  let lastReplaceEnd = -1;
  for (const d of decos) {
    const isReplace = (d.deco.spec as { widget?: unknown }).widget !== undefined || d.deco === hide;
    if (isReplace && d.from < lastReplaceEnd) continue;
    try {
      builder.add(d.from, d.to, d.deco);
      if (isReplace && d.to > d.from) lastReplaceEnd = d.to;
    } catch {
      // Overlapping decorations that can't be ordered are skipped.
    }
  }
  return builder.finish();
}

function startSide(d: Decoration) {
  return (d as unknown as { startSide: number }).startSide ?? 0;
}

function addDays(date: string, n: number) {
  const d = new Date(date + "T00:00:00");
  d.setDate(d.getDate() + n);
  return d.toISOString().slice(0, 10);
}

export function livePreview(hooks: PreviewHooks): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view, hooks);
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.viewportChanged || u.selectionSet || u.focusChanged || syntaxTree(u.startState) !== syntaxTree(u.state)) {
          this.decorations = build(u.view, hooks);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );

  const clicks = EditorView.domEventHandlers({
    mousedown(e) {
      const el = (e.target as HTMLElement).closest<HTMLElement>("[data-link],[data-url],[data-jira]");
      if (!el) return false;
      const follow = e.ctrlKey || e.metaKey || !el.classList.contains("raw");
      if (!follow) return false;
      if (el.dataset.link) hooks.followLink(el.dataset.link);
      else if (el.dataset.url) hooks.openUrl(el.dataset.url);
      else if (el.dataset.jira) {
        if (!(e.ctrlKey || e.metaKey)) return false;
        hooks.openUrl(`jira:${el.dataset.jira}`);
      }
      e.preventDefault();
      return true;
    },
  });

  return [plugin, clicks];
}
