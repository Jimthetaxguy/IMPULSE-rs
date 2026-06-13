// ASCII rocket sprite — hand-tuned, monospace-aligned.
// Rendered as a <pre> with per-line color shading.

const RocketSprite = ({scale = 1, color = "var(--cyan)", glow = true}) => {
  // 21 columns wide, designed to feel chunky like the reference pixel ship
  // but built from real ASCII glyphs. Symmetric.
  const lines = [
    "          ▲          ",
    "         ╱█╲         ",
    "        ╱███╲        ",
    "       ╱█████╲       ",
    "      ╱███▓███╲      ",
    "     ╱██▓░▒░▓██╲     ",
    "    ┌──┤█████├──┐    ",
    "   ╱█  │█▓░▓█│  █╲   ",
    "  ╱██  │█░◉░█│  ██╲  ",
    " ╱███  │█▓░▓█│  ███╲ ",
    "▕████══╪═════╪══████▏",
    "▕████  │█████│  ████▏",
    " ╲███  │▓███▓│  ███╱ ",
    "  ╲██  │░███░│  ██╱  ",
    "   ╲█  └──┬──┘  █╱   ",
    "    ╲────┤├────╱     ",
    "       ╱ ││ ╲        ",
    "      ╱  ╲╱  ╲       ",
    "     ▾▾▾ ▾▾ ▾▾▾      ",
    "     ░▒▓ ▓▒ ▓▒░      ",
    "      ░   ░   ░      ",
  ];

  // Lines fade slightly toward the exhaust
  return (
    <pre style={{
      margin: 0,
      fontFamily: "var(--font-mono)",
      fontSize: 14 * scale,
      lineHeight: 1.05,
      letterSpacing: 0,
      color,
      textShadow: glow ? `0 0 6px ${color}55, 0 0 14px ${color}22` : "none",
      whiteSpace: "pre",
    }}>
      {lines.map((ln, i) => {
        const t = i / (lines.length - 1);
        // Body cyan; exhaust shifts toward warm magenta-ish
        const c = i < 16 ? color
                : i < 18 ? "var(--blue)"
                : i < 20 ? "var(--magenta)"
                         : "var(--amber)";
        const op = i >= 18 ? 0.85 : 1;
        return <div key={i} style={{color:c, opacity: op}}>{ln}</div>;
      })}
    </pre>
  );
};

window.RocketSprite = RocketSprite;
