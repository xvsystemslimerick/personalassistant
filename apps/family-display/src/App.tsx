import { useEffect, useMemo, useState } from "react";
import type { DisplayItem, DisplayMode, DisplaySnapshot } from "./types";

const modes = new Set<DisplayMode>(["today", "week", "month", "morning", "evening"]);
const kinds = new Set(["task", "appointment", "reminder", "notice"]);
const categories = new Set(["work", "family", "school", "creche", "kids", "personal", "home"]);

function parseSnapshot(value: unknown): DisplaySnapshot | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Record<string, unknown>;
  if (
    candidate.schemaVersion !== 1 ||
    typeof candidate.generatedAt !== "string" ||
    Number.isNaN(Date.parse(candidate.generatedAt)) ||
    typeof candidate.mode !== "string" ||
    !modes.has(candidate.mode as DisplayMode) ||
    !Array.isArray(candidate.items) ||
    candidate.items.length > 100
  ) return null;
  const valid = candidate.items.every((item) => {
    if (!item || typeof item !== "object") return false;
    const row = item as Record<string, unknown>;
    return typeof row.displayId === "string" && row.displayId.length >= 16 && row.displayId.length <= 100 &&
      typeof row.title === "string" && row.title.length > 0 && [...row.title].length <= 200 &&
      typeof row.kind === "string" && kinds.has(row.kind) &&
      typeof row.category === "string" && categories.has(row.category) &&
      (row.detailLevel === "full" || row.detailLevel === "generic");
  });
  return valid ? candidate as unknown as DisplaySnapshot : null;
}

function timeLabel(item: DisplayItem): string {
  if (!item.startsAt) return "";
  const value = new Date(item.startsAt);
  return Number.isNaN(value.getTime()) ? "" : value.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function App() {
  const [snapshot, setSnapshot] = useState<DisplaySnapshot | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "offline">("loading");

  useEffect(() => {
    let controller: AbortController | null = null;
    const refresh = () => {
      controller?.abort();
      controller = new AbortController();
      fetch("/api/v1/snapshot", { cache: "no-store", credentials: "same-origin", signal: controller.signal })
        .then(async (response) => response.ok ? parseSnapshot(await response.json()) : null)
        .then((parsed) => {
          setSnapshot(parsed);
          setState(parsed ? "ready" : "offline");
        })
        .catch((reason: unknown) => { if (!(reason instanceof DOMException && reason.name === "AbortError")) setState("offline"); });
    };
    refresh();
    const interval = window.setInterval(refresh, 60_000);
    return () => { window.clearInterval(interval); controller?.abort(); };
  }, []);

  const date = useMemo(() => {
    const source = snapshot?.generatedAt ? new Date(snapshot.generatedAt) : new Date();
    return source.toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" });
  }, [snapshot?.generatedAt]);
  const clock = useMemo(() => {
    const source = snapshot?.generatedAt ? new Date(snapshot.generatedAt) : new Date();
    return source.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }, [snapshot?.generatedAt]);

  return (
    <main>
      <header><div><p className="eyebrow">Family Display</p><h1>{date}</h1></div><time>{clock}</time></header>
      {state !== "ready" || snapshot === null ? (
        <section className="connection" aria-live="polite">
          <p className="eyebrow">{state === "loading" ? "Connecting" : "Display offline"}</p>
          <h2>{state === "loading" ? "Loading the family view…" : "Waiting for the secure home service"}</h2>
          <p>The last view is not cached on this screen.</p>
        </section>
      ) : (
        <section className="board" aria-label={`${snapshot.mode} family view`}>
          <div className="panel schedule"><p className="eyebrow">{snapshot.mode}</p><h2>Schedule</h2><ItemList items={snapshot.items.filter((item) => item.kind === "appointment")} /></div>
          <div className="panel remember"><p className="eyebrow">Things to remember</p><h2>Reminders</h2><ItemList items={snapshot.items.filter((item) => item.kind !== "appointment")} /></div>
        </section>
      )}
      <footer>{snapshot ? `${snapshot.items.length} display-safe item${snapshot.items.length === 1 ? "" : "s"}` : "Private details stay on the Mac"}</footer>
    </main>
  );
}

function ItemList({ items }: { items: DisplayItem[] }) {
  if (items.length === 0) return <p className="empty">Nothing scheduled.</p>;
  return <ol>{items.map((item) => <li key={item.displayId}><span className={`marker ${item.category}`} /><time>{timeLabel(item)}</time><strong>{item.title}</strong><small>{item.category}</small></li>)}</ol>;
}
