# The offline dictionary

`oewn.dict` is the dictionary built into mouce (see `src/dictionary/data.rs`). It is made
from **Open English WordNet, 2025 edition**,
<https://github.com/globalwordnet/english-wordnet>, © the Open English WordNet team, used
under the [Creative Commons Attribution 4.0 licence](https://creativecommons.org/licenses/by/4.0/).

## What was changed

`build_dictionary.py` trims the original to what a word card needs:

- Single words only; phrases ("by and large") are left out.
- For each word and part of speech: its spelling, one pronunciation (the US one when there
  are several), its first six meanings in WordNet's order, and the first example of each.
- Irregular forms point back to their word ("geese" to "goose").
- Relations between words (synonyms, opposites, and so on) are left out.

The result is stored in compressed blocks; the script's header describes the layout.

## Rebuilding it

1. Download `english-wordnet-2025.xml.gz` from the project's
   [Releases page](https://github.com/globalwordnet/english-wordnet/releases) and unzip it.
2. From this folder, run `python build_dictionary.py english-wordnet-2025.xml oewn.dict`.

The script uses only the Python standard library, and the same input always gives the same
file.
