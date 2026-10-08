# Kiosk showcase asset spec

The spec and the generation prompts for the **33 images** that lift `examples/kiosk.rs` into a
**product-grade showcase**. It is written so that everything can be made from this one document.

**Provenance and licence.** The images in this folder were made for this project from these
prompts with a generative image model and contain no third-party material. They are distributed
under the MIT licence, the same as the repository's code (`LICENSE`).

| Group | Count | Nature |
|---|---|---|
| Backgrounds and brand | 4 | 2 photos · 2 transparent logos |
| Product photos | 16 | All square transparent PNGs |
| Dine-in / takeout | 2 | Square transparent PNGs |
| Payment and receipt icons | 7 | Single-colour white, transparent |
| State graphics | 4 | 1 photo · 2 icons · 1 background |

> **Why they are needed.** Without them the example sits line-drawn shapes where the product
> photos go. It is a coffee kiosk with no coffee in it. A library showcase exists to show how far
> the crate goes, so a screen that could not be built for lack of assets makes the crate look
> worse than it is.

---

## 0. Shared art direction

**For the 33 to read as one brand, everything below has to match.** It is repeated in each
prompt, but if the model drifts, prefix this section and run it again.

| Axis | Value |
|---|---|
| Scene | On a dark matte counter (around deep charcoal `#14181F`), with a shop interior falling out of focus behind |
| Light | One softbox at upper left 45° plus a weak fill from the right. Shadows soft and not long |
| Colour temperature | **Warm amber and caramel highlights, cool blue-grey shadows.** The UI accent is `#4C8DFF` (a cool blue), so the photos have to be warm for the contrast to work |
| Lens | 50 mm equivalent, f/2.8 shallow depth of field. The product sharp, only the background soft |
| Angle | Drinks from a 3/4 high angle, desserts from the front slightly high, bean bags straight on |
| Finish | Photographic studio product work. No illustration or 3D-render feel |
| Never | **No text, logos, watermarks, people or hands.** Nothing cropped by the frame. No strong colour backgrounds |

Always append to the prompt:

```
photorealistic studio product photography, single soft key light from upper left 45 degrees,
warm amber highlights with cool blue-grey shadows, very dark charcoal background,
50mm lens shallow depth of field, no text, no logo, no watermark, no hands, no people,
centered, full subject visible with margin
```

---

## 1. Backgrounds and brand (4)

The attract screen becomes **a full-bleed photo with a gradient scrim at the bottom**. Today it
is flat black, so half the screen does nothing.

| # | File | Size | Background | Format | Where it is used |
|---|---|---|---|---|---|
| 1 | `bg-attract-portrait.webp` | 1080×1920 | Opaque | WebP q92 | The portrait kiosk's attract screen, full bleed |
| 2 | `bg-attract-landscape.webp` | 1920×1080 | Opaque | WebP q92 | The counter POS and the landscape attract screen |
| 3 | `logo-wordmark.png` | 1600×400 | **Transparent** | PNG | The wordmark on the attract screen and at the top of the receipt |
| 4 | `logo-mark.png` | 1024×1024 | **Transparent** | PNG | The customer-facing display, and as an app icon |

**1 · `bg-attract-portrait.webp` — portrait attract background**
> The **top 45 % has to be empty** for the wordmark and the guidance line to sit in. A counter
> and beans blurred across the lower part.
```
Moody dark coffee shop interior shot vertically, empty dark space in the upper half,
a blurred espresso machine and scattered coffee beans on a matte dark counter in the
lower third, warm amber rim light from the left, deep charcoal tones, cinematic,
photorealistic, no text, no logo, no people, vertical 9:16 composition
```

**2 · `bg-attract-landscape.webp` — landscape attract background**
```
Moody dark coffee shop counter shot horizontally, empty dark space on the left half,
a blurred espresso machine and coffee beans on the right, warm amber rim light,
deep charcoal tones, cinematic, photorealistic, no text, no logo, no people,
horizontal 16:9 composition
```

**3 · `logo-wordmark.png` — the wordmark**
> The only image with real lettering in it. The shop is called `페어링 커피` (Fairing Coffee);
> change it and the example's strings change with it. Only the Korean screen (`--lang=ko`)
> shows it: the English screen sets "Fairing Coffee" as text in the display face, in the same
> cream.
```
Minimal wordmark logo for a specialty coffee shop, the Korean text "페어링 커피" in a clean
modern geometric sans-serif, warm cream color #F2E8DC, horizontal lockup, transparent
background, flat vector style, no icon, no frame, generous side margin
```

**4 · `logo-mark.png` — the mark**
```
Minimal abstract logo mark for a specialty coffee shop, a single continuous line forming a
coffee cup silhouette, warm cream color #F2E8DC, geometric and confident, transparent
background, flat vector style, square composition, no text
```

---

## 2. Product photos (16)

**The photo fills the top 62 % of a tile and the name and price take the bottom 38 %.** So they
all have to be **square transparent PNGs** — an opaque rectangle gets its corners clipped by the
tile and stops reading as a card.

| Shared | Value |
|---|---|
| Size | 1024×1024 |
| Background | **Transparent** (alpha) |
| Format | PNG |
| Margin | 8 % clear on every side. Cups and plates must not touch the frame |
| Shadow | **A soft contact shadow only**, under the product, inside the alpha. No background plate |
| File name | `menu/<id>.png`, with the ids in the tables below |

The prompt prefix (the same for all of them):
```
Product photo of {subject}, isolated on transparent background with a soft contact shadow
beneath, photorealistic studio product photography, single soft key light from upper left
45 degrees, warm amber highlights with cool blue-grey shadows, 50mm lens, sharp focus,
no text, no logo, no watermark, no hands, centered with margin, square 1:1
```

### Coffee (6)

| id | Item | What goes in `{subject}` |
|---|---|---|
| `americano` | Americano | `a tall glass of iced americano with clear ice cubes and a thin crema layer, viewed from a 3/4 high angle` |
| `latte` | Caffè Latte | `a ceramic cup of hot cafe latte with delicate rosetta latte art, on a matching saucer, 3/4 high angle` |
| `cold-brew` | Cold Brew | `a tall glass of cold brew coffee, deep mahogany color, condensation on the glass, single large ice sphere, 3/4 high angle` |
| `espresso` | Espresso | `a small white demitasse of espresso with thick golden crema, on a saucer with a tiny spoon, 3/4 high angle` |
| `cappuccino` | Cappuccino | `a cappuccino in a wide ceramic cup with thick milk foam and a light cocoa dusting, 3/4 high angle` |
| `vanilla` | Vanilla Latte | `an iced vanilla latte in a tall glass showing distinct layers of milk and espresso, vanilla bean pod resting beside, 3/4 high angle` |

### Drinks (4)

| id | Item | `{subject}` |
|---|---|---|
| `earl-grey` | Earl Grey | `a clear glass teapot and cup of earl grey tea, amber liquid, a dried bergamot slice beside, 3/4 high angle` |
| `peach-tea` | Peach Iced Tea | `a tall glass of iced peach tea, golden orange, ice cubes and a fresh peach wedge on the rim, 3/4 high angle` |
| `lemonade` | Lemonade | `a tall glass of cloudy lemonade with ice and a lemon wheel, fresh mint sprig, 3/4 high angle` |
| `choco` | Hot Chocolate | `a mug of hot chocolate topped with whipped cream and cocoa powder, a few marshmallows, 3/4 high angle` |

### Desserts (4)

| id | Item | `{subject}` |
|---|---|---|
| `cheesecake` | Cheesecake | `a single slice of New York cheesecake on a small dark plate, smooth creamy top, graham crust, front slightly high angle` |
| `brownie` | Brownie | `a thick fudgy chocolate brownie square on a dark plate, glossy crackled top, front slightly high angle` |
| `scone` | Plain Scone | `a golden plain scone on a small dark plate, crumbly texture, split top, front slightly high angle` |
| `cookie` | Cookie | `two chocolate chip cookies stacked on a dark plate, melted chocolate chunks visible, front slightly high angle` |

### Beans (2)

| id | Item | `{subject}` |
|---|---|---|
| `ethiopia` | Ethiopia 200 g | `a matte kraft coffee bean bag standing upright with a one-way valve, blank front with no label, warm tan paper, a few whole coffee beans at the base, straight-on front view` |
| `colombia` | Colombia 200 g | `a matte dark brown coffee bean bag standing upright with a one-way valve, blank front with no label, a few whole coffee beans at the base, straight-on front view` |

> The two bean bags **must have no label.** The UI draws the name.

---

## 3. Dine-in / takeout (2)

The two large tiles on the first fork in the flow. They have to be **photographs**, not icons,
for a customer to choose in half a second.

| # | File | Size | Background | Format |
|---|---|---|---|---|
| 17 | `mode-dinein.png` | 1024×1024 | Transparent | PNG |
| 18 | `mode-takeout.png` | 1024×1024 | Transparent | PNG |

```
17: a ceramic coffee mug on a saucer with a small dessert fork beside it, isolated on
    transparent background with soft contact shadow, [shared suffix]

18: a brown kraft paper takeout bag with a disposable coffee cup with lid beside it,
    isolated on transparent background with soft contact shadow, [shared suffix]
```

---

## 4. Payment and receipt icons (7)

These are **icons, not photos** — the only group that is. The crate's 78 built-ins have no
payment domain, so they live with the example (`assets/kiosk/icons/`).

| Shared | Value |
|---|---|
| Size | 512×512 |
| Background | **Transparent** |
| Format | PNG (SVG is better still — it scales without limit) |
| Colour | **Solid white `#FFFFFF`.** The UI tints them with a role colour, and more than one colour breaks the tint |
| Style | Outline at a uniform stroke weight, rounded caps, 2 px on a 24 grid (about 42 px at 512) |
| Margin | 10 % clear on every side |

| # | File | Prompt |
|---|---|---|
| 19 | `icons/pay-card.png` | `minimal outline icon of a credit card with a magnetic stripe, uniform stroke weight, rounded caps, pure white on transparent background, flat vector, centered, square` |
| 20 | `icons/pay-easy.png` | `minimal outline icon of a QR code with a phone, uniform stroke weight, rounded caps, pure white on transparent, flat vector, centered, square` |
| 21 | `icons/pay-cash.png` | `minimal outline icon of a banknote with a coin, uniform stroke weight, rounded caps, pure white on transparent, flat vector, centered, square` |
| 22 | `icons/receipt.png` | `minimal outline icon of a paper receipt with a zigzag torn bottom edge and two text lines, uniform stroke weight, pure white on transparent, flat vector, centered, square` |
| 23 | `icons/sms.png` | `minimal outline icon of a mobile phone with a speech bubble, uniform stroke weight, rounded caps, pure white on transparent, flat vector, centered, square` |
| 24 | `icons/cup.png` | `minimal outline icon of a takeaway coffee cup with a lid and a sleeve, uniform stroke weight, rounded caps, pure white on transparent, flat vector, centered, square` |
| 25 | `icons/bag.png` | `minimal outline icon of a paper shopping bag with handles, uniform stroke weight, rounded caps, pure white on transparent, flat vector, centered, square` |

---

## 5. State graphics (4)

| # | File | Size | Background | Where it is used |
|---|---|---|---|---|
| 26 | `state-insert-card.png` | 1024×1024 | Transparent | Mid-payment — the prompt to insert a card in the terminal |
| 27 | `state-done.png` | 1024×1024 | Transparent | The payment-complete check |
| 28 | `state-sold-out.png` | 512×512 | Transparent | The overlay badge on a sold-out tile |
| 29 | `bg-ticket.webp` | 1080×760 | Opaque | A quiet background behind the order number on the receipt screen |

```
26: a card reader terminal with a credit card being inserted into the slot, isolated on
    transparent background with soft contact shadow, photorealistic studio product
    photography, warm amber key light, no text, no logo, no hands, centered, square

27: minimal outline icon of a checkmark inside a circle, uniform stroke weight, rounded
    caps, pure white on transparent, flat vector, centered, square

28: minimal outline icon of a circle with a diagonal slash (sold out mark), uniform stroke
    weight, pure white on transparent, flat vector, centered, square

29: abstract dark texture of coffee bean bokeh, very low contrast, deep charcoal with faint
    warm amber points of light, no subject in focus, meant to sit behind large white text,
    horizontal composition
```

---

## 6. How to deliver

```
assets/kiosk/
├── SPEC.md                     ← this document
├── bg-attract-portrait.webp
├── bg-attract-landscape.webp
├── bg-ticket.webp
├── logo-wordmark.png
├── logo-mark.png
├── mode-dinein.png
├── mode-takeout.png
├── state-insert-card.png
├── state-done.png
├── state-sold-out.png
├── menu/
│   ├── americano.png … colombia.png     (16 files)
└── icons/
    ├── pay-card.png … bag.png           (7 files)
```

**Checklist**

- [ ] Nothing that should be transparent has a white plate under it (check the alpha channel)
- [ ] No text, logos or watermarks (the wordmark is the one exception)
- [ ] The 7 icons are **solid white** (grey or a gradient breaks the tint)
- [ ] The 16 product photos share a light direction and colour temperature — **get this wrong and
      they do not read as one brand**
- [ ] Nothing touches the frame

Partial deliveries are fine; whatever arrives gets wired in, and the current shapes stay as the
fallback for the rest.

---

## 7. What changes on screen once the assets land

It is not only a swap of assets — **the layout gets rebuilt too.** The screens do not look empty
purely for want of photographs.

| Screen | Today | With the assets |
|---|---|---|
| Attract | Black background, a shape of a cup, half the screen empty | **A full-bleed photo with a gradient scrim at the bottom**, the wordmark, three real recommendation photos, and the CTA over the scrim |
| Menu tiles | A shape of a cup, a name, a price | **The photo across the top 62 %** of the tile, name and price in the bottom 38 %. Sold out desaturates the photo and lays `state-sold-out` over it |
| Dine-in / takeout | A shape of a cup versus a shape of a bag | Two real photographs |
| Payment methods | A card, a QR and a banknote drawn in code | A refined icon set |
| Mid-payment | Just the progress ring | The `state-insert-card` photo plus the progress ring |
| Receipt | A big number on black | A big number over `bg-ticket`, with the completion check |
| Customer display | Three lines of text | `logo-mark` plus the large amount |

**The typography gets work too.** What is there now is "centred labels", not a hierarchy — there
is no weight contrast between the item name, the price and the supporting line. Adding a bold
face means registering two faces in the `FontSet` and having `layout` use them.
