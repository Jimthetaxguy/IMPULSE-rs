// GUI v1 — egui-style native workbench (Overview view)
// Reflects what impulse-gui actually is: titlebar, left rail of views,
// content area, status bar with daemon RTT.

const GuiWorkbench = () => {
  const C = {
    bg: "#0a1117", panel: "#0f1820", border: "#1c2a35",
    fg: "#d6f3ff", dim: "#8fb8c8", mute: "#5d8090",
    cyan: "#7be0ff", amber: "#ffce6b", green: "#86e8a8", magenta: "#e995d5"
  };

  const Card = ({title, children, style}) => (
    <div style={{background:C.panel, border:`1px solid ${C.border}`, padding:14, ...style}}>
      <div style={{fontSize:10, letterSpacing:"0.2em", color:C.mute, marginBottom:10, textTransform:"uppercase"}}>{title}</div>
      {children}
    </div>
  );

  const Big = ({n, label, color=C.cyan}) => (
    <div>
      <div style={{fontFamily:"var(--font-mono)", fontSize:28, color, lineHeight:1}}>{n}</div>
      <div style={{fontSize:11, color:C.dim, marginTop:6}}>{label}</div>
    </div>
  );

  const Sig = ({k, v, color}) => (
    <div style={{display:"flex", justifyContent:"space-between", padding:"7px 0", borderBottom:`1px solid ${C.border}`, fontSize:12}}>
      <span style={{color:C.dim}}>{k}</span>
      <span style={{color, fontFamily:"var(--font-mono)"}}>{v}</span>
    </div>
  );

  return (
    <div className="gui-window" style={{width: 1280, height: 820}}>
      {/* Title bar */}
      <div className="gui-titlebar">
        <div style={{display:"flex", gap:6}}>
          <div className="tlight" style={{background:"#ff5f57"}}/>
          <div className="tlight" style={{background:"#ffbd2e"}}/>
          <div className="tlight" style={{background:"#28c941"}}/>
        </div>
        <div style={{flex:1, textAlign:"center"}}>
          <span style={{color:C.cyan}}>impulse-gui</span>
          <span style={{color:C.mute}}>  ·  cli-cu-l8r  ·  session a1b2c3d4</span>
        </div>
        <div style={{display:"flex", gap:14, fontSize:11, color:C.mute}}>
          <span><span style={{color:C.green}}>●</span> daemon · 4ms</span>
          <span>proto v3</span>
        </div>
      </div>

      <div style={{display:"grid", gridTemplateColumns:"58px 1fr", height: "calc(100% - 36px - 26px)"}}>
        {/* Left rail */}
        <div style={{background:"#081116", borderRight:`1px solid ${C.border}`, padding:"12px 0", display:"flex", flexDirection:"column", alignItems:"center", gap:4}}>
          {[
            ["▣", "Overview", true],
            ["▤", "Terminals", false],
            ["◇", "Context", false],
            ["✦", "Memory", false],
            ["◫", "Artifacts", false],
            ["⌂", "Settings", false],
          ].map((v,i) => (
            <div key={i} title={v[1]} style={{
              width:42, height:42, display:"grid", placeItems:"center",
              fontSize:18, color: v[2] ? C.cyan : C.mute,
              borderLeft: v[2] ? `2px solid ${C.cyan}` : "2px solid transparent",
              background: v[2] ? "#0f1c25" : "transparent",
              fontFamily:"var(--font-mono)"
            }}>{v[0]}</div>
          ))}
          <div style={{flex:1}}/>
          <div style={{fontSize:9, color:C.mute, writingMode:"vertical-rl", transform:"rotate(180deg)", letterSpacing:"0.3em"}}>v0.9.4</div>
        </div>

        {/* Main */}
        <div style={{padding:24, overflow:"hidden", background:C.bg}}>
          {/* Header */}
          <div style={{display:"flex", justifyContent:"space-between", alignItems:"flex-end", marginBottom:18}}>
            <div>
              <div style={{fontSize:11, color:C.mute, letterSpacing:"0.2em"}}>OVERVIEW</div>
              <div style={{fontFamily:"var(--font-mono)", fontSize:22, color:C.fg, marginTop:4}}>
                Watching <span style={{color:C.cyan}}>cli-cu-l8r</span>{" "}
                <span style={{color:C.mute, fontSize:14}}>· 12d 7h uptime</span>
              </div>
            </div>
            <div style={{display:"flex", gap:8}}>
              <button style={{background:"transparent", border:`1px solid ${C.border}`, color:C.dim, padding:"6px 12px", fontFamily:"var(--font-mono)", fontSize:12}}>↻ refresh</button>
              <button style={{background:C.cyan, border:"none", color:"#06121a", padding:"6px 12px", fontFamily:"var(--font-mono)", fontSize:12, fontWeight:600}}>+ session</button>
            </div>
          </div>

          {/* Stat cards */}
          <div style={{display:"grid", gridTemplateColumns:"repeat(4, 1fr)", gap:12, marginBottom:16}}>
            <Card title="Memory">
              <Big n="47.2k" label="tokens in genome"/>
              <div className="meter" style={{marginTop:14}}>
                <span className="track"><i style={{width:"23%", background:C.cyan}}/></span>
              </div>
              <div style={{fontSize:11, color:C.mute, marginTop:6}}>23% of 200k window</div>
            </Card>
            <Card title="Agents">
              <Big n="4" label="online · 1 writing" color={C.cyan}/>
              <div style={{display:"flex", gap:4, marginTop:12}}>
                {["claude","opencode","codex","aider"].map((a,i) => (
                  <div key={a} style={{flex:1, height:24, background:C.panel, border:`1px solid ${C.border}`, fontSize:9, color:C.dim, display:"grid", placeItems:"center"}}>{a}</div>
                ))}
              </div>
            </Card>
            <Card title="Sessions">
              <Big n="23" label="total · 1 active" color={C.amber}/>
              <div style={{fontSize:11, color:C.dim, marginTop:14, lineHeight:1.6}}>
                today <span style={{color:C.fg}}>3</span>  ·  this week <span style={{color:C.fg}}>11</span>
              </div>
            </Card>
            <Card title="Retrieval">
              <Big n="2,341" label="chunks · FTS5" color={C.green}/>
              <div style={{fontSize:11, color:C.dim, marginTop:14}}>
                p95 <span style={{color:C.fg, fontFamily:"var(--font-mono)"}}>47ms</span>  ·  cold <span style={{color:C.fg, fontFamily:"var(--font-mono)"}}>112ms</span>
              </div>
            </Card>
          </div>

          {/* Two columns */}
          <div style={{display:"grid", gridTemplateColumns:"1.2fr 1fr", gap:12}}>
            <Card title="Signal History" style={{minHeight:340}}>
              {[
                ["14:27", "ContextThreshold", "claude-code at 78%", C.amber],
                ["14:25", "TaskCompleted",    "daemon split landed",  C.green],
                ["14:20", "FileConflict",     "src/state/mod.rs (claude/codex)", C.magenta],
                ["14:18", "CompactionDetected","resolved threads pruned", C.cyan],
                ["14:09", "ErrorEncountered", "codex · oauth refresh failed", "#ff8a8a"],
                ["13:52", "TaskCompleted",    "guardrail rule installed", C.green],
              ].map((r,i) => (
                <div key={i} style={{display:"grid", gridTemplateColumns:"56px 170px 1fr 8px", gap:10, padding:"8px 0", borderBottom:`1px solid ${C.border}`, fontSize:12, alignItems:"center"}}>
                  <span style={{color:C.mute, fontFamily:"var(--font-mono)"}}>{r[0]}</span>
                  <span style={{color:r[3], fontFamily:"var(--font-mono)", fontSize:11}}>{r[1]}</span>
                  <span style={{color:C.fg}}>{r[2]}</span>
                  <span style={{width:8, height:8, background:r[3]}}/>
                </div>
              ))}
            </Card>

            <Card title="Subsystems">
              {[
                ["Memory Core",   "online",   C.green],
                ["Context Engine","active",   C.cyan],
                ["Summarizer",    "active",   C.cyan],
                ["Watcher",       "active",   C.cyan],
                ["Orchestrator",  "active",   C.cyan],
                ["Retrieval/FTS5","indexing", C.amber],
                ["Guardrail",     "armed",    C.cyan],
                ["Steward",       "1 proposal", C.amber],
              ].map(s => <Sig key={s[0]} k={s[0]} v={`›  ${s[1]}`} color={s[2]}/>)}
            </Card>
          </div>
        </div>
      </div>

      {/* Status bar */}
      <div style={{
        height:26, padding:"0 14px", display:"flex", alignItems:"center", justifyContent:"space-between",
        background:"#081116", borderTop:`1px solid ${C.border}`,
        fontFamily:"var(--font-mono)", fontSize:11, color:C.mute
      }}>
        <span><span style={{color:C.green}}>●</span> daemon connected · RTT 4ms · proto v3</span>
        <span>signals 6 · errors 1 · ctx 47.2k/200k · steward 1</span>
      </div>
    </div>
  );
};

window.GuiWorkbench = GuiWorkbench;
