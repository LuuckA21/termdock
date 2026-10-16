import { modal } from "./dialogs";
import { h } from "./dom";
import { LANGUAGES, languageSetting, setLanguage, systemLanguage, t, type LanguageSetting } from "./i18n";

/** Settings dialog. The language is applied when the dialog is confirmed. */
export async function openSettings(): Promise<void> {
  const system = LANGUAGES.find((l) => l.code === systemLanguage())?.name ?? "English";
  const language = h(
    "select",
    { class: "input", id: "settings-language" },
    h("option", { value: "auto" }, t("languageAuto", { name: system })),
    ...LANGUAGES.map((l) => h("option", { value: l.code, lang: l.code }, l.name)),
  );
  language.value = languageSetting();

  const chosen = await modal<LanguageSetting | null>({
    title: t("settings"),
    className: "settings",
    body: [h("label", { class: "field" }, h("span", {}, t("language")), language)],
    buttons: [
      { label: t("cancel"), value: null },
      { label: t("save"), value: null, kind: "primary" },
    ],
    cancelValue: null,
    initialFocus: language,
    onSubmit: () => language.value as LanguageSetting,
  });
  if (chosen) setLanguage(chosen);
}
