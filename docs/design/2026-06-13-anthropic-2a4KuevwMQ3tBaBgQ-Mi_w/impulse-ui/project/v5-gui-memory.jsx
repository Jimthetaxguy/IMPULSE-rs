// GUI v2 — Memory Stream / Context Inspector
// Concept: a "neural map" of the project genome, with injection diff preview.
// Heavy data viz, dual pane.

const GuiMemoryMap = () => {
  const C = {
    bg: "#0a1117", panel: "#0f1820", border: "#1c2a35",
    fg: "#d6f3ff", dim: "#8fb8c8", mute: "#5d8090",
    cyan: "#7be0ff", amber: "#ffce6b", green: "#86e8a8", magenta: "#e995d5", blue:"#7aa6ff"
  };

  // Build a simple force-graph-like layout deterministically
  const nodes = [
    {id:"daemon",   x: 220, y: 180, r: 22, c: C.cyan,   t: "decisions", n: 8},
    {id:"retrieval",x: 360, y: 110, r: 18, c: C.cyan,   t: "decisions", n: 5},
    {id:"injection",x: 480, y: 200, r: 24, c: C.amber,  t: "decisions", n: 11},
    {id:"guardrail",x: 380, y: 290, r: 14, c: C.green,  t: "rules",     n: 3},
    {id:"prefs",    x: 150, y: 290, r: 16, c: C.magenta,t: "preferences", n: 6},
    {id:"agents",   x: 580, y: 110, r: 18, c: C.blue,   t: "ops",       n: 7},
    {id:"steward",  x: 600, y: 300, r: 12, c: C.amber,  t: "proposals", n: 1},
    {id:"genome",   x: 320, y: 200, r: 30, c: C.cyan,   t: "ROOT",      n: null},
  ];
  const edges = [
    ["genome","daemon"],["genome","retrieval"],["genome","injection"],
    ["genome","guardrail"],["genome","prefs"],["genome","agents"],
    ["injection","steward"],["daemon","guardrail"],["agents","injection"],
    ["retrieval","injection"],
  ];
  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));

  return (
    <div className="gui-window" style={{width:1280, height:820}}>
      <div className="gui-titlebar">
        <div style={{display:"flex", gap:6}}>
          <div className="tlight" style={{background:"#ff5f57"}}/>
          <div className="tlight" style={{background:"#ffbd2e"}}/>
          <div className="tlight" style={{background:"#28c941"}}/>
        </div>
        <div style={{flex:1, textAlign:"center"}}>
          <span style={{color:C.cyan}}>impulse-gui</span>
          <span style={{color:C.mute}}>  ·  Memory  ·  project genome</span>
        </div>
        <div style={{fontSize:11, color:C.mute}}>47,238 tokens · 31 nodes · 47 edges</div>
      </div>

      <div style={{display:"grid", gridTemplateColumns:"1fr 380px", height:"calc(100% - 36px - 26px)"}}>
        {/* Graph canvas */}
        <div style={{position:"relative", background:`
          radial-gradient(circle at 30% 30%, rgba(123,224,255,0.05), transparent 50%),
          ${C.bg}`, overflow:"hidden"}}>
          {/* Grid */}
          <svg width="100%" height="100%" style={{position:"absolute", inset:0}}>
            <defs>
              <pattern id="g" width="32" height="32" patternUnits="userSpaceOnUse">
                <path d="M32 0H0V32" fill="none" stroke={C.border} strokeWidth="1"/>
              </pattern>
            </defs>
            <rect width="100%" height="100%" fill="url(#g)"/>

            {/* Edges */}
            {edges.map(([a,b], i) => {
              const A = byId[a], B = byId[b];
              return <line key={i} x1={A.x} y1={A.y} x2={B.x} y2={B.y} stroke={C.cyan} strokeWidth="1" strokeDasharray="3 4" opacity="0.45"/>;
            })}

            {/* Nodes */}
            {nodes.map(n => (
              <g key={n.id}>
                <circle cx={n.x} cy={n.y} r={n.r + 6} fill="none" stroke={n.c} strokeWidth="1" opacity="0.25"/>
                <circle cx={n.x} cy={n.y} r={n.r} fill={C.panel} stroke={n.c} strokeWidth="1.5"/>
                <text x={n.x} y={n.y + 4} textAnchor="middle" fontFamily="var(--font-mono)" fontSize="11" fill={n.c}>{n.id}</text>
                {n.n !== null && (
                  <text x={n.x} y={n.y + n.r + 14} textAnchor="middle" fontFamily="var(--font-mono)" fontSize="10" fill={C.mute}>{n.n} {n.t}</text>
                )}
              </g>
            ))}

            {/* Selected ring */}
            <circle cx={byId.injection.x} cy={byId.injection.y} r={byId.injection.r + 12} fill="none" stroke={C.amber} strokeWidth="1.5" strokeDasharray="2 3"/>
          </svg>

          {/* Floating legend / chips */}
          <div style={{position:"absolute", left:16, top:16, display:"flex", flexDirection:"column", gap:6, fontFamily:"var(--font-mono)", fontSize:11}}>
            {[
              ["decisions", C.cyan],
              ["preferences", C.magenta],
              ["constraints", C.amber],
              ["sessions", C.blue],
            ].map(l => (
              <div key={l[0]} style={{display:"flex", alignItems:"center", gap:8, color:C.dim}}>
                <span style={{width:8, height:8, background:l[1]}}/>{l[0]}
              </div>
            ))}
          </div>

          {/* Floating timeline at bottom */}
          <div style={{position:"absolute", left:16, right:16, bottom:16, background:C.panel, border:`1px solid ${C.border}`, padding:"10px 14px"}}>
            <div style={{fontSize:10, color:C.mute, letterSpacing:"0.2em", marginBottom:8}}>GENOME GROWTH · 30 DAYS</div>
            <div style={{display:"flex", alignItems:"flex-end", gap:2, height:36}}>
              {Array.from({length: 30}).map((_, i) => {
                const h = 4 + (Math.sin(i*1.3) + 1) * 14 + (i > 22 ? 8 : 0);
                return <div key={i} style={{flex:1, height: h, background: i > 22 ? C.cyan : C.dim, opacity: i > 22 ? 1 : 0.45}}/>;
              })}
            </div>
            <div style={{display:"flex", justifyContent:"space-between", marginTop:6, fontSize:10, color:C.mute}}>
              <span>Apr 4</span><span>+18 decisions this week</span><span>May 4</span>
            </div>
          </div>
        </div>

        {/* Right inspector */}
        <div style={{borderLeft:`1px solid ${C.border}`, background:C.panel, display:"flex", flexDirection:"column"}}>
          <div style={{padding:"16px 18px", borderBottom:`1px solid ${C.border}`}}>
            <div style={{fontSize:10, color:C.mute, letterSpacing:"0.2em"}}>SELECTED NODE</div>
            <div style={{fontFamily:"var(--font-mono)", fontSize:18, color:C.amber, marginTop:6}}>injection</div>
            <div style={{fontSize:11, color:C.dim, marginTop:4}}>11 decisions · last touched 4m ago</div>
          </div>

          <div style={{padding:"14px 18px", flex:1, overflow:"hidden"}}>
            <div style={{fontSize:10, color:C.mute, letterSpacing:"0.2em", marginBottom:10}}>PENDING INJECTION · REVIEW</div>

            {/* Diff-style preview */}
            <div style={{fontFamily:"var(--font-mono)", fontSize:12, lineHeight:1.65, background:"#06101a", border:`1px solid ${C.border}`, padding:12}}>
              <div style={{color:C.mute}}>+++ session prelude</div>
              <div style={{color:C.green}}>+ decision · use FTS5 over LIKE for retrieval</div>
              <div style={{color:C.green}}>+ decision · review-first injection mode default</div>
              <div style={{color:C.green}}>+ pref     · no rounded corners in TUI</div>
              <div style={{color:C.amber}}>~ session  · daemon refactor (claude-code, 4m)</div>
              <div style={{color:C.amber}}>~ session  · guardrail rule add (codex, 18m)</div>
              <div style={{color:"#ff8a8a"}}>- pref     · "verbose logs" (superseded)</div>
            </div>

            <div style={{marginTop:14}}>
              <div style={{fontSize:10, color:C.mute, letterSpacing:"0.2em", marginBottom:8}}>BUDGET</div>
              {[
                ["essential", 12.1, 28],
                ["critical",   7.4, 17],
                ["minimal",    3.9,  9],
              ].map(r => (
                <div key={r[0]} style={{display:"grid", gridTemplateColumns:"90px 1fr 56px", gap:8, alignItems:"center", padding:"4px 0"}}>
                  <span style={{fontSize:11, color:C.dim, fontFamily:"var(--font-mono)"}}>{r[0]}</span>
                  <span style={{height:6, background:"#06101a", border:`1px solid ${C.border}`, position:"relative"}}>
                    <span style={{position:"absolute", inset:0, width: r[2]+"%", background:C.cyan}}/>
                  </span>
                  <span style={{fontSize:11, color:C.fg, fontFamily:"var(--font-mono)", textAlign:"right"}}>{r[1]}k</span>
                </div>
              ))}
            </div>

            <div style={{marginTop:18, display:"flex", gap:8}}>
              <button style={{flex:1, background:C.cyan, border:"none", color:"#06121a", padding:"10px", fontFamily:"var(--font-mono)", fontSize:12, fontWeight:700}}>APPLY</button>
              <button style={{flex:1, background:"transparent", border:`1px solid ${C.border}`, color:C.dim, padding:"10px", fontFamily:"var(--font-mono)", fontSize:12}}>EDIT</button>
              <button style={{flex:1, background:"transparent", border:`1px solid ${C.border}`, color:C.dim, padding:"10px", fontFamily:"var(--font-mono)", fontSize:12}}>SKIP</button>
            </div>
          </div>
        </div>
      </div>

      <div style={{height:26, padding:"0 14px", display:"flex", alignItems:"center", justifyContent:"space-between", background:"#081116", borderTop:`1px solid ${C.border}`, fontFamily:"var(--font-mono)", fontSize:11, color:C.mute}}>
        <span><span style={{color:C.green}}>●</span> daemon · 6ms</span>
        <span>genome · 31 nodes · 47 edges · 1 pending injection</span>
      </div>
    </div>
  );
};

window.GuiMemoryMap = GuiMemoryMap;
