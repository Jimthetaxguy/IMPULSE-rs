// V8 — IMPULSE Retro Broadcast Brand Lockup
// Scanimate-era CRT identity: aperture-iris emblem (Aperture-style blades)
// with a bold rocket silhouette ascending through it, phosphor wordmark,
// all under grille + bloom. Plus phosphor palette + bloom-level breakdown.

const RetroBrand = () => {
  // Aperture iris: 8 angled blades around a ring, cycling hot phosphor hues
  const bladeColors = [
    "#ff8a1e", "#ff6a00", "#ffb01a", "#2fd6a8",
    "#2e7bff", "#5b63ff", "#2fd0ff", "#ff8a1e",
  ];
  const blades = bladeColors.map((c, i) => {
    const a = (i / 8) * Math.PI * 2;
    const cx = 130, cy = 130, r = 78;
    const x = cx + Math.cos(a) * r;
    const y = cy + Math.sin(a) * r;
    const rot = (a * 180 / Math.PI) + 90;
    return { c, x, y, rot, key: i };
  });

  // Bold rocket silhouette (SVG path) — chunky, survives bloom
  const Rocket = ({size = 92, color = "#5b63ff"}) => (
    <svg width={size} height={size * 1.55} viewBox="0 0 60 93" className="glow-blue">
      {/* body */}
      <path d="M30 2
               C40 14 44 30 44 48
               L44 64 L16 64 L16 48
               C16 30 20 14 30 2 Z"
            fill={color}/>
      {/* window */}
      <circle cx="30" cy="34" r="8" fill="#000"/>
      <circle cx="30" cy="34" r="5" fill="#2fd0ff"/>
      {/* fins */}
      <path d="M16 50 L4 70 L16 64 Z" fill="#ff6a00"/>
      <path d="M44 50 L56 70 L44 64 Z" fill="#ff6a00"/>
      {/* base */}
      <rect x="16" y="64" width="28" height="6" fill={color}/>
      {/* exhaust flames */}
      <path d="M20 70 L30 92 L40 70 Z" fill="#ffb01a"/>
      <path d="M24 70 L30 84 L36 70 Z" fill="#ff3b1f"/>
    </svg>
  );

  const Swatch = ({c, name, hex}) => (
    <div style={{display:"flex", flexDirection:"column", gap:6, alignItems:"center"}}>
      <div style={{
        width:56, height:56, background:c,
        boxShadow:`0 0 8px ${c}, 0 0 22px ${c}99`,
      }}/>
      <div style={{fontFamily:"var(--font-mono)", fontSize:10, color:"#8fb8c8"}}>{name}</div>
      <div style={{fontFamily:"var(--font-mono)", fontSize:10, color:"#5d8090"}}>{hex}</div>
    </div>
  );

  return (
    <div className="crt grille flicker" style={{width: 1000, height: 720, padding: 0}}>
      <div className="scan-sweep" style={{position:"absolute", inset:0}}>
        <div className="sweep"></div>
      </div>

      {/* HERO LOCKUP */}
      <div style={{
        position:"relative", zIndex:5,
        height: 470, display:"flex", flexDirection:"column",
        alignItems:"center", justifyContent:"center", gap: 4,
      }}>
        {/* Emblem: iris + rocket */}
        <div style={{position:"relative", width:260, height:260}}>
          <svg width="260" height="260" viewBox="0 0 260 260" style={{position:"absolute", inset:0}}>
            {blades.map(b => (
              <g key={b.key} className="glow-soft" transform={`translate(${b.x},${b.y}) rotate(${b.rot})`}>
                <rect x={-9} y={-30} width={18} height={52} rx={3} fill={b.c}
                      style={{filter:`drop-shadow(0 0 4px ${b.c})`}}/>
              </g>
            ))}
            {/* inner ring */}
            <circle cx="130" cy="130" r="46" fill="none" stroke="#ffb01a" strokeWidth="3"
                    style={{filter:"drop-shadow(0 0 6px #ff6a00)"}}/>
          </svg>
          {/* rocket through center */}
          <div style={{position:"absolute", inset:0, display:"grid", placeItems:"center"}}>
            <Rocket size={84}/>
          </div>
        </div>

        {/* Wordmark */}
        <div className="phos phos-amber" style={{
          fontFamily:"'Baloo 2', system-ui", fontWeight:800,
          fontSize: 92, lineHeight: 0.9, letterSpacing:"0.02em",
          marginTop: 6,
        }}>impulse</div>

        {/* Tagline */}
        <div className="phos phos-cyan" style={{
          fontFamily:"'Baloo 2', system-ui", fontWeight:700,
          fontSize: 18, letterSpacing:"0.42em", textTransform:"uppercase",
          marginTop: 2,
        }}>your ai remembers</div>
      </div>

      {/* Lower band: palette + bloom levels */}
      <div style={{
        position:"relative", zIndex:5,
        borderTop:"1px solid rgba(255,140,40,0.18)",
        margin:"0 36px", paddingTop: 22,
        display:"grid", gridTemplateColumns:"1.3fr 1fr", gap: 36,
      }}>
        <div>
          <div style={{fontFamily:"var(--font-mono)", fontSize:10, letterSpacing:"0.25em", color:"#5d8090", marginBottom:14}}>
            PHOSPHOR PALETTE
          </div>
          <div style={{display:"flex", gap:18, flexWrap:"wrap"}}>
            <Swatch c="#ffb01a" name="amber"  hex="#ffb01a"/>
            <Swatch c="#ff6a00" name="orange" hex="#ff6a00"/>
            <Swatch c="#ff3b1f" name="red"    hex="#ff3b1f"/>
            <Swatch c="#5b63ff" name="blue"   hex="#5b63ff"/>
            <Swatch c="#2fd0ff" name="cyan"   hex="#2fd0ff"/>
            <Swatch c="#2fd6a8" name="teal"   hex="#2fd6a8"/>
            <Swatch c="#b6f03c" name="lime"   hex="#b6f03c"/>
          </div>
        </div>
        <div>
          <div style={{fontFamily:"var(--font-mono)", fontSize:10, letterSpacing:"0.25em", color:"#5d8090", marginBottom:14}}>
            BLOOM · CORE → HALO
          </div>
          <div style={{display:"flex", flexDirection:"column", gap:14}}>
            <div className="phos phos-amber" style={{fontFamily:"'Baloo 2'", fontWeight:800, fontSize:28}}>
              IMPULSE <span style={{fontFamily:"var(--font-mono)", fontSize:11, fontWeight:400}} className="">amber · hero</span>
            </div>
            <div className="phos phos-blue" style={{fontFamily:"'Baloo 2'", fontWeight:800, fontSize:22}}>
              skyline <span style={{fontFamily:"var(--font-mono)", fontSize:11, fontWeight:400}}>blue · structure</span>
            </div>
            <div className="phos phos-cyan" style={{fontFamily:"'Baloo 2'", fontWeight:800, fontSize:18}}>
              status <span style={{fontFamily:"var(--font-mono)", fontSize:11, fontWeight:400}}>cyan · live data</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

window.RetroBrand = RetroBrand;
