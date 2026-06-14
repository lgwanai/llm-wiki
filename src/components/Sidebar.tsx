import { useState, useMemo } from "react";
import { Search, FolderOpen, FileText, Box } from "lucide-react";

interface PageInfo { id: string; name: string; type: string; }

interface Props {
  pages: PageInfo[];
  projectPath: string | null;
  onSelectPage: (pageId: string) => void;
  onOpenProject: () => void;
}

export default function Sidebar({ pages, projectPath, onSelectPage, onOpenProject }: Props) {
  const [search, setSearch] = useState("");
  const filtered = useMemo(() => {
    const q = search.toLowerCase().trim();
    return q ? pages.filter(p => p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q)) : pages;
  }, [pages, search]);
  const concepts = filtered.filter(p => p.type === "concept");
  const entities = filtered.filter(p => p.type !== "concept");

  return (
    <div style={{ height: "100%", display: "flex", flexDirection: "column" }}>
      <div style={{ padding: "8px 12px" }}>
        <div style={{ position: "relative" }}>
          <Search size={14} style={{ position: "absolute", left: 10, top: 10, color: "var(--color-muted-fg)" }} />
          <input placeholder="Search..." value={search} onChange={e => setSearch(e.target.value)} style={{ width: "100%", paddingLeft: 30 }} />
        </div>
      </div>
      <div style={{ padding: "0 12px 8px" }}>
        <button className="btn" onClick={onOpenProject} style={{ width: "100%", justifyContent: "flex-start" }}>
          <FolderOpen size={14} />
          {projectPath ? projectPath.split("/").pop() : "Open Project"}
        </button>
      </div>
      <div style={{ flex: 1, overflow: "auto" }}>
        <Section icon={<FileText size={12} />} title="Concepts" count={concepts.length}>
          {concepts.map(p => <PageItem key={p.id} page={p} onClick={() => onSelectPage(p.id)} />)}
        </Section>
        <Section icon={<Box size={12} />} title="Entities" count={entities.length}>
          {entities.map(p => <PageItem key={p.id} page={p} onClick={() => onSelectPage(p.id)} />)}
        </Section>
      </div>
    </div>
  );
}

function Section({ icon, title, count, children }: { icon: React.ReactNode; title: string; count: number; children: React.ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div>
      <div onClick={() => setOpen(!open)} style={{ display: "flex", alignItems: "center", gap: 6, padding: "4px 12px", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 11, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--color-muted-fg)" }}>
        <span style={{ transform: open ? "rotate(90deg)" : "none", transition: "transform 0.15s" }}>▸</span>
        {icon} {title} ({count})
      </div>
      {open && <div>{children}</div>}
    </div>
  );
}

function PageItem({ page, onClick }: { page: PageInfo; onClick: () => void }) {
  return (
    <div onClick={onClick} style={{ padding: "4px 12px 4px 28px", cursor: "pointer", fontSize: 13, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}
      onMouseEnter={e => { (e.target as HTMLElement).style.background = "var(--color-muted)"; }}
      onMouseLeave={e => { (e.target as HTMLElement).style.background = "transparent"; }}>
      {page.name}
    </div>
  );
}
