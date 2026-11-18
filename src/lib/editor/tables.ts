// GFM tables in live preview: a table renders as a real <table> whose cells
// edit in place; the whole table's Markdown shows only when the cursor
// enters it (or via the table's "Edit as Markdown" button). Block widgets must
// come from a state field, so this lives apart from the view plugin.
import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import DOMPurify from "dompurify";
import { marked } from "marked";
import type { PreviewHooks } from "./livePreview";

type Align = "left" | "center" | "right" | null;

interface Cell {
  /** Cell value for rendering; inside code spans `\|` is already a plain pipe. */
  text: string;
  /** Trimmed source of the cell, as offsets into the line. */
  from: number;
  to: number;
  /** Everything between the cell's pipes. */
  segFrom: number;
  segTo: number;
}

/**
 * Scans "| a | b \| c | `x` | [[p|alias]] |" into cells. `\|` is a literal
 * pipe (in code spans too, as in GFM); a [[wikilink|alias]] stays whole.
 */
export function scanRow(line: string): Cell[] {
  let start = line.length - line.trimStart().length;
  let end = line.trimEnd().length;
  if (line[start] === "|") start++;
  if (end > start && line[end - 1] === "|" && line[end - 2] !== "\\") end--;
  const cells: Cell[] = [];
  let cur = "";
  let seg = start;
  let ticks = 0;
  const push = (at: number) => {
    const raw = line.slice(seg, at);
    const from = seg + (raw.length - raw.trimStart().length);
    cells.push({ text: cur.trim(), from, to: Math.max(from, seg + raw.trimEnd().length), segFrom: seg, segTo: at });
    cur = "";
    seg = at + 1;
  };
  for (let i = start; i < end; i++) {
    const c = line[i];
    if (c === "\\" && line[i + 1] === "|") {
      cur += ticks ? "|" : "\\|";
      i++;
    } else if (!ticks && c === "[" && line[i + 1] === "[") {
      const close = line.indexOf("]]", i + 2);
      if (close < 0 || close + 2 > end) {
        cur += c;
        continue;
      }
      cur += line.slice(i, close + 2);
      i = close + 1;
    } else if (c === "`") {
      let n = 1;
      while (line[i + n] === "`") n++;
      ticks = ticks === n ? 0 : ticks || n;
      cur += "`".repeat(n);
      i += n - 1;
    } else if (c === "|" && !ticks) push(i);
    else cur += c;
  }
  push(end);
  return cells;
}

export function splitRow(line: string): string[] {
  return scanRow(line).map((c) => c.text);
}

/** A cell's Markdown as shown while editing it: pipes unescaped. */
export function cellSource(line: string, col: number): string {
  const c = scanRow(line)[col];
  return c ? line.slice(c.from, c.to).replace(/\\\|/g, "|") : "";
}

/** The edit that sets cell `col` of `line` to `value` (typed text, pipes unescaped). */
export function setCell(line: string, col: number, value: string): { from: number; to: number; insert: string } | null {
  const md = value.replace(/\s*\n\s*/g, " ").trim().replace(/\\?\|/g, "\\|");
  const cells = scanRow(line);
  const c = cells[col];
  if (c) {
    if (line.slice(c.from, c.to) === md) return null;
    if (c.from < c.to) return { from: c.from, to: c.to, insert: md };
    return md ? { from: c.segFrom, to: c.segTo, insert: ` ${md} ` } : null;
  }
  if (!md) return null;
  const end = line.trimEnd().length;
  const closed = /(^|[^\\])\|$/.test(line.trimEnd());
  return { from: end, to: end, insert: (closed ? "" : " |") + "  |".repeat(col - cells.length) + ` ${md} |` };
}

export type TableOp = "rowAbove" | "rowBelow" | "deleteRow" | "colLeft" | "colRight" | "deleteCol";

/**
 * Applies a row or column edit to a table's source, for the cell at table
 * line `line`, column `col`. Returns the new source and the cell to edit
 * next, or null when the edit doesn't apply (e.g. deleting the header).
 */
export function editTable(source: string, op: TableOp, line: number, col: number): { source: string; focus: { line: number; col: number } | null } | null {
  const lines = source.split("\n");
  const width = splitRow(lines[0]).length;
  if (op === "rowAbove" || op === "rowBelow") {
    if (op === "rowAbove" && line === 0) return null;
    const at = line === 0 ? 2 : op === "rowBelow" ? line + 1 : line;
    lines.splice(at, 0, "|" + "  |".repeat(width));
    return { source: lines.join("\n"), focus: { line: at, col } };
  }
  if (op === "deleteRow") {
    if (line < 2) return null;
    lines.splice(line, 1);
    return { source: lines.join("\n"), focus: null };
  }
  if (op === "deleteCol" && width === 1) return null;
  const at = op === "colRight" ? col + 1 : col;
  const out = lines.map((l, i) => {
    if (i >= 2 && !l.trim()) return l;
    const cells = scanRow(l).map((c) => l.slice(c.from, c.to));
    while (cells.length < width) cells.push(i === 1 ? "---" : "");
    if (op === "deleteCol") cells.splice(col, 1);
    else cells.splice(at, 0, i === 1 ? "---" : "");
    return `${l.match(/^\s*/)![0]}| ${cells.join(" | ")} |`;
  });
  return { source: out.join("\n"), focus: op === "deleteCol" ? null : { line, col: at } };
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

/** The widget each table block currently shows, so DOM handlers read fresh source. */
const shown = new WeakMap<HTMLElement, TableWidget>();

const EDITING = "cm-cell-editing";
const CODE_ICON =
  '<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>';

/** Line offsets (within the table) of the rows that render: header, then body. */
function rowLines(source: string): number[] {
  const rows = [0];
  source.split("\n").forEach((l, i) => i >= 2 && l.trim() && rows.push(i));
  return rows;
}

function shape(source: string): string {
  return `${rowLines(source).join(",")}:${splitRow(source.split("\n")[0]).length}`;
}

function caretAt(cell: HTMLElement, x: number, y: number) {
  const sel = window.getSelection();
  if (!sel) return;
  const doc = document as Document & { caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null };
  let range: Range | null = null;
  const pos = doc.caretPositionFromPoint?.(x, y);
  if (pos) {
    range = document.createRange();
    range.setStart(pos.offsetNode, pos.offset);
  } else range = document.caretRangeFromPoint?.(x, y) ?? null;
  if (!range || !cell.contains(range.startContainer)) {
    range = document.createRange();
    range.selectNodeContents(cell);
    range.collapse(false);
  }
  range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
}

/**
 * A rendered table whose cells edit in place: clicking a cell shows that
 * cell's Markdown, and every keystroke is written straight to the document.
 */
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

  private renderCell(cell: HTMLElement, md: string) {
    cell.innerHTML = renderInline(md);
    decorateJira(cell, this.hooks);
  }

  private table(): HTMLTableElement {
    const lines = this.source.split("\n");
    const header = splitRow(lines[0]);
    const align = alignments(lines[1] ?? "");
    const cols = header.length;
    const table = document.createElement("table");
    table.className = "cm-table";
    const row = (lineNo: number, tag: "th" | "td") => {
      const cells = splitRow(lines[lineNo]);
      const tr = document.createElement("tr");
      for (let i = 0; i < cols; i++) {
        const cell = document.createElement(tag);
        cell.dataset.line = String(lineNo);
        cell.dataset.col = String(i);
        if (align[i]) cell.style.textAlign = align[i]!;
        this.renderCell(cell, cells[i] ?? "");
        tr.append(cell);
      }
      return tr;
    };
    const [head, ...body] = rowLines(this.source);
    const thead = document.createElement("thead");
    thead.append(row(head, "th"));
    table.append(thead);
    if (body.length) {
      const tbody = document.createElement("tbody");
      for (const n of body) tbody.append(row(n, "td"));
      table.append(tbody);
    }
    return table;
  }

  toDOM(view: EditorView) {
    const dom = document.createElement("div");
    dom.className = "cm-table-block";
    const raw = document.createElement("button");
    raw.className = "cm-table-raw";
    raw.title = "Edit table as Markdown";
    raw.innerHTML = CODE_ICON;
    const wrap = document.createElement("div");
    wrap.className = "cm-table-wrap";
    wrap.append(this.table());
    dom.append(wrap, raw);
    shown.set(dom, this);
    attachEditing(dom, view);
    return dom;
  }

  updateDOM(dom: HTMLElement, _view: EditorView, prev: TableWidget) {
    // Only take over this table's own DOM, so a cell being edited survives
    // the document change its own keystrokes caused.
    if (prev.from !== this.from) return false;
    shown.set(dom, this);
    const wrap = dom.querySelector(".cm-table-wrap")!;
    const editing = wrap.querySelector<HTMLElement>(`.${EDITING}`);
    const fresh = this.table();
    if (editing && shape(prev.source) === shape(this.source)) {
      const old = wrap.querySelectorAll<HTMLElement>("th, td");
      fresh.querySelectorAll<HTMLElement>("th, td").forEach((cell, i) => {
        if (old[i] !== editing) old[i].innerHTML = cell.innerHTML;
      });
    } else wrap.replaceChildren(fresh);
    return true;
  }

  ignoreEvent() {
    return true;
  }
}

interface MenuItem {
  label: string;
  run: () => void;
  disabled?: boolean;
  danger?: boolean;
}

let closeMenu: (() => void) | null = null;

/** A small context menu at (x, y); `null` items are separators. */
function openMenu(view: EditorView, x: number, y: number, items: (MenuItem | null)[]) {
  closeMenu?.();
  const menu = document.createElement("div");
  menu.className = "cm-table-menu";
  menu.setAttribute("role", "menu");
  for (const item of items) {
    if (!item) {
      menu.append(Object.assign(document.createElement("hr"), { className: "cm-table-menu-sep" }));
      continue;
    }
    const b = document.createElement("button");
    b.setAttribute("role", "menuitem");
    b.textContent = item.label;
    b.disabled = !!item.disabled;
    if (item.danger) b.classList.add("danger");
    b.addEventListener("mousedown", (e) => e.preventDefault()); // keep the cell's focus
    b.addEventListener("click", () => {
      close();
      item.run();
    });
    menu.append(b);
  }
  // Inside the editor so its theme applies; fixed, so the scroller doesn't clip it.
  view.dom.append(menu);
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(4, Math.min(x, innerWidth - r.width - 4))}px`;
  menu.style.top = `${Math.max(4, Math.min(y, innerHeight - r.height - 4))}px`;

  const outside = (e: Event) => {
    if (!menu.contains(e.target as Node)) close();
  };
  const key = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  };
  const close = () => {
    menu.remove();
    document.removeEventListener("mousedown", outside, true);
    document.removeEventListener("keydown", key, true);
    window.removeEventListener("blur", close);
    view.scrollDOM.removeEventListener("scroll", close);
    closeMenu = null;
  };
  document.addEventListener("mousedown", outside, true);
  document.addEventListener("keydown", key, true);
  window.addEventListener("blur", close);
  view.scrollDOM.addEventListener("scroll", close);
  closeMenu = close;
}

/** Cell editing, delegated on the table block so it survives re-renders. */
function attachEditing(dom: HTMLElement, view: EditorView) {
  const current = () => shown.get(dom)!;
  const cells = () => Array.from(dom.querySelectorAll<HTMLElement>("tr")).map((tr) => Array.from(tr.querySelectorAll<HTMLElement>("th, td")));
  const docLine = (cell: HTMLElement) => view.state.doc.lineAt(current().from).number + Number(cell.dataset.line);
  const col = (cell: HTMLElement) => Number(cell.dataset.col);

  const begin = (cell: HTMLElement, at?: { x: number; y: number }) => {
    cell.textContent = cellSource(view.state.doc.line(docLine(cell)).text, col(cell));
    try {
      cell.contentEditable = "plaintext-only";
    } catch {
      cell.contentEditable = "true";
    }
    cell.classList.add(EDITING);
    cell.spellcheck = true;
    cell.focus({ preventScroll: true });
    if (at) caretAt(cell, at.x, at.y);
    else {
      const range = document.createRange();
      range.selectNodeContents(cell);
      range.collapse(false);
      window.getSelection()?.removeAllRanges();
      window.getSelection()?.addRange(range);
    }
    cell.scrollIntoView({ block: "nearest", inline: "nearest" });
  };

  const finish = (cell: HTMLElement) => {
    if (!cell.classList.contains(EDITING)) return;
    cell.classList.remove(EDITING);
    cell.removeAttribute("contenteditable");
    if (!cell.isConnected) return;
    const line = view.state.doc.line(docLine(cell)).text;
    cell.innerHTML = renderInline(splitRow(line)[col(cell)] ?? "");
    decorateJira(cell, current().hooks);
  };

  const commit = (cell: HTMLElement) => {
    const line = view.state.doc.line(docLine(cell));
    const change = setCell(line.text, col(cell), cell.textContent ?? "");
    if (change) view.dispatch({ changes: { from: line.from + change.from, to: line.from + change.to, insert: change.insert }, userEvent: "input.type" });
  };

  /** Appends an empty row and starts editing it at `column`. */
  const addRow = (column: number) => {
    const w = current();
    const n = splitRow(w.source.split("\n")[0]).length;
    const end = w.from + w.source.length;
    view.dispatch({ changes: { from: end, insert: "\n|" + "  |".repeat(n) }, userEvent: "input" });
    const grid = cells();
    const target = grid[grid.length - 1]?.[column];
    if (target) begin(target);
  };

  /** Leaves the table for the line above or below it. */
  const exit = (dir: -1 | 1) => {
    const w = current();
    const end = w.from + w.source.length;
    const doc = view.state.doc;
    const anchor = dir < 0 ? (w.from > 0 ? w.from - 1 : w.from) : end < doc.length ? end + 1 : end;
    view.focus();
    view.dispatch({ selection: { anchor }, scrollIntoView: true });
  };

  /** Shows the whole table's Markdown by putting the cursor in its header. */
  const rawMode = () => {
    view.focus();
    view.dispatch({ selection: { anchor: view.state.doc.lineAt(current().from).to }, scrollIntoView: true });
  };

  const move = (cell: HTMLElement, key: string, shift: boolean) => {
    const grid = cells();
    const r = grid.findIndex((row) => row.includes(cell));
    const c = grid[r].indexOf(cell);
    const last = grid.length - 1;
    const width = grid[0].length;
    let to: [number, number] | null = null;
    if (key === "Tab" && !shift) {
      if (c < width - 1) to = [r, c + 1];
      else if (r < last) to = [r + 1, 0];
      else return addRow(0);
    } else if (key === "Tab") {
      if (c > 0) to = [r, c - 1];
      else if (r > 0) to = [r - 1, width - 1];
      else return exit(-1);
    } else if (key === "Enter") {
      if (r < last) to = [r + 1, c];
      else return addRow(c);
    } else if (key === "ArrowUp") {
      if (r > 0) to = [r - 1, c];
      else return exit(-1);
    } else if (key === "ArrowDown") {
      if (r < last) to = [r + 1, c];
      else return exit(1);
    }
    if (to) begin(grid[to[0]][to[1]]);
  };

  dom.addEventListener("mousedown", (e) => {
    const target = e.target as HTMLElement;
    const cell = target.closest<HTMLElement>("th, td");
    if (cell?.classList.contains(EDITING)) return; // place the caret natively
    e.preventDefault();
    if (target.closest(".cm-table-raw")) return rawMode();
    const a = target.closest("a");
    if (a && !e.altKey) {
      const href = a.getAttribute("href") ?? "";
      if (href.startsWith("#wiki:")) current().hooks.followLink(decodeURIComponent(href.slice(6)));
      else current().hooks.openUrl(href);
      return;
    }
    if (cell && e.button === 0) begin(cell, { x: e.clientX, y: e.clientY });
  });

  dom.addEventListener("input", (e) => {
    const cell = (e.target as HTMLElement).closest<HTMLElement>(`.${EDITING}`);
    if (cell) commit(cell);
  });

  dom.addEventListener("focusout", (e) => {
    const cell = (e.target as HTMLElement).closest<HTMLElement>(`.${EDITING}`);
    if (cell) finish(cell);
  });

  dom.addEventListener("paste", (e) => {
    if (!(e.target as HTMLElement).closest(`.${EDITING}`)) return;
    e.preventDefault();
    const text = (e.clipboardData?.getData("text/plain") ?? "").replace(/\s*\n\s*/g, " ");
    document.execCommand("insertText", false, text);
  });

  /** Applies a row/column edit for `cell` and continues editing where it points. */
  const structural = (cell: HTMLElement, op: TableOp) => {
    const w = current();
    const res = editTable(w.source, op, Number(cell.dataset.line), col(cell));
    if (!res) return;
    if (cell.classList.contains(EDITING)) finish(cell);
    view.dispatch({ changes: { from: w.from, to: w.from + w.source.length, insert: res.source }, userEvent: "input" });
    const { focus } = res;
    const target = focus && dom.querySelector<HTMLElement>(`[data-line="${focus.line}"][data-col="${focus.col}"]`);
    if (target) begin(target);
  };

  dom.addEventListener("contextmenu", (e) => {
    const cell = (e.target as HTMLElement).closest<HTMLElement>("th, td");
    if (!cell) return;
    e.preventDefault();
    const header = cell.dataset.line === "0";
    const single = (cell.parentElement?.children.length ?? 0) < 2;
    openMenu(view, e.clientX, e.clientY, [
      { label: "Insert row above", disabled: header, run: () => structural(cell, "rowAbove") },
      { label: "Insert row below", run: () => structural(cell, "rowBelow") },
      { label: "Delete row", disabled: header, danger: true, run: () => structural(cell, "deleteRow") },
      null,
      { label: "Insert column left", run: () => structural(cell, "colLeft") },
      { label: "Insert column right", run: () => structural(cell, "colRight") },
      { label: "Delete column", disabled: single, danger: true, run: () => structural(cell, "deleteCol") },
      null,
      { label: "Edit table as Markdown", run: () => rawMode() },
    ]);
  });

  dom.addEventListener("keydown", (e) => {
    const cell = (e.target as HTMLElement).closest<HTMLElement>(`.${EDITING}`);
    if (!cell || e.isComposing || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === "Escape") {
      e.preventDefault();
      exit(1);
    } else if (e.key === "Enter" && e.shiftKey) {
      e.preventDefault(); // cells are single-line
    } else if (e.key === "Tab" || e.key === "Enter" || e.key === "ArrowUp" || e.key === "ArrowDown") {
      if (e.shiftKey && e.key !== "Tab") return;
      e.preventDefault();
      move(cell, e.key, e.shiftKey);
    }
  });
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
