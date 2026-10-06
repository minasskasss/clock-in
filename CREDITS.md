# Credits

Clock In includes the following third-party material.

## EFF large wordlist

- File: `assets/eff_large_wordlist.txt` (unmodified; SHA-256 recorded in `docs/DECISIONS.md`).
- Source: Electronic Frontier Foundation, <https://www.eff.org/files/2016/07/18/eff_large_wordlist.txt>.
- Licence: [Creative Commons Attribution 4.0 International (CC BY 4.0)](https://creativecommons.org/licenses/by/4.0/), per EFF's copyright policy (<https://www.eff.org/copyright>).
- Used to check and generate the admin passphrase.

## KaTeX Main Italic (glyph outlines)

- Used in: `src/components/wordmark.ts` (the "Clock In" wordmark). Only the outlines of the letters are included; no font file is bundled or redistributed.
- Font: KaTeX Main Italic. Copyright (c) 2009-2010 Design Science, Inc.; Copyright (c) 2014-2018 Khan Academy.
- Licence: [SIL Open Font License, Version 1.1](https://openfontlicense.org).

## Everything else

The app icon, sounds and the rest of the code and artwork in this repository were made for Clock In. Libraries pulled in by Cargo and pnpm keep their own licences.
