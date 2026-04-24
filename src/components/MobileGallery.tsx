import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MobileResource, CategoryGroup } from "../types";
import { MobilePrivacyDashboard } from "./MobilePrivacyDashboard";

// ── Helpers ───────────────────────────────────────────────────────────────────

function relativeTime(capturedAt: number): string {
  const nowSec = Date.now() / 1000;
  const diff = nowSec - capturedAt;

  if (diff < 60) return "hace un momento";
  if (diff < 3600) return `hace ${Math.floor(diff / 60)} min`;
  if (diff < 86400) return `hace ${Math.floor(diff / 3600)}h`;
  if (diff < 604800) return `hace ${Math.floor(diff / 86400)} días`;

  const d = new Date(capturedAt * 1000);
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const yyyy = d.getFullYear();
  return `${dd}/${mm}/${yyyy}`;
}

function faviconUrl(domain: string): string {
  return `https://www.google.com/s2/favicons?domain=${domain}&sz=32`;
}

// ── Sub-components ────────────────────────────────────────────────────────────

interface ResourceRowProps {
  resource: MobileResource;
  onOpen: (uuid: string) => void;
}

function ResourceRow({ resource, onOpen }: ResourceRowProps) {
  return (
    <button
      className="mobile-gallery__resource-row"
      onClick={() => onOpen(resource.uuid)}
      type="button"
    >
      <img
        className="mobile-gallery__favicon"
        src={faviconUrl(resource.domain)}
        alt=""
        width={32}
        height={32}
        loading="lazy"
      />
      <span className="mobile-gallery__resource-info">
        <span className="mobile-gallery__resource-domain">{resource.domain}</span>
        <span className="mobile-gallery__resource-title">{resource.title}</span>
      </span>
      <span className="mobile-gallery__resource-time">
        {relativeTime(resource.captured_at)}
      </span>
    </button>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

type GalleryView = "main" | "category-detail";

export function MobileGallery() {
  const [groups, setGroups] = useState<CategoryGroup[]>([]);
  const [loading, setLoading] = useState(true);
  const [view, setView] = useState<GalleryView>("main");
  const [activeCategory, setActiveCategory] = useState<string | null>(null);
  const [showPrivacy, setShowPrivacy] = useState(false);

  useEffect(() => {
    loadResources();
  }, []);

  async function loadResources() {
    setLoading(true);
    try {
      const data = await invoke<CategoryGroup[]>("get_mobile_resources");
      setGroups(data);
    } finally {
      setLoading(false);
    }
  }

  function openResource(uuid: string) {
    // D1: URL never reaches the frontend — only uuid is passed to the backend
    invoke("open_resource_url", { uuid }).catch(() => undefined);
  }

  function openCategoryDetail(category: string) {
    setActiveCategory(category);
    setView("category-detail");
  }

  function goBack() {
    setView("main");
    setActiveCategory(null);
  }

  function handleDataCleared() {
    setGroups([]);
    loadResources();
  }

  // ── Derived data ────────────────────────────────────────────────────────────

  // All resources flat-sorted by captured_at DESC
  const allResources: MobileResource[] = groups
    .flatMap((g) => g.resources)
    .sort((a, b) => b.captured_at - a.captured_at);

  const recents = allResources.slice(0, 10);

  // Categories sorted by resource count DESC
  const categoriesSorted = [...groups].sort(
    (a, b) => b.resources.length - a.resources.length
  );

  const hasResources = allResources.length > 0;

  // ── Category detail view ────────────────────────────────────────────────────

  if (view === "category-detail" && activeCategory !== null) {
    const group = groups.find((g) => g.category === activeCategory);
    const detailResources = group
      ? [...group.resources].sort((a, b) => b.captured_at - a.captured_at)
      : [];

    return (
      <div className="mobile-gallery">
        <header className="mobile-gallery__header">
          <button
            className="mobile-gallery__back-btn"
            onClick={goBack}
            type="button"
          >
            ← {activeCategory} ({detailResources.length})
          </button>
          <div className="mobile-gallery__header-actions">
            <button
              className="mobile-gallery__privacy-btn"
              onClick={() => setShowPrivacy(true)}
              type="button"
              aria-label="Privacidad"
            >
              🔒
            </button>
            <button
              className="mobile-gallery__refresh-btn"
              onClick={loadResources}
              type="button"
              aria-label="Actualizar"
            >
              ⟳
            </button>
          </div>
        </header>

        <div className="mobile-gallery__content">
          {detailResources.length === 0 ? (
            <p className="mobile-gallery__empty">
              No hay recursos en esta categoría.
            </p>
          ) : (
            detailResources.map((r) => (
              <ResourceRow key={r.uuid} resource={r} onOpen={openResource} />
            ))
          )}
        </div>

        {showPrivacy && (
          <MobilePrivacyDashboard
            onClose={() => setShowPrivacy(false)}
            onDataCleared={handleDataCleared}
          />
        )}
      </div>
    );
  }

  // ── Main view ───────────────────────────────────────────────────────────────

  return (
    <div className="mobile-gallery">
      <header className="mobile-gallery__header">
        <span className="mobile-gallery__header-title">FlowWeaver</span>
        <div className="mobile-gallery__header-actions">
          <button
            className="mobile-gallery__privacy-btn"
            onClick={() => setShowPrivacy(true)}
            type="button"
            aria-label="Privacidad"
          >
            🔒
          </button>
          <button
            className="mobile-gallery__refresh-btn"
            onClick={loadResources}
            type="button"
            aria-label="Actualizar"
          >
            ⟳
          </button>
        </div>
      </header>

      {loading && (
        <p className="mobile-gallery__loading">Cargando…</p>
      )}

      {!loading && !hasResources && (
        <p className="mobile-gallery__empty">
          Comparte algo desde Instagram, YouTube o cualquier app para empezar.
        </p>
      )}

      {!loading && hasResources && (
        <div className="mobile-gallery__content">
          {/* Recientes */}
          <p className="mobile-gallery__section-title">Recientes</p>
          {recents.map((r) => (
            <ResourceRow key={r.uuid} resource={r} onOpen={openResource} />
          ))}

          {/* Por categoría */}
          <p className="mobile-gallery__section-title">Por categoría</p>
          {categoriesSorted.map((g) => (
            <button
              key={g.category}
              className="mobile-gallery__category-row"
              onClick={() => openCategoryDetail(g.category)}
              type="button"
            >
              <span className="mobile-gallery__category-name">{g.category}</span>
              <span className="mobile-gallery__category-count">
                {g.resources.length}
              </span>
              <span className="mobile-gallery__category-chevron">→</span>
            </button>
          ))}
        </div>
      )}

      {showPrivacy && (
        <MobilePrivacyDashboard
          onClose={() => setShowPrivacy(false)}
          onDataCleared={handleDataCleared}
        />
      )}
    </div>
  );
}
