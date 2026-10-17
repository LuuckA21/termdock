// A tab holds a binary tree of panes: every split divides one pane in two,
// side by side ("right") or stacked ("down"), with a draggable divider.
import { h } from "./dom";
import type { PaneState, TerminalPane } from "./terminal";

export type SplitDir = "right" | "down";
export type Direction = "left" | "right" | "up" | "down";

interface PaneNode {
  kind: "pane";
  pane: TerminalPane;
  parent: SplitNode | null;
}

interface SplitNode {
  kind: "split";
  dir: SplitDir;
  a: Node;
  b: Node;
  /** Share of the space taken by `a`. */
  ratio: number;
  el: HTMLElement;
  parent: SplitNode | null;
}

type Node = PaneNode | SplitNode;

const MIN_RATIO = 0.1;

const elementOf = (n: Node): HTMLElement => (n.kind === "pane" ? n.pane.el : n.el);

export class Tab {
  readonly el = h("div", { class: "tab-panel", role: "tabpanel" });
  private readonly nodes = new Map<TerminalPane, PaneNode>();
  private current: TerminalPane;

  constructor(first: TerminalPane) {
    this.leaf(first, null);
    this.current = first;
    this.el.append(first.el);
    this.highlight();
  }

  /** The pane with focus in this tab: title, state and colour of the tab. */
  get active(): TerminalPane {
    return this.current;
  }

  get title(): string {
    return this.current.title;
  }

  get state(): PaneState {
    return this.current.state;
  }

  get panes(): TerminalPane[] {
    return [...this.nodes.keys()];
  }

  has(pane: TerminalPane): boolean {
    return this.nodes.has(pane);
  }

  /** Marks `pane` as the active one; `focus` also moves the keyboard focus. */
  setActive(pane: TerminalPane, focus: boolean): void {
    if (!this.nodes.has(pane)) return;
    this.current = pane;
    this.highlight();
    if (focus) pane.focus();
  }

  /** Splits `target` in two; `created` takes the right or bottom half. */
  split(target: TerminalPane, dir: SplitDir, created: TerminalPane): void {
    const node = this.nodes.get(target);
    if (!node) return;
    const split: SplitNode = {
      kind: "split",
      dir,
      a: node,
      b: this.leaf(created, null),
      ratio: 0.5,
      el: h("div", { class: `split split-${dir}` }),
      parent: node.parent,
    };
    this.replace(node, split);
    node.parent = split;
    split.b.parent = split;

    const divider = h("div", { class: "divider", role: "separator", "aria-orientation": dir === "right" ? "vertical" : "horizontal" });
    divider.addEventListener("pointerdown", (e) => this.drag(split, divider, e));
    // Moving a focused element does not fire "blur": xterm.js would keep
    // drawing it as focused.
    target.term.blur();
    target.el.replaceWith(split.el);
    split.el.append(target.el, divider, created.el);
    this.size(split);
    // The new split takes the place (and the share) of the pane it divides.
    if (split.parent) this.size(split.parent);
    this.highlight();
  }

  /**
   * Removes and disposes `pane`. Returns true when it was the last one, so
   * the caller closes the tab.
   */
  remove(pane: TerminalPane): boolean {
    const node = this.nodes.get(pane);
    if (!node) return this.nodes.size === 0;
    this.nodes.delete(pane);
    pane.dispose();
    const parent = node.parent;
    if (!parent) return true;

    const sibling = parent.a === node ? parent.b : parent.a;
    this.replace(parent, sibling);
    sibling.parent = parent.parent;
    const el = elementOf(sibling);
    parent.el.replaceWith(el);
    // The sibling takes the place (and the share) of the removed split.
    if (sibling.parent) this.size(sibling.parent);
    else el.style.flex = "";

    if (this.current === pane) this.current = this.firstPane(sibling);
    this.highlight();
    return false;
  }

  /** Disposes every pane. */
  dispose(): void {
    for (const pane of this.nodes.keys()) pane.dispose();
    this.nodes.clear();
    this.el.remove();
  }

  /** The closest pane next to the active one in `dir`, by on-screen position. */
  neighbor(dir: Direction): TerminalPane | null {
    const from = this.current.el.getBoundingClientRect();
    let best: TerminalPane | null = null;
    let bestDistance = Infinity;
    for (const pane of this.nodes.keys()) {
      if (pane === this.current) continue;
      const r = pane.el.getBoundingClientRect();
      const horizontal = dir === "left" || dir === "right";
      // Must overlap on the other axis…
      const overlap = horizontal
        ? Math.min(from.bottom, r.bottom) - Math.max(from.top, r.top)
        : Math.min(from.right, r.right) - Math.max(from.left, r.left);
      if (overlap <= 0) continue;
      // …and lie on the requested side.
      const distance = {
        left: from.left - r.right,
        right: r.left - from.right,
        up: from.top - r.bottom,
        down: r.top - from.bottom,
      }[dir];
      if (distance < -1) continue;
      if (distance < bestDistance) {
        bestDistance = distance;
        best = pane;
      }
    }
    return best;
  }

  private leaf(pane: TerminalPane, parent: SplitNode | null): PaneNode {
    const node: PaneNode = { kind: "pane", pane, parent };
    this.nodes.set(pane, node);
    return node;
  }

  /** Puts `next` where `prev` is in the tree (not in the DOM). */
  private replace(prev: Node, next: Node): void {
    const parent = prev.parent;
    if (parent?.a === prev) parent.a = next;
    else if (parent) parent.b = next;
  }

  private firstPane(node: Node): TerminalPane {
    return node.kind === "pane" ? node.pane : this.firstPane(node.a);
  }

  private size(split: SplitNode): void {
    elementOf(split.a).style.flex = `${split.ratio} 1 0px`;
    elementOf(split.b).style.flex = `${1 - split.ratio} 1 0px`;
  }

  private highlight(): void {
    const several = this.nodes.size > 1;
    for (const pane of this.nodes.keys()) pane.el.classList.toggle("focused", several && pane === this.current);
  }

  private drag(split: SplitNode, divider: HTMLElement, start: PointerEvent): void {
    start.preventDefault();
    divider.setPointerCapture(start.pointerId);
    divider.classList.add("dragging");
    const move = (e: PointerEvent) => {
      const r = split.el.getBoundingClientRect();
      const ratio = split.dir === "right" ? (e.clientX - r.left) / r.width : (e.clientY - r.top) / r.height;
      split.ratio = Math.min(1 - MIN_RATIO, Math.max(MIN_RATIO, ratio));
      this.size(split);
    };
    const end = () => {
      divider.classList.remove("dragging");
      divider.removeEventListener("pointermove", move);
      divider.removeEventListener("pointerup", end);
      divider.removeEventListener("pointercancel", end);
      this.current.focus();
    };
    divider.addEventListener("pointermove", move);
    divider.addEventListener("pointerup", end);
    divider.addEventListener("pointercancel", end);
  }
}
