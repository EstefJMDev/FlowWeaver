import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PatternSummary, TemporalWindowView } from "../types";

export function PatternsSection() {
  const [patterns, setPatterns] = useState<PatternSummary[] | null>(null);
  const [pendingId, setPendingId] = useState<string | null>(null);

  useEffect(() => { refresh(); }, []);

  async function refresh() {
    try {
      const list = await invoke<PatternSummary[]>("get_detected_patterns");
      setPatterns(list);
    } catch {
      setPatterns([]);
    }
  }

  async function toggle(p: PatternSummary) {
    setPendingId(p.pattern_id);
    try {
      if (p.is_blocked) {
        await invoke("unblock_pattern", { patternId: p.pattern_id });
      } else {
        await invoke("block_pattern", { patternId: p.pattern_id });
      }
      await refresh();
    } finally {
      setPendingId(null);
    }
  }

  if (patterns === null) {
    return (
      <section aria-labelledby="pd-patrones">
        <h4 id="pd-patrones">Patrones detectados</h4>
        <p>Cargando…</p>
      </section>
    );
  }
  if (patterns.length === 0) {
    return (
      <section aria-labelledby="pd-patrones">
        <h4 id="pd-patrones">Patrones detectados</h4>
        <p>Aún no se han detectado patrones recurrentes.</p>
      </section>
    );
  }

  return (
    <section aria-labelledby="pd-patrones">
      <h4 id="pd-patrones">Patrones detectados</h4>
      <ul>
        {patterns.map((p) => (
          <li key={p.pattern_id} className={p.is_blocked ? "is-blocked" : undefined}>
            <div className="pattern__label">{p.label}</div>
            <div className="pattern__signatures">
              {p.category_signature.slice(0, 5).map((c) => (
                <span key={c.category} className="pattern__badge">
                  {c.category} {Math.round(c.weight * 100)}%
                </span>
              ))}
              {p.category_signature.length > 5 && (
                <span className="pattern__badge pattern__badge--more">
                  +{p.category_signature.length - 5} más
                </span>
              )}
            </div>
            <div className="pattern__meta">
              {formatTemporalWindow(p.temporal_window)} · {p.frequency} veces ·
              {" "}última hace {formatRelative(p.last_seen)}
            </div>
            <button
              onClick={() => toggle(p)}
              disabled={pendingId === p.pattern_id}
              aria-label={p.is_blocked ? "Desbloquear patrón" : "Bloquear patrón"}
            >
              {p.is_blocked ? "Desbloquear" : "Bloquear"}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function formatTemporalWindow(tw: TemporalWindowView): string {
  const bucketLabel: Record<string, string> = {
    Morning: "Mañana",
    Afternoon: "Tarde",
    Evening: "Noche",
  };
  const label = bucketLabel[tw.time_bucket] ?? tw.time_bucket;
  const days = ["L", "M", "X", "J", "V", "S", "D"];
  const active = days.filter((_, i) => (tw.day_of_week_mask & (1 << i)) !== 0).join(",");
  return active ? `${label} — ${active}` : label;
}

function formatRelative(unixSec: number): string {
  const diffSec = Math.max(0, Math.floor(Date.now() / 1000) - unixSec);
  if (diffSec < 3600) return "menos de 1 h";
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)} h`;
  return `${Math.floor(diffSec / 86400)} días`;
}
