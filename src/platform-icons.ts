const controller = '<path d="M7 7h10c2.7 0 4 3 4.5 5.7l.3 2.3c.3 1.9-1.7 3-3.1 1.9L15 14H9l-3.7 2.9c-1.4 1.1-3.4 0-3.1-1.9l.3-2.3C3 10 4.3 7 7 7Z"/><path d="M7 9v4m-2-2h4m7 0h.01m2-2h.01"/>';
const handheld = '<rect x="2" y="5" width="20" height="14" rx="3"/><path d="M9 8h6v8H9zm-4 2v4m-1-2h2m12-1h.01m2 2h.01"/>';
const discConsole = '<rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="12" cy="11" r="3"/><path d="M5 16h3m8 0h3"/>';
const cube = '<path d="M6 8 12 5l6 3v10l-6 3-6-3V8Zm0 0 6 3 6-3m-6 3v10M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2m0 9h.01m1.5-.8h.01"/>';
const arcade = '<path d="M7 3h10l1 9 2 3v6H4v-6l2-3 1-9Z"/><path d="M9 6h6v6H9zm-1 9h8m-6 3h4"/>';

const platformIcons = new Map([
  ["arcade", arcade],
  ["mame", arcade],
  ["fbneo", arcade],
  ["snes", '<rect x="3" y="6" width="18" height="12" rx="5"/><path d="M8 9v6m-3-3h6m4 0h.01m3-2h.01m0 4h.01"/><circle cx="15" cy="10" r=".35"/>'],
  ["gba", handheld],
  ["gb", '<rect x="6" y="2" width="12" height="20" rx="2"/><path d="M9 5h6v6H9zm1 9v4m-2-2h4m3 0h.01m1.5-2h.01"/>' ],
  ["gbc", '<rect x="6" y="2" width="12" height="20" rx="3"/><path d="M8.5 5h7v6h-7zm1.5 9v4m-2-2h4m3 0h.01m1.5-2h.01"/>' ],
  ["nds", '<rect x="4" y="3" width="16" height="18" rx="2"/><path d="M7 5h10v6H7zm2 9h6v4H9zM4 12h16"/>' ],
  ["3ds", '<rect x="3" y="3" width="18" height="18" rx="3"/><path d="M6 6h12v5H6zm3 8h6v4H9zM3 12.5h18"/>' ],
  ["psx", discConsole],
  ["ps2", '<rect x="2" y="7" width="20" height="10" rx="1.5"/><path d="M2 12h20m-16-2h8m-8 4.5h4m8-4.5h.01"/>' ],
  ["ps3", '<path d="M3 12c0-3 3-5 8-5h6a4 4 0 0 1 4 4v6H3v-5Zm0 2h18m-5-3h3"/>' ],
  ["psp", '<rect x="2" y="6" width="20" height="12" rx="5"/><path d="M8 8.5h8v7H8zM5 10.5v3m-1.5-1.5h3m12 0h.01m1.5-1h.01"/>' ],
  ["psvita", '<rect x="2" y="6" width="20" height="12" rx="5"/><path d="M7 8.5h10v7H7zM4.5 10h.01m15 0h.01"/><circle cx="5" cy="14" r="1"/><circle cx="19" cy="14" r="1"/>' ],
  ["gamecube", cube],
  ["gc", cube],
  ["wii", '<rect x="7" y="3" width="10" height="17" rx="1.5"/><path d="M5 21h14M13.5 8v7M10 6h.01"/>' ],
  ["wiiu", '<rect x="2" y="6" width="20" height="12" rx="3"/><path d="M7 8.5h10v7H7zM5 10v4m-1 0h2m13-3h.01m1 3h.01"/>' ],
  ["switch", '<path d="M8 3h2v18H8a5 5 0 0 1-5-5V8a5 5 0 0 1 5-5Zm6 0h2a5 5 0 0 1 5 5v8a5 5 0 0 1-5 5h-2V3Z"/><circle cx="7" cy="8" r="1"/><circle cx="17" cy="15" r="1"/>' ],
  ["dreamcast", discConsole],
  ["saturn", discConsole],
  ["segacd", discConsole],
  ["xbox", discConsole],
  ["xbox360", '<rect x="3" y="7" width="18" height="10" rx="3"/><path d="M6 10h6m-6 3h4"/><circle cx="17" cy="12" r="1.5"/>' ],
]);

export function platformIcon(name: string): string {
  const shape = platformIcons.get(name.toLowerCase()) ?? controller;
  return `<svg class="platform-icon" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${shape}</svg>`;
}
