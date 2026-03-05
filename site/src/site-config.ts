import type { SiteConfig } from '@mcptoolshop/site-theme';

export const config: SiteConfig = {
  title: 'Escape the Valley',
  description: 'Oregon Trail-style survival game with AI narration and optional XRPL ledger',
  logoBadge: 'EV',
  brandName: 'Escape the Valley',
  repoUrl: 'https://github.com/mcp-tool-shop-org/escape-the-valley',
  footerText: 'MIT Licensed — built by <a href="https://mcp-tool-shop.github.io/" style="color:var(--color-muted);text-decoration:underline">MCP Tool Shop</a>',

  hero: {
    badge: 'Terminal survival game',
    headline: 'Escape the Valley:',
    headlineAccent: 'Ledger Trail.',
    description: 'Lead a party through procedurally generated wilderness. Manage food, water, wagon condition, and morale. An AI Game Master narrates your journey. An optional XRPL ledger tracks your supplies on-chain.',
    primaryCta: { href: '#quickstart', label: 'Get started' },
    secondaryCta: { href: 'handbook/', label: 'Read the Handbook' },
    previews: [
      { label: 'Install', code: 'pip install -e ".[dev]"' },
      { label: 'Play', code: 'trail tui --seed 42' },
      { label: 'Resume', code: 'trail tui --continue' },
    ],
  },

  sections: [
    {
      kind: 'features',
      id: 'features',
      title: 'Features',
      subtitle: 'Everything that makes the trail worth walking.',
      features: [
        {
          title: 'AI Game Master',
          desc: 'Three GM profiles (Chronicler, Fireside, Lantern-Bearer) narrate your journey with distinct voices. Powered by Ollama — runs locally, zero API cost.',
        },
        {
          title: 'Deterministic Physics',
          desc: 'Seeded RNG, scarcity curves, and doctrine modifiers. Every run is reproducible. The engine is the rules-lawyer; the GM just tells the story.',
        },
        {
          title: 'Oregon Trail Mechanics',
          desc: 'Travel, rest, hunt, repair. Events with real choices. Escape valves for emergencies. Maintenance windows for the careful. Pace matters.',
        },
        {
          title: 'XRPL Ledger Backpack',
          desc: 'Optional on-chain inventory tracking on XRPL Testnet. Supply tokens, settlement receipts, and parcels. Proof that you survived — or tried.',
        },
        {
          title: 'Full-Screen TUI',
          desc: 'Rich terminal interface built with Textual. Camp actions, event choices, route forks, and a trail ledger at journey\'s end.',
        },
        {
          title: '316 Tests',
          desc: 'Comprehensive test suite covering physics, events, save/load, ledger, backpack, and death cause enrichment. Lint clean.',
        },
      ],
    },
    {
      kind: 'code-cards',
      id: 'quickstart',
      title: 'Quick Start',
      cards: [
        {
          title: 'Install',
          code: '# Install the game\npip install -e ".[dev]"\n\n# Optional: XRPL backpack\npip install -e ".[xrpl]"',
        },
        {
          title: 'Play',
          code: '# Launch the TUI (recommended)\ntrail tui --seed 42\n\n# With AI narration (requires Ollama)\ntrail tui --seed 42 --voice\n\n# Without AI (deterministic mode)\ntrail tui --seed 42 --gm-off',
        },
        {
          title: 'Commands',
          code: 'trail tui              # Full-screen TUI\ntrail tui --continue   # Resume saved game\ntrail status           # Party & supplies\ntrail journal -n 5     # Recent events\ntrail ledger enable    # Enable XRPL backpack\ntrail self-check       # Environment health',
        },
      ],
    },
    {
      kind: 'data-table',
      id: 'gm-profiles',
      title: 'GM Profiles',
      subtitle: 'The narrator shapes the tone, not the mechanics. All three play the same game.',
      columns: ['Profile', 'Tone', 'Best For'],
      rows: [
        ['Chronicler', 'Grounded, practical, spare', 'Players who want facts'],
        ['Fireside', 'Serious campfire narrator (default)', 'First playthrough'],
        ['Lantern-Bearer', 'Uncanny and liminal', 'Experienced players'],
      ],
    },
  ],
};
