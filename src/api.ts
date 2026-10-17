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
  SessionNotice,
  EditEvent,
  PickedFile,
  SftpOpened,
  RemoteListing,
  TermSize,
  TransferProgress,
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
  /** Shows a native save dialog; resolves with the saved path, or null if cancelled. */
  sftpDownload: (id: number, dir: string, name: string, onProgress: (p: TransferProgress) => void) =>
    invoke<string | null>("sftp_download", { id, dir, name, onProgress: progressChannel(onProgress) }),
  /** Shows a native file picker; resolves with tokens for the picked files. */
  sftpPickUpload: () => invoke<PickedFile[]>("sftp_pick_upload"),
  sftpForgetPicks: (tokens: string[]) => invoke<void>("sftp_forget_picks", { tokens }),
  /** Opens a remote text file in the system text editor; saves are uploaded back. */
  sftpEdit: (id: number, dir: string, name: string, onEvent: (e: EditEvent) => void) => {
    const channel = new Channel<EditEvent>();
    channel.onmessage = onEvent;
    return invoke<number>("sftp_edit", { id, dir, name, onEvent: channel });
  },
  sftpEditResolve: (edit: number, overwrite: boolean) => invoke<void>("sftp_edit_resolve", { edit, overwrite }),
  sftpEditStop: (edit: number) => invoke<void>("sftp_edit_stop", { edit }),
  sftpUpload: (id: number, dir: string, token: string, overwrite: boolean, onProgress: (p: TransferProgress) => void) =>
    invoke<void>("sftp_upload", { id, dir, token, overwrite, onProgress: progressChannel(onProgress) }),

  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  installUpdate: (onProgress: (p: UpdateProgress) => void) => {
    const progress = new Channel<UpdateProgress>();
    progress.onmessage = onProgress;
    return invoke<void>("install_update", { onProgress: progress });
  },
};

function progressChannel(fn: (p: TransferProgress) => void): Channel<TransferProgress> {
  const channel = new Channel<TransferProgress>();
  channel.onmessage = fn;
  return channel;
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
