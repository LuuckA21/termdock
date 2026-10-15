import { api, errorText } from "./api";
import { alertBox, modal } from "./dialogs";
import { h } from "./dom";
import { t } from "./i18n";
import type { App } from "./main";

/** "Import from ~/.ssh/config": preview with checkboxes, then import. */
export async function openImport(app: App): Promise<boolean> {
  let preview;
  try {
    preview = await api.previewSshConfig();
  } catch (e) {
    await alertBox(t("errorTitle"), errorText(e));
    return false;
  }
  if (!preview.found) {
    await alertBox(t("importSshConfig"), t("importNotFound", { path: preview.path }));
    return false;
  }
  if (preview.items.length === 0) {
    await alertBox(t("importSshConfig"), [t("importNone"), ...preview.warnings].join("\n"));
    return false;
  }

  const boxes = new Map<string, HTMLInputElement>();
  const list = h("div", { class: "import-list" });
  for (const item of preview.items) {
    const box = h("input", { type: "checkbox" });
    box.checked = !item.existing;
    box.disabled = !!item.existing;
    boxes.set(item.alias, box);
    const details: Node[] = [h("div", { class: "mgr-sub" }, item.target)];
    if (item.jump) details.push(h("div", { class: "muted" }, t("importVia", { jump: item.jump })));
    for (const f of item.forwards) details.push(h("div", { class: "muted mono" }, f));
    if (item.existing) details.push(h("div", { class: "muted" }, t("importAlreadySaved", { name: item.existing })));
    for (const w of item.warnings) details.push(h("div", { class: "warn" }, w));
    list.append(h("label", { class: "import-row" }, box, h("div", { class: "mgr-text" }, h("div", { class: "mgr-name" }, item.alias), ...details)));
  }
  const body: Node[] = [h("p", { class: "muted mono" }, preview.path), list];
  if (preview.warnings.length) body.push(h("div", { class: "import-warnings" }, ...preview.warnings.map((w) => h("p", { class: "warn" }, w))));

  const selected = await modal<string[] | null>({
    title: t("importSshConfig"),
    className: "import",
    body,
    buttons: [
      { label: t("cancel"), value: null },
      { label: t("importButton"), value: null, kind: "primary" },
    ],
    cancelValue: null,
    onSubmit: () => [...boxes].filter(([, b]) => b.checked && !b.disabled).map(([a]) => a),
  });
  if (!selected || selected.length === 0) return false;

  try {
    const res = await api.importSshConfig(selected);
    await app.reload();
    const lines = [t("importDone", { n: res.imported.length }), ...res.imported.map((n) => `• ${n}`), ...res.warnings];
    await alertBox(t("importSshConfig"), lines.join("\n"));
    return true;
  } catch (e) {
    await alertBox(t("errorTitle"), errorText(e));
    return false;
  }
}
