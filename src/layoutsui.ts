// Dialogs for saved layouts: naming one and managing the list (rename,
// delete, which one opens at startup).
import { confirmBox, modal } from "./dialogs";
import { clear, h, icon } from "./dom";
import { t } from "./i18n";
import { deleteLayout, getLayouts, MAX_NAME, paneCount, renameLayout, setStartup, startupLayout } from "./layouts";

/**
 * Asks for a layout name (trimmed, 1–60 characters); null when cancelled.
 * `current`: the layout being renamed (keeping its name needs no confirmation).
 */
export async function askLayoutName(title: string, initial: string, current: string | null): Promise<string | null> {
  const input = h("input", { class: "input", maxlength: MAX_NAME, spellcheck: "false", autocomplete: "off" });
  input.value = initial.slice(0, MAX_NAME);
  const name = await modal<string | null>({
    title,
    body: [h("label", { class: "field" }, h("span", {}, t("name")), input)],
    buttons: [
      { label: t("cancel"), value: null },
      { label: t("save"), value: null, kind: "primary" },
    ],
    cancelValue: null,
    initialFocus: input,
    onSubmit: () => input.value.trim() || undefined,
  });
  if (name && name !== current && getLayouts().some((l) => l.name === name)) {
    return (await confirmBox(t("layoutReplace", { name }), t("save"))) ? name : null;
  }
  return name;
}

/** Lists the saved layouts: open, rename, delete, choose the startup one. */
export function manageLayouts(open: (id: string) => void): Promise<void> {
  const list = h("div", { class: "layout-list" });
  const render = () => {
    clear(list);
    const startup = startupLayout()?.id ?? null;
    const radio = (id: string | null, label: string, detail: string) => {
      const input = h("input", { type: "radio", name: "layout-startup", checked: startup === id });
      input.addEventListener("change", () => setStartup(id));
      return h("label", { class: "layout-startup", title: t("layoutStartup") }, input, h("span", {}, label), h("span", { class: "muted" }, detail));
    };
    list.append(radio(null, t("layoutStartupShell"), ""));
    for (const layout of getLayouts()) {
      const openBtn = h("button", { type: "button", class: "icon-btn", title: t("layoutOpen"), "aria-label": t("layoutOpen") }, icon("splitRight"));
      openBtn.addEventListener("click", () => {
        // Close this dialog first, so the new tab is in front.
        openBtn.closest("dialog")?.dispatchEvent(new Event("cancel"));
        open(layout.id);
      });
      const renameBtn = h("button", { type: "button", class: "icon-btn", title: t("rename"), "aria-label": t("rename") }, icon("edit"));
      renameBtn.addEventListener("click", async () => {
        const name = await askLayoutName(t("rename"), layout.name, layout.name);
        if (name && name !== layout.name) {
          const clash = getLayouts().find((l) => l.name === name);
          if (clash) deleteLayout(clash.id);
          renameLayout(layout.id, name);
        }
        render();
      });
      const deleteBtn = h("button", { type: "button", class: "icon-btn danger", title: t("delete"), "aria-label": t("delete") }, icon("trash"));
      deleteBtn.addEventListener("click", async () => {
        if (await confirmBox(t("layoutDelete", { name: layout.name }), t("delete"), true)) deleteLayout(layout.id);
        render();
      });
      list.append(
        h(
          "div",
          { class: "layout-row" },
          radio(layout.id, layout.name, t("layoutPanes", { count: paneCount(layout.root) })),
          openBtn,
          renameBtn,
          deleteBtn,
        ),
      );
    }
    if (!getLayouts().length) list.append(h("p", { class: "muted" }, t("layoutNone")));
  };
  render();
  return modal<void>({
    title: t("layouts"),
    className: "layouts",
    body: [h("p", { class: "muted" }, t("layoutHelp")), list],
    buttons: [{ label: t("close"), value: undefined }],
    cancelValue: undefined,
  });
}
