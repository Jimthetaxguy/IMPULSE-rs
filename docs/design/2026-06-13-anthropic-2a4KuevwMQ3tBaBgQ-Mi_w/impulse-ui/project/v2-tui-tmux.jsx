// TUI v2 — Tmux-style multipane operator workbench (ratatui)
// Single terminal split into agent panes, sidebar, status line.
// Closer to what `cargo run -- run` actually renders.

const TuiTmux = () => {
  const Box = ({title, children, color = "var(--cyan)", style}) => (
    <div className="panel-rel" style={{border:"1px solid var(--border)", padding:"10px 12px 10px", ...style}}>
      <span className="panel-title" style={{color}}>{title}</span>
      {children}
    </div>
  );

  const FakeTerm = ({lines}) => (
    <pre style={{margin:0, fontFamily:"var(--font-mono)", fontSize:12, lineHeight:1.6, whiteSpace:"pre-wrap"}}>
      {lines.map((ln, i) => (
        <div key={i} dangerouslySetInnerHTML={{__html: ln}}/>
      ))}
    </pre>
  );

  return (
    <div className="tui-frame" style={{padding: 0, width: 1280, fontSize:13}}>
      {/* Top bar */}
      <div style={{display:"flex", justifyContent:"space-between", padding:"6px 12px", background:"var(--bg-1)", borderBottom:"1px solid var(--border)"}}>
        <div>
          <span className="cyan">[impulse]</span>{" "}
          <span className="dim">cli-cu-l8r</span>{" "}
          <span className="dim">·</span>{" "}
          <span className="cyan">main ↑2</span>
        </div>
        <div className="dim">
          ⌐ session a1b2c3d4 · 14:27:32 · ctx 47.2k/200k
        </div>
      </div>

      {/* Tab strip */}
      <div style={{display:"flex", gap:0, fontSize:12, background:"var(--bg-1)", borderBottom:"1px solid var(--border)"}}>
        {[
          ["1","Overview", true],
          ["2","Sessions", false],
          ["3","Memory", false],
          ["4","Context", false],
          ["5","Agents", false],
          ["6","Steward", false],
          ["7","Tools", false],
          ["8","Logs", false],
          ["9","Config", false],
        ].map(t => (
          <div key={t[0]} style={{
            padding:"6px 14px",
            borderRight:"1px solid var(--border)",
            background: t[2] ? "var(--bg-3)" : "transparent",
            color: t[2] ? "var(--cyan)" : "var(--fg-1)",
          }}>
            <span className="dim">{t[0]}</span> {t[1]}
          </div>
        ))}
        <div style={{flex:1, borderBottom:"none"}}/>
        <div style={{padding:"6px 14px", color:"var(--fg-2)"}}>Ctrl+B ?</div>
      </div>

      {/* Body — sidebar + 2x2 pane grid */}
      <div style={{display:"grid", gridTemplateColumns:"220px 1fr", gap:0}}>
        {/* Sidebar */}
        <div style={{borderRight:"1px solid var(--border)", padding:"12px"}}>
          <div className="dim" style={{fontSize:10, letterSpacing:"0.2em", marginBottom:8}}>AGENTS</div>
          {[
            ["claude-code","WRITING","cyan", true],
            ["opencode",   "IDLE",   "dim", false],
            ["codex",      "REVIEW", "amber", false],
            ["aider",      "READY",  "green", false],
          ].map(a => (
            <div key={a[0]} style={{
              padding:"6px 8px", marginBottom:2, fontSize:12,
              background: a[3] ? "var(--bg-3)" : "transparent",
              borderLeft: a[3] ? "2px solid var(--cyan)" : "2px solid transparent"
            }}>
              <div><span className={"dot dot-" + a[2]}/> <span className="cyan" style={{marginLeft:6}}>{a[0]}</span></div>
              <div className="dim" style={{fontSize:10, marginLeft:14}}>{a[1]}</div>
            </div>
          ))}

          <div className="dim" style={{fontSize:10, letterSpacing:"0.2em", margin:"18px 0 8px"}}>SHORTCUTS</div>
          <div style={{fontSize:11, lineHeight:1.9, color:"var(--fg-1)"}}>
            <div><span className="cyan">^B c</span> <span className="dim">new shell</span></div>
            <div><span className="cyan">^B C</span> <span className="dim">claude</span></div>
            <div><span className="cyan">^B X</span> <span className="dim">codex</span></div>
            <div><span className="cyan">^B O</span> <span className="dim">opencode</span></div>
            <div><span className="cyan">^B n/p</span> <span className="dim">cycle pane</span></div>
            <div><span className="cyan">^B [</span> <span className="dim">scroll</span></div>
            <div><span className="cyan">^B i</span> <span className="dim">chat input</span></div>
          </div>

          <div className="dim" style={{fontSize:10, letterSpacing:"0.2em", margin:"18px 0 8px"}}>CTX BUDGET</div>
          <div className="meter">
            <span className="dim">used</span>
            <span className="track"><i style={{width:"23%"}}/></span>
            <span className="cyan">23%</span>
          </div>
          <div className="dim" style={{fontSize:10, marginTop:4}}>47.2k / 200k tokens</div>
        </div>

        {/* 2x2 panes */}
        <div style={{display:"grid", gridTemplateColumns:"1fr 1fr", gridTemplateRows:"1fr 1fr", height: 540}}>
          {/* Pane 1 — claude-code (active) */}
          <div style={{borderRight:"1px solid var(--border)", borderBottom:"1px solid var(--border)", padding:"6px 10px", position:"relative"}}>
            <div style={{display:"flex", justifyContent:"space-between", color:"var(--cyan)", fontSize:11, marginBottom:6, borderBottom:"1px solid var(--border)", paddingBottom:4}}>
              <span>● claude-code · src/daemon/mod.rs</span>
              <span className="dim">▲ 1</span>
            </div>
            <FakeTerm lines={[
              `<span class="dim">$</span> <span class="cyan">claude</span> "split process_request into sub-handlers"`,
              `<span class="dim">───────────────────────────────────</span>`,
              `Reading <span class="amber">daemon/mod.rs</span> (815 lines)…`,
              `Plan: extract <span class="cyan">handle_session_request</span>,`,
              `      <span class="cyan">handle_tool_request</span>, <span class="cyan">handle_steward_request</span>`,
              ``,
              `<span class="green">✓</span> Edited <span class="amber">daemon/mod.rs</span>  +84 −612`,
              `<span class="green">✓</span> Edited <span class="amber">handlers/session.rs</span>  +198 −0`,
              `<span class="green">✓</span> Edited <span class="amber">handlers/tool.rs</span>  +143 −0`,
              ``,
              `<span class="cyan">tests</span>: 920 passing · <span class="dim">cargo clippy clean</span>`,
              `<span class="blink">▎</span>`,
            ]}/>
          </div>

          {/* Pane 2 — codex (review) */}
          <div style={{borderBottom:"1px solid var(--border)", padding:"6px 10px"}}>
            <div style={{display:"flex", justifyContent:"space-between", color:"var(--amber)", fontSize:11, marginBottom:6, borderBottom:"1px solid var(--border)", paddingBottom:4}}>
              <span>○ codex · review queue</span>
              <span className="dim">3 items</span>
            </div>
            <FakeTerm lines={[
              `<span class="amber">[1/3]</span> daemon split — risk: <span class="green">low</span>`,
              `       diff size 1,037 lines · 0 unsafe`,
              ``,
              `<span class="amber">[2/3]</span> guardrail rule: --force pushes`,
              `       affects: <span class="cyan">claude-code, opencode, aider</span>`,
              ``,
              `<span class="amber">[3/3]</span> retrieval reindex (FTS5)`,
              `       <span class="cyan">2,341</span> chunks · 47ms p95`,
              ``,
              `<span class="dim">[a]ccept  [r]eject  [d]iff  [n]ext</span>`,
            ]}/>
          </div>

          {/* Pane 3 — context inspector */}
          <div style={{borderRight:"1px solid var(--border)", padding:"6px 10px"}}>
            <div style={{display:"flex", justifyContent:"space-between", color:"var(--blue)", fontSize:11, marginBottom:6, borderBottom:"1px solid var(--border)", paddingBottom:4}}>
              <span>● context inspector</span>
              <span className="dim">essential · critical · minimal</span>
            </div>
            <FakeTerm lines={[
              `<span class="cyan">essential</span>   ████████░░░░░░░░  12.1k`,
              `<span class="cyan">critical</span>    █████░░░░░░░░░░░   7.4k`,
              `<span class="cyan">minimal</span>     ███░░░░░░░░░░░░░   3.9k`,
              ``,
              `<span class="dim">last injection · 14:27:09 · review-first</span>`,
              `  <span class="amber">▸</span> <span class="cyan">decisions</span>(4)  switching to FTS5`,
              `  <span class="amber">▸</span> <span class="cyan">prefs</span>(2)      no rounded corners`,
              `  <span class="amber">▸</span> <span class="cyan">sessions</span>(7)  daemon refactor thread`,
              ``,
              `<span class="dim">[r]eview  [a]pply  [s]kip</span>`,
            ]}/>
          </div>

          {/* Pane 4 — supervisor chat */}
          <div style={{padding:"6px 10px"}}>
            <div style={{display:"flex", justifyContent:"space-between", color:"var(--magenta)", fontSize:11, marginBottom:6, borderBottom:"1px solid var(--border)", paddingBottom:4}}>
              <span>● supervisor chat</span>
              <span className="dim">⏎ send · ^B i focus</span>
            </div>
            <FakeTerm lines={[
              `<span class="dim">you ›</span> what changed since last session?`,
              `<span class="magenta">imp ›</span> 3 deltas:`,
              `        · daemon process_request now sub-`,
              `          handlers (Plan §4)`,
              `        · guardrail blocks --force pushes`,
              `        · retrieval rebuilt against FTS5`,
              `<span class="dim">you ›</span> who touched daemon/mod.rs?`,
              `<span class="magenta">imp ›</span> claude-code · 14:24 · split into 6`,
              `        sub-handlers (cf. PLAN.md step 4)`,
              ``,
              `<span class="dim">›</span> <span class="blink">▎</span>`,
            ]}/>
          </div>
        </div>
      </div>

      {/* Status line */}
      <div style={{display:"flex", justifyContent:"space-between", padding:"4px 12px", background:"var(--cyan)", color:"var(--bg-0)", fontSize:11, fontWeight:600}}>
        <span>1·OVERVIEW  ·  4 panes  ·  claude-code WRITING</span>
        <span>ctx 23%  ·  retrieval 2,341  ·  guardrail ARMED  ·  ^B ?</span>
      </div>
    </div>
  );
};

window.TuiTmux = TuiTmux;
