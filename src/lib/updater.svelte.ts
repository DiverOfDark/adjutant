// App updates from GitHub releases, as in difflab: downloaded quietly,
// installed when you quit from the tray, or now with "Restart to update".
import { inTauri, listenTo } from "./api";

const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;

type Pending = { install: () => Promise<void> };

class Updater {
  version = $state<string | null>(null);
  available = $state<{ version: string; ready: boolean } | null>(null);
  note = $state("");
  private pending: Pending | null = null;
  private started = false;

  async start() {
    if (this.started || !inTauri) return;
    this.started = true;
    const { getVersion } = await import("@tauri-apps/api/app");
    this.version = await getVersion();
    // Closing the window hides it to the tray; Quit in the tray menu ends the
    // app, installing a downloaded update on the way out.
    await listenTo("quit-requested", async () => {
      if (this.pending) await this.pending.install().catch(() => undefined);
      const { exit } = await import("@tauri-apps/plugin-process");
      await exit(0);
    });
    this.check();
    setInterval(() => this.check(), CHECK_EVERY_MS);
  }

  async check() {
    if (!inTauri || this.available?.ready) return;
    this.note = "Checking for updates";
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const found = await check();
      if (!found) {
        this.note = "Up to date";
        return;
      }
      this.available = { version: found.version, ready: false };
      await found.download();
      this.pending = found;
      this.available = { version: found.version, ready: true };
    } catch (e) {
      this.note = `Could not check for updates (${String(e)})`;
    }
  }

  async installNow() {
    if (!this.pending) return;
    await this.pending.install();
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  }
}

export const updater = new Updater();
