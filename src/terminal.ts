import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { Terminal } from "@xterm/xterm";

import { api, errorText } from "./api";
import { askSecret, confirmHostKey } from "./dialogs";
import { h } from "./dom";
import { t } from "./i18n";
import { fontStack, getPrefs, onPrefsChange, terminalScheme } from "./prefs";
import type { Pane, PaneOptions, PaneState } from "./pane";
import { SearchBar } from "./search";
import type { PromptAnswer, SessionNotice } from "./types";


const DIM = "\x1b[2m";

/**
 * Text from the server or from errors (banner, exit signal, messages) is shown
 * with all control characters removed except new lines, so it cannot inject
 * escape sequences into the terminal (title, colours, cursor, hyperlinks…).
 */
function plain(text: string): string {
  return text
    .replace(/\r\n?/g, "\n")
    .replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u2069]/g, "")
    .replace(/\n/g, "\r\n");
}
const RED = "\x1b[31m";
const CYAN = "\x1b[36m";
const RESET = "\x1b[0m";

export interface TerminalOptions extends PaneOptions {
  vaultAvailable: boolean;
}

/** One terminal session with its xterm.js view. */
export class TerminalPane implements Pane {
  readonly el: HTMLElement;
  readonly term: Terminal;
  state: PaneState = "connecting";
  private readonly fit = new FitAddon();
  private readonly searchAddon = new SearchAddon({ highlightLimit: 1000 });
  private readonly searchBar: SearchBar;
  private sessionId: number | null = null;
  /** Aborted when the pane closes or reconnects: closes its pending dialogs. */
  private prompts = new AbortController();
  /** Bumped on every (re)connect so late events of an old session are ignored. */
  private generation = 0;
  private readonly resizeObserver: ResizeObserver;
  private readonly unsubscribePrefs: () => void;
  private disposed = false;
  /** Window title set by the remote shell (often "user@host: dir"); untrusted. */
  remoteTitle = "";
  private readonly titleListeners = new Set<(title: string) => void>();

  constructor(readonly opts: TerminalOptions) {
    this.el = h("div", { class: "pane terminal-host" });
    this.term = new Terminal({
      allowProposedApi: true,
      macOptionIsMeta: true,
      rightClickSelectsWord: true,
      ...TerminalPane.prefsOptions(),
    });
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.searchAddon);
    const unicode = new Unicode11Addon();
    this.term.loadAddon(unicode);
    this.term.unicode.activeVersion = "11";
    this.term.open(this.el);
    this.searchBar = new SearchBar(this.term, this.searchAddon);
    this.el.append(this.searchBar.el);

    this.term.attachCustomKeyEventHandler((e) => this.handleKey(e));
    this.term.onData((d) => this.input(d, false));
    this.term.onBinary((d) => this.input(d, true));
    this.term.onTitleChange((title) => {
      this.remoteTitle = title;
      for (const listener of this.titleListeners) listener(title);
    });
    this.term.onResize(({ cols, rows }) => {
      if (this.sessionId !== null) void api.resizeSession(this.sessionId, { cols, rows }).catch(() => {});
    });
    this.el.addEventListener("contextmenu", (e) => {
      // Right click: copy the selection if any, otherwise paste (PuTTY style).
      e.preventDefault();
      if (this.term.hasSelection()) void this.copy();
      else void this.paste();
    });

    this.term.textarea?.addEventListener("focus", () => this.opts.onFocus(this));

    this.resizeObserver = new ResizeObserver(() => this.refit());
    this.resizeObserver.observe(this.el);
    this.unsubscribePrefs = onPrefsChange(() => {
      Object.assign(this.term.options, TerminalPane.prefsOptions());
      this.refit();
    });
  }

  private static prefsOptions() {
    const p = getPrefs();
    return {
      theme: terminalScheme().colors,
      fontFamily: fontStack(),
      fontSize: p.fontSize,
      lineHeight: p.lineHeight,
      cursorStyle: p.cursorStyle,
      cursorBlink: p.cursorBlink,
      scrollback: p.scrollback,
    };
  }

  get title(): string {
    return this.opts.title;
  }

  async connect(): Promise<void> {
    const gen = ++this.generation;
    this.prompts.abort();
    this.prompts = new AbortController();
    this.setState("connecting");
    this.refit();
    if (this.opts.target.kind !== "shell") {
      this.term.write(`${DIM}${t("connecting", { target: this.opts.subtitle })}${RESET}\r\n`);
    }
    try {
      const id = await api.openSession(
        this.opts.target,
        { cols: this.term.cols, rows: this.term.rows },
        (bytes) => {
          if (gen === this.generation) this.term.write(bytes);
        },
        (notice) => {
          if (gen === this.generation) void this.onNotice(notice);
          else void this.rejectStalePrompt(notice);
        },
      );
      if (gen !== this.generation || this.disposed) {
        void api.closeSession(id);
        return;
      }
      this.sessionId = id;
      if (this.state === "connecting") this.setState("open");
      this.refit();
      void api.resizeSession(id, { cols: this.term.cols, rows: this.term.rows }).catch(() => {});
    } catch (e) {
      if (gen !== this.generation) return;
      this.term.write(`\r\n${RED}${plain(t("connectionFailed", { error: errorText(e) }))}${RESET}\r\n`);
      this.ended();
    }
  }

  focus(): void {
    this.term.focus();
    this.refit();
  }

  blur(): void {
    this.term.blur();
  }

  /** Opens the find bar (Ctrl+Shift+F). */
  find(): void {
    this.searchBar.open();
  }

  /** The backend session id while connected (for SFTP on the same connection). */
  get session(): number | null {
    return this.sessionId;
  }

  refit(): void {
    if (this.disposed || this.el.offsetParent === null) return;
    try {
      this.fit.fit();
    } catch {
      /* not visible yet */
    }
  }

  /** Calls `listener` when the remote shell changes the title; returns an unsubscribe. */
  onRemoteTitle(listener: (title: string) => void): () => void {
    this.titleListeners.add(listener);
    return () => this.titleListeners.delete(listener);
  }

  dispose(): void {
    this.disposed = true;
    this.titleListeners.clear();
    this.generation++;
    this.prompts.abort();
    this.resizeObserver.disconnect();
    this.unsubscribePrefs();
    if (this.sessionId !== null) void api.closeSession(this.sessionId);
    this.sessionId = null;
    this.term.dispose();
    this.el.remove();
  }

  async copy(): Promise<void> {
    const sel = this.term.getSelection();
    if (sel) await writeText(sel);
  }

  async paste(): Promise<void> {
    const text = await readText().catch(() => "");
    if (text) this.term.paste(text);
  }

  /** `binary`: xterm.js delivers raw bytes (some mouse reports) as one char per byte. */
  private input(data: string, binary: boolean): void {
    if (this.state === "closed") {
      if (data === "\r") void this.connect();
      return;
    }
    if (this.sessionId !== null) void api.writeSession(this.sessionId, data, binary).catch(() => {});
  }

  private handleKey(e: KeyboardEvent): boolean {
    if (e.type !== "keydown") return true;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.shiftKey && e.code === "KeyC") {
      void this.copy();
      return false;
    }
    if (mod && e.shiftKey && e.code === "KeyV") {
      void this.paste();
      e.preventDefault();
      return false;
    }
    // App-level shortcuts are handled in main.ts.
    return !this.opts.isAppShortcut(e);
  }

  private async onNotice(n: SessionNotice): Promise<void> {
    switch (n.type) {
      case "banner":
        this.term.write(`${DIM}${plain(n.text)}${RESET}\r\n`);
        break;
      case "info":
        this.term.write(`${CYAN}» ${plain(n.text)}${RESET}\r\n`);
        break;
      case "hostKey": {
        const accept = await confirmHostKey(n.info, this.prompts.signal);
        await api.answerPrompt(n.promptId, { kind: "hostKey", accept });
        break;
      }
      case "secret": {
        const res = await askSecret(n.prompt, this.opts.vaultAvailable, this.prompts.signal);
        const answer: PromptAnswer = res ? { kind: "secret", ...res } : { kind: "cancel" };
        await api.answerPrompt(n.promptId, answer);
        break;
      }
      case "exit": {
        const detail = [n.code !== null ? t("exitCode", { code: n.code }) : null, n.message && plain(n.message)]
          .filter(Boolean)
          .join(", ");
        this.term.write(`\r\n${DIM}[${t("sessionEnded")}${detail ? `: ${detail}` : ""}] ${t("pressEnterReconnect")}${RESET}\r\n`);
        this.ended();
        break;
      }
    }
  }

  private async rejectStalePrompt(n: SessionNotice): Promise<void> {
    if (n.type === "hostKey" || n.type === "secret") {
      await api.answerPrompt(n.promptId, { kind: "cancel" }).catch(() => {});
    }
  }

  private ended(): void {
    this.sessionId = null;
    this.setState("closed");
  }

  private setState(s: PaneState): void {
    this.state = s;
    this.opts.onStateChange(this);
  }
}
