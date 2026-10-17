// File browser pane: SFTP on the SSH connection of a terminal pane.
//
// Remote names are untrusted: they are only ever shown through textContent,
// with control and bidi characters made visible, and sent back to the
// backend as single path components. Local files are chosen in native
// dialogs opened by the backend; this module never handles local paths.
import { api, errorText } from "./api";
import { confirmBox, modal } from "./dialogs";
import { clear, h, icon } from "./dom";
import { t } from "./i18n";
import { showMenu } from "./menu";
import type { Pane, PaneOptions, PaneState } from "./pane";
import type { RemoteEntry, RemoteListing, TransferProgress } from "./types";

/** Control, format (bidi, zero-width, BOM, tags…) and line/paragraph separators. */
const UNSAFE = /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/gu;

/** A server-provided name made safe to display: invisible/reordering characters become U+FFFD. */
export function displayName(name: string): string {
  return name.replace(UNSAFE, "\ufffd");
}

/**
 * Names the server sent in another encoding arrive with U+FFFD: they cannot
 * be named back safely, so they are shown but not touched.
 */
function unusable(entry: RemoteEntry): boolean {
  return entry.name.includes("\ufffd");
}

function formatSize(bytes: number | null): string {
  if (bytes === null) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(1)} ${units[i]}`;
}

function formatMode(mode: number | null, kind: RemoteEntry["kind"]): string {
  if (mode === null) return "";
  const type = kind === "dir" ? "d" : kind === "link" ? "l" : "-";
  const bits = "rwxrwxrwx";
  return type + [...bits].map((c, i) => (mode & (1 << (8 - i)) ? c : "-")).join("");
}

function formatDate(secs: number | null): string {
  return secs === null ? "" : new Date(secs * 1000).toLocaleString();
}

export interface FilesOptions extends PaneOptions {
  /** Backend id of the terminal session whose connection is used. */
  session: number;
  /** The terminal pane that owns that session. */
  source: Pane;
}

export class FilesPane implements Pane {
  readonly el: HTMLElement;
  state: PaneState = "connecting";
  private id: number | null = null;
  private home = "/";
  private listing: RemoteListing | null = null;
  private selected: string | null = null;
  /** Set while a download or upload runs, including its file dialog. */
  private busy = false;
  private transfer: number | null = null;
  private disposed = false;

  private readonly pathInput: HTMLInputElement;
  private readonly list: HTMLElement;
  private readonly status: HTMLElement;
  private readonly progress: HTMLProgressElement;
  private readonly cancelBtn: HTMLButtonElement;
  private readonly actions: HTMLButtonElement[] = [];

  constructor(readonly opts: FilesOptions) {
    this.pathInput = h("input", {
      class: "input files-path",
      spellcheck: "false",
      autocomplete: "off",
      "aria-label": t("filesPath"),
    });
    this.pathInput.addEventListener("keydown", (e) => {
      if (e.key !== "Enter") return;
      // Unchanged text is the displayed form of the current path: use the real one.
      const listing = this.listing;
      const typed = this.pathInput.value;
      void this.navigate(listing && typed === displayName(listing.path) ? listing.path : typed);
    });
    this.list = h("div", { class: "files-list", role: "listbox", tabindex: "0", "aria-label": t("files") });
    this.list.addEventListener("keydown", (e) => this.onKey(e));
    this.list.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      e.stopPropagation();
      this.menu(e.clientX, e.clientY, null);
    });
    this.status = h("span", { class: "files-status-text", role: "status" });
    this.progress = h("progress", { max: "1", hidden: true });
    this.cancelBtn = h("button", { type: "button", class: "btn btn-ghost", hidden: true }, t("cancel"));
    this.cancelBtn.addEventListener("click", () => {
      if (this.transfer !== null) void api.sftpCancel(this.transfer);
    });

    const tool = (name: Parameters<typeof icon>[0], label: string, action: () => void) => {
      const b = h("button", { type: "button", class: "icon-btn", title: label, "aria-label": label }, icon(name));
      b.addEventListener("click", action);
      this.actions.push(b);
      return b;
    };
    const bar = h(
      "div",
      { class: "files-bar" },
      tool("arrowUp", t("filesUp"), () => void this.up()),
      tool("home", t("filesHome"), () => void this.navigate(this.home)),
      tool("refresh", t("filesRefresh"), () => void this.refresh()),
      this.pathInput,
      tool("folderPlus", t("filesNewFolder"), () => void this.mkdir()),
      tool("upload", t("filesUpload"), () => void this.upload()),
    );
    this.el = h(
      "div",
      { class: "pane files-pane" },
      bar,
      this.list,
      h("div", { class: "files-status" }, this.status, this.progress, this.cancelBtn),
    );
    this.el.addEventListener("focusin", () => this.opts.onFocus(this));
    this.setBusy(true);
    void this.open();
  }

  get title(): string {
    return t("filesTitle", { name: this.opts.title });
  }

  focus(): void {
    this.list.focus();
  }

  blur(): void {
    (document.activeElement as HTMLElement | null)?.blur?.();
  }

  dispose(): void {
    this.disposed = true;
    if (this.transfer !== null) void api.sftpCancel(this.transfer);
    if (this.id !== null) void api.sftpClose(this.id).catch(() => {});
    this.el.remove();
  }

  /** The terminal's session ended: this connection is gone. */
  sessionEnded(): void {
    if (this.state === "closed") return;
    this.id = null;
    this.setState("closed");
    this.setBusy(true);
    this.pathInput.disabled = true;
    this.say(t("filesClosed"), true);
  }

  private async open(): Promise<void> {
    this.say(t("filesOpening"));
    try {
      const { id, home } = await api.sftpOpen(this.opts.session);
      if (this.disposed) {
        void api.sftpClose(id);
        return;
      }
      this.id = id;
      this.home = home;
      this.setState("open");
      this.setBusy(false);
      await this.navigate(home);
    } catch (e) {
      this.fail(e);
    }
  }

  private setState(s: PaneState): void {
    this.state = s;
    this.opts.onStateChange(this);
  }

  /** Shows an error; failing to open closes the browser for good. */
  private fail(e: unknown): void {
    if (this.id === null) {
      this.setState("closed");
      this.setBusy(true);
      this.pathInput.disabled = true;
    }
    this.say(displayName(errorText(e)), true);
  }

  private say(text: string, error = false): void {
    this.status.textContent = text;
    this.status.classList.toggle("error", error);
  }

  private setBusy(busy: boolean): void {
    for (const b of this.actions) b.disabled = busy;
  }

  private async navigate(path: string): Promise<void> {
    if (this.id === null) return;
    try {
      const listing = await api.sftpList(this.id, path.trim() || this.home);
      this.listing = listing;
      this.selected = null;
      this.pathInput.value = displayName(listing.path);
      this.render();
      this.say(
        listing.truncated
          ? t("filesTruncated", { count: listing.entries.length })
          : t("filesCount", { count: listing.entries.length }),
      );
    } catch (e) {
      this.fail(e);
    }
  }

  private refresh(): Promise<void> {
    return this.navigate(this.listing?.path ?? this.home);
  }

  private up(): Promise<void> {
    const path = this.listing?.path ?? this.home;
    return this.navigate(path === "/" ? "/" : `${path.replace(/\/+$/, "")}/..`);
  }

  private render(): void {
    clear(this.list);
    const entries = this.listing?.entries ?? [];
    if (!entries.length) {
      this.list.append(h("div", { class: "files-empty muted" }, t("filesEmpty")));
      return;
    }
    for (const entry of entries) {
      const name = displayName(entry.name);
      const kindIcon = entry.kind === "dir" ? "folder" : entry.kind === "link" ? "link" : "file";
      const row = h(
        "div",
        {
          class: `files-row${entry.name === this.selected ? " selected" : ""}`,
          role: "option",
          "aria-selected": String(entry.name === this.selected),
          title: name,
        },
        icon(kindIcon),
        h("span", { class: "files-name" }, name),
        h("span", { class: "files-size muted" }, entry.kind === "dir" ? "" : formatSize(entry.size)),
        h("span", { class: "files-date muted" }, formatDate(entry.modified)),
        h("span", { class: "files-mode muted mono" }, formatMode(entry.mode, entry.kind)),
      );
      row.addEventListener("click", () => this.select(entry.name));
      row.addEventListener("dblclick", () => void this.activate(entry));
      row.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.select(entry.name);
        this.menu(e.clientX, e.clientY, entry);
      });
      this.list.append(row);
    }
  }

  private select(name: string | null): void {
    this.selected = name;
    const rows = this.list.querySelectorAll<HTMLElement>(".files-row");
    const entries = this.listing?.entries ?? [];
    rows.forEach((row, i) => {
      const on = entries[i]?.name === name;
      row.classList.toggle("selected", on);
      row.setAttribute("aria-selected", String(on));
      if (on) row.scrollIntoView({ block: "nearest" });
    });
  }

  private current(): RemoteEntry | null {
    return this.listing?.entries.find((e) => e.name === this.selected) ?? null;
  }

  /** Double click / Enter: folders open, files download; links are tried as folders first. */
  private async activate(entry: RemoteEntry): Promise<void> {
    if (!this.listing) return;
    if (unusable(entry)) {
      this.say(t("filesUnusableName"), true);
      return;
    }
    const path = `${this.listing.path.replace(/\/+$/, "")}/${entry.name}`;
    if (entry.kind === "dir") return this.navigate(path);
    if (entry.kind === "link" && this.id !== null) {
      try {
        const listing = await api.sftpList(this.id, path);
        this.listing = listing;
        this.selected = null;
        this.pathInput.value = displayName(listing.path);
        this.render();
        return;
      } catch {
        // Not a directory: download it.
      }
    }
    if (entry.kind !== "other") await this.download(entry);
  }

  private menu(x: number, y: number, entry: RemoteEntry | null): void {
    if (this.state !== "open") return;
    const items = [];
    if (entry && !unusable(entry)) {
      items.push({
        label: entry.kind === "dir" ? t("filesOpen") : t("filesDownload"),
        icon: icon(entry.kind === "dir" ? "folder" : "download"),
        action: () => void this.activate(entry),
      });
      items.push({ label: t("filesRename"), hint: "F2", action: () => void this.rename(entry) });
      items.push({ label: t("delete"), danger: true, hint: "Del", action: () => void this.remove(entry) });
    }
    items.push({ label: t("filesNewFolder"), icon: icon("folderPlus"), action: () => void this.mkdir() });
    items.push({ label: t("filesUpload"), icon: icon("upload"), action: () => void this.upload() });
    items.push({ label: t("filesRefresh"), icon: icon("refresh"), action: () => void this.refresh() });
    showMenu(x, y, items);
  }

  private onKey(e: KeyboardEvent): void {
    if (this.opts.isAppShortcut(e) || this.state !== "open") return;
    const entries = this.listing?.entries ?? [];
    const i = entries.findIndex((x) => x.name === this.selected);
    const entry = this.current();
    let handled = true;
    if (e.key === "ArrowDown") this.select(entries[Math.min(i + 1, entries.length - 1)]?.name ?? null);
    else if (e.key === "ArrowUp") this.select(entries[Math.max(i - 1, 0)]?.name ?? null);
    else if (e.key === "Home") this.select(entries[0]?.name ?? null);
    else if (e.key === "End") this.select(entries[entries.length - 1]?.name ?? null);
    else if (e.key === "Enter" && entry) void this.activate(entry);
    else if (e.key === "Backspace") void this.up();
    else if (e.key === "F2" && entry && !unusable(entry)) void this.rename(entry);
    else if (e.key === "Delete" && entry && !unusable(entry)) void this.remove(entry);
    else if (e.key === "F5") void this.refresh();
    else handled = false;
    if (handled) e.preventDefault();
  }

  /** Asks for a name; null when cancelled. */
  private askName(title: string, initial = ""): Promise<string | null> {
    const input = h("input", { class: "input", spellcheck: "false", autocomplete: "off", maxlength: 255 });
    input.value = initial;
    return modal<string | null>({
      title,
      body: [h("label", { class: "field" }, h("span", {}, t("name")), input)],
      buttons: [
        { label: t("cancel"), value: null },
        { label: t("ok"), value: null, kind: "primary" },
      ],
      cancelValue: null,
      initialFocus: input,
      // Taken as typed (no trimming): the backend validates the name.
      onSubmit: () => input.value || undefined,
    });
  }

  private async run(action: () => Promise<void>): Promise<void> {
    try {
      await action();
      await this.refresh();
    } catch (e) {
      this.fail(e);
    }
  }

  private async mkdir(): Promise<void> {
    if (this.id === null || !this.listing) return;
    const name = await this.askName(t("filesNewFolder"));
    if (!name) return;
    const { id, listing } = this;
    await this.run(() => api.sftpMkdir(id, listing.path, name));
    this.select(name);
  }

  private async rename(entry: RemoteEntry): Promise<void> {
    if (this.id === null || !this.listing) return;
    // Pre-filled with the displayed form: invisible characters are not carried over.
    const shown = displayName(entry.name);
    const name = await this.askName(t("filesRename"), shown);
    if (!name || name === shown) return;
    const { id, listing } = this;
    await this.run(() => api.sftpRename(id, listing.path, entry.name, name));
    this.select(name);
  }

  private async remove(entry: RemoteEntry): Promise<void> {
    if (this.id === null || !this.listing) return;
    const name = displayName(entry.name);
    const ok = await confirmBox(
      entry.kind === "dir" ? t("filesDeleteDir", { name }) : t("filesDeleteFile", { name }),
      t("delete"),
      true,
    );
    if (!ok) return;
    const { id, listing } = this;
    await this.run(() => api.sftpRemove(id, listing.path, entry.name));
  }

  private startTransfer(label: string): (p: TransferProgress) => void {
    this.busy = true;
    this.setBusy(true);
    this.progress.hidden = false;
    this.progress.removeAttribute("value");
    this.cancelBtn.hidden = false;
    this.say(label);
    return (p) => {
      this.transfer = p.transfer;
      if (p.total) {
        this.progress.value = Math.min(p.done / p.total, 1);
        this.say(`${label} ${Math.floor((p.done / p.total) * 100)}%`);
      } else {
        this.say(`${label} ${formatSize(p.done)}`);
      }
    };
  }

  private endTransfer(): void {
    this.busy = false;
    this.transfer = null;
    this.progress.hidden = true;
    this.cancelBtn.hidden = true;
    if (this.state === "open") this.setBusy(false);
  }

  private async download(entry: RemoteEntry): Promise<void> {
    if (this.id === null || !this.listing || this.busy) return;
    const name = displayName(entry.name);
    const onProgress = this.startTransfer(t("filesDownloading", { name }));
    try {
      const saved = await api.sftpDownload(this.id, this.listing.path, entry.name, onProgress);
      this.say(saved ? t("filesSaved", { path: saved }) : "");
    } catch (e) {
      if (/cancelled/i.test(errorText(e))) this.say(t("filesCancelled"));
      else this.fail(e);
    } finally {
      this.endTransfer();
    }
  }

  private async upload(): Promise<void> {
    if (this.id === null || !this.listing || this.busy) return;
    this.busy = true;
    const { id } = this;
    const dir = this.listing.path;
    let picked;
    try {
      picked = await api.sftpPickUpload();
    } catch (e) {
      this.busy = false;
      this.fail(e);
      return;
    }
    if (!picked.length) {
      this.busy = false;
      return;
    }
    const existing = new Set(this.listing.entries.map((e) => e.name));
    const pending = new Set(picked.map((f) => f.token));
    let uploaded = 0;
    try {
      for (const file of picked) {
        pending.delete(file.token);
        const name = displayName(file.name);
        let overwrite = false;
        if (existing.has(file.name)) {
          const choice = await modal<"replace" | "skip" | "stop">({
            title: t("filesExistsTitle"),
            body: [h("p", {}, t("filesExistsBody", { name }))],
            buttons: [
              { label: t("filesStop"), value: "stop" },
              { label: t("filesSkip"), value: "skip" },
              { label: t("filesReplace"), value: "replace", kind: "danger" },
            ],
            cancelValue: "stop",
            focusButton: 1,
          });
          if (choice === "stop") break;
          if (choice === "skip") continue;
          overwrite = true;
        }
        const onProgress = this.startTransfer(t("filesUploading", { name }));
        try {
          await api.sftpUpload(id, dir, file.token, overwrite, onProgress);
          uploaded++;
        } finally {
          this.endTransfer();
          this.busy = true;
        }
      }
      this.say(t("filesUploaded", { count: uploaded }));
    } catch (e) {
      if (/cancelled/i.test(errorText(e))) this.say(t("filesCancelled"));
      else this.fail(e);
    } finally {
      this.busy = false;
      // Skipped or stopped files: release their tokens at once.
      if (pending.size) void api.sftpForgetPicks([...pending]).catch(() => {});
    }
    if (this.state === "open") {
      await this.navigate(dir);
      if (uploaded) this.say(t("filesUploaded", { count: uploaded }));
    }
  }
}
