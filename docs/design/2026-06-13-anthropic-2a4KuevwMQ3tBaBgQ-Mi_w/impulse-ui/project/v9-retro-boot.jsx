// V9 — Retro CRT Boot / Home screen
// The Impulse home UI wearing the broadcast vibe: hero gets full phosphor bloom,
// but live data stays in calmer cyan so it remains legible (P04: loud on signal).
// Demonstrates the vibe applied to product UI, not just the logo.

const RetroBoot = () => {
  const Rocket = ({size = 56}) => (
    <svg width={size} height={size * 1.55} viewBox="0 0 60 93" className="glow-blue">
      <path d="M30 2 C40 14 44 30 44 48 L44 64 L16 64 L16 48 C16 30 20 14 30 2 Z" fill="#5b63ff"/>
      <circle cx="30" cy="34" r="8" fill="#000"/>
      <circle cx="30" cy="34" r="5" fill="#2fd0ff"/>
      <path d="M16 50 L4 70 L16 64 Z" fill="#ff6a00"/>
      <path d="M44 50 L56 70 L44 64 Z" fill="#ff6a00"/>
      <rect x="16" y="64" width="28" height="6" fill="#5b63ff"/>
      <path d="M20 70 L30 92 L40 70 Z" fill="#ffb01a"/>
      <path d="M24 70 L30 84 L36 70 Z" fill="#ff3b1f"/>
    </svg>
  );

  const bootLines = [
    ["OK", "memory core"],
    ["OK", "context engine"],
    ["OK", "retrieval · 2,341 chunks"],
    ["OK", "4 agents detected"],
    ["··", "guardrail armed"],
  ];

  return (
    <div className="crt grille flicker" style={{width: 1100, height: 720, fontFamily:"var(--font-mono)"}}>
      <div className="scan-sweep" style={{position:"absolute", inset:0}}>
        <div className="sweep"></div>
      </div>

      {/* top hairline status */}
      <div style={{
        position:"relative", zIndex:5,
        display:"flex", justifyContent:"space-between",
        padding:"14px 28px", fontSize:11, letterSpacing:"0.18em",
        color:"#6f5a3a",
      }}>
        <span className="phos phos-amber" style={{fontWeight:700, fontSize:11, letterSpacing:"0.2em"}}>IMPULSE SUPERVISOR</span>
        <span style={{color:"#5d8090"}}>cli-cu-l8r · v0.9.4 · 14:27:32</span>
      </div>

      {/* HERO */}
      <div style={{
        position:"relative", zIndex:5,
        display:"flex", alignItems:"center", justifyContent:"center", gap: 40,
        marginTop: 30, marginBottom: 14,
      }}>
        <Rocket size={72}/>
        <div style={{textAlign:"left"}}>
          <div className="phos phos-amber" style={{
            fontFamily:"'Baloo 2', system-ui", fontWeight:800,
            fontSize: 86, lineHeight: 0.85, letterSpacing:"0.02em",
          }}>impulse</div>
          <div className="phos phos-cyan" style={{
            fontFamily:"'Baloo 2'", fontWeight:700,
            fontSize: 14, letterSpacing:"0.4em", textTransform:"uppercase", marginTop:8,
          }}>online · watching · remembering</div>
        </div>
      </div>

      {/* boot checklist — center */}
      <div style={{
        position:"relative", zIndex:5,
        width: 420, margin:"0 auto", marginTop: 18,
        fontSize: 14, lineHeight: 2,
      }}>
        {bootLines.map((l, i) => (
          <div key={i} style={{display:"flex", justifyContent:"space-between"}}>
            <span style={{color:"#8fb8c8"}}>
              <span className={l[0]==="OK" ? "phos phos-lime" : "phos phos-amber"} style={{fontWeight:800, marginRight:14}}>
                [{l[0]}]
              </span>
              {l[1]}
            </span>
            <span className="phos phos-cyan" style={{fontWeight:700, fontSize:12, opacity: l[0]==="OK"?1:0.75}}>
              {l[0]==="OK" ? "ready" : "armed"}
            </span>
          </div>
        ))}
      </div>

      {/* bottom: 3 calm stats + one pending */}
      <div style={{
        position:"relative", zIndex:5,
        position:"absolute", left:36, right:36, bottom: 86,
        display:"grid", gridTemplateColumns:"repeat(3, 1fr)",
        borderTop:"1px solid rgba(120,200,255,0.18)",
        borderBottom:"1px solid rgba(120,200,255,0.18)",
      }}>
        {[
          ["MEMORY", "47.2k", "tokens"],
          ["AGENTS", "4", "online · 1 writing"],
          ["RETRIEVAL", "2,341", "chunks · 47ms"],
        ].map((s, i) => (
          <div key={i} style={{
            padding:"18px 22px",
            borderRight: i<2 ? "1px solid rgba(120,200,255,0.14)" : "none",
          }}>
            <div style={{fontSize:10, letterSpacing:"0.25em", color:"#5d8090"}}>{s[0]}</div>
            <div className="phos phos-cyan" style={{fontFamily:"'Baloo 2'", fontWeight:800, fontSize:34, marginTop:6, lineHeight:1}}>{s[1]}</div>
            <div style={{fontSize:11, color:"#8fb8c8", marginTop:6}}>{s[2]}</div>
          </div>
        ))}
      </div>

      {/* pending review bar — the one "loud" signal */}
      <div style={{
        position:"absolute", left:36, right:36, bottom: 24, zIndex:5,
        display:"flex", justifyContent:"space-between", alignItems:"center",
        border:"1px solid rgba(255,140,40,0.45)",
        padding:"12px 20px",
        boxShadow:"0 0 18px rgba(255,106,0,0.25) inset",
      }}>
        <span style={{fontSize:13, color:"#ffce8a"}}>
          <span className="phos phos-amber" style={{fontWeight:800, marginRight:10}}>⏵</span>
          1 injection awaiting review
          <span style={{color:"#8a7050", marginLeft:8}}>— 4 decisions · 2 prefs · 7 sessions</span>
        </span>
        <span style={{fontSize:12, color:"#8fb8c8"}}>
          <span className="phos phos-amber" style={{fontWeight:700}}>[a]</span> apply&nbsp;&nbsp;
          <span className="phos phos-amber" style={{fontWeight:700}}>[d]</span> diff&nbsp;&nbsp;
          <span className="phos phos-amber" style={{fontWeight:700}}>[s]</span> skip
        </span>
      </div>
    </div>
  );
};

window.RetroBoot = RetroBoot;
