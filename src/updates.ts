// In-app updates. The backend decides where to look and verifies every
// package; this module only schedules checks and asks the user.
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { api, errorText } from "./api";
import { alertBox, modal } from "./dialogs";
import { h } from "./dom";
import { currentLanguage, t } from "./i18n";
import type { UpdateInfo, UpdateProgress } from "./types";

export const RELEASES_URL = "https://github.com/LuuckA21/termdock/releases/latest";

const AUTO_KEY = "termdock.updates.auto";
const FIRST_CHECK_DELAY_MS = 10_000;
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

/** True while a check, the update dialog or an installation is running. */
let busy = false;
/** Versions the user postponed: not offered again until the next start. */
const postponed = new Set<string>();

export function autoCheckEnabled(): boolean {
  try {
    return localStorage.getItem(AUTO_KEY) !== "off";
  } catch {
    return true;
  }
}

export function setAutoCheck(enabled: boolean): void {
  try {
    localStorage.setItem(AUTO_KEY, enabled ? "on" : "off");
  } catch {
    // Not persisted: the default (on) applies at the next start.
  }
}

/** Checks shortly after start, then once a day while the app stays open. */
export function startUpdateChecks(): void {
  if (import.meta.env.DEV) return;
  const run = () => {
    if (autoCheckEnabled()) void check(false);
  };
  setTimeout(run, FIRST_CHECK_DELAY_MS);
  setInterval(run, CHECK_INTERVAL_MS);
}

/**
 * Asks the backend for a newer release and offers it. `manual` checks report
 * every outcome and ignore earlier "Later" answers; automatic ones stay quiet
 * unless there is something to install.
 *
 * Returns a status line for the Settings dialog, or null when an update was offered
 * or another check was already running.
 */
export async function check(manual: boolean): Promise<string | null> {
  if (busy) return null;
  busy = true;
  try {
    const update = await api.checkUpdate();
    if (!update) return manual ? t("updateUpToDate", { version: (await api.appInfo()).version }) : null;
    if (!manual && postponed.has(update.version)) return null;
    await offer(update);
    return null;
  } catch (e) {
    if (!manual) console.warn("update check failed:", e);
    return t("updateCheckFailed", { error: errorText(e) });
  } finally {
    busy = false;
  }
}

async function offer(update: UpdateInfo): Promise<void> {
  const notes = localizedNotes(update.notes);
  const body: Node[] = [h("p", {}, t("updateBody", { version: update.version, current: update.currentVersion }))];
  if (notes) body.push(h("div", { class: "update-notes pre selectable" }, notes));
  body.push(h("p", { class: "muted" }, update.installable ? t("updateRestartNote") : t("updatePackageManaged")));

  const choice = await modal<"install" | "copy" | "later">({
    title: t("updateTitle"),
    className: "update",
    body,
    buttons: update.installable
      ? [
          { label: t("updateLater"), value: "later" },
          { label: t("updateInstall"), value: "install", kind: "primary" },
        ]
      : [
          { label: t("updateCopyLink"), value: "copy" },
          { label: t("ok"), value: "later", kind: "primary" },
        ],
    cancelValue: "later",
  });
  if (choice === "install") {
    await installWithProgress();
    return;
  }
  postponed.add(update.version);
  if (choice === "copy") await copyReleasesLink();
}

async function installWithProgress(): Promise<void> {
  const status = h("p", {}, t("updateDownloading", { percent: 0 }));
  const bar = h("progress", { max: "1", value: "0" });
  const dlg = h("dialog", { class: "modal update-progress" });
  dlg.append(h("form", {}, h("h2", {}, t("updateTitle")), h("div", { class: "modal-body" }, status, bar)));
  // Not dismissable: the download cannot be interrupted half way.
  dlg.addEventListener("cancel", (e) => e.preventDefault());
  document.body.append(dlg);
  dlg.showModal();

  const onProgress = (p: UpdateProgress) => {
    if (p.stage === "installing") {
      status.textContent = t("updateInstalling");
      bar.removeAttribute("value");
    } else if (p.total) {
      const fraction = Math.min(p.downloaded / p.total, 1);
      status.textContent = t("updateDownloading", { percent: Math.floor(fraction * 100) });
      bar.value = fraction;
    } else {
      status.textContent = t("updateDownloadingSize", { size: (p.downloaded / 1_048_576).toFixed(1) });
      bar.removeAttribute("value");
    }
  };

  try {
    // On success the app restarts; nothing after this runs.
    await api.installUpdate(onProgress);
  } catch (e) {
    dlg.close();
    dlg.remove();
    await failed(errorText(e));
  }
}

async function failed(error: string): Promise<void> {
  const copy = await modal<boolean>({
    title: t("updateFailed"),
    body: [h("p", { class: "pre selectable" }, t("updateFailedBody", { error, url: RELEASES_URL }))],
    buttons: [
      { label: t("updateCopyLink"), value: true },
      { label: t("ok"), value: false, kind: "primary" },
    ],
    cancelValue: false,
  });
  if (copy) await copyReleasesLink();
}

async function copyReleasesLink(): Promise<void> {
  await writeText(RELEASES_URL).catch((e) => alertBox(t("errorTitle"), errorText(e)));
}

/**
 * Release notes hold the English changelog, then "---", "**Italiano**" and the
 * Italian one: keep the part in the UI language and drop Markdown markers.
 */
export function localizedNotes(notes: string): string {
  const [en, it] = notes.split(/\n-{3,}\s*\n\s*\*\*Italiano\*\*\s*\n/);
  const text = currentLanguage() === "it" && it ? it : en;
  return text
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/\*\*(.+?)\*\*/g, "$1")
    .replace(/`([^`\n]+)`/g, "$1")
    .trim();
}
