import { api } from "./api";
import { modal } from "./dialogs";
import { h } from "./dom";
import { LANGUAGES, languageSetting, setLanguage, systemLanguage, t, type LanguageSetting } from "./i18n";
import {
  DEFAULT_FONT_STACK,
  DEFAULTS,
  getPrefs,
  LIMITS,
  normalize,
  setPrefs,
  type CursorStyle,
  type Prefs,
  type ThemeSetting,
} from "./prefs";
import { SCHEMES, type SchemeId } from "./schemes";
import { autoCheckEnabled, check, setAutoCheck } from "./updates";

const FONT_SUGGESTIONS = [
  "Cascadia Mono",
  "Cascadia Code",
  "Consolas",
  "JetBrains Mono",
  "Fira Code",
  "Source Code Pro",
  "SF Mono",
  "Menlo",
  "DejaVu Sans Mono",
  "Ubuntu Mono",
  "Courier New",
];

function select<T extends string>(value: T, options: [T, string][]): HTMLSelectElement {
  const el = h("select", { class: "input" }, ...options.map(([v, label]) => h("option", { value: v }, label)));
  el.value = value;
  return el;
}

function number(value: number, min: number, max: number, step: number): HTMLInputElement {
  const el = h("input", { class: "input", type: "number", min, max, step });
  el.value = String(value);
  return el;
}

function field(label: string, control: HTMLElement): HTMLElement {
  return h("label", { class: "field" }, h("span", {}, label), control);
}

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

  // ---- appearance
  const p = getPrefs();
  const theme = select<ThemeSetting>(p.theme, [
    ["system", t("themeSystem")],
    ["dark", t("themeDark")],
    ["light", t("themeLight")],
  ]);
  const scheme = select<SchemeId | "auto">(p.scheme, [
    ["auto", t("schemeAuto")],
    ...Object.entries(SCHEMES).map(([id, s]) => [id as SchemeId, s.name] as [SchemeId, string]),
  ]);
  const fontList = h("datalist", { id: "settings-fonts" }, ...FONT_SUGGESTIONS.map((f) => h("option", { value: f })));
  const fontFamily = h("input", {
    class: "input",
    list: "settings-fonts",
    maxlength: LIMITS.fontFamilyLength,
    placeholder: t("fontDefault"),
    spellcheck: "false",
    autocomplete: "off",
  });
  fontFamily.value = p.fontFamily;
  const fontSize = number(p.fontSize, LIMITS.fontSize.min, LIMITS.fontSize.max, 1);
  const lineHeight = number(p.lineHeight, LIMITS.lineHeight.min, LIMITS.lineHeight.max, 0.05);
  const cursorStyle = select<CursorStyle>(p.cursorStyle, [
    ["block", t("cursorBlock")],
    ["bar", t("cursorBar")],
    ["underline", t("cursorUnderline")],
  ]);
  const cursorBlink = h("input", { type: "checkbox" });
  cursorBlink.checked = p.cursorBlink;
  const scrollback = number(p.scrollback, LIMITS.scrollback.min, LIMITS.scrollback.max, 1000);

  const formPrefs = (): Prefs =>
    normalize({
      theme: theme.value,
      scheme: scheme.value,
      fontFamily: fontFamily.value,
      fontSize: fontSize.value,
      lineHeight: lineHeight.value,
      cursorStyle: cursorStyle.value,
      cursorBlink: cursorBlink.checked,
      scrollback: scrollback.value,
    });

  // Live preview of colours and font, before saving.
  const preview = h("div", { class: "appearance-preview", "aria-hidden": "true" });
  const renderPreview = () => {
    const next = formPrefs();
    const dark = next.theme === "system" ? window.matchMedia("(prefers-color-scheme: dark)").matches : next.theme === "dark";
    const colors = (next.scheme === "auto" ? SCHEMES[dark ? "termdock-dark" : "termdock-light"] : SCHEMES[next.scheme])
      .colors;
    preview.replaceChildren(
      h("div", {}, h("span", { "data-c": "green" }, "user@host"), ":", h("span", { "data-c": "blue" }, "~"), "$ ls -l"),
      h(
        "div",
        {},
        ...(["red", "green", "yellow", "blue", "magenta", "cyan"] as const).map((c) => h("span", { "data-c": c }, "■ ")),
      ),
    );
    preview.style.background = colors.background ?? "";
    preview.style.color = colors.foreground ?? "";
    preview.style.fontFamily = next.fontFamily ? `"${next.fontFamily}", ${DEFAULT_FONT_STACK}` : DEFAULT_FONT_STACK;
    preview.style.fontSize = `${next.fontSize}px`;
    preview.style.lineHeight = String(next.lineHeight);
    for (const span of preview.querySelectorAll<HTMLElement>("[data-c]")) {
      span.style.color = colors[span.dataset.c as "red"] ?? "";
    }
  };
  for (const control of [theme, scheme, fontFamily, fontSize, lineHeight]) {
    control.addEventListener("input", renderPreview);
  }
  renderPreview();

  const restore = h("button", { type: "button", class: "btn btn-ghost" }, t("restoreDefaults"));
  restore.addEventListener("click", () => {
    theme.value = DEFAULTS.theme;
    scheme.value = DEFAULTS.scheme;
    fontFamily.value = DEFAULTS.fontFamily;
    fontSize.value = String(DEFAULTS.fontSize);
    lineHeight.value = String(DEFAULTS.lineHeight);
    cursorStyle.value = DEFAULTS.cursorStyle;
    cursorBlink.checked = DEFAULTS.cursorBlink;
    scrollback.value = String(DEFAULTS.scrollback);
    renderPreview();
  });

  // ---- updates
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

  const chosen = await modal<{ language: LanguageSetting; autoUpdate: boolean; prefs: Prefs } | null>({
    title: t("settings"),
    className: "settings",
    body: [
      field(t("language"), language),
      h(
        "fieldset",
        { class: "settings-group" },
        h("legend", {}, t("appearance")),
        h("div", { class: "settings-grid" }, field(t("theme"), theme), field(t("colorScheme"), scheme)),
        field(t("font"), fontFamily),
        fontList,
        h(
          "div",
          { class: "settings-grid" },
          field(t("fontSize"), fontSize),
          field(t("lineHeight"), lineHeight),
          field(t("cursor"), cursorStyle),
          field(t("scrollback"), scrollback),
        ),
        h("label", { class: "check" }, cursorBlink, h("span", {}, t("cursorBlink"))),
        preview,
        h("div", { class: "settings-row" }, h("span"), restore),
      ),
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
    onSubmit: () => ({ language: language.value as LanguageSetting, autoUpdate: autoUpdate.checked, prefs: formPrefs() }),
  });
  if (!chosen) return;
  setAutoCheck(chosen.autoUpdate);
  setPrefs(chosen.prefs);
  setLanguage(chosen.language);
}
