---
name: Navette · Ragnarok Festival
description: Live tracker for the free festival shuttle, set as a label-catalogue sleeve on matte black.
colors:
  ink: "#0b0b0a"
  ink-2: "#151513"
  paper: "#f2f1ec"
  muted: "#a3a29b"
  grey: "#6f6e68"
  rule: "rgba(242, 241, 236, 0.2)"
  rule-strong: "rgba(242, 241, 236, 0.55)"
  station-blue: "#2f66d8"
  usine-yellow: "#f2c300"
  shuttle-red: "#e2321f"
  shadow: "rgba(0, 0, 0, 0.6)"
  scrim: "rgba(0, 0, 0, 0.75)"
  veil: "rgba(11, 11, 10, 0.8)"
typography:
  display-numeral:
    fontFamily: "Archivo Variable, system-ui, -apple-system, Segoe UI, Roboto, sans-serif"
    fontSize: "60px"
    fontWeight: 300
    lineHeight: 1
    letterSpacing: "-0.03em"
    fontFeature: "tnum"
    fontVariation: "'wdth' 112"
  headline:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "23px"
    fontWeight: 300
    lineHeight: 1.12
    letterSpacing: "-0.01em"
    fontVariation: "'wdth' 112"
  numeral:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "24px"
    fontWeight: 300
    lineHeight: 1.1
    fontFeature: "tnum"
    fontVariation: "'wdth' 112"
  body:
    fontFamily: "Archivo Variable, system-ui, -apple-system, Segoe UI, Roboto, sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.45
  body-strong:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: 1.45
  label-caps:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 600
    letterSpacing: "0.1em"
    fontVariation: "'wdth' 125"
  small:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.45
  caption-caps:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    letterSpacing: "0.08em"
    fontVariation: "'wdth' 125"
  subhead:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 600
    lineHeight: 1.2
    fontFeature: "tnum"
  numeral-large:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "30px"
    fontWeight: 300
    lineHeight: 1
    fontFeature: "tnum"
    fontVariation: "'wdth' 112"
  input:
    fontFamily: "Archivo Variable, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 400
  wordmark:
    fontFamily: "Norse, Archivo Variable, sans-serif"
    fontSize: "24px"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "0.1em"
rounded:
  none: "0px"
spacing:
  hair: "2px"
  xs: "8px"
  sm: "12px"
  md: "14px"
  lg: "16px"
  xl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
    typography: "{typography.body-strong}"
    rounded: "{rounded.none}"
    padding: "10px 18px"
    height: "48px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.paper}"
    rounded: "{rounded.none}"
    padding: "10px 18px"
    height: "48px"
  map-control:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    rounded: "{rounded.none}"
    padding: "8px 14px"
    height: "44px"
  choice:
    backgroundColor: "transparent"
    textColor: "{colors.paper}"
    rounded: "{rounded.none}"
    padding: "10px 14px"
    height: "48px"
  choice-selected:
    backgroundColor: "{colors.paper}"
    textColor: "{colors.ink}"
  status-square:
    size: "14px"
    rounded: "{rounded.none}"
  seat-cell:
    width: "11px"
    height: "18px"
    rounded: "{rounded.none}"
  stop-marker:
    backgroundColor: "{colors.station-blue}"
    size: "16px"
    rounded: "{rounded.none}"
  shuttle-marker:
    backgroundColor: "{colors.shuttle-red}"
    textColor: "{colors.paper}"
    size: "26px"
    rounded: "{rounded.none}"
  banner-info:
    backgroundColor: "{colors.station-blue}"
    padding: "12px 16px"
  banner-warn:
    backgroundColor: "{colors.usine-yellow}"
    textColor: "{colors.ink}"
    padding: "12px 16px"
  banner-alert:
    backgroundColor: "{colors.shuttle-red}"
    padding: "12px 16px"
---

# Design System: Navette · Ragnarok Festival

## Overview

**Creative North Star: "Pochette Factory"**

The shuttle tracker is set like a back sleeve from a label catalogue: matte black, white hairlines, square corners, engraved wide capitals, a ruled key/value spec sheet, and one plotted line that owns the field. The information is a trace. Where is the shuttle, and when does it arrive? The answer is drawn as a white line with a pulse at the shuttle's real position, plus one big thin numeral. Colour is never decoration. It is an industrial code: blue marks the station stop, yellow marks the Usine stop and the "at the stop" state, and red marks the shuttle.

Density is calm and vertical: one column of ruled blocks under a black map. Type does the structural work. Wide engraved caps label things, thin wide numerals give the time, and plain Archivo carries the sentences. Depth comes from rules (hairlines), never from shadows or light. The confirmed anti-reference, from the owner, is the previous look: a dark background with violet neon glows that "looks like AI". This system has no glow, no gradient, no rounded card and no tint.

The only identity asset the brand imposes is the "RAGNAROK" wordmark set in Norse. Norse appears in the wordmark and nowhere else.

**Key Characteristics:**
- Matte black field (`ink`) with paper-white type and translucent white hairlines.
- Three industrial colour codes with fixed meanings: station blue, Usine yellow, shuttle red.
- Square corners everywhere (0px). The only round shapes stand for "you".
- Archivo Variable's width axis does the voicing: 125% for engraved caps, 112% for thin numerals and headings.
- Signature trace: one line per leg, with a gaussian pulse and a red square at the shuttle's position.
- Ruled key/value spec table, like a sleeve's technical credits.

## Colors

A near-monochrome sleeve (warm black and off-white) punctuated by three saturated industrial codes, each with a single fixed meaning.

### Primary
- **Shuttle Red** (`shuttle-red`): the shuttle itself and its live state. Used for the live connection square, the "en route" status square, the map marker body, the pulse's position square on the trace, the solid end block of an active "Suivre la navette" control, every seat cell when the shuttle is full, alert banners, and long-distance train categories (IC, IR, EC, ICE, TGV, RJX) as on CFF departure boards.

### Secondary
- **Station Blue** (`station-blue`): the "Gare CFF de Sion" stop. Used for its map square, its end blocks on the trace, its ETA row square, and the informational (default) banner.

### Tertiary
- **Usine Yellow** (`usine-yellow`): the "Usine de Chandoline" stop, plus the states "at the stop" and "warning". Used for its map square and trace end blocks, the filled status square when the shuttle is at a stop (dashed outline for a warning), the ETA "À quai" figure, train delays and platform changes, the scam warning text, the pickup card outline and diamond, form errors, warn banners, and the global focus ring.

### Neutral
- **Ink** (`ink`): the page, the sheet, the map background, the dialogs, the map controls, and the dark casing under the route line.
- **Ink Raised** (`ink-2`): a second black, declared for a raised surface. The public page barely uses it so far.
- **Paper** (`paper`): all primary text, the primary button fill, taken seat cells, the active leg line, the route line on the map, and the outline of stop markers.
- **Muted** (`muted`): section headings, spec keys, inactive leg names, timestamps, secondary notes.
- **Grey** (`grey`): "off" or "stale". Used for the empty status square outline, the dimmed trace and shuttle marker when the signal is stale, and unknown ETAs.
- **Rule** (`rule`, 20% paper): the hairlines between blocks, rows and spec lines.
- **Rule Strong** (`rule-strong`, 55% paper): control outlines, empty seat cells, inactive leg lines, the sheet's edge, and the drag handle.

### Named Rules
**The Colour Is a Code Rule.** Each colour has exactly one meaning: blue is the station, yellow is the Usine, "at the stop" or "warning", and red is the shuttle. A colour never appears just because a surface "needs some colour". If an element is neither a stop, a state nor the shuttle, it stays ink and paper.

**The Truthful State Rule.** Lost signal, offline and paused states fall back to grey and never take red. Red means the shuttle is live. The trace and the marker dim to grey whenever the position is stale.

**The No Glow Rule.** No colour is ever used as light: no glows, coloured shadows, gradients or tinted overlays. Colour appears only as flat, solid fills or 1px outlines.

## Typography

**Display Font:** Norse Bold (self-hosted), for the "RAGNAROK" wordmark only
**Body Font:** Archivo Variable (with system-ui, -apple-system, Segoe UI, Roboto, sans-serif)

**Character:** One variable family voiced through its width axis. Wide, tracked, heavy uppercase gives the engraved catalogue caps. Slightly wide, very light weights give the big figures and headings. Regular Archivo carries the plain sentences. Norse is the festival's own mark and stays confined to the logo.

### Hierarchy
- **Display Numeral** (300, 60px, 1.0, width 112%, tabular, -0.03em): the ETA to the next stop, beside the status heading. Its unit ("MIN") is set in engraved caps at 13px.
- **Headline** (300, 23px, 1.12, width 112%, balanced wrap): the status sentence in the h1 ("En route vers Usine de Chandoline"). The dialog title uses the same voice at 21px.
- **Numeral** (300, 24px, 1.1, width 112%, tabular): ETA figures in the stop list. The stepper count uses it at 30px.
- **Body** (400, 15px, 1.45): sentences and notes, capped at about 46ch in the status block. **Body Strong** (600) is used for stop names, spec values, seat counts and train destinations.
- **Small** (400, 13px): timestamps, ETA details, dialog notes, train meta, in muted colour.
- **Label / Engraved Caps** (600, 12px, width 125%, uppercase, 0.1 to 0.14em tracking): section headings, spec keys, leg names, connection state, the brand subline, and stop labels on the map (11px). This is the voice of the system.
- **Wordmark** (Norse 700, 24px, uppercase, 0.1em): the header logo.
- **Caption Caps** (600, 11px, width 125%, 0.08em): units under ETA numerals, train line tags, map stop labels, and the header subline on phones under 400px.
- **Subhead** (600, 17px, tabular): train departure times, "À quai" in the stop list, and the title of the help-alert card.
- **Large Numeral** (300, 30px, width 112%, tabular): the stepper count and the 117 / 144 emergency numbers in the help dialog.
- **Input** (400, 16px): text fields, kept at 16px so phones do not zoom on focus.
- Text on solid blue or red fields (banner, alert button, shuttle marker, long-distance train tag) is `paper`, never pure white.

### Named Rules
**The Engraved Caps Rule.** All labelling (section headings, spec keys, leg names, map stop labels, states) uses one style: Archivo at width 125%, weight 600, uppercase and tracked. Never set a label in plain small caps or in Norse.

**The Thin Figure Rule.** Times and counts are set in Archivo at width 112%, weight 300, with tabular figures. The time is the hero, so it gets size and lightness, never weight or colour (yellow "À quai" is the single state exception).

**The Norse Is the Logo Rule.** Norse sets only the "RAGNAROK" wordmark. It never sets headings, numbers or UI copy.

## Layout

Mobile first, map over sheet. The 56px header sits on a hairline. Below it, the Leaflet map fills the viewport, and a bottom sheet overlays it (up to 52% of the height, or 190px when collapsed). A 20px hit area with a 36x3px handle bar toggles the sheet. From **860px** up, the layout becomes a two-column grid: a **400px** sheet column on the left, with a strong hairline on its right edge, and the map filling the rest.

The sheet is a single column of **ruled blocks**. Each block starts 24px below the previous one, with a 1px `rule` line on top and 14px of padding under it. Rows inside blocks (stops, trains, spec lines) are divided by the same hairline, and the first row has none. Horizontal padding is 16px on mobile and 24px on desktop.

The first viewport, sheet collapsed, always shows the status square with the heading and the ETA, then the active leg of the trace. The active leg is reordered first so the shuttle stays visible when the sheet is collapsed. The seat strip follows.

Map controls float 12px from the map's edges. "Me localiser" and the Leaflet attribution track the live sheet height (`--sheet-h`) so they always sit just above the sheet on mobile. Zoom controls are hidden below 860px.

Spacing rhythm: 8, 12, 14, 16, 24px, with 2px gaps inside the seat strip. Touch targets are 48px (44px for map controls).

## Elevation & Depth

The system is flat. Depth comes from hairlines and from the order of the map layers, not from shadows. Surfaces are all `ink`. They are separated by 1px `rule` or `rule-strong` lines, and an active or selected surface turns solid `paper`.

There is one shadow, and it does a job: on mobile, the bottom sheet floating over the map carries a dark downward-fading shadow so the map's edge reads clearly beneath it. It disappears on desktop, where the sheet becomes a column.

### Shadow Vocabulary
- **Sheet over map** (`box-shadow: 0 -12px 32px rgba(0, 0, 0, 0.6)`): mobile bottom sheet only. Black, never coloured.

### Named Rules
**The Hairline Depth Rule.** Separate surfaces with a 1px rule, never with a shadow or a lighter fill. A shadow is allowed only where an overlay physically covers the map, and it is always black.

## Shapes

Square corners everywhere (0px): buttons, dialogs, cards, map controls, Leaflet's zoom bar and tooltips (forced to 0), stop markers, the shuttle marker, status squares, seat cells and trace end blocks. Squares are the system's unit of meaning. A stop is a coloured square, the shuttle is a red square, a state is a 14px square, and a seat is an 11x18px cell.

Outline versus fill carries state. An outline means empty, off or pending, and a solid fill means taken, live or reached. A dashed outline means warning.

The two exceptions are deliberate. Round shapes mean "you": your location on the map (a paper disc ringed in ink and paper) and the 8px "Tu es à cet arrêt" dot in the stop list. The yellow pickup marker ("Tu attends ici") is a square rotated 45 degrees into a diamond.

**The Only You Are Round Rule.** Circles are reserved for the user's own position. Everything belonging to the shuttle or the route is square.

## Components

### Buttons
Flat, full-width slabs that read as printed blocks rather than pills.
- **Shape:** square corners (0px), at least 48px tall, full width of the sheet.
- **Primary:** a solid `paper` fill with `ink` text, weight 700 at width 112%, and 10px 18px padding ("Envoyer").
- **Ghost:** transparent, with a 1px `rule-strong` outline and paper text. On hover, the outline turns paper ("Faire signe à la navette", "Annuler").
- **Focus:** a global 2px `usine-yellow` outline, offset 2px.
- **Disabled:** 40% opacity.

### Map Controls
Ink slabs with a hairline outline that float on the map: "Suivre la navette", "Trajet", "Me localiser".
- **Style:** `ink` fill, 1px `rule-strong` outline, 13px weight 600, a 16px stroked square-cap icon, and 44px height.
- **Active state:** the outline turns paper, and a solid 8px block fills the right end of the button, edge to edge. The block is red when you follow the shuttle and paper when your own location is on. This is the sleeve's colour-code tab, used as a toggle indicator.
- **Pending:** while locating, the icon blinks (opacity 1 to 0.35 over 1s).

### Choice and Stepper (pickup dialog)
- **Choice:** a 48px, left-aligned, outlined row. Once selected, it fills solid paper with ink text.
- **Stepper:** 48px square outlined buttons with stroked plus and minus icons around a thin 30px tabular count.

### Dialog
- An ink panel with a 1px paper outline and square corners, at most 420px wide, with 20px padding and a 75% black backdrop. The title uses the headline voice. Notes are small and muted, and errors are yellow.

### Status Head
- A 14px square, then the headline, then the display ETA numeral, on a three-column grid. The square is grey outlined when off, red filled when live, yellow filled at a stop, and dashed yellow on a warning. No label sits above the heading: the square is the state marker.

### Seat Strip
- One cell per seat (16), each 11x18px with 2px gaps. Empty cells are outlined in `rule-strong`, taken cells are solid paper, and when the shuttle is full every cell turns solid red. It sits next to a weight-600 count ("12 places libres").

### Spec Table
The sleeve's technical credits.
- A two-column grid (124px key, then the value), with hairlines between rows. Keys use engraved caps in muted colour at 12px, and values are body strong ("LIEU · Usine de Chandoline, Sion", "TARIF · Gratuit", "CHAUFFEUR · Bénévole").

### Lists (stops, trains)
- Hairline-divided rows. A stop row has a 12px square in the stop's code colour (outlined, or filled when it is the next or current stop), the name with a muted detail line, and a right-aligned thin ETA numeral with an engraved-caps unit. A train row has a tabular time (yellow delay below it), the destination, and a meta line with an outlined category tag that fills red for long-distance lines.

### Banner
- A full-width strip under the header with weight-600 text and 12px 16px padding: blue for information, yellow with ink text for a warning, red for an alert.

### Map Treatment
- **Tiles:** swisstopo grey national map, filtered with `grayscale(1) invert(1) brightness(0.3) contrast(0.95)` into matte black with no tint.
- **Route:** a 3.5px paper line over a 9px ink casing at 90% opacity, with square caps and mitred joins.
- **Stops:** 16px squares in the stop's code colour, with a 2px ink border and a 1px paper outline. Their labels are engraved-caps tooltips (ink, hairline, square, no arrow) that sit in a pane below the markers, so the shuttle always passes in front. They are offset by the shuttle marker's width so a docked shuttle never covers its own stop name.
- **Shuttle:** a red square with a 2px paper border and a stroked bus icon, plus a paper heading arrow that rotates over 0.6s. It turns grey when the signal is stale.

### The Trace (signature)
The shuttle as a pulse on a plotted line.
- One row per leg between stops. An engraved-caps leg name sits above a 46px plot framed by two 10px end blocks in the stops' code colours.
- Inactive legs are flat `rule-strong` lines. The active leg is drawn in paper, is moved to the top of the list, and carries a gaussian bump at the shuttle's real fraction of the leg, with a red 8px square at the peak. Near a stop, the bump spills over the end block instead of being clipped.
- When the shuttle is at a stop, the bump "breathes" (it scales vertically to 55% and back over 1.8s, ease-in-out). When the signal is stale, the line and the square dim to grey.
- With reduced motion, the breathing, the blinking and the marker or sheet transitions are switched off.

### Help (« Aide »)
For someone in danger near the route: the emergency numbers come first, the crew alert second.
- **Map button:** a map control pinned top-right, with a 1px red outline and a red stroked warning triangle ("Aide"). It works even before the server answers.
- **Dialog:** the headline "Besoin d'aide ?", then two outlined call tiles side by side (`117` Police, `144` Ambulance) in the large numeral with caption-caps labels, then a muted note saying the exact position goes to the crew only, an optional 16px text field, and the actions "Annuler" (ghost) and "Prévenir l'équipe" (danger button).
- **Danger button:** solid `shuttle-red` with paper text, square, 48px. It is used only to send or call for help ("Prévenir l'équipe", "Appeler le 117").
- **Alert card:** at the top of the sheet, a 1px red frame with a 6px red top edge, a subhead title and body text that follow the alert's state (sent, seen by the crew, closed). It holds "Appeler le 117" and "Annuler l'alerte". Once closed, the frame turns `rule-strong`.
- The position is never shown on the public map or in the public stream.

## Do's and Don'ts

### Do:
- **Do** keep every surface on `ink`, with paper text, and separate surfaces with 1px `rule` hairlines.
- **Do** give each new stop its own industrial code colour and use it the same way on the map square, the trace end blocks and the ETA row.
- **Do** set every label in engraved caps (Archivo width 125%, weight 600, uppercase, 0.1 to 0.14em tracking).
- **Do** set times and counts as thin wide numerals (width 112%, weight 300, tabular figures).
- **Do** show an active toggle as a solid colour block at the right end of the control, and a selected option as a solid paper fill.
- **Do** express state with outline versus fill: outlined means empty or off, filled means taken, live or reached, and dashed means warning.
- **Do** use the ruled key/value spec table for factual product details (place, price, driver, seats, departure, service hours).
- **Do** keep touch targets at least 48px (44px for floating map controls) and the focus ring 2px yellow.

### Don't:
- **Don't** use rounded corners, pills or rounded cards. The radius is 0, and circles are only for the user's own position.
- **Don't** use glows, neon, gradients, coloured shadows or violet. This is the "looks like AI" look the owner rejected.
- **Don't** use red for anything but the shuttle, its live state, a full shuttle, alerts and long-distance trains. In particular, a third stop must not inherit red.
- **Don't** show a stale, offline or paused shuttle in red or as a live pulse. It goes grey.
- **Don't** put a small caps label above a heading. The status square is the state marker.
- **Don't** set anything but the "RAGNAROK" wordmark in Norse.
- **Don't** add photos, posters or line-up imagery. None were supplied, and none may be invented.

## Open Items

- `ink-2` is declared but no surface uses it. Drop it or give it a role.

## Crew Surfaces

`static/driver.html` and `static/admin.html` share `static/crew.css`, built on the same tokens, type and rules as the public page: the Norse wordmark plus an engraved-caps role ("CHAUFFEUR", "ORGA") in a 56px header, ruled blocks, and the spec table for live figures (thin numerals, yellow for warnings, red for errors).
- **GPS state card:** a hairline frame with a 14px status square: paper when positions arrive, dashed yellow while searching, red (frame and square) on an error.
- **Seat counter:** a 60px thin numeral between two 72px square steppers, the 16-cell seat strip under it, then "Vide / Plein".
- **"On part":** a 64px outlined button that fills `usine-yellow` while the shuttle waits at a stop, the stop-state colour.
- **Help alerts:** like the public alert card (1px red frame, 6px red top edge), turning yellow once someone answers "J'y vais". On the map, an alert is a red square with a paper border whose outline beats until answered.
- **Crew map and route editor:** the public map treatment at 240px (340px for the editor). Pickups are yellow diamonds, the shuttle a red square, stops coloured squares. In the route editor, the waypoints are paper squares numbered in order, the published route is paper and the preview is a dashed yellow line.
- **Destructive actions** ("Clore", "Hors service", "Appliquer", "Trajet d'origine", "Changer de jeton") confirm in place: the first tap turns the button red with "Confirmer…", the second within 4 s acts. No browser dialogs.
