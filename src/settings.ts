import { api, errorText } from "./api";
import { modal } from "./dialogs";
import { h } from "./dom";
import { LANGUAGES, languageSetting, setLanguage, systemLanguage, t, type LanguageSetting } from "./i18n";
import {
  DEFAULTS,
  fontStack,
  getPrefs,
  LIMITS,
  normalize,
  setPrefs,
  terminalScheme,
  type CursorStyle,
  type Prefs,
  type ThemeSetting,
} from "./prefs";
import { SCHEMES, type SchemeId } from "./schemes";
import type { EditorInfo } from "./types";
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
  const scrollback = number(p.scrollback, LIMITS.scrollback.min, LIMITS.scrollback.max, 1);

  const formPrefs = (): Prefs =>
    normalize({
      theme: theme.value,
      scheme: scheme.value,
      fontFamily: fontFamily.value,
      // An emptied number field keeps the current value.
      fontSize: fontSize.value || p.fontSize,
      lineHeight: lineHeight.value || p.lineHeight,
      cursorStyle: cursorStyle.value,
      cursorBlink: cursorBlink.checked,
      scrollback: scrollback.value || p.scrollback,
    });

  // Live preview of colours and font, before saving.
  const preview = h("div", { class: "appearance-preview", "aria-hidden": "true" });
  const renderPreview = () => {
    const next = formPrefs();
    const { colors } = terminalScheme(next);
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
    preview.style.fontFamily = fontStack(next);
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

  // ---- editor for Edit: kept by the backend, changed at once (the program
  // is only ever picked in the system's own file dialog)
  const editorName = h("span", { class: "editor-name" });
  const editorStatus = h("p", { class: "muted", role: "status" });
  const editorChoose = h("button", { type: "button", class: "btn" }, t("editorChoose"));
  const editorReset = h("button", { type: "button", class: "btn btn-ghost" }, t("editorSystem"));
  const showEditor = (info: EditorInfo | null) => {
    editorName.textContent = info ? info.name : t("editorSystemName");
    editorName.title = info ? info.path : "";
    editorReset.hidden = !info;
  };
  showEditor(null);
  api.editorGet().then(showEditor).catch(() => undefined);
  const editorAction = async (run: () => Promise<void>) => {
    editorChoose.disabled = editorReset.disabled = true;
    editorStatus.textContent = "";
    try {
      await run();
    } catch (e) {
      editorStatus.textContent = errorText(e);
    }
    editorChoose.disabled = editorReset.disabled = false;
  };
  editorChoose.addEventListener("click", () =>
    editorAction(async () => {
      const info = await api.editorPick();
      if (info) showEditor(info);
    }),
  );
  editorReset.addEventListener("click", () =>
    editorAction(async () => {
      await api.editorReset();
      showEditor(null);
    }),
  );

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
        h("legend", {}, t("editorGroup")),
        h("div", { class: "settings-row" }, editorName, h("span", { class: "editor-buttons" }, editorReset, editorChoose)),
        h("p", { class: "muted" }, t("editorHelp")),
        editorStatus,
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
