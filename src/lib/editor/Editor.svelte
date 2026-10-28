<script lang="ts">
  import { onMount } from "svelte";
  import { EditorState, Compartment, Prec, type Extension } from "@codemirror/state";
  import { EditorView, keymap, drawSelection, highlightActiveLine, placeholder as cmPlaceholder } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
  import { languages } from "@codemirror/language-data";
  import { syntaxHighlighting, HighlightStyle, indentOnInput, LanguageDescription } from "@codemirror/language";
  import { autocompletion, closeBrackets, type CompletionContext } from "@codemirror/autocomplete";
  import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
  import { tags as t } from "@lezer/highlight";
  import { livePreview, type PreviewHooks } from "./livePreview";
  import { tablePreview } from "./tables";
  import { app, personName } from "$lib/app.svelte";

  let {
    path,
    content,
    onchange,
    onsave,
    line,
  }: { path: string; content: string; onchange: (text: string) => void; onsave?: () => void; line?: number } = $props();

  // Only Markdown files get live preview, wikilinks and task widgets; every
  // other file is edited as plain source with syntax highlighting.
  // The editor is remounted for every file, so reading `path` once is intended.
  // svelte-ignore state_referenced_locally
  const isMarkdown = /\.(md|markdown)$/i.test(path);
  const language = new Compartment();

  let host: HTMLDivElement;
  let view: EditorView | undefined;

  function wikilinkCompletion(ctx: CompletionContext) {
    const m = ctx.matchBefore(/\[\[[^\]\n]*/);
    if (!m) return null;
    const typed = m.text.slice(2);
    return {
      from: m.from + 2,
      options: app.notes.map((n) => {
        const target = n.replace(/\.md$/, "");
        return {
          label: target,
          detail: target.startsWith("people/") ? personName(target) : undefined,
          apply: target + "]]",
          boost: target.startsWith("people/") ? 1 : 0,
        };
      }),
      validFor: /^[^\]\n]*$/,
      filter: true,
      ...(typed ? {} : {}),
    };
  }

  function tagCompletion(ctx: CompletionContext) {
    const m = ctx.matchBefore(/(^|\s)#[\p{L}\p{N}_/-]*/u);
    if (!m || ctx.state.doc.lineAt(ctx.pos).text.match(/^#+\s/)) return null;
    const tags = [...new Set(app.tasks.flatMap((t) => t.tags))];
    if (!tags.length) return null;
    const hash = m.text.indexOf("#");
    return { from: m.from + hash + 1, options: tags.map((tag) => ({ label: tag, type: "keyword" })), validFor: /^[\p{L}\p{N}_/-]*$/u };
  }

  const highlight = HighlightStyle.define([
    { tag: t.heading, fontWeight: "800" },
    { tag: t.strong, fontWeight: "700" },
    { tag: t.emphasis, fontStyle: "italic" },
    { tag: t.strikethrough, textDecoration: "line-through" },
    { tag: t.link, color: "var(--verdigris)" },
    { tag: t.url, color: "var(--muted)" },
    { tag: t.monospace, fontFamily: "var(--font-mono)", fontSize: "0.88em" },
    { tag: t.processingInstruction, color: "var(--faint)", fontFamily: "var(--font-mono)", fontSize: "0.85em" },
    { tag: t.quote, color: "var(--ink-2)" },
    { tag: t.keyword, color: "var(--verdigris)" },
    { tag: t.string, color: "var(--brass)" },
    { tag: t.comment, color: "var(--muted)", fontStyle: "italic" },
    { tag: [t.number, t.bool], color: "var(--signal)" },
    { tag: t.meta, color: "var(--faint)" },
  ]);

  const theme = EditorView.theme({
    "&": { height: "100%", fontSize: "1rem", background: "transparent" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "var(--font-ui)", lineHeight: "1.7", overflow: "auto" },
    ".cm-content": { maxWidth: "46rem", margin: "0 auto", padding: "0 0 40vh", caretColor: "var(--verdigris)" },
    ".cm-line": { padding: "0 2px" },
    ".cm-activeLine": { background: "transparent" },
    ".cm-cursor": { borderLeftColor: "var(--verdigris)", borderLeftWidth: "2px" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": { background: "var(--select) !important" },
    ".cm-h": { fontWeight: "800", letterSpacing: "-0.015em", color: "var(--ink)" },
    ".cm-h1": { fontSize: "2rem", lineHeight: "1.2", padding: "0.4rem 0 0.3rem" },
    ".cm-h2": { fontSize: "1.4rem", lineHeight: "1.3", paddingTop: "1.1rem" },
    ".cm-h3": { fontSize: "1.12rem", paddingTop: "0.7rem" },
    ".cm-h4, .cm-h5, .cm-h6": { fontSize: "1rem", color: "var(--ink-2)" },
    ".cm-strong": { fontWeight: "700" },
    ".cm-em": { fontStyle: "italic" },
    ".cm-strike": { textDecoration: "line-through", color: "var(--muted)" },
    ".cm-inline-code": { fontFamily: "var(--font-mono)", fontSize: "0.86em", background: "var(--sunken)", borderRadius: "4px", padding: "0.1em 0.25em" },
    ".cm-codeblock": { fontFamily: "var(--font-mono)", fontSize: "0.86em", background: "var(--sunken)", lineHeight: "1.55" },
    ".cm-quote": { borderLeft: "3px solid var(--rule-strong)", paddingLeft: "0.9rem !important", color: "var(--ink-2)" },
    ".cm-table-wrap": { overflowX: "auto", margin: "0.4rem 0 0.6rem", cursor: "text" },
    ".cm-table": { borderCollapse: "collapse", fontSize: "0.94em", lineHeight: "1.45" },
    ".cm-table th, .cm-table td": { border: "1px solid var(--rule)", padding: "0.3rem 0.7rem", verticalAlign: "top", textAlign: "left" },
    ".cm-table th": { background: "var(--sunken)", fontWeight: "650" },
    ".cm-table tbody tr:nth-child(even) td": { background: "color-mix(in srgb, var(--sunken) 45%, transparent)" },
    ".cm-table code": { fontFamily: "var(--font-mono)", fontSize: "0.88em", background: "var(--sunken)", padding: "0.05em 0.3em", borderRadius: "3px" },
    ".cm-table a": { color: "var(--verdigris)", textDecoration: "none", cursor: "pointer" },
    ".cm-table a:hover": { textDecoration: "underline" },
    ".cm-hr": { display: "inline-block", width: "100%", borderTop: "1px solid var(--rule-strong)", verticalAlign: "middle" },
    ".cm-link": { color: "var(--verdigris)", textDecoration: "underline", textUnderlineOffset: "2px", cursor: "pointer" },
    ".cm-wikilink": { color: "var(--verdigris)", fontWeight: "600", cursor: "pointer", textDecoration: "none" },
    ".cm-wikilink:hover": { textDecoration: "underline" },
    ".cm-wikilink.raw": { fontWeight: "400" },
    ".cm-tag": { color: "var(--muted)" },
    ".cm-due": { color: "var(--muted)", fontSize: "0.88em" },
    ".cm-due.soon": { color: "var(--brass)", fontWeight: "700" },
    ".cm-due.late": { color: "var(--signal)", fontWeight: "700" },
    ".cm-done-date": { color: "var(--faint)", fontSize: "0.85em" },
    ".cm-task-done": { color: "var(--muted)", textDecoration: "line-through", textDecorationColor: "var(--faint)" },
    ".cm-jira-key": { fontWeight: "700", background: "var(--sunken)", borderRadius: "4px 0 0 4px", padding: "0.05em 0.3em" },
    ".cm-jira-key:not(.cat-new):not(.cat-indeterminate):not(.cat-done)": { borderRadius: "4px" },
    ".cm-jira-status": { fontSize: "0.82em", background: "var(--sunken)", color: "var(--muted)", borderRadius: "0 4px 4px 0", padding: "0.12em 0.4em 0.12em 0.1em" },
    ".cm-jira-status.cat-indeterminate": { color: "var(--verdigris)" },
    ".cm-jira-key.cat-done": { textDecoration: "line-through" },
    ".cm-task-box": {
      display: "inline-grid",
      placeItems: "center",
      width: "17px",
      height: "17px",
      margin: "0 0.55rem 0 0.1rem",
      verticalAlign: "-3px",
      border: "1.6px solid var(--rule-strong)",
      borderRadius: "5px",
      background: "var(--sheet)",
      cursor: "pointer",
    },
    ".cm-task-box svg": { width: "12px", height: "12px", fill: "none", stroke: "var(--verdigris-ink)", strokeWidth: "2.4", strokeLinecap: "round", strokeLinejoin: "round", opacity: "0" },
    ".cm-task-box:hover": { borderColor: "var(--verdigris)" },
    ".cm-task-box.checked": { background: "var(--verdigris)", borderColor: "var(--verdigris)" },
    ".cm-task-box.checked svg": { opacity: "1" },
    ".cm-tooltip": { border: "none", borderRadius: "var(--radius-m)", boxShadow: "var(--shadow-pop)", background: "var(--sheet)", overflow: "hidden" },
    ".cm-tooltip-autocomplete > ul": { fontFamily: "var(--font-ui)", maxHeight: "16rem" },
    ".cm-tooltip-autocomplete > ul > li": { padding: "0.3rem 0.7rem !important" },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": { background: "var(--verdigris)", color: "var(--verdigris-ink)" },
    ".cm-completionDetail": { color: "inherit", opacity: 0.7, marginLeft: "0.8rem", fontStyle: "normal" },
    ".cm-panels": { background: "var(--paper)", color: "var(--ink)", borderColor: "var(--rule)" },
    ".cm-searchMatch": { background: "var(--brass-wash)" },
    ".cm-placeholder": { color: "var(--faint)" },
  });

  export function focus() {
    view?.focus();
  }

  export function setContent(text: string) {
    if (!view || view.state.doc.toString() === text) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  }

  function scrollToLine(n: number | undefined) {
    if (!view || n == null) return;
    const ln = view.state.doc.line(Math.min(n + 1, view.state.doc.lines));
    view.dispatch({ selection: { anchor: ln.from }, effects: EditorView.scrollIntoView(ln.from, { y: "center" }) });
    view.focus();
  }

  $effect(() => {
    scrollToLine(line);
  });

  const previewHooks: PreviewHooks = {
    followLink: (target) => app.followLink(target),
    jiraStatus: (key) => {
      const i = app.jira?.issues[key];
      return i ? { status: i.status, category: i.statusCategory, summary: i.summary } : null;
    },
    openUrl: async (url) => {
      const href = url.startsWith("jira:") ? `${app.jira?.baseUrl ?? ""}/browse/${url.slice(5)}` : url;
      try {
        const { openUrl } = await import("@tauri-apps/plugin-opener");
        await openUrl(href);
      } catch {
        window.open(href, "_blank");
      }
    },
  };

  const markdownExtensions = (): Extension[] => [
    markdown({ base: markdownLanguage, codeLanguages: languages, addKeymap: true }),
    autocompletion({ override: [wikilinkCompletion, tagCompletion], icons: false }),
    livePreview(previewHooks),
    tablePreview(previewHooks),
    cmPlaceholder("Start writing. [[ links a note, #tag tags it, - [ ] makes a task."),
  ];

  const sourceTheme = EditorView.theme({
    ".cm-scroller": { fontFamily: "var(--font-mono)", fontSize: "0.86rem", lineHeight: "1.6" },
    ".cm-content": { maxWidth: "none", margin: "0" },
  });

  async function loadSourceLanguage() {
    const desc = LanguageDescription.matchFilename(languages, path.split("/").pop() ?? path);
    if (!desc || !view) return;
    const support = await desc.load();
    view?.dispatch({ effects: language.reconfigure(support) });
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: content,
        extensions: [
          history(),
          drawSelection(),
          highlightActiveLine(),
          indentOnInput(),
          closeBrackets(),
          highlightSelectionMatches(),
          EditorView.lineWrapping,
          syntaxHighlighting(highlight),
          isMarkdown ? markdownExtensions() : [language.of([]), Prec.highest(sourceTheme)],
          keymap.of([
            { key: "Mod-s", run: () => (onsave?.(), true) },
            indentWithTab,
            ...defaultKeymap,
            ...historyKeymap,
            ...searchKeymap,
          ]),
          theme,
          EditorView.updateListener.of((u) => {
            if (u.docChanged) onchange(u.state.doc.toString());
          }),
        ],
      }),
    });
    if (!isMarkdown) loadSourceLanguage();
    scrollToLine(line);
    return () => view?.destroy();
  });
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    height: 100%;
    min-height: 0;
  }
</style>
