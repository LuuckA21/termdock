// The transfer queue of a file browser: uploads and downloads run one at a
// time, in order, listed under the files with their progress, a Cancel
// button and how they ended. Names come from the server or the computer and
// are only ever shown as text (`displayName`).
import { api, errorText } from "./api";
import { displayName, h, icon, setLabel } from "./dom";
import { t } from "./i18n";
import type { RunProgress, RunSummary } from "./types";

export interface Job {
  kind: "upload" | "download";
  /** What the row says, already safe to show. */
  title: string;
  plan: number;
  /** Replace what already exists (otherwise it is kept). */
  replace: boolean;
  /** Left out when planned: links, special files, unusable names. */
  leftOut: number;
  /** Called once it ended, whatever the outcome. */
  onEnd?: () => void;
}

type State = "queued" | "running" | "done" | "failed" | "cancelled";

interface Row {
  job: Job;
  state: State;
  transfer: number | null;
  last: RunProgress | null;
  /** How it ended; a function so it follows a language change. */
  outcome: (() => string) | null;
  el: HTMLElement;
  bar: HTMLProgressElement;
  detail: HTMLElement;
  button: HTMLButtonElement;
}

/** Finished rows kept in the list (the oldest go first). */
const MAX_FINISHED = 20;

const finished = (r: Row) => r.state === "done" || r.state === "failed" || r.state === "cancelled";

function describe(s: RunSummary, leftOut: number): string {
  const parts = [t("queueDone", { count: s.copied })];
  if (s.skipped) parts.push(t("queueSkipped", { count: s.skipped }));
  if (leftOut) parts.push(t("queueLeftOut", { count: leftOut }));
  if (s.failed) parts.push(t("queueFailed", { count: s.failed }));
  if (s.stopped) parts.push(t("queueStopped"));
  return parts.join(", ");
}

export class TransferQueue {
  readonly el: HTMLElement;
  private readonly list: HTMLElement;
  private readonly heading: HTMLElement;
  private readonly clearBtn: HTMLButtonElement;
  private rows: Row[] = [];
  private running = false;
  private disposed = false;

  /** `onChange`: a transfer was added or ended. */
  constructor(private readonly onChange: () => void) {
    this.heading = h("span", { class: "files-queue-title" });
    this.clearBtn = h("button", { type: "button", class: "btn btn-ghost" });
    this.clearBtn.addEventListener("click", () => {
      for (const row of this.rows.filter(finished)) this.remove(row);
    });
    this.list = h("div", { class: "files-queue-list", role: "list" });
    this.el = h(
      "div",
      { class: "files-queue", hidden: true },
      h("div", { class: "files-queue-head" }, this.heading, this.clearBtn),
      this.list,
    );
    this.relabel();
  }

  /** Some transfer is waiting or running. */
  get active(): boolean {
    return this.rows.some((r) => !finished(r));
  }

  add(job: Job): void {
    const bar = h("progress", { max: "1", hidden: true });
    const detail = h("span", { class: "files-queue-detail" });
    const button = h("button", { type: "button", class: "icon-btn" }, icon("close"));
    const el = h(
      "div",
      { class: "files-queue-row", role: "listitem" },
      icon(job.kind === "upload" ? "upload" : "download"),
      h("span", { class: "files-queue-name" }, job.title),
      bar,
      detail,
      button,
    );
    const row: Row = { job, state: "queued", transfer: null, last: null, outcome: null, el, bar, detail, button };
    button.addEventListener("click", () => this.stop(row));
    this.rows.push(row);
    this.list.append(el);
    this.render(row);
    this.update();
    void this.pump();
  }

  /** The browser goes away: the running transfer is cancelled, waiting ones dropped. */
  dispose(): void {
    this.disposed = true;
    for (const row of this.rows) {
      if (row.state === "queued") void api.sftpDropPlan(row.job.plan).catch(() => {});
      if (row.state === "running" && row.transfer !== null) void api.sftpCancel(row.transfer).catch(() => {});
    }
  }

  relabel(): void {
    this.heading.textContent = t("queueTitle");
    this.clearBtn.textContent = t("queueClear");
    for (const row of this.rows) this.render(row);
  }

  private stop(row: Row): void {
    if (row.state === "queued") {
      void api.sftpDropPlan(row.job.plan).catch(() => {});
      this.finish(row, "cancelled", () => t("queueCancelled"));
    } else if (row.state === "running") {
      if (row.transfer !== null) void api.sftpCancel(row.transfer).catch(() => {});
    } else {
      this.remove(row);
    }
  }

  private remove(row: Row): void {
    this.rows = this.rows.filter((r) => r !== row);
    row.el.remove();
    this.update();
  }

  private async pump(): Promise<void> {
    if (this.running || this.disposed) return;
    const row = this.rows.find((r) => r.state === "queued");
    if (!row) return;
    this.running = true;
    row.state = "running";
    this.render(row);
    try {
      const summary = await api.sftpRunPlan(row.job.plan, row.job.replace, (p) => this.progress(row, p));
      row.el.title = summary.errors.map((e) => displayName(e)).join("\n");
      this.finish(row, summary.failed ? "failed" : "done", () => describe(summary, row.job.leftOut));
    } catch (e) {
      const text = errorText(e);
      if (/cancelled/i.test(text)) this.finish(row, "cancelled", () => t("queueCancelled"));
      else this.finish(row, "failed", () => t("queueError", { error: displayName(text) }));
    } finally {
      this.running = false;
      row.job.onEnd?.();
      void this.pump();
    }
  }

  private progress(row: Row, p: RunProgress): void {
    row.transfer = p.transfer;
    row.last = p;
    this.render(row);
  }

  private finish(row: Row, state: State, outcome: () => string): void {
    row.state = state;
    row.outcome = outcome;
    this.render(row);
    const done = this.rows.filter(finished);
    for (const old of done.slice(0, Math.max(0, done.length - MAX_FINISHED))) this.remove(old);
    this.update();
  }

  private render(row: Row): void {
    row.el.dataset.state = row.state;
    const p = row.last;
    row.bar.hidden = row.state !== "running";
    if (row.state === "running" && p && p.total > 0) row.bar.value = Math.min(p.done / p.total, 1);
    else row.bar.removeAttribute("value");
    if (row.state === "queued") row.detail.textContent = t("queueWaiting");
    else if (row.state === "running") {
      const percent = p && p.total > 0 ? Math.floor((p.done / p.total) * 100) : 0;
      row.detail.textContent =
        p && p.count > 1 ? t("queueProgress", { files: p.files, count: p.count, percent }) : `${percent}%`;
      // The file being copied, on hover.
      if (p?.current) row.el.title = displayName(p.current);
    } else row.detail.textContent = row.outcome?.() ?? "";
    // The full text when it does not fit, and what "left out" means.
    row.detail.title = [row.detail.textContent, finished(row) && row.job.leftOut ? t("queueLeftOutHelp") : ""]
      .filter(Boolean)
      .join("\n");
    setLabel(row.button, finished(row) ? t("queueRemove") : t("queueCancel"));
  }

  private update(): void {
    this.el.hidden = this.rows.length === 0;
    this.clearBtn.hidden = !this.rows.some(finished);
    this.onChange();
  }
}
