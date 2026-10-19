// Addresses that quick connect reached, newest first, offered again in its
// box. Only `user@host:port` strings (no secrets), in local storage, checked
// on every read.
const STORAGE_KEY = "termdock.quickHistory";
const MAX_ENTRIES = 10;
const MAX_LENGTH = 255;

function valid(v: unknown): v is string {
  return typeof v === "string" && v.length > 0 && v.length <= MAX_LENGTH && !/[\p{Cc}\p{Cf}\s]/u.test(v);
}

export function quickHistory(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]");
    return Array.isArray(raw) ? [...new Set(raw.filter(valid))].slice(0, MAX_ENTRIES) : [];
  } catch {
    return [];
  }
}

function save(list: string[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
  } catch {
    // Not kept, nothing else depends on it.
  }
}

/** Puts `address` first (it just connected). */
export function rememberQuick(address: string): void {
  if (!valid(address)) return;
  const list = quickHistory();
  if (list[0] === address) return;
  save([address, ...list.filter((a) => a !== address)].slice(0, MAX_ENTRIES));
}

export function clearQuickHistory(): void {
  save([]);
}
