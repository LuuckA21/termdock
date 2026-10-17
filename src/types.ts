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

export interface TransferProgress {
  transfer: number;
  done: number;
  total: number | null;
}

export type SftpOpened =
  | { status: "open"; id: number; home: string; user: string; loginUser: string }
  | { status: "passwordRequired"; pending: number; retry: boolean; asksFor: string | null };

export interface PickedFile {
  token: string;
  name: string;
  size: number;
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
  | { type: "exit"; code: number | null; message: string | null };

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
