// TUI exploration v1 — Cockpit Supervisor
// Inspired directly by the reference image: chunky pixel-grid mark,
// HUD chrome with corner brackets, agent network panel, log feed,
// memory stream sparkline column.

const TuiCockpit = () => {
  // Build the IMPULSE wordmark out of a 5xN pixel grid using ASCII
  const W = (rows) => rows.map(r => r.padEnd(5, " ")).join("\n");
  const I = W(["XXXXX","  X  ","  X  ","  X  ","XXXXX"]);
  const M = W(["X   X","XX XX","X X X","X   X","X   X"]);
  const P = W(["XXXX ","X   X","XXXX ","X    ","X    "]);
  const U = W(["X   X","X   X","X   X","X   X"," XXX "]);
  const L = W(["X    ","X    ","X    ","X    ","XXXXX"]);
  const S = W([" XXXX","X    "," XXX ","    X","XXXX "]);
  const E = W(["XXXXX","X    ","XXX  ","X    ","XXXXX"]);

  // Render a single 5x5 char by mapping X→filled glyph, space→space
  const Char = ({ data, color = "var(--cyan)" }) => {
    const lines = data.split("\n");
    return (
      <div style={{display:"grid", gridTemplateRows:`repeat(5, 12px)`, gap:1}}>
        {lines.map((line, ri) => (
          <div key={ri} style={{display:"grid", gridTemplateColumns:`repeat(5, 12px)`, gap:1}}>
            {[...line].map((ch, ci) => (
              <div key={ci} style={{
                width:12, height:12,
                background: ch === "X" ? color : "transparent",
                opacity: ch === "X" ? (Math.random() > 0.85 ? 0.55 : 1) : 1,
              }}/>
            ))}
          </div>
        ))}
      </div>
    );
  };

  // Memory stream — vertical sparkline columns
  const memCol = (seed) => {
    const rows = 28;
    const out = [];
    for (let i = 0; i < rows; i++) {
      const v = ((seed * (i+3)) ^ (i*7)) % 5;
      const cls = v === 0 ? "transparent"
                : v === 1 ? "rgba(120,220,255,0.25)"
                : v === 2 ? "rgba(120,220,255,0.55)"
                : v === 3 ? "var(--cyan)"
                          : "var(--blue)";
      out.push(cls);
    }
    return out;
  };

  return (
    <div className="tui-frame scanlines" style={{padding: 16, width: 1100}}>
      {/* Top status bar */}
      <div className="hud-frame" style={{
        display:"grid", gridTemplateColumns:"1fr 1fr 1fr auto",
        padding:"6px 14px", marginBottom:14, fontSize:12, letterSpacing:"0.08em"
      }}>
        <span className="bl"></span><span className="br"></span>
        <span><span className="cyan">IMPULSE SUPERVISOR</span> <span className="dim">v0.9.4</span></span>
        <span style={{justifySelf:"center"}}><span className="dim">MEMORY:</span> <span className="cyan">47.2k TOKENS</span></span>
        <span style={{justifySelf:"center"}}><span className="dim">AGENTS:</span> <span className="cyan">4 ONLINE</span></span>
        <span style={{display:"flex", gap:4}}>
          <span className="dot"/><span className="dot"/><span className="dot"/><span className="dot dot-dim"/>
        </span>
      </div>

      {/* Main grid */}
      <div style={{display:"grid", gridTemplateColumns:"230px 1fr 130px", gap:14}}>
        {/* Left status block */}
        <div>
          <div style={{fontSize:12, lineHeight:1.9}}>
            <div><span className="dim">SYSTEM:</span> <span className="cyan">ONLINE</span></div>
            <div><span className="dim">MODE:</span> <span className="cyan">SUPERVISOR</span></div>
            <div><span className="dim">STATUS:</span> <span className="cyan">ACTIVE</span></div>
            <div><span className="dim">UPTIME:</span> <span className="cyan">12d 07:42:19</span></div>
            <div><span className="dim">PROJECT:</span> <span className="cyan">cli-cu-l8r</span></div>
            <div><span className="dim">SESSION:</span> <span className="cyan">a1b2c3d4</span></div>
          </div>
        </div>

        {/* Center stage — wordmark + small craft glyph */}
        <div style={{position:"relative", minHeight: 360, display:"grid", placeItems:"center"}}>
          {/* Tiny iconographic placeholder where the reference had a ship */}
          <div style={{position:"absolute", top:8, left:"50%", transform:"translateX(-50%)", color:"var(--fg-2)", fontSize:11, letterSpacing:"0.2em"}}>
            ┌─── PROJECT GENOME ───┐
          </div>
          <div style={{position:"absolute", top:34, color:"var(--cyan)", fontSize:10, opacity:0.6}}>
            <pre className="ascii" style={{textAlign:"center"}}>{`        .  .  .
       /█\\/█\\/█\\
      ▟███████████▙
       ▜█▘ ◢◣ ▝█▛
        ▔▔│▼▼│▔▔
          ╲▲╱
           ▼`}</pre>
          </div>

          {/* Pixel wordmark */}
          <div style={{display:"flex", gap:6, alignItems:"flex-end", marginTop:120}}>
            <Char data={I}/><Char data={M}/><Char data={P}/><Char data={U}/><Char data={L}/><Char data={S}/><Char data={E}/>
          </div>
          <div style={{position:"absolute", bottom: -2, left:"50%", transform:"translateX(-50%)", color:"var(--fg-2)", fontSize:10, letterSpacing:"0.3em"}}>
            ▼ ▼ ▼  YOUR AI REMEMBERS  ▼ ▼ ▼
          </div>
        </div>

        {/* Right — memory stream */}
        <div className="panel-rel" style={{border:"1px solid var(--border)", padding:10}}>
          <span className="panel-title">▸ MEMORY STREAM</span>
          <div style={{display:"flex", gap:3, justifyContent:"space-between", paddingTop:6}}>
            {[3,7,11,17,23,29].map((seed, i) => (
              <div key={i} style={{display:"grid", gridTemplateRows:`repeat(28, 6px)`, gap:1}}>
                {memCol(seed).map((c, j) => (
                  <div key={j} style={{width:6, height:6, background:c}}/>
                ))}
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Agent network */}
      <div className="panel-rel" style={{border:"1px solid var(--border)", padding:"14px 14px 10px", marginTop:18}}>
        <span className="panel-title">▸ AGENT NETWORK</span>
        <div style={{display:"grid", gridTemplateColumns:"repeat(4, 1fr)", gap:14, fontSize:12}}>
          {[
            {name:"claude-code",   sess:"sess-a1b2", tok:"12.4k", st:"WRITING", cls:"cyan"},
            {name:"opencode",      sess:"sess-c3d4", tok:" 8.1k", st:"IDLE",    cls:"dim"},
            {name:"codex",         sess:"sess-e5f6", tok:"21.7k", st:"REVIEW",  cls:"amber"},
            {name:"aider",         sess:"sess-g7h8", tok:" 5.0k", st:"READY",   cls:"green"},
          ].map(a => (
            <div key={a.name}>
              <div><span className="cyan">▸ {a.name}</span> <span className={a.cls === "dim" ? "dot dot-dim" : a.cls === "amber" ? "dot dot-amber" : a.cls === "green" ? "dot dot-green" : "dot blink"}/></div>
              <div className="dim" style={{fontSize:11, marginTop:4}}>{a.sess}</div>
              <div className="dim" style={{fontSize:11}}>tokens {a.tok}</div>
              <div className={a.cls} style={{fontSize:11, marginTop:2}}>{a.st}</div>
              <div className="bar-track" style={{marginTop:6}}>
                <i style={{width: a.name === "codex" ? "82%" : a.name === "claude-code" ? "47%" : a.name === "opencode" ? "31%" : "19%"}}/>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* Bottom split — log + system meters */}
      <div style={{display:"grid", gridTemplateColumns:"1.5fr 1fr", gap:14, marginTop:14}}>
        <div className="panel-rel" style={{border:"1px solid var(--border)", padding:"10px 12px"}}>
          <span className="panel-title">▸ EVENT LOG</span>
          <div style={{fontSize:12, lineHeight:1.7}}>
            {[
              ["14:23:11","SYS","Supervisor online. All systems nominal.",      "cyan"],
              ["14:23:12","MEM","Project genome loaded. 47,238 tokens.",         "cyan"],
              ["14:23:13","AGT","4 agents detected and connected.",              "cyan"],
              ["14:23:14","CTX","Context injection engine armed.",               "cyan"],
              ["14:23:15","IMP","Impulse engaged. Watching. Remembering.",        "cyan"],
              ["14:24:02","RTR","Retrieval index built · 2,341 chunks · FTS5",   "dim"],
              ["14:25:47","GRD","Guardrail: blocked 'git push --force' (codex)", "amber"],
              ["14:27:09","HND","Handoff prepared: claude-code → opencode",       "blue"],
            ].map((r,i) => (
              <div key={i}>
                <span className="dim">{r[0]}</span>{" "}
                <span className="cyan">[{r[1]}]</span>{" "}
                <span className={r[3]}>{r[2]}</span>
              </div>
            ))}
            <div><span className="dim">14:27:14</span> <span className="cyan">[IMP]</span> <span className="cyan">_</span><span className="blink">█</span></div>
          </div>
        </div>

        <div className="panel-rel" style={{border:"1px solid var(--border)", padding:"10px 14px"}}>
          <span className="panel-title">▸ SUBSYSTEMS</span>
          <div style={{fontSize:12, lineHeight:1.95}}>
            {[
              ["MEMORY CORE","ONLINE","green"],
              ["CONTEXT ENGINE","ACTIVE","cyan"],
              ["SUMMARIZER","ACTIVE","cyan"],
              ["WATCHER","ACTIVE","cyan"],
              ["ORCHESTRATOR","ACTIVE","cyan"],
              ["RETRIEVAL/FTS5","INDEXING","amber"],
              ["GUARDRAIL","ARMED","cyan"],
            ].map((r,i) => (
              <div key={i} style={{display:"flex", justifyContent:"space-between"}}>
                <span className="dim">{r[0]}</span>
                <span><span className={"dot dot-" + (r[2]==="cyan"?"":r[2])} style={{marginRight:6}}/><span className={r[2]}>›  {r[1]}</span></span>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Prompt */}
      <div style={{marginTop:14, fontSize:13}}>
        <span className="cyan">impulse</span><span className="dim">@</span><span className="cyan">supervisor</span>
        <span className="dim">:~$</span> <span className="blink" style={{background:"var(--cyan)", display:"inline-block", width:8, height:14, verticalAlign:"middle"}}/>
      </div>
    </div>
  );
};

window.TuiCockpit = TuiCockpit;
