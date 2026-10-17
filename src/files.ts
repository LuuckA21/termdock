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
import type { TerminalPane } from "./terminal";
import type { EditEvent, PickedFile, RemoteEntry, RemoteListing, TransferProgress } from "./types";

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
  source: TerminalPane;
}

const USER_NAME = /^[A-Za-z_][A-Za-z0-9_.-]{0,31}$/;

/**
 * The user in a terminal title like "bob@host: ~/dir" (what bash sets on
 * Debian/Ubuntu and RHEL). The title comes from the server: it is only used
 * to suggest switching, never acted on by itself.
 */
export function titleUser(title: string): string | null {
  const m = /^([A-Za-z_][A-Za-z0-9_.-]{0,31})@[A-Za-z0-9][A-Za-z0-9.-]*(?:[:\s]|$)/.exec(title);
  return m ? m[1]! : null;
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
  /** The user the browser acts as, and the terminal's login user. */
  private user = "";
  private loginUser = "";
  /** Set while switching user (sudo, password dialog). */
  private switching = false;
  /** Closes an open password dialog when the pane goes away. */
  private readonly closing = new AbortController();
  private readonly unsubscribeTitle: () => void;
  private readonly userBtn: HTMLButtonElement;
  private readonly userName: HTMLElement;
  private readonly suggestion: HTMLElement;
  private readonly cwdBtn: HTMLButtonElement;
  private readonly followBtn: HTMLButtonElement;
  /** Go to the terminal's folder whenever it changes. */
  private follow = false;

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
    this.userName = h("span", { class: "files-user-name" });
    this.userBtn = h(
      "button",
      { type: "button", class: "btn btn-ghost files-user", title: t("filesUserMenu"), "aria-label": t("filesUserMenu") },
      icon("user"),
      this.userName,
    );
    this.userBtn.addEventListener("click", () => {
      const r = this.userBtn.getBoundingClientRect();
      this.userMenu(r.left, r.bottom + 4);
    });
    this.actions.push(this.userBtn);
    this.suggestion = h("div", { class: "files-suggest", hidden: true });
    this.cwdBtn = h(
      "button",
      { type: "button", class: "icon-btn", title: t("filesTerminalFolder"), "aria-label": t("filesTerminalFolder") },
      icon("terminal"),
    );
    this.cwdBtn.addEventListener("click", () => {
      const folder = this.terminalFolder();
      if (folder) void this.navigate(folder);
    });
    this.followBtn = h(
      "button",
      { type: "button", class: "icon-btn toggle", title: t("filesFollow"), "aria-label": t("filesFollow"), "aria-pressed": "false" },
      icon("link"),
    );
    this.followBtn.addEventListener("click", () => {
      this.follow = !this.follow;
      this.followBtn.setAttribute("aria-pressed", String(this.follow));
      if (this.follow) this.followTerminal();
    });
    const bar = h(
      "div",
      { class: "files-bar" },
      this.userBtn,
      tool("arrowUp", t("filesUp"), () => void this.up()),
      tool("home", t("filesHome"), () => void this.navigate(this.home)),
      tool("refresh", t("filesRefresh"), () => void this.refresh()),
      this.cwdBtn,
      this.followBtn,
      this.pathInput,
      tool("folderPlus", t("filesNewFolder"), () => void this.mkdir()),
      tool("upload", t("filesUpload"), () => void this.upload()),
    );
    this.el = h(
      "div",
      { class: "pane files-pane" },
      bar,
      this.suggestion,
      this.list,
      h("div", { class: "files-status" }, this.status, this.progress, this.cancelBtn),
    );
    this.el.addEventListener("focusin", () => this.opts.onFocus(this));
    this.setBusy(true);
    this.unsubscribeTitle = opts.source.onRemoteTitle(() => {
      this.suggest();
      this.followTerminal();
    });
    void this.open(null);
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
    this.closing.abort();
    this.unsubscribeTitle();
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

  /**
   * Opens the browser as the login user (`null`) or, through sudo, as `user`.
   * On success it replaces the current one; on failure the current one stays.
   */
  private async open(user: string | null): Promise<void> {
    if (this.switching) return;
    this.switching = true;
    this.setBusy(true);
    this.say(user ? t("filesOpeningAs", { user }) : t("filesOpening"));
    try {
      let res = await api.sftpOpen(this.opts.session, user);
      for (;;) {
        if (res.status === "passwordRequired") {
          // sudo printed its prompt and waits: the password goes to it only.
          const password = await this.askPassword(user ?? "", res.asksFor ?? this.loginUser, res.retry);
          if (password === null || this.disposed) {
            void api.sftpSudoCancel(res.pending).catch(() => {});
            this.say(t("filesSudoCancelled"));
            if (this.id === null) this.fail(t("filesSudoCancelled"));
            return;
          }
          res = await api.sftpSudoPassword(res.pending, password);
          continue;
        }
        if (this.disposed) {
          void api.sftpClose(res.id);
          return;
        }
        const previous = this.id;
        this.id = res.id;
        this.home = res.home;
        this.user = res.user;
        this.loginUser = res.loginUser;
        if (previous !== null) void api.sftpClose(previous).catch(() => {});
        this.userName.textContent = displayName(res.user);
        this.userBtn.classList.toggle("root", res.user === "root");
        this.setState("open");
        this.setBusy(false);
        this.pathInput.disabled = false;
        this.suggest();
        await this.navigate(res.home);
        return;
      }
    } catch (e) {
      this.fail(e);
    } finally {
      this.switching = false;
      if (this.state === "open") this.setBusy(this.busy);
    }
  }

  /**
   * Opens a remote text file in the system text editor; each save there is
   * uploaded back (asking first if the server's copy changed meanwhile).
   */
  private async editFile(entry: RemoteEntry): Promise<void> {
    if (this.id === null || !this.listing) return;
    const dir = this.listing.path;
    const name = displayName(entry.name);
    this.say(t("filesEditOpening", { name }));
    try {
      await api.sftpEdit(this.id, dir, entry.name, (ev) => void this.onEdit(ev, name, dir));
      this.say(t("filesEditOpened", { name }));
    } catch (e) {
      this.fail(e);
    }
  }

  private async onEdit(ev: EditEvent, name: string, dir: string): Promise<void> {
    if (this.disposed) return;
    if (ev.kind === "saved") {
      if (this.listing?.path === dir && !this.busy) await this.navigate(dir);
      this.say(t("filesEditSaved", { name, time: new Date().toLocaleTimeString() }));
    } else if (ev.kind === "conflict") {
      const overwrite = await confirmBox(t("filesEditConflict", { name }), t("filesEditOverwrite"), true);
      await api.sftpEditResolve(ev.edit, overwrite).catch((e) => this.fail(e));
      if (!overwrite) this.say(t("filesEditStopped", { name }));
    } else {
      this.say(displayName(t("filesEditFailed", { name, error: ev.message })), true);
    }
  }

  /** Switches the browser to another user (null: back to the login user). */
  private async switchUser(user: string | null): Promise<void> {
    if (this.busy || this.switching || this.state === "closed") return;
    // The suggestion comes from the server (terminal title): root needs a yes.
    if (user === "root" && !(await confirmBox(t("filesRootConfirm"), t("filesOpenAs", { user }), true))) return;
    void this.open(user);
  }

  /**
   * The terminal's current folder: from OSC 7 or from a title like
   * "user@host: ~/dir" ("~" only for the user the browser acts as). Null if
   * the shell does not say.
   */
  private terminalFolder(): string | null {
    const src = this.opts.source;
    const m = /^([A-Za-z_][A-Za-z0-9_.-]{0,31})@[A-Za-z0-9][A-Za-z0-9.-]*:\s*(\S.*)$/.exec(src.remoteTitle);
    let fromTitle: string | null = null;
    if (m) {
      const [, user, dir] = m;
      const d = dir!.trim();
      if (d.startsWith("/")) fromTitle = d;
      else if ((d === "~" || d.startsWith("~/")) && user === this.user) fromTitle = this.home.replace(/\/$/, "") + d.slice(1);
    }
    return src.cwdIsNewer ? (src.remoteCwd ?? fromTitle) : (fromTitle ?? src.remoteCwd);
  }

  /** With "follow" on, goes to the terminal's folder when it changes. */
  private followTerminal(): void {
    const folder = this.terminalFolder();
    this.cwdBtn.disabled = this.state !== "open" || folder === null;
    this.cwdBtn.title = folder ? t("filesTerminalFolder") : t("filesTerminalFolderUnknown");
    if (!this.follow || !folder || this.busy || this.switching || this.state !== "open") return;
    if (this.listing && folder === this.listing.path) return;
    void this.navigate(folder);
  }

  /** Offers to follow the terminal when its title shows another user. */
  private suggest(): void {
    const other = titleUser(this.opts.source.remoteTitle);
    const show = this.state === "open" && other !== null && other !== this.user;
    this.suggestion.hidden = !show;
    if (!show) return;
    const btn = h("button", { type: "button", class: "btn btn-primary" }, t("filesOpenAs", { user: other }));
    btn.addEventListener("click", () => void this.switchUser(other === this.loginUser ? null : other));
    clear(this.suggestion);
    this.suggestion.append(icon("user"), h("span", {}, t("filesTerminalUser", { user: other })), btn);
  }

  private userMenu(x: number, y: number): void {
    if (this.state !== "open") return;
    const items = [];
    const other = titleUser(this.opts.source.remoteTitle);
    // The login user is covered by "Back to" below.
    if (other && other !== this.user && other !== this.loginUser) {
      items.push({ label: t("filesOpenAs", { user: other }), action: () => void this.switchUser(other === this.loginUser ? null : other) });
    }
    if (this.user !== this.loginUser) {
      items.push({ label: t("filesBackTo", { user: this.loginUser }), action: () => void this.switchUser(null) });
    }
    items.push({ label: t("filesOpenAsOther"), icon: icon("user"), action: () => void this.askUser() });
    showMenu(x, y, items);
  }

  private async askUser(): Promise<void> {
    const input = h("input", { class: "input", spellcheck: "false", autocomplete: "off", maxlength: 32 });
    const warn = h("p", { class: "warn", hidden: true }, t("filesUserInvalid"));
    const user = await modal<string | null>({
      title: t("filesOpenAsOther"),
      body: [
        h("p", { class: "muted" }, t("filesOpenAsHelp", { user: this.loginUser })),
        h("label", { class: "field" }, h("span", {}, t("username")), input),
        warn,
      ],
      buttons: [
        { label: t("cancel"), value: null },
        { label: t("ok"), value: null, kind: "primary" },
      ],
      cancelValue: null,
      initialFocus: input,
      onSubmit: () => {
        const name = input.value.trim();
        warn.hidden = USER_NAME.test(name);
        return warn.hidden ? name : undefined;
      },
    });
    if (user) await this.switchUser(user === this.loginUser ? null : user);
  }

  /** The password sudo asks for (`who`'s); null when cancelled. Never stored. */
  private async askPassword(user: string, who: string, retry: boolean): Promise<string | null> {
    const input = h("input", { class: "input", type: "password", autocomplete: "off" });
    const password = await modal<string | null>({
      title: t("filesSudoTitle", { user }),
      body: [
        h("p", {}, t("filesSudoBody", { who: who || "?", host: this.opts.title, user })),
        h("p", { class: "warn", hidden: !retry }, t("filesSudoWrong")),
        h("label", { class: "field" }, h("span", {}, t("authPassword")), input),
      ],
      buttons: [
        { label: t("cancel"), value: null },
        { label: t("ok"), value: null, kind: "primary" },
      ],
      cancelValue: null,
      initialFocus: input,
      onSubmit: () => input.value || undefined,
      signal: this.closing.signal,
    });
    input.value = "";
    return password;
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
      if (entry.kind === "file" || entry.kind === "link") {
        items.push({ label: t("filesEdit"), icon: icon("edit"), hint: "F4", action: () => void this.editFile(entry) });
      }
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
    else if (e.key === "F4" && entry && !unusable(entry) && entry.kind !== "dir") void this.editFile(entry);
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
    await this.uploadAll(
      dir,
      picked.map((f) => ({
        name: f.name,
        send: (overwrite: boolean, onProgress: (p: TransferProgress) => void) =>
          api.sftpUpload(id, dir, f.token, overwrite, onProgress),
        release: () => void api.sftpForgetPicks([f.token]).catch(() => {}),
      })),
    );
  }

  /** Highlights the browser while files from the computer are dragged over it. */
  setDropHover(on: boolean): void {
    this.el.classList.toggle("drop-target", on && this.state === "open");
  }

  /**
   * Files dropped on the browser from the computer: the backend opened them
   * and gives tokens, uploaded like files picked in the dialog. Returns
   * false (tokens untouched) when the browser cannot take them now.
   */
  dropFiles(files: PickedFile[], folders: number): boolean {
    this.setDropHover(false);
    if (this.id === null || !this.listing || this.busy || this.state !== "open") return false;
    if (folders) this.say(t("filesDropFolders", { count: folders }), true);
    else if (!files.length) this.say(t("filesDropNothing"), true);
    if (!files.length) return true;
    this.busy = true;
    const { id } = this;
    const dir = this.listing.path;
    void this.uploadAll(
      dir,
      files.map((f) => ({
        name: f.name,
        send: (overwrite: boolean, onProgress: (p: TransferProgress) => void) =>
          api.sftpUpload(id, dir, f.token, overwrite, onProgress),
        release: () => void api.sftpForgetPicks([f.token]).catch(() => {}),
      })),
    );
    return true;
  }

  /** Uploads one by one, asking before replacing; `busy` is set by the caller. */
  private async uploadAll(
    dir: string,
    items: {
      name: string;
      send: (overwrite: boolean, onProgress: (p: TransferProgress) => void) => Promise<void>;
      release?: () => void;
    }[],
  ): Promise<void> {
    if (!this.listing || !items.length) {
      this.busy = false;
      return;
    }
    const existing = new Set(this.listing.entries.map((e) => e.name));
    let uploaded = 0;
    let next = 0;
    try {
      for (; next < items.length; next++) {
        const item = items[next]!;
        const name = displayName(item.name);
        let overwrite = false;
        if (existing.has(item.name)) {
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
          if (choice === "skip") {
            item.release?.();
            continue;
          }
          overwrite = true;
        }
        const onProgress = this.startTransfer(t("filesUploading", { name }));
        try {
          await item.send(overwrite, onProgress);
          uploaded++;
        } finally {
          this.endTransfer();
          this.busy = true;
        }
      }
      this.say(t("filesUploaded", { count: uploaded }));
    } catch (e) {
      next++;
      if (/cancelled/i.test(errorText(e))) this.say(t("filesCancelled"));
      else this.fail(e);
    } finally {
      this.busy = false;
      // Skipped or stopped files: release what they hold at once.
      for (const item of items.slice(next)) item.release?.();
    }
    if (this.state === "open") {
      await this.navigate(dir);
      if (uploaded) this.say(t("filesUploaded", { count: uploaded }));
    }
  }
}

/**
 * A drag that may carry files from the computer: "Files", or a URI list
 * (how WebKitGTK presents files from a file manager). Only real files are
 * taken on drop; links are ignored.
 */
export function carriesFiles(e: DragEvent): boolean {
  const types = e.dataTransfer ? Array.from(e.dataTransfer.types) : [];
  return types.includes("Files") || types.includes("text/uri-list");
}
