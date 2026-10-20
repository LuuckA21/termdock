import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";

import { api, errorText } from "./api";
import { alertBox, askSecret, confirmBox, confirmHostKey, modal } from "./dialogs";
import { h } from "./dom";
import { t } from "./i18n";
import { actionFor, isMac, keyHint } from "./keys";
import { fontStack, getPrefs, onPrefsChange, terminalScheme } from "./prefs";
import type { Pane, PaneOptions, PaneState } from "./pane";
import { HIGHLIGHT_LIMIT, SearchBar } from "./search";
import type { PromptAnswer, Reconnected, SessionNotice } from "./types";


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
/** The absolute path of an OSC 7 `file://host/path` report, or null. */
/** Pasted text without ESC and other control characters (tab and line breaks kept). */
function cleanPaste(text: string): string {
  return text.replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]/g, "");
}

function osc7Path(data: string): string | null {
  if (data.length > 4096) return null;
  try {
    const url = new URL(data);
    if (url.protocol !== "file:") return null;
    const path = decodeURIComponent(url.pathname);
    return path.startsWith("/") && !/[\p{Cc}\p{Cf}]/u.test(path) ? path : null;
  } catch {
    return null;
  }
}

/** Lines of a multi-line paste shown before it is sent, and their width. */
const PREVIEW_LINES = 12;
const PREVIEW_WIDTH = 160;

/**
 * Asks before pasting several lines (each would run at once), showing them:
 * text copied from a web page can hold commands that were not visible there.
 * Invisible and bidi characters are shown as codes.
 */
function confirmPaste(text: string, count: number): Promise<boolean> {
  const lines = text.replace(/[\r\n]+$/, "").split(/\r\n|\r|\n/);
  const visible = (line: string) => {
    const shown = line.replace(/[\p{Cf}\u2028\u2029]/gu, (c) => `<U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}>`);
    return shown.length > PREVIEW_WIDTH ? `${shown.slice(0, PREVIEW_WIDTH)}…` : shown;
  };
  const preview = lines.slice(0, PREVIEW_LINES).map(visible).join("\n");
  return modal<boolean>({
    title: t("confirmTitle"),
    body: [
      h("p", {}, t("pasteLines", { count })),
      h("pre", { class: "paste-preview" }, preview),
      lines.length > PREVIEW_LINES ? h("p", { class: "muted" }, t("pasteMore", { count: lines.length - PREVIEW_LINES })) : h("span"),
    ],
    buttons: [
      { label: t("cancel"), value: false },
      { label: t("paste"), value: true, kind: "primary" },
    ],
    cancelValue: false,
  });
}

/**
 * A web link clicked in a terminal: shown in full (its host apart, so a
 * look-alike stands out) and opened in the browser only once confirmed. The
 * text on screen comes from the server, so it is never opened blindly.
 */
async function openLink(uri: string): Promise<void> {
  let url: URL;
  try {
    url = new URL(uri);
  } catch {
    return;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return;
  // "https://bank.example@evil.example" goes to evil.example: never opened.
  if (url.username || url.password) {
    await alertBox(t("openLinkTitle"), t("openLinkCredentials", { host: url.host }));
    return;
  }
  const ok = await modal<boolean>({
    title: t("openLinkTitle"),
    body: [
      h("p", {}, t("openLinkHost", { host: url.host })),
      h("p", { class: "mono link-preview" }, url.href),
      h("p", { class: "muted" }, t("openLinkHelp")),
    ],
    buttons: [
      { label: t("cancel"), value: false },
      { label: t("openLink"), value: true, kind: "primary" },
    ],
    cancelValue: false,
    // Enter alone does not open it.
    focusButton: 0,
  });
  if (!ok) return;
  try {
    await api.openUrl(url.href);
  } catch (e) {
    await alertBox(t("openLinkTitle"), errorText(e));
  }
}

const RED = "\x1b[31m";
const YELLOW = "\x1b[33m";
const CYAN = "\x1b[36m";
const RESET = "\x1b[0m";

/** Seconds before each automatic reconnection attempt; the last repeats. */
const RETRY_DELAYS = [2, 5, 10, 20, 30];
/** Automatic attempts before giving up (about five minutes in all). */
const MAX_ATTEMPTS = 12;

interface TerminalOptions extends PaneOptions {
  vaultAvailable: boolean;
  /** Output arrived while `watchOutput` was set (it is then cleared). */
  onOutput?: (pane: TerminalPane) => void;
  /** The program rang the bell. */
  onBell?: (pane: TerminalPane) => void;
}

/** One terminal session with its xterm.js view. */
export class TerminalPane implements Pane {
  readonly el: HTMLElement;
  readonly term: Terminal;
  state: PaneState = "connecting";
  waiting = false;
  private readonly fit = new FitAddon();
  private readonly searchAddon = new SearchAddon({ highlightLimit: HIGHLIGHT_LIMIT });
  private readonly searchBar: SearchBar;
  private sessionId: number | null = null;
  /** Aborted when the pane closes or reconnects: closes its pending dialogs. */
  private prompts = new AbortController();
  /** Bumped on every (re)connect so late events of an old session are ignored. */
  private generation = 0;
  /** Set while the pane's tab is hidden: the next output is reported once. */
  watchOutput = false;
  /** Bumped when a dropped connection starts (or stops) being reconnected. */
  private reconnectRun = 0;
  /** While waiting before a reconnection attempt: Enter tries now, Esc stops. */
  private retryWait: ((go: boolean) => void) | null = null;
  private readonly resizeObserver: ResizeObserver;
  private readonly unsubscribePrefs: () => void;
  private disposed = false;
  /** Window title set by the remote shell (often "user@host: dir"); untrusted. */
  remoteTitle = "";
  /** Current folder reported by the shell with OSC 7 (absolute); untrusted. */
  remoteCwd: string | null = null;
  /** Which of the two the shell updated last (some shells send both). */
  cwdIsNewer = false;
  private readonly titleListeners = new Set<(title: string) => void>();

  constructor(readonly opts: TerminalOptions) {
    this.el = h("div", { class: "pane terminal-host" });
    this.term = new Terminal({
      allowProposedApi: true,
      // Option must keep typing @ # [ ] { } on Italian, German… Mac layouts.
      macOptionIsMeta: false,
      // Right click copies an existing selection, otherwise pastes: selecting
      // the word under the pointer first would turn every paste into a copy.
      rightClickSelectsWord: false,
      // Links sent by the server (OSC 8) are never opened: no browser window
      // or new web view for a URL the server chose.
      linkHandler: { activate: () => {} },
      ...TerminalPane.prefsOptions(),
    });
    this.term.loadAddon(this.fit);
    this.term.loadAddon(this.searchAddon);
    // Web links in the output: Ctrl+click (Cmd on macOS) asks to open them.
    this.term.loadAddon(
      new WebLinksAddon(
        (e, uri) => {
          if (isMac ? e.metaKey : e.ctrlKey) void openLink(uri);
        },
        {
          hover: () => (this.el.title = t("linkHint", { key: isMac ? "Cmd" : "Ctrl" })),
          leave: () => this.el.removeAttribute("title"),
        },
      ),
    );
    const unicode = new Unicode11Addon();
    this.term.loadAddon(unicode);
    this.term.unicode.activeVersion = "11";
    this.term.open(this.el);
    this.searchBar = new SearchBar(this.term, this.searchAddon);
    this.el.append(this.searchBar.el);

    this.term.attachCustomKeyEventHandler((e) => this.handleKey(e));
    this.term.onData((d) => this.input(d, false));
    this.term.onBinary((d) => this.input(d, true));
    this.term.onBell(() => this.opts.onBell?.(this));
    this.term.onTitleChange((title) => {
      this.remoteTitle = title;
      this.cwdIsNewer = false;
      for (const listener of this.titleListeners) listener(title);
    });
    // OSC 7 "file://host/path": the shell's current folder (vte.sh, fish,
    // zsh setups). Only remembered, used to suggest a folder to browse.
    this.term.parser.registerOscHandler(7, (data) => {
      const cwd = osc7Path(data);
      if (cwd) {
        this.remoteCwd = cwd;
        this.cwdIsNewer = true;
        for (const listener of this.titleListeners) listener(this.remoteTitle);
      }
      return true;
    });
    this.term.onResize(({ cols, rows }) => {
      if (this.sessionId !== null) void api.resizeSession(this.sessionId, { cols, rows }).catch(() => {});
    });
    this.el.addEventListener("contextmenu", (e) => {
      // The find bar keeps the normal text-field menu: nothing is pasted
      // into the session from there.
      if (e.target instanceof Node && this.searchBar.el.contains(e.target)) return;
      // Right click: copy the selection if any, otherwise paste (PuTTY style);
      // with copy on select the selection is already copied, so it pastes.
      e.preventDefault();
      if (this.term.hasSelection() && !getPrefs().copyOnSelect) void this.copy();
      else void this.paste();
    });

    // Copy on select (Settings): what the mouse selected goes to the clipboard.
    this.el.addEventListener("mouseup", (e) => {
      if (e.button !== 0 || !getPrefs().copyOnSelect || !this.term.hasSelection()) return;
      if (e.target instanceof Node && this.searchBar.el.contains(e.target)) return;
      void this.copy();
    });

    // Every paste (keys, menu, the system's own Ctrl/Cmd+V) goes through paste().
    this.el.addEventListener(
      "paste",
      (e) => {
        if (e.target instanceof Node && this.searchBar.el.contains(e.target)) return;
        e.preventDefault();
        e.stopPropagation();
        void this.paste(e.clipboardData?.getData("text/plain") ?? "");
      },
      true,
    );

    this.term.textarea?.addEventListener("focus", () => this.opts.onFocus(this));
    // Focus in the find bar also makes this the active pane.
    this.searchBar.el.addEventListener("focusin", () => this.opts.onFocus(this));

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
      this.term.write(`${DIM}${plain(t("connecting", { target: this.opts.subtitle }))}${RESET}\r\n`);
    }
    try {
      const id = await api.openSession(
        this.opts.target,
        { cols: this.term.cols, rows: this.term.rows },
        (bytes) => {
          if (gen !== this.generation) return;
          this.term.write(bytes);
          if (this.watchOutput) {
            this.watchOutput = false;
            this.opts.onOutput?.(this);
          }
        },
        (notice) => {
          if (gen === this.generation) void this.onNotice(notice);
          else void this.rejectStalePrompt(notice);
        },
      );
      if (gen !== this.generation || this.disposed || this.state === "closed") {
        // Not wanted any more, or it already ended (its notice came first).
        void api.closeSession(id).catch(() => {});
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

  /** Opens the find bar. */
  find(): void {
    this.searchBar.open();
  }

  /** The backend session id while connected (for SFTP on the same connection). */
  get session(): number | null {
    return this.sessionId;
  }

  /** The file this terminal's output is logged to, while it is. */
  logPath: string | null = null;

  /**
   * Logs what this terminal shows to a file chosen in the save dialog, after
   * saying that the log may hold secrets shown on screen.
   */
  async startLog(): Promise<void> {
    const id = this.sessionId;
    if (id === null || this.logPath) return;
    if (!(await confirmBox(t("logConfirm"), t("logChoose")))) return;
    try {
      const path = await api.sessionLogStart(id, this.opts.title);
      if (!path || this.sessionId !== id) return;
      this.logPath = path;
      this.term.write(`\r\n${CYAN}» ${plain(t("logStarted", { path }))}${RESET}\r\n`);
      this.opts.onStateChange(this);
    } catch (e) {
      this.term.write(`\r\n${RED}${plain(errorText(e))}${RESET}\r\n`);
    }
  }

  /** Empties the screen and the scrollback, keeping the line the cursor is on. */
  clearScreen(): void {
    this.term.clear();
    this.term.focus();
  }

  /** Shows a line of Termdock's own in the terminal (not sent to the session). */
  notify(text: string, error = false): void {
    this.term.write(`\r\n${error ? RED : CYAN}» ${plain(text)}${RESET}\r\n`);
  }

  async stopLog(): Promise<void> {
    const path = this.logPath;
    if (!path) return;
    this.logPath = null;
    if (this.sessionId !== null) await api.sessionLogStop(this.sessionId).catch(() => false);
    this.term.write(`\r\n${CYAN}» ${plain(t("logStopped", { path }))}${RESET}\r\n`);
    this.opts.onStateChange(this);
  }

  refit(): void {
    if (this.disposed || this.el.offsetParent === null) return;
    try {
      this.fit.fit();
    } catch {
      /* not visible yet */
    }
  }

  /** Calls `listener` when the remote shell changes its title or folder; returns an unsubscribe. */
  onRemoteTitle(listener: (title: string) => void): () => void {
    this.titleListeners.add(listener);
    return () => this.titleListeners.delete(listener);
  }

  relabel(): void {
    this.searchBar.relabel();
  }

  dispose(): void {
    this.disposed = true;
    this.titleListeners.clear();
    this.generation++;
    this.retryWait?.(false);
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

  /**
   * Pastes `text` (default: the clipboard) as typed input. Escape and other
   * control characters are dropped, so pasted text cannot end bracketed paste
   * early or send key sequences; several lines are confirmed first when the
   * program does not use bracketed paste (each line would run at once).
   */
  /** Returns whether the text was sent. */
  async paste(text?: string): Promise<boolean> {
    const raw = text ?? (await readText().catch(() => ""));
    const clean = cleanPaste(raw);
    if (!clean || this.disposed) return false;
    const lines = clean.replace(/[\r\n]+$/, "").split(/\r\n|\r|\n/).length;
    if (lines > 1 && !this.term.modes.bracketedPasteMode) {
      const ok = await confirmPaste(clean, lines);
      if (!ok || this.disposed) return false;
    }
    this.term.paste(clean);
    this.term.focus();
    return true;
  }

  /**
   * Types a snippet like a paste (control characters removed, several lines
   * confirmed when the program would run each one), then Enter if `run`.
   */
  async sendSnippet(text: string, run: boolean): Promise<void> {
    if (this.state !== "open") return;
    const body = run ? text.replace(/[\r\n]+$/, "") : text;
    if ((await this.paste(body)) && run && this.state === "open") this.input("\r", false);
  }

  /** `binary`: xterm.js delivers raw bytes (some mouse reports) as one char per byte. */
  private input(data: string, binary: boolean): void {
    if (this.retryWait) {
      if (data === "\r") this.retryWait(true);
      else if (data === "\x1b" || data === "\x03") this.retryWait(false);
      return;
    }
    if (this.state === "closed") {
      if (data === "\r") void this.connect();
      return;
    }
    if (this.sessionId !== null) void api.writeSession(this.sessionId, data, binary).catch(() => {});
  }

  private handleKey(e: KeyboardEvent): boolean {
    if (e.type !== "keydown") return true;
    const action = actionFor(e);
    if (action === "copy") {
      void this.copy();
      return false;
    }
    if (action === "paste") {
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
        await this.untilShown();
        const accept = await confirmHostKey(n.info, this.prompts.signal);
        await api.answerPrompt(n.promptId, { kind: "hostKey", accept });
        break;
      }
      case "secret": {
        await this.untilShown();
        // Someone typing in another pane keeps typing there, not into this field.
        const active = document.activeElement;
        const elsewhere = active instanceof HTMLElement && active !== document.body && !this.el.contains(active);
        const res = await askSecret(n.prompt, this.opts.vaultAvailable, this.prompts.signal, elsewhere);
        const answer: PromptAnswer = res ? { kind: "secret", ...res } : { kind: "cancel" };
        await api.answerPrompt(n.promptId, answer);
        break;
      }
      case "exit": {
        if (n.lost && this.sessionId !== null) {
          void this.reconnect(this.sessionId);
          break;
        }
        const detail = [n.code !== null ? t("exitCode", { code: n.code }) : null, n.message && plain(n.message)]
          .filter(Boolean)
          .join(", ");
        this.term.write(`\r\n${DIM}[${t("sessionEnded")}${detail ? `: ${detail}` : ""}] ${keyHint("closePane") ? t("pressEnterReconnect", { key: keyHint("closePane") }) : t("pressEnterReconnectOnly")}${RESET}\r\n`);
        this.ended();
        break;
      }
    }
  }

  /**
   * The connection of session `id` dropped: connects it again under the same
   * id, by itself after a pause (Enter: now, Esc: stop) if the preference is
   * on, otherwise when Enter is pressed. Gives up on errors that need the
   * user (authentication, host key) or after MAX_ATTEMPTS.
   */
  private async reconnect(id: number): Promise<void> {
    const run = ++this.reconnectRun;
    const gen = this.generation;
    const stale = () => run !== this.reconnectRun || gen !== this.generation || this.disposed;
    const auto = getPrefs().autoReconnect;
    this.setState("connecting");
    this.term.write(`\r\n${YELLOW}[${t("connectionLost")}]${RESET}\r\n`);
    for (let attempt = 1; ; attempt++) {
      const delay = RETRY_DELAYS[Math.min(attempt - 1, RETRY_DELAYS.length - 1)]!;
      this.term.write(
        `${DIM}${auto ? t("reconnectIn", { seconds: delay, attempt, max: MAX_ATTEMPTS }) : t("reconnectPrompt")}${RESET}\r\n`,
      );
      const go = await this.waitRetry(auto ? delay : null);
      if (stale()) return;
      if (!go) return this.giveUp(id, t("reconnectStopped"));
      this.term.write(`${DIM}${plain(t("connecting", { target: this.opts.subtitle }))}${RESET}\r\n`);
      let res: Reconnected;
      try {
        res = await api.reconnectSession(id, { cols: this.term.cols, rows: this.term.rows });
      } catch (e) {
        res = { status: "failed", error: errorText(e), retry: false };
      }
      if (stale()) return;
      switch (res.status) {
        case "connected":
          this.term.write(`${CYAN}» ${t("reconnected")}${RESET}\r\n`);
          this.setState("open");
          this.refit();
          void api.resizeSession(id, { cols: this.term.cols, rows: this.term.rows }).catch(() => {});
          return;
        case "closed":
          // Its end was already reported (or the pane closed it).
          if (this.state !== "closed" && this.sessionId === id) this.ended();
          return;
        case "failed":
          this.term.write(`${RED}${plain(t("connectionFailed", { error: res.error }))}${RESET}\r\n`);
          if (!res.retry || (auto && attempt >= MAX_ATTEMPTS)) return this.giveUp(id, t("reconnectGaveUp"));
      }
    }
  }

  /** Resolves true after `seconds` (never if null) or on Enter, false on Esc or when the pane closes. */
  private waitRetry(seconds: number | null): Promise<boolean> {
    return new Promise((resolve) => {
      const timer = seconds === null ? undefined : setTimeout(() => done(true), seconds * 1000);
      const done = (go: boolean) => {
        clearTimeout(timer);
        this.retryWait = null;
        resolve(go);
      };
      this.retryWait = done;
    });
  }

  /** Stops reconnecting: the session (and its log) ends; Enter opens a new one. */
  private giveUp(id: number, reason: string): void {
    void api.closeSession(id).catch(() => {});
    this.term.write(`${DIM}[${reason}] ${t("pressEnterReconnectOnly")}${RESET}\r\n`);
    this.ended();
  }

  /**
   * Waits until this pane is on screen (its tab is the active one), so a
   * question from a session in a background tab never pops up over another
   * tab, where keys typed for that tab would answer it.
   */
  private async untilShown(): Promise<void> {
    const signal = this.prompts.signal;
    if (this.el.offsetParent !== null) return;
    // Its tab shows a mark meanwhile, so the question is not missed.
    this.setWaiting(true);
    try {
      while (!this.disposed && !signal.aborted && this.el.offsetParent === null) {
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
    } finally {
      this.setWaiting(false);
    }
  }

  private setWaiting(waiting: boolean): void {
    if (this.waiting === waiting) return;
    this.waiting = waiting;
    if (!this.disposed) this.opts.onStateChange(this);
  }

  private async rejectStalePrompt(n: SessionNotice): Promise<void> {
    if (n.type === "hostKey" || n.type === "secret") {
      await api.answerPrompt(n.promptId, { kind: "cancel" }).catch(() => {});
    }
  }

  private ended(): void {
    this.reconnectRun++;
    this.retryWait?.(false);
    this.sessionId = null;
    // The backend closed the log with the session.
    this.logPath = null;
    this.setState("closed");
  }

  private setState(s: PaneState): void {
    this.state = s;
    this.opts.onStateChange(this);
  }
}
