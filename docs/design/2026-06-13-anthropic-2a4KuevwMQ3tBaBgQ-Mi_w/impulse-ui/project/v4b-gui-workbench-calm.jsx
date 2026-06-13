// V4b — GUI Calm Workbench
// Reduces the previous 4-card-grid + signal-history wall into a single hero
// + 2 progressive-disclosure regions. Heavy use of negative space.

const GuiWorkbenchCalm = () => {
  const C = {
    bg: "#070d12", panel: "#0d1820", border: "#19262f",
    fg: "#d6f3ff", dim: "#8fb8c8", mute: "#5d8090",
    cyan: "#7be0ff", amber: "#ffce6b", green: "#86e8a8", magenta: "#e995d5"
  };

  return (
    <div className="gui-window" style={{width: 1280, height: 820, background:C.bg, fontFamily:"Inter, system-ui"}}>
      {/* Title bar */}
      <div className="gui-titlebar">
        <div style={{display:"flex", gap:6}}>
          <div className="tlight" style={{background:"#ff5f57"}}/>
          <div className="tlight" style={{background:"#ffbd2e"}}/>
          <div className="tlight" style={{background:"#28c941"}}/>
        </div>
        <div style={{flex:1, textAlign:"center"}}>
          <span style={{color:C.cyan}}>impulse</span>
          <span style={{color:C.mute}}>  ·  cli-cu-l8r</span>
        </div>
        <div style={{fontSize:11, color:C.mute}}>
          <span style={{color:C.green}}>●</span> daemon · 4ms
        </div>
      </div>

      <div style={{display:"grid", gridTemplateColumns:"58px 1fr", height: "calc(100% - 36px)"}}>
        {/* Slim left rail */}
        <div style={{borderRight:`1px solid ${C.border}`, padding:"14px 0", display:"flex", flexDirection:"column", alignItems:"center", gap:6}}>
          {[["▣","Home",true],["✦","Memory",false],["◇","Context",false],["▤","Agents",false],["⌂","Settings",false]].map((v,i)=>(
            <div key={i} style={{
              width:42, height:42, display:"grid", placeItems:"center",
              fontSize:18, color: v[2] ? C.cyan : C.mute,
              borderLeft: v[2] ? `2px solid ${C.cyan}` : "2px solid transparent",
              fontFamily:"var(--font-mono)"
            }}>{v[0]}</div>
          ))}
        </div>

        {/* Main */}
        <div style={{padding:"40px 56px", overflow:"hidden"}}>
          {/* Hero block — rocket + headline */}
          <div style={{display:"grid", gridTemplateColumns:"220px 1fr", gap:48, alignItems:"center", marginBottom:48}}>
            <window.RocketSprite scale={0.95}/>
            <div>
              <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.25em"}}>
                YOUR AI REMEMBERS · SILENTLY
              </div>
              <div style={{fontFamily:"Inter", fontSize:36, color:C.fg, marginTop:10, lineHeight:1.1, fontWeight:300, letterSpacing:"-0.01em"}}>
                Watching <span style={{color:C.cyan, fontWeight:500}}>cli-cu-l8r</span><br/>
                <span style={{color:C.mute}}>for 12 days, 7 hours.</span>
              </div>
              <div style={{marginTop:18, fontSize:14, color:C.dim, lineHeight:1.65, maxWidth:520}}>
                Quiet. Remembering 47.2k tokens across 4 agents.
                One injection awaits your review.
              </div>
            </div>
          </div>

          {/* The single most important action — one button */}
          <div style={{
            display:"flex", alignItems:"center", justifyContent:"space-between",
            background: C.panel, border:`1px solid ${C.amber}55`,
            padding:"18px 22px", marginBottom:32
          }}>
            <div>
              <div style={{fontSize:11, color:C.amber, fontFamily:"var(--font-mono)", letterSpacing:"0.2em"}}>1 PENDING REVIEW</div>
              <div style={{fontSize:15, color:C.fg, marginTop:6}}>
                Inject 4 decisions, 2 preferences, 7 sessions into next prompt
              </div>
              <div style={{fontSize:11, color:C.mute, marginTop:4, fontFamily:"var(--font-mono)"}}>
                8.3k tokens · review-first mode · prepared 4 minutes ago
              </div>
            </div>
            <div style={{display:"flex", gap:8}}>
              <button style={{background:"transparent", border:`1px solid ${C.border}`, color:C.dim, padding:"10px 18px", fontFamily:"var(--font-mono)", fontSize:12}}>SKIP</button>
              <button style={{background:"transparent", border:`1px solid ${C.border}`, color:C.dim, padding:"10px 18px", fontFamily:"var(--font-mono)", fontSize:12}}>DIFF</button>
              <button style={{background:C.amber, border:"none", color:"#1a1006", padding:"10px 22px", fontFamily:"var(--font-mono)", fontSize:12, fontWeight:700}}>REVIEW & APPLY</button>
            </div>
          </div>

          {/* Three numbers, calmly */}
          <div style={{display:"grid", gridTemplateColumns:"repeat(3, 1fr)", gap:0, borderTop:`1px solid ${C.border}`, borderBottom:`1px solid ${C.border}`}}>
            {[
              ["MEMORY",   "47.2k",  "tokens in genome",      C.cyan],
              ["AGENTS",   "4",      "online · 1 writing",    C.cyan],
              ["RETRIEVAL","2,341",  "chunks · p95 47ms",     C.cyan],
            ].map((s,i) => (
              <div key={s[0]} style={{
                padding:"24px 28px",
                borderRight: i < 2 ? `1px solid ${C.border}` : "none"
              }}>
                <div style={{fontSize:10, color:C.mute, letterSpacing:"0.25em", fontFamily:"var(--font-mono)"}}>{s[0]}</div>
                <div style={{fontSize:38, color:s[3], fontFamily:"var(--font-mono)", marginTop:8, lineHeight:1, fontWeight:300}}>{s[1]}</div>
                <div style={{fontSize:12, color:C.dim, marginTop:8}}>{s[2]}</div>
              </div>
            ))}
          </div>

          {/* "Show more" hint — drawer peeking */}
          <div style={{marginTop: 22, display:"flex", justifyContent:"space-between", fontSize:12, color:C.mute, fontFamily:"var(--font-mono)"}}>
            <span style={{cursor:"pointer"}}>▾ recent activity (8)</span>
            <span style={{cursor:"pointer"}}>▾ subsystems (7) · all green</span>
            <span style={{cursor:"pointer"}}>▾ signal history (12)</span>
          </div>
        </div>
      </div>
    </div>
  );
};

window.GuiWorkbenchCalm = GuiWorkbenchCalm;
