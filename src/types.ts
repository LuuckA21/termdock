// Mirrors of the Rust types exchanged over IPC (see crates/termdock-core).

export type Uuid = string;

export type SshAuth =
  | { method: "auto" }
  | { method: "agent" }
  | { method: "key"; path: string }
  | { method: "password" };

export type Forward =
  | { kind: "local"; bindHost: string; bindPort: number; destHost: string; destPort: number }
  | { kind: "remote"; bindHost: string; bindPort: number; destHost: string; destPort: number }
  | { kind: "dynamic"; bindHost: string; bindPort: number };

export interface SshFields {
  type: "ssh";
  host: string;
  port: number;
  username: string;
  auth: SshAuth;
  keepaliveSecs: number;
  /** Saved SSH connection used as jump host (ProxyJump). */
  jump?: Uuid;
  forwards?: Forward[];
}

export interface LocalFields {
  type: "local";
  program: string;
  args: string[];
  cwd?: string;
}

export type Profile = {
  id: Uuid;
  name: string;
  color?: string;
  group?: string;
} & (SshFields | LocalFields);

export interface ProfileList {
  profiles: Profile[];
  favorites: Uuid[];
}

/** The editor chosen in Settings for Edit; null means the system text editor. */
export interface EditorInfo {
  name: string;
  /** Full path, shown as a hint. */
  path: string;
}

export interface AppInfo {
  version: string;
  vaultAvailable: boolean;
  platform: string;
}

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string;
  /** False for deb/rpm packages and dev builds: they are updated another way. */
  installable: boolean;
}

export type UpdateProgress =
  | { stage: "downloading"; downloaded: number; total: number | null }
  | { stage: "installing" };

export type EntryKind = "dir" | "file" | "link" | "other";

export interface RemoteEntry {
  name: string;
  kind: EntryKind;
  size: number | null;
  /** Seconds since the Unix epoch. */
  modified: number | null;
  mode: number | null;
}

export interface RemoteListing {
  path: string;
  entries: RemoteEntry[];
  truncated: boolean;
}

/** What a planned transfer will do, before it starts. */
export interface PlanInfo {
  plan: number;
  files: number;
  folders: number;
  bytes: number;
  /** Files that already exist at the destination, and a few of their paths. */
  existing: number;
  examples: string[];
  /** Left out: links, special files, unusable names, name clashes. */
  skipped: number;
}

export interface RunProgress {
  transfer: number;
  /** Bytes copied, of `total`. */
  done: number;
  total: number;
  /** Files finished, of `count`. */
  files: number;
  count: number;
  /** The file being copied (a path below the transferred folder). */
  current: string;
}

export interface RunSummary {
  copied: number;
  skipped: number;
  failed: number;
  /** The first errors, as "path: reason". */
  errors: string[];
  /** Stopped early after repeated failures. */
  stopped: boolean;
}

export type SftpOpened =
  | { status: "open"; id: number; home: string; user: string; loginUser: string }
  | { status: "passwordRequired"; pending: number; retry: boolean; asksFor: string | null };

export type EditEvent =
  | { kind: "saved"; edit: number }
  | { kind: "conflict"; edit: number }
  | { kind: "failed"; edit: number; message: string }
  | { kind: "stopped"; edit: number };

/** What a backup file holds, before importing it. */
export interface ImportPreview {
  token: string;
  /** Termdock version that wrote it. */
  app: string;
  /** Seconds since the Unix epoch. */
  exported: number;
  profiles: number;
  newProfiles: number;
  newNames: string[];
  favorites: number;
  /** The interface's settings, validated before use. */
  ui: unknown;
}

export interface PickedFile {
  token: string;
  name: string;
  /** Bytes for a file; 0 for a folder. */
  size: number;
  folder: boolean;
}

export interface DetectedShell {
  id: string;
  name: string;
  shell: { program: string; args: string[]; cwd?: string };
  isDefault: boolean;
}

export interface TermSize {
  cols: number;
  rows: number;
}

export type OpenTarget =
  | { kind: "profile"; id: Uuid }
  | { kind: "quick"; address: string }
  | { kind: "shell"; id: string };

export interface HostKeyInfo {
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
}

export type SecretPrompt =
  | { kind: "password"; target: string; canRemember: boolean }
  | { kind: "keyPassphrase"; path: string; canRemember: boolean }
  | {
      kind: "challenge";
      target: string;
      name: string;
      instructions: string;
      prompt: string;
      echo: boolean;
      canRemember: boolean;
    };

export type SessionNotice =
  | { type: "hostKey"; promptId: Uuid; info: HostKeyInfo }
  | { type: "secret"; promptId: Uuid; prompt: SecretPrompt }
  | { type: "banner"; text: string }
  | { type: "info"; text: string }
  /** `lost`: the connection dropped; the session waits for `reconnectSession` (or to be closed). */
  | { type: "exit"; code: number | null; message: string | null; lost: boolean };

/** A public key read or generated by the backend (never the private key). */
export interface KeyInfo {
  /** Where the private key was saved (generation only). */
  path: string | null;
  /** The `authorized_keys` line. */
  publicKey: string;
  fingerprint: string;
  comment: string;
}

export type Reconnected =
  | { status: "connected" }
  /** `retry`: trying again later may work (network, server restarting). */
  | { status: "failed"; error: string; retry: boolean }
  | { status: "closed" };

export type PromptAnswer =
  | { kind: "hostKey"; accept: boolean }
  | { kind: "secret"; secret: string; remember: boolean }
  | { kind: "cancel" };

export interface SshConfigPreview {
  path: string;
  found: boolean;
  items: {
    alias: string;
    target: string;
    jump: string | null;
    forwards: string[];
    warnings: string[];
    existing: string | null;
  }[];
  warnings: string[];
}

export interface ImportResult {
  imported: string[];
  warnings: string[];
}
