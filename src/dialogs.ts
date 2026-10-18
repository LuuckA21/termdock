import { displayName, h } from "./dom";
import { t } from "./i18n";
import type { HostKeyInfo, SecretPrompt } from "./types";

interface Button<T> {
  label: string;
  value: T;
  kind?: "primary" | "danger";
}

/**
 * Generic modal. Resolves with the clicked button's value, or `cancelValue`
 * when the dialog is dismissed with Escape.
 */
export function modal<T>(opts: {
  title: string;
  body: Node[];
  buttons: Button<T>[];
  cancelValue: T;
  className?: string;
  /** Element focused on open; otherwise the button at `focusButton` (default: last). */
  initialFocus?: HTMLElement;
  focusButton?: number;
  onSubmit?: () => T | undefined;
  /** Closes the dialog with `cancelValue`, e.g. when its terminal tab is closed. */
  signal?: AbortSignal;
}): Promise<T> {
  return new Promise((resolve) => {
    if (opts.signal?.aborted) {
      resolve(opts.cancelValue);
      return;
    }
    let done = false;
    const dlg = h("dialog", { class: `modal ${opts.className ?? ""}` });
    const onAbort = () => finish(opts.cancelValue);
    const finish = (v: T) => {
      if (done) return;
      done = true;
      opts.signal?.removeEventListener("abort", onAbort);
      dlg.close();
      dlg.remove();
      resolve(v);
    };
    opts.signal?.addEventListener("abort", onAbort);
    const form = h("form", { method: "dialog" });
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      const v = opts.onSubmit?.();
      if (v !== undefined) finish(v);
    });
    const footer = h("footer", { class: "modal-actions" });
    opts.buttons.forEach((b, i) => {
      const btn = h(
        "button",
        {
          type: b.kind === "primary" && opts.onSubmit ? "submit" : "button",
          class: `btn ${b.kind ? `btn-${b.kind}` : ""}`,
        },
        b.label,
      );
      if (!(b.kind === "primary" && opts.onSubmit)) btn.addEventListener("click", () => finish(b.value));
      if (!opts.initialFocus && i === (opts.focusButton ?? opts.buttons.length - 1)) btn.autofocus = true;
      footer.append(btn);
    });
    form.append(h("h2", {}, opts.title), h("div", { class: "modal-body" }, ...opts.body), footer);
    dlg.append(form);
    dlg.addEventListener("cancel", (e) => {
      e.preventDefault();
      finish(opts.cancelValue);
    });
    document.body.append(dlg);
    dlg.showModal();
    opts.initialFocus?.focus();
  });
}

export function alertBox(title: string, message: string): Promise<void> {
  return modal({
    title,
    body: [h("p", { class: "pre" }, message)],
    buttons: [{ label: t("ok"), value: undefined, kind: "primary" }],
    cancelValue: undefined,
  });
}

export function confirmBox(message: string, okLabel: string, danger = false): Promise<boolean> {
  return modal({
    title: t("confirmTitle"),
    body: [h("p", {}, message)],
    buttons: [
      { label: t("cancel"), value: false },
      { label: okLabel, value: true, kind: danger ? "danger" : "primary" },
    ],
    cancelValue: false,
    // Destructive actions need a deliberate click: Enter cancels.
    focusButton: danger ? 0 : 1,
  });
}

/** Trust-on-first-use dialog. Defaults to "no". */
export function confirmHostKey(info: HostKeyInfo, signal?: AbortSignal): Promise<boolean> {
  const target = info.port === 22 ? info.host : `[${info.host}]:${info.port}`;
  return modal({
    title: t("hostKeyTitle"),
    className: "hostkey",
    body: [
      h("p", {}, t("hostKeyBody", { host: target })),
      h(
        "dl",
        { class: "kv" },
        h("dt", {}, t("hostKeyAlgorithm")),
        h("dd", {}, info.algorithm),
        h("dt", {}, t("hostKeyFingerprint")),
        h("dd", { class: "mono selectable" }, info.fingerprint),
      ),
    ],
    buttons: [
      { label: t("cancel"), value: false },
      { label: t("trustAndConnect"), value: true, kind: "danger" },
    ],
    cancelValue: false,
    // "Cancel" is the default: trusting a key must be a deliberate click.
    focusButton: 0,
    signal,
  });
}

/**
 * `quiet`: the user is typing elsewhere (another pane), so the field does not
 * take the focus at once: keys meant for that pane must not end up here.
 */
export function askSecret(
  prompt: SecretPrompt,
  vaultAvailable: boolean,
  signal?: AbortSignal,
  quiet = false,
): Promise<{ secret: string; remember: boolean } | null> {
  const input = h("input", {
    type: prompt.kind === "challenge" && prompt.echo ? "text" : "password",
    autocomplete: "off",
    autocapitalize: "off",
    spellcheck: "false",
    class: "input",
  });
  const remember = h("input", { type: "checkbox" });

  let title: string;
  const body: Node[] = [];
  switch (prompt.kind) {
    case "password":
      title = t("passwordTitle", { target: prompt.target });
      break;
    case "keyPassphrase":
      title = t("passphraseTitle", { path: prompt.path });
      break;
    case "challenge":
      title = t("challengeTitle", { target: prompt.target });
      // Server text: shown with invisible and reordering characters made visible.
      if (prompt.name) body.push(h("p", { class: "muted" }, displayName(prompt.name)));
      if (prompt.instructions) body.push(h("p", { class: "pre" }, displayName(prompt.instructions, true)));
      break;
  }
  const label =
    prompt.kind === "challenge"
      ? displayName(prompt.prompt)
      : prompt.kind === "password"
        ? t("authPassword")
        : t("authPassphrase");
  body.push(h("label", { class: "field" }, h("span", {}, label), input));
  if (prompt.canRemember) {
    body.push(
      vaultAvailable
        ? h("label", { class: "check" }, remember, h("span", {}, t("remember")))
        : h("p", { class: "muted" }, t("rememberUnavailable")),
    );
  }

  return modal<{ secret: string; remember: boolean } | null>({
    title,
    body,
    buttons: [
      { label: t("cancel"), value: null },
      { label: t("ok"), value: null, kind: "primary" },
    ],
    cancelValue: null,
    initialFocus: quiet ? undefined : input,
    focusButton: 0,
    signal,
    onSubmit: () => {
      const result = { secret: input.value, remember: vaultAvailable && remember.checked };
      input.value = "";
      return result;
    },
  });
}
