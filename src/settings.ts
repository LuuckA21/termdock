import { api } from "./api";
import { modal } from "./dialogs";
import { h } from "./dom";
import { LANGUAGES, languageSetting, setLanguage, systemLanguage, t, type LanguageSetting } from "./i18n";
import { autoCheckEnabled, check, setAutoCheck } from "./updates";

/** Settings dialog. Choices are applied when the dialog is confirmed. */
export async function openSettings(): Promise<void> {
  const system = LANGUAGES.find((l) => l.code === systemLanguage())?.name ?? "English";
  const language = h(
    "select",
    { class: "input", id: "settings-language" },
    h("option", { value: "auto" }, t("languageAuto", { name: system })),
    ...LANGUAGES.map((l) => h("option", { value: l.code, lang: l.code }, l.name)),
  );
  language.value = languageSetting();

  const autoUpdate = h("input", { type: "checkbox" });
  autoUpdate.checked = autoCheckEnabled();
  const version = h("span", { class: "muted" });
  api
    .appInfo()
    .then((info) => (version.textContent = t("versionLabel", { version: info.version })))
    .catch(() => undefined);
  const status = h("p", { class: "muted pre selectable", role: "status" });
  const checkNow = h("button", { type: "button", class: "btn" }, t("updateCheckNow"));
  checkNow.addEventListener("click", async () => {
    checkNow.disabled = true;
    status.textContent = t("updateChecking");
    status.textContent = (await check(true)) ?? "";
    checkNow.disabled = false;
  });

  const chosen = await modal<{ language: LanguageSetting; autoUpdate: boolean } | null>({
    title: t("settings"),
    className: "settings",
    body: [
      h("label", { class: "field" }, h("span", {}, t("language")), language),
      h(
        "fieldset",
        { class: "settings-group" },
        h("legend", {}, t("updates")),
        h("label", { class: "check" }, autoUpdate, h("span", {}, t("updateAuto"))),
        h("div", { class: "settings-row" }, version, checkNow),
        status,
      ),
    ],
    buttons: [
      { label: t("cancel"), value: null },
      { label: t("save"), value: null, kind: "primary" },
    ],
    cancelValue: null,
    initialFocus: language,
    onSubmit: () => ({ language: language.value as LanguageSetting, autoUpdate: autoUpdate.checked }),
  });
  if (!chosen) return;
  setAutoCheck(chosen.autoUpdate);
  setLanguage(chosen.language);
}
