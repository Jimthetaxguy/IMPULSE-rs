// TUI v3 — Minimalist single-column flow (think: gh, charm, glow)
// Quiet, narrow, prose-first. The opposite end of the spectrum from cockpit.

const TuiMinimal = () => {
  const Row = ({k, v, color="cyan"}) => (
    <div style={{display:"flex", justifyContent:"space-between", fontSize:13, padding:"3px 0"}}>
      <span className="dim">{k}</span>
      <span className={color}>{v}</span>
    </div>
  );

  return (
    <div className="tui-frame" style={{padding:32, width:760, fontSize:14, background:"#080d10"}}>
      {/* Tiny brand */}
      <div style={{display:"flex", alignItems:"baseline", gap:12, marginBottom:24}}>
        <div style={{display:"flex", gap:2}}>
          {[1,2,3,4,5,6,7].map(i => (
            <div key={i} style={{
              width:6, height: i % 2 ? 18 : 12,
              background: i <= 4 ? "var(--cyan)" : "var(--blue)",
              opacity: 0.9
            }}/>
          ))}
        </div>
        <span style={{color:"var(--fg-0)", fontSize:18, letterSpacing:"0.3em"}}>impulse</span>
        <span className="dim" style={{fontSize:11}}>v0.9.4</span>
        <span style={{flex:1}}/>
        <span className="dim" style={{fontSize:11}}>your AI remembers · silently</span>
      </div>

      {/* Headline */}
      <div style={{fontSize:13, lineHeight:1.7, color:"var(--fg-1)", marginBottom:20}}>
        Watching <span className="cyan">cli-cu-l8r</span> for{" "}
        <span className="cyan">12d 7h</span>. Captured{" "}
        <span className="cyan">47,238 tokens</span> across{" "}
        <span className="cyan">23 sessions</span>. <span className="dim">Last injection 4 minutes ago.</span>
      </div>

      {/* Status block */}
      <div style={{borderTop:"1px solid var(--border)", borderBottom:"1px solid var(--border)", padding:"10px 0", marginBottom:18}}>
        <Row k="session"        v="a1b2c3d4 · claude-code"/>
        <Row k="memory core"    v="online" color="green"/>
        <Row k="context engine" v="active"/>
        <Row k="retrieval"      v="2,341 chunks · FTS5"/>
        <Row k="guardrail"      v="armed · 1 rule blocked today" color="amber"/>
      </div>

      {/* Recent activity */}
      <div className="dim" style={{fontSize:10, letterSpacing:"0.25em", marginBottom:10}}>RECENT</div>
      <div style={{fontSize:13, lineHeight:1.85, marginBottom:24}}>
        <div><span className="cyan">›</span> <span className="cyan">claude-code</span> split <span className="dim">daemon/mod.rs</span> into sub-handlers <span className="muted">· 4m</span></div>
        <div><span className="cyan">›</span> <span className="cyan">guardrail</span> blocked <span className="amber">git push --force</span> from codex <span className="muted">· 18m</span></div>
        <div><span className="cyan">›</span> <span className="cyan">retrieval</span> reindexed <span className="dim">2,341 chunks</span> in 6.2s <span className="muted">· 1h</span></div>
        <div><span className="cyan">›</span> <span className="cyan">handoff</span> claude-code → opencode <span className="dim">"continue debugging auth"</span> <span className="muted">· 3h</span></div>
        <div><span className="cyan">›</span> <span className="cyan">genome</span> learned 2 decisions, 1 preference <span className="muted">· yesterday</span></div>
      </div>

      {/* Pending */}
      <div className="dim" style={{fontSize:10, letterSpacing:"0.25em", marginBottom:10}}>PENDING</div>
      <div style={{fontSize:13, lineHeight:1.85, marginBottom:24}}>
        <div>
          <span className="amber">⏵</span>{" "}
          <span style={{color:"var(--fg-0)"}}>review injection bundle</span>{" "}
          <span className="dim">— 4 decisions · 2 preferences · 7 sessions</span>
        </div>
        <div className="dim" style={{fontSize:11, marginLeft:18, marginTop:2}}>
          <span className="cyan">a</span> apply&nbsp;&nbsp;<span className="cyan">d</span> diff&nbsp;&nbsp;<span className="cyan">s</span> skip&nbsp;&nbsp;<span className="cyan">e</span> edit
        </div>
        <div style={{marginTop:10}}>
          <span className="amber">⏵</span>{" "}
          <span style={{color:"var(--fg-0)"}}>steward proposal</span>{" "}
          <span className="dim">— compact 12k tokens of resolved threads</span>
        </div>
      </div>

      {/* Prompt */}
      <div style={{borderTop:"1px solid var(--border)", paddingTop:16, fontSize:13}}>
        <span className="cyan">impulse</span>{" "}
        <span style={{color:"var(--fg-0)"}}>›</span>{" "}
        <span className="dim">tell me about the daemon refactor</span>
        <span className="blink" style={{marginLeft:2, background:"var(--cyan)", display:"inline-block", width:7, height:14, verticalAlign:"middle"}}/>
      </div>
      <div className="dim" style={{fontSize:11, marginTop:14, display:"flex", gap:18}}>
        <span><span className="cyan">⏎</span> ask</span>
        <span><span className="cyan">^k</span> search</span>
        <span><span className="cyan">^r</span> review</span>
        <span><span className="cyan">^h</span> handoff</span>
        <span><span className="cyan">?</span> help</span>
      </div>
    </div>
  );
};

window.TuiMinimal = TuiMinimal;
