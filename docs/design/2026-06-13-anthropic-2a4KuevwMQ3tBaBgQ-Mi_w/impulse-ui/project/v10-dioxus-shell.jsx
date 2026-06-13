// V10 — Dioxus desktop shell, retro-themed.
// This mirrors the EXACT DOM emitted by impulse-desktop/src/ui.rs
// (.impulse-shell > .top-bar / .workspace-grid / .event-strip), styled by the
// shipped dioxus-impl/impulse_crt.css. It's visual proof the stylesheet drops
// onto the real shell. Data shown matches the ProjectOpsSnapshot binding map.

const DioxusShellRetro = () => {
  const agents = [
    {id:"claude-code", label:"Claude Code", status:"working",   active:true},
    {id:"codex",       label:"Codex",       status:"blocked",   active:false},
    {id:"opencode",    label:"OpenCode",    status:"idle",      active:false},
    {id:"shell",       label:"Shell",       status:"completed", active:false},
  ];

  const Rocket = () => (
    <svg width="60" height="93" viewBox="0 0 60 93" className="glow-blue">
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

  const Iris = () => {
    const cols = ["#ff8a1e","#ff6a00","#ffb01a","#2fd6a8","#2e7bff","#5b63ff","#2fd0ff","#ff8a1e"];
    return (
      <svg width="170" height="170" viewBox="0 0 260 260" style={{position:"absolute", inset:0}}>
        {cols.map((c,i) => {
          const a = (i/8)*Math.PI*2, cx=130, cy=130, r=78;
          const x = cx+Math.cos(a)*r, y = cy+Math.sin(a)*r, rot=a*180/Math.PI+90;
          return (
            <g key={i} className="glow-soft" transform={`translate(${x},${y}) rotate(${rot})`}>
              <rect x="-9" y="-30" width="18" height="52" rx="3" fill={c}/>
            </g>
          );
        })}
        <circle cx="130" cy="130" r="46" fill="none" stroke="#ffb01a" strokeWidth="3" style={{filter:"drop-shadow(0 0 6px #ff6a00)"}}/>
      </svg>
    );
  };

  return (
    <main className="impulse-shell" style={{height: 820}}>
      <header className="top-bar">
        <div className="brand">
          <h1>impulse</h1>
          <span className="daemon-state" data-state="online">online · watching</span>
        </div>
        <nav className="command-surface">
          <button className="icon-button" title="Command palette">⌘K</button>
          <button className="icon-button" title="Review context">Review</button>
          <button className="icon-button" title="Settings">Settings</button>
        </nav>
      </header>

      <div className="workspace-grid">
        <aside className="left-rail" data-owner="dioxus">
          <h2>Views</h2>
          <button className="rail-item active">Terminal</button>
          <button className="rail-item">Memory</button>
          <button className="rail-item">Artifacts</button>
          <button className="rail-item">Supervisor</button>
          <section className="agent-pool" data-source="agent_snapshot">
            <h2>Agents · 1 online</h2>
            {agents.map(a => (
              <button key={a.id} className={a.active ? "rail-item active" : "rail-item"}>
                <span className={"dot status-" + a.status}></span>
                {a.label}
                <span style={{float:"right", color:"var(--c-label)", fontSize:10}}>
                  {a.status === "completed" ? "done" : a.status}
                </span>
              </button>
            ))}
          </section>
        </aside>

        <section className="terminal-stage" data-terminal-renderer="xterm.js">
          <div className="crt-hero">
            <div style={{position:"relative", width:170, height:170}}>
              <Iris/>
              <div style={{position:"absolute", inset:0, display:"grid", placeItems:"center"}}>
                <Rocket/>
              </div>
            </div>
            <div style={{textAlign:"left"}}>
              <div className="brand-wordmark">impulse</div>
              <div className="brand-tagline">your ai remembers</div>
            </div>
          </div>

          <div className="stat-row">
            <div className="stat"><div className="k">Memory</div><div className="v">47.2k</div><div className="s">tokens · 23% of 200.0k</div></div>
            <div className="stat"><div className="k">Agents</div><div className="v">1</div><div className="s">online · 1 working</div></div>
            <div className="stat"><div className="k">Retrieval</div><div className="v">fts5</div><div className="s">12 genome decisions</div></div>
          </div>

          <div className="pending-bar">
            <span className="label"><span className="mark">⏵</span>1 injection(s) awaiting review</span>
            <span className="keys"><b>[a]</b> apply  <b>[d]</b> diff  <b>[s]</b> skip</span>
          </div>

          <div className="terminal-tabs" data-owner="dioxus">
            {agents.slice(0,4).map(a => (
              <button key={a.id} className={a.active ? "terminal-tab active" : "terminal-tab"}>{a.label.toLowerCase()}</button>
            ))}
          </div>
          <div id="terminal-pane-primary" className="xterm-mount" data-xterm-mount="true" data-agent-id="shell">
            <pre style={{margin:0, fontFamily:"var(--font-mono)", fontSize:12.5, lineHeight:1.6, color:"#d6f3ff"}}>
{`impulse@supervisor:~$ claude "split process_request into sub-handlers"
`}<span style={{color:"#b6f03c"}}>✓</span>{` edited daemon/mod.rs  +84 −612
`}<span style={{color:"#b6f03c"}}>✓</span>{` tests: 920 passing · clippy clean
impulse@supervisor:~$ `}<span style={{background:"#2fd0ff", display:"inline-block", width:7, height:14, verticalAlign:"middle"}}></span>
            </pre>
          </div>
        </section>

        <aside className="right-inspector" data-owner="dioxus">
          <section className="inspector-section">
            <h2>Context · essential</h2>
            <p>47.2k / 200.0k tokens · 11 injections · 2 compactions</p>
          </section>
          <section className="inspector-section">
            <h2>Pending review</h2>
            <p>1 bundle(s) awaiting review-first apply</p>
          </section>
          <section className="inspector-section">
            <h2>Retrieval</h2>
            <p>keyword · fts5</p>
          </section>
        </aside>
      </div>

      <footer className="event-strip" data-owner="dioxus">
        <span>ops_update 14:27:32Z</span>
        <span>4 agents</span>
        <span>3 artifacts</span>
        <span>1 interventions</span>
      </footer>
    </main>
  );
};

window.DioxusShellRetro = DioxusShellRetro;
