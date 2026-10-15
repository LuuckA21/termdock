import { clear, h, icon } from "./dom";
import { t } from "./i18n";
import type { Forward } from "./types";

const LOOPBACK = new Set(["127.0.0.1", "::1", "localhost"]);

/** Editable list of port forwards for the connection form. */
export function forwardsEditor(initial: Forward[]): { el: HTMLElement; value: () => Forward[] } {
  type Row = { kind: Forward["kind"]; bindHost: string; bindPort: string; destHost: string; destPort: string };
  const rows: Row[] = initial.map((f) => ({
    kind: f.kind,
    bindHost: f.bindHost,
    bindPort: String(f.bindPort),
    destHost: f.kind === "dynamic" ? "" : f.destHost,
    destPort: f.kind === "dynamic" ? "" : String(f.destPort),
  }));

  const list = h("div", { class: "fwd-list" });
  const el = h("div", { class: "fwd span2" }, list);

  const exposed = (row: Row) => row.kind !== "remote" && !!row.bindHost.trim() && !LOOPBACK.has(row.bindHost.trim().toLowerCase());

  const textInput = (row: Row, key: "bindHost" | "destHost", placeholder: string, onChange?: () => void) => {
    const i = h("input", { class: "input", spellcheck: "false", autocomplete: "off", placeholder, "aria-label": placeholder });
    i.value = row[key];
    i.addEventListener("input", () => {
      row[key] = i.value;
      onChange?.();
    });
    return i;
  };
  const portInput = (row: Row, key: "bindPort" | "destPort", label: string) => {
    const i = h("input", { class: "input port", type: "number", min: "1", max: "65535", placeholder: label, "aria-label": label });
    i.value = row[key];
    i.addEventListener("input", () => (row[key] = i.value));
    return i;
  };

  const render = () => {
    clear(list);
    rows.forEach((row, idx) => {
      const kind = h(
        "select",
        { class: "input", "aria-label": t("type") },
        h("option", { value: "local" }, t("fwdLocal")),
        h("option", { value: "remote" }, t("fwdRemote")),
        h("option", { value: "dynamic" }, t("fwdDynamic")),
      );
      kind.value = row.kind;
      kind.addEventListener("change", () => {
        row.kind = kind.value as Row["kind"];
        if (row.kind === "remote" && row.bindHost === "127.0.0.1") row.bindHost = "localhost";
        if (row.kind !== "remote" && row.bindHost === "localhost") row.bindHost = "127.0.0.1";
        render();
      });
      const remove = h("button", { type: "button", class: "icon-btn", title: t("removeForward"), "aria-label": t("removeForward") }, icon("close"));
      remove.addEventListener("click", () => {
        rows.splice(idx, 1);
        render();
      });
      const warning = h("p", { class: "warn" }, t("fwdPublicWarning"));
      warning.hidden = !exposed(row);
      const line = h(
        "div",
        { class: "fwd-row" },
        kind,
        textInput(row, "bindHost", t("bindHost"), () => (warning.hidden = !exposed(row))),
        portInput(row, "bindPort", t("bindPort")),
        h("span", { class: "fwd-arrow", "aria-hidden": "true" }, row.kind === "dynamic" ? "SOCKS5" : "→"),
      );
      if (row.kind !== "dynamic") {
        line.append(textInput(row, "destHost", t("destHost")), portInput(row, "destPort", t("destPort")));
      } else {
        line.append(h("span"), h("span"));
      }
      line.append(remove);
      list.append(line);
      list.append(h("p", { class: "muted fwd-help" }, t(row.kind === "local" ? "fwdLocalHelp" : row.kind === "remote" ? "fwdRemoteHelp" : "fwdDynamicHelp")));
      list.append(warning);
    });
    const add = h("button", { type: "button", class: "btn btn-ghost" }, icon("plus"), h("span", {}, t("addForward")));
    add.addEventListener("click", () => {
      rows.push({ kind: "local", bindHost: "127.0.0.1", bindPort: "", destHost: "localhost", destPort: "" });
      render();
      list.querySelectorAll<HTMLInputElement>(".fwd-row:last-of-type input")[1]?.focus();
    });
    list.append(add);
  };
  render();

  const value = (): Forward[] =>
    rows.map((r): Forward => {
      const bindHost = r.bindHost.trim() || (r.kind === "remote" ? "localhost" : "127.0.0.1");
      const bindPort = Number(r.bindPort) || 0;
      if (r.kind === "dynamic") return { kind: "dynamic", bindHost, bindPort };
      return { kind: r.kind, bindHost, bindPort, destHost: r.destHost.trim(), destPort: Number(r.destPort) || 0 };
    });

  return { el, value };
}
