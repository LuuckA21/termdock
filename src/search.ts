// Find bar of a terminal: searches the scrollback with
// highlights, next/previous, match case and regular expressions.
import type { ISearchOptions, SearchAddon } from "@xterm/addon-search";
import type { Terminal } from "@xterm/xterm";

import { h, icon } from "./dom";
import { t } from "./i18n";

/** Matches highlighted (and counted) at most; beyond it the count shows "+". */
export const HIGHLIGHT_LIMIT = 1000;

/** Highlights: decorations need #RRGGBB colours. */
const DECORATIONS = {
  matchBackground: "#6b5a12",
  matchOverviewRuler: "#c9a227",
  activeMatchBackground: "#c27c0e",
  activeMatchColorOverviewRuler: "#ff9f1c",
};

export class SearchBar {
  readonly el: HTMLElement;
  private readonly input: HTMLInputElement;
  private readonly count: HTMLElement;
  private readonly caseBtn: HTMLButtonElement;
  private readonly regexBtn: HTMLButtonElement;
  private caseSensitive = false;
  private regex = false;

  constructor(
    private readonly term: Terminal,
    private readonly addon: SearchAddon,
  ) {
    this.input = h("input", {
      class: "input search-input",
      spellcheck: "false",
      autocomplete: "off",
      placeholder: t("searchPlaceholder"),
      "aria-label": t("searchPlaceholder"),
    });
    this.count = h("span", { class: "search-count muted", "aria-live": "polite" });
    const toggle = (label: string, title: string, onClick: () => void) => {
      const b = h("button", { type: "button", class: "search-toggle", title, "aria-label": title, "aria-pressed": "false" }, label);
      b.addEventListener("click", () => {
        onClick();
        this.input.focus();
      });
      return b;
    };
    this.caseBtn = toggle("Aa", t("searchCase"), () => {
      this.caseSensitive = !this.caseSensitive;
      this.caseBtn.setAttribute("aria-pressed", String(this.caseSensitive));
      this.restart();
    });
    this.regexBtn = toggle(".*", t("searchRegex"), () => {
      this.regex = !this.regex;
      this.regexBtn.setAttribute("aria-pressed", String(this.regex));
      this.restart();
    });
    const button = (name: "arrowUp" | "chevron" | "close", title: string, onClick: () => void) => {
      const b = h("button", { type: "button", class: "icon-btn", title, "aria-label": title }, icon(name));
      b.addEventListener("click", onClick);
      return b;
    };
    const prev = button("arrowUp", `${t("searchPrevious")} (Shift+Enter)`, () => this.find(false, false));
    const next = button("chevron", `${t("searchNext")} (Enter)`, () => this.find(true, false));
    const close = button("close", `${t("close")} (Esc)`, () => this.close());

    this.el = h("div", { class: "search-bar", role: "search", hidden: true }, this.input, this.count, this.caseBtn, this.regexBtn, prev, next, close);
    this.input.addEventListener("input", () => this.find(true, true));
    this.input.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.isComposing) {
        e.preventDefault();
        this.find(!e.shiftKey, false);
      }
    });
    // Esc from anywhere in the bar (input or its buttons).
    this.el.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && !e.isComposing) {
        e.preventDefault();
        this.close();
      }
    });
    addon.onDidChangeResults(({ resultIndex, resultCount }) => this.showCount(resultIndex, resultCount));
  }

  get isOpen(): boolean {
    return !this.el.hidden;
  }

  /** Opens (or refocuses) the bar, starting from the terminal's selection if any. */
  open(): void {
    const selection = this.term.getSelection();
    if (selection && !selection.includes("\n") && selection.length <= 200) this.input.value = selection;
    this.el.hidden = false;
    this.input.focus();
    this.input.select();
    if (this.input.value) this.find(true, true);
  }

  close(): void {
    if (this.el.hidden) return;
    this.el.hidden = true;
    this.addon.clearDecorations();
    this.term.clearSelection();
    this.term.focus();
  }

  /** Options changed: search again from scratch, so the count is recomputed. */
  private restart(): void {
    this.addon.clearDecorations();
    this.term.clearSelection();
    this.find(true, false);
  }

  private options(incremental: boolean): ISearchOptions {
    return { caseSensitive: this.caseSensitive, regex: this.regex, incremental, decorations: DECORATIONS };
  }

  private find(forward: boolean, incremental: boolean): void {
    const text = this.input.value;
    this.input.classList.remove("invalid");
    if (!text) {
      this.addon.clearDecorations();
      this.term.clearSelection();
      this.count.textContent = "";
      return;
    }
    if (this.regex) {
      try {
        new RegExp(text);
      } catch {
        // Drop the previous highlights: the addon would keep refreshing them.
        this.addon.clearDecorations();
        this.term.clearSelection();
        this.input.classList.add("invalid");
        this.count.textContent = t("searchBadRegex");
        return;
      }
    }
    const found = forward
      ? this.addon.findNext(text, this.options(incremental))
      : this.addon.findPrevious(text, this.options(false));
    if (!found) this.count.textContent = t("searchNone");
  }

  private showCount(index: number, count: number): void {
    if (!this.input.value) return;
    const total = count >= HIGHLIGHT_LIMIT ? t("searchMany", { count: HIGHLIGHT_LIMIT }) : String(count);
    if (count === 0) this.count.textContent = t("searchNone");
    else if (index < 0) this.count.textContent = total;
    else this.count.textContent = `${index + 1}/${total}`;
  }
}
