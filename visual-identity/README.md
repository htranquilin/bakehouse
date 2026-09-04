# Bakehouse — visual identity

A SQL Server workbench for macOS that brings its own instances.

Open `brand.html` in a browser to see everything below rendered, in both themes.

## Files

```
bakehouse/
├── brand.html              Reference sheet. Start here.
├── tokens/
│   ├── tokens.json         Source of truth. Import into the app or a token pipeline.
│   └── tokens.css          Custom properties for both themes. Link this in the shell.
├── logo/
│   ├── mark.svg            The arch, in ember gradient. Default logo.
│   ├── mark-mono.svg       Single color via currentColor. Menu bar, favicons, stamps.
│   ├── lockup-horizontal.svg
│   └── lockup-stacked.svg
└── icon/
    ├── app-icon.svg        1024pt, Apple's 824/1024 squircle proportion.
    └── build-icons.sh      Renders the .icns and PNG set. Needs librsvg.
```

## The idea

**Cold iron, hot ember.** The interface is dark, cool, neutral grey — the cast iron of an oven,
not the warm clay of a bakery storefront. Heat appears in exactly one place: data that is live.
An ember-colored element means something is running, connected, or about to change on disk.
Everything else stays cool, so state is findable across a window full of panels.

The mark is a hearth arch with three racks. One reading is an oven mouth; the other is the stacked
discs of the standard database glyph. It holds both without committing to either, and it survives
at 16px.

## Color

| Token | Hex | Meaning |
| --- | --- | --- |
| Iron | `#131518` | Deepest surface, window background |
| Surface | `#1A1D21` | Panels, sidebars |
| Border | `#2E333A` | Panel edges, dividers |
| Ember | `#FF5A1F` | Primary accent — live, running, connected |
| Gold | `#FFB020` | Secondary — pulling, restoring, in progress |
| Flour | `#E8E6E1` | Primary text on dark |

Instance lifecycle: running `#3DD68C`, starting and pulling `#FFB020`, stopped `#6C747C`,
failed `#F2385A`, info `#4EA3F5`.

Two constraints worth keeping in mind as you build:

- **Ember fails contrast on the light theme.** `tokens.css` swaps `--bh-accent` to `#D93F0C`
  automatically. Use `var(--bh-accent)` for text and icons, and reserve raw `--bh-ember` for
  large fills and the logo.
- **Failed is deliberately pink-shifted** (`#F2385A`) rather than a warm red, so an error badge
  never gets confused with an ember "running" badge at a glance.

## Type

| Role | Family | Where |
| --- | --- | --- |
| Display | Fraunces (variable, `SOFT 40`, `WONK 1`) | Wordmark, empty states, first run. Nowhere else. |
| Interface | Geist | Everything |
| Mono | JetBrains Mono, ligatures off | SQL editor, grids, logs, connection strings |

Scale is a minor third from a 13px base, which matches native macOS density. Sizes are in
`tokens.json` under `typeScale`.

Fraunces and JetBrains Mono are SIL Open Font License; Geist is MIT. All three can ship inside the
app bundle. Do that rather than loading from a CDN — the app should look right offline, and this
tool is going to be used offline.

## Working rules

- Ember means live. Primary action, running instance, active connection, editor cursor. Nothing else.
- Gold means working, and gold is the only color allowed to animate.
- Radius encodes hierarchy: 5px controls, 6px fields, 10px panels, 12px window, 14px modals.
- Every state pairs a color with a shape — filled dot, pulsing ring, hollow dot, triangle. Color is
  never the only signal.
- Depth comes from borders and surface lightness. Shadow is reserved for things that actually float.
- No bread. Wheat, rolling pins and loaf illustrations turn a database tool into a café. The oven
  metaphor lives in the name and one arch.
- No terracotta. Muting ember toward clay collapses the warm/cool contrast the identity rests on.

## Trademarks

Keep "SQL Server", "Microsoft", "Azure", and "SSMS" out of the product name, the icon, the window
chrome, and the domain. Describe compatibility in prose instead — "a macOS workbench for SQL Server"
is a factual, nominative use and is fine. Don't use Microsoft's logos or approximate them.

Same applies to Docker: you can say the app manages Docker containers, but don't use the whale.

## Before shipping

1. **Outline the wordmark.** `lockup-horizontal.svg` and `lockup-stacked.svg` use a live `<text>`
   element so they're editable now. Convert to paths once the type is final, or the lockup will
   render in Georgia on any machine without Fraunces.
2. **Run `icon/build-icons.sh`** to produce `Bakehouse.icns` plus the PNG set. It also writes the
   filenames Tauri expects in `src-tauri/icons/`.
3. **Check the 16px mark** in the menu bar. If the racks close up on a non-Retina display, ship the
   solid arch with no knockouts at that size.
4. **Test the light theme.** It exists, it is not an afterthought, and macOS users flip themes at
   sunset.

## Using the tokens

```html
<link rel="stylesheet" href="tokens/tokens.css">
```

```css
.instance-row[data-state="running"] .dot {
  background: var(--bh-running);
  box-shadow: 0 0 0 3px rgb(61 214 140 / 0.14);
}

.btn-primary {
  background: var(--bh-ember);
  color: #17110D;                 /* dark text on ember, not white */
  border-radius: var(--bh-radius-control);
  transition: background var(--bh-dur-quick) var(--bh-ease);
}
```

Theme is driven by `data-theme="dark" | "light"` on `<html>`. With no attribute set, it follows
`prefers-color-scheme`. `prefers-reduced-motion` is already respected in `tokens.css`.
