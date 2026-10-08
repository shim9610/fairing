# Fonts the examples carry

| File | Source | Licence |
|---|---|---|
| `NotoSansKR-Regular.ttf` · `NotoSansKR-Bold.ttf` | Noto Sans KR v39 (Google Fonts static TTFs), **subset** | OFL-1.1 — `LICENSE-OFL-NotoSansKR` |

The examples look for a Korean font on the system first (`fairing::fonts::korean_font()`,
`strong_font()`) and fall back to these, **built into the example binaries** with
`include_bytes!`, so an example draws Hangul and `₩` wherever it is run from — the Latin faces
have no won sign, and the kiosk is priced in won.

The subset keeps: basic Latin and Latin-1 (`U+0020–007E`, `U+00A0–00FF`), general punctuation
(`U+2010–2027`, `U+2030–203A`), currency (`U+20A0–20BF`), letterlike, arrows and mathematical
operators (`U+2100–214F`, `U+2190–21FF`, `U+2200–22FF`), geometric shapes (`U+25A0–25FF`), CJK
symbols and punctuation (`U+3000–303F`), Hangul jamo and compatibility jamo (`U+1100–11FF`,
`U+3131–318E`), every Hangul syllable (`U+AC00–D7A3`) and the fullwidth forms (`U+FF01–FF5E`,
`U+FFE6`). No Hanja: 14,329 glyphs, 2.8 MB a weight, against 24,853 and 6.2 MB whole.

Regenerate from the Google Fonts static files with fonttools:

```text
pyftsubset NotoSansKR-Regular.ttf --output-file=NotoSansKR-Regular.ttf \
  --unicodes="U+0020-007E,U+00A0-00FF,U+2010-2027,U+2030-203A,U+20A0-20BF,U+2100-214F,\
U+2190-21FF,U+2200-22FF,U+25A0-25FF,U+3000-303F,U+1100-11FF,U+3131-318E,U+AC00-D7A3,\
U+FF01-FF5E,U+FFE6" --layout-features='*' --name-IDs='*' --notdef-outline
```

The library crate does not carry a font: an integrator ships the face their panel needs
(`FontSource::from_static` / `from_path`) — see `fairing::fonts`.
