import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  DetectedShell,
  OpenTarget,
  Profile,
  ProfileList,
  PromptAnswer,
  SessionNotice,
  TermSize,
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
  writeSession: (id: number, data: string) => invoke<void>("write_session", { id, data }),
  resizeSession: (id: number, size: TermSize) => invoke<void>("resize_session", { id, size }),
  closeSession: (id: number) => invoke<void>("close_session", { id }),
  answerPrompt: (promptId: Uuid, answer: PromptAnswer) =>
    invoke<boolean>("answer_prompt", { promptId, answer }),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
