// Backup of the configuration (Settings > General): the backend writes and
// reads the file in native dialogs and handles the connections; this module
// adds the interface's settings and applies them back. Everything read from
// a backup goes through the same validation as the saved settings.
import { api, errorText } from "./api";
import { alertBox, modal } from "./dialogs";
import { displayName, h } from "./dom";
import { currentLanguage, LANGUAGES, languageSetting, setLanguage, t, type LanguageSetting } from "./i18n";
import { importedSnippetKeys, importKeyMap, storedKeyMap } from "./keys";
import { importedStartup, importLayouts, paneCount, storedLayouts } from "./layouts";
import { getPrefs, normalize as normalizePrefs, setPrefs } from "./prefs";
import { getSnippets, importSnippets, parseSnippets } from "./snippets";
import { autoCheckEnabled, setAutoCheck } from "./updates";

/** Fired after an import changed the connections, so the app reloads them. */
export const PROFILES_CHANGED = "termdock:profiles-changed";

function exportUi(): unknown {
  return {
    version: 1,
    prefs: getPrefs(),
    language: languageSetting(),
    autoUpdate: autoCheckEnabled(),
    keys: storedKeyMap(),
    layouts: storedLayouts(),
    snippets: getSnippets(),
  };
}

function record(ui: unknown): Record<string, unknown> {
  return ui && typeof ui === "object" ? (ui as Record<string, unknown>) : {};
}

const count = (v: unknown, key?: string): number => {
  const list = key ? record(v)[key] : v;
  return Array.isArray(list) ? list.length : 0;
};

/** Applies the interface's part of a backup. "Add" only brings in new layouts and snippets. */
function importUi(ui: unknown, replace: boolean): void {
  const o = record(ui);
  importLayouts(o.layouts ?? {}, replace);
  importSnippets(o.snippets ?? [], replace);
  if (!replace) return;
  if ("prefs" in o) setPrefs(normalizePrefs(o.prefs));
  if ("keys" in o) importKeyMap(o.keys);
  if (typeof o.autoUpdate === "boolean") setAutoCheck(o.autoUpdate);
  const language = o.language;
  if (language === "auto" || LANGUAGES.some((l) => l.code === language)) setLanguage(language as LanguageSetting);
}

/** Saves a backup; resolves with a message for the status line ("" if cancelled). */
export async function exportBackup(): Promise<string> {
  const path = await api.configExport(exportUi());
  return path ? t("backupSaved", { path }) : "";
}

/**
 * Picks a backup, shows what it holds and applies it on the user's choice.
 * `settings`: the open Settings dialog, closed without saving before the
 * import is applied (its form holds copies that would undo it).
 */
export async function importBackup(settings: HTMLDialogElement | null): Promise<string> {
  const preview = await api.configImportPick();
  if (!preview) return "";
  const ui = record(preview.ui);
  const names = preview.newNames.join(", ");
  const list = (items: string[], mono = true) =>
    h("ul", { class: mono ? "mono backup-startups" : "backup-startups" }, ...items.map((s) => h("li", {}, displayName(s))));
  // What "replace" changes beyond the connections, read as importUi would.
  const startup = importedStartup(ui.layouts ?? {});
  const snippetKeys = importedSnippetKeys("keys" in ui ? ui.keys : storedKeyMap());
  const typed = snippetKeys ? parseSnippets(ui.snippets ?? []).slice(0, 9) : [];
  const replaceNotes = [
    preview.changedCount ? t("backupReplaceChanged", { count: preview.changedCount, names: preview.changedNames.join(", ") }) : "",
    startup ? t("backupReplaceStartup", { name: startup.name, count: paneCount(startup.root) }) : "",
    typed.length
      ? t("backupReplaceSnippetKeys", { keys: snippetKeys, names: typed.map((s) => (s.run ? `${s.name} ↵` : s.name)).join(", ") })
      : "",
  ].filter(Boolean);
  const choice = await modal<"cancel" | "add" | "replace">({
    title: t("backupImportTitle"),
    body: [
      h("p", {}, t("backupFrom", { date: new Date(preview.exported * 1000).toLocaleString(currentLanguage()), version: preview.app })),
      h(
        "p",
        {},
        t("backupHolds", {
          profiles: preview.profiles,
          newProfiles: preview.newProfiles,
          layouts: count(ui.layouts, "layouts"),
          snippets: count(ui.snippets),
        }),
      ),
      h("p", { class: "muted", hidden: !names }, t("backupNewNames", { names })),
      ...(preview.startupCount
        ? [
            h("p", { class: "warn" }, t("backupStartups", { count: preview.startupCount })),
            list(preview.startups.map((s) => `${s.name}: ${s.command}`)),
          ]
        : []),
      ...(preview.localCount
        ? [
            h("p", { class: "warn" }, t("backupLocals", { count: preview.localCount })),
            list(preview.locals.map((l) => `${l.name}: ${l.command}${l.cwd ? ` (${t("backupInFolder", { path: l.cwd })})` : ""}`)),
          ]
        : []),
      ...(replaceNotes.length ? [h("p", { class: "warn" }, t("backupReplaceNotes")), list(replaceNotes, false)] : []),
      h("p", {}, t("backupChoice")),
      h("p", { class: "warn" }, t("backupTrust")),
    ],
    buttons: [
      { label: t("cancel"), value: "cancel" },
      { label: t("backupAdd"), value: "add", kind: "primary" },
      { label: t("backupReplace"), value: "replace", kind: "danger" },
    ],
    cancelValue: "cancel",
    focusButton: 1,
  });
  if (choice === "cancel") {
    await api.configImportCancel().catch(() => {});
    return "";
  }
  const replace = choice === "replace";
  const imported = await api.configImportApply(preview.token, replace);
  settings?.dispatchEvent(new Event("cancel"));
  importUi(preview.ui, replace);
  window.dispatchEvent(new Event(PROFILES_CHANGED));
  const message = replace ? t("backupReplaced", { count: imported }) : t("backupAdded", { count: imported });
  // Settings is closed by now: say it in a box.
  await alertBox(t("backupImportTitle"), message);
  return message;
}

/** Runs a backup action, turning errors into a status message. */
export async function backupAction(run: () => Promise<string>): Promise<string> {
  try {
    return await run();
  } catch (e) {
    return errorText(e);
  }
}
