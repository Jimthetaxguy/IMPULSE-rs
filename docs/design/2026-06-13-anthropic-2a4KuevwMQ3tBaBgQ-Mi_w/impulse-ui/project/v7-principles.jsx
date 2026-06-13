// Design Principles — Impulse
// A reference card distilling the calm-vs-dense tension into reusable rules.

const Principles = () => {
  const C = {
    bg:"#070d12", panel:"#0d1820", border:"#19262f",
    fg:"#d6f3ff", dim:"#8fb8c8", mute:"#5d8090",
    cyan:"#7be0ff", amber:"#ffce6b", green:"#86e8a8", magenta:"#e995d5", red:"#ff8a8a"
  };

  const Principle = ({n, name, oneLiner, doList, dontList, accent=C.cyan}) => (
    <div style={{
      background: C.panel, border: `1px solid ${C.border}`, borderLeft: `3px solid ${accent}`,
      padding: "20px 24px", display:"flex", flexDirection:"column", gap:14
    }}>
      <div style={{display:"flex", alignItems:"baseline", gap:14}}>
        <span style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.2em"}}>P{String(n).padStart(2,"0")}</span>
        <span style={{fontFamily:"var(--font-mono)", fontSize:18, color:accent, letterSpacing:"0.04em"}}>{name}</span>
      </div>
      <div style={{fontSize:14, color:C.fg, lineHeight:1.55, fontFamily:"Inter, system-ui"}}>{oneLiner}</div>
      <div style={{display:"grid", gridTemplateColumns:"1fr 1fr", gap:14, fontFamily:"var(--font-mono)", fontSize:11.5, lineHeight:1.6}}>
        <div>
          <div style={{color:C.green, marginBottom:4}}>DO</div>
          {doList.map((d,i) => <div key={i} style={{color:C.dim}}>+ {d}</div>)}
        </div>
        <div>
          <div style={{color:C.red, marginBottom:4}}>DON'T</div>
          {dontList.map((d,i) => <div key={i} style={{color:C.mute}}>− {d}</div>)}
        </div>
      </div>
    </div>
  );

  const Tier = ({tier, label, color, when, examples}) => (
    <div style={{display:"grid", gridTemplateColumns:"110px 1fr 1fr", gap:18, padding:"14px 0", borderBottom:`1px dashed ${C.border}`, alignItems:"start"}}>
      <div>
        <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.2em"}}>{tier}</div>
        <div style={{fontFamily:"var(--font-mono)", fontSize:16, color, marginTop:4}}>{label}</div>
      </div>
      <div style={{fontSize:13, color:C.fg, lineHeight:1.55}}>{when}</div>
      <div style={{fontFamily:"var(--font-mono)", fontSize:11.5, color:C.dim, lineHeight:1.65}}>{examples}</div>
    </div>
  );

  const Token = ({k, v, swatch}) => (
    <div style={{display:"flex", alignItems:"center", gap:10, padding:"6px 0", borderBottom:`1px dashed ${C.border}`, fontFamily:"var(--font-mono)", fontSize:12}}>
      {swatch && <span style={{width:14, height:14, background:swatch, border:`1px solid ${C.border}`, flexShrink:0}}/>}
      <span style={{color:C.dim, width:160}}>{k}</span>
      <span style={{color:C.fg}}>{v}</span>
    </div>
  );

  return (
    <div style={{
      width: 1180, padding: "44px 56px",
      background: C.bg, color: C.fg,
      fontFamily: "Inter, system-ui",
    }}>
      {/* Header */}
      <div style={{display:"grid", gridTemplateColumns:"180px 1fr", gap:36, alignItems:"center", marginBottom:36}}>
        <window.RocketSprite scale={0.85}/>
        <div>
          <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.25em"}}>
            DESIGN PRINCIPLES · v0.1
          </div>
          <div style={{fontFamily:"var(--font-pixel)", fontSize:30, color:C.cyan, letterSpacing:"0.1em", marginTop:14, lineHeight:1}}>
            IMPULSE
          </div>
          <div style={{fontSize:16, color:C.dim, marginTop:14, lineHeight:1.55, maxWidth:680}}>
            A working set of rules, distilled from the six explorations, to keep the
            cockpit aesthetic without overwhelming the operator. Use this as a contract
            for future screens.
          </div>
        </div>
      </div>

      {/* The Tension */}
      <div style={{
        background: C.panel, border:`1px solid ${C.border}`,
        padding:"18px 22px", marginBottom: 36,
        display:"grid", gridTemplateColumns:"1fr 14px 1fr", gap:18
      }}>
        <div>
          <div style={{fontFamily:"var(--font-mono)", fontSize:10, color:C.mute, letterSpacing:"0.2em"}}>BRAND PROMISE</div>
          <div style={{fontSize:18, color:C.cyan, fontFamily:"var(--font-mono)", marginTop:6}}>Cockpit confidence.</div>
          <div style={{fontSize:13, color:C.dim, marginTop:6, lineHeight:1.5}}>
            Operators feel commanding control. HUD chrome, glyph density, status everywhere.
          </div>
        </div>
        <div style={{color:C.amber, fontSize:18, alignSelf:"center", textAlign:"center", fontFamily:"var(--font-mono)"}}>×</div>
        <div>
          <div style={{fontFamily:"var(--font-mono)", fontSize:10, color:C.mute, letterSpacing:"0.2em"}}>BRAND PROMISE</div>
          <div style={{fontSize:18, color:C.cyan, fontFamily:"var(--font-mono)", marginTop:6}}>Silent memory.</div>
          <div style={{fontSize:13, color:C.dim, marginTop:6, lineHeight:1.5}}>
            "Your AI remembers. Silently." Quiet, ambient, out-of-the-way unless you ask.
          </div>
        </div>
      </div>

      {/* Six Principles */}
      <div style={{display:"grid", gridTemplateColumns:"1fr 1fr", gap:14, marginBottom:42}}>
        <Principle n={1} name="ONE FOCAL POINT PER SCREEN" accent={C.cyan}
          oneLiner="Each screen earns one hero — the rocket, a single number, or one pending action. Every other element is supporting cast."
          doList={[
            "Use scale + color contrast to pick the hero",
            "Demote secondary panels to dim/mute palette",
            "Reserve --cyan saturation for the focal element",
          ]}
          dontList={[
            "Use 6 cyan panels of equal weight",
            "Stack 4 brand marks (logo + ship + wordmark + tagline)",
            "Repeat the same pattern at the same brightness",
          ]}/>

        <Principle n={2} name="PROGRESSIVE DISCLOSURE" accent={C.cyan}
          oneLiner="Show the headline. Tease the detail. Reveal the rest only when asked. Never dump everything at once."
          doList={[
            "Number-first cards; details under a 'show more' chevron",
            "Peeking drawers (▾ recent activity (8))",
            "Keyboard hints rendered tiny next to actions",
          ]}
          dontList={[
            "Render every JSONL field on the dashboard",
            "Show subsystem table + log feed + agent grid + memory stream simultaneously",
            "Treat the dashboard as a database admin view",
          ]}/>

        <Principle n={3} name="DENSITY BY MODE" accent={C.amber}
          oneLiner="The TUI has three modes — Calm, Operator, Diagnostic — and density follows mode, not surface."
          doList={[
            "Calm = home/idle (V1b cockpit)",
            "Operator = active multi-agent work (V2 tmux)",
            "Diagnostic = debugging (full V1 dense HUD)",
          ]}
          dontList={[
            "Force operator-density on a user who's just starting",
            "Have one fixed layout for all task states",
            "Hide diagnostic detail entirely — make it a key away",
          ]}/>

        <Principle n={4} name="QUIET BY DEFAULT, LOUD ON SIGNAL" accent={C.amber}
          oneLiner="Everything is dim/mute until a real signal arrives — then that one element gets the cyan/amber/red treatment."
          doList={[
            "Dim subsystem rows when 'all green'",
            "Reserve --amber border only for pending review",
            "Use --red sparingly — only for true blocks",
          ]}
          dontList={[
            "Color every label cyan because cyan is brand",
            "Animate things that aren't actively changing",
            "Surface every signal as a toast",
          ]}/>

        <Principle n={5} name="CHROME SERVES, NEVER SHOUTS" accent={C.green}
          oneLiner="Brackets, scanlines, ASCII frames — these are texture. They tile under content; they never compete with it."
          doList={[
            "Scanlines at <=4% opacity",
            "Bracket corners only on hero containers, not every card",
            "ASCII rocket as identity, not as filler",
          ]}
          dontList={[
            "Wrap every panel in HUD brackets",
            "Repeat the rocket sprite on multiple screens",
            "Stack scanlines + grid + glow + dashes simultaneously",
          ]}/>

        <Principle n={6} name="MONOSPACE FOR DATA, INTER FOR PROSE" accent={C.green}
          oneLiner="JetBrains Mono is for tokens, IDs, paths, numbers. Inter is for sentences. Press Start 2P is reserved for the wordmark. Three faces, three jobs."
          doList={[
            "Use mono for stats, file paths, tokens, time",
            "Use Inter for headlines and descriptions in GUI",
            "Use pixel face only on splash / brand moments",
          ]}
          dontList={[
            "Set body copy in pixel font",
            "Mix mono and sans inside the same sentence",
            "Set numbers in proportional fonts (kerning lies)",
          ]}/>
      </div>

      {/* Information Hierarchy — 4 tiers */}
      <div style={{marginBottom: 42}}>
        <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.25em", marginBottom:6}}>
          INFORMATION HIERARCHY
        </div>
        <div style={{fontFamily:"Inter", fontSize:22, color:C.fg, fontWeight:300, marginBottom:18}}>
          Four tiers — assign every element to exactly one.
        </div>
        <div style={{borderTop:`1px dashed ${C.border}`}}>
          <Tier tier="TIER 1" label="HERO" color={C.cyan}
            when="The single most important thing on the screen right now. One per view, max."
            examples="rocket sprite · pending review CTA · large memory total · active agent name"/>
          <Tier tier="TIER 2" label="SUMMARY" color={C.fg}
            when="Three to four supporting numbers or chips that frame the hero."
            examples="stat trio (memory · agents · retrieval) · status row · one-line activity"/>
          <Tier tier="TIER 3" label="PEEKING" color={C.dim}
            when="Detail teased behind a chevron, count, or drawer. Available, not demanded."
            examples="▾ recent activity (8) · ▾ subsystems · injection diff preview"/>
          <Tier tier="TIER 4" label="AMBIENT" color={C.mute}
            when="Chrome — title bar, status bar, scanlines, brackets. Never crosses into Tier 1–3."
            examples="daemon RTT · uptime · scanline overlay · keyboard hints"/>
        </div>
      </div>

      {/* Token table */}
      <div style={{display:"grid", gridTemplateColumns:"1fr 1fr", gap:32, marginBottom: 32}}>
        <div>
          <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.25em", marginBottom:14}}>
            COLOR TOKENS
          </div>
          <Token k="--bg-0 (void)"        v="#070d12" swatch="#070d12"/>
          <Token k="--bg-1 (panel)"       v="#0d1820" swatch="#0d1820"/>
          <Token k="--fg-0 (primary)"     v="#d6f3ff" swatch="#d6f3ff"/>
          <Token k="--fg-2 (label)"       v="#5d8090" swatch="#5d8090"/>
          <Token k="--cyan (hero)"        v="oklch(0.82 0.14 215)" swatch={C.cyan}/>
          <Token k="--amber (pending)"    v="oklch(0.84 0.13 78)"  swatch={C.amber}/>
          <Token k="--green (healthy)"    v="oklch(0.82 0.16 145)" swatch={C.green}/>
          <Token k="--red (blocked)"      v="oklch(0.72 0.18 25)"  swatch={C.red}/>
          <Token k="--magenta (signal)"   v="oklch(0.72 0.17 330)" swatch={C.magenta}/>
        </div>
        <div>
          <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.25em", marginBottom:14}}>
            TYPE & SPACE
          </div>
          <Token k="font · mono"  v="JetBrains Mono · data, code, time"/>
          <Token k="font · ui"    v="Inter · prose, headlines (GUI)"/>
          <Token k="font · brand" v="Press Start 2P · wordmark only"/>
          <Token k="hero numeric"   v="36–52px / weight 300–400"/>
          <Token k="body numeric"   v="13–14px / mono"/>
          <Token k="label"          v="10–11px / 0.20em / uppercase"/>
          <Token k="grid unit"      v="8px"/>
          <Token k="card padding"   v="20px / 22px"/>
          <Token k="screen margin"  v="40–56px (GUI) / 22–32px (TUI)"/>
        </div>
      </div>

      {/* How to apply */}
      <div style={{
        background: C.panel, border:`1px solid ${C.border}`,
        padding:"22px 26px"
      }}>
        <div style={{fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.25em", marginBottom:10}}>
          HOW TO APPLY · 4-STEP REVIEW
        </div>
        <ol style={{margin:0, paddingLeft:22, fontSize:14, lineHeight:1.7, color:C.fg}}>
          <li>Pick the <span style={{color:C.cyan}}>one hero</span> for the screen. If you can't, the screen has no purpose yet.</li>
          <li>Assign every other element to <span style={{color:C.cyan}}>Tier 2, 3, or 4</span>. Move ties down.</li>
          <li>Run the <span style={{color:C.cyan}}>color audit</span>: count cyan elements; if &gt;3 demote some to dim/mute.</li>
          <li>Choose the <span style={{color:C.cyan}}>density mode</span> (Calm / Operator / Diagnostic) and stick to it for that screen.</li>
        </ol>
      </div>

      <div style={{marginTop:24, fontFamily:"var(--font-mono)", fontSize:11, color:C.mute, letterSpacing:"0.15em", textAlign:"center"}}>
        ▼ ▼ ▼ &nbsp; YOUR AI REMEMBERS &nbsp; ▼ ▼ ▼
      </div>
    </div>
  );
};

window.Principles = Principles;
