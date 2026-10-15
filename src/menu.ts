import { h } from "./dom";

export interface MenuItem {
  label: string;
  icon?: Node;
  hint?: string;
  danger?: boolean;
  action: () => void;
}

let current: { el: HTMLElement; cleanup: () => void } | null = null;

export function closeMenu(): void {
  current?.cleanup();
  current = null;
}

/** Shows a small popup menu at the given viewport position. */
export function showMenu(x: number, y: number, items: MenuItem[]): void {
  closeMenu();
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
    if (e.key === "Escape") closeMenu();
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const buttons = [...el.querySelectorAll<HTMLButtonElement>("button")];
      const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = (i + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
      buttons[next]?.focus();
    }
  };
  setTimeout(() => document.addEventListener("mousedown", onDown), 0);
  document.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", closeMenu, { once: true });
  current = {
    el,
    cleanup: () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
      el.remove();
    },
  };
}

export function showMenuBelow(anchor: HTMLElement, items: MenuItem[]): void {
  const r = anchor.getBoundingClientRect();
  showMenu(r.left, r.bottom + 4, items);
}
