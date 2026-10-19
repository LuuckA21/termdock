// SSH keys: generate a key pair (connection editor) and install a public
// key on the server of an open terminal (tab menu). The backend writes and
// reads the files in native dialogs; the passphrase is only sent to it.
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

import { api, errorText } from "./api";
import { alertBox, confirmBox, modal } from "./dialogs";
import { h } from "./dom";
import { t } from "./i18n";
import type { KeyInfo } from "./types";

/** What ssh-keygen asks for at least (the backend checks it too). */
const MIN_PASSPHRASE = 5;
const MAX_COMMENT = 100;

/** The comment and passphrase for a new key; null when cancelled. */
async function askKeyOptions(): Promise<{ comment: string; passphrase: string } | null> {
  const comment = h("input", { class: "input", maxlength: MAX_COMMENT, spellcheck: "false", autocomplete: "off", placeholder: t("keyCommentHint") });
  const pass = h("input", { class: "input", type: "password", autocomplete: "new-password" });
  const again = h("input", { class: "input", type: "password", autocomplete: "new-password" });
  const warn = h("p", { class: "warn", hidden: true });
  for (;;) {
    const res = await modal<{ comment: string; passphrase: string } | null>({
      title: t("keyGenerateTitle"),
      body: [
        h("p", { class: "muted" }, t("keyGenerateHelp")),
        h("label", { class: "field" }, h("span", {}, t("keyComment")), comment),
        h("label", { class: "field" }, h("span", {}, t("keyPassphrase")), pass),
        h("label", { class: "field" }, h("span", {}, t("keyPassphraseAgain")), again),
        warn,
      ],
      buttons: [
        { label: t("cancel"), value: null },
        { label: t("keyGenerateSave"), value: null, kind: "primary" },
      ],
      cancelValue: null,
      initialFocus: comment,
      onSubmit: () => {
        const problem =
          pass.value !== again.value
            ? t("keyPassphraseMismatch")
            : pass.value && [...pass.value].length < MIN_PASSPHRASE
              ? t("keyPassphraseShort", { count: MIN_PASSPHRASE })
              : "";
        warn.textContent = problem;
        warn.hidden = !problem;
        return problem ? undefined : { comment: comment.value.trim(), passphrase: pass.value };
      },
    });
    if (!res || res.passphrase || (await confirmBox(t("keyNoPassphrase"), t("keyNoPassphraseOk"), true))) {
      pass.value = again.value = "";
      return res;
    }
  }
}

/** Shows a public key with its fingerprint, to copy. */
function showPublicKey(title: string, intro: string, key: KeyInfo): Promise<void> {
  const text = h("textarea", { class: "input mono key-public", rows: "4", readonly: true, spellcheck: "false" });
  text.value = key.publicKey;
  const status = h("span", { class: "muted", role: "status" });
  const copy = h("button", { type: "button", class: "btn" }, t("copy"));
  copy.addEventListener("click", () => {
    void writeText(key.publicKey).then(
      () => (status.textContent = t("keyCopied")),
      (e) => (status.textContent = errorText(e)),
    );
  });
  return modal<void>({
    title,
    className: "key-result",
    body: [
      h("p", {}, intro),
      h("p", { class: "muted" }, t("keyFingerprint", { fingerprint: key.fingerprint })),
      text,
      h("div", { class: "settings-row" }, status, copy),
      h("p", { class: "muted" }, t("keyInstallHint")),
    ],
    buttons: [{ label: "OK", value: undefined, kind: "primary" }],
    cancelValue: undefined,
  });
}

/**
 * Generates a key pair where the user saves it; resolves with where the
 * private key went (to use it in a connection), or null.
 */
export async function generateKey(): Promise<string | null> {
  const options = await askKeyOptions();
  if (!options) return null;
  let key: KeyInfo | null;
  try {
    key = await api.sshKeyGenerate(options.comment, options.passphrase);
  } catch (e) {
    await alertBox(t("keyGenerateTitle"), errorText(e));
    return null;
  }
  if (!key?.path) return null;
  await showPublicKey(t("keyGenerated"), t("keySavedAt", { path: key.path }), key);
  return key.path;
}

/**
 * Picks a public key and, once confirmed, adds it to `~/.ssh/authorized_keys`
 * on the server of session `id` (`target`: who that is, e.g. "bob@host").
 * Resolves with a line for the terminal, or "" when cancelled.
 */
export async function installKey(id: number, target: string): Promise<string> {
  const key = await api.sshKeyPickPublic();
  if (!key) return "";
  const ok = await modal<boolean>({
    title: t("keyInstallTitle"),
    body: [
      h("p", {}, t("keyInstallConfirm", { target })),
      h("p", { class: "mono" }, key.fingerprint),
      key.comment ? h("p", { class: "muted" }, t("keyCommentIs", { comment: key.comment })) : h("span"),
      h("p", { class: "warn" }, t("keyInstallWarn")),
    ],
    buttons: [
      { label: t("cancel"), value: false },
      { label: t("keyInstall"), value: true, kind: "primary" },
    ],
    cancelValue: false,
  });
  if (!ok) return "";
  const added = await api.sshKeyInstall(id, key.publicKey);
  return added ? t("keyInstalled", { target }) : t("keyAlreadyInstalled", { target });
}
