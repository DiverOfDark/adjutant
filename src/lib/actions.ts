// One-click agent actions. Plain prompts (not slash commands) so they work
// with any ACP agent. Rules about folders and syntax live in the vault's AGENTS.md.

const today = () => new Date().toISOString().slice(0, 10);

export interface AgentAction {
  id: string;
  label: string;
  /** Builds the prompt; `file` is the open note, if any. */
  prompt: (ctx: { file?: string; person?: string }) => string;
  needsFile?: boolean;
  needsPerson?: boolean;
}

export const ACTIONS: AgentAction[] = [
  {
    id: "plan-day",
    label: "Plan my day",
    prompt: () =>
      `Plan my day (${today()}). Read tasks.md, jira/snapshot.md and the last entries of log/. ` +
      `Propose my top 3 for today with one line of reasoning each, then list anything slipping ` +
      `(overdue @due dates, Waiting on items older than a week, Jira tickets stuck in the same status). ` +
      `Do not edit files; I'll decide. Cite notes as [[links]].`,
  },
  {
    id: "organize-inbox",
    label: "Organize inbox",
    prompt: () =>
      `Organize inbox/. For each note: decide where it belongs (people/<name>.md, teams/<team>/, me/, or a new topic note), ` +
      `append its content there under a dated heading with a one-line summary above the raw text, ` +
      `turn action items into tasks in tasks.md (Now, Next or Waiting on, linked to the person), ` +
      `then move the original into inbox/filed/. Never delete text. Finish with a short list of what went where.`,
  },
  {
    id: "extract-tasks",
    label: "Extract tasks from this note",
    needsFile: true,
    prompt: ({ file }) =>
      `Read ${file} and extract action items. Add mine to tasks.md under ## Now or ## Next; ` +
      `add things I'm waiting for under ## Waiting on with the owner as [[people/<name>]]. ` +
      `Keep each task one line, add @due(YYYY-MM-DD) only when a date is stated, and link back with [[${file?.replace(/\.md$/, "")}]]. ` +
      `Skip anything already in tasks.md.`,
  },
  {
    id: "end-of-day",
    label: "End-of-day summary",
    prompt: () =>
      `Write today's (${today()}) end-of-day entry. Use tasks/done/ for what was completed today, ` +
      `\`git log --since=midnight --stat\` for notes that changed, and tasks.md for what's still open. ` +
      `Append it to log/${today().slice(0, 7)}.md under "## ${today()}" with sections Moved, Decided, Open. Keep it under 15 lines.`,
  },
  {
    id: "draft-review",
    label: "Draft review",
    needsPerson: true,
    prompt: ({ person }) =>
      `Draft a performance review for [[${person}]]. Use ${person}.md (1:1s, feedback, review evidence), ` +
      `log/ and tasks/done/ from the last six months, and reports/. Structure: summary, impact with concrete examples, ` +
      `strengths, growth areas, goals for next period. Cite the note for every example. ` +
      `Save it as ${person}-review-${today()}.md next to the person note and tell me what evidence is thin.`,
  },
];

export function repoQuestion(url: string, question: string) {
  const name = url.replace(/\.git$/, "").split(/[/:]/).pop();
  return (
    `Repository: ${url}\n` +
    `If repos/${name} does not exist, clone it there with \`git clone --depth 50 ${url} repos/${name}\`; ` +
    `otherwise run \`git -C repos/${name} pull --ff-only\`. Then answer, citing file paths and line numbers:\n\n${question}`
  );
}

export function importPage(url: string) {
  const today = new Date().toISOString().slice(0, 10);
  return (
    `Import this page into the vault: ${url}\n` +
    `Fetch it with your fetch tool, or the browser MCP server if it needs my login (read-only: never click anything that changes data). ` +
    `For Confluence prefer the Atlassian MCP server if available, and import child pages too if I asked for a page tree. ` +
    `Save it as clean Markdown under sources/ (Confluence pages under sources/confluence/), file name from the title in kebab-case, starting with front matter:\n` +
    `---\nsource: ${url}\ntitle: <page title>\nfetched: ${today}\n---\n` +
    `Keep headings, lists, tables and links; drop navigation and boilerplate. Turn links to other imported pages into [[wikilinks]]. Then give me a three-line summary.`
  );
}

export function resyncSource(file: string, url: string) {
  return (
    `Re-sync ${file} from its source ${url}. Fetch the current page, convert it to Markdown the same way, ` +
    `and update ${file} (keep the front matter, set fetched to today). Propose the edit as a diff so I can review it before it is written.`
  );
}
