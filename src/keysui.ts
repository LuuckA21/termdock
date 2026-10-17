// Settings > Keys: change, remove or reset the keyboard shortcuts. Works on a
// copy of the key map; Settings saves it when the dialog is confirmed.
import { confirmBox } from "./dialogs";
import { h, icon } from "./dom";
import { t } from "./i18n";
import {
  ACTION_GROUPS,
  altGrRisk,
  chordFromEvent,
  chordLabel,
  chordSpec,
  defaultKeyMap,
  DIGIT_ACTIONS,
  DIGIT_MODS,
  digitChords,
  digitRangeLabel,
  isDefault,
  owner,
  reachesTerminal,
  usable,
  type Action,
  type Chord,
  type DigitAction,
  type DigitMods,
  type KeyMap,
} from "./keys";

function labelOf(who: { action?: Action; digit?: DigitAction }): string {
  if (who.action) {
    const found = ACTION_GROUPS.flatMap((g) => g.actions).find((a) => a.id === who.action);
    return found ? t(found.label) : who.action;
  }
  const digit = DIGIT_ACTIONS.find((d) => d.id === who.digit);
  return digit ? t(digit.label) : "";
}

/** Frees `ch` wherever else it is used in `map`. */
function release(map: KeyMap, who: { action?: Action; digit?: DigitAction }, ch: Chord): void {
  if (who.action) {
    map.actions[who.action] = map.actions[who.action].filter((other) => chordSpec(other) !== chordSpec(ch));
  } else if (who.digit) {
    map.digits[who.digit] = "off";
  }
}

export function keysPanel(map: KeyMap): HTMLElement[] {
  const status = h("p", { class: "keys-status", role: "status" });
  const rows = new Map<Action, () => void>();
  const digitSelects = new Map<DigitAction, HTMLSelectElement>();
  const refreshAll = () => {
    rows.forEach((render) => render());
    digitSelects.forEach((select, id) => (select.value = map.digits[id]));
  };

  const actionRow = (id: Action, label: string) => {
    const keys = h("button", { type: "button", class: "keys-binding" });
    const reset = h("button", { type: "button", class: "icon-btn", title: t("keysReset"), "aria-label": t("keysReset") }, icon("refresh"));
    const remove = h("button", { type: "button", class: "icon-btn", title: t("keysClear"), "aria-label": t("keysClear") }, icon("close"));
    let recording = false;
    const render = () => {
      keys.classList.toggle("recording", recording);
      keys.replaceChildren(
        ...(recording
          ? [h("span", { class: "muted" }, t("keysRecording"))]
          : map.actions[id].length
            ? map.actions[id].map((ch) => h("kbd", {}, chordLabel(ch)))
            : [h("span", { class: "muted" }, t("keysNone"))]),
      );
      keys.setAttribute("aria-label", `${label}: ${map.actions[id].map(chordLabel).join(", ") || t("keysNone")}`);
      reset.style.visibility = isDefault(map, id) ? "hidden" : "visible";
      remove.style.visibility = map.actions[id].length ? "visible" : "hidden";
    };
    rows.set(id, render);
    const stop = () => {
      recording = false;
      render();
    };
    keys.addEventListener("click", () => {
      status.textContent = "";
      recording = !recording;
      render();
    });
    keys.addEventListener("blur", () => recording && stop());
    keys.addEventListener("keydown", async (e) => {
      if (!recording) return;
      // Every key goes to the recorder: Esc must not close Settings, Tab must
      // not move the focus.
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat) return;
      if (e.key === "Escape" && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
        stop();
        return;
      }
      const ch = chordFromEvent(e);
      if (!ch) return; // a modifier on its own: wait for the key
      stop();
      if (!usable(ch)) {
        status.textContent = t("keysNeedModifier");
        return;
      }
      const keysText = chordLabel(ch);
      const who = owner(map, ch, id);
      if (who && !(await confirmBox(t("keysInUse", { keys: keysText, action: labelOf(who) }), t("keysUse")))) return;
      const warning = altGrRisk(ch) ? "keysAltGr" : reachesTerminal(ch) ? "keysReachesTerminal" : null;
      if (warning && !(await confirmBox(t(warning, { keys: keysText }), t("keysUse")))) return;
      if (who) release(map, who, ch);
      map.actions[id] = [ch];
      refreshAll();
      keys.focus();
    });
    reset.addEventListener("click", () => {
      const defaults = defaultKeyMap().actions[id];
      // A default taken meanwhile by another action is left there.
      map.actions[id] = defaults.filter((ch) => !owner(map, ch, id));
      status.textContent = "";
      refreshAll();
    });
    remove.addEventListener("click", () => {
      map.actions[id] = [];
      status.textContent = "";
      refreshAll();
    });
    render();
    return h("div", { class: "keys-row" }, h("span", { class: "keys-label" }, label), keys, reset, remove);
  };

  const digitRow = (id: DigitAction, label: string) => {
    const select = h(
      "select",
      { class: "input keys-digits", "aria-label": label },
      ...DIGIT_MODS.map((m) => h("option", { value: m }, m === "off" ? t("keysOff") : digitRangeLabel(m))),
    );
    select.value = map.digits[id];
    digitSelects.set(id, select);
    select.addEventListener("change", async () => {
      const next = select.value as DigitMods;
      const previous = map.digits[id];
      map.digits[id] = "off";
      const clashes = digitChords(next)
        .map((ch) => ({ ch, who: owner(map, ch) }))
        .filter((c): c is { ch: Chord; who: NonNullable<ReturnType<typeof owner>> } => c.who !== null);
      if (
        clashes.length &&
        !(await confirmBox(
          t("keysInUse", { keys: digitRangeLabel(next), action: labelOf(clashes[0].who) }),
          t("keysUse"),
        ))
      ) {
        map.digits[id] = previous;
        refreshAll();
        return;
      }
      clashes.forEach(({ ch, who }) => release(map, who, ch));
      map.digits[id] = next;
      status.textContent = "";
      refreshAll();
    });
    return h("div", { class: "keys-row" }, h("span", { class: "keys-label" }, label), select);
  };

  const resetAll = h("button", { type: "button", class: "btn btn-ghost" }, t("keysResetAll"));
  resetAll.addEventListener("click", () => {
    const defaults = defaultKeyMap();
    Object.assign(map.actions, defaults.actions);
    Object.assign(map.digits, defaults.digits);
    status.textContent = "";
    refreshAll();
  });

  return [
    h("p", { class: "muted" }, t("keysHelp")),
    status,
    ...ACTION_GROUPS.map((group) =>
      h(
        "div",
        { class: "keys-group" },
        h("h4", {}, t(group.label)),
        ...group.actions.map((a) => actionRow(a.id, t(a.label))),
        ...DIGIT_ACTIONS.filter((d) => d.group === group.label).map((d) => digitRow(d.id, t(d.label))),
      ),
    ),
    h("p", { class: "muted" }, t("keysFixed")),
    h("div", { class: "settings-row" }, h("span"), resetAll),
  ];
}
