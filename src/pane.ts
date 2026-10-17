// What a tab holds in each slot of its split layout: a terminal or a file
// browser (SFTP) bound to a terminal's SSH connection.
import type { OpenTarget } from "./types";

export type PaneState = "connecting" | "open" | "closed";

export interface PaneOptions {
  title: string;
  subtitle: string;
  color?: string;
  target: OpenTarget;
  onStateChange: (pane: Pane) => void;
  /** The pane got keyboard focus (click or programmatic). */
  onFocus: (pane: Pane) => void;
  /** Keys the app handles itself (tabs, panes, settings…): panes ignore them. */
  isAppShortcut: (e: KeyboardEvent) => boolean;
}

export interface Pane {
  readonly el: HTMLElement;
  readonly opts: PaneOptions;
  readonly state: PaneState;
  readonly title: string;
  focus(): void;
  /** Called before the pane is moved in the DOM (moving does not fire "blur"). */
  blur(): void;
  dispose(): void;
}
