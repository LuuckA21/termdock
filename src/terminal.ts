import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { Terminal } from "@xterm/xterm";

import { api, errorText } from "./api";
import { askSecret, confirmHostKey } from "./dialogs";
import { h } from "./dom";
import { t } from "./i18n";
import type { OpenTarget, PromptAnswer, SessionNotice } from "./types";

export type TabState = "connecting" | "open" | "closed";

const THEME = {
  background: "#0f1420",
  foreground: "#d7dce8",
  cursor: "#ffd166",
  cursorAccent: "#0f1420",
  selectionBackground: "#3d5af166",
  black: "#1b2130",
  red: "#ff6b6b",
  green: "#7ee0b0",
  yellow: "#ffd166",
  blue: "#6c8cff",
  magenta: "#c792ea",
  cyan: "#5fd7e0",
  white: "#d7dce8",
  brightBlack: "#5a6378",
  brightRed: "#ff8787",
  brightGreen: "#9ff0c8",
  brightYellow: "#ffe08a",
  brightBlue: "#8fa8ff",
  brightMagenta: "#ddb3ff",
  brightCyan: "#8ae9ef",
  brightWhite: "#ffffff",
};

const DIM = "\x1b[2m";
const RED = "\x1b[31m";
const CYAN = "\x1b[36m";
const RESET = "\x1b[0m";

export interface TabOptions {
  title: string;
  subtitle: string;
  color?: string;
  target: OpenTarget;
  vaultAvailable: boolean;
  onStateChange: (tab: TerminalTab) => void;
}

export class TerminalTab {
  readonly el: HTMLElement;
  readonly term: Terminal;
  state: TabState = "connecting";
  private readonly fit = new FitAddon();
  private sessionId: number | null = null;
  /** Bumped on every (re)connect so late events of an old session are ignored. */
  private generation = 0;
  private readonly resizeObserver: ResizeObserver;
  private disposed = false;

  constructor(readonly opts: TabOptions) {
    this.el = h("div", { class: "terminal-host", role: "tabpanel" });
    this.term = new Terminal({
      allowProposedApi: true,
      cursorBlink: true,
      fontFamily: '"Cascadia Mono", "JetBrains Mono", "SF Mono", Menlo, Consolas, "DejaVu Sans Mono", monospace',
      fontSize: 14,
      lineHeight: 1.1,
      scrollback: 10000,
      theme: THEME,
      macOptionIsMeta: true,
      rightClickSelectsWord: true,
    });
    this.term.loadAddon(this.fit);
    const unicode = new Unicode11Addon();
    this.term.loadAddon(unicode);
    this.term.unicode.activeVersion = "11";
    this.term.open(this.el);

    this.term.attachCustomKeyEventHandler((e) => this.handleKey(e));
    this.term.onData((d) => this.input(d));
    this.term.onBinary((d) => this.input(d));
    this.term.onResize(({ cols, rows }) => {
      if (this.sessionId !== null) void api.resizeSession(this.sessionId, { cols, rows }).catch(() => {});
    });
    this.el.addEventListener("contextmenu", (e) => {
      // Right click: copy the selection if any, otherwise paste (PuTTY style).
      e.preventDefault();
      if (this.term.hasSelection()) void this.copy();
      else void this.paste();
    });

    this.resizeObserver = new ResizeObserver(() => this.refit());
    this.resizeObserver.observe(this.el);
  }

  get title(): string {
    return this.opts.title;
  }

  async connect(): Promise<void> {
    const gen = ++this.generation;
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
      this.term.write(`\r\n${RED}${t("connectionFailed", { error: errorText(e) })}${RESET}\r\n`);
      this.ended();
    }
  }

  focus(): void {
    this.term.focus();
    this.refit();
  }

  refit(): void {
    if (this.disposed || this.el.offsetParent === null) return;
    try {
      this.fit.fit();
    } catch {
      /* not visible yet */
    }
  }

  dispose(): void {
    this.disposed = true;
    this.generation++;
    this.resizeObserver.disconnect();
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

  private input(data: string): void {
    if (this.state === "closed") {
      if (data === "\r") void this.connect();
      return;
    }
    if (this.sessionId !== null) void api.writeSession(this.sessionId, data).catch(() => {});
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
    // Let the app-level shortcuts through (handled in main.ts).
    if (mod && e.shiftKey && /^(KeyT|KeyW|KeyK|KeyM|Digit\d)$/.test(e.code)) return false;
    if (e.ctrlKey && e.code === "Tab") return false;
    if (e.altKey && !mod && /^Digit[1-9]$/.test(e.code)) return false;
    return true;
  }

  private async onNotice(n: SessionNotice): Promise<void> {
    switch (n.type) {
      case "banner":
        this.term.write(`${DIM}${n.text.replace(/\r?\n/g, "\r\n")}${RESET}\r\n`);
        break;
      case "info":
        this.term.write(`${CYAN}» ${n.text.replace(/[\x00-\x1f\x7f]/g, " ")}${RESET}\r\n`);
        break;
      case "hostKey": {
        const accept = await confirmHostKey(n.info);
        await api.answerPrompt(n.promptId, { kind: "hostKey", accept });
        break;
      }
      case "secret": {
        const res = await askSecret(n.prompt, this.opts.vaultAvailable);
        const answer: PromptAnswer = res ? { kind: "secret", ...res } : { kind: "cancel" };
        await api.answerPrompt(n.promptId, answer);
        break;
      }
      case "exit": {
        const detail = [n.code !== null ? t("exitCode", { code: n.code }) : null, n.message]
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

  private setState(s: TabState): void {
    this.state = s;
    this.opts.onStateChange(this);
  }
}
