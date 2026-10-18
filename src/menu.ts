import { h } from "./dom";

interface MenuItem {
  label: string;
  icon?: Node;
  hint?: string;
  danger?: boolean;
  action: () => void;
}

let current: { el: HTMLElement; cleanup: (restore: boolean) => void } | null = null;

/** `restore`: give the focus back to where it was before the menu opened. */
function closeMenu(restore = false): void {
  current?.cleanup(restore);
  current = null;
}

/** Shows a small popup menu at the given viewport position. */
export function showMenu(x: number, y: number, items: MenuItem[]): void {
  closeMenu();
  const before = document.activeElement;
  const restoreFocus = () => {
    // Not into a tab that is no longer shown (the menu of another tab).
    if (before instanceof HTMLElement && before.isConnected && before.offsetParent !== null) before.focus();
  };
  const el = h("div", { class: "menu", role: "menu" });
  for (const item of items) {
    const btn = h(
      "button",
      { class: `menu-item${item.danger ? " danger" : ""}`, role: "menuitem", type: "button" },
      item.icon ?? "",
      h("span", { class: "menu-label" }, item.label),
      item.hint ? h("span", { class: "menu-hint" }, item.hint) : "",
    );
    btn.addEventListener("click", () => {
      closeMenu();
      item.action();
      // An action that does not move the focus leaves it where it was.
      if (document.activeElement === document.body || document.activeElement === null) restoreFocus();
    });
    el.append(btn);
  }
  document.body.append(el);
  const rect = el.getBoundingClientRect();
  el.style.left = `${Math.max(4, Math.min(x, window.innerWidth - rect.width - 4))}px`;
  el.style.top = `${Math.max(4, Math.min(y, window.innerHeight - rect.height - 4))}px`;
  (el.querySelector("button") as HTMLButtonElement | null)?.focus();

  const onDown = (e: MouseEvent) => {
    if (!el.contains(e.target as Node)) closeMenu();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      closeMenu(true);
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      const buttons = [...el.querySelectorAll<HTMLButtonElement>("button")];
      const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = (i + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
      buttons[next]?.focus();
    }
  };
  setTimeout(() => document.addEventListener("mousedown", onDown), 0);
  document.addEventListener("keydown", onKey, true);
  const onBlur = () => closeMenu();
  window.addEventListener("blur", onBlur);
  current = {
    el,
    cleanup: (restore) => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onBlur);
      el.remove();
      if (restore) restoreFocus();
    },
  };
}

export function showMenuBelow(anchor: HTMLElement, items: MenuItem[]): void {
  const r = anchor.getBoundingClientRect();
  showMenu(r.left, r.bottom + 4, items);
}
