// GFM tables in live preview: a table renders as a real <table> until the
// cursor enters it, then shows its Markdown for editing. Block widgets must
// come from a state field, so this lives apart from the view plugin.
import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import DOMPurify from "dompurify";
import { marked } from "marked";
import type { PreviewHooks } from "./livePreview";

type Align = "left" | "center" | "right" | null;

/**
 * Splits "| a | b \| c | `x` | [[p|alias]] |" into cells. `\|` is a literal
 * pipe (in code spans too, as in GFM); a [[wikilink|alias]] stays whole.
 */
export function splitRow(line: string): string[] {
  let s = line.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|") && !s.endsWith("\\|")) s = s.slice(0, -1);
  const cells: string[] = [];
  let cur = "";
  let ticks = 0;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (c === "\\" && s[i + 1] === "|") {
      cur += ticks ? "|" : "\\|";
      i++;
    } else if (!ticks && c === "[" && s[i + 1] === "[") {
      const end = s.indexOf("]]", i + 2);
      if (end < 0) {
        cur += c;
        continue;
      }
      cur += s.slice(i, end + 2);
      i = end + 1;
    } else if (c === "`") {
      let n = 1;
      while (s[i + n] === "`") n++;
      ticks = ticks === n ? 0 : ticks || n;
      cur += "`".repeat(n);
      i += n - 1;
    } else if (c === "|" && !ticks) {
      cells.push(cur.trim());
      cur = "";
    } else cur += c;
  }
  cells.push(cur.trim());
  return cells;
}

function alignments(delimiter: string): Align[] {
  return splitRow(delimiter).map((c) => {
    const l = c.startsWith(":");
    const r = c.endsWith(":");
    return l && r ? "center" : r ? "right" : l ? "left" : null;
  });
}

const JIRA = /\b[A-Z][A-Z0-9]+-\d+\b/g;

/** Turns Jira keys in rendered text (not code or links) into status chips. */
function decorateJira(root: HTMLElement, hooks: PreviewHooks) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  while (walker.nextNode()) {
    const t = walker.currentNode as Text;
    if (!t.parentElement?.closest("code, a")) nodes.push(t);
  }
  for (const t of nodes) {
    const text = t.data;
    const parts: Node[] = [];
    let last = 0;
    for (const m of text.matchAll(JIRA)) {
      const info = hooks.jiraStatus(m[0]);
      parts.push(document.createTextNode(text.slice(last, m.index)));
      const key = document.createElement("span");
      key.className = "cm-jira-key" + (info ? ` cat-${info.category}` : "");
      key.textContent = m[0];
      key.title = info?.summary ?? "Not in the Jira snapshot";
      parts.push(key);
      if (info) {
        const chip = document.createElement("span");
        chip.className = `cm-jira-status cat-${info.category}`;
        chip.textContent = info.status;
        parts.push(chip);
      }
      last = m.index! + m[0].length;
    }
    if (!parts.length) continue;
    parts.push(document.createTextNode(text.slice(last)));
    t.replaceWith(...parts);
  }
}

const WIKILINK = /\[\[([^\]|#\n]+)(?:#[^\]|\n]*)?(?:\|([^\]\n]*))?\]\]/g;

function renderInline(md: string): string {
  const linked = md.replace(/\[\[[^\]\n]*\]\]/g, (w) => w.replace(/\\\|/g, "|")).replace(WIKILINK, (_, target: string, alias?: string) => {
    const t = target.trim();
    const label = alias ?? t.split("/").pop()!;
    return `[${label}](#wiki:${encodeURIComponent(t)})`;
  });
  return DOMPurify.sanitize(marked.parseInline(linked, { async: false, gfm: true }) as string);
}

class TableWidget extends WidgetType {
  constructor(
    readonly source: string,
    readonly from: number,
    readonly hooks: PreviewHooks,
  ) {
    super();
  }

  eq(o: TableWidget) {
    return o.source === this.source && o.from === this.from;
  }

  toDOM(view: EditorView) {
    const lines = this.source.split("\n");
    const header = splitRow(lines[0]);
    const align = alignments(lines[1] ?? "");
    const body = lines.slice(2).filter((l) => l.trim()).map(splitRow);
    const cols = header.length;

    const wrap = document.createElement("div");
    wrap.className = "cm-table-wrap";
    const table = document.createElement("table");
    table.className = "cm-table";
    const row = (cells: string[], tag: "th" | "td", lineNo: number) => {
      const tr = document.createElement("tr");
      tr.dataset.line = String(lineNo);
      for (let i = 0; i < cols; i++) {
        const cell = document.createElement(tag);
        if (align[i]) cell.style.textAlign = align[i]!;
        cell.innerHTML = renderInline(cells[i] ?? "");
        decorateJira(cell, this.hooks);
        tr.append(cell);
      }
      return tr;
    };
    const thead = document.createElement("thead");
    thead.append(row(header, "th", 0));
    const tbody = document.createElement("tbody");
    let lineNo = 2;
    for (const l of lines.slice(2)) {
      if (l.trim()) tbody.append(row(splitRow(l), "td", lineNo));
      lineNo++;
    }
    table.append(thead);
    if (body.length) table.append(tbody);
    wrap.append(table);

    wrap.addEventListener("mousedown", (e) => {
      const a = (e.target as HTMLElement).closest("a");
      if (a) {
        e.preventDefault();
        const href = a.getAttribute("href") ?? "";
        if (href.startsWith("#wiki:")) this.hooks.followLink(decodeURIComponent(href.slice(6)));
        else this.hooks.openUrl(href);
        return;
      }
      // Anywhere else: edit the Markdown of the row that was clicked.
      e.preventDefault();
      const tr = (e.target as HTMLElement).closest<HTMLElement>("tr");
      const n = Math.min(Number(tr?.dataset.line ?? 0), lines.length - 1);
      const line = view.state.doc.lineAt(this.from);
      const target = view.state.doc.line(Math.min(line.number + n, view.state.doc.lines));
      view.focus();
      view.dispatch({ selection: { anchor: target.to } });
    });
    return wrap;
  }

  ignoreEvent() {
    return true;
  }
}

function build(state: EditorState, hooks: PreviewHooks): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const sel = state.selection.ranges;
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name !== "Table") return;
      // Whole lines, so the widget replaces the block cleanly.
      const from = state.doc.lineAt(node.from).from;
      const to = state.doc.lineAt(node.to).to;
      if (!sel.some((r) => r.to >= from && r.from <= to)) {
        builder.add(from, to, Decoration.replace({ block: true, widget: new TableWidget(state.sliceDoc(from, to), from, hooks) }));
      }
      return false;
    },
  });
  return builder.finish();
}

export function tablePreview(hooks: PreviewHooks): Extension {
  return StateField.define<DecorationSet>({
    create: (state) => build(state, hooks),
    update(value, tr) {
      if (tr.docChanged || tr.selection || syntaxTree(tr.startState) !== syntaxTree(tr.state)) return build(tr.state, hooks);
      return value;
    },
    provide: (f) => EditorView.decorations.from(f),
  });
}
