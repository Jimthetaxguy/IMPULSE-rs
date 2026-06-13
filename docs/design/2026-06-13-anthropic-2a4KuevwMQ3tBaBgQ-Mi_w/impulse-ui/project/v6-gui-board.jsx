// GUI v3 — Agent Orchestration Board
// A kanban / dispatch panel: queue → in-flight → review → done.
// Operator can drag tasks between agents; supervisor watches.

const GuiAgentBoard = () => {
  const C = {
    bg: "#0a1117", panel: "#0f1820", panel2:"#11202b", border: "#1c2a35",
    fg: "#d6f3ff", dim: "#8fb8c8", mute: "#5d8090",
    cyan: "#7be0ff", amber: "#ffce6b", green: "#86e8a8", magenta: "#e995d5", blue:"#7aa6ff"
  };

  const Card = ({tag, title, agent, agentColor, meta, body, accent}) => (
    <div style={{
      background:C.panel, border:`1px solid ${C.border}`,
      borderLeft:`3px solid ${accent}`, padding:"10px 12px",
      fontFamily:"var(--font-ui)"
    }}>
      <div style={{display:"flex", justifyContent:"space-between", fontSize:10, color:C.mute, fontFamily:"var(--font-mono)", letterSpacing:"0.1em"}}>
        <span>{tag}</span>
        <span style={{color:agentColor}}>{agent}</span>
      </div>
      <div style={{fontSize:13, color:C.fg, marginTop:6, lineHeight:1.4}}>{title}</div>
      {body && <div style={{fontSize:11, color:C.dim, marginTop:6, fontFamily:"var(--font-mono)"}}>{body}</div>}
      {meta && (
        <div style={{display:"flex", gap:10, marginTop:8, fontSize:10, color:C.mute, fontFamily:"var(--font-mono)"}}>
          {meta.map((m,i) => <span key={i}>{m}</span>)}
        </div>
      )}
    </div>
  );

  const Col = ({label, count, color, children}) => (
    <div style={{background:"#08111740", border:`1px solid ${C.border}`, padding:12, display:"flex", flexDirection:"column", gap:10, minHeight:0}}>
      <div style={{display:"flex", justifyContent:"space-between", alignItems:"center"}}>
        <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:color, letterSpacing:"0.18em"}}>{label}</div>
        <div style={{fontSize:11, color:C.mute, fontFamily:"var(--font-mono)"}}>{count}</div>
      </div>
      <div style={{display:"flex", flexDirection:"column", gap:8}}>
        {children}
      </div>
    </div>
  );

  const AgentChip = ({name, status, color, load}) => (
    <div style={{
      background:C.panel, border:`1px solid ${C.border}`,
      padding:"8px 10px", display:"flex", flexDirection:"column", gap:4
    }}>
      <div style={{display:"flex", justifyContent:"space-between", alignItems:"center"}}>
        <span style={{fontFamily:"var(--font-mono)", fontSize:12, color:C.fg}}>
          <span style={{display:"inline-block", width:6, height:6, background:color, marginRight:8, verticalAlign:"middle"}}/>
          {name}
        </span>
        <span style={{fontSize:10, color, fontFamily:"var(--font-mono)"}}>{status}</span>
      </div>
      <div style={{height:4, background:"#06101a", position:"relative"}}>
        <div style={{position:"absolute", inset:0, width: load+"%", background: color}}/>
      </div>
      <div style={{display:"flex", justifyContent:"space-between", fontSize:10, color:C.mute, fontFamily:"var(--font-mono)"}}>
        <span>load {load}%</span><span>1 task</span>
      </div>
    </div>
  );

  return (
    <div className="gui-window" style={{width:1320, height:820}}>
      <div className="gui-titlebar">
        <div style={{display:"flex", gap:6}}>
          <div className="tlight" style={{background:"#ff5f57"}}/>
          <div className="tlight" style={{background:"#ffbd2e"}}/>
          <div className="tlight" style={{background:"#28c941"}}/>
        </div>
        <div style={{flex:1, textAlign:"center"}}>
          <span style={{color:C.cyan}}>impulse-gui</span>
          <span style={{color:C.mute}}>  ·  Orchestrate  ·  cli-cu-l8r</span>
        </div>
        <div style={{fontSize:11, color:C.mute}}>4 agents · 9 tasks</div>
      </div>

      <div style={{display:"grid", gridTemplateColumns:"260px 1fr", height:"calc(100% - 36px - 26px)"}}>
        {/* Agent rail */}
        <div style={{background:C.bg, borderRight:`1px solid ${C.border}`, padding:16, display:"flex", flexDirection:"column", gap:12}}>
          <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.2em"}}>AGENTS</div>
          <AgentChip name="claude-code" status="WRITING" color={C.cyan}    load={47}/>
          <AgentChip name="opencode"    status="IDLE"    color={C.mute}    load={4}/>
          <AgentChip name="codex"       status="REVIEW"  color={C.amber}   load={82}/>
          <AgentChip name="aider"       status="READY"   color={C.green}   load={19}/>

          <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.2em", marginTop:12}}>HANDOFFS</div>
          <div style={{background:C.panel, border:`1px solid ${C.border}`, padding:"10px 12px", fontSize:12, fontFamily:"var(--font-mono)"}}>
            <div style={{color:C.cyan}}>claude-code → opencode</div>
            <div style={{color:C.mute, fontSize:10, marginTop:4}}>continue debugging auth refresh</div>
            <div style={{color:C.dim, fontSize:10, marginTop:6}}>8.3k tokens · review-first</div>
            <div style={{display:"flex", gap:6, marginTop:8}}>
              <button style={{flex:1, background:C.cyan, border:"none", color:"#06121a", padding:"6px", fontSize:11, fontFamily:"var(--font-mono)", fontWeight:700}}>SEND</button>
              <button style={{flex:1, background:"transparent", border:`1px solid ${C.border}`, color:C.dim, padding:"6px", fontSize:11, fontFamily:"var(--font-mono)"}}>EDIT</button>
            </div>
          </div>

          <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.2em", marginTop:12}}>STEWARD</div>
          <div style={{background:C.panel, border:`1px solid ${C.amber}`, padding:"10px 12px", fontFamily:"var(--font-ui)"}}>
            <div style={{fontSize:12, color:C.amber, fontFamily:"var(--font-mono)"}}>1 PROPOSAL</div>
            <div style={{fontSize:12, color:C.fg, marginTop:6, lineHeight:1.45}}>
              Compact 12k tokens of resolved threads in <code style={{color:C.cyan}}>session a1b2</code>?
            </div>
            <div style={{display:"flex", gap:6, marginTop:10}}>
              <button style={{flex:1, background:"transparent", border:`1px solid ${C.amber}`, color:C.amber, padding:"6px", fontSize:11, fontFamily:"var(--font-mono)"}}>APPROVE</button>
              <button style={{flex:1, background:"transparent", border:`1px solid ${C.border}`, color:C.dim, padding:"6px", fontSize:11, fontFamily:"var(--font-mono)"}}>REJECT</button>
            </div>
          </div>
        </div>

        {/* Board */}
        <div style={{padding:16, display:"grid", gridTemplateColumns:"repeat(4, 1fr)", gap:12, overflow:"hidden"}}>
          <Col label="QUEUE" count={3} color={C.mute}>
            <Card tag="TASK-204" title="Add boundary validation to daemon" agent="unassigned" agentColor={C.mute} accent={C.mute}
              meta={["~2h","plan §5"]}/>
            <Card tag="TASK-205" title="Wire validate module before dispatch" agent="unassigned" agentColor={C.mute} accent={C.mute}
              meta={["~1h"]}/>
            <Card tag="TASK-206" title="Document IPC protocol v3 → v4 migration" agent="unassigned" agentColor={C.mute} accent={C.mute}
              meta={["docs", "low"]}/>
          </Col>

          <Col label="IN FLIGHT" count={2} color={C.cyan}>
            <Card tag="TASK-201" title="Split daemon process_request into sub-handlers" agent="claude-code" agentColor={C.cyan} accent={C.cyan}
              body="+84 −612 · 3 files"
              meta={["12.4k tok", "4m elapsed"]}/>
            <Card tag="TASK-203" title="Index retrieval rebuild (FTS5)" agent="aider" agentColor={C.green} accent={C.green}
              body="2,341 chunks · 47ms p95"
              meta={["5.0k tok", "running"]}/>
          </Col>

          <Col label="REVIEW" count={3} color={C.amber}>
            <Card tag="TASK-201" title="Daemon split — diff" agent="codex" agentColor={C.amber} accent={C.amber}
              body="risk · low · 1037 lines"
              meta={["accept · reject"]}/>
            <Card tag="TASK-198" title="Guardrail rule: --force pushes" agent="codex" agentColor={C.amber} accent={C.amber}
              body="affects 3 agents"
              meta={["1 conflict noted"]}/>
            <Card tag="TASK-197" title="Steward · compact resolved threads" agent="codex" agentColor={C.amber} accent={C.amber}
              body="-12k tokens"
              meta={["genome unchanged"]}/>
          </Col>

          <Col label="DONE" count={11} color={C.green}>
            <Card tag="TASK-196" title="Remove dead code from state/mod.rs" agent="claude-code" agentColor={C.cyan} accent={C.green}
              body="−35 lines"
              meta={["1h ago"]}/>
            <Card tag="TASK-195" title="Extract guardrail eval helper" agent="opencode" agentColor={C.cyan} accent={C.green}
              body="−15 lines"
              meta={["3h ago"]}/>
            <Card tag="TASK-194" title="Conflict audit trail (CONFLICTS.jsonl)" agent="claude-code" agentColor={C.cyan} accent={C.green}
              meta={["yesterday"]}/>
          </Col>
        </div>
      </div>

      <div style={{height:26, padding:"0 14px", display:"flex", alignItems:"center", justifyContent:"space-between", background:"#081116", borderTop:`1px solid ${C.border}`, fontFamily:"var(--font-mono)", fontSize:11, color:C.mute}}>
        <span><span style={{color:C.green}}>●</span> daemon · 5ms</span>
        <span>9 tasks · 4 agents · 1 handoff queued · 1 steward proposal</span>
      </div>
    </div>
  );
};

window.GuiAgentBoard = GuiAgentBoard;
