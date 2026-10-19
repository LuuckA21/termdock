import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  ImportResult,
  SshConfigPreview,
  DetectedShell,
  OpenTarget,
  Profile,
  ProfileList,
  PromptAnswer,
  Reconnected,
  SessionNotice,
  EditEvent,
  EditorInfo,
  ImportPreview,
  KeyInfo,
  PickedFile,
  PlanInfo,
  RunProgress,
  RunSummary,
  SftpOpened,
  RemoteListing,
  TermSize,
  UpdateInfo,
  UpdateProgress,
  Uuid,
} from "./types";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  listProfiles: () => invoke<ProfileList>("list_profiles"),
  saveProfile: (profile: Profile, favorite?: boolean) =>
    invoke<Profile>("save_profile", { profile, favorite }),
  deleteProfile: (id: Uuid) => invoke<void>("delete_profile", { id }),
  setFavorite: (id: Uuid, favorite: boolean) => invoke<void>("set_favorite", { id, favorite }),
  reorderFavorites: (order: Uuid[]) => invoke<void>("reorder_favorites", { order }),
  forgetSecrets: (id: Uuid) => invoke<void>("forget_secrets", { id }),
  forgetHostKey: (host: string, port: number) => invoke<number>("forget_host_key", { host, port }),
  listShells: () => invoke<DetectedShell[]>("list_shells"),
  previewSshConfig: () => invoke<SshConfigPreview>("preview_ssh_config"),
  importSshConfig: (aliases: string[]) => invoke<ImportResult>("import_ssh_config", { aliases }),

  openSession: (
    target: OpenTarget,
    size: TermSize,
    onData: (bytes: Uint8Array) => void,
    onNotice: (notice: SessionNotice) => void,
  ) => {
    const data = new Channel<ArrayBuffer | number[]>();
    data.onmessage = (msg) => onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg));
    const notices = new Channel<SessionNotice>();
    notices.onmessage = onNotice;
    return invoke<number>("open_session", { target, size, onData: data, onNotice: notices });
  },
  writeSession: (id: number, data: string, binary = false) => invoke<void>("write_session", { id, data, binary }),
  resizeSession: (id: number, size: TermSize) => invoke<void>("resize_session", { id, size }),
  closeSession: (id: number) => invoke<void>("close_session", { id }),
  /** Generates an Ed25519 key pair saved where the user picks; null when cancelled. */
  sshKeyGenerate: (comment: string, passphrase: string) =>
    invoke<KeyInfo | null>("ssh_key_generate", { comment, passphrase }),
  /** Reads a public key the user picks; null when cancelled. */
  sshKeyPickPublic: () => invoke<KeyInfo | null>("ssh_key_pick_public"),
  /** Adds a public key to ~/.ssh/authorized_keys on the server of session `id`; false if already there. */
  sshKeyInstall: (id: number, publicKey: string) => invoke<boolean>("ssh_key_install", { id, publicKey }),
  /** One attempt to connect again a session whose connection dropped (same id). */
  reconnectSession: (id: number, size: TermSize) => invoke<Reconnected>("reconnect_session", { id, size }),
  answerPrompt: (promptId: Uuid, answer: PromptAnswer) =>
    invoke<boolean>("answer_prompt", { promptId, answer }),

  /** `user`: open as that user through sudo (which may ask for a password). */
  sftpOpen: (session: number, user: string | null) => invoke<SftpOpened>("sftp_open", { session, user }),
  /** Answers the sudo prompt of a pending open; the password is not kept. */
  sftpSudoPassword: (pending: number, password: string) =>
    invoke<SftpOpened>("sftp_sudo_password", { pending, password }),
  sftpSudoCancel: (pending: number) => invoke<void>("sftp_sudo_cancel", { pending }),
  sftpClose: (id: number) => invoke<void>("sftp_close", { id }),
  sftpList: (id: number, path: string) => invoke<RemoteListing>("sftp_list", { id, path }),
  sftpMkdir: (id: number, dir: string, name: string) => invoke<void>("sftp_mkdir", { id, dir, name }),
  sftpRename: (id: number, dir: string, from: string, to: string) =>
    invoke<void>("sftp_rename", { id, dir, from, to }),
  sftpRemove: (id: number, dir: string, name: string) => invoke<void>("sftp_remove", { id, dir, name }),
  sftpCancel: (transfer: number) => invoke<void>("sftp_cancel", { transfer }),
  /** Shows a native file picker; resolves with tokens for the picked files. */
  sftpPickUpload: () => invoke<PickedFile[]>("sftp_pick_upload"),
  /** Shows a native folder picker; resolves with tokens for the picked folders. */
  sftpPickUploadFolders: () => invoke<PickedFile[]>("sftp_pick_upload_folders"),
  /** Plans uploading picked files/folders into `dir` (the tokens are used up). */
  sftpPlanUpload: (id: number, dir: string, tokens: string[]) =>
    invoke<PlanInfo>("sftp_plan_upload", { id, dir, tokens }),
  /** Asks where to save `dir/name` (file or folder) and plans it; null if the dialog was cancelled. */
  sftpPlanDownload: (id: number, dir: string, name: string) =>
    invoke<PlanInfo | null>("sftp_plan_download", { id, dir, name }),
  /** Runs a plan; `replace`: what already exists is replaced, otherwise kept. */
  sftpRunPlan: (plan: number, replace: boolean, onProgress: (p: RunProgress) => void) => {
    const channel = new Channel<RunProgress>();
    channel.onmessage = onProgress;
    return invoke<RunSummary>("sftp_run_plan", { plan, replace, onProgress: channel });
  },
  sftpDropPlan: (plan: number) => invoke<void>("sftp_drop_plan", { plan }),
  sftpForgetPicks: (tokens: string[]) => invoke<void>("sftp_forget_picks", { tokens }),
  /** Opens a remote text file in the system text editor; saves are uploaded back. */
  sftpEdit: (id: number, dir: string, name: string, onEvent: (e: EditEvent) => void) => {
    const channel = new Channel<EditEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("sftp_edit", { id, dir, name, onEvent: channel });
  },
  sftpEditResolve: (edit: number, overwrite: boolean) => invoke<void>("sftp_edit_resolve", { edit, overwrite }),
  editorGet: () => invoke<EditorInfo | null>("editor_get"),
  /** Shows a native file picker for the editor; resolves with the new choice, or null if cancelled. */
  editorPick: () => invoke<EditorInfo | null>("editor_pick"),
  editorReset: () => invoke<void>("editor_reset"),

  /** Shows a save dialog and logs the session there; resolves with the path, or null if cancelled. */
  sessionLogStart: (id: number, title: string) => invoke<string | null>("session_log_start", { id, title }),
  sessionLogStop: (id: number) => invoke<boolean>("session_log_stop", { id }),
  /** Shows a save dialog and writes a backup (connections + `ui`); resolves with the path, or null. */
  configExport: (ui: unknown) => invoke<string | null>("config_export", { ui }),
  /** Shows an open dialog and reads a backup; nothing changes until `configImportApply`. */
  configImportPick: () => invoke<ImportPreview | null>("config_import_pick"),
  configImportApply: (token: string, replace: boolean) => invoke<number>("config_import_apply", { token, replace }),
  configImportCancel: () => invoke<void>("config_import_cancel"),

  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: (onProgress: (p: UpdateProgress) => void) => {
    const progress = new Channel<UpdateProgress>();
    progress.onmessage = onProgress;
    return invoke<void>("install_update", { onProgress: progress });
  },
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
