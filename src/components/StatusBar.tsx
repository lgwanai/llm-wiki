import { Folder, FileText, Box, Network } from "lucide-react";

interface Props {
  projectPath: string | null;
  pageCount: number;
  entityCount: number;
  edgeCount: number;
  onOpenProject: () => void;
}

export default function StatusBar({ projectPath, pageCount, entityCount, edgeCount, onOpenProject }: Props) {
  return (
    <div style={{
      height: 28, minHeight: 28,
      background: "var(--color-muted)",
      borderTop: "1px solid var(--color-border)",
      display: "flex", alignItems: "center",
      padding: "0 12px",
      fontFamily: "var(--font-mono)", fontSize: 11,
      color: "var(--color-muted-fg)",
    }}>
      {/* Left */}
      <div style={{ display: "flex", alignItems: "center", gap: 16, flex: 1 }}>
        <button
          onClick={onOpenProject}
          style={{ display: "flex", alignItems: "center", gap: 4, background: "none", border: "none", color: "var(--color-accent)", cursor: "pointer", fontFamily: "inherit", fontSize: 11 }}
        >
          <Folder size={12} />
          {projectPath || "No project"}
        </button>
      </div>

      {/* Right */}
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
          <FileText size={12} /> {pageCount} pages
        </span>
        <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
          <Box size={12} /> {entityCount} entities
        </span>
        <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
          <Network size={12} /> {edgeCount} edges
        </span>
      </div>
    </div>
  );
}
