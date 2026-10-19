import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { describeEngineError, generateOrganizationPlan } from "./lib/organizer";
import type { Category, ConflictStatus, OrganizationPlan, PlanEntry, ScanIssue, ScanWarning } from "./types/organizer";
import "./App.css";

const CATEGORY_ORDER: Category[] = ["documents", "images", "videos", "audio", "archives", "code", "spreadsheets", "presentations", "other"];
const CATEGORY_LABELS: Record<Category, string> = {
  documents: "Documents", images: "Images", videos: "Videos", audio: "Audio", archives: "Archives",
  code: "Code", spreadsheets: "Spreadsheets", presentations: "Presentations", other: "Other",
};
const STALE_AFTER_MS = 60_000;
const PAGE_SIZE = 100;

type Snapshot = { plan: OrganizationPlan; generatedAt: number; folder: string };
type StatusFilter = "all" | "ready" | "conflict" | "unchanged";
type SortKey = "name" | "source" | "category" | "destination" | "status";

function FolderGlyph() {
  return <svg aria-hidden="true" viewBox="0 0 48 48" className="folder-glyph" fill="none"><path d="M6 13.5a3.5 3.5 0 0 1 3.5-3.5h10l4 4h15A3.5 3.5 0 0 1 42 17.5v17a3.5 3.5 0 0 1-3.5 3.5h-29A3.5 3.5 0 0 1 6 34.5v-21Z"/><path d="M6.5 19h35"/></svg>;
}

function labelStatus(status: ConflictStatus): string {
  switch (status) {
    case "none": return "Ready";
    case "already_correct": return "Unchanged";
    case "unverifiable_destination": return "Error · destination unchecked";
    case "existing_destination": return "Conflict · destination exists";
    case "proposed_collision": return "Conflict · duplicate proposal";
    case "existing_and_proposed_collision": return "Conflict · multiple collisions";
  }
}

function statusKind(status: ConflictStatus): "ready" | "conflict" | "unchanged" | "error" {
  if (status === "none") return "ready";
  if (status === "already_correct") return "unchanged";
  if (status === "unverifiable_destination") return "error";
  return "conflict";
}

function StatusBadge({ status }: { status: ConflictStatus }) {
  const kind = statusKind(status);
  const symbol = kind === "ready" ? "✓" : kind === "unchanged" ? "＝" : kind === "error" ? "!" : "×";
  return <span className={`status-badge status-${kind}`}><span aria-hidden="true">{symbol}</span>{labelStatus(status)}</span>;
}

function DiagnosticList({ items, kind }: { items: Array<ScanWarning | ScanIssue>; kind: "warning" | "error" }) {
  if (!items.length) return null;
  const title = kind === "warning" ? "Warnings" : "Issues";
  return <details className={`diagnostics diagnostics-${kind}`}>
    <summary><span className="diagnostic-symbol" aria-hidden="true">{kind === "warning" ? "!" : "×"}</span>{title}<span className="diagnostic-count">{items.length}</span></summary>
    <ul>{items.map((item, index) => <li key={`${item.kind}-${item.path ?? "root"}-${index}`}>
      <strong>{item.path ?? "Folder"}</strong><span>{item.message}</span>
      <small>{item.kind.replace(/_/g, " ")}</small>
    </li>)}</ul>
  </details>;
}

function App() {
  const [folder, setFolder] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isChoosing, setIsChoosing] = useState(false);
  const [isScanning, setIsScanning] = useState(false);
  const [search, setSearch] = useState("");
  const [categoryFilter, setCategoryFilter] = useState<Category | "all">("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [sortAscending, setSortAscending] = useState(true);
  const [page, setPage] = useState(0);
  const [now, setNow] = useState(Date.now());
  const requestInFlight = useRef(false);
  const chooserInFlight = useRef(false);

  useEffect(() => {
    if (!snapshot) return;
    const timer = window.setInterval(() => setNow(Date.now()), 5000);
    return () => window.clearInterval(timer);
  }, [snapshot]);

  async function chooseFolder() {
    if (requestInFlight.current || chooserInFlight.current) return;
    chooserInFlight.current = true;
    setError(null);
    setIsChoosing(true);
    try {
      const selection = await open({ directory: true, multiple: false, title: "Choose a folder" });
      if (typeof selection === "string" && selection !== folder) {
        setFolder(selection);
        setSnapshot(null);
        setSearch(""); setCategoryFilter("all"); setStatusFilter("all"); setPage(0);
      }
    } catch (cause) {
      setError(describeEngineError(cause));
    } finally {
      chooserInFlight.current = false;
      setIsChoosing(false);
    }
  }

  async function refreshPlan() {
    if (!folder || requestInFlight.current) return;
    requestInFlight.current = true;
    const requestedFolder = folder;
    setError(null);
    setIsScanning(true);
    try {
      const result = await generateOrganizationPlan(requestedFolder);
      if (requestedFolder === folder) setSnapshot({ plan: result, generatedAt: Date.now(), folder: requestedFolder });
    } catch (cause) {
      setError(describeEngineError(cause));
    } finally {
      requestInFlight.current = false;
      setIsScanning(false);
    }
  }

  const plan = snapshot?.plan;
  const stale = snapshot !== null && now - snapshot.generatedAt >= STALE_AFTER_MS;
  const counts = useMemo(() => {
    const value: Record<Category, number> = { documents: 0, images: 0, videos: 0, audio: 0, archives: 0, code: 0, spreadsheets: 0, presentations: 0, other: 0 };
    for (const entry of plan?.entries ?? []) value[entry.category] += 1;
    return value;
  }, [plan]);
  const conflictCount = plan?.entries.filter((entry) => statusKind(entry.conflict_status) === "conflict").length ?? 0;
  const unchangedCount = plan?.entries.filter((entry) => entry.conflict_status === "already_correct").length ?? 0;
  const warningCount = plan ? plan.warnings.length + plan.issues.length : 0;
  const filteredEntries = useMemo(() => {
    if (!plan) return [];
    const query = search.trim().toLocaleLowerCase();
    return plan.entries.filter((entry) => {
      const matchesCategory = categoryFilter === "all" || entry.category === categoryFilter;
      const kind = statusKind(entry.conflict_status);
      const matchesStatus = statusFilter === "all" || kind === statusFilter;
      const matchesSearch = !query || [entry.file_name, entry.source, entry.destination, CATEGORY_LABELS[entry.category], entry.reason ?? ""]
        .some((value) => value.toLocaleLowerCase().includes(query));
      return matchesCategory && matchesStatus && matchesSearch;
    }).sort((left, right) => {
      const leftValue = sortKey === "name" ? left.file_name : sortKey === "source" ? left.source : sortKey === "category" ? CATEGORY_LABELS[left.category] : sortKey === "destination" ? left.destination : labelStatus(left.conflict_status);
      const rightValue = sortKey === "name" ? right.file_name : sortKey === "source" ? right.source : sortKey === "category" ? CATEGORY_LABELS[right.category] : sortKey === "destination" ? right.destination : labelStatus(right.conflict_status);
      return leftValue.localeCompare(rightValue, undefined, { numeric: true, sensitivity: "base" }) * (sortAscending ? 1 : -1);
    });
  }, [plan, search, categoryFilter, statusFilter, sortKey, sortAscending]);
  const pageCount = Math.ceil(filteredEntries.length / PAGE_SIZE);
  const visibleEntries = filteredEntries.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  function chooseSort(key: SortKey) {
    if (sortKey === key) setSortAscending((ascending) => !ascending);
    else { setSortKey(key); setSortAscending(true); }
    setPage(0);
  }

  const sortButton = (key: SortKey, label: string) => <button type="button" className="sort-button" onClick={() => chooseSort(key)} aria-label={`Sort by ${label}`}>
    {label}<span aria-hidden="true">{sortKey === key ? sortAscending ? " ↑" : " ↓" : " ↕"}</span>
  </button>;

  return <main className="app-shell">
    <header className="topbar">
      <div className="brand"><span className="brand-mark" aria-hidden="true">s</span><span>shellsort</span></div>
      <span className="phase-badge">READ-ONLY PREVIEW <span aria-hidden="true">·</span> PHASE 3</span>
    </header>

    <section className="hero" aria-labelledby="page-title">
      <div className="eyebrow"><span className="eyebrow-dot" /> A calmer way to sort things out</div>
      <h1 id="page-title">A little more <span>order.</span></h1>
      <p className="intro">Review a proposed home for each file before making any decisions. Shellsort does not change your files.</p>
    </section>

    <section className="workspace" aria-label="Folder and organization preview">
      <div className="workspace-heading"><div><p className="section-kicker">YOUR WORKSPACE</p><h2>Folder preview</h2></div><span className="step-count">01 <span>/ 01</span></span></div>
      <div className="folder-card">
        <div className="folder-art"><FolderGlyph /></div>
        <div className="folder-copy"><h3>{folder ? "Selected folder" : "No folder selected"}</h3>
          {folder ? <details className="folder-path-details"><summary>Inspect full folder path</summary><code className="folder-path">{folder}</code></details> : <p>Choose a folder on your Mac to create a read-only preview.</p>}
        </div>
        <button className="choose-button" type="button" onClick={chooseFolder} disabled={isChoosing || isScanning}>
          <span>{isChoosing ? "Opening…" : folder ? "Change folder" : "Choose folder"}</span><span aria-hidden="true">⌘O</span>
        </button>
      </div>

      {folder && <div className="action-row">
        <button className="scan-button" type="button" onClick={refreshPlan} disabled={isScanning || isChoosing}>
          <span aria-hidden="true">{isScanning ? "◌" : "↻"}</span>{isScanning ? snapshot ? "Refreshing preview…" : "Scanning folder…" : snapshot ? "Rescan folder" : "Scan and preview"}
        </button>
        <span className="action-hint">Names and basic file metadata only</span>
      </div>}

      {error && <div className="error-message" role="alert"><span aria-hidden="true">!</span><div><strong>{snapshot ? "The refresh could not complete" : "Could not create a preview"}</strong><p>{error}{snapshot && " The last successful preview remains visible and may be out of date."}</p></div></div>}

      <section className="preview-card" aria-labelledby="preview-title" aria-busy={isScanning}>
        <div className="preview-topline"><div><p className="section-kicker">READ-ONLY PREVIEW</p><h3 id="preview-title">Proposed organization</h3></div>
          {snapshot && <span className={`freshness ${stale || error ? "freshness-stale" : ""}`}><span aria-hidden="true">{stale || error ? "!" : "●"}</span>{stale || error ? "May be out of date" : "Current snapshot"}</span>}
        </div>

        {!folder ? <div className="empty-state"><span className="empty-icon" aria-hidden="true">⌕</span><h4>Choose a folder to get started</h4><p>Your preview will show file locations and possible conflicts. Nothing will be moved.</p></div>
          : !plan && isScanning ? <div className="loading-state" role="status"><span className="spinner" aria-hidden="true"/><div><strong>Reading folder metadata</strong><p>This can take a little while for folders with many files.</p></div></div>
          : !plan ? <div className="empty-state"><span className="empty-icon" aria-hidden="true">⌁</span><h4>Ready when you are</h4><p>Select <strong>Scan and preview</strong> to inspect this folder. The selected folder will not be changed.</p></div>
          : <div className="plan-results">
            <div className="summary-grid" aria-label="Preview summary">
              <SummaryCard label="Files in plan" value={plan.entries.length} detail="regular files" />
              <SummaryCard label="Categories" value={CATEGORY_ORDER.filter((category) => counts[category] > 0).length} detail="with files" />
              <SummaryCard label="Conflicts" value={conflictCount} detail="need review" tone={conflictCount ? "warning" : "normal"} />
              <SummaryCard label="Warnings & issues" value={warningCount} detail="scan diagnostics" tone={warningCount ? "warning" : "normal"} />
              <SummaryCard label="Unchanged" value={unchangedCount} detail="already in category" />
            </div>
            {plan.is_incomplete && <div className="incomplete-note" role="status"><span aria-hidden="true">!</span><span>This preview is incomplete. Review the scan warnings and issues before relying on it.</span></div>}

            <section className="category-panel" aria-labelledby="category-title"><div className="subsection-heading"><div><p className="section-kicker">OVERVIEW</p><h4 id="category-title">Categories</h4></div><span>{CATEGORY_ORDER.filter((category) => counts[category] > 0).length} groups</span></div>
              <div className="category-grid">{CATEGORY_ORDER.filter((category) => counts[category] > 0).map((category, index) => <button key={category} className={`category-chip category-${index}`} type="button" onClick={() => { setCategoryFilter(categoryFilter === category ? "all" : category); setPage(0); document.getElementById("filter-category")?.focus(); }} aria-pressed={categoryFilter === category}>
                <span className="category-mark" aria-hidden="true">{CATEGORY_LABELS[category].slice(0, 1)}</span><span>{CATEGORY_LABELS[category]}</span><strong>{counts[category]}</strong></button>)}</div>
            </section>

            <div className="table-heading"><div><p className="section-kicker">FILE-BY-FILE</p><h4>Proposed destinations</h4></div><span>{filteredEntries.length.toLocaleString()} {filteredEntries.length === 1 ? "result" : "results"}</span></div>
            <div className="filters" role="search" aria-label="Filter preview entries">
              <label className="search-control"><span aria-hidden="true">⌕</span><span className="sr-only">Search files and paths</span><input value={search} onChange={(event) => { setSearch(event.target.value); setPage(0); }} placeholder="Search name or path" /></label>
              <label><span className="sr-only">Filter by category</span><select id="filter-category" value={categoryFilter} onChange={(event) => { setCategoryFilter(event.target.value as Category | "all"); setPage(0); }}><option value="all">All categories</option>{CATEGORY_ORDER.map((category) => <option key={category} value={category}>{CATEGORY_LABELS[category]}</option>)}</select></label>
              <label><span className="sr-only">Filter by status</span><select value={statusFilter} onChange={(event) => { setStatusFilter(event.target.value as StatusFilter); setPage(0); }}><option value="all">All statuses</option><option value="ready">Ready</option><option value="conflict">Conflicts</option><option value="unchanged">Unchanged</option></select></label>
            </div>

            {filteredEntries.length ? <div className="table-scroll"><table><caption className="sr-only">Read-only file organization preview</caption><thead><tr><th scope="col">{sortButton("name", "Filename")}</th><th scope="col">{sortButton("source", "Current relative path")}</th><th scope="col">{sortButton("category", "Category")}</th><th scope="col">{sortButton("destination", "Proposed relative destination")}</th><th scope="col">{sortButton("status", "Status")}</th></tr></thead>
              <tbody>{visibleEntries.map((entry) => <PreviewRow key={`${entry.source}:${entry.destination}`} entry={entry} />)}</tbody></table></div>
              : <div className="table-empty"><strong>{plan.entries.length ? "No files match these filters" : "No regular files found"}</strong><p>{plan.entries.length ? "Try a different search or clear the category and status filters." : "There are no preview entries in the scanned folders."}</p></div>}
            {pageCount > 1 && <nav className="pagination" aria-label="Preview result pages"><span>Showing {(page * PAGE_SIZE + 1).toLocaleString()}–{Math.min((page + 1) * PAGE_SIZE, filteredEntries.length).toLocaleString()} of {filteredEntries.length.toLocaleString()}</span><div><button type="button" onClick={() => setPage((value) => Math.max(0, value - 1))} disabled={page === 0}>Previous</button><span aria-current="page">Page {page + 1} of {pageCount}</span><button type="button" onClick={() => setPage((value) => Math.min(pageCount - 1, value + 1))} disabled={page + 1 >= pageCount}>Next</button></div></nav>}

            {(plan.warnings.length > 0 || plan.issues.length > 0) && <section className="diagnostic-panel" aria-labelledby="diagnostic-title"><div className="subsection-heading"><div><p className="section-kicker">SCAN NOTES</p><h4 id="diagnostic-title">What Shellsort could not verify</h4></div><span>{warningCount} notes</span></div><p className="diagnostic-intro">Some items were skipped or could not be checked. These notes do not mean any files were changed.</p><DiagnosticList items={plan.warnings} kind="warning"/><DiagnosticList items={plan.issues} kind="error"/></section>}
          </div>}
      </section>
      <p className="safety-note"><span aria-hidden="true">⌁</span><span><strong>Preview only.</strong> Shellsort reads file names and basic metadata. It does not move, rename, delete, or overwrite files.</span></p>
    </section>
    <footer className="footer"><span>Thoughtful organization, one step at a time.</span><span>MADE FOR MAC <span aria-hidden="true">⌘</span></span></footer>
  </main>;
}

function SummaryCard({ label, value, detail, tone = "normal" }: { label: string; value: number; detail: string; tone?: "normal" | "warning" }) {
  return <div className={`summary-card ${tone === "warning" && value > 0 ? "summary-warning" : ""}`}><span>{label}</span><strong>{value.toLocaleString()}</strong><small>{detail}</small></div>;
}

function PreviewRow({ entry }: { entry: PlanEntry }) {
  return <tr>
    <th scope="row"><code className="filename" title={entry.file_name}>{entry.file_name}</code><details className="row-details"><summary>Details</summary><div><span>Size</span><strong>{formatBytes(entry.size_bytes)}</strong>{entry.extension && <><span>Extension</span><strong>.{entry.extension}</strong></>}{entry.reason && <><span>Why</span><strong>{entry.reason}</strong></>}</div></details></th>
    <td><code className="path-cell" title={entry.source}>{entry.source}</code></td>
    <td><span className="table-category">{CATEGORY_LABELS[entry.category]}</span></td>
    <td><code className="path-cell destination-cell" title={entry.destination}>{entry.destination}</code></td>
    <td><StatusBadge status={entry.conflict_status}/></td>
  </tr>;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024; let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}

export default App;
