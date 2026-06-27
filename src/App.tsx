import React, { useState, useCallback, useEffect, useRef } from "react";
import MDEditor from "@uiw/react-md-editor";
import { Folder, FileText, Box, Network, Database, Search, Settings, Play, Circle, CheckCircle, XCircle, Loader, Send, ChevronRight } from "lucide-react";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import remarkGfm from "remark-gfm";
import "@uiw/react-md-editor/markdown-editor.css";
import "@uiw/react-markdown-preview/markdown.css";

// Types
interface PageInfo { id: string; name: string; type: string; path: string; }
interface Status { pages: { concepts: number; entities: number; total: number }; graph: { entities: number; edges: number }; files: { index: boolean; log: boolean; audit: boolean }; }
interface GraphNode { id: string; name: string; type: string; confidence: number; }
interface GraphEdge { source: string; target: string; type: string; description?: string; }
interface SourceFile { path: string; name: string; status: "pending" | "compiling" | "done" | "error"; pages: number; error?: string; }
interface TableInfo { table: string; display_name: string; description: string; record_count: number; }

let _invoke: any = null;
async function invoke(cmd: string, args?: any) { if (!_invoke) { const m = await import("@tauri-apps/api/core"); _invoke = m.invoke; } return _invoke(cmd, args); }

// ═══════════════════════════════════════════════════
export default function App() {
  const [wsPath, setWsPath] = useState<string | null>(null);
  const [wsName, setWsName] = useState("");
  const [status, setStatus] = useState<Status>({ pages: { concepts: 0, entities: 0, total: 0 }, graph: { entities: 0, edges: 0 }, files: { index: false, log: false, audit: false } });
  const [pages, setPages] = useState<PageInfo[]>([]);
  const [graphNodes, setGraphNodes] = useState<GraphNode[]>([]);
  const [graphEdges, setGraphEdges] = useState<GraphEdge[]>([]);
  const [sourceFiles, setSourceFiles] = useState<SourceFile[]>([]);
  const [tables, setTables] = useState<TableInfo[]>([]);
  const [selectedPage, setSelectedPage] = useState<string | null>(null);
  const [pageContent, setPageContent] = useState("");
  const [selectedTable, setSelectedTable] = useState<string | null>(null);
  const [tableContent, setTableContent] = useState("");
  const [tableError, setTableError] = useState<string | null>(null);
  const [section, setSection] = useState<"files" | "graph" | "tables">("files");
  const [showConfig, setShowConfig] = useState(false);
  const [chatMessages, setChatMessages] = useState<{ role: "user" | "assistant"; content: string }[]>([]);
  const [editMode, setEditMode] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [selectedSource, setSelectedSource] = useState<{ path: string; name: string } | null>(null);
  const [sourceEditContent, setSourceEditContent] = useState("");
  const [compileConfirm, setCompileConfirm] = useState<{ path: string; name: string } | null>(null);

  const openWorkspace = useCallback(async () => {
    try {
      const dialog = await import("@tauri-apps/plugin-dialog");
      const dir = await dialog.open({ directory: true, multiple: false, title: "Open Workspace" });
      if (dir) {
        const path = dir as string;
        setWsPath(path); setWsName(path.split("/").pop() || path);
        try { await invoke("set_project_path", { path }); } catch {}
        await refreshAll();
        await listFiles(path);
        await listTables();
      }
    } catch (e) { console.error(e); }
  }, []);

  const refreshAll = useCallback(async () => {
    try {
      const s = await invoke("get_wiki_status") as Status; setStatus(s);
      const p = await invoke("get_wiki_pages") as PageInfo[]; setPages(p);
      const g = await invoke("get_graph_data") as { nodes: GraphNode[]; edges: GraphEdge[] };
      setGraphNodes(g.nodes); setGraphEdges(g.edges);
    } catch {}
  }, []);

  const listFiles = useCallback(async (root: string) => {
    try { setSourceFiles(await invoke("list_source_files", { root }) as SourceFile[]); } catch {}
  }, []);

  const listTables = useCallback(async () => {
    try { setTables(await invoke("list_ledger_tables") as TableInfo[]); } catch { setTables([]); }
  }, []);

  const importFiles = useCallback(async () => {
    try {
      const dialog = await import("@tauri-apps/plugin-dialog");
      const files = await dialog.open({ multiple: true, title: "Import Files", filters: [{ name: "Documents", extensions: ["md","markdown","mdown","txt","pdf","png","jpg","jpeg","py","rs","js","ts","json","csv","tsv","xlsx","xls","yaml"] }] });
      if (files) {
        const fileList = Array.isArray(files) ? files : [files];
        for (const src of fileList) {
          const name = (src as string).split("/").pop() || "file";
          try { await invoke("import_file", { source: src, destDir: wsPath }); } catch (e) { console.error("Import failed:", e); }
        }
        listFiles(wsPath!);
      }
    } catch (e) { console.error(e); }
  }, [wsPath, listFiles]);

  const compileFile = useCallback(async (filePath: string) => {
    setSourceFiles(prev => prev.map(f => f.path === filePath ? { ...f, status: "compiling" as const } : f));
    try {
      const result = await invoke("compile_source_file", { path: filePath }) as any;
      const errors = result.errors || [];
      const hasErr = errors.length > 0 || result.pages_created === 0;
      setSourceFiles(prev => prev.map(f => f.path === filePath ? { ...f, status: hasErr ? "error" as const : "done" as const, pages: result.pages_created || 0, error: errors[0] || (result.pages_created === 0 ? "No pages extracted" : undefined) } : f));
    } catch (e: any) { setSourceFiles(prev => prev.map(f => f.path === filePath ? { ...f, status: "error" as const, error: String(e) } : f)); }
    refreshAll();
    await listTables();
  }, [refreshAll, listTables]);

  const compileAll = useCallback(async () => {
    for (const f of sourceFiles) {
      if (f.status !== "done") await compileFile(f.path);
    }
  }, [sourceFiles, compileFile]);

  const selectPage = useCallback(async (pageId: string) => {
    setSection("files"); setSelectedSource(null); setSelectedTable(null); setTableError(null); setSelectedPage(pageId); setEditMode(false);
    try { setPageContent(await invoke("get_page_content", { pageId }) as string); }
    catch { setPageContent(`# ${pageId}\n\nNot found.`); }
  }, []);

  const selectTable = useCallback(async (tableName: string) => {
    setSection("tables"); setSelectedSource(null); setSelectedPage(null); setSelectedTable(tableName); setTableError(null); setTableContent("");
    try { setTableContent(await invoke("get_table_content", { table: tableName }) as string); }
    catch (e) { setTableError(String(e)); setTableContent("[]"); }
    setEditMode(false);
  }, []);

  const openSourceMarkdown = useCallback(async (file: SourceFile) => {
    setSection("files"); setSelectedPage(null); setSelectedTable(null); setTableError(null); setEditMode(false); setSelectedSource({ path: file.path, name: file.name });
    try { setSourceEditContent(await invoke("get_source_file_content", { path: file.path }) as string); }
    catch (e) { setSourceEditContent(`# ${file.name}\n\n${String(e)}`); }
  }, []);

  const saveSourceMarkdown = useCallback(async () => {
    if (!selectedSource) return;
    try {
      await invoke("save_source_file_content", { path: selectedSource.path, content: sourceEditContent });
      if (wsPath) await listFiles(wsPath);
    } catch (e) { console.error(e); }
  }, [selectedSource, sourceEditContent, wsPath, listFiles]);

  const saveEdit = useCallback(async () => {
    if (section === "tables" && selectedTable) {
      setTableContent(editContent);
      setEditMode(false);
      return;
    }
    if (!selectedPage) return;
    try { await invoke("save_page_content", { pageId: selectedPage, content: editContent }); setPageContent(editContent); setEditMode(false); refreshAll(); }
    catch (e) { console.error(e); }
  }, [section, selectedTable, selectedPage, editContent, refreshAll]);

  const startEdit = useCallback(() => {
    setEditContent(section === "tables" ? tableContent : pageContent);
    setEditMode(true);
  }, [section, tableContent, pageContent]);

  const sendChat = useCallback(async (question: string) => {
    setChatMessages(prev => {
      const msgIdx = prev.length + 1; // Compute after state update to avoid stale index
      const timerInterval = setInterval(() => {
        const elapsed = (Date.now() - startTime) / 1000;
        setChatMessages(p => p.map((m, i) => i === msgIdx ? { ...m, elapsed } : m));
      }, 100);
      const startTime = Date.now();

      // Setup cleanup wrapper
      let cleanup = () => { clearInterval(timerInterval); };
      const safeCleanup = (fn?: () => void) => { cleanup(); if (fn) fn(); };

      // Kick off async chain
      (async () => {
        try {
          const { listen } = await import("@tauri-apps/api/event");
          const unlisten = await listen("chat-phase", (event: any) => {
            const { phase } = event.payload;
            setChatMessages(p => p.map((m, i) => i === msgIdx ? { ...m, phase, elapsed: (Date.now() - startTime) / 1000 } : m));
          });
          cleanup = () => { clearInterval(timerInterval); unlisten(); };

          const result = await invoke("chat_query", { question }) as { answer: string; sources: any[]; searchTime: number; genTime: number };
          safeCleanup();
          setChatMessages(p => p.map((m, i) => i === msgIdx ? { ...m, content: result.answer, citations: result.sources, status: "done", phase: "done", searchTime: result.searchTime, genTime: result.genTime } : m));
        } catch (e: any) {
          safeCleanup();
          setChatMessages(p => p.map((m, i) => i === msgIdx ? { ...m, content: `Error: ${e}`, status: "error" } : m));
        }
      })();

      return [...prev, { role: "user", content: question }, { role: "assistant", content: "", citations: [], status: "searching", phase: "searching" }];
    });
  }, []);

  const navigateTo = useCallback((source: { id: string; name: string; path: string; pageType: string }) => {
    if (source.pageType === "table") selectTable(source.id);
    else selectPage(source.id);
  }, [selectPage, selectTable]);

  // Menu action handler
  useEffect(() => {
    let unlisten: any;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen("menu-action", (event: any) => {
        switch (event.payload) {
          case "open_workspace": openWorkspace(); break;
          case "import_files": importFiles(); break;
          case "compile_all": compileAll(); break;
          case "settings": setShowConfig(true); break;
          case "toggle_files": setSection("files"); break;
          case "toggle_graph": setSection("graph"); break;
          case "toggle_tables": setSection("tables"); break;
          case "reload": window.location.reload(); break;
          case "lint": invoke("run_lint").then((r: any) => alert(r)).catch(console.error); break;
        }
      });
    })();
    return () => { if (unlisten) unlisten(); };
  }, [openWorkspace, importFiles, compileAll]);

  useEffect(() => { refreshAll(); }, []);

  const isGraph = section === "graph";
  const isTables = section === "tables";
  const pageName = isTables ? selectedPage : pages.find(p => p.id === selectedPage)?.name;
  const isPageContent = !isTables && !isGraph;

  return (
    <div style={{ height: "100vh", display: "flex", flexDirection: "column", background: "var(--color-bg)", color: "var(--color-fg)", fontFamily: "var(--font-sans)" }}>
      <div style={{ flex: 1, display: "flex", overflow: "hidden" }}>
        {/* === SIDEBAR === */}
        <Sidebar wsPath={wsPath} wsName={wsName} section={section} onSectionChange={setSection}
          sourceFiles={sourceFiles} pages={pages} graphNodes={graphNodes} tables={tables}
          onOpenWorkspace={openWorkspace} onSelectPage={selectPage} onSelectTable={selectTable}
          onCompileAll={compileAll} onCompileFile={(path: string) => { const f = sourceFiles.find(f => f.path === path); if (f) setCompileConfirm({ path, name: f.name }); }}
          onOpenSourceMarkdown={openSourceMarkdown}
          onImportFiles={importFiles} onShowConfig={() => setShowConfig(true)}
          onCompileClick={(path: string, name: string) => setCompileConfirm({ path, name })} />

        {/* === MAIN CONTENT === */}
        <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column" }}>
          {!wsPath ? (
            <Welcome onOpen={openWorkspace} />
          ) : isGraph ? (
            <GraphCanvas nodes={graphNodes} edges={graphEdges} onSelectNode={selectPage} />
          ) : isTables ? (
            <LedgerView tableName={selectedTable} content={tableContent} error={tableError} tables={tables} onSelectTable={selectTable}
              editMode={editMode} editContent={editContent} setEditContent={setEditContent} onStartEdit={startEdit} onSaveEdit={saveEdit} onCancelEdit={() => setEditMode(false)} />
          ) : selectedSource ? (
            <SourceMarkdownEditor source={selectedSource} content={sourceEditContent} onChange={setSourceEditContent} onSave={saveSourceMarkdown} onCompile={() => setCompileConfirm({ path: selectedSource.path, name: selectedSource.name })} />
          ) : selectedPage ? (
            <PageView pageId={selectedPage} pageName={pageName} content={pageContent} onSelectPage={selectPage} onSelectTable={selectTable}
              editMode={editMode} editContent={editContent} setEditContent={setEditContent}
              onStartEdit={startEdit} onSaveEdit={saveEdit} onCancelEdit={() => setEditMode(false)} />
          ) : (
            <EmptyState />
          )}
        </div>

        {/* === CHAT PANEL === */}
        <ChatPanel messages={chatMessages} onSend={sendChat} onNavigate={navigateTo} />
      </div>

      <StatusBar wsName={wsName} pageCount={status.pages.total} entityCount={status.graph.entities} edgeCount={status.graph.edges}
        compileDone={sourceFiles.filter(f => f.status === "done").length} compileTotal={sourceFiles.length} onOpenWorkspace={openWorkspace} />

      {showConfig && <ConfigModal onClose={() => setShowConfig(false)} />}

      {/* Compile confirmation dialog */}
      {compileConfirm && (
        <div style={{ position: "fixed", inset: 0, background: "hsla(220 13% 5% / 0.8)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
          <div style={{ background: "var(--color-surface)", border: "1px solid var(--color-accent-dim)", borderRadius: 12, padding: 28, width: 400, boxShadow: "0 0 40px hsla(160 84% 39% / 0.15)" }}>
            <h3 style={{ fontFamily: "var(--font-mono)", fontSize: 15, color: "var(--color-accent)", marginBottom: 12 }}>Compile File?</h3>
            <p style={{ fontSize: 14, marginBottom: 8, wordBreak: "break-all" }}>{compileConfirm.name}</p>
            <p style={{ fontSize: 12, color: "var(--color-muted-fg)", marginBottom: 20 }}>This will send the file to the LLM for analysis. May take 10-60 seconds depending on file size.</p>
            <div style={{ display: "flex", gap: 10 }}>
              <button className="btn btn-primary" style={{ flex: 1 }} onClick={() => { compileFile(compileConfirm.path); setCompileConfirm(null); }}>Compile</button>
              <button className="btn" onClick={() => setCompileConfirm(null)}>Cancel</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ═══════════════════════════════════════════════ SIDEBAR
function Sidebar({ wsPath, wsName, section, onSectionChange, sourceFiles, pages, graphNodes, tables, onOpenWorkspace, onSelectPage, onSelectTable, onCompileAll, onCompileFile, onOpenSourceMarkdown, onImportFiles, onShowConfig, onCompileClick }: any) {
  const [search, setSearch] = useState("");
  const tabs: { key: string; icon: React.ReactNode; label: string }[] = [
    { key: "files", icon: <FileText size={13} />, label: "Files" },
    { key: "graph", icon: <Network size={13} />, label: "Graph" },
    { key: "tables", icon: <Database size={13} />, label: "Tables" },
  ];

  const filteredFiles = sourceFiles.filter((f: any) => f.name.toLowerCase().includes(search.toLowerCase()));
  const filteredPages = pages.filter((p: any) => p.name.toLowerCase().includes(search.toLowerCase()) || p.id.toLowerCase().includes(search.toLowerCase()));
  const filteredGraph = graphNodes.filter((n: any) => n.name.toLowerCase().includes(search.toLowerCase()));

  return (
    <div style={{ width: 260, minWidth: 260, borderRight: "1px solid var(--color-border)", display: "flex", flexDirection: "column", background: "var(--color-surface)" }}>
      <div style={{ padding: "10px 12px", borderBottom: "1px solid var(--color-border)", cursor: "pointer" }} onClick={!wsPath ? onOpenWorkspace : undefined}>
        <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--color-accent)", textTransform: "uppercase", letterSpacing: "0.05em", display: "flex", alignItems: "center", gap: 6 }}>
          <Folder size={13} /> {wsPath ? wsName : "No workspace"}
        </div>
        {wsPath && <div style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--color-muted-fg)", marginTop: 2, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{wsPath}</div>}
      </div>

      <div style={{ display: "flex", borderBottom: "1px solid var(--color-border)" }}>
        {tabs.map(t => (
          <button key={t.key} onClick={() => onSectionChange(t.key)}
            style={{ flex: 1, padding: "8px 0", border: "none", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 11, textTransform: "uppercase", letterSpacing: "0.05em", background: section === t.key ? "var(--color-muted)" : "transparent", color: section === t.key ? "var(--color-accent)" : "var(--color-muted-fg)", borderBottom: section === t.key ? "2px solid var(--color-accent)" : "2px solid transparent", display: "flex", alignItems: "center", justifyContent: "center", gap: 4 }}>
            {t.icon} {t.label}
          </button>
        ))}
      </div>

      <div style={{ padding: "6px 10px" }}>
        <div style={{ position: "relative", display: "flex", alignItems: "center" }}>
          <Search size={12} style={{ position: "absolute", left: 8, color: "var(--color-muted-fg)" }} />
          <input placeholder="Filter..." value={search} onChange={e => setSearch(e.target.value)} style={{ width: "100%", padding: "5px 8px 5px 24px", fontSize: 12, background: "var(--color-bg)", border: "1px solid var(--color-border)", borderRadius: 4, color: "var(--color-fg)", outline: "none" }} />
        </div>
      </div>

      <div style={{ flex: 1, overflow: "auto" }}>
        {section === "files" && (
          <>
            <div style={{ padding: "6px 10px", display: "flex", gap: 4 }}>
              <button className="btn" onClick={onOpenWorkspace} style={{ flex: 1, fontSize: 11, justifyContent: "center", padding: "5px 8px" }}>
                <Folder size={12} /> {wsPath ? "Change" : "Open"}
              </button>
              {wsPath && <button className="btn" onClick={onImportFiles} style={{ flex: 1, fontSize: 11, justifyContent: "center", padding: "5px 8px" }}>Import</button>}
            </div>
            <SectionLabel icon={<FileText size={11} />} text="Source Files" count={filteredFiles.length} action={filteredFiles.length > 0 ? "Compile All" : undefined} onAction={onCompileAll} />
            {filteredFiles.map((f: any, i: number) => (
              <div key={f.path} style={{ padding: "3px 10px", cursor: "pointer", display: "flex", alignItems: "center", gap: 6, fontSize: 12 }}
                onClick={() => isMarkdownSource(f.name) ? onOpenSourceMarkdown(f) : onCompileClick(f.path, f.name)}
                onMouseEnter={e => { (e.target as HTMLElement).style.background = "var(--color-muted)"; }}
                onMouseLeave={e => { (e.target as HTMLElement).style.background = "transparent"; }}>
                {f.status === "done" ? <CheckCircle size={11} style={{ color: "hsl(160 84% 39%)" }} /> : f.status === "error" ? <span title={f.error || "Compile failed"}><XCircle size={11} style={{ color: "hsl(0 84% 60%)", cursor: "help" }} /></span> : f.status === "compiling" ? <Loader size={11} style={{ color: "hsl(40 84% 60%)" }} /> : <Circle size={11} style={{ color: "var(--color-muted-fg)" }} />}
                <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {f.name}
                  {f.status === "error" && f.error && <span style={{ color: "hsl(0 84% 60%)", fontSize: 10, marginLeft: 6 }}>{f.error.slice(0, 50)}</span>}
                </span>
                {f.pages > 0 && <span style={{ fontSize: 10, opacity: 0.5 }}>{f.pages}p</span>}
              </div>
            ))}
            <SectionLabel icon={<Box size={11} />} text="Wiki Pages" count={filteredPages.length} />
            {filteredPages.map((p: any) => <ListRow key={p.id} label={p.name} onClick={() => onSelectPage(p.id)} />)}
          </>
        )}
        {section === "graph" && (
          <>
            <SectionLabel icon={<Network size={11} />} text="Entities" count={filteredGraph.length} />
            {filteredGraph.map((n: any) => (
              <div key={n.id} onClick={() => onSelectPage(n.id)} style={{ display: "flex", alignItems: "center", gap: 6, padding: "5px 10px", cursor: "pointer", fontSize: 13 }}
                onMouseEnter={e => { (e.target as HTMLElement).style.background = "var(--color-muted)"; }} onMouseLeave={e => { (e.target as HTMLElement).style.background = "transparent"; }}>
                <span style={{ width: 8, height: 8, borderRadius: "50%", background: n.type === "concept" ? "var(--color-accent)" : "hsl(173 80% 50%)", flexShrink: 0 }} />
                {n.name}
                <span style={{ marginLeft: "auto", opacity: 0.4, fontSize: 10 }}>{n.confidence.toFixed(1)}</span>
              </div>
            ))}
          </>
        )}
        {section === "tables" && (
          <>
            <SectionLabel icon={<Database size={11} />} text="Ledger Tables" count={tables.length} />
            {tables.map((t: TableInfo) => <ListRow key={t.table} label={t.display_name || t.table} sub={`${t.record_count} rows`} onClick={() => onSelectTable(t.table)} />)}
          </>
        )}
      </div>

      <div style={{ borderTop: "1px solid var(--color-border)", padding: "6px 10px" }}>
        <button className="btn" onClick={() => invoke("open_settings_window")} style={{ width: "100%", justifyContent: "flex-start", fontSize: 12 }}>
          <Settings size={13} /> Settings
        </button>
      </div>
    </div>
  );
}

function SectionLabel({ icon, text, count, action, onAction }: { icon: React.ReactNode; text: string; count: number; action?: string; onAction?: () => void }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "8px 10px 3px", fontFamily: "var(--font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--color-muted-fg)" }}>
      {icon} {text} ({count})
      {action && <button onClick={onAction} style={{ marginLeft: "auto", background: "var(--color-accent)", color: "var(--color-bg)", border: "none", borderRadius: 3, padding: "1px 6px", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 9, display: "flex", alignItems: "center", gap: 2 }}><Play size={9} /> {action}</button>}
    </div>
  );
}

function ListRow({ label, sub, onClick }: { label: string; sub?: string; onClick: () => void }) {
  return (
    <div onClick={onClick} style={{ padding: "4px 10px 4px 20px", cursor: "pointer", fontSize: 13, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", display: "flex", justifyContent: "space-between", alignItems: "center" }}
      onMouseEnter={e => { (e.target as HTMLElement).style.background = "var(--color-muted)"; }} onMouseLeave={e => { (e.target as HTMLElement).style.background = "transparent"; }}>
      <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{label}</span>
      {sub && <span style={{ opacity: 0.4, fontSize: 11, flexShrink: 0, marginLeft: 8 }}>{sub}</span>}
    </div>
  );
}

function isMarkdownSource(name: string) {
  return /\.(md|markdown|mdown)$/i.test(name);
}

// Tree view for source files
function FileTree({ files, onCompileFile, onOpenPage }: { files: SourceFile[]; onCompileFile: (path: string) => void; onOpenPage: (id: string) => void }) {
  if (files.length === 0) return null;
  // Build tree from flat file list
  const roots: { name: string; children: SourceFile[]; dirs: Map<string, any> } = { name: "", children: [], dirs: new Map() };
  files.forEach((f, idx) => {
    // Store original index on the file object
    const parts = f.name.split("/");
    if (parts.length === 1) {
      roots.children.push(f);
    } else {
      let node = roots;
      for (let i = 0; i < parts.length - 1; i++) {
        if (!node.dirs.has(parts[i])) {
          const dir = { name: parts[i], children: [] as SourceFile[], dirs: new Map() as Map<string, any> };
          node.dirs.set(parts[i], dir);
          node.children.push(dir as any);
        }
        node = node.dirs.get(parts[i])!;
      }
      node.children.push({ ...f, name: parts[parts.length - 1] });
    }
  });

  return <TreeNode node={roots} onCompileFile={onCompileFile} onOpenPage={onOpenPage} depth={0} />;
}

function TreeNode({ node, onCompileFile, onOpenPage, depth }: { node: any; onCompileFile: (path: string) => void; onOpenPage: (id: string) => void; depth: number }) {
  const [open, setOpen] = useState(depth < 2);
  const isDir = node.dirs !== undefined;
  const indent = depth * 14;

  if (isDir) {
    return (
      <div>
        <div onClick={() => setOpen(!open)} style={{ padding: "3px 10px", cursor: "pointer", display: "flex", alignItems: "center", gap: 4, fontSize: 12, paddingLeft: 10 + indent, fontFamily: "var(--font-mono)", color: "var(--color-muted-fg)" }}>
          <ChevronRight size={10} style={{ transform: open ? "rotate(90deg)" : "none", transition: "transform 0.15s" }} />
          <Folder size={12} /> {node.name}
        </div>
        {open && node.children.map((child: any, i: number) => (
          child.dirs ? <TreeNode key={child.name} node={child} onCompileFile={onCompileFile} onOpenPage={onOpenPage} depth={depth + 1} />
            : <FileRow key={i} file={child} onCompile={onCompileFile} onOpenPage={onOpenPage} depth={depth + 1} />
        ))}
      </div>
    );
  }
  return <FileRow file={node} onCompile={() => onCompileFile(node.path)} onOpenPage={onOpenPage} depth={depth} />;
}

function FileRow({ file, onCompile, onOpenPage, depth }: { file: SourceFile; onCompile: (path: string) => void; onOpenPage: (id: string) => void; depth: number }) {
  const icon = file.status === "compiling" ? <Loader size={11} style={{ color: "hsl(40 84% 60%)" }} />
    : file.status === "done" ? <CheckCircle size={11} style={{ color: "hsl(160 84% 39%)" }} />
    : file.status === "error" ? <XCircle size={11} style={{ color: "hsl(0 84% 60%)" }} />
    : <Circle size={11} style={{ color: "var(--color-muted-fg)" }} />;
  const indent = depth * 14;
  const isPdf = file.name.endsWith(".pdf");
  const handleClick = () => {
    if (isMarkdownSource(file.name)) {
      onOpenPage(file.name.replace(/\.\w+$/, ""));
    } else if (file.status === "done" && !isPdf) {
      onOpenPage(file.name.replace(/\.\w+$/, ""));
    } else {
      onCompile(file.path);
    }
  };

  return (
    <div style={{ padding: "3px 10px", cursor: "pointer", display: "flex", alignItems: "center", gap: 6, fontSize: 12, paddingLeft: 10 + indent }}
      onClick={handleClick}
      onMouseEnter={e => { (e.target as HTMLElement).style.background = "var(--color-muted)"; }}
      onMouseLeave={e => { (e.target as HTMLElement).style.background = "transparent"; }}>
      {icon}
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{file.name}</span>
      {file.pages > 0 && <span style={{ fontSize: 10, opacity: 0.5 }}>{file.pages}p</span>}
    </div>
  );
}

function SourceMarkdownEditor({ source, content, onChange, onSave, onCompile }: { source: { path: string; name: string }; content: string; onChange: (value: string) => void; onSave: () => void; onCompile: () => void }) {
  return (
    <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column" }} data-color-mode="dark">
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "16px 24px 12px", borderBottom: "1px solid var(--color-border)" }}>
        <div style={{ minWidth: 0 }}>
          <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--color-accent)", textTransform: "uppercase", letterSpacing: "0.05em" }}>source markdown</div>
          <h1 style={{ fontSize: 20, fontWeight: 600, margin: "2px 0 0", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{source.name}</h1>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button className="btn btn-primary" onClick={onSave} style={{ fontSize: 12 }}>Save</button>
          <button className="btn" onClick={onCompile} style={{ fontSize: 12 }}><Play size={13} /> Compile</button>
        </div>
      </div>
      <div style={{ flex: 1, minHeight: 0, padding: 16 }}>
        <MDEditor value={content} onChange={value => onChange(value || "")} height="100%" preview="live" textareaProps={{ spellCheck: false }} />
      </div>
    </div>
  );
}

// ═══════════════════════════════════════════════ PAGE VIEWER
function PageView({ pageId, pageName, content, editMode, editContent, setEditContent, onStartEdit, onSaveEdit, onCancelEdit, onSelectPage, onSelectTable }: any) {
  // Split frontmatter + body for styled display
  let fm: Record<string, any> | null = null;
  let body = content;
  if (content.startsWith("---")) {
    const end = content.indexOf("\n---", 4);
    if (end > 0) {
      try { fm = JSON.parse(JSON.stringify(content.slice(4, end).split("\n").reduce((acc: any, line: string) => { const [k, ...v] = line.split(":"); if (k && v.length) acc[k.trim()] = v.join(":").trim().replace(/^["']|["']$/g, ""); return acc; }, {}))); } catch {}
      body = content.slice(end + 4).trim();
    }
  }

  return (
    <div style={{ flex: 1, overflow: "auto" }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "16px 24px 12px", borderBottom: "1px solid var(--color-border)" }}>
        <div>
          <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--color-accent)", textTransform: "uppercase", letterSpacing: "0.05em" }}>page</div>
          <h1 style={{ fontSize: 20, fontWeight: 600, margin: "2px 0 0" }}>{pageName || pageId}</h1>
        </div>
        {editMode ? (
          <div style={{ display: "flex", gap: 8 }}><button className="btn btn-primary" onClick={onSaveEdit} style={{ fontSize: 12 }}>Save</button><button className="btn" onClick={onCancelEdit} style={{ fontSize: 12 }}>Cancel</button></div>
        ) : (
          <button className="btn" onClick={onStartEdit} style={{ fontSize: 12 }}><Box size={13} /> Edit</button>
        )}
      </div>

      {fm && Object.keys(fm).length > 0 && (
        <div style={{ margin: "16px 24px 0", padding: "14px 18px", background: "var(--color-muted)", border: "1px solid var(--color-border)", borderRadius: 8, display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))", gap: "6px 16px", fontSize: 12 }}>
          {Object.entries(fm).map(([k, v]) => (
            <div key={k} style={{ display: "flex", gap: 6, alignItems: "baseline" }}>
              <span style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--color-accent)", textTransform: "uppercase", flexShrink: 0 }}>{k}</span>
              <span style={{ color: "var(--color-fg)", wordBreak: "break-word" }}>
                {Array.isArray(v) ? v.join(", ") : String(v)}
              </span>
            </div>
          ))}
        </div>
      )}

      <div style={{ padding: "20px 32px 48px", maxWidth: 800 }}>
        {editMode ? (
          <div data-color-mode="dark">
            <MDEditor value={editContent} onChange={value => setEditContent(value || "")} height={520} preview="live" textareaProps={{ spellCheck: false }} />
          </div>
        ) : (
          <MarkdownView content={body} onNavigate={(id: string) => onSelectPage(id)} onNavigateTable={(tableId: string) => onSelectTable?.(tableId)} />
        )}
      </div>
    </div>
  );
}

function MarkdownView({ content, onNavigate, onNavigateTable }: { content: string; onNavigate?: (id: string) => void; onNavigateTable?: (tableId: string) => void }) {
  const markdown = preprocessWikiLinks(content);
  return (
    <div className="markdown-body"
      style={{ lineHeight: 1.8, fontSize: 15, color: "hsl(160 30% 88%)" }}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        urlTransform={(url) => {
          if (url.startsWith("wiki:") || url.startsWith("table:")) return url;
          return defaultUrlTransform(url);
        }}
        components={{
          a({ href, children, ...props }) {
            if (href?.startsWith("wiki:")) {
              const id = decodeURIComponent(href.slice(5));
              return <a href="#" className="wiki-link" onClick={(e) => { e.preventDefault(); onNavigate?.(id); }}>{children}</a>;
            }
            if (href?.startsWith("table:")) {
              const id = decodeURIComponent(href.slice(6));
              return <a href="#" className="table-link" onClick={(e) => { e.preventDefault(); onNavigateTable?.(id); }}>{children}</a>;
            }
            return <a href={href} target="_blank" rel="noopener noreferrer" {...props}>{children}</a>;
          },
        }}
      >
        {markdown}
      </ReactMarkdown>
    </div>
  );
}

function preprocessWikiLinks(text: string) {
  return text
    .replace(/\[\[table:([^\]]+?)\]\]/g, (_: string, inner: string) => {
      const [tableId, label] = inner.includes("|") ? inner.split("|") : [inner, inner];
      return `[${label.trim()}](table:${encodeURIComponent(tableId.trim())})`;
    })
    .replace(/\[\[([^\]]+?)\]\]/g, (_: string, inner: string) => {
      const [id, label] = inner.includes("|") ? inner.split("|") : [inner, inner];
      return `[${label.trim()}](wiki:${encodeURIComponent(id.trim())})`;
    });
}

// ═══════════════════════════════════════════════ LEDGER VIEW
function LedgerView({ tableName, content, error, tables, onSelectTable, editMode, editContent, setEditContent, onStartEdit, onSaveEdit, onCancelEdit }: any) {
  if (!tableName) {
    return (
      <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--color-muted-fg)", flexDirection: "column", gap: 8 }}>
        <Database size={48} style={{ opacity: 0.3 }} />
        <p style={{ fontFamily: "var(--font-mono)", fontSize: 13 }}>Select a table from the left list</p>
        {tables?.length > 0 && <p style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--color-muted-fg)" }}>{tables.length} tables available</p>}
      </div>
    );
  }

  // Parse content as JSON array of objects
  let rows: any[] = [];
  let cols: string[] = [];
  let parseError = false;
  const rawContent = typeof content === "string" && content.trim() ? content : "[]";
  try {
    let data = JSON.parse(rawContent);
    if (!Array.isArray(data)) data = [data];
    rows = data.filter((r: any) => r && typeof r === "object");
    if (rows.length > 0) {
      // Collect all unique keys as columns
      const keySet = new Set<string>();
      rows.forEach((r: any) => Object.keys(r).forEach(k => keySet.add(k)));
      cols = Array.from(keySet);
    }
  } catch { parseError = true; }

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 24px" }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 16 }}>
        <div>
          <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--color-accent)", textTransform: "uppercase", letterSpacing: "0.05em", display: "flex", alignItems: "center", gap: 6 }}>
            <Database size={12} /> ledger table
          </div>
          <h2 style={{ fontSize: 18, margin: "4px 0 0" }}>{tableName}</h2>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          <button className="btn" onClick={onStartEdit} style={{ fontSize: 12 }} disabled={Boolean(error)}>Edit JSON</button>
        </div>
      </div>

      {error ? (
        <div style={{ padding: 16, background: "hsla(0 84% 60% / 0.1)", border: "1px solid hsla(0 84% 60% / 0.3)", borderRadius: 6 }}>
          <p style={{ fontFamily: "var(--font-mono)", fontSize: 13, color: "hsl(0 84% 60%)" }}>Failed to load table</p>
          <pre style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--color-muted-fg)", marginTop: 8, whiteSpace: "pre-wrap" }}>{error}</pre>
        </div>
      ) : editMode ? (
        <div>
          <textarea value={editContent} onChange={e => setEditContent(e.target.value)}
            style={{ width: "100%", minHeight: "50vh", padding: 12, fontFamily: "var(--font-mono)", fontSize: 13, background: "var(--color-bg)", color: "var(--color-fg)", border: "1px solid var(--color-accent)", borderRadius: 6, resize: "vertical", outline: "none" }} />
          <div style={{ marginTop: 12, display: "flex", gap: 8 }}>
            <button className="btn btn-primary" onClick={onSaveEdit} style={{ fontSize: 12 }}>Save</button>
            {onCancelEdit && <button className="btn" onClick={onCancelEdit} style={{ fontSize: 12 }}>Cancel</button>}
          </div>
        </div>
      ) : rows.length > 0 ? (
        <div style={{ overflowX: "auto" }}>
          <div style={{ marginBottom: 8, fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--color-muted-fg)" }}>{rows.length} rows</div>
          <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 13 }}>
            <thead>
              <tr style={{ background: "var(--color-muted)" }}>
                <th style={{ padding: "8px 12px", textAlign: "left", width: 40, fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--color-muted-fg)", borderBottom: "2px solid var(--color-border)" }}>#</th>
                {cols.map(c => <th key={c} style={{ padding: "8px 12px", textAlign: "left", fontFamily: "var(--font-mono)", fontSize: 11, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--color-accent)", borderBottom: "2px solid var(--color-border)" }}>{c}</th>)}
              </tr>
            </thead>
            <tbody>
              {rows.map((row, i) => (
                <tr key={i} style={{ background: i % 2 === 0 ? "hsla(220 13% 8% / 0.5)" : "transparent" }}>
                  <td style={{ padding: "6px 12px", borderBottom: "1px solid var(--color-border)", color: "var(--color-muted-fg)", fontFamily: "var(--font-mono)", fontSize: 11 }}>{i + 1}</td>
                  {cols.map(c => <td key={c} style={{ padding: "6px 12px", borderBottom: "1px solid var(--color-border)" }}>{formatCell(row[c])}</td>)}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : parseError ? (
        <div style={{ padding: 16, background: "hsla(0 84% 60% / 0.1)", border: "1px solid hsla(0 84% 60% / 0.3)", borderRadius: 6 }}>
          <p style={{ fontFamily: "var(--font-mono)", fontSize: 13, color: "hsl(0 84% 60%)" }}>Failed to parse table data</p>
          <pre style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--color-muted-fg)", marginTop: 8, maxHeight: 200, overflow: "auto" }}>{rawContent.slice(0, 500)}</pre>
        </div>
      ) : (
        <div style={{ color: "var(--color-muted-fg)", fontFamily: "var(--font-mono)", fontSize: 13, textAlign: "center", padding: 40 }}>
          Empty table — no data yet
        </div>
      )}
    </div>
  );
}

function formatCell(val: any): string {
  if (val === null || val === undefined) return "—";
  if (typeof val === "boolean") return val ? "✓" : "✗";
  if (typeof val === "object") return JSON.stringify(val);
  return String(val);
}

function GraphCanvas({ nodes, edges, onSelectNode }: { nodes: any[]; edges: any[]; onSelectNode: (id: string) => void }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [hoveredNode, setHoveredNode] = useState<any>(null);
  const [zoomLevel, setZoomLevel] = useState(1);
  const [labelMode, setLabelMode] = useState<"key" | "all" | "none">("key");
  const hoveredNodeRef = useRef<any>(null);
  const transformRef = useRef({ x: 0, y: 0, scale: 1 });
  const targetRef = useRef({ x: 0, y: 0, scale: 1 });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || nodes.length === 0) return;
    const ctx = canvas.getContext("2d")!;
    const dpr = window.devicePixelRatio || 1;
    let W = 0, H = 0;
    function resize() {
      const r = canvas!.getBoundingClientRect();
      const nextW = Math.max(1, Math.floor(r.width));
      const nextH = Math.max(1, Math.floor(r.height));
      if (canvas!.width !== nextW * dpr || canvas!.height !== nextH * dpr) {
        canvas!.width = nextW * dpr;
        canvas!.height = nextH * dpr;
      }
      W = nextW;
      H = nextH;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    resize(); window.addEventListener("resize", resize);
    const nodeIds = new Set(nodes.map((n: any) => n.id));
    const validEdges = edges.filter((e: any) => nodeIds.has(e.source) && nodeIds.has(e.target));
    const degree = new Map<string, number>();
    for (const n of nodes) degree.set(n.id, 0);
    for (const e of validEdges) {
      degree.set(e.source, (degree.get(e.source) || 0) + 1);
      degree.set(e.target, (degree.get(e.target) || 0) + 1);
    }
    const sortedByDegree = [...nodes].sort((a, b) => (degree.get(b.id) || 0) - (degree.get(a.id) || 0));
    const keyNodeIds = new Set(sortedByDegree.slice(0, Math.max(8, Math.min(28, Math.ceil(nodes.length * 0.18)))).map((n: any) => n.id));
    const edgeBudget = Math.max(80, Math.min(420, nodes.length * 4));
    const linksForLayout = [...validEdges]
      .sort((a: any, b: any) => {
        const ap = a.type === "same_source" ? 1 : 0;
        const bp = b.type === "same_source" ? 1 : 0;
        return ap - bp;
      })
      .slice(0, edgeBudget);
    const spacing = Math.max(86, Math.min(150, 620 / Math.max(1, Math.sqrt(nodes.length))));
    const cols = Math.max(1, Math.ceil(Math.sqrt(nodes.length)));
    const rows = Math.max(1, Math.ceil(nodes.length / cols));
    const startX = W / 2 - ((cols - 1) * spacing) / 2;
    const startY = H / 2 - ((rows - 1) * spacing) / 2;
    const simNodes: any[] = sortedByDegree.map((n: any, i: number) => ({
      id: n.id,
      name: n.name || n.id,
      type: n.type,
      confidence: n.confidence || 0.5,
      degree: degree.get(n.id) || 0,
      isKey: keyNodeIds.has(n.id),
      x: startX + (i % cols) * spacing + (Math.random() - 0.5) * 18,
      y: startY + Math.floor(i / cols) * spacing + (Math.random() - 0.5) * 18,
      vx: 0,
      vy: 0,
      color: n.type === "concept" ? "#2dd4a0" : "#22c8dc"
    }));
    const graphW = (cols - 1) * spacing + 220;
    const graphH = (rows - 1) * spacing + 220;
    const fitScale = Math.max(0.35, Math.min(1, W / graphW, H / graphH));
    transformRef.current = { x: 0, y: 0, scale: fitScale };
    targetRef.current = { x: 0, y: 0, scale: fitScale };
    const nodeMap = new Map(simNodes.map(n => [n.id, n]));
    const links = linksForLayout.filter((e: any) => nodeMap.has(e.source) && nodeMap.has(e.target));
    let settled = false; let iter = 0;
    let lastZoom = 1;
    function step() {
      if (settled) return; iter++; let tv = 0;
      const repulsion = Math.max(26000, Math.min(90000, nodes.length * 980));
      const spring = links.length > nodes.length * 2 ? 0.0028 : 0.006;
      const springLength = Math.max(170, Math.min(260, 120 + nodes.length * 1.3));
      const damping = 0.82;
      const collision = labelMode === "all" ? 76 : 50;
      for (const n of simNodes) {
        n.vx += (W / 2 - n.x) * 0.00045;
        n.vy += (H / 2 - n.y) * 0.00045;
      }
      if (simNodes.length <= 350) {
        for (let i=0;i<simNodes.length;i++) for (let j=i+1;j<simNodes.length;j++) {
          const dx=simNodes[j].x-simNodes[i].x, dy=simNodes[j].y-simNodes[i].y, d=Math.hypot(dx,dy)+1;
          const f=repulsion/(d*d);
          simNodes[i].vx-=f*dx/d; simNodes[i].vy-=f*dy/d; simNodes[j].vx+=f*dx/d; simNodes[j].vy+=f*dy/d;
          const minD = collision + Math.min(20, (simNodes[i].degree + simNodes[j].degree) * 0.4);
          if (d < minD) {
            const push = (minD - d) * 0.035;
            simNodes[i].vx-=push*dx/d; simNodes[i].vy-=push*dy/d; simNodes[j].vx+=push*dx/d; simNodes[j].vy+=push*dy/d;
          }
        }
      }
      for (const l of links) {
        const s=nodeMap.get(l.source), t=nodeMap.get(l.target); if(!s||!t)continue;
        const dx=t.x-s.x,dy=t.y-s.y,d=Math.hypot(dx,dy)+1;
        const linkLength = l.type === "same_source" ? springLength * 1.35 : springLength;
        const linkSpring = l.type === "same_source" ? spring * 0.45 : spring;
        const f=(d-linkLength)*linkSpring; s.vx+=f*dx/d;s.vy+=f*dy/d;t.vx-=f*dx/d;t.vy-=f*dy/d;
      }
      for (const n of simNodes) {
        n.vx*=damping; n.vy*=damping; n.x+=n.vx; n.y+=n.vy;
        if (!Number.isFinite(n.x) || !Number.isFinite(n.y)) { n.x = W/2; n.y = H/2; n.vx = 0; n.vy = 0; }
        tv+=Math.abs(n.vx)+Math.abs(n.vy);
      }
      if (iter>(simNodes.length > 350 ? 120 : 520)||tv<.35) settled=true;
    }
    let animId = 0;
    function nodeRadius(n: any, scale: number) {
      return (7 + Math.sqrt(Math.max(1, n.degree)) * 2.3 + n.confidence * 7) / Math.sqrt(scale);
    }
    function shouldLabel(n: any, scale: number) {
      const hovered = hoveredNodeRef.current;
      if (labelMode === "none") return hovered?.id === n.id;
      if (labelMode === "all") return scale >= 0.55 || n.isKey || hovered?.id === n.id;
      return n.isKey || hovered?.id === n.id || (scale > 1.45 && n.degree > 1);
    }
    function drawLabel(n: any, r: number, scale: number, boxes: Array<{x:number;y:number;w:number;h:number}>) {
      if (!shouldLabel(n, scale)) return;
      const raw = n.name || n.id;
      const lbl = raw.length > 24 ? raw.slice(0, 22) + "…" : raw;
      const fontSize = Math.max(11, Math.min(15, 12 / Math.sqrt(scale)));
      ctx.font = `${fontSize}px Inter, system-ui, sans-serif`;
      const w = Math.min(260, ctx.measureText(lbl).width + 12);
      const h = fontSize + 8;
      const x = n.x - w / 2;
      const y = n.y - r - h - 8 / scale;
      const overlaps = boxes.some(b => x < b.x + b.w && x + w > b.x && y < b.y + b.h && y + h > b.y);
      const hovered = hoveredNodeRef.current;
      if (overlaps && hovered?.id !== n.id) return;
      boxes.push({ x, y, w, h });
      ctx.fillStyle = hovered?.id === n.id ? "rgba(18, 28, 26, 0.96)" : "rgba(8, 13, 12, 0.72)";
      ctx.strokeStyle = hovered?.id === n.id ? "rgba(125, 255, 215, 0.55)" : "rgba(125, 255, 215, 0.16)";
      ctx.lineWidth = 1 / scale;
      roundRect(ctx, x, y, w, h, 5 / scale);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = "#e4fff5";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(lbl, n.x, y + h / 2);
    }
    function draw() {
      resize(); const t = targetRef.current, tr = transformRef.current;
      tr.scale += (t.scale-tr.scale)*.12; tr.x += (t.x-tr.x)*.12; tr.y += (t.y-tr.y)*.12;
      const nextZoom = Math.round(tr.scale*100)/100;
      if (Math.abs(nextZoom - lastZoom) >= 0.01) { lastZoom = nextZoom; setZoomLevel(nextZoom); }
      for (let i=0;i<3;i++) step();
      ctx.clearRect(0,0,W,H); ctx.save();
      ctx.translate(W/2+tr.x, H/2+tr.y); ctx.scale(tr.scale,tr.scale); ctx.translate(-W/2,-H/2);
      for (const l of links) { const s=nodeMap.get(l.source),t=nodeMap.get(l.target); if(!s||!t)continue; ctx.beginPath();ctx.moveTo(s.x,s.y);ctx.lineTo(t.x,t.y); ctx.strokeStyle=l.type==="same_source"?"rgba(45,212,160,0.13)":"rgba(45,212,160,0.25)";ctx.lineWidth=(l.type==="same_source"?0.7:1.1)/tr.scale;ctx.stroke(); }
      for (const n of simNodes) {
        const r=nodeRadius(n,tr.scale);
        ctx.beginPath();ctx.arc(n.x,n.y,r,0,Math.PI*2);
        ctx.fillStyle=n.color;ctx.fill();
        const hovered = hoveredNodeRef.current;
        ctx.strokeStyle=hovered?.id===n.id?"rgba(224,255,240,0.9)":"rgba(0,0,0,0.42)";
        ctx.lineWidth=(hovered?.id===n.id?2.2:1.4)/tr.scale;ctx.stroke();
      }
      const labelBoxes: Array<{x:number;y:number;w:number;h:number}> = [];
      for (const n of [...simNodes].sort((a,b)=>Number(a.isKey)-Number(b.isKey))) drawLabel(n,nodeRadius(n,tr.scale),tr.scale,labelBoxes);
      ctx.restore(); animId = requestAnimationFrame(draw);
    }
    draw();
    function world(e: MouseEvent) { const r = canvas!.getBoundingClientRect(); return { x: e.clientX-r.left, y: e.clientY-r.top }; }
    function toGraphPoint(p: {x:number;y:number}) {
      const tr = transformRef.current;
      const wx=p.x-W/2,wy=p.y-H/2;
      return { x:(wx-tr.x)/tr.scale+W/2, y:(wy-tr.y)/tr.scale+H/2 };
    }
    function hitNode(p: {x:number;y:number}) {
      const gp = toGraphPoint(p);
      const tr = transformRef.current;
      let best: any = null;
      let bestD = Infinity;
      for (const n of simNodes) {
        const d = Math.hypot(gp.x-n.x,gp.y-n.y);
        const r = nodeRadius(n, tr.scale) + 8 / tr.scale;
        if (d < r && d < bestD) { best = n; bestD = d; }
      }
      return best;
    }
    let pan: any = null;
    canvas.onmousedown = e => { const p = world(e); let hit = false; const tr = transformRef.current;
      const n = hitNode(p);
      if(n){targetRef.current={x:(W/2-n.x)*1.55,y:(H/2-n.y)*1.55,scale:1.55};onSelectNode(n.id);hit=true;}
      if(!hit) pan = { x:p.x, y:p.y, tx:tr.x, ty:tr.y }; };
    window.onmousemove = e => {
      const p=world(e);
      if(!pan) {
        const next = hitNode(p);
        if (hoveredNodeRef.current?.id !== next?.id) {
          hoveredNodeRef.current = next;
          setHoveredNode(next);
        }
        return;
      }
      transformRef.current.x=pan.tx+p.x-pan.x; transformRef.current.y=pan.ty+p.y-pan.y; targetRef.current.x=transformRef.current.x; targetRef.current.y=transformRef.current.y;
    };
    window.onmouseup = () => { pan = null; };
    canvas.onwheel = e => { e.preventDefault(); targetRef.current.scale=Math.max(.3,Math.min(5,targetRef.current.scale*(e.deltaY>0?.9:1.1))); };
    canvas.ondblclick = () => { targetRef.current = { x:0,y:0,scale:1 }; };
    return () => {
      cancelAnimationFrame(animId);
      window.removeEventListener("resize",resize);
      canvas.onmousedown = null;
      canvas.onwheel = null;
      canvas.ondblclick = null;
      window.onmousemove = null;
      window.onmouseup = null;
    };
  }, [nodes, edges, onSelectNode, labelMode]);

  if (nodes.length === 0) return <Empty icon={<Network size={48} style={{opacity:.3}} />} text="No graph data — compile documents first" />;
  return (
    <div style={{ flex: 1, position: "relative", overflow: "hidden", background: "var(--color-bg)" }}>
      <canvas ref={canvasRef} style={{ width: "100%", height: "100%", display: "block" }} />
      <div style={{ position: "absolute", top: 12, left: 12, display: "flex", gap: 6, alignItems: "center", background: "color-mix(in srgb, var(--color-surface) 88%, transparent)", border: "1px solid var(--color-border)", borderRadius: 6, padding: 4 }}>
        {(["key", "all", "none"] as const).map(mode => (
          <button key={mode} className={labelMode === mode ? "btn btn-primary" : "btn"} style={{ padding: "4px 8px", fontSize: 11 }} onClick={() => setLabelMode(mode)}>
            {mode === "key" ? "Key labels" : mode === "all" ? "All labels" : "No labels"}
          </button>
        ))}
      </div>
      {hoveredNode && (
        <div style={{ position: "absolute", left: 12, bottom: 12, maxWidth: 360, background: "var(--color-surface)", border: "1px solid var(--color-border-glow)", borderRadius: 6, padding: "8px 10px", boxShadow: "0 10px 30px rgba(0,0,0,.28)" }}>
          <div style={{ fontSize: 13, color: "var(--color-fg)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{hoveredNode.name}</div>
          <div style={{ marginTop: 4, fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--color-muted-fg)" }}>{hoveredNode.type || "node"} · {hoveredNode.degree || 0} links</div>
        </div>
      )}
      <div style={{ position: "absolute", bottom: 8, right: 8, fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--color-muted-fg)", background: "var(--color-surface)", padding: "3px 8px", borderRadius: 4 }}>
        {Math.round(zoomLevel*100)}% · {nodes.length} nodes · {edges.length} edges · scroll · drag · click · dblclick
      </div>
    </div>
  );
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  const rr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
}

// ═══════════════════════════════════════════════ SHARED
function Welcome({ onOpen }: { onOpen: () => void }) {
  return (
    <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", flexDirection: "column", gap: 16, color: "var(--color-muted-fg)" }}>
      <div style={{ fontFamily: "var(--font-mono)", fontSize: 48, color: "var(--color-accent)", opacity: 0.3, display: "flex", alignItems: "center", gap: 12 }}>
        <Folder size={42} style={{ opacity: 0.5 }} /> llm-wiki
      </div>
      <p style={{ fontFamily: "var(--font-mono)", fontSize: 14 }}>Personal Knowledge Base</p>
      <button className="btn btn-primary" onClick={onOpen}>Open Workspace</button>
    </div>
  );
}

function EmptyState() {
  return (
    <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--color-muted-fg)", flexDirection: "column", gap: 8 }}>
      <FileText size={48} style={{ opacity: 0.2 }} />
      <p style={{ fontFamily: "var(--font-mono)", fontSize: 13 }}>Select a page to view</p>
    </div>
  );
}

function Empty({ icon, text }: { icon: React.ReactNode; text: string }) {
  return <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--color-muted-fg)", flexDirection: "column", gap: 8 }}>{icon}<p style={{ fontFamily: "var(--font-mono)", fontSize: 13 }}>{text}</p></div>;
}

function ChatPanel({ messages, onSend, onNavigate }: any) {
  const [input, setInput] = useState("");
  const isBusy = messages.length > 0 && (messages[messages.length - 1]?.status === "searching" || messages[messages.length - 1]?.status === "streaming");

  const handleSend = () => {
    if (!input.trim() || isBusy) return;
    onSend(input.trim()); setInput("");
  };

  return (
    <div style={{ width: 340, minWidth: 340, borderLeft: "1px solid var(--color-border)", display: "flex", flexDirection: "column", background: "var(--color-surface)" }}>
      <div className="panel-header">Chat</div>
      <div style={{ flex: 1, overflow: "auto", padding: "10px 12px", display: "flex", flexDirection: "column", gap: 10 }}>
        {messages.length === 0 && (
          <div style={{ color: "var(--color-muted-fg)", fontFamily: "var(--font-mono)", fontSize: 12, textAlign: "center", marginTop: 40 }}>
            Ask questions about your wiki
          </div>
        )}
        {messages.map((m: any, i: number) => <ChatBubble key={i} msg={m} onNavigate={onNavigate} />)}
      </div>
      <div style={{ padding: "8px 10px", borderTop: "1px solid var(--color-border)" }}>
        <div style={{ display: "flex", gap: 6 }}>
          <input value={input} onChange={e => setInput(e.target.value)} onKeyDown={e => e.key === "Enter" && handleSend()} placeholder="Ask..." disabled={isBusy}
            style={{ flex: 1, padding: "7px 10px", fontSize: 13, background: "var(--color-bg)", border: "1px solid var(--color-border)", borderRadius: 6, color: "var(--color-fg)", outline: "none" }} />
          <button onClick={handleSend} className="btn btn-primary" style={{ padding: "6px 10px", fontSize: 12 }} disabled={isBusy}><Send size={14} /></button>
        </div>
      </div>
    </div>
  );
}

function ChatBubble({ msg, onNavigate }: { msg: any; onNavigate: (s: any) => void }) {
  if (msg.role === "user") {
    return <div style={{ alignSelf: "flex-end", maxWidth: "85%", padding: "8px 12px", borderRadius: 8, background: "var(--color-muted)", fontSize: 13, lineHeight: 1.5 }}>{msg.content}</div>;
  }

  const citations: any[] = msg.citations || [];
  const isStreaming = msg.status === "streaming";
  const isSearching = msg.status === "searching";

  // Clean answer: remove inline [N] markers (shown as cards instead)
  const cleanAnswer = msg.content
    ? msg.content.replace(/\[(\d+)\]/g, "").replace(/\[\[([^\]]+)\]\]/g, "$1")
    : "";

  if (isSearching) {
    const phaseLabel = msg.phase === "generating" ? "Generating answer with LLM..." : "Searching wiki pages...";
    const elapsed = msg.elapsed ? ` (${msg.elapsed.toFixed(1)}s)` : "";
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 6, padding: "10px 13px", borderRadius: 8, background: "hsl(160 20% 8%)", border: "1px solid var(--color-accent-dim)", maxWidth: "92%" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, color: "var(--color-accent)", fontFamily: "var(--font-mono)" }}>
          <Loader size={14} style={{ animation: "spin 1s linear infinite" }} /> {phaseLabel}<span style={{ opacity: 0.6, fontSize: 11 }}>{elapsed}</span>
        </div>
        {msg.phase === "generating" && <div style={{ height: 3, background: "var(--color-muted)", borderRadius: 2, overflow: "hidden" }}><div style={{ height: "100%", width: "60%", background: "var(--color-accent)", borderRadius: 2, animation: "pulse 1.5s ease-in-out infinite" }} /></div>}
      </div>
    );
  }

  return (
    <div style={{ alignSelf: "flex-start", maxWidth: "92%" }}>
      <div style={{ padding: "10px 13px", borderRadius: 8, background: "hsl(160 20% 8%)", border: "1px solid var(--color-accent-dim)", fontSize: 13, lineHeight: 1.7, color: "hsl(160 20% 90%)" }}>
        <div style={{ whiteSpace: "pre-wrap" }}>{cleanAnswer}</div>
        {isStreaming && <span style={{ display: "inline-block", width: 8, height: 14, background: "var(--color-accent)", animation: "pulse 0.8s ease-in-out infinite", marginLeft: 2, verticalAlign: "middle" }} />}
      </div>

      {(msg.searchTime !== undefined || msg.genTime !== undefined) && (
        <div style={{ marginTop: 4, fontSize: 10, color: "var(--color-muted-fg)", fontFamily: "var(--font-mono)", display: "flex", gap: 10 }}>
          {msg.searchTime !== undefined && <span>Search: {msg.searchTime}s</span>}
          {msg.genTime !== undefined && <span>LLM: {msg.genTime}s</span>}
        </div>
      )}
      {!isStreaming && citations.length > 0 && (
        <div style={{ marginTop: 6, display: "flex", flexWrap: "wrap", gap: 4 }}>
          {citations.map((s: any, i: number) => (
            <div key={i} onClick={() => onNavigate(s)}
              style={{ display: "inline-flex", alignItems: "center", gap: 5, padding: "4px 8px", borderRadius: 4, cursor: "pointer", fontSize: 11, background: "var(--color-muted)", border: "1px solid var(--color-border)" }}
              onMouseEnter={e => { (e.target as HTMLElement).style.borderColor = "var(--color-accent)"; }}
              onMouseLeave={e => { (e.target as HTMLElement).style.borderColor = "var(--color-border)"; }}>
              <span style={{ color: "var(--color-accent)", fontFamily: "var(--font-mono)", fontSize: 10 }}>[{i+1}]</span> {s.name}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function StatusBar({ wsName, pageCount, entityCount, edgeCount, compileDone, compileTotal, onOpenWorkspace }: any) {
  return (
    <div style={{ height: 26, minHeight: 26, background: "var(--color-muted)", borderTop: "1px solid var(--color-border)", display: "flex", alignItems: "center", padding: "0 10px", fontFamily: "var(--font-mono)", fontSize: 11, color: "var(--color-muted-fg)", gap: 14 }}>
      <button onClick={onOpenWorkspace} style={{ background: "none", border: "none", color: "var(--color-accent)", cursor: "pointer", fontFamily: "inherit", fontSize: 11, display: "flex", alignItems: "center", gap: 4 }}>
        <Folder size={12} /> {wsName || "No workspace"}
      </button>
      {compileTotal > 0 && <span style={{ display: "flex", alignItems: "center", gap: 4 }}><Play size={10} /> {compileDone}/{compileTotal}</span>}
      <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 12 }}>
        <span><FileText size={11} /> {pageCount}p</span>
        <span><Box size={11} /> {entityCount}e</span>
        <span><Network size={11} /> {edgeCount}↗</span>
      </span>
    </div>
  );
}

const OCR_MODEL_OPTIONS: Record<string, { v: string; l: string; desc: string }[]> = {
  "unlimited-ocr-mlx": [
    { v: "Unlimited-OCR-MLX", l: "Unlimited-OCR-MLX", desc: "ModelScope MLX OCR, macOS" },
  ],
  "paddleocr-vl": [
    { v: "PaddleOCR-VL-1.5-8bit", l: "PaddleOCR-VL-1.5-8bit", desc: "MLX spotting, macOS" },
  ],
  "paddleocr": [
    { v: "PP-OCRv5_server", l: "PP-OCRv5 Server", desc: "High accuracy boxes" },
    { v: "PP-OCRv5_mobile", l: "PP-OCRv5 Mobile", desc: "Smaller local model" },
    { v: "default", l: "PaddleOCR Default", desc: "Use runtime default" },
  ],
  "mineru": [
    { v: "MinerU2.5", l: "MinerU2.5", desc: "Document parser boxes" },
  ],
  "deepseek-ocr": [
    { v: "DeepSeek-OCR-2", l: "DeepSeek-OCR-2", desc: "Grounding boxes" },
  ],
};

function ocrModelOptions(engine: string, current: string) {
  const options = OCR_MODEL_OPTIONS[engine] || OCR_MODEL_OPTIONS["paddleocr-vl"];
  if (current && !options.some(o => o.v === current)) {
    return [{ v: current, l: current, desc: "Configured model" }, ...options];
  }
  return options;
}

function defaultOcrModel(engine: string) {
  return (OCR_MODEL_OPTIONS[engine] || OCR_MODEL_OPTIONS["unlimited-ocr-mlx"])[0].v;
}

function ConfigModal({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState("model");
  const [provider, setProvider] = useState("deepseek");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("deepseek-v4-flash");
  const [baseUrl, setBaseUrl] = useState("");
  const [temperature, setTemperature] = useState("0.3");
  const [ocrUrl, setOcrUrl] = useState("");
  const [ocrEngine, setOcrEngine] = useState("unlimited-ocr-mlx");
  const [ocrModel, setOcrModel] = useState("Unlimited-OCR-MLX");
  const [ocrModelRoot, setOcrModelRoot] = useState("");
  const [ocrDevice, setOcrDevice] = useState("auto");
  const [ocrAutoDownload, setOcrAutoDownload] = useState(true);
  const [unlimitedOcrTask, setUnlimitedOcrTask] = useState("document");
  const [unlimitedOcrPrompt, setUnlimitedOcrPrompt] = useState("");
  const [unlimitedOcrMaxNewTokens, setUnlimitedOcrMaxNewTokens] = useState("4096");
  const [unlimitedOcrCropMode, setUnlimitedOcrCropMode] = useState(true);
  const [ocrLang, setOcrLang] = useState("chi_sim+eng");
  const [ocrEnabled, setOcrEnabled] = useState(true);
  const [maxResults, setMaxResults] = useState("5");
  const [saved, setSaved] = useState(false);

  const changeOcrEngine = (value: string) => {
    setOcrEngine(value);
    setOcrModel(defaultOcrModel(value));
  };

  // Load current config on mount
  useEffect(() => { (async () => {
    try { const c = await invoke("get_full_config") as any;
      setProvider(c.model?.provider || "deepseek"); setApiKey(c.model?.apiKey || ""); setModel(c.model?.model || ""); setBaseUrl(c.model?.baseUrl || ""); setTemperature(String(c.model?.temperature || 0.3));
      setOcrUrl(c.liteparse?.ocrServerUrl || ""); setOcrLang(c.liteparse?.ocrLanguage || "chi_sim+eng"); setOcrEnabled(c.liteparse?.ocrEnabled !== false);
      setOcrEngine(c.ocr?.engine || "unlimited-ocr-mlx"); setOcrModel(c.ocr?.model || "Unlimited-OCR-MLX"); setOcrModelRoot(c.ocr?.modelRoot || ""); setOcrDevice(c.ocr?.device || "auto"); setOcrAutoDownload(c.ocr?.autoDownload !== false);
      setUnlimitedOcrTask(c.ocr?.options?.task || "document"); setUnlimitedOcrPrompt(c.ocr?.options?.prompt || ""); setUnlimitedOcrMaxNewTokens(String(c.ocr?.options?.max_new_tokens || 4096)); setUnlimitedOcrCropMode(c.ocr?.options?.crop_mode !== false);
      setMaxResults(String(c.query?.maxResults || 5));
    } catch {} })();
  }, []);

  const save = async () => {
    try {
      await invoke("save_config", { config: { provider, apiKey, model, baseUrl, temperature: parseFloat(temperature) || 0.3, ocrServerUrl: ocrUrl, ocrLanguage: ocrLang, ocrEnabled, ocrEngine, ocrModel, ocrModelRoot, ocrDevice, ocrAutoDownload, unlimitedOcrTask, unlimitedOcrPrompt, unlimitedOcrMaxNewTokens: parseInt(unlimitedOcrMaxNewTokens) || 4096, unlimitedOcrCropMode, maxResults: parseInt(maxResults) || 5 } });
      setSaved(true); setTimeout(() => { onClose(); window.location.reload(); }, 800);
    } catch (e) { console.error(e); }
  };

  const tabs = [{ k: "model", l: "LLM Model" }, { k: "liteparse", l: "Liteparse OCR" }, { k: "query", l: "Query" }];

  return (
    <div style={{ position: "fixed", inset: 0, background: "hsla(220 13% 5% / 0.85)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
      <div style={{ background: "var(--color-surface)", border: "1px solid var(--color-border-glow)", borderRadius: 12, width: 540, maxHeight: "80vh", overflow: "auto", boxShadow: "0 0 40px hsla(160 84% 39% / 0.15)" }}>
        <div style={{ display: "flex", borderBottom: "1px solid var(--color-border)" }}>
          {tabs.map(t => <button key={t.k} onClick={() => setTab(t.k)} style={{ flex: 1, padding: "10px 0", border: "none", cursor: "pointer", fontFamily: "var(--font-mono)", fontSize: 12, textTransform: "uppercase", letterSpacing: "0.05em", background: tab === t.k ? "var(--color-muted)" : "transparent", color: tab === t.k ? "var(--color-accent)" : "var(--color-muted-fg)", borderBottom: tab === t.k ? "2px solid var(--color-accent)" : "2px solid transparent" }}>{t.l}</button>)}
        </div>
        <div style={{ padding: 24 }}>
          {saved ? <div style={{ textAlign: "center", color: "var(--color-accent)", fontFamily: "var(--font-mono)" }}>Saved! Restarting...</div> : <>
            {tab === "model" && <>
              <Field label="Provider"><select value={provider} onChange={e => setProvider(e.target.value)} className="field-input"><option value="deepseek">DeepSeek</option><option value="openai">OpenAI</option><option value="ollama">Ollama</option><option value="custom">Custom</option></select></Field>
              <Field label="API Key"><input type="password" value={apiKey} onChange={e => setApiKey(e.target.value)} placeholder="sk-..." className="field-input" /></Field>
              <Field label="Model"><input value={model} onChange={e => setModel(e.target.value)} className="field-input" /></Field>
              <Field label="Base URL"><input value={baseUrl} onChange={e => setBaseUrl(e.target.value)} placeholder="Auto from provider" className="field-input" /></Field>
              <Field label="Temperature"><input value={temperature} onChange={e => setTemperature(e.target.value)} className="field-input" /></Field>
            </>}
            {tab === "liteparse" && <>
              <Field label="OCR Engine"><select value={ocrEngine} onChange={e => changeOcrEngine(e.target.value)} className="field-input"><option value="unlimited-ocr-mlx">Unlimited-OCR-MLX</option><option value="paddleocr-vl">PaddleOCR-VL</option><option value="paddleocr">PaddleOCR PP-OCR</option><option value="mineru">MinerU</option><option value="deepseek-ocr">DeepSeek-OCR</option></select></Field>
              <Field label="Model Root"><input value={ocrModelRoot} onChange={e => setOcrModelRoot(e.target.value)} placeholder="Leave empty for .wiki/models/ocr" className="field-input" /></Field>
              <Field label="OCR Model"><select value={ocrModel} onChange={e => setOcrModel(e.target.value)} className="field-input">{ocrModelOptions(ocrEngine, ocrModel).map(o => <option key={o.v} value={o.v}>{o.l} — {o.desc}</option>)}</select></Field>
              <Field label="Device"><select value={ocrDevice} onChange={e => setOcrDevice(e.target.value)} className="field-input"><option value="auto">Auto</option><option value="cpu">CPU</option><option value="cuda">CUDA</option><option value="mps">MPS</option></select></Field>
              <Field label="Auto Download"><input type="checkbox" checked={ocrAutoDownload} onChange={e => setOcrAutoDownload(e.target.checked)} /></Field>
              {ocrEngine === "unlimited-ocr-mlx" && <>
                <Field label="Unlimited Task"><select value={unlimitedOcrTask} onChange={e => setUnlimitedOcrTask(e.target.value)} className="field-input"><option value="document">Document</option><option value="text">Text</option><option value="figure">Figure</option><option value="free">Free OCR</option></select></Field>
                <Field label="Custom Prompt"><input value={unlimitedOcrPrompt} onChange={e => setUnlimitedOcrPrompt(e.target.value)} placeholder="Overrides task prompt when set" className="field-input" /></Field>
                <Field label="Max New Tokens"><input value={unlimitedOcrMaxNewTokens} onChange={e => setUnlimitedOcrMaxNewTokens(e.target.value)} className="field-input" /></Field>
                <Field label="Dynamic Tiling"><input type="checkbox" checked={unlimitedOcrCropMode} onChange={e => setUnlimitedOcrCropMode(e.target.checked)} /></Field>
              </>}
              <Field label="Advanced OCR Server URL"><input value={ocrUrl} onChange={e => setOcrUrl(e.target.value)} placeholder="Optional liteparse-compatible /ocr endpoint" className="field-input" /></Field>
              <Field label="OCR Language"><input value={ocrLang} onChange={e => setOcrLang(e.target.value)} className="field-input" /></Field>
              <Field label="OCR Enabled"><input type="checkbox" checked={ocrEnabled} onChange={e => setOcrEnabled(e.target.checked)} /></Field>
            </>}
            {tab === "query" && <>
              <Field label="Max Results"><input value={maxResults} onChange={e => setMaxResults(e.target.value)} className="field-input" /></Field>
            </>}
            <div style={{ display: "flex", gap: 10, marginTop: 24 }}><button className="btn btn-primary" onClick={save} style={{ flex: 1 }}>Save Settings</button><button className="btn" onClick={onClose}>Cancel</button></div>
          </>}
        </div>
      </div>
    </div>
  );
}
function Field({ label, children }: { label: string; children: React.ReactNode }) { return <div style={{ marginBottom: 12 }}><label style={{ display: "block", fontFamily: "var(--font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--color-muted-fg)", marginBottom: 3 }}>{label}</label>{children}</div>; }
