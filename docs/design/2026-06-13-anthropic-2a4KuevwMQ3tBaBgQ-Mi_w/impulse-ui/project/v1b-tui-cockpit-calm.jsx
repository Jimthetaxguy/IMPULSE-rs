// V1b — Calmer Cockpit with ASCII rocket + progressive disclosure
// Replaces the dense reference layout with a focal hero + 3 calm cards.
// Information hierarchy:
//   1. HERO  — rocket sprite + wordmark + one headline status line
//   2. THREE cards (Memory · Agents · Activity), each one number-first
//   3. Collapsible drawers for detail (shown peeking)

const TuiCockpitCalm = () => {
  const Card = ({title, big, sub, foot, accent="var(--cyan)", expanded}) => (
    <div className="panel-rel" style={{
      border: "1px solid var(--border)",
      padding: "16px 18px 14px",
      background: "rgba(15,24,32,0.55)",
      minHeight: 168,
      position:"relative"
    }}>
      <span className="panel-title" style={{color:"var(--fg-2)"}}>▸ {title}</span>
      <div style={{
        fontFamily:"var(--font-mono)",
        fontSize: 36, color: accent, lineHeight: 1, marginTop: 6,
        letterSpacing:"-0.01em"
      }}>{big}</div>
      <div style={{fontSize:12, color:"var(--fg-1)", marginTop: 8, lineHeight:1.5}}>{sub}</div>
      {foot && (
        <div style={{
          marginTop:14, paddingTop:10,
          borderTop:"1px dashed var(--border)",
          fontSize:11, color:"var(--fg-2)",
          fontFamily:"var(--font-mono)",
          display:"flex", justifyContent:"space-between"
        }}>{foot}</div>
      )}
      {expanded && (
        <div style={{marginTop:10}}>{expanded}</div>
      )}
    </div>
  );

  return (
    <div className="tui-frame scanlines" style={{padding: 22, width: 1100, background:"#070d12"}}>
      {/* Slim top status bar — 1 line, dimmed */}
      <div style={{
        display:"flex", justifyContent:"space-between",
        fontSize:11, color:"var(--fg-2)",
        fontFamily:"var(--font-mono)",
        letterSpacing:"0.12em",
        padding:"4px 2px 14px",
        borderBottom:"1px solid var(--border)",
        marginBottom: 22
      }}>
        <span><span className="cyan">IMPULSE</span> SUPERVISOR · v0.9.4</span>
        <span>cli-cu-l8r · session a1b2c3d4</span>
        <span>14:27:32 · uptime 12d 7h</span>
      </div>

      {/* Hero */}
      <div style={{display:"grid", gridTemplateColumns:"260px 1fr", gap:32, alignItems:"center", marginBottom:28}}>
        <div style={{display:"grid", placeItems:"center"}}>
          <window.RocketSprite scale={1.1}/>
        </div>
        <div>
          <div style={{
            fontFamily:"var(--font-pixel)",
            fontSize: 52, color:"var(--cyan)",
            letterSpacing:"0.12em", lineHeight:1,
            textShadow:"0 0 18px rgba(123,224,255,0.35)"
          }}>IMPULSE</div>
          <div style={{
            marginTop:14,
            fontSize:14, color:"var(--fg-1)",
            lineHeight:1.6, maxWidth:560
          }}>
            Watching <span className="cyan">cli-cu-l8r</span> across{" "}
            <span className="cyan">4 agents</span>. Genome holds{" "}
            <span className="cyan">47.2k tokens</span>. Last injection{" "}
            <span className="cyan">4 min ago</span>.
          </div>
          <div style={{
            marginTop:14, fontFamily:"var(--font-mono)",
            fontSize:12, color:"var(--fg-2)",
            display:"flex", gap:18
          }}>
            <span><span className="dot dot-green"/>&nbsp; all systems nominal</span>
            <span className="dim">·</span>
            <span className="dim">1 review pending</span>
          </div>
        </div>
      </div>

      {/* Three calm cards */}
      <div style={{display:"grid", gridTemplateColumns:"repeat(3, 1fr)", gap:14}}>
        <Card
          title="MEMORY"
          big="47.2k"
          sub={<>tokens in genome <span className="dim">· 23% of 200k</span></>}
          foot={<><span>essential 12.1k</span><span>critical 7.4k</span><span>minimal 3.9k</span></>}
        />
        <Card
          title="AGENTS"
          big="4"
          sub={<>online · 1 writing, 1 reviewing</>}
          accent="var(--cyan)"
          expanded={
            <div style={{display:"flex", flexDirection:"column", gap:4, marginTop:4, fontFamily:"var(--font-mono)", fontSize:12}}>
              {[
                ["claude-code","writing","cyan"],
                ["codex",      "review", "amber"],
                ["aider",      "ready",  "green"],
                ["opencode",   "idle",   "dim"],
              ].map(a => (
                <div key={a[0]} style={{display:"flex", justifyContent:"space-between"}}>
                  <span><span className={"dot dot-" + a[2]}/> <span style={{marginLeft:8}} className="cyan">{a[0]}</span></span>
                  <span className={a[2]}>{a[1]}</span>
                </div>
              ))}
            </div>
          }
        />
        <Card
          title="ACTIVITY"
          big="3"
          sub={<>events in the last hour</>}
          accent="var(--amber)"
          expanded={
            <div style={{fontFamily:"var(--font-mono)", fontSize:12, lineHeight:1.7}}>
              <div><span className="dim">4m</span>&nbsp; <span className="cyan">claude-code</span> split daemon/mod.rs</div>
              <div><span className="dim">18m</span> <span className="amber">guardrail</span> blocked --force push</div>
              <div><span className="dim">1h</span>&nbsp; <span className="cyan">retrieval</span> reindexed 2,341 chunks</div>
            </div>
          }
        />
      </div>

      {/* One peeking detail row — collapsible */}
      <div style={{
        marginTop:22,
        border:"1px solid var(--border)",
        background:"rgba(15,24,32,0.55)",
        padding:"12px 18px",
        position:"relative"
      }}>
        <div style={{
          display:"flex", justifyContent:"space-between", alignItems:"center",
          fontFamily:"var(--font-mono)", fontSize:12,
        }}>
          <div>
            <span className="amber">⏵</span>{" "}
            <span style={{color:"var(--fg-0)"}}>1 injection bundle awaiting review</span>{" "}
            <span className="dim">— 4 decisions, 2 prefs, 7 sessions · 8.3k tokens</span>
          </div>
          <div className="dim" style={{fontSize:11}}>
            <span className="cyan">a</span> apply&nbsp;&nbsp;
            <span className="cyan">d</span> diff&nbsp;&nbsp;
            <span className="cyan">s</span> skip
          </div>
        </div>
      </div>

      {/* Quiet prompt */}
      <div style={{
        marginTop:20, fontSize:13, fontFamily:"var(--font-mono)",
        display:"flex", alignItems:"center", gap:6
      }}>
        <span className="cyan">impulse</span>
        <span className="dim">›</span>
        <span className="dim" style={{fontStyle:"italic"}}>ask anything · ⏎ to send</span>
        <span className="blink" style={{
          background:"var(--cyan)", display:"inline-block",
          width:7, height:14, marginLeft:2
        }}/>
      </div>
    </div>
  );
};

window.TuiCockpitCalm = TuiCockpitCalm;
