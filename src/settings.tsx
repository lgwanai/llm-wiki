import React, { useState, useEffect, useRef } from "react";
import ReactDOM from "react-dom/client";
import { Settings, Cpu, Search, Globe, Key, Zap, Eye, EyeOff, Languages, ChevronRight } from "lucide-react";

let _invoke: any = null;
async function invoke(cmd: string, args?: any) { if (!_invoke) { const m = await import("@tauri-apps/api/core"); _invoke = m.invoke; } return _invoke(cmd, args); }

const OCR_MODEL_OPTIONS: Record<string, { v: string; l: string; desc: string }[]> = {
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
  return (OCR_MODEL_OPTIONS[engine] || OCR_MODEL_OPTIONS["paddleocr-vl"])[0].v;
}

function SettingsWindow() {
  const [section, setSection] = useState("model");
  const [provider, setProvider] = useState("deepseek");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [temperature, setTemperature] = useState(0.3);
  const [ocrUrl, setOcrUrl] = useState("");
  const [ocrEngine, setOcrEngine] = useState("paddleocr-vl");
  const [ocrModel, setOcrModel] = useState("PaddleOCR-VL-1.5-8bit");
  const [ocrModelRoot, setOcrModelRoot] = useState("");
  const [ocrDevice, setOcrDevice] = useState("auto");
  const [ocrAutoDownload, setOcrAutoDownload] = useState(true);
  const [ocrLang, setOcrLang] = useState("chi_sim+eng");
  const [ocrEnabled, setOcrEnabled] = useState(false);
  const [maxResults, setMaxResults] = useState(5);
  const [stripSensitive, setStripSensitive] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const saveTimer = useRef<any>(null);

  const changeOcrEngine = (value: string) => {
    setOcrEngine(value);
    setOcrModel(defaultOcrModel(value));
  };

  // Load config on mount
  useEffect(() => { (async () => {
    try { const c = await invoke("get_full_config") as any;
      setProvider(c.model?.provider || "deepseek"); setApiKey(c.model?.apiKey || ""); setModel(c.model?.model || ""); setBaseUrl(c.model?.baseUrl || ""); setTemperature(c.model?.temperature || 0.3);
      setOcrUrl(c.liteparse?.ocrServerUrl || ""); setOcrLang(c.liteparse?.ocrLanguage || "chi_sim+eng"); setOcrEnabled(c.liteparse?.ocrEnabled === true);
      setOcrEngine(c.ocr?.engine || "paddleocr-vl"); setOcrModel(c.ocr?.model || "PaddleOCR-VL-1.5-8bit"); setOcrModelRoot(c.ocr?.modelRoot || ""); setOcrDevice(c.ocr?.device || "auto"); setOcrAutoDownload(c.ocr?.autoDownload !== false);
      setMaxResults(c.query?.maxResults || 5);
      setStripSensitive(c.compile?.stripSensitive === true);
      setLoaded(true);
    } catch {} })();
  }, []);

  // Auto-save with debounce
  const autoSave = () => {
    if (!loaded) return;
    clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      invoke("save_config", { config: { provider, apiKey, model, baseUrl, temperature, ocrServerUrl: ocrUrl, ocrLanguage: ocrLang, ocrEnabled, ocrEngine, ocrModel, ocrModelRoot, ocrDevice, ocrAutoDownload, maxResults, stripSensitive } });
    }, 400);
  };

  useEffect(autoSave, [provider, apiKey, model, baseUrl, temperature, ocrUrl, ocrLang, ocrEnabled, ocrEngine, ocrModel, ocrModelRoot, ocrDevice, ocrAutoDownload, maxResults, stripSensitive]);

  const sections = [
    { id: "model", label: "Model", icon: <Cpu size={15} />, desc: "LLM provider & API settings" },
    { id: "ocr", label: "OCR", icon: <Eye size={15} />, desc: "Liteparse document OCR" },
    { id: "compile", label: "Compile", icon: <Zap size={15} />, desc: "Compilation behaviour" },
    { id: "query", label: "Query", icon: <Search size={15} />, desc: "Search & retrieval" },
  ];

  return (
    <div style={{ minHeight: "100vh", background: "var(--color-bg)", display: "flex", fontFamily: "var(--font-sans)" }}>
      {/* Sidebar */}
      <div style={{ width: 200, minWidth: 200, background: "var(--color-surface)", borderRight: "1px solid var(--color-border)", padding: "20px 0", WebkitAppRegion: "drag" } as any}>
        <div style={{ padding: "0 16px 16px", borderBottom: "1px solid var(--color-border)", marginBottom: 8 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Settings size={18} style={{ color: "var(--color-accent)" }} />
            <span style={{ fontFamily: "var(--font-mono)", fontSize: 14, color: "var(--color-accent)", fontWeight: 600 }}>Settings</span>
          </div>
          <div style={{ fontSize: 10, color: "var(--color-muted-fg)", marginTop: 4, fontFamily: "var(--font-mono)" }}>Global configuration</div>
        </div>
        {sections.map(s => (
          <div key={s.id}
            onClick={() => setSection(s.id)}
            style={{
              display: "flex", alignItems: "center", gap: 10, padding: "10px 16px", cursor: "pointer",
              background: section === s.id ? "var(--color-muted)" : "transparent",
              borderLeft: section === s.id ? "3px solid var(--color-accent)" : "3px solid transparent",
              color: section === s.id ? "var(--color-fg)" : "var(--color-muted-fg)",
              transition: "all 0.15s",
            }}>
            <span style={{ opacity: section === s.id ? 1 : 0.5 }}>{s.icon}</span>
            <div>
              <div style={{ fontSize: 13, fontWeight: 500 }}>{s.label}</div>
              <div style={{ fontSize: 10, opacity: 0.6 }}>{s.desc}</div>
            </div>
          </div>
        ))}
        {/* Saved indicator */}
        <div style={{ position: "absolute", bottom: 16, left: 16, fontSize: 10, color: "var(--color-accent)", fontFamily: "var(--font-mono)", opacity: 0.6 }}>
          Auto-saved
        </div>
      </div>

      {/* Main content */}
      <div style={{ flex: 1, padding: "28px 36px", overflow: "auto", WebkitAppRegion: "drag" } as any}>
        {section === "model" && (
          <div style={{ maxWidth: 480 }}>
            <SectionTitle icon={<Cpu size={16} />} title="LLM Model" subtitle="Configure your language model provider" />
            <div style={{ marginTop: 20, display: "flex", flexDirection: "column", gap: 16 }}>
              <SelectField label="Provider" value={provider} onChange={setProvider} options={[
                { v: "deepseek", l: "DeepSeek", desc: "api.deepseek.com" },
                { v: "openai", l: "OpenAI", desc: "api.openai.com" },
                { v: "ollama", l: "Ollama", desc: "Local — localhost:11434" },
                { v: "custom", l: "Custom", desc: "Self-hosted endpoint" },
              ]} />
              <div>
                <Label text="API Key" />
                <div style={{ display: "flex", gap: 0, position: "relative" }}>
                  <input type={showKey ? "text" : "password"} value={apiKey} onChange={e => setApiKey(e.target.value)} placeholder="sk-..." className="field-input" style={{ paddingRight: 36 }} />
                  <button onClick={() => setShowKey(!showKey)} style={{ position: "absolute", right: 8, top: 9, background: "none", border: "none", color: "var(--color-muted-fg)", cursor: "pointer" }}>
                    {showKey ? <EyeOff size={14} /> : <Eye size={14} />}
                  </button>
                </div>
              </div>
              <TextField label="Model" value={model} onChange={setModel} placeholder="deepseek-v4-flash" />
              <TextField label="Base URL" value={baseUrl} onChange={setBaseUrl} placeholder="Auto-detected from provider" />
              <RangeField label="Temperature" value={temperature} onChange={setTemperature} min={0} max={2} step={0.1} />
            </div>
          </div>
        )}

        {section === "ocr" && (
          <div style={{ maxWidth: 480 }}>
            <SectionTitle icon={<Eye size={16} />} title="Liteparse OCR" subtitle="Document OCR for image-based PDFs" />
            <div style={{ marginTop: 20, display: "flex", flexDirection: "column", gap: 16 }}>
              <ToggleField label="OCR Enabled" value={ocrEnabled} onChange={setOcrEnabled} desc="Enable OCR for text-sparse pages and embedded images" />
              <SelectField label="OCR Engine" value={ocrEngine} onChange={changeOcrEngine} options={[
                { v: "paddleocr-vl", l: "PaddleOCR-VL", desc: "Local spotting model" },
                { v: "paddleocr", l: "PaddleOCR PP-OCR", desc: "Local text boxes" },
                { v: "mineru", l: "MinerU", desc: "Local document parser boxes" },
                { v: "deepseek-ocr", l: "DeepSeek-OCR", desc: "Local grounding boxes" },
              ]} />
              <TextField label="Model Root" value={ocrModelRoot} onChange={setOcrModelRoot} placeholder="Leave empty for .wiki/models/ocr" />
              <SelectField label="OCR Model" value={ocrModel} onChange={setOcrModel} options={ocrModelOptions(ocrEngine, ocrModel)} />
              <SelectField label="Device" value={ocrDevice} onChange={setOcrDevice} options={[
                { v: "auto", l: "Auto", desc: "Detect automatically" },
                { v: "cpu", l: "CPU", desc: "Most compatible" },
                { v: "cuda", l: "CUDA", desc: "NVIDIA GPU" },
                { v: "mps", l: "MPS", desc: "Apple Silicon" },
              ]} />
              <ToggleField label="Auto Download" value={ocrAutoDownload} onChange={setOcrAutoDownload} desc="Create the local runtime and download OCR model weights when first used" />
              <TextField label="Advanced OCR Server URL" value={ocrUrl} onChange={setOcrUrl} placeholder="Optional liteparse-compatible /ocr endpoint" />
              <TextField label="OCR Language" value={ocrLang} onChange={setOcrLang} placeholder="chi_sim+eng" />
            </div>
          </div>
        )}

        {section === "compile" && (
          <div style={{ maxWidth: 480 }}>
            <SectionTitle icon={<Zap size={16} />} title="Compile" subtitle="Document compilation behaviour" />
            <div style={{ marginTop: 20, display: "flex", flexDirection: "column", gap: 16 }}>
              <ToggleField label="Strip Sensitive Data" value={stripSensitive} onChange={setStripSensitive} desc="Redact API keys, tokens, passwords and emails from source content before sending to LLM. Off by default." />
            </div>
          </div>
        )}

        {section === "query" && (
          <div style={{ maxWidth: 480 }}>
            <SectionTitle icon={<Search size={16} />} title="Query" subtitle="Search and retrieval configuration" />
            <div style={{ marginTop: 20, display: "flex", flexDirection: "column", gap: 16 }}>
              <RangeField label="Max Results" value={maxResults} onChange={setMaxResults} min={1} max={20} step={1} />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ── Components ──

function SectionTitle({ icon, title, subtitle }: { icon: React.ReactNode; title: string; subtitle: string }) {
  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
        <span style={{ color: "var(--color-accent)" }}>{icon}</span>
        <h2 style={{ fontSize: 16, fontWeight: 600, margin: 0, color: "var(--color-fg)" }}>{title}</h2>
      </div>
      <p style={{ fontSize: 12, color: "var(--color-muted-fg)", margin: 0 }}>{subtitle}</p>
    </div>
  );
}

function Label({ text }: { text: string }) {
  return <label style={{ display: "block", fontFamily: "var(--font-mono)", fontSize: 10, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--color-muted-fg)", marginBottom: 5, fontWeight: 500 }}>{text}</label>;
}

function TextField({ label, value, onChange, placeholder }: { label: string; value: string; onChange: (v: string) => void; placeholder?: string }) {
  return (
    <div>
      <Label text={label} />
      <input value={value} onChange={e => onChange(e.target.value)} placeholder={placeholder} className="field-input" spellCheck={false} />
    </div>
  );
}

function SelectField({ label, value, onChange, options }: { label: string; value: string; onChange: (v: string) => void; options: { v: string; l: string; desc: string }[] }) {
  return (
    <div>
      <Label text={label} />
      <select value={value} onChange={e => onChange(e.target.value)} className="field-input">
        {options.map(o => <option key={o.v} value={o.v}>{o.l} — {o.desc}</option>)}
      </select>
    </div>
  );
}

function RangeField({ label, value, onChange, min, max, step }: { label: string; value: number; onChange: (v: number) => void; min: number; max: number; step: number }) {
  return (
    <div>
      <Label text={label} />
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <input type="range" min={min} max={max} step={step} value={value}
          onChange={e => onChange(parseFloat(e.target.value))}
          style={{ flex: 1, accentColor: "var(--color-accent)" }} />
        <span style={{ fontFamily: "var(--font-mono)", fontSize: 13, minWidth: 30, textAlign: "right", color: "var(--color-accent)", fontWeight: 600 }}>{value}</span>
      </div>
    </div>
  );
}

function ToggleField({ label, value, onChange, desc }: { label: string; value: boolean; onChange: (v: boolean) => void; desc: string }) {
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "12px 16px", background: "var(--color-muted)", borderRadius: 8, border: "1px solid var(--color-border)" }}>
      <div>
        <div style={{ fontSize: 13, fontWeight: 500, color: "var(--color-fg)" }}>{label}</div>
        <div style={{ fontSize: 11, color: "var(--color-muted-fg)", marginTop: 2 }}>{desc}</div>
      </div>
      <div
        onClick={() => onChange(!value)}
        style={{
          width: 44, height: 24, borderRadius: 12, cursor: "pointer",
          background: value ? "var(--color-accent)" : "var(--color-muted-fg)",
          position: "relative", transition: "background 0.2s", flexShrink: 0,
        }}>
        <div style={{
          position: "absolute", top: 2, width: 20, height: 20, borderRadius: "50%",
          background: "var(--color-bg)", left: value ? 22 : 2, transition: "left 0.2s",
          boxShadow: "0 1px 3px rgba(0,0,0,0.3)",
        }} />
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><SettingsWindow /></React.StrictMode>);
