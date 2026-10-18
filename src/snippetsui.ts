// Settings > Snippets: add, edit, reorder and delete snippets. Works on a copy
// of the list; Settings saves it when the dialog is confirmed.
import { api } from "./api";
import { confirmBox, modal } from "./dialogs";
import { clear, h, icon } from "./dom";
import { t } from "./i18n";
import { digitHint } from "./keys";
import { MAX_NAME, MAX_SNIPPETS, MAX_TEXT, type Snippet } from "./snippets";

/** Asks for a snippet's fields; null when cancelled. */
async function editSnippet(title: string, initial: Snippet): Promise<Snippet | null> {
  const name = h("input", { class: "input", maxlength: MAX_NAME, spellcheck: "false", autocomplete: "off" });
  name.value = initial.name;
  const text = h("textarea", { class: "input mono", rows: "6", spellcheck: "false", maxlength: MAX_TEXT });
  text.value = initial.text;
  const run = h("input", { type: "checkbox", checked: initial.run });
  const group = h("select", { class: "input" }, h("option", { value: "" }, t("snippetAllGroups")));
  // The groups of the saved connections, and the snippet's own even if no
  // connection uses it any more.
  const groups = new Set<string>(initial.group ? [initial.group] : []);
  try {
    for (const p of (await api.listProfiles()).profiles) if (p.group?.trim()) groups.add(p.group.trim());
  } catch {
    // Only "all" (and the current group) then.
  }
  for (const g of [...groups].sort((a, b) => a.localeCompare(b))) {
    group.append(h("option", { value: g }, t("snippetGroup", { group: g })));
  }
  group.value = initial.group;
  const warn = h("p", { class: "warn", hidden: true }, t("snippetIncomplete"));
  return modal<Snippet | null>({
    title,
    className: "snippet-edit",
    body: [
      h("label", { class: "field" }, h("span", {}, t("name")), name),
      h("label", { class: "field" }, h("span", {}, t("snippetText")), text),
      h("label", { class: "field" }, h("span", {}, t("snippetUseWith")), group),
      h("label", { class: "check" }, run, h("span", {}, t("snippetRun"))),
      warn,
    ],
    buttons: [
      { label: t("cancel"), value: null },
      { label: t("save"), value: null, kind: "primary" },
    ],
    cancelValue: null,
    initialFocus: name,
    onSubmit: () => {
      const ok = name.value.trim() && text.value;
      warn.hidden = !!ok;
      if (!ok) return undefined;
      return { ...initial, name: name.value.trim(), text: text.value, run: run.checked, group: group.value };
    },
  });
}

export function snippetsPanel(list: Snippet[]): HTMLElement[] {
  const rows = h("div", { class: "snippet-list" });
  const addBtn = h("button", { type: "button", class: "btn" }, icon("plus"), h("span", {}, t("snippetNew")));
  const render = () => {
    clear(rows);
    list.forEach((s, i) => {
      const button = (name: Parameters<typeof icon>[0], label: string, action: () => void, danger = false) => {
        const b = h("button", { type: "button", class: `icon-btn${danger ? " danger" : ""}`, title: label, "aria-label": `${label}: ${s.name}` }, icon(name));
        b.addEventListener("click", action);
        return b;
      };
      const move = (by: number) => {
        const [item] = list.splice(i, 1);
        list.splice(i + by, 0, item!);
        render();
      };
      const up = button("arrowUp", t("snippetMoveUp"), () => move(-1));
      up.style.visibility = i > 0 ? "visible" : "hidden";
      const down = button("chevron", t("snippetMoveDown"), () => move(1));
      down.style.visibility = i < list.length - 1 ? "visible" : "hidden";
      const key = i < 9 ? digitHint("sendSnippet", i + 1) : "";
      const details = [s.group ? t("snippetGroup", { group: s.group }) : t("snippetAllGroups"), s.run ? t("snippetRuns") : "", key]
        .filter(Boolean)
        .join(" · ");
      rows.append(
        h(
          "div",
          { class: "snippet-row" },
          icon(s.run ? "terminal" : "snippet"),
          h("span", { class: "snippet-name" }, h("strong", {}, s.name), h("span", { class: "muted" }, details)),
          up,
          down,
          button("edit", t("edit"), async () => {
            const next = await editSnippet(t("snippetEdit"), s);
            if (next) list[i] = next;
            render();
          }),
          button(
            "trash",
            t("delete"),
            async () => {
              if (await confirmBox(t("snippetDelete", { name: s.name }), t("delete"), true)) list.splice(i, 1);
              render();
            },
            true,
          ),
        ),
      );
    });
    if (!list.length) rows.append(h("p", { class: "muted" }, t("snippetsEmpty")));
    addBtn.disabled = list.length >= MAX_SNIPPETS;
  };
  addBtn.addEventListener("click", async () => {
    const created = await editSnippet(t("snippetNew"), { id: crypto.randomUUID(), name: "", text: "", run: false, group: "" });
    if (created) list.push(created);
    render();
  });
  render();
  return [
    h("p", { class: "muted" }, t("snippetsHelp")),
    h("p", { class: "warn" }, t("snippetsNoSecrets")),
    rows,
    h("div", { class: "settings-row" }, h("span"), addBtn),
  ];
}
