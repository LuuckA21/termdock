import { api, errorText } from "./api";
import { alertBox, confirmBox } from "./dialogs";
import { clear, h, icon } from "./dom";
import { describe, setDotColor } from "./favorites";
import { t } from "./i18n";
import type { App } from "./main";
import type { Profile, SshAuth, Uuid } from "./types";

const COLORS = ["#3d5af1", "#7ee0b0", "#ffd166", "#ff6b6b", "#c792ea", "#5fd7e0", "#f4a261", "#9aa5b8"];

export interface ManagerOptions {
  edit?: Uuid;
  create?: boolean;
}

/** Connection manager: list of all saved connections plus an editor. */
export function openManager(app: App, opts: ManagerOptions): void {
  document.querySelector("dialog.manager")?.remove();

  const dlg = h("dialog", { class: "modal manager" });
  const search = h("input", { class: "input", type: "search", placeholder: t("search"), spellcheck: "false" });
  const list = h("div", { class: "mgr-list", role: "listbox" });
  const editor = h("section", { class: "mgr-editor" });
  let selected: Uuid | null = opts.edit ?? null;

  const close = () => {
    dlg.close();
    dlg.remove();
    app.focusActive();
  };

  const renderList = () => {
    clear(list);
    const q = search.value.trim().toLowerCase();
    const items = app.profiles.filter(
      (p) => !q || `${p.name} ${describe(p)} ${p.group ?? ""}`.toLowerCase().includes(q),
    );
    if (items.length === 0) {
      list.append(h("p", { class: "muted pad" }, t("noProfiles")));
      return;
    }
    const groups = new Map<string, Profile[]>();
    for (const p of [...items].sort((a, b) => a.name.localeCompare(b.name))) {
      const g = p.group ?? "";
      groups.set(g, [...(groups.get(g) ?? []), p]);
    }
    for (const g of [...groups.keys()].sort((a, b) => (a === "" ? 1 : b === "" ? -1 : a.localeCompare(b)))) {
      list.append(h("div", { class: "mgr-group" }, g || t("ungrouped")));
      for (const p of groups.get(g)!) list.append(row(p));
    }
  };

  const row = (p: Profile): HTMLElement => {
    const fav = app.favorites.includes(p.id);
    const star = h(
      "button",
      {
        class: `icon-btn star${fav ? " on" : ""}`,
        type: "button",
        title: fav ? t("removeFromFavorites") : t("addToFavorites"),
        "aria-pressed": String(fav),
      },
      icon("star"),
    );
    star.addEventListener("click", async (e) => {
      e.stopPropagation();
      await app.setFavorite(p.id, !fav);
      renderList();
      // Keep the editor's "In favorites" box in sync with the star.
      if (selected === p.id) renderEditor(app.profiles.find((x) => x.id === p.id) ?? null);
    });
    const connect = h("button", { class: "icon-btn", type: "button", title: t("connect") }, icon("bolt"));
    connect.addEventListener("click", (e) => {
      e.stopPropagation();
      close();
      app.openProfile(p);
    });
    const dot = h("span", { class: "dot" });
    setDotColor(dot, p.color);
    const r = h(
      "div",
      { class: `mgr-row${selected === p.id ? " selected" : ""}`, role: "option", tabindex: "0" },
      star,
      dot,
      h("div", { class: "mgr-text" }, h("div", { class: "mgr-name" }, p.name), h("div", { class: "mgr-sub" }, describe(p))),
      connect,
    );
    r.addEventListener("click", () => {
      selected = p.id;
      renderList();
      renderEditor(p);
    });
    r.addEventListener("dblclick", () => {
      close();
      app.openProfile(p);
    });
    r.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        close();
        app.openProfile(p);
      }
    });
    return r;
  };

  const renderEditor = (p: Profile | null) => {
    clear(editor);
    editor.append(profileForm(app, p, async (saved) => {
      selected = saved?.id ?? null;
      renderList();
      renderEditor(saved);
    }));
  };

  search.addEventListener("input", renderList);

  const newBtn = h("button", { class: "btn btn-primary", type: "button" }, icon("plus"), h("span", {}, t("newProfile")));
  newBtn.addEventListener("click", () => {
    selected = null;
    renderList();
    renderEditor(null);
  });
  const closeBtn = h("button", { class: "icon-btn", type: "button", title: t("close"), "aria-label": t("close") }, icon("close"));
  closeBtn.addEventListener("click", close);

  dlg.append(
    h(
      "header",
      { class: "mgr-head" },
      h("h2", {}, t("connections")),
      h("div", { class: "spacer" }),
      h("span", { class: "mgr-saved", role: "status" }),
      newBtn,
      closeBtn,
    ),
    h("div", { class: "mgr-body" }, h("aside", { class: "mgr-side" }, search, list), editor),
    h("footer", { class: "mgr-foot muted" }, t("shortcutsBody")),
  );
  dlg.addEventListener("cancel", (e) => {
    e.preventDefault();
    close();
  });
  document.body.append(dlg);
  dlg.showModal();

  renderList();
  const initial = selected ? (app.profiles.find((p) => p.id === selected) ?? null) : null;
  renderEditor(opts.create ? null : initial);
  if (!opts.create && !initial) search.focus();
}

function field(label: string, control: HTMLElement, cls = ""): HTMLElement {
  return h("label", { class: `field ${cls}` }, h("span", {}, label), control);
}

function input(value: string | number | undefined, attrs: Record<string, string> = {}): HTMLInputElement {
  const el = h("input", { class: "input", spellcheck: "false", autocomplete: "off", autocapitalize: "off", ...attrs });
  el.value = value === undefined ? "" : String(value);
  return el;
}

function profileForm(app: App, existing: Profile | null, onSaved: (p: Profile | null) => void): HTMLElement {
  const isNew = existing === null;
  const base: Profile =
    existing ??
    ({
      id: crypto.randomUUID(),
      name: "",
      type: "ssh",
      host: "",
      port: 22,
      username: "",
      auth: { method: "auto" },
      keepaliveSecs: 30,
    } as Profile);

  const ssh = base.type === "ssh" ? base : null;
  const local = base.type === "local" ? base : null;
  const defaultShell = app.shells.find((s) => s.isDefault) ?? app.shells[0];

  const name = input(base.name, { required: "", maxlength: "64" });
  const type = h("select", { class: "input" }, h("option", { value: "ssh" }, t("ssh")), h("option", { value: "local" }, t("local")));
  type.value = base.type;
  const host = input(ssh?.host, { placeholder: "server.example.com" });
  const port = input(ssh?.port ?? 22, { type: "number", min: "1", max: "65535" });
  const user = input(ssh?.username, { placeholder: "root" });
  const auth = h(
    "select",
    { class: "input" },
    h("option", { value: "auto" }, t("authAuto")),
    h("option", { value: "agent" }, t("authAgent")),
    h("option", { value: "key" }, t("authKey")),
    h("option", { value: "password" }, t("authPassword")),
  );
  auth.value = ssh?.auth.method ?? "auto";
  const keyPath = input(ssh?.auth.method === "key" ? ssh.auth.path : "", { placeholder: "~/.ssh/id_ed25519" });
  const keepalive = input(ssh?.keepaliveSecs ?? 30, { type: "number", min: "0", max: "3600" });
  const program = input(local?.program ?? defaultShell?.shell.program ?? "");
  const args = h("textarea", { class: "input", rows: "3", spellcheck: "false" });
  args.value = (local?.args ?? defaultShell?.shell.args ?? []).join("\n");
  const cwd = input(local?.cwd ?? "");
  const group = input(base.group ?? "", { maxlength: "64", list: "mgr-groups" });
  const groups = h("datalist", { id: "mgr-groups" });
  for (const g of new Set(app.profiles.map((p) => p.group).filter(Boolean))) groups.append(h("option", { value: g! }));
  const favorite = h("input", { type: "checkbox" });
  const wasFavorite = !isNew && app.favorites.includes(base.id);
  favorite.checked = isNew || wasFavorite;

  let color = base.color ?? COLORS[0]!;
  const swatches = h("div", { class: "swatches", role: "radiogroup" });
  const renderSwatches = () => {
    clear(swatches);
    for (const c of COLORS) {
      const b = h("button", { type: "button", class: `swatch${c === color ? " on" : ""}`, title: c, "aria-checked": String(c === color), role: "radio" });
      b.style.background = c;
      b.addEventListener("click", () => {
        color = c;
        renderSwatches();
      });
      swatches.append(b);
    }
  };
  renderSwatches();

  const sshBox = h(
    "div",
    { class: "grid" },
    field(t("host"), host, "span2"),
    field(t("port"), port),
    field(t("username"), user),
    field(t("auth"), auth, "span2"),
    field(t("keyPath"), keyPath, "span2 key-row"),
    field(t("keepalive"), keepalive),
  );
  const localBox = h("div", { class: "grid" }, field(t("program"), program, "span2"), field(t("args"), args, "span2"), field(t("cwd"), cwd, "span2"));

  const sync = () => {
    sshBox.hidden = type.value !== "ssh";
    localBox.hidden = type.value !== "local";
    (sshBox.querySelector(".key-row") as HTMLElement).hidden = auth.value !== "key";
  };
  type.addEventListener("change", sync);
  auth.addEventListener("change", sync);
  sync();

  const error = h("p", { class: "error", role: "alert" });
  const flash = (msg: string) => {
    const note = document.querySelector<HTMLElement>("dialog.manager .mgr-saved");
    if (!note) return;
    note.textContent = msg;
    note.classList.add("show");
    setTimeout(() => note.classList.remove("show"), 1600);
  };

  const build = (): Profile => {
    const common = {
      id: base.id,
      name: name.value.trim(),
      color,
      group: group.value.trim() || undefined,
    };
    if (type.value === "local") {
      return {
        ...common,
        type: "local",
        program: program.value.trim(),
        args: args.value.split("\n").map((a) => a.trim()).filter(Boolean),
        cwd: cwd.value.trim() || undefined,
      };
    }
    const method = auth.value as SshAuth["method"];
    const a: SshAuth = method === "key" ? { method, path: keyPath.value.trim() } : { method };
    return {
      ...common,
      type: "ssh",
      host: host.value.trim(),
      port: Number(port.value) || 22,
      username: user.value.trim(),
      auth: a,
      keepaliveSecs: Math.max(0, Number(keepalive.value) || 0),
    };
  };

  const form = h("form", { class: "mgr-form", novalidate: true });
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    error.textContent = "";
    try {
      // Only send the favorite flag when the user changed it here, so a
      // star toggled elsewhere meanwhile is not overwritten.
      const fav = isNew || favorite.checked !== wasFavorite ? favorite.checked : undefined;
      const saved = await api.saveProfile(build(), fav);
      await app.reload();
      onSaved(saved);
      flash(t("saved"));
    } catch (err) {
      error.textContent = errorText(err);
    }
  });

  const actions = h("div", { class: "modal-actions" });
  if (!isNew) {
    const del = h("button", { type: "button", class: "btn btn-danger" }, icon("trash"), h("span", {}, t("delete")));
    del.addEventListener("click", async () => {
      if (!(await confirmBox(t("confirmDelete", { name: base.name }), t("delete"), true))) return;
      try {
        await api.deleteProfile(base.id);
        await app.reload();
        onSaved(null);
      } catch (err) {
        error.textContent = errorText(err);
      }
    });
    actions.append(del);
    if (ssh) {
      const forget = h("button", { type: "button", class: "btn" }, t("forgetSecrets"));
      forget.addEventListener("click", async () => {
        await api.forgetSecrets(base.id).then(() => alertBox(t("forgetSecrets"), t("secretsForgotten")), (err) => (error.textContent = errorText(err)));
      });
      const forgetKey = h("button", { type: "button", class: "btn" }, t("forgetHostKey"));
      forgetKey.addEventListener("click", async () => {
        await api
          .forgetHostKey(ssh.host, ssh.port)
          .then((n) => alertBox(t("forgetHostKey"), t("hostKeysForgotten", { n })), (err) => (error.textContent = errorText(err)));
      });
      actions.append(forget, forgetKey);
    }
  }
  actions.append(h("div", { class: "spacer" }));
  if (!isNew) {
    const connect = h("button", { type: "button", class: "btn" }, icon("bolt"), h("span", {}, t("connect")));
    connect.addEventListener("click", () => {
      document.querySelector<HTMLDialogElement>("dialog.manager")?.close();
      document.querySelector("dialog.manager")?.remove();
      app.openProfile(base);
    });
    actions.append(connect);
  }
  actions.append(h("button", { type: "submit", class: "btn btn-primary" }, t("save")));

  form.append(
    h("h3", {}, isNew ? t("newProfile") : t("editProfile")),
    h("div", { class: "grid" }, field(t("name"), name, "span2"), field(t("type"), type), field(t("group"), group), groups),
    sshBox,
    localBox,
    h("div", { class: "grid" }, field(t("color"), swatches, "span2"), h("label", { class: "check span2" }, favorite, h("span", {}, t("favorite")))),
    error,
    actions,
  );
  setTimeout(() => name.focus(), 0);
  return form;
}
